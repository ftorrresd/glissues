use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use ratatui_themes::ThemeName;
use reqwest::Url;
use serde::{Deserialize, Serialize};

#[derive(Debug, Parser)]
#[command(name = "glissues", version, about = "GitLab issues TUI")]
pub struct Cli {
    #[arg(long)]
    pub config: Option<PathBuf>,
    #[arg(long, value_name = "URL")]
    pub project: Option<String>,
    #[arg(long = "private-token")]
    pub private_token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub project_url: String,
    pub gitlab_url: String,
    pub project: String,
    pub private_token: String,
    pub theme: ThemeName,
    pub stored: bool,
}

#[derive(Debug, Clone)]
pub struct StoredProjectConfig {
    pub project_url: String,
    pub gitlab_url: String,
    pub project: String,
    pub private_token: String,
    pub theme: ThemeName,
}

#[derive(Debug, Clone)]
pub struct ConfigStore {
    pub config_path: PathBuf,
    pub last_project: Option<String>,
    pub last_theme: ThemeName,
    pub stored_projects: Vec<StoredProjectConfig>,
}

#[derive(Debug, Clone)]
pub enum StartupProject {
    Direct {
        config: AppConfig,
        should_prompt_store: bool,
    },
    Stored {
        project_url: String,
    },
}

#[derive(Debug, Clone)]
pub struct BootstrapConfig {
    pub store: ConfigStore,
    pub startup: StartupProject,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct FileConfig {
    last_project: Option<String>,
    last_theme: Option<ThemeName>,
    #[serde(default)]
    projects: Vec<StoredProjectRecord>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct StoredProjectRecord {
    url: String,
    #[serde(default)]
    private_token: String,
    theme: Option<ThemeName>,
}

impl BootstrapConfig {
    pub fn load(cli: Cli) -> Result<Self> {
        let config_path = cli.config.unwrap_or_else(default_config_path);
        let file_config = read_file_config(&config_path)?;
        let last_theme = file_config.last_theme.unwrap_or(ThemeName::RosePine);

        let stored_projects = file_config
            .projects
            .into_iter()
            .map(|record| {
                let (gitlab_url, project) = parse_project_url(&record.url)?;
                Ok(StoredProjectConfig {
                    project_url: record.url,
                    gitlab_url,
                    project,
                    private_token: record.private_token,
                    theme: record.theme.unwrap_or(last_theme),
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let store = ConfigStore {
            config_path,
            last_project: file_config.last_project,
            last_theme,
            stored_projects,
        };

        let startup = if let Some(project_url) = cli
            .project
            .or_else(|| env::var("GLISSUES_PROJECT").ok())
            .or_else(|| env::var("GLISSUES_PROJECT_URL").ok())
        {
            let (gitlab_url, project) = parse_project_url(&project_url)?;
            let private_token = cli
                .private_token
                .or_else(|| env::var("GLISSUES_PRIVATE_TOKEN").ok())
                .ok_or_else(|| {
                    anyhow!(
                        "missing GitLab private token; set GLISSUES_PRIVATE_TOKEN or pass --private-token"
                    )
                })?;

            let theme = store
                .find_project(&project_url)
                .map(|project| project.theme)
                .unwrap_or(store.last_theme);

            StartupProject::Direct {
                config: AppConfig {
                    project_url: project_url.clone(),
                    gitlab_url,
                    project,
                    private_token,
                    theme,
                    stored: store.find_project(&project_url).is_some(),
                },
                should_prompt_store: store.find_project(&project_url).is_none(),
            }
        } else if let Some(project_url) = store
            .last_project
            .clone()
            .filter(|url| {
                store
                    .find_project(url)
                    .map(|project| !project.private_token.trim().is_empty())
                    .unwrap_or(false)
            })
            .or_else(|| {
                store
                    .stored_projects
                    .iter()
                    .find(|project| !project.private_token.trim().is_empty())
                    .map(|project| project.project_url.clone())
            })
        {
            StartupProject::Stored { project_url }
        } else if !store.stored_projects.is_empty() {
            return Err(anyhow!(
                "stored projects are missing private tokens; reopen a project with --private-token and save it again"
            ));
        } else {
            return Err(anyhow!(
                "no project configured; pass --project and --private-token or store a project first"
            ));
        };

        Ok(Self { store, startup })
    }
}

impl ConfigStore {
    pub fn find_project(&self, project_url: &str) -> Option<&StoredProjectConfig> {
        self.stored_projects
            .iter()
            .find(|project| project.project_url == project_url)
    }

    pub fn save_project_theme(&mut self, project_url: &str, theme: ThemeName) -> Result<()> {
        self.last_theme = theme;
        if let Some(project) = self
            .stored_projects
            .iter_mut()
            .find(|project| project.project_url == project_url)
        {
            project.theme = theme;
        }
        self.write()
    }

    pub fn set_last_project(&mut self, project_url: &str) -> Result<()> {
        self.last_project = Some(project_url.to_string());
        self.write()
    }

    pub fn save_last_theme(&mut self, theme: ThemeName) -> Result<()> {
        self.last_theme = theme;
        self.write()
    }

    pub fn store_project(
        &mut self,
        project_url: &str,
        private_token: String,
        theme: ThemeName,
    ) -> Result<StoredProjectConfig> {
        let (gitlab_url, project) = parse_project_url(project_url)?;
        let stored_project = StoredProjectConfig {
            project_url: project_url.to_string(),
            gitlab_url,
            project,
            private_token,
            theme,
        };

        if let Some(existing) = self
            .stored_projects
            .iter_mut()
            .find(|project| project.project_url == project_url)
        {
            *existing = stored_project.clone();
        } else {
            self.stored_projects.push(stored_project.clone());
        }

        self.last_project = Some(project_url.to_string());
        self.last_theme = theme;
        self.write()?;
        Ok(stored_project)
    }

    fn write(&self) -> Result<()> {
        let file_config = FileConfig {
            last_project: self.last_project.clone(),
            last_theme: Some(self.last_theme),
            projects: self
                .stored_projects
                .iter()
                .map(|project| StoredProjectRecord {
                    url: project.project_url.clone(),
                    private_token: project.private_token.clone(),
                    theme: Some(project.theme),
                })
                .collect(),
        };
        write_file_config(&self.config_path, &file_config)
    }
}

fn read_file_config(path: &Path) -> Result<FileConfig> {
    if !path.exists() {
        return Ok(FileConfig::default());
    }

    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read config file {}", path.display()))?;
    let parsed = toml::from_str::<FileConfig>(&raw)
        .with_context(|| format!("failed to parse config file {}", path.display()))?;
    Ok(parsed)
}

fn write_file_config(path: &Path, file_config: &FileConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory {}", parent.display()))?;
    }

    let raw = toml::to_string_pretty(file_config).context("failed to serialize config file")?;
    fs::write(path, raw).with_context(|| format!("failed to write config file {}", path.display()))
}

fn default_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("glissues")
        .join("config.toml")
}

pub fn parse_project_url(value: &str) -> Result<(String, String)> {
    let url = Url::parse(value).with_context(|| format!("invalid project URL '{value}'"))?;

    if !matches!(url.scheme(), "http" | "https") {
        return Err(anyhow!(
            "invalid project URL '{value}'; only http and https URLs are supported"
        ));
    }

    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("invalid project URL '{value}'; missing host"))?;

    let mut segments = url
        .path_segments()
        .map(|segments| {
            segments
                .take_while(|segment| !segment.is_empty() && *segment != "-")
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if let Some(last) = segments.last_mut() {
        if last.ends_with(".git") {
            *last = last.trim_end_matches(".git").to_string();
        }
    }

    segments.retain(|segment| !segment.is_empty());

    if segments.len() < 2 {
        return Err(anyhow!(
            "invalid project URL '{value}'; expected a GitLab project URL like https://gitlab.example.com/group/project"
        ));
    }

    let gitlab_url = match url.port() {
        Some(port) => format!("{}://{}:{}", url.scheme(), host, port),
        None => format!("{}://{}", url.scheme(), host),
    };

    Ok((gitlab_url, segments.join("/")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parses_project_url_into_base_and_path() {
        let (gitlab_url, project) =
            parse_project_url("https://gitlab.cern.ch/group/sub/project").unwrap();

        assert_eq!(gitlab_url, "https://gitlab.cern.ch");
        assert_eq!(project, "group/sub/project");
    }

    #[test]
    fn parses_git_suffix_from_project_url() {
        let (_, project) =
            parse_project_url("https://gitlab.example.com/group/project.git").unwrap();
        assert_eq!(project, "group/project");
    }

    #[test]
    fn rejects_non_http_project_urls() {
        let error = parse_project_url("ssh://gitlab.example.com/group/project").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("only http and https URLs are supported")
        );
    }

    #[test]
    fn stores_theme_per_project() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir()
            .join(format!("glissues-test-{unique}"))
            .join("config.toml");

        let mut store = ConfigStore {
            config_path: path.clone(),
            last_project: Some(String::from("https://gitlab.example.com/group/project")),
            last_theme: ThemeName::RosePine,
            stored_projects: vec![StoredProjectConfig {
                project_url: String::from("https://gitlab.example.com/group/project"),
                gitlab_url: String::from("https://gitlab.example.com"),
                project: String::from("group/project"),
                private_token: String::from("private-token"),
                theme: ThemeName::RosePine,
            }],
        };

        store
            .save_project_theme(
                "https://gitlab.example.com/group/project",
                ThemeName::TokyoNight,
            )
            .unwrap();

        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("theme = \"tokyo-night\""));

        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn stores_plaintext_project_credentials_in_config_file() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir()
            .join(format!("glissues-test-{unique}"))
            .join("config.toml");

        let mut store = ConfigStore {
            config_path: path.clone(),
            last_project: None,
            last_theme: ThemeName::RosePine,
            stored_projects: Vec::new(),
        };

        let project = store
            .store_project(
                "https://gitlab.example.com/group/project",
                String::from("private-token"),
                ThemeName::TokyoNight,
            )
            .unwrap();

        assert_eq!(project.project, "group/project");
        assert_eq!(
            store.last_project.as_deref(),
            Some("https://gitlab.example.com/group/project")
        );

        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("private_token = \"private-token\""));
        assert!(raw.contains("theme = \"tokyo-night\""));

        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
