use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::thread;

use anyhow::{Context, Result, anyhow};
use reqwest::Client;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::de::DeserializeOwned;
use tokio::runtime::Builder;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::config::AppConfig;
use crate::gitlab::{IssueDraft, IssueUpdate};
use crate::model::{Issue, IssueLink, Note, ProjectLabel};

const STARTUP_PROJECT_CONCURRENCY: usize = 4;
const ISSUE_DETAIL_CONCURRENCY: usize = 8;

#[derive(Debug)]
pub struct RefreshPayload {
    pub issues: Vec<Issue>,
    pub labels: Vec<String>,
    pub notes_cache: HashMap<u64, Vec<Note>>,
    pub issue_links_cache: HashMap<u64, Vec<IssueLink>>,
}

#[derive(Debug, Clone)]
pub struct ProjectLoadRequest {
    pub config: AppConfig,
    pub generation: u64,
}

#[derive(Debug)]
pub enum BackgroundEvent {
    ProjectLoadProgress {
        project_url: String,
        generation: u64,
        loaded: usize,
        total: usize,
    },
    ProjectLoaded {
        project_url: String,
        generation: u64,
        payload: RefreshPayload,
    },
    ProjectLoadFailed {
        project_url: String,
        generation: u64,
        error: String,
    },
    NotesLoaded {
        project_url: String,
        issue_iid: u64,
        notes: Vec<Note>,
    },
    NotesLoadFailed {
        project_url: String,
        issue_iid: u64,
        error: String,
    },
    IssueLinksLoaded {
        project_url: String,
        issue_iid: u64,
        links: Vec<IssueLink>,
    },
    IssueLinksLoadFailed {
        project_url: String,
        issue_iid: u64,
        error: String,
    },
}

#[derive(Debug, Clone)]
pub struct AsyncGitLabClient {
    http: Client,
    project_api_base: String,
    project_ref: String,
}

pub fn spawn_startup_preload(requests: Vec<ProjectLoadRequest>, sender: Sender<BackgroundEvent>) {
    if requests.is_empty() {
        return;
    }

    let failures = requests
        .iter()
        .map(|request| (request.config.project_url.clone(), request.generation))
        .collect::<Vec<_>>();

    thread::spawn(move || {
        let failure_sender = sender.clone();
        let result = run_multi_thread(async move {
            let permits = Arc::new(Semaphore::new(STARTUP_PROJECT_CONCURRENCY));
            let mut tasks = JoinSet::new();

            for request in requests {
                let permits = Arc::clone(&permits);
                let sender = sender.clone();
                tasks.spawn(async move {
                    let _permit = permits.acquire_owned().await;
                    load_project(request, sender).await;
                });
            }

            while tasks.join_next().await.is_some() {}

            Ok(())
        });

        if let Err(error) = result {
            let error = format!("{error:#}");
            for (project_url, generation) in failures {
                let _ = failure_sender.send(BackgroundEvent::ProjectLoadFailed {
                    project_url: project_url.clone(),
                    generation,
                    error: error.clone(),
                });
            }
        }
    });
}

pub fn spawn_project_load(request: ProjectLoadRequest, sender: Sender<BackgroundEvent>) {
    let project_url = request.config.project_url.clone();
    let generation = request.generation;

    thread::spawn(move || {
        let failure_sender = sender.clone();
        let result = run_multi_thread(async move {
            load_project(request, sender).await;
            Ok(())
        });

        if let Err(error) = result {
            let _ = failure_sender.send(BackgroundEvent::ProjectLoadFailed {
                project_url,
                generation,
                error: format!("{error:#}"),
            });
        }
    });
}

pub fn spawn_notes_load(config: AppConfig, issue_iid: u64, sender: Sender<BackgroundEvent>) {
    let project_url = config.project_url.clone();

    thread::spawn(move || {
        let result = run_current_thread(async move {
            let client = AsyncGitLabClient::new(&config)?;
            client.list_notes(issue_iid).await
        });

        let event = match result {
            Ok(notes) => BackgroundEvent::NotesLoaded {
                project_url,
                issue_iid,
                notes,
            },
            Err(error) => BackgroundEvent::NotesLoadFailed {
                project_url,
                issue_iid,
                error: format!("{error:#}"),
            },
        };
        let _ = sender.send(event);
    });
}

pub fn spawn_issue_links_load(config: AppConfig, issue_iid: u64, sender: Sender<BackgroundEvent>) {
    let project_url = config.project_url.clone();

    thread::spawn(move || {
        let result = run_current_thread(async move {
            let client = AsyncGitLabClient::new(&config)?;
            client.list_issue_links(issue_iid).await
        });

        let event = match result {
            Ok(links) => BackgroundEvent::IssueLinksLoaded {
                project_url,
                issue_iid,
                links,
            },
            Err(error) => BackgroundEvent::IssueLinksLoadFailed {
                project_url,
                issue_iid,
                error: format!("{error:#}"),
            },
        };
        let _ = sender.send(event);
    });
}

pub fn spawn_async_result<T, Fut>(sender: Sender<Result<T, String>>, future: Fut)
where
    T: Send + 'static,
    Fut: Future<Output = Result<T>> + Send + 'static,
{
    thread::spawn(move || {
        let result = run_current_thread(future).map_err(|error| format!("{error:#}"));
        let _ = sender.send(result);
    });
}

async fn load_project(request: ProjectLoadRequest, sender: Sender<BackgroundEvent>) {
    let project_url = request.config.project_url.clone();
    let generation = request.generation;

    let result = async {
        let client = AsyncGitLabClient::new(&request.config)?;
        let payload = client
            .load_full_project(&project_url, generation, sender.clone())
            .await?;
        Ok::<_, anyhow::Error>(payload)
    }
    .await;

    let event = match result {
        Ok(payload) => BackgroundEvent::ProjectLoaded {
            project_url,
            generation,
            payload,
        },
        Err(error) => BackgroundEvent::ProjectLoadFailed {
            project_url,
            generation,
            error: format!("{error:#}"),
        },
    };

    let _ = sender.send(event);
}

fn run_current_thread<Fut, T>(future: Fut) -> Result<T>
where
    Fut: Future<Output = Result<T>>,
{
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .context("failed to create async runtime")?;
    runtime.block_on(future)
}

fn run_multi_thread<Fut, T>(future: Fut) -> Result<T>
where
    Fut: Future<Output = Result<T>>,
{
    let runtime = Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .context("failed to create async runtime")?;
    runtime.block_on(future)
}

impl AsyncGitLabClient {
    pub fn new(config: &AppConfig) -> Result<Self> {
        let mut headers = HeaderMap::new();
        let private_token =
            HeaderValue::from_str(&config.private_token).context("invalid GitLab private token")?;
        headers.insert("PRIVATE-TOKEN", private_token);

        let http = Client::builder()
            .default_headers(headers)
            .user_agent("glissues")
            .build()
            .context("failed to create HTTP client")?;

        let project = urlencoding::encode(&config.project);
        let project_api_base = format!("{}/api/v4/projects/{}", config.gitlab_url, project);

        Ok(Self {
            http,
            project_api_base,
            project_ref: config.project.clone(),
        })
    }

    async fn load_full_project(
        &self,
        project_url: &str,
        generation: u64,
        sender: Sender<BackgroundEvent>,
    ) -> Result<RefreshPayload> {
        let (mut issues, labels) = tokio::try_join!(self.list_issues(), self.list_labels())?;
        issues.sort_by_key(|issue| issue.iid);

        let total = issues.len();
        let mut notes_cache = HashMap::new();
        let mut issue_links_cache = HashMap::new();

        if total == 0 {
            return Ok(RefreshPayload {
                issues,
                labels,
                notes_cache,
                issue_links_cache,
            });
        }

        let permits = Arc::new(Semaphore::new(ISSUE_DETAIL_CONCURRENCY));
        let mut tasks = JoinSet::new();

        for issue in issues.iter().cloned() {
            let permits = Arc::clone(&permits);
            let client = self.clone();
            tasks.spawn(async move {
                let _permit = permits.acquire_owned().await;
                let iid = issue.iid;
                let notes_fut = async {
                    if issue.user_notes_count == 0 {
                        Ok(Vec::new())
                    } else {
                        client.list_notes(iid).await
                    }
                };
                let (notes, links) = tokio::try_join!(notes_fut, client.list_issue_links(iid))?;
                Ok::<_, anyhow::Error>((iid, notes, links))
            });
        }

        let mut loaded = 0;
        while let Some(result) = tasks.join_next().await {
            let (iid, notes, links) = result.context("project detail task failed")??;
            notes_cache.insert(iid, notes);
            issue_links_cache.insert(iid, links);
            loaded += 1;

            let _ = sender.send(BackgroundEvent::ProjectLoadProgress {
                project_url: project_url.to_string(),
                generation,
                loaded,
                total,
            });
        }

        Ok(RefreshPayload {
            issues,
            labels,
            notes_cache,
            issue_links_cache,
        })
    }

    pub async fn list_issues(&self) -> Result<Vec<Issue>> {
        self.get_paginated(
            "/issues",
            &[
                ("scope", "all".to_string()),
                ("state", "all".to_string()),
                ("order_by", "updated_at".to_string()),
                ("sort", "desc".to_string()),
            ],
        )
        .await
    }

    pub async fn list_labels(&self) -> Result<Vec<String>> {
        let labels = self.get_paginated::<ProjectLabel>("/labels", &[]).await?;
        Ok(labels.into_iter().map(|label| label.name).collect())
    }

    pub async fn list_notes(&self, issue_iid: u64) -> Result<Vec<Note>> {
        self.get_paginated(
            &format!("/issues/{issue_iid}/notes"),
            &[
                ("sort", "asc".to_string()),
                ("order_by", "created_at".to_string()),
                ("activity_filter", "only_comments".to_string()),
            ],
        )
        .await
    }

    pub async fn list_issue_links(&self, issue_iid: u64) -> Result<Vec<IssueLink>> {
        self.get_paginated(&format!("/issues/{issue_iid}/links"), &[])
            .await
    }

    pub async fn create_issue(&self, draft: &IssueDraft) -> Result<Issue> {
        let mut form = vec![("title", draft.title.clone())];

        if !draft.description.is_empty() {
            form.push(("description", draft.description.clone()));
        }
        if !draft.labels.is_empty() {
            form.push(("labels", draft.labels.join(",")));
        }
        if let Some(due_date) = &draft.due_date {
            form.push(("due_date", due_date.clone()));
        }

        self.http
            .post(self.url("/issues"))
            .form(&form)
            .send()
            .await
            .context("failed to create issue")?
            .error_for_status()
            .context("GitLab rejected issue creation")?
            .json::<Issue>()
            .await
            .context("failed to decode create issue response")
    }

    pub async fn update_issue(&self, issue_iid: u64, update: &IssueUpdate) -> Result<Issue> {
        let mut form = Vec::<(&str, String)>::new();

        if let Some(title) = &update.title {
            form.push(("title", title.clone()));
        }
        if let Some(description) = &update.description {
            form.push(("description", description.clone()));
        }
        if let Some(labels) = &update.labels {
            form.push(("labels", labels.join(",")));
        }
        if let Some(due_date) = &update.due_date {
            form.push(("due_date", due_date.clone().unwrap_or_default()));
        }
        if let Some(state_event) = update.state_event {
            let value = match state_event {
                crate::gitlab::StateEvent::Close => "close",
                crate::gitlab::StateEvent::Reopen => "reopen",
            };
            form.push(("state_event", value.to_string()));
        }

        if form.is_empty() {
            return Err(anyhow!("issue update requires at least one field"));
        }

        self.http
            .put(self.url(&format!("/issues/{issue_iid}")))
            .form(&form)
            .send()
            .await
            .context("failed to update issue")?
            .error_for_status()
            .context("GitLab rejected issue update")?
            .json::<Issue>()
            .await
            .context("failed to decode update issue response")
    }

    pub async fn add_note(&self, issue_iid: u64, body: &str) -> Result<Note> {
        self.http
            .post(self.url(&format!("/issues/{issue_iid}/notes")))
            .form(&[("body", body.to_string())])
            .send()
            .await
            .context("failed to create note")?
            .error_for_status()
            .context("GitLab rejected note creation")?
            .json::<Note>()
            .await
            .context("failed to decode note response")
    }

    pub async fn delete_issue(&self, issue_iid: u64) -> Result<()> {
        self.http
            .delete(self.url(&format!("/issues/{issue_iid}")))
            .send()
            .await
            .context("failed to delete issue")?
            .error_for_status()
            .context("GitLab rejected issue deletion")?;
        Ok(())
    }

    pub async fn add_blocker(&self, issue_iid: u64, blocker_iid: u64) -> Result<()> {
        self.http
            .post(self.url(&format!("/issues/{issue_iid}/links")))
            .form(&[
                ("target_project_id", self.project_ref.clone()),
                ("target_issue_iid", blocker_iid.to_string()),
                ("link_type", String::from("is_blocked_by")),
            ])
            .send()
            .await
            .context("failed to add blocker")?
            .error_for_status()
            .context("GitLab rejected blocker creation")?;
        Ok(())
    }

    pub async fn delete_issue_link(&self, issue_iid: u64, issue_link_id: u64) -> Result<()> {
        self.http
            .delete(self.url(&format!("/issues/{issue_iid}/links/{issue_link_id}")))
            .send()
            .await
            .context("failed to remove blocker")?
            .error_for_status()
            .context("GitLab rejected blocker removal")?;
        Ok(())
    }

    async fn get_paginated<T>(&self, path: &str, query: &[(&str, String)]) -> Result<Vec<T>>
    where
        T: DeserializeOwned,
    {
        let mut page = 1_u32;
        let mut items = Vec::new();

        loop {
            let mut params = query
                .iter()
                .map(|(key, value)| (*key, value.clone()))
                .collect::<Vec<(&str, String)>>();
            params.push(("per_page", "100".to_string()));
            params.push(("page", page.to_string()));

            let response = self
                .http
                .get(self.url(path))
                .query(&params)
                .send()
                .await
                .with_context(|| format!("failed to fetch {}", self.url(path)))?
                .error_for_status()
                .with_context(|| format!("GitLab rejected {}", self.url(path)))?;

            let next_page = response
                .headers()
                .get("x-next-page")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string();

            let mut chunk = response
                .json::<Vec<T>>()
                .await
                .with_context(|| format!("failed to decode {}", self.url(path)))?;
            items.append(&mut chunk);

            if next_page.is_empty() {
                break;
            }

            page = next_page.parse::<u32>().unwrap_or(page + 1);
        }

        Ok(items)
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.project_api_base, path)
    }
}
