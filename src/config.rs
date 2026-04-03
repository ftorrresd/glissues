use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use ratatui_themes::ThemeName;
use reqwest::Url;
use serde::{Deserialize, Serialize};

#[derive(Debug, Parser)]
#[command(name = "glissues", version, about = "Yazi-inspired GitLab issues TUI")]
pub struct Cli {
    #[arg(long)]
    pub config: Option<PathBuf>,
    #[arg(long, value_name = "URL")]
    pub project: Option<String>,
    #[arg(long)]
    pub token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub gitlab_url: String,
    pub project: String,
    pub token: String,
    pub config_path: PathBuf,
    pub theme: ThemeName,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct FileConfig {
    theme: Option<ThemeName>,
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

        let theme = file_config.theme.unwrap_or(ThemeName::RosePine);

        Ok(Self {
            gitlab_url,
            project,
            token,
            config_path,
            theme,
        })
    }

    pub fn save_theme(&self, theme: ThemeName) -> Result<()> {
        let mut file_config = read_file_config(&self.config_path)?;
        file_config.theme = Some(theme);
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
    fn saves_theme_into_config_file() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir()
            .join(format!("glissues-test-{unique}"))
            .join("config.toml");

        let config = AppConfig {
            gitlab_url: String::from("https://gitlab.example.com"),
            project: String::from("group/project"),
            token: String::from("token"),
            config_path: path.clone(),
            theme: ThemeName::RosePine,
        };

        config.save_theme(ThemeName::TokyoNight).unwrap();
        let saved = read_file_config(&path).unwrap();
        assert_eq!(saved.theme, Some(ThemeName::TokyoNight));

        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
