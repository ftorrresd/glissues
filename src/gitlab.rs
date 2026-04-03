use anyhow::{Context, Result, anyhow};
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::de::DeserializeOwned;

use crate::config::AppConfig;
use crate::model::{Issue, IssueLink, Note, ProjectLabel};

#[derive(Debug, Clone)]
pub struct GitLabClient {
    http: Client,
    project_api_base: String,
    project_ref: String,
}

#[derive(Debug, Clone)]
pub struct IssueDraft {
    pub title: String,
    pub description: String,
    pub labels: Vec<String>,
    pub due_date: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct IssueUpdate {
    pub title: Option<String>,
    pub description: Option<String>,
    pub labels: Option<Vec<String>>,
    pub due_date: Option<Option<String>>,
    pub state_event: Option<StateEvent>,
}

#[derive(Debug, Clone, Copy)]
pub enum StateEvent {
    Close,
    Reopen,
}

impl GitLabClient {
    pub fn new(config: &AppConfig) -> Result<Self> {
        let mut headers = HeaderMap::new();
        let token = HeaderValue::from_str(&config.token).context("invalid GitLab token")?;
        headers.insert("PRIVATE-TOKEN", token);

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

    pub fn list_issues(&self) -> Result<Vec<Issue>> {
        self.get_paginated(
            "/issues",
            &[
                ("scope", "all".to_string()),
                ("state", "all".to_string()),
                ("order_by", "updated_at".to_string()),
                ("sort", "desc".to_string()),
            ],
        )
    }

    pub fn list_labels(&self) -> Result<Vec<String>> {
        let labels = self.get_paginated::<ProjectLabel>("/labels", &[])?;
        Ok(labels.into_iter().map(|label| label.name).collect())
    }

    pub fn list_notes(&self, issue_iid: u64) -> Result<Vec<Note>> {
        self.get_paginated(
            &format!("/issues/{issue_iid}/notes"),
            &[
                ("sort", "asc".to_string()),
                ("order_by", "created_at".to_string()),
                ("activity_filter", "only_comments".to_string()),
            ],
        )
    }

    pub fn list_issue_links(&self, issue_iid: u64) -> Result<Vec<IssueLink>> {
        self.get_paginated(&format!("/issues/{issue_iid}/links"), &[])
    }

    pub fn create_issue(&self, draft: &IssueDraft) -> Result<Issue> {
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
            .context("failed to create issue")?
            .error_for_status()
            .context("GitLab rejected issue creation")?
            .json::<Issue>()
            .context("failed to decode create issue response")
    }

    pub fn update_issue(&self, issue_iid: u64, update: &IssueUpdate) -> Result<Issue> {
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
                StateEvent::Close => "close",
                StateEvent::Reopen => "reopen",
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
            .context("failed to update issue")?
            .error_for_status()
            .context("GitLab rejected issue update")?
            .json::<Issue>()
            .context("failed to decode update issue response")
    }

    pub fn add_note(&self, issue_iid: u64, body: &str) -> Result<Note> {
        self.http
            .post(self.url(&format!("/issues/{issue_iid}/notes")))
            .form(&[("body", body.to_string())])
            .send()
            .context("failed to create note")?
            .error_for_status()
            .context("GitLab rejected note creation")?
            .json::<Note>()
            .context("failed to decode note response")
    }

    pub fn delete_issue(&self, issue_iid: u64) -> Result<()> {
        self.http
            .delete(self.url(&format!("/issues/{issue_iid}")))
            .send()
            .context("failed to delete issue")?
            .error_for_status()
            .context("GitLab rejected issue deletion")?;
        Ok(())
    }

    pub fn add_blocker(&self, issue_iid: u64, blocker_iid: u64) -> Result<()> {
        self.http
            .post(self.url(&format!("/issues/{issue_iid}/links")))
            .form(&[
                ("target_project_id", self.project_ref.clone()),
                ("target_issue_iid", blocker_iid.to_string()),
                ("link_type", String::from("is_blocked_by")),
            ])
            .send()
            .context("failed to add blocker")?
            .error_for_status()
            .context("GitLab rejected blocker creation")?;
        Ok(())
    }

    pub fn delete_issue_link(&self, issue_iid: u64, issue_link_id: u64) -> Result<()> {
        self.http
            .delete(self.url(&format!("/issues/{issue_iid}/links/{issue_link_id}")))
            .send()
            .context("failed to remove blocker")?
            .error_for_status()
            .context("GitLab rejected blocker removal")?;
        Ok(())
    }

    fn get_paginated<T>(&self, path: &str, query: &[(&str, String)]) -> Result<Vec<T>>
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
