use std::collections::{BTreeSet, HashMap, HashSet};
use std::future::Future;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use anyhow::{Result, anyhow};
use chrono::{Datelike, Duration, Local, NaiveDate};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui_themes::Theme;

use crate::background::{
    AsyncGitLabClient, BackgroundEvent, ProjectLoadRequest, RefreshPayload, spawn_async_result,
    spawn_issue_links_load, spawn_notes_load, spawn_project_load, spawn_startup_preload,
};
use crate::config::{AppConfig, BootstrapConfig, ConfigStore, StartupProject};
use crate::editor::TextBuffer;
use crate::gitlab::{IssueDraft, IssueUpdate, StateEvent};
use crate::model::{Issue, IssueLink, Note};

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    IssueView,
    ConfirmDelete,
    BlockerPicker,
    ThemePicker,
    ProjectPicker,
    StoreProjectPrompt,
    Search,
    Command,
    IssueEditor,
    CommentEditor,
    LabelEditor,
    Selector,
    DueDatePicker,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorField {
    Title,
    Body,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateFilter {
    All,
    Open,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorKind {
    LabelFilter,
}

#[derive(Debug, Clone)]
pub struct Filters {
    pub state: StateFilter,
    pub label: Option<String>,
    pub search: String,
}

#[derive(Debug, Clone)]
pub struct IssueEditorState {
    pub editing_iid: Option<u64>,
    pub title: TextBuffer,
    pub body: TextBuffer,
    pub focus: EditorField,
}

#[derive(Debug, Clone)]
pub struct CommentEditorState {
    pub target_iid: u64,
    pub body: TextBuffer,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LabelPane {
    Add,
    Exclude,
}

#[derive(Debug, Clone)]
pub struct LabelPickerState {
    pub query: String,
    pub current_labels: BTreeSet<String>,
    pub to_add: BTreeSet<String>,
    pub to_exclude: BTreeSet<String>,
    pub cursor_add: usize,
    pub cursor_exclude: usize,
    pub active_pane: LabelPane,
    pub history: Vec<(BTreeSet<String>, BTreeSet<String>)>,
}

#[derive(Debug, Clone)]
pub struct SelectorState {
    pub title: String,
    pub query: String,
    pub options: Vec<String>,
    pub selected: Option<String>,
    pub cursor: usize,
    pub allow_clear: bool,
}

#[derive(Debug, Clone)]
pub struct DueDatePickerState {
    pub month: NaiveDate,
    pub selected: NaiveDate,
}

#[derive(Debug, Clone)]
pub struct DeleteConfirmationState {
    pub iid: u64,
    pub title: String,
}

#[derive(Debug, Clone)]
pub struct AlertState {
    pub title: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy)]
pub enum MentionTarget {
    IssueTitle,
    IssueBody,
    CommentBody,
}

#[derive(Debug, Clone)]
pub struct MentionPickerState {
    pub target: MentionTarget,
    pub query: String,
    pub cursor: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockerAction {
    Add,
    Remove,
}

#[derive(Debug, Clone)]
pub struct BlockerPickerState {
    pub action: BlockerAction,
    pub query: String,
    pub cursor: usize,
}

#[derive(Debug, Clone)]
pub struct BlockerCandidate {
    pub iid: u64,
    pub title: String,
    pub state: String,
    pub issue_link_id: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ProjectPickerState {
    pub query: String,
    pub cursor: usize,
}

#[derive(Debug, Clone)]
pub struct StoreProjectPromptState {
    pub project_url: String,
}

#[derive(Debug, Clone)]
pub struct ProjectMeta {
    pub project_url: String,
    pub gitlab_url: String,
    pub project: String,
    pub theme: ratatui_themes::ThemeName,
    pub stored: bool,
    pub private_token: Option<String>,
}

#[derive(Clone)]
struct ProjectSession {
    config: AppConfig,
    theme: Theme,
    issues: Vec<Issue>,
    labels: Vec<String>,
    notes_cache: HashMap<u64, Vec<Note>>,
    issue_links_cache: HashMap<u64, Vec<IssueLink>>,
    filters: Filters,
    selected: usize,
    issue_view_scroll: u16,
    issue_view_view_height: u16,
    issue_view_content_height: u16,
    issue_editor: Option<IssueEditorState>,
    comment_editor: Option<CommentEditorState>,
}

#[derive(Debug, Clone)]
struct ProjectLoadState {
    generation: u64,
    message: String,
    loaded: usize,
    total: Option<usize>,
    foreground: bool,
    selected_iid: Option<u64>,
}

struct LoadingState {
    spinner_frame: usize,
}

enum PendingActionState {
    IssueSave {
        draft: IssueEditorState,
        return_mode: Mode,
        receiver: Receiver<Result<PendingActionResult, String>>,
    },
    CommentAdd {
        draft: CommentEditorState,
        return_mode: Mode,
        receiver: Receiver<Result<PendingActionResult, String>>,
    },
    Background {
        receiver: Receiver<Result<PendingActionResult, String>>,
    },
}

enum PendingActionResult {
    IssueSaved(Issue),
    CommentAdded {
        iid: u64,
        note: Note,
    },
    IssueUpdated {
        issue: Issue,
        message: String,
    },
    IssueDeleted {
        iid: u64,
        message: String,
    },
    IssueLinksUpdated {
        iid: u64,
        links: Vec<IssueLink>,
        message: String,
    },
    LabelDeleted {
        label: String,
        message: String,
    },
}

pub struct App {
    pub store: ConfigStore,
    pub config: AppConfig,
    pub theme: Theme,
    pub current_project_url: String,
    pub projects: Vec<ProjectMeta>,
    inactive_sessions: HashMap<String, ProjectSession>,
    project_loads: HashMap<String, ProjectLoadState>,
    background_sender: mpsc::Sender<BackgroundEvent>,
    background_receiver: Receiver<BackgroundEvent>,
    loading_notes: HashSet<(String, u64)>,
    loading_issue_links: HashSet<(String, u64)>,
    pub issues: Vec<Issue>,
    pub labels: Vec<String>,
    pub notes_cache: HashMap<u64, Vec<Note>>,
    pub issue_links_cache: HashMap<u64, Vec<IssueLink>>,
    pub filters: Filters,
    pub selected: usize,
    pub issue_view_scroll: u16,
    pub issue_view_view_height: u16,
    pub issue_view_content_height: u16,
    pub mode: Mode,
    pub status_line: String,
    pub should_quit: bool,
    pub search_input: String,
    pub command_input: String,
    pub search_backup: String,
    pub issue_editor: Option<IssueEditorState>,
    pub comment_editor: Option<CommentEditorState>,
    pub label_picker: Option<LabelPickerState>,
    pub selector: Option<SelectorState>,
    pub selector_kind: Option<SelectorKind>,
    pub due_date_picker: Option<DueDatePickerState>,
    pub delete_confirmation: Option<DeleteConfirmationState>,
    pub alert: Option<AlertState>,
    pub mention_picker: Option<MentionPickerState>,
    pub blocker_picker: Option<BlockerPickerState>,
    pub project_picker: Option<ProjectPickerState>,
    pub store_project_prompt: Option<StoreProjectPromptState>,
    return_mode: Mode,
    pending_g: bool,
    next_load_generation: u64,
    loading: Option<LoadingState>,
    pending_action: Option<PendingActionState>,
    undo_history: Vec<Issue>,
}

impl App {
    pub fn new(bootstrap: BootstrapConfig) -> Result<Self> {
        let (config, current_project_url, prompt_store) = match &bootstrap.startup {
            StartupProject::Direct {
                config,
                should_prompt_store,
            } => (
                config.clone(),
                config.project_url.clone(),
                *should_prompt_store,
            ),
            StartupProject::Stored { project_url } => {
                let project = bootstrap
                    .store
                    .find_project(project_url)
                    .ok_or_else(|| anyhow!("stored project not found: {project_url}"))?;
                (
                    AppConfig {
                        project_url: project.project_url.clone(),
                        gitlab_url: project.gitlab_url.clone(),
                        project: project.project.clone(),
                        private_token: project.private_token.clone(),
                        theme: project.theme,
                        stored: true,
                    },
                    project.project_url.clone(),
                    false,
                )
            }
        };

        let theme_name = config.theme;
        let (background_sender, background_receiver) = mpsc::channel();
        let mut app = Self {
            store: bootstrap.store.clone(),
            config,
            theme: Theme::new(theme_name),
            current_project_url,
            projects: project_metas_from_store(&bootstrap.store),
            inactive_sessions: HashMap::new(),
            project_loads: HashMap::new(),
            background_sender,
            background_receiver,
            loading_notes: HashSet::new(),
            loading_issue_links: HashSet::new(),
            issues: Vec::new(),
            labels: Vec::new(),
            notes_cache: HashMap::new(),
            issue_links_cache: HashMap::new(),
            filters: Filters {
                state: StateFilter::Open,
                label: None,
                search: String::new(),
            },
            selected: 0,
            issue_view_scroll: 0,
            issue_view_view_height: 0,
            issue_view_content_height: 0,
            mode: Mode::Normal,
            status_line: String::from("booting glissues"),
            should_quit: false,
            search_input: String::new(),
            command_input: String::new(),
            search_backup: String::new(),
            issue_editor: None,
            comment_editor: None,
            label_picker: None,
            selector: None,
            selector_kind: None,
            due_date_picker: None,
            delete_confirmation: None,
            alert: None,
            mention_picker: None,
            blocker_picker: None,
            project_picker: None,
            store_project_prompt: None,
            return_mode: Mode::Normal,
            pending_g: false,
            next_load_generation: 1,
            loading: None,
            pending_action: None,
            undo_history: Vec::new(),
        };

        if !app
            .projects
            .iter()
            .any(|project| project.project_url == app.current_project_url)
        {
            app.projects.push(ProjectMeta {
                project_url: app.config.project_url.clone(),
                gitlab_url: app.config.gitlab_url.clone(),
                project: app.config.project.clone(),
                theme: app.config.theme,
                stored: app.config.stored,
                private_token: Some(app.config.private_token.clone()),
            });
        }

        match bootstrap.startup {
            StartupProject::Direct { .. } => {
                app.begin_startup_preload("Loading GitLab data");
                if prompt_store {
                    app.store_project_prompt = Some(StoreProjectPromptState {
                        project_url: app.current_project_url.clone(),
                    });
                    app.mode = Mode::StoreProjectPrompt;
                }
            }
            StartupProject::Stored { .. } => {
                app.begin_startup_preload("Loading GitLab data");
            }
        }

        Ok(app)
    }

    pub fn begin_refresh(&mut self, message: &str) {
        let project_url = self.current_project_url.clone();
        if self.project_is_loading(&project_url) {
            return;
        }
        let selected_iid = self.selected_issue().map(|issue| issue.iid);
        self.start_project_load(&project_url, message, true, selected_iid);
    }

    fn begin_startup_preload(&mut self, message: &str) {
        if self.projects.is_empty() {
            return;
        }

        let mut requests = Vec::new();
        let current_project_url = self.current_project_url.clone();

        if let Some(meta) = self
            .projects
            .iter()
            .find(|project| project.project_url == current_project_url)
            .cloned()
        {
            let generation = self.reserve_load_generation();
            self.project_loads.insert(
                meta.project_url.clone(),
                ProjectLoadState {
                    generation,
                    message: message.to_string(),
                    loaded: 0,
                    total: None,
                    foreground: true,
                    selected_iid: None,
                },
            );
            self.loading = Some(LoadingState { spinner_frame: 0 });
            self.status_line = message.to_string();
            if let Some(config) = self.project_config(&meta.project_url) {
                requests.push(ProjectLoadRequest { config, generation });
            }
        }

        let startup_projects = self
            .projects
            .iter()
            .filter(|project| project.project_url != current_project_url)
            .cloned()
            .collect::<Vec<_>>();

        for meta in startup_projects {
            let Some(config) = self.project_config(&meta.project_url) else {
                continue;
            };
            let generation = self.reserve_load_generation();
            self.project_loads.insert(
                meta.project_url.clone(),
                ProjectLoadState {
                    generation,
                    message: format!("Preloading {}", meta.project),
                    loaded: 0,
                    total: None,
                    foreground: false,
                    selected_iid: None,
                },
            );
            requests.push(ProjectLoadRequest { config, generation });
        }

        spawn_startup_preload(requests, self.background_sender.clone());
    }

    fn start_project_load(
        &mut self,
        project_url: &str,
        message: &str,
        foreground: bool,
        selected_iid: Option<u64>,
    ) {
        let Some(config) = self.project_config(project_url) else {
            self.show_error(format!("missing credentials for project {project_url}"));
            return;
        };
        let generation = self.reserve_load_generation();
        self.project_loads.insert(
            project_url.to_string(),
            ProjectLoadState {
                generation,
                message: message.to_string(),
                loaded: 0,
                total: None,
                foreground,
                selected_iid,
            },
        );

        if foreground {
            self.loading = Some(LoadingState { spinner_frame: 0 });
            self.status_line = message.to_string();
        }

        spawn_project_load(
            ProjectLoadRequest { config, generation },
            self.background_sender.clone(),
        );
    }

    fn reserve_load_generation(&mut self) -> u64 {
        let generation = self.next_load_generation;
        self.next_load_generation += 1;
        generation
    }

    pub fn tick(&mut self) {
        if let Some(loading) = self.loading.as_mut() {
            loading.spinner_frame = (loading.spinner_frame + 1) % SPINNER_FRAMES.len();
        }

        loop {
            match self.background_receiver.try_recv() {
                Ok(event) => self.handle_background_event(event),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.show_error("background loader disconnected");
                    break;
                }
            }
        }

        let pending_result = match self.pending_action.as_mut() {
            Some(PendingActionState::IssueSave { receiver, .. }) => match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Disconnected) => {
                    Some(Err(String::from("issue save worker disconnected")))
                }
                Err(TryRecvError::Empty) => None,
            },
            Some(PendingActionState::CommentAdd { receiver, .. }) => match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Disconnected) => {
                    Some(Err(String::from("comment worker disconnected")))
                }
                Err(TryRecvError::Empty) => None,
            },
            Some(PendingActionState::Background { receiver }) => match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Disconnected) => {
                    Some(Err(String::from("background action worker disconnected")))
                }
                Err(TryRecvError::Empty) => None,
            },
            None => None,
        };

        if let Some(result) = pending_result {
            let pending_action = self.pending_action.take();
            if let Some(pending_action) = pending_action {
                self.finish_pending_action(pending_action, result);
            }
        }
    }

    pub fn is_loading(&self) -> bool {
        self.loading.is_some() && self.project_is_loading(&self.current_project_url)
    }

    pub fn loading_message(&self) -> Option<&str> {
        self.project_loads
            .get(&self.current_project_url)
            .map(|state| state.message.as_str())
    }

    pub fn loading_progress_label(&self) -> Option<String> {
        let state = self.project_loads.get(&self.current_project_url)?;
        state
            .total
            .map(|total| format!("{}/{} issues", state.loaded, total))
    }

    pub fn spinner_index(&self) -> usize {
        self.loading
            .as_ref()
            .map(|loading| loading.spinner_frame % SPINNER_FRAMES.len())
            .unwrap_or(0)
    }

    pub fn has_alert(&self) -> bool {
        self.alert.is_some()
    }

    pub fn is_text_editing(&self) -> bool {
        matches!(self.mode, Mode::IssueEditor | Mode::CommentEditor)
    }

    pub fn has_mention_picker(&self) -> bool {
        self.mention_picker.is_some()
    }

    pub fn has_blocker_picker(&self) -> bool {
        self.blocker_picker.is_some()
    }

    pub fn has_project_picker(&self) -> bool {
        self.project_picker.is_some()
    }

    pub fn has_store_project_prompt(&self) -> bool {
        self.store_project_prompt.is_some()
    }

    pub fn is_project_loaded(&self, project_url: &str) -> bool {
        self.current_project_url == project_url || self.inactive_sessions.contains_key(project_url)
    }

    pub fn project_load_label(&self, project_url: &str) -> Option<String> {
        let state = self.project_loads.get(project_url)?;
        let label = match state.total {
            Some(total) => format!("loading {}/{}", state.loaded, total),
            None => String::from("loading"),
        };
        Some(label)
    }

    fn project_is_loading(&self, project_url: &str) -> bool {
        self.project_loads.contains_key(project_url)
    }

    fn project_config(&self, project_url: &str) -> Option<AppConfig> {
        let project = self
            .projects
            .iter()
            .find(|project| project.project_url == project_url)?;
        let private_token = project.private_token.clone()?;
        Some(AppConfig {
            project_url: project.project_url.clone(),
            gitlab_url: project.gitlab_url.clone(),
            project: project.project.clone(),
            private_token,
            theme: project.theme,
            stored: project.stored,
        })
    }

    pub fn show_warning(&mut self, message: impl Into<String>) {
        let message = message.into();
        self.status_line = first_line(&message);
        self.alert = Some(AlertState {
            title: String::from("Warning"),
            message,
        });
    }

    pub fn show_error(&mut self, message: impl Into<String>) {
        let message = message.into();
        self.status_line = first_line(&message);
        self.alert = Some(AlertState {
            title: error_title(&message),
            message,
        });
    }

    fn begin_background_action<F>(&mut self, status_line: impl Into<String>, future: F)
    where
        F: Future<Output = Result<PendingActionResult>> + Send + 'static,
    {
        let (sender, receiver) = mpsc::channel();
        spawn_async_result(sender, future);
        self.pending_action = Some(PendingActionState::Background { receiver });
        self.status_line = status_line.into();
    }

    fn has_pending_action_guard(&mut self) -> bool {
        if self.pending_action.is_some() {
            self.status_line = String::from("wait for the current background action to finish");
            true
        } else {
            false
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Result<()> {
        if self.alert.is_some() {
            match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => self.alert = None,
                _ => {}
            }
            return Ok(());
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('r')) {
            self.pending_g = false;
            self.begin_refresh("Refreshing GitLab data");
            return Ok(());
        }

        if self.is_loading() && !matches!(self.mode, Mode::StoreProjectPrompt) {
            return Ok(());
        }

        match self.mode {
            Mode::Normal => self.handle_normal_mode(key),
            Mode::IssueView => self.handle_issue_view_mode(key),
            Mode::ConfirmDelete => self.handle_confirm_delete_mode(key),
            Mode::BlockerPicker => self.handle_blocker_picker_mode(key),
            Mode::ThemePicker => self.handle_theme_picker_mode(key),
            Mode::ProjectPicker => self.handle_project_picker_mode(key),
            Mode::StoreProjectPrompt => self.handle_store_project_prompt_mode(key),
            Mode::Search => self.handle_search_mode(key),
            Mode::Command => self.handle_command_mode(key),
            Mode::IssueEditor => self.handle_issue_editor_mode(key),
            Mode::CommentEditor => self.handle_comment_editor_mode(key),
            Mode::LabelEditor => self.handle_label_editor_mode(key),
            Mode::Selector => self.handle_selector_mode(key),
            Mode::DueDatePicker => self.handle_due_date_mode(key),
            Mode::Help => {
                self.mode = Mode::Normal;
                Ok(())
            }
        }
    }

    pub fn sync_issue_view_layout(&mut self, view_height: u16, content_height: u16) {
        self.issue_view_view_height = view_height;
        self.issue_view_content_height = content_height;
        self.issue_view_scroll = self.issue_view_scroll.min(self.max_issue_view_scroll());
    }

    pub fn selected_issue(&self) -> Option<&Issue> {
        let visible = self.visible_issue_indices();
        visible
            .get(self.selected)
            .and_then(|index| self.issues.get(*index))
    }

    pub fn selected_notes(&self) -> Option<&[Note]> {
        self.selected_issue()
            .and_then(|issue| self.notes_cache.get(&issue.iid))
            .map(Vec::as_slice)
    }

    pub fn selected_blockers(&self) -> Vec<&IssueLink> {
        let Some(issue) = self.selected_issue() else {
            return Vec::new();
        };

        self.issue_links_cache
            .get(&issue.iid)
            .map(|links| {
                links
                    .iter()
                    .filter(|link| link.link_type == "is_blocked_by")
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn selected_issue_links_loaded(&self) -> bool {
        self.selected_issue()
            .map(|issue| self.issue_links_cache.contains_key(&issue.iid))
            .unwrap_or(true)
    }

    pub fn visible_issue_indices(&self) -> Vec<usize> {
        self.issues
            .iter()
            .enumerate()
            .filter_map(|(index, issue)| self.issue_matches_filters(issue).then_some(index))
            .collect()
    }

    pub fn visible_count(&self) -> usize {
        self.visible_issue_indices().len()
    }

    pub fn state_label(&self) -> &'static str {
        match self.filters.state {
            StateFilter::All => "all",
            StateFilter::Open => "open",
            StateFilter::Closed => "closed",
        }
    }

    pub fn mode_label(&self) -> &'static str {
        match self.mode {
            Mode::Normal => "glissues",
            Mode::IssueView => "ISSUE",
            Mode::ConfirmDelete => "DELETE",
            Mode::BlockerPicker => "BLOCKERS",
            Mode::ThemePicker => "THEMES",
            Mode::ProjectPicker => "PROJECTS",
            Mode::StoreProjectPrompt => "STORE",
            Mode::Search => "SEARCH",
            Mode::Command => "COMMAND",
            Mode::IssueEditor => "EDITOR",
            Mode::CommentEditor => "COMMENT",
            Mode::LabelEditor => "LABELS",
            Mode::Selector => "SELECT",
            Mode::DueDatePicker => "DUE",
            Mode::Help => "HELP",
        }
    }

    pub fn project_label_options(&self) -> Vec<String> {
        let mut labels = self.labels.clone();
        labels.sort();
        labels
    }

    pub fn count_open(&self) -> usize {
        self.issues
            .iter()
            .filter(|issue| issue.state == "opened")
            .count()
    }

    pub fn count_closed(&self) -> usize {
        self.issues
            .iter()
            .filter(|issue| issue.state == "closed")
            .count()
    }

    pub fn count_overdue(&self) -> usize {
        let today = Local::now().date_naive();
        self.issues
            .iter()
            .filter(|issue| issue.state == "opened")
            .filter(|issue| {
                issue
                    .due_date
                    .as_deref()
                    .and_then(parse_due_date)
                    .map(|due| due < today)
                    .unwrap_or(false)
            })
            .count()
    }

    fn handle_normal_mode(&mut self, key: KeyEvent) -> Result<()> {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            self.pending_g = false;
        }

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('?') => self.mode = Mode::Help,
            KeyCode::Char('/') => {
                self.pending_g = false;
                self.search_backup = self.filters.search.clone();
                self.search_input = self.filters.search.clone();
                self.mode = Mode::Search;
            }
            KeyCode::Char(':') => {
                self.pending_g = false;
                self.command_input.clear();
                self.mode = Mode::Command;
            }
            KeyCode::Char('n') => {
                self.pending_g = false;
                self.open_issue_editor(None);
            }
            KeyCode::Char('e') => {
                self.pending_g = false;
                self.open_issue_editor(self.selected_issue().cloned());
            }
            KeyCode::Char('x') => {
                self.pending_g = false;
                self.toggle_selected_issue_state()?;
            }
            KeyCode::Char('D') => {
                self.pending_g = false;
                self.open_delete_confirmation();
            }
            KeyCode::Char('t') => {
                self.pending_g = false;
                self.capture_return_mode();
                self.mode = Mode::ThemePicker;
            }
            KeyCode::Char('p') => {
                self.pending_g = false;
                self.open_project_picker();
            }
            KeyCode::Char('P') => {
                self.pending_g = false;
                self.cycle_project(1)?;
            }
            KeyCode::Char('[') => {
                self.pending_g = false;
                self.cycle_project(-1)?;
            }
            KeyCode::Char(']') => {
                self.pending_g = false;
                self.cycle_project(1)?;
            }
            KeyCode::Char('c') => {
                self.pending_g = false;
                self.open_comment_editor();
            }
            KeyCode::Char('b') => {
                self.pending_g = false;
                self.open_blocker_picker(BlockerAction::Add);
            }
            KeyCode::Char('B') => {
                self.pending_g = false;
                self.open_blocker_picker(BlockerAction::Remove);
            }
            KeyCode::Char('a') => {
                self.pending_g = false;
                self.open_label_editor();
            }
            KeyCode::Char('d') => {
                self.pending_g = false;
                self.open_due_date_picker();
            }
            KeyCode::Char('u') => {
                self.pending_g = false;
                self.undo_issue_state()?;
            }
            KeyCode::Tab => {
                self.pending_g = false;
                self.cycle_state_filter()?;
            }
            KeyCode::BackTab => {
                self.pending_g = false;
                self.cycle_state_filter_backwards()?;
            }
            KeyCode::Char('f') => {
                self.pending_g = false;
                self.cycle_state_filter()?;
            }
            KeyCode::Char('F') | KeyCode::Char('l') => {
                self.pending_g = false;
                self.open_label_filter();
            }
            _ => self.handle_list_key(key)?,
        }

        Ok(())
    }

    fn handle_list_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.pending_g = false;
                self.move_selection(1)?;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.pending_g = false;
                self.move_selection(-1)?;
            }
            KeyCode::PageDown => {
                self.pending_g = false;
                self.move_selection(8)?;
            }
            KeyCode::PageUp => {
                self.pending_g = false;
                self.move_selection(-8)?;
            }
            KeyCode::Char('g') if self.pending_g => {
                self.selected = 0;
                self.pending_g = false;
                self.ensure_selected_notes_loaded()?;
                self.ensure_selected_issue_links_loaded()?;
            }
            KeyCode::Char('g') => self.pending_g = true,
            KeyCode::Char('G') => {
                self.selected = self.visible_count().saturating_sub(1);
                self.pending_g = false;
                self.ensure_selected_notes_loaded()?;
                self.ensure_selected_issue_links_loaded()?;
            }
            KeyCode::Enter => {
                self.pending_g = false;
                self.open_issue_view()?;
            }
            _ => self.pending_g = false,
        }

        Ok(())
    }

    fn handle_issue_view_mode(&mut self, key: KeyEvent) -> Result<()> {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('d') => {
                    self.pending_g = false;
                    self.scroll_issue_view_by(8);
                    return Ok(());
                }
                KeyCode::Char('u') => {
                    self.pending_g = false;
                    self.scroll_issue_view_by(-8);
                    return Ok(());
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Esc => {
                self.pending_g = false;
                self.mode = Mode::Normal;
            }
            KeyCode::Char('e') => {
                self.pending_g = false;
                self.open_issue_editor(self.selected_issue().cloned());
            }
            KeyCode::Char('c') => {
                self.pending_g = false;
                self.open_comment_editor();
            }
            KeyCode::Char('b') => {
                self.pending_g = false;
                self.open_blocker_picker(BlockerAction::Add);
            }
            KeyCode::Char('B') => {
                self.pending_g = false;
                self.open_blocker_picker(BlockerAction::Remove);
            }
            KeyCode::Char('a') => {
                self.pending_g = false;
                self.open_label_editor();
            }
            KeyCode::Char('d') => {
                self.pending_g = false;
                self.open_due_date_picker();
            }
            KeyCode::Char('x') => {
                self.pending_g = false;
                self.toggle_selected_issue_state()?;
            }
            KeyCode::Char('D') => {
                self.pending_g = false;
                self.open_delete_confirmation();
            }
            KeyCode::Char('t') => {
                self.pending_g = false;
                self.capture_return_mode();
                self.mode = Mode::ThemePicker;
            }
            KeyCode::Char('p') => {
                self.pending_g = false;
                self.open_project_picker();
            }
            KeyCode::Char('P') => {
                self.pending_g = false;
                self.cycle_project(1)?;
            }
            KeyCode::Char('[') => {
                self.pending_g = false;
                self.cycle_project(-1)?;
            }
            KeyCode::Char(']') => {
                self.pending_g = false;
                self.cycle_project(1)?;
            }
            KeyCode::Char('?') => {
                self.pending_g = false;
                self.mode = Mode::Help;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.pending_g = false;
                self.scroll_issue_view_by(1);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.pending_g = false;
                self.scroll_issue_view_by(-1);
            }
            KeyCode::PageDown => {
                self.pending_g = false;
                self.scroll_issue_view_by(12);
            }
            KeyCode::PageUp => {
                self.pending_g = false;
                self.scroll_issue_view_by(-12);
            }
            KeyCode::Char('g') if self.pending_g => {
                self.issue_view_scroll = 0;
                self.pending_g = false;
            }
            KeyCode::Char('g') => self.pending_g = true,
            KeyCode::Char('G') => {
                self.issue_view_scroll = self.max_issue_view_scroll();
                self.pending_g = false;
            }
            _ => self.pending_g = false,
        }

        Ok(())
    }

    fn handle_confirm_delete_mode(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') => {
                self.delete_confirmation = None;
                self.restore_return_mode();
            }
            KeyCode::Char('y') => self.confirm_delete_issue()?,
            _ => {}
        }

        Ok(())
    }

    fn handle_blocker_picker_mode(&mut self, key: KeyEvent) -> Result<()> {
        if self.blocker_picker.is_none() {
            self.restore_return_mode();
            return Ok(());
        }

        match key.code {
            KeyCode::Esc => {
                self.blocker_picker = None;
                self.restore_return_mode();
            }
            KeyCode::Enter => self.apply_blocker_picker()?,
            KeyCode::Backspace => {
                if let Some(picker) = self.blocker_picker.as_mut() {
                    picker.query.pop();
                    picker.cursor = 0;
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let count = self.blocker_candidates().len().max(1);
                if let Some(picker) = self.blocker_picker.as_mut() {
                    picker.cursor = (picker.cursor + 1).min(count - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(picker) = self.blocker_picker.as_mut() {
                    picker.cursor = picker.cursor.saturating_sub(1);
                }
            }
            KeyCode::Char(ch)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if let Some(picker) = self.blocker_picker.as_mut() {
                    picker.query.push(ch);
                    picker.cursor = 0;
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn handle_theme_picker_mode(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                self.restore_return_mode();
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.theme.prev();
                self.config.theme = self.theme.name;
                self.update_project_theme(self.current_project_url.clone(), self.theme.name)?;
                self.status_line = format!("theme: {}", self.theme.name.display_name());
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.theme.next();
                self.config.theme = self.theme.name;
                self.update_project_theme(self.current_project_url.clone(), self.theme.name)?;
                self.status_line = format!("theme: {}", self.theme.name.display_name());
            }
            _ => {}
        }

        Ok(())
    }

    fn handle_project_picker_mode(&mut self, key: KeyEvent) -> Result<()> {
        if self.project_picker.is_none() {
            self.restore_return_mode();
            return Ok(());
        }

        match key.code {
            KeyCode::Esc => {
                self.project_picker = None;
                self.restore_return_mode();
            }
            KeyCode::Enter => {
                let project_url = self.current_project_picker_choice();
                self.project_picker = None;
                self.restore_return_mode();
                if let Some(project_url) = project_url {
                    self.request_project_activation(&project_url)?;
                }
            }
            KeyCode::Backspace => {
                if let Some(picker) = self.project_picker.as_mut() {
                    picker.query.pop();
                    picker.cursor = 0;
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let count = self.project_picker_candidates().len().max(1);
                if let Some(picker) = self.project_picker.as_mut() {
                    picker.cursor = (picker.cursor + 1).min(count - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(picker) = self.project_picker.as_mut() {
                    picker.cursor = picker.cursor.saturating_sub(1);
                }
            }
            KeyCode::Char(ch)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if let Some(picker) = self.project_picker.as_mut() {
                    picker.query.push(ch);
                    picker.cursor = 0;
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn handle_store_project_prompt_mode(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('y') => {
                self.persist_current_project()?;
                self.store_project_prompt = None;
                self.restore_return_mode();
            }
            KeyCode::Esc | KeyCode::Char('n') => {
                self.store_project_prompt = None;
                self.restore_return_mode();
            }
            _ => {}
        }

        Ok(())
    }

    fn handle_search_mode(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.filters.search = self.search_backup.clone();
                self.search_input = self.search_backup.clone();
                self.mode = Mode::Normal;
                self.selected = 0;
                self.clamp_selection();
                self.ensure_selected_notes_loaded()?;
                self.ensure_selected_issue_links_loaded()?;
            }
            KeyCode::Enter => {
                self.mode = Mode::Normal;
                self.selected = 0;
                self.issue_view_scroll = 0;
                self.clamp_selection();
                self.ensure_selected_notes_loaded()?;
                self.ensure_selected_issue_links_loaded()?;
            }
            KeyCode::Backspace => {
                self.search_input.pop();
                self.filters.search = self.search_input.clone();
                self.selected = 0;
                self.clamp_selection();
            }
            KeyCode::Char(ch)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.search_input.push(ch);
                self.filters.search = self.search_input.clone();
                self.selected = 0;
                self.clamp_selection();
            }
            _ => {}
        }

        Ok(())
    }

    fn handle_command_mode(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.command_input.clear();
            }
            KeyCode::Enter => {
                let command = self.command_input.trim().to_string();
                self.command_input.clear();
                self.mode = Mode::Normal;
                self.execute_command(&command)?;
            }
            KeyCode::Backspace => {
                self.command_input.pop();
            }
            KeyCode::Char(ch)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.command_input.push(ch);
            }
            _ => {}
        }

        Ok(())
    }

    fn handle_issue_editor_mode(&mut self, key: KeyEvent) -> Result<()> {
        if self.mention_picker.is_some() {
            return self.handle_mention_picker(key);
        }

        let Some(editor) = self.issue_editor.as_mut() else {
            self.mode = self.return_mode;
            return Ok(());
        };

        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('s')) {
            return self.save_issue_editor();
        }

        match key.code {
            KeyCode::Esc => {
                self.restore_return_mode();
                self.status_line = String::from("issue draft kept locally");
            }
            KeyCode::Tab => editor.focus = next_field(editor.focus),
            KeyCode::BackTab => editor.focus = previous_field(editor.focus),
            KeyCode::Enter if matches!(editor.focus, EditorField::Title) => {
                editor.focus = EditorField::Body;
            }
            KeyCode::Char('#')
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                let target = match editor.focus {
                    EditorField::Title => MentionTarget::IssueTitle,
                    EditorField::Body => MentionTarget::IssueBody,
                };
                let buffer = active_issue_buffer(editor);
                buffer.insert_char('#');
                self.open_mention_picker(target);
            }
            _ => {
                let multiline = matches!(editor.focus, EditorField::Body);
                let buffer = active_issue_buffer(editor);
                buffer.handle_insert_key(key, multiline);
            }
        }

        Ok(())
    }

    fn handle_comment_editor_mode(&mut self, key: KeyEvent) -> Result<()> {
        if self.mention_picker.is_some() {
            return self.handle_mention_picker(key);
        }

        let Some(editor) = self.comment_editor.as_mut() else {
            self.mode = self.return_mode;
            return Ok(());
        };

        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('s')) {
            return self.save_comment_editor();
        }

        match key.code {
            KeyCode::Esc => {
                self.restore_return_mode();
                self.status_line = String::from("comment draft kept locally");
            }
            KeyCode::Char('#')
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                editor.body.insert_char('#');
                self.open_mention_picker(MentionTarget::CommentBody);
            }
            _ => {
                editor.body.handle_insert_key(key, true);
            }
        }

        Ok(())
    }

    fn handle_label_editor_mode(&mut self, key: KeyEvent) -> Result<()> {
        let Some(picker) = self.label_picker.as_mut() else {
            self.mode = self.return_mode;
            return Ok(());
        };

        match key.code {
            KeyCode::Esc => {
                self.label_picker = None;
                self.restore_return_mode();
            }
            KeyCode::Enter => return self.save_label_picker(),
            KeyCode::Tab => {
                picker.active_pane = match picker.active_pane {
                    LabelPane::Add => LabelPane::Exclude,
                    LabelPane::Exclude => LabelPane::Add,
                };
            }
            KeyCode::Char('h') | KeyCode::Left => {
                picker.active_pane = LabelPane::Add;
            }
            KeyCode::Char('l') | KeyCode::Right => {
                picker.active_pane = LabelPane::Exclude;
            }
            KeyCode::Backspace => {
                if picker.query.is_empty() {
                    match picker.active_pane {
                        LabelPane::Add => {
                            if let Some(last) = picker.to_add.iter().last().cloned() {
                                picker.to_add.remove(&last);
                            }
                        }
                        LabelPane::Exclude => {
                            if let Some(last) = picker.to_exclude.iter().last().cloned() {
                                picker.to_exclude.remove(&last);
                            }
                        }
                    }
                } else {
                    picker.query.pop();
                }
            }
            KeyCode::Char('j') | KeyCode::Down => match picker.active_pane {
                LabelPane::Add => {
                    let count = picker.filtered_labels(&self.labels).len().max(1);
                    picker.cursor_add = (picker.cursor_add + 1).min(count - 1);
                }
                LabelPane::Exclude => {
                    let count = picker.filtered_exclude_labels().len().max(1);
                    picker.cursor_exclude = (picker.cursor_exclude + 1).min(count - 1);
                }
            },
            KeyCode::Char('k') | KeyCode::Up => match picker.active_pane {
                LabelPane::Add => {
                    picker.cursor_add = picker.cursor_add.saturating_sub(1);
                }
                LabelPane::Exclude => {
                    picker.cursor_exclude = picker.cursor_exclude.saturating_sub(1);
                }
            },
            KeyCode::Char(' ') => {
                picker.push_history();
                match picker.active_pane {
                    LabelPane::Add => {
                        if let Some(label) = picker.current_choice_add(&self.labels) {
                            if picker.to_add.contains(&label) {
                                picker.to_add.remove(&label);
                            } else {
                                picker.to_add.insert(label.clone());
                                picker.to_exclude.remove(&label);
                            }
                        }
                    }
                    LabelPane::Exclude => {
                        if let Some(label) = picker.current_choice_exclude() {
                            if picker.to_exclude.contains(&label) {
                                picker.to_exclude.remove(&label);
                            } else {
                                picker.to_exclude.insert(label.clone());
                                picker.to_add.remove(&label);
                            }
                        }
                    }
                }
            }
            KeyCode::Char('u') if picker.query.is_empty() => {
                picker.undo();
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(label) = match picker.active_pane {
                    LabelPane::Add => picker.current_choice_add(&self.labels),
                    LabelPane::Exclude => picker.current_choice_exclude(),
                } {
                    picker.push_history();
                    return self.delete_label_from_project(&label);
                }
            }
            KeyCode::Char(ch)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                picker.query.push(ch);
            }
            _ => {}
        }

        Ok(())
    }

    fn handle_selector_mode(&mut self, key: KeyEvent) -> Result<()> {
        let Some(selector) = self.selector.as_mut() else {
            self.mode = self.return_mode;
            return Ok(());
        };

        match key.code {
            KeyCode::Esc => {
                self.selector = None;
                self.selector_kind = None;
                self.restore_return_mode();
            }
            KeyCode::Enter => return self.apply_selector(),
            KeyCode::Char(' ') => {
                let filtered = selector.filtered_options();
                if let Some(option) = filtered.get(selector.cursor) {
                    if selector.selected.as_deref() == Some(option.as_str()) {
                        selector.selected = None;
                    } else {
                        selector.selected = Some(option.clone());
                    }
                    if self.selector_kind == Some(SelectorKind::LabelFilter) {
                        self.filters.label = selector.selected.clone();
                        self.selected = 0;
                        self.issue_view_scroll = 0;
                        self.clamp_selection();
                        self.ensure_selected_notes_loaded()?;
                        self.ensure_selected_issue_links_loaded()?;
                        self.status_line = match &self.filters.label {
                            Some(label) => format!("label filter: {label}"),
                            None => String::from("label filter cleared"),
                        };
                    }
                }
            }
            KeyCode::Backspace => {
                if selector.query.is_empty() && selector.allow_clear {
                    selector.selected = None;
                } else {
                    selector.query.pop();
                }
                selector.cursor = 0;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let count = selector.filtered_options().len().max(1);
                selector.cursor = (selector.cursor + 1).min(count - 1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                selector.cursor = selector.cursor.saturating_sub(1);
            }
            KeyCode::Char(ch)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                selector.query.push(ch);
                selector.cursor = 0;
            }
            _ => {}
        }

        Ok(())
    }

    fn handle_due_date_mode(&mut self, key: KeyEvent) -> Result<()> {
        let Some(picker) = self.due_date_picker.as_mut() else {
            self.mode = self.return_mode;
            return Ok(());
        };

        match key.code {
            KeyCode::Esc => {
                self.due_date_picker = None;
                self.restore_return_mode();
            }
            KeyCode::Enter => {
                let selected = picker.selected;
                return self.save_due_date_picker(Some(selected));
            }
            KeyCode::Backspace | KeyCode::Delete | KeyCode::Char('x') => {
                return self.save_due_date_picker(None);
            }
            KeyCode::Char('t') => {
                picker.selected = Local::now().date_naive();
                picker.month = first_of_month(picker.selected);
            }
            KeyCode::Char('h') | KeyCode::Left => shift_due_date(picker, -1),
            KeyCode::Char('l') | KeyCode::Right => shift_due_date(picker, 1),
            KeyCode::Char('j') | KeyCode::Down => shift_due_date(picker, 7),
            KeyCode::Char('k') | KeyCode::Up => shift_due_date(picker, -7),
            KeyCode::Char('H') => shift_month(picker, -1),
            KeyCode::Char('L') => shift_month(picker, 1),
            _ => {}
        }

        Ok(())
    }

    fn execute_command(&mut self, command: &str) -> Result<()> {
        match command {
            "" => {}
            "q" | "quit" => self.should_quit = true,
            "refresh" | "r" => self.begin_refresh("Refreshing GitLab data"),
            "new" => self.open_issue_editor(None),
            "close" => self.close_selected_issue()?,
            "reopen" => self.reopen_selected_issue()?,
            "comment" => self.open_comment_editor(),
            "labels" => self.open_label_editor(),
            "due" => self.open_due_date_picker(),
            "filter open" => self.set_state_filter(StateFilter::Open)?,
            "filter closed" => self.set_state_filter(StateFilter::Closed)?,
            "filter all" => self.set_state_filter(StateFilter::All)?,
            "filter label clear" => {
                self.filters.label = None;
                self.selected = 0;
                self.clamp_selection();
                self.ensure_selected_notes_loaded()?;
                self.ensure_selected_issue_links_loaded()?;
            }
            _ => self.status_line = format!("unknown command: {command}"),
        }

        Ok(())
    }

    fn handle_mention_picker(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.mention_picker = None;
            }
            KeyCode::Enter => {
                if let Some(issue_iid) = self.current_mention_issue_iid() {
                    self.insert_issue_mention(issue_iid);
                } else {
                    self.mention_picker = None;
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let count = self.mention_candidates().len().max(1);
                if let Some(picker) = self.mention_picker.as_mut() {
                    picker.cursor = (picker.cursor + 1).min(count - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(picker) = self.mention_picker.as_mut() {
                    picker.cursor = picker.cursor.saturating_sub(1);
                }
            }
            KeyCode::Backspace => {
                if let Some(picker) = self.mention_picker.as_mut() {
                    if !picker.query.is_empty() {
                        picker.query.pop();
                        picker.cursor = 0;
                    } else {
                        let target = picker.target;
                        self.mention_picker = None;
                        if let Some(buffer) = self.mention_target_buffer_mut(target) {
                            buffer.backspace();
                        }
                    }
                }
            }
            KeyCode::Char(ch)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if let Some(picker) = self.mention_picker.as_mut() {
                    picker.query.push(ch);
                    picker.cursor = 0;
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn move_selection(&mut self, delta: isize) -> Result<()> {
        let count = self.visible_count();
        if count == 0 {
            self.selected = 0;
            return Ok(());
        }

        let next = (self.selected as isize + delta).clamp(0, (count - 1) as isize) as usize;
        if next != self.selected {
            self.selected = next;
            self.ensure_selected_notes_loaded()?;
            self.ensure_selected_issue_links_loaded()?;
        }
        Ok(())
    }

    pub fn mention_candidates(&self) -> Vec<usize> {
        let query = self
            .mention_picker
            .as_ref()
            .map(|picker| picker.query.trim().to_lowercase())
            .unwrap_or_default();

        self.issues
            .iter()
            .enumerate()
            .filter(|(_, issue)| mention_matches(issue, &query))
            .map(|(index, _)| index)
            .take(12)
            .collect()
    }

    pub fn current_mention_issue_iid(&self) -> Option<u64> {
        let candidates = self.mention_candidates();
        let cursor = self.mention_picker.as_ref()?.cursor;
        candidates
            .get(cursor.min(candidates.len().saturating_sub(1)))
            .and_then(|index| self.issues.get(*index))
            .map(|issue| issue.iid)
    }

    pub fn blocker_candidates(&self) -> Vec<BlockerCandidate> {
        let Some(picker) = self.blocker_picker.as_ref() else {
            return Vec::new();
        };

        let query = picker.query.trim().to_lowercase();
        match picker.action {
            BlockerAction::Add => {
                let selected_iid = self.selected_issue().map(|issue| issue.iid);
                let blocked_iids = self
                    .selected_blockers()
                    .into_iter()
                    .map(|link| link.iid)
                    .collect::<Vec<_>>();

                self.issues
                    .iter()
                    .filter(|issue| Some(issue.iid) != selected_iid)
                    .filter(|issue| !blocked_iids.contains(&issue.iid))
                    .filter(|issue| mention_matches(issue, &query))
                    .map(|issue| BlockerCandidate {
                        iid: issue.iid,
                        title: issue.title.clone(),
                        state: issue.state.clone(),
                        issue_link_id: None,
                    })
                    .take(20)
                    .collect()
            }
            BlockerAction::Remove => self
                .selected_blockers()
                .into_iter()
                .filter(|link| blocker_matches(link, &query))
                .map(|link| BlockerCandidate {
                    iid: link.iid,
                    title: link.title.clone(),
                    state: link.state.clone(),
                    issue_link_id: Some(link.issue_link_id),
                })
                .take(20)
                .collect(),
        }
    }

    fn current_blocker_candidate(&self) -> Option<BlockerCandidate> {
        let candidates = self.blocker_candidates();
        let cursor = self.blocker_picker.as_ref()?.cursor;
        candidates
            .get(cursor.min(candidates.len().saturating_sub(1)))
            .cloned()
    }

    pub fn project_picker_candidates(&self) -> Vec<ProjectMeta> {
        let query = self
            .project_picker
            .as_ref()
            .map(|picker| picker.query.trim().to_lowercase())
            .unwrap_or_default();

        self.projects
            .iter()
            .filter(|project| {
                if query.is_empty() {
                    true
                } else {
                    project.project_url.to_lowercase().contains(&query)
                        || project.project.to_lowercase().contains(&query)
                }
            })
            .cloned()
            .collect()
    }

    fn current_project_picker_choice(&self) -> Option<String> {
        let candidates = self.project_picker_candidates();
        let cursor = self.project_picker.as_ref()?.cursor;
        candidates
            .get(cursor.min(candidates.len().saturating_sub(1)))
            .map(|project| project.project_url.clone())
    }

    fn open_issue_view(&mut self) -> Result<()> {
        self.issue_view_scroll = 0;
        self.ensure_selected_notes_loaded()?;
        self.ensure_selected_issue_links_loaded()?;
        self.mode = Mode::IssueView;
        Ok(())
    }

    fn open_project_picker(&mut self) {
        if !self.can_switch_projects() {
            return;
        }

        self.capture_return_mode();
        self.project_picker = Some(ProjectPickerState {
            query: String::new(),
            cursor: self.current_project_index().unwrap_or(0),
        });
        self.mode = Mode::ProjectPicker;
    }

    fn cycle_project(&mut self, delta: isize) -> Result<()> {
        if !self.can_switch_projects() || self.projects.is_empty() {
            return Ok(());
        }

        let current = self.current_project_index().unwrap_or(0);
        let next = (current as isize + delta).rem_euclid(self.projects.len() as isize) as usize;
        let project_url = self.projects[next].project_url.clone();
        self.request_project_activation(&project_url)
    }

    fn request_project_activation(&mut self, project_url: &str) -> Result<()> {
        if self.current_project_url == project_url {
            return Ok(());
        }

        if let Some(session) = self.inactive_sessions.remove(project_url) {
            self.activate_existing_session(project_url, session)?;
            return Ok(());
        }

        let meta = self
            .projects
            .iter()
            .find(|project| project.project_url == project_url)
            .cloned()
            .ok_or_else(|| anyhow!("unknown project: {project_url}"))?;

        self.activate_new_project(meta)?;

        Ok(())
    }

    fn activate_existing_session(
        &mut self,
        project_url: &str,
        session: ProjectSession,
    ) -> Result<()> {
        self.stash_current_session();
        self.restore_session(session);
        self.current_project_url = project_url.to_string();
        self.mode = Mode::Normal;
        self.loading = None;
        self.store.set_last_project(project_url)?;
        Ok(())
    }

    fn activate_new_project(&mut self, meta: ProjectMeta) -> Result<()> {
        self.stash_current_session();

        let config = self
            .project_config(&meta.project_url)
            .ok_or_else(|| anyhow!("stored project is missing a private token"))?;
        self.set_active_project_context(config, meta.theme)?;
        self.mode = Mode::Normal;
        self.store.set_last_project(&meta.project_url)?;

        if !self.project_is_loading(&meta.project_url) {
            self.start_project_load(
                &meta.project_url,
                &format!("Loading {}", meta.project),
                true,
                None,
            );
        } else if let Some(state) = self.project_loads.get_mut(&meta.project_url) {
            state.foreground = true;
            state.message = format!("Loading {}", meta.project);
            self.loading = Some(LoadingState { spinner_frame: 0 });
            self.status_line = state.message.clone();
        }

        Ok(())
    }

    fn set_active_project_context(
        &mut self,
        config: AppConfig,
        theme_name: ratatui_themes::ThemeName,
    ) -> Result<()> {
        self.config = config;
        self.theme = Theme::new(theme_name);
        self.current_project_url = self.config.project_url.clone();
        self.clear_active_project_state();
        Ok(())
    }

    fn stash_current_session(&mut self) {
        if self.current_project_url.is_empty() || self.project_is_loading(&self.current_project_url)
        {
            return;
        }

        self.inactive_sessions.insert(
            self.current_project_url.clone(),
            ProjectSession {
                config: self.config.clone(),
                theme: self.theme.clone(),
                issues: self.issues.clone(),
                labels: self.labels.clone(),
                notes_cache: self.notes_cache.clone(),
                issue_links_cache: self.issue_links_cache.clone(),
                filters: self.filters.clone(),
                selected: self.selected,
                issue_view_scroll: self.issue_view_scroll,
                issue_view_view_height: self.issue_view_view_height,
                issue_view_content_height: self.issue_view_content_height,
                issue_editor: self.issue_editor.clone(),
                comment_editor: self.comment_editor.clone(),
            },
        );
    }

    fn restore_session(&mut self, session: ProjectSession) {
        self.config = session.config;
        self.theme = session.theme;
        self.issues = session.issues;
        self.labels = session.labels;
        self.notes_cache = session.notes_cache;
        self.issue_links_cache = session.issue_links_cache;
        self.filters = session.filters;
        self.selected = session.selected;
        self.issue_view_scroll = session.issue_view_scroll;
        self.issue_view_view_height = session.issue_view_view_height;
        self.issue_view_content_height = session.issue_view_content_height;
        self.issue_editor = session.issue_editor;
        self.comment_editor = session.comment_editor;
        self.label_picker = None;
        self.selector = None;
        self.selector_kind = None;
        self.due_date_picker = None;
        self.delete_confirmation = None;
        self.alert = None;
        self.mention_picker = None;
        self.blocker_picker = None;
        self.project_picker = None;
        self.store_project_prompt = None;
        self.return_mode = Mode::Normal;
        self.pending_g = false;
        self.loading = None;
        self.pending_action = None;
        self.loading_notes.clear();
        self.loading_issue_links.clear();
    }

    fn clear_active_project_state(&mut self) {
        self.issues.clear();
        self.labels.clear();
        self.notes_cache.clear();
        self.issue_links_cache.clear();
        self.filters = Filters {
            state: StateFilter::Open,
            label: None,
            search: String::new(),
        };
        self.selected = 0;
        self.issue_view_scroll = 0;
        self.issue_view_view_height = 0;
        self.issue_view_content_height = 0;
        self.search_input.clear();
        self.command_input.clear();
        self.search_backup.clear();
        self.issue_editor = None;
        self.comment_editor = None;
        self.label_picker = None;
        self.selector = None;
        self.selector_kind = None;
        self.due_date_picker = None;
        self.delete_confirmation = None;
        self.alert = None;
        self.mention_picker = None;
        self.blocker_picker = None;
        self.project_picker = None;
        self.store_project_prompt = None;
        self.return_mode = Mode::Normal;
        self.pending_g = false;
        self.loading_notes.clear();
        self.loading_issue_links.clear();
        self.pending_action = None;
    }

    fn persist_current_project(&mut self) -> Result<()> {
        if self.config.stored {
            self.status_line = String::from("project already stored");
            return Ok(());
        }

        let stored_project = self.store.store_project(
            &self.config.project_url,
            self.config.private_token.clone(),
            self.theme.name,
        )?;

        self.config.stored = true;
        self.update_project_meta(ProjectMeta {
            project_url: stored_project.project_url.clone(),
            gitlab_url: stored_project.gitlab_url.clone(),
            project: stored_project.project.clone(),
            theme: stored_project.theme,
            stored: true,
            private_token: Some(stored_project.private_token.clone()),
        });
        self.status_line = String::from("stored project configuration");
        Ok(())
    }

    fn update_project_theme(
        &mut self,
        project_url: String,
        theme: ratatui_themes::ThemeName,
    ) -> Result<()> {
        if let Some(project) = self
            .projects
            .iter_mut()
            .find(|project| project.project_url == project_url)
        {
            project.theme = theme;
        }

        if self.config.stored {
            self.store.save_project_theme(&project_url, theme)
        } else {
            self.store.save_last_theme(theme)
        }
    }

    fn current_project_index(&self) -> Option<usize> {
        self.projects
            .iter()
            .position(|project| project.project_url == self.current_project_url)
    }

    fn can_switch_projects(&mut self) -> bool {
        if self.loading.is_some() || self.pending_action.is_some() {
            self.status_line = String::from("wait for the current project action to finish");
            return false;
        }

        if !matches!(self.mode, Mode::Normal | Mode::IssueView) {
            self.status_line = String::from("close the current popup before switching projects");
            return false;
        }

        true
    }
    fn update_project_meta(&mut self, meta: ProjectMeta) {
        if let Some(existing) = self
            .projects
            .iter_mut()
            .find(|project| project.project_url == meta.project_url)
        {
            *existing = meta;
        } else {
            self.projects.push(meta);
        }
        self.projects.sort_by(|left, right| {
            left.project
                .cmp(&right.project)
                .then(left.project_url.cmp(&right.project_url))
        });
    }

    fn open_blocker_picker(&mut self, action: BlockerAction) {
        if self.selected_issue().is_none() {
            return;
        }
        if action == BlockerAction::Remove && self.selected_blockers().is_empty() {
            self.status_line = String::from("no blockers to remove");
            return;
        }

        self.capture_return_mode();
        self.blocker_picker = Some(BlockerPickerState {
            action,
            query: String::new(),
            cursor: 0,
        });
        self.mode = Mode::BlockerPicker;
    }

    fn apply_blocker_picker(&mut self) -> Result<()> {
        if self.has_pending_action_guard() {
            return Ok(());
        }

        let Some(candidate) = self.current_blocker_candidate() else {
            self.blocker_picker = None;
            self.restore_return_mode();
            return Ok(());
        };
        let Some(issue_iid) = self.selected_issue().map(|issue| issue.iid) else {
            self.blocker_picker = None;
            self.restore_return_mode();
            return Ok(());
        };

        let action = self
            .blocker_picker
            .as_ref()
            .map(|picker| picker.action)
            .unwrap_or(BlockerAction::Add);

        self.blocker_picker = None;
        self.restore_return_mode();

        let config = self.config.clone();
        let status_line = match action {
            BlockerAction::Add => format!("adding blocker #{} in background", candidate.iid),
            BlockerAction::Remove => {
                format!("removing blocker #{} in background", candidate.iid)
            }
        };

        self.begin_background_action(status_line, async move {
            let client = AsyncGitLabClient::new(&config)?;
            let message = match action {
                BlockerAction::Add => {
                    client.add_blocker(issue_iid, candidate.iid).await?;
                    format!("added blocker #{}", candidate.iid)
                }
                BlockerAction::Remove => {
                    let link_id = candidate
                        .issue_link_id
                        .ok_or_else(|| anyhow!("missing blocker link id"))?;
                    client.delete_issue_link(issue_iid, link_id).await?;
                    format!("removed blocker #{}", candidate.iid)
                }
            };

            let links = client.list_issue_links(issue_iid).await?;
            Ok(PendingActionResult::IssueLinksUpdated {
                iid: issue_iid,
                links,
                message,
            })
        });

        Ok(())
    }

    fn open_mention_picker(&mut self, target: MentionTarget) {
        self.mention_picker = Some(MentionPickerState {
            target,
            query: String::new(),
            cursor: 0,
        });
    }

    fn insert_issue_mention(&mut self, iid: u64) {
        let Some(target) = self.mention_picker.take().map(|picker| picker.target) else {
            return;
        };

        if let Some(buffer) = self.mention_target_buffer_mut(target) {
            buffer.insert_str(&iid.to_string());
        }
    }

    fn open_delete_confirmation(&mut self) {
        let Some((iid, title)) = self
            .selected_issue()
            .map(|issue| (issue.iid, issue.title.clone()))
        else {
            return;
        };

        self.capture_return_mode();
        self.delete_confirmation = Some(DeleteConfirmationState { iid, title });
        self.mode = Mode::ConfirmDelete;
    }

    fn confirm_delete_issue(&mut self) -> Result<()> {
        if self.has_pending_action_guard() {
            return Ok(());
        }

        let Some(confirm) = self.delete_confirmation.clone() else {
            self.restore_return_mode();
            return Ok(());
        };

        self.delete_confirmation = None;
        self.mode = Mode::Normal;
        self.return_mode = Mode::Normal;

        let config = self.config.clone();
        let iid = confirm.iid;
        self.begin_background_action(format!("deleting #{} in background", iid), async move {
            let client = AsyncGitLabClient::new(&config)?;
            client.delete_issue(iid).await?;
            Ok(PendingActionResult::IssueDeleted {
                iid,
                message: format!("deleted #{}", iid),
            })
        });

        Ok(())
    }

    fn cycle_state_filter(&mut self) -> Result<()> {
        let next = match self.filters.state {
            StateFilter::All => StateFilter::Open,
            StateFilter::Open => StateFilter::Closed,
            StateFilter::Closed => StateFilter::All,
        };
        self.set_state_filter(next)
    }

    fn cycle_state_filter_backwards(&mut self) -> Result<()> {
        let next = match self.filters.state {
            StateFilter::All => StateFilter::Closed,
            StateFilter::Closed => StateFilter::Open,
            StateFilter::Open => StateFilter::All,
        };
        self.set_state_filter(next)
    }

    pub fn scroll_issue_view_by(&mut self, delta: i16) {
        let max_scroll = self.max_issue_view_scroll();
        let next = if delta >= 0 {
            self.issue_view_scroll.saturating_add(delta as u16)
        } else {
            self.issue_view_scroll.saturating_sub(delta.unsigned_abs())
        };
        self.issue_view_scroll = next.min(max_scroll);
    }

    pub fn max_issue_view_scroll(&self) -> u16 {
        self.issue_view_content_height
            .saturating_sub(self.issue_view_view_height)
    }

    fn toggle_selected_issue_state(&mut self) -> Result<()> {
        if let Some(issue) = self.selected_issue() {
            if issue.state == "opened" {
                return self.close_selected_issue();
            }
            return self.reopen_selected_issue();
        }

        Ok(())
    }

    fn close_selected_issue(&mut self) -> Result<()> {
        if self.has_pending_action_guard() {
            return Ok(());
        }

        if let Some(issue) = self.selected_issue() {
            self.undo_history.push(issue.clone());
        }

        let iid = self
            .selected_issue()
            .map(|issue| issue.iid)
            .ok_or_else(|| anyhow!("no issue selected"))?;

        let config = self.config.clone();
        self.begin_background_action(format!("closing #{} in background", iid), async move {
            let client = AsyncGitLabClient::new(&config)?;
            let issue = client
                .update_issue(
                    iid,
                    &IssueUpdate {
                        state_event: Some(StateEvent::Close),
                        ..IssueUpdate::default()
                    },
                )
                .await?;
            Ok(PendingActionResult::IssueUpdated {
                issue,
                message: format!("closed #{}", iid),
            })
        });

        Ok(())
    }

    fn reopen_selected_issue(&mut self) -> Result<()> {
        if self.has_pending_action_guard() {
            return Ok(());
        }

        if let Some(issue) = self.selected_issue() {
            self.undo_history.push(issue.clone());
        }

        let iid = self
            .selected_issue()
            .map(|issue| issue.iid)
            .ok_or_else(|| anyhow!("no issue selected"))?;

        let config = self.config.clone();
        self.begin_background_action(format!("reopening #{} in background", iid), async move {
            let client = AsyncGitLabClient::new(&config)?;
            let issue = client
                .update_issue(
                    iid,
                    &IssueUpdate {
                        state_event: Some(StateEvent::Reopen),
                        ..IssueUpdate::default()
                    },
                )
                .await?;
            Ok(PendingActionResult::IssueUpdated {
                issue,
                message: format!("reopened #{}", iid),
            })
        });

        Ok(())
    }

    fn undo_issue_state(&mut self) -> Result<()> {
        if let Some(previous_issue) = self.undo_history.pop() {
            self.replace_issue(previous_issue);
            self.status_line = String::from("undo");
        }
        Ok(())
    }

    fn ensure_selected_notes_loaded(&mut self) -> Result<()> {
        let Some(iid) = self.selected_issue().map(|issue| issue.iid) else {
            return Ok(());
        };
        let key = (self.current_project_url.clone(), iid);
        if !self.notes_cache.contains_key(&iid) && !self.loading_notes.contains(&key) {
            self.loading_notes.insert(key);
            spawn_notes_load(self.config.clone(), iid, self.background_sender.clone());
        }
        Ok(())
    }

    fn ensure_selected_issue_links_loaded(&mut self) -> Result<()> {
        let Some(iid) = self.selected_issue().map(|issue| issue.iid) else {
            return Ok(());
        };
        let key = (self.current_project_url.clone(), iid);
        if !self.issue_links_cache.contains_key(&iid) && !self.loading_issue_links.contains(&key) {
            self.loading_issue_links.insert(key);
            spawn_issue_links_load(self.config.clone(), iid, self.background_sender.clone());
        }
        Ok(())
    }

    fn set_state_filter(&mut self, state: StateFilter) -> Result<()> {
        self.filters.state = state;
        self.selected = 0;
        self.issue_view_scroll = 0;
        self.clamp_selection();
        self.ensure_selected_notes_loaded()?;
        self.ensure_selected_issue_links_loaded()?;
        self.status_line = format!("state filter: {}", self.state_label());
        Ok(())
    }

    fn open_issue_editor(&mut self, issue: Option<Issue>) {
        if self.pending_action.is_some() {
            self.status_line = String::from("wait for the current background save to finish");
            return;
        }

        self.capture_return_mode();
        let target_iid = issue.as_ref().map(|item| item.iid);
        if self
            .issue_editor
            .as_ref()
            .map(|draft| draft.editing_iid == target_iid)
            .unwrap_or(false)
        {
            self.mode = Mode::IssueEditor;
            return;
        }

        self.mention_picker = None;
        self.issue_editor = Some(match issue {
            Some(issue) => IssueEditorState {
                editing_iid: Some(issue.iid),
                title: TextBuffer::from_text(&issue.title),
                body: TextBuffer::from_text(&issue.description),
                focus: EditorField::Title,
            },
            None => IssueEditorState {
                editing_iid: None,
                title: TextBuffer::new(),
                body: TextBuffer::new(),
                focus: EditorField::Title,
            },
        });
        self.mode = Mode::IssueEditor;
    }

    fn save_issue_editor(&mut self) -> Result<()> {
        let Some(mut editor) = self.issue_editor.take() else {
            return Ok(());
        };
        self.mention_picker = None;

        if self.pending_action.is_some() {
            self.issue_editor = Some(editor);
            self.status_line = String::from("wait for the current background save to finish");
            return Ok(());
        }

        let title = editor.title.to_text().trim().to_string();
        if title.is_empty() {
            self.issue_editor = Some(editor);
            self.status_line = String::from("title cannot be empty");
            return Ok(());
        }

        let description = normalized_issue_body(&title, &editor.body.to_text());
        if !has_meaningful_content(&editor.body.to_text()) {
            editor.body = TextBuffer::from_text(&description);
        }

        let config = self.config.clone();
        let editor_snapshot = editor.clone();
        let return_mode = self.return_mode;
        let (sender, receiver) = mpsc::channel();
        let status_line = match editor.editing_iid {
            Some(iid) => format!("saving #{} in background", iid),
            None => String::from("creating issue in background"),
        };

        spawn_async_result(sender, async move {
            let client = AsyncGitLabClient::new(&config)?;
            let issue = match editor_snapshot.editing_iid {
                Some(iid) => {
                    client
                        .update_issue(
                            iid,
                            &IssueUpdate {
                                title: Some(title),
                                description: Some(description),
                                ..IssueUpdate::default()
                            },
                        )
                        .await?
                }
                None => {
                    client
                        .create_issue(&IssueDraft {
                            title,
                            description,
                            labels: Vec::new(),
                            due_date: None,
                        })
                        .await?
                }
            };
            Ok(PendingActionResult::IssueSaved(issue))
        });

        self.restore_return_mode();
        self.pending_action = Some(PendingActionState::IssueSave {
            draft: editor,
            return_mode,
            receiver,
        });
        self.status_line = status_line;
        Ok(())
    }

    fn open_comment_editor(&mut self) {
        if self.pending_action.is_some() {
            self.status_line = String::from("wait for the current background save to finish");
            return;
        }

        let Some(target_iid) = self.selected_issue().map(|issue| issue.iid) else {
            return;
        };

        self.capture_return_mode();
        if self
            .comment_editor
            .as_ref()
            .map(|draft| draft.target_iid == target_iid)
            .unwrap_or(false)
        {
            self.mode = Mode::CommentEditor;
            return;
        }

        self.mention_picker = None;
        self.comment_editor = Some(CommentEditorState {
            target_iid,
            body: TextBuffer::new(),
        });
        self.mode = Mode::CommentEditor;
    }

    fn save_comment_editor(&mut self) -> Result<()> {
        let Some(editor) = self.comment_editor.take() else {
            return Ok(());
        };
        self.mention_picker = None;

        if self.pending_action.is_some() {
            self.comment_editor = Some(editor);
            self.status_line = String::from("wait for the current background save to finish");
            return Ok(());
        }

        let iid = editor.target_iid;

        let body = editor.body.to_text().trim().to_string();
        if body.is_empty() {
            self.comment_editor = Some(editor);
            self.show_warning("comments cannot be empty; please write a message before saving");
            return Ok(());
        }

        let config = self.config.clone();
        let return_mode = self.return_mode;
        let (sender, receiver) = mpsc::channel();

        spawn_async_result(sender, async move {
            let client = AsyncGitLabClient::new(&config)?;
            let note = client.add_note(iid, &body).await?;
            Ok(PendingActionResult::CommentAdded { iid, note })
        });

        self.restore_return_mode();
        self.pending_action = Some(PendingActionState::CommentAdd {
            draft: editor,
            return_mode,
            receiver,
        });
        self.status_line = format!("adding comment to #{} in background", iid);
        Ok(())
    }

    fn open_label_editor(&mut self) {
        let Some(current_labels) = self
            .selected_issue()
            .map(|issue| issue.labels.iter().cloned().collect::<BTreeSet<_>>())
        else {
            return;
        };

        self.capture_return_mode();
        self.label_picker = Some(LabelPickerState {
            query: String::new(),
            current_labels,
            to_add: BTreeSet::new(),
            to_exclude: BTreeSet::new(),
            cursor_add: 0,
            cursor_exclude: 0,
            active_pane: LabelPane::Add,
            history: Vec::new(),
        });
        self.mode = Mode::LabelEditor;
    }

    fn save_label_picker(&mut self) -> Result<()> {
        if self.has_pending_action_guard() {
            return Ok(());
        }

        let Some(picker) = self.label_picker.take() else {
            return Ok(());
        };
        let iid = self
            .selected_issue()
            .map(|issue| issue.iid)
            .ok_or_else(|| anyhow!("no issue selected"))?;

        let labels = picker
            .current_labels
            .union(&picker.to_add)
            .cloned()
            .collect::<BTreeSet<_>>()
            .difference(&picker.to_exclude)
            .cloned()
            .collect::<Vec<_>>();

        self.restore_return_mode();

        let config = self.config.clone();
        self.begin_background_action(
            format!("updating labels for #{} in background", iid),
            async move {
                let client = AsyncGitLabClient::new(&config)?;
                let issue = client
                    .update_issue(
                        iid,
                        &IssueUpdate {
                            labels: Some(labels),
                            ..IssueUpdate::default()
                        },
                    )
                    .await?;
                Ok(PendingActionResult::IssueUpdated {
                    issue,
                    message: format!("labels updated for #{}", iid),
                })
            },
        );

        Ok(())
    }

    fn delete_label_from_project(&mut self, label: &str) -> Result<()> {
        if self.has_pending_action_guard() {
            return Ok(());
        }

        let label = label.to_string();
        let config = self.config.clone();
        self.begin_background_action(
            format!("deleting label '{}' from project", label),
            async move {
                let client = AsyncGitLabClient::new(&config)?;
                client.delete_label(&label).await?;
                Ok(PendingActionResult::LabelDeleted {
                    label: label.clone(),
                    message: format!("label '{}' deleted", label),
                })
            },
        );

        Ok(())
    }

    fn open_label_filter(&mut self) {
        self.capture_return_mode();
        self.selector = Some(SelectorState {
            title: String::from("Filter by Label"),
            query: String::new(),
            options: self.project_label_options(),
            selected: self.filters.label.clone(),
            cursor: 0,
            allow_clear: true,
        });
        self.selector_kind = Some(SelectorKind::LabelFilter);
        self.mode = Mode::Selector;
    }

    fn apply_selector(&mut self) -> Result<()> {
        let kind = self.selector_kind.take();
        let selector = self.selector.take();
        let Some(kind) = kind else {
            self.restore_return_mode();
            return Ok(());
        };
        let Some(selector) = selector else {
            self.restore_return_mode();
            return Ok(());
        };

        let choice = selector.current_choice();

        match kind {
            SelectorKind::LabelFilter => {
                self.filters.label = choice;
                self.selected = 0;
                self.issue_view_scroll = 0;
                self.clamp_selection();
                self.ensure_selected_notes_loaded()?;
                self.ensure_selected_issue_links_loaded()?;
                self.status_line = match &self.filters.label {
                    Some(label) => format!("label filter: {label}"),
                    None => String::from("label filter cleared"),
                };
            }
        }

        self.restore_return_mode();
        Ok(())
    }

    fn open_due_date_picker(&mut self) {
        let selected = self
            .selected_issue()
            .and_then(|issue| issue.due_date.as_deref())
            .and_then(parse_due_date)
            .unwrap_or_else(|| Local::now().date_naive());

        self.capture_return_mode();
        self.due_date_picker = Some(DueDatePickerState {
            month: first_of_month(selected),
            selected,
        });
        self.mode = Mode::DueDatePicker;
    }

    fn save_due_date_picker(&mut self, value: Option<NaiveDate>) -> Result<()> {
        if self.has_pending_action_guard() {
            return Ok(());
        }

        let iid = self
            .selected_issue()
            .map(|issue| issue.iid)
            .ok_or_else(|| anyhow!("no issue selected"))?;

        self.due_date_picker = None;
        self.restore_return_mode();

        let config = self.config.clone();
        let message = match value {
            Some(date) => format!("due date set to {}", date.format("%Y-%m-%d")),
            None => format!("due date cleared for #{iid}"),
        };
        self.begin_background_action(
            format!("saving due date for #{} in background", iid),
            async move {
                let client = AsyncGitLabClient::new(&config)?;
                let issue = client
                    .update_issue(
                        iid,
                        &IssueUpdate {
                            due_date: Some(value.map(|date| date.format("%Y-%m-%d").to_string())),
                            ..IssueUpdate::default()
                        },
                    )
                    .await?;
                Ok(PendingActionResult::IssueUpdated { issue, message })
            },
        );

        Ok(())
    }

    fn issue_matches_filters(&self, issue: &Issue) -> bool {
        if !matches_state_filter(issue, self.filters.state) {
            return false;
        }

        if let Some(label) = &self.filters.label {
            if !issue.labels.iter().any(|item| item == label) {
                return false;
            }
        }

        if !self.filters.search.trim().is_empty() {
            let search = self.filters.search.to_lowercase();
            let haystack = format!(
                "{}\n{}\n{}",
                issue.title,
                issue.description,
                issue.labels.join(" ")
            )
            .to_lowercase();

            if !haystack.contains(&search) {
                return false;
            }
        }

        true
    }

    fn clamp_selection(&mut self) {
        let count = self.visible_count();
        if count == 0 {
            self.selected = 0;
        } else if self.selected >= count {
            self.selected = count - 1;
        }
    }

    fn restore_selection(&mut self, selected_iid: Option<u64>) {
        if let Some(iid) = selected_iid {
            let visible = self.visible_issue_indices();
            if let Some(position) = visible
                .iter()
                .position(|index| self.issues[*index].iid == iid)
            {
                self.selected = position;
                return;
            }
        }
        self.clamp_selection();
    }

    fn rebuild_label_catalog(&mut self) {
        rebuild_label_catalog_for(&self.issues, &mut self.labels);
    }

    fn replace_issue(&mut self, issue: Issue) {
        let iid = issue.iid;
        if let Some(index) = self.issues.iter().position(|item| item.iid == iid) {
            self.issues[index] = issue;
        } else {
            self.issues.insert(0, issue);
        }

        self.issues.sort_by_key(|issue| issue.iid);
        self.rebuild_label_catalog();
        self.restore_selection(Some(iid));
    }

    fn remove_issue(&mut self, iid: u64) {
        self.issues.retain(|issue| issue.iid != iid);
        self.notes_cache.remove(&iid);
        self.issue_links_cache.remove(&iid);
        self.clamp_selection();
    }

    fn apply_refresh_payload(&mut self, payload: RefreshPayload, selected_iid: Option<u64>) {
        self.issues = payload.issues;
        self.labels = payload.labels;
        self.notes_cache = payload.notes_cache;
        self.issue_links_cache = payload.issue_links_cache;
        self.loading_notes.clear();
        self.loading_issue_links.clear();
        self.rebuild_label_catalog();
        self.restore_selection(selected_iid);
        self.status_line = format!(
            "{} issues loaded from {}",
            self.issues.len(),
            self.config.project
        );
    }

    fn handle_background_event(&mut self, event: BackgroundEvent) {
        match event {
            BackgroundEvent::ProjectLoadProgress {
                project_url,
                generation,
                loaded,
                total,
            } => {
                if let Some(state) = self.project_loads.get_mut(&project_url) {
                    if state.generation == generation {
                        state.loaded = loaded;
                        state.total = Some(total);
                    }
                }
            }
            BackgroundEvent::ProjectLoaded {
                project_url,
                generation,
                payload,
            } => {
                let Some(state) = self.project_loads.get(&project_url).cloned() else {
                    return;
                };
                if state.generation != generation {
                    return;
                }

                self.project_loads.remove(&project_url);

                if self.current_project_url == project_url {
                    self.apply_refresh_payload(payload, state.selected_iid);
                } else if let Err(error) = self.store_prefetched_session(&project_url, payload) {
                    self.show_error(format!("failed to cache {}: {error:#}", project_url));
                }

                self.sync_loading_overlay();
            }
            BackgroundEvent::ProjectLoadFailed {
                project_url,
                generation,
                error,
            } => {
                let Some(state) = self.project_loads.get(&project_url).cloned() else {
                    return;
                };
                if state.generation != generation {
                    return;
                }

                self.project_loads.remove(&project_url);
                if self.current_project_url == project_url && state.foreground {
                    self.sync_loading_overlay();
                    self.show_error(format!("refresh failed: {error}"));
                } else {
                    self.status_line = format!("failed to preload {}", project_url);
                    self.sync_loading_overlay();
                }
            }
            BackgroundEvent::NotesLoaded {
                project_url,
                issue_iid,
                notes,
            } => {
                self.loading_notes.remove(&(project_url.clone(), issue_iid));
                if self.current_project_url == project_url {
                    self.notes_cache.insert(issue_iid, notes);
                } else if let Some(session) = self.inactive_sessions.get_mut(&project_url) {
                    session.notes_cache.insert(issue_iid, notes);
                }
            }
            BackgroundEvent::NotesLoadFailed {
                project_url,
                issue_iid,
                error,
            } => {
                self.loading_notes.remove(&(project_url.clone(), issue_iid));
                if self.current_project_url == project_url {
                    self.status_line = format!("failed to load comments for #{}", issue_iid);
                    self.show_error(format!("comment preload failed: {error}"));
                }
            }
            BackgroundEvent::IssueLinksLoaded {
                project_url,
                issue_iid,
                links,
            } => {
                self.loading_issue_links
                    .remove(&(project_url.clone(), issue_iid));
                if self.current_project_url == project_url {
                    self.issue_links_cache.insert(issue_iid, links);
                } else if let Some(session) = self.inactive_sessions.get_mut(&project_url) {
                    session.issue_links_cache.insert(issue_iid, links);
                }
            }
            BackgroundEvent::IssueLinksLoadFailed {
                project_url,
                issue_iid,
                error,
            } => {
                self.loading_issue_links
                    .remove(&(project_url.clone(), issue_iid));
                if self.current_project_url == project_url {
                    self.status_line = format!("failed to load blockers for #{}", issue_iid);
                    self.show_error(format!("blocker preload failed: {error}"));
                }
            }
        }
    }

    fn store_prefetched_session(
        &mut self,
        project_url: &str,
        payload: RefreshPayload,
    ) -> Result<()> {
        let config = self
            .project_config(project_url)
            .ok_or_else(|| anyhow!("missing project config for {project_url}"))?;
        let theme = Theme::new(config.theme);
        let mut labels = payload.labels;
        rebuild_label_catalog_for(&payload.issues, &mut labels);

        self.inactive_sessions.insert(
            project_url.to_string(),
            ProjectSession {
                config,
                theme,
                issues: payload.issues,
                labels,
                notes_cache: payload.notes_cache,
                issue_links_cache: payload.issue_links_cache,
                filters: Filters {
                    state: StateFilter::Open,
                    label: None,
                    search: String::new(),
                },
                selected: 0,
                issue_view_scroll: 0,
                issue_view_view_height: 0,
                issue_view_content_height: 0,
                issue_editor: None,
                comment_editor: None,
            },
        );

        Ok(())
    }

    fn sync_loading_overlay(&mut self) {
        if self.project_is_loading(&self.current_project_url) {
            if self.loading.is_none() {
                self.loading = Some(LoadingState { spinner_frame: 0 });
            }
        } else {
            self.loading = None;
        }
    }

    fn finish_pending_action(
        &mut self,
        pending_action: PendingActionState,
        result: Result<PendingActionResult, String>,
    ) {
        match (pending_action, result) {
            (PendingActionState::IssueSave { .. }, Ok(PendingActionResult::IssueSaved(issue))) => {
                let iid = issue.iid;
                self.replace_issue(issue);
                self.status_line = format!("saved #{}", iid);
            }
            (
                PendingActionState::CommentAdd { .. },
                Ok(PendingActionResult::CommentAdded { iid, note }),
            ) => {
                self.notes_cache.entry(iid).or_default().push(note);
                if let Some(issue) = self.issues.iter_mut().find(|issue| issue.iid == iid) {
                    issue.user_notes_count = issue.user_notes_count.saturating_add(1);
                }
                self.status_line = format!("comment added to #{}", iid);
            }
            (
                PendingActionState::Background { .. },
                Ok(PendingActionResult::IssueUpdated { issue, message }),
            ) => {
                self.replace_issue(issue);
                self.status_line = message;
            }
            (
                PendingActionState::Background { .. },
                Ok(PendingActionResult::IssueDeleted { iid, message }),
            ) => {
                self.remove_issue(iid);
                self.status_line = message;
            }
            (
                PendingActionState::Background { .. },
                Ok(PendingActionResult::IssueLinksUpdated {
                    iid,
                    links,
                    message,
                }),
            ) => {
                self.issue_links_cache.insert(iid, links);
                self.status_line = message;
            }
            (
                PendingActionState::Background { .. },
                Ok(PendingActionResult::LabelDeleted { label, message }),
            ) => {
                self.labels.retain(|l| l != &label);
                self.status_line = message;
            }
            (
                PendingActionState::IssueSave {
                    draft, return_mode, ..
                },
                Err(error),
            ) => {
                self.issue_editor = Some(draft);
                self.return_mode = return_mode;
                self.mode = Mode::IssueEditor;
                self.show_error(format!("issue save failed: {error}"));
            }
            (
                PendingActionState::CommentAdd {
                    draft, return_mode, ..
                },
                Err(error),
            ) => {
                self.comment_editor = Some(draft);
                self.return_mode = return_mode;
                self.mode = Mode::CommentEditor;
                self.show_error(format!("comment save failed: {error}"));
            }
            (PendingActionState::Background { .. }, Err(error)) => {
                self.show_error(format!("background action failed: {error}"));
            }
            _ => {
                self.show_error("background action finished with an unexpected result");
            }
        }
    }

    fn capture_return_mode(&mut self) {
        self.return_mode = if matches!(self.mode, Mode::IssueView) {
            Mode::IssueView
        } else {
            Mode::Normal
        };
    }

    fn restore_return_mode(&mut self) {
        self.mode = self.return_mode;
        self.return_mode = Mode::Normal;
        self.mention_picker = None;
        self.blocker_picker = None;
        self.project_picker = None;
        self.store_project_prompt = None;
    }

    fn mention_target_buffer_mut(&mut self, target: MentionTarget) -> Option<&mut TextBuffer> {
        match target {
            MentionTarget::IssueTitle => self.issue_editor.as_mut().map(|editor| &mut editor.title),
            MentionTarget::IssueBody => self.issue_editor.as_mut().map(|editor| &mut editor.body),
            MentionTarget::CommentBody => {
                self.comment_editor.as_mut().map(|editor| &mut editor.body)
            }
        }
    }
}

impl LabelPickerState {
    pub fn filtered_labels(&self, labels: &[String]) -> Vec<String> {
        let query = self.query.trim().to_lowercase();
        let mut options = labels
            .iter()
            .filter(|label| {
                if query.is_empty() {
                    true
                } else {
                    label.to_lowercase().contains(&query)
                }
            })
            .cloned()
            .collect::<Vec<_>>();
        options.sort();

        if !self.query.trim().is_empty() && !options.iter().any(|label| label == self.query.trim())
        {
            options.insert(0, self.query.trim().to_string());
        }

        options
    }

    pub fn filtered_exclude_labels(&self) -> Vec<String> {
        let query = self.query.trim().to_lowercase();
        let mut options = self
            .current_labels
            .iter()
            .filter(|label| {
                if query.is_empty() {
                    true
                } else {
                    label.to_lowercase().contains(&query)
                }
            })
            .cloned()
            .collect::<Vec<_>>();
        options.sort();
        options
    }

    pub fn current_choice_add(&self, labels: &[String]) -> Option<String> {
        let filtered = self.filtered_labels(labels);
        if filtered.is_empty() {
            return (!self.query.trim().is_empty()).then(|| self.query.trim().to_string());
        }
        filtered
            .get(self.cursor_add.min(filtered.len() - 1))
            .cloned()
    }

    pub fn current_choice_exclude(&self) -> Option<String> {
        let filtered = self.filtered_exclude_labels();
        if filtered.is_empty() {
            return None;
        }
        filtered
            .get(self.cursor_exclude.min(filtered.len() - 1))
            .cloned()
    }

    pub fn push_history(&mut self) {
        self.history
            .push((self.to_add.clone(), self.to_exclude.clone()));
    }

    pub fn undo(&mut self) {
        if let Some((to_add, to_exclude)) = self.history.pop() {
            self.to_add = to_add;
            self.to_exclude = to_exclude;
        }
    }
}

impl SelectorState {
    pub fn filtered_options(&self) -> Vec<String> {
        let query = self.query.trim().to_lowercase();
        let mut options = self
            .options
            .iter()
            .filter(|option| {
                if query.is_empty() {
                    true
                } else {
                    option.to_lowercase().contains(&query)
                }
            })
            .cloned()
            .collect::<Vec<_>>();
        options.sort();
        options
    }

    pub fn current_choice(&self) -> Option<String> {
        let filtered = self.filtered_options();
        if filtered.is_empty() {
            return self.selected.clone();
        }
        filtered.get(self.cursor.min(filtered.len() - 1)).cloned()
    }
}

fn active_issue_buffer(editor: &mut IssueEditorState) -> &mut TextBuffer {
    match editor.focus {
        EditorField::Title => &mut editor.title,
        EditorField::Body => &mut editor.body,
    }
}

fn next_field(field: EditorField) -> EditorField {
    match field {
        EditorField::Title => EditorField::Body,
        EditorField::Body => EditorField::Title,
    }
}

fn previous_field(field: EditorField) -> EditorField {
    next_field(field)
}

fn rebuild_label_catalog_for(issues: &[Issue], labels: &mut Vec<String>) {
    for issue in issues {
        for label in &issue.labels {
            if !labels.contains(label) {
                labels.push(label.clone());
            }
        }
    }

    labels.sort();
    labels.dedup();
}

fn matches_state_filter(issue: &Issue, filter: StateFilter) -> bool {
    match filter {
        StateFilter::All => true,
        StateFilter::Open => issue.state == "opened",
        StateFilter::Closed => issue.state == "closed",
    }
}

pub fn parse_due_date(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()
}

pub fn format_timestamp(value: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|datetime| {
            datetime
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|_| value.to_string())
}

fn first_of_month(date: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(date.year(), date.month(), 1).expect("valid first day of month")
}

fn shift_due_date(state: &mut DueDatePickerState, days: i64) {
    state.selected += Duration::days(days);
    state.month = first_of_month(state.selected);
}

fn shift_month(state: &mut DueDatePickerState, delta: i32) {
    let mut year = state.selected.year();
    let mut month = state.selected.month() as i32 + delta;

    while month < 1 {
        month += 12;
        year -= 1;
    }
    while month > 12 {
        month -= 12;
        year += 1;
    }

    let month_u32 = month as u32;
    let day = state.selected.day().min(last_day_of_month(year, month_u32));
    state.selected =
        NaiveDate::from_ymd_opt(year, month_u32, day).expect("shifted month should stay valid");
    state.month = first_of_month(state.selected);
}

fn last_day_of_month(year: i32, month: u32) -> u32 {
    let next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1).expect("valid date")
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1).expect("valid date")
    };
    (next - Duration::days(1)).day()
}

fn first_line(message: &str) -> String {
    message.lines().next().unwrap_or(message).to_string()
}

fn error_title(message: &str) -> String {
    if message.contains("GitLab rejected")
        || message.contains("HTTP status")
        || message.contains("failed to fetch")
    {
        String::from("HTTP Error")
    } else {
        String::from("Error")
    }
}

fn mention_matches(issue: &Issue, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }

    issue.iid.to_string().contains(query) || issue.title.to_lowercase().contains(query)
}

fn blocker_matches(link: &IssueLink, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }

    link.iid.to_string().contains(query) || link.title.to_lowercase().contains(query)
}

fn has_meaningful_content(value: &str) -> bool {
    !value.trim().is_empty()
}

fn normalized_issue_body(title: &str, body: &str) -> String {
    if has_meaningful_content(body) {
        body.to_string()
    } else {
        title.to_string()
    }
}

fn project_metas_from_store(store: &ConfigStore) -> Vec<ProjectMeta> {
    let mut projects = store
        .stored_projects
        .iter()
        .map(|project| ProjectMeta {
            project_url: project.project_url.clone(),
            gitlab_url: project.gitlab_url.clone(),
            project: project.project.clone(),
            theme: project.theme,
            stored: true,
            private_token: Some(project.private_token.clone()),
        })
        .collect::<Vec<_>>();
    projects.sort_by(|left, right| {
        left.project
            .cmp(&right.project)
            .then(left.project_url.cmp(&right.project_url))
    });
    projects
}

#[cfg(test)]
mod tests {
    use super::{has_meaningful_content, normalized_issue_body};

    #[test]
    fn rejects_empty_or_whitespace_only_issue_bodies() {
        assert!(!has_meaningful_content(""));
        assert!(!has_meaningful_content("   \t   "));
        assert!(!has_meaningful_content("\n\n\t\r\n"));
    }

    #[test]
    fn accepts_issue_bodies_with_actual_content() {
        assert!(has_meaningful_content("hello"));
        assert!(has_meaningful_content("\n  hello\n"));
    }

    #[test]
    fn replaces_empty_issue_body_with_title() {
        assert_eq!(normalized_issue_body("Title", ""), "Title");
        assert_eq!(normalized_issue_body("Title", " \n\t "), "Title");
    }

    #[test]
    fn preserves_non_empty_issue_body() {
        assert_eq!(normalized_issue_body("Title", "Body"), "Body");
    }
}
