use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use reqwest::Url;
use serde::Deserialize;

pub const DEFAULT_STATUS_LABELS: &[&str] = &[
    "status::todo",
    "status::doing",
    "status::blocked",
    "status::done",
];

#[derive(Debug, Parser)]
#[command(name = "glissues", version, about = "Yazi-inspired GitLab issues TUI")]
pub struct Cli {
    #[arg(long)]
    pub config: Option<PathBuf>,
    #[arg(long, value_name = "URL")]
    pub project: Option<String>,
    #[arg(long)]
    pub token: Option<String>,
    #[arg(long = "status-label")]
    pub status_labels: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub gitlab_url: String,
    pub project: String,
    pub token: String,
    pub status_labels: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    status_labels: Option<Vec<String>>,
}

impl AppConfig {
    pub fn load(cli: Cli) -> Result<Self> {
        let config_path = cli.config.unwrap_or_else(default_config_path);
        let file_config = read_file_config(&config_path)?;

        let project_url = cli
            .project
            .or_else(|| env::var("GLISSUES_PROJECT").ok())
            .or_else(|| env::var("GLISSUES_PROJECT_URL").ok())
            .ok_or_else(|| {
                anyhow!(
                    "missing GitLab project URL; set GLISSUES_PROJECT or pass --project https://gitlab.example.com/group/project"
                )
            })?;

        let (gitlab_url, project) = parse_project_url(&project_url)?;

        let token = cli
            .token
            .or_else(|| env::var("GLISSUES_TOKEN").ok())
            .ok_or_else(|| anyhow!("missing GitLab token; set GLISSUES_TOKEN or pass --token"))?;

        let status_labels = if !cli.status_labels.is_empty() {
            cli.status_labels
        } else if let Ok(value) = env::var("GLISSUES_STATUS_LABELS") {
            split_csv(&value)
        } else if let Some(labels) = file_config.status_labels {
            labels
        } else {
            DEFAULT_STATUS_LABELS
                .iter()
                .map(|value| value.to_string())
                .collect()
        };

        Ok(Self {
            gitlab_url,
            project,
            token,
            status_labels,
        })
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

fn default_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("glissues")
        .join("config.toml")
}

fn split_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn parse_project_url(value: &str) -> Result<(String, String)> {
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
