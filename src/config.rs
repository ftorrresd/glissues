use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use serde::Deserialize;

use crate::theme::ThemeName;

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
    #[arg(long)]
    pub gitlab_url: Option<String>,
    #[arg(long)]
    pub project: Option<String>,
    #[arg(long)]
    pub token: Option<String>,
    #[arg(long)]
    pub theme: Option<String>,
    #[arg(long = "status-label")]
    pub status_labels: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub gitlab_url: String,
    pub project: String,
    pub token: String,
    pub theme: ThemeName,
    pub status_labels: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    gitlab_url: Option<String>,
    project: Option<String>,
    token: Option<String>,
    theme: Option<String>,
    status_labels: Option<Vec<String>>,
}

impl AppConfig {
    pub fn load(cli: Cli) -> Result<Self> {
        let config_path = cli.config.unwrap_or_else(default_config_path);
        let file_config = read_file_config(&config_path)?;

        let gitlab_url = cli
            .gitlab_url
            .or_else(|| env::var("GLISSUES_GITLAB_URL").ok())
            .or(file_config.gitlab_url)
            .unwrap_or_else(|| "https://gitlab.cern.ch".to_string());

        let project = cli
            .project
            .or_else(|| env::var("GLISSUES_PROJECT").ok())
            .or(file_config.project)
            .unwrap_or_else(|| "ftorresd/todo".to_string());

        let token = cli
            .token
            .or_else(|| env::var("GLISSUES_TOKEN").ok())
            .or(file_config.token)
            .ok_or_else(|| {
                anyhow!(
                    "missing GitLab token; set GLISSUES_TOKEN or add token to {}",
                    config_path.display()
                )
            })?;

        let theme = cli
            .theme
            .or_else(|| env::var("GLISSUES_THEME").ok())
            .or(file_config.theme)
            .map(|value| ThemeName::parse(&value))
            .transpose()?
            .unwrap_or(ThemeName::Catppuccin);

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
            gitlab_url: gitlab_url.trim_end_matches('/').to_string(),
            project,
            token,
            theme,
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
