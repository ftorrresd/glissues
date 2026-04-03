use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Author {
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Issue {
    pub iid: u64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub web_url: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub due_date: Option<String>,
    #[serde(default)]
    pub user_notes_count: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Note {
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub author: Option<Author>,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IssueLink {
    #[serde(default)]
    pub issue_link_id: u64,
    pub iid: u64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub link_type: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectLabel {
    pub name: String,
}
