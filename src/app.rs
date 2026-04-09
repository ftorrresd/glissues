use std::collections::{BTreeSet, HashMap};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use anyhow::{Result, anyhow};
use chrono::{Datelike, Duration, Local, NaiveDate};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui_themes::Theme;

use crate::config::AppConfig;
use crate::editor::TextBuffer;
use crate::gitlab::{GitLabClient, IssueDraft, IssueUpdate, StateEvent};
use crate::model::{Issue, IssueLink, Note};

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const DEFAULT_STATUS_LABELS: &[&str] = &[
    "status::todo",
    "status::doing",
    "status::blocked",
    "status::done",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    IssueView,
    ConfirmDelete,
    BlockerPicker,
    ThemePicker,
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
    StatusFilter,
    StatusEditor,
}

#[derive(Debug, Clone)]
pub struct Filters {
    pub state: StateFilter,
    pub label: Option<String>,
    pub status: Option<String>,
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

#[derive(Debug, Clone)]
pub struct LabelPickerState {
    pub query: String,
    pub selected: BTreeSet<String>,
    pub cursor: usize,
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

struct RefreshPayload {
    issues: Vec<Issue>,
    labels: Vec<String>,
    notes_cache: HashMap<u64, Vec<Note>>,
    issue_links_cache: HashMap<u64, Vec<IssueLink>>,
}

struct LoadingState {
    message: String,
    spinner_frame: usize,
    selected_iid: Option<u64>,
    receiver: Receiver<Result<RefreshPayload, String>>,
}

enum PendingActionState {
    IssueSave {
        draft: IssueEditorState,
        return_mode: Mode,
        receiver: Receiver<Result<Issue, String>>,
    },
    CommentAdd {
        draft: CommentEditorState,
        return_mode: Mode,
        receiver: Receiver<Result<(u64, Note), String>>,
    },
}

enum PendingActionResult {
    IssueSaved(Issue),
    CommentAdded { iid: u64, note: Note },
}

pub struct App {
    pub config: AppConfig,
    pub theme: Theme,
    client: GitLabClient,
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
    return_mode: Mode,
    pending_g: bool,
    loading: Option<LoadingState>,
    pending_action: Option<PendingActionState>,
}

impl App {
    pub fn new(config: AppConfig, client: GitLabClient) -> Result<Self> {
        let theme_name = config.theme;
        Ok(Self {
            config,
            theme: Theme::new(theme_name),
            client,
            issues: Vec::new(),
            labels: Vec::new(),
            notes_cache: HashMap::new(),
            issue_links_cache: HashMap::new(),
            filters: Filters {
                state: StateFilter::Open,
                label: None,
                status: None,
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
            return_mode: Mode::Normal,
            pending_g: false,
            loading: None,
            pending_action: None,
        })
    }

    pub fn begin_refresh(&mut self, message: &str) {
        if self.loading.is_some() {
            return;
        }

        let client = self.client.clone();
        let selected_iid = self.selected_issue().map(|issue| issue.iid);
        let (sender, receiver) = mpsc::channel();
        let loading_message = message.to_string();

        thread::spawn(move || {
            let result = (|| -> Result<RefreshPayload> {
                let mut issues = client.list_issues()?;
                issues.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
                let labels = client.list_labels()?;
                let mut notes_cache = HashMap::new();
                let mut issue_links_cache = HashMap::new();
                for issue in &issues {
                    let notes = client.list_notes(issue.iid)?;
                    notes_cache.insert(issue.iid, notes);
                    let links = client.list_issue_links(issue.iid)?;
                    issue_links_cache.insert(issue.iid, links);
                }
                Ok(RefreshPayload {
                    issues,
                    labels,
                    notes_cache,
                    issue_links_cache,
                })
            })()
            .map_err(|error| format!("{error:#}"));

            let _ = sender.send(result);
        });

        self.loading = Some(LoadingState {
            message: loading_message.clone(),
            spinner_frame: 0,
            selected_iid,
            receiver,
        });
        self.status_line = loading_message;
    }

    pub fn tick(&mut self) {
        let result = if let Some(loading) = self.loading.as_mut() {
            loading.spinner_frame = (loading.spinner_frame + 1) % SPINNER_FRAMES.len();
            match loading.receiver.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Disconnected) => {
                    Some(Err(String::from("refresh worker disconnected")))
                }
                Err(TryRecvError::Empty) => None,
            }
        } else {
            None
        };

        if let Some(result) = result {
            let loading = self.loading.take();
            match result {
                Ok(payload) => {
                    self.apply_refresh_payload(
                        payload,
                        loading.and_then(|state| state.selected_iid),
                    );
                }
                Err(error) => self.show_error(format!("refresh failed: {error}")),
            }
        }

        let pending_result = match self.pending_action.as_mut() {
            Some(PendingActionState::IssueSave { receiver, .. }) => match receiver.try_recv() {
                Ok(result) => Some(result.map(PendingActionResult::IssueSaved)),
                Err(TryRecvError::Disconnected) => {
                    Some(Err(String::from("issue save worker disconnected")))
                }
                Err(TryRecvError::Empty) => None,
            },
            Some(PendingActionState::CommentAdd { receiver, .. }) => match receiver.try_recv() {
                Ok(result) => {
                    Some(result.map(|(iid, note)| PendingActionResult::CommentAdded { iid, note }))
                }
                Err(TryRecvError::Disconnected) => {
                    Some(Err(String::from("comment worker disconnected")))
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
        self.loading.is_some()
    }

    pub fn loading_message(&self) -> Option<&str> {
        self.loading
            .as_ref()
            .map(|loading| loading.message.as_str())
    }

    pub fn spinner_frame(&self) -> &'static str {
        self.loading
            .as_ref()
            .map(|loading| SPINNER_FRAMES[loading.spinner_frame % SPINNER_FRAMES.len()])
            .unwrap_or(" ")
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

        if self.is_loading() {
            return Ok(());
        }

        match self.mode {
            Mode::Normal => self.handle_normal_mode(key),
            Mode::IssueView => self.handle_issue_view_mode(key),
            Mode::ConfirmDelete => self.handle_confirm_delete_mode(key),
            Mode::BlockerPicker => self.handle_blocker_picker_mode(key),
            Mode::ThemePicker => self.handle_theme_picker_mode(key),
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
            Mode::Normal => "BROWSE",
            Mode::IssueView => "ISSUE",
            Mode::ConfirmDelete => "DELETE",
            Mode::BlockerPicker => "BLOCKERS",
            Mode::ThemePicker => "THEMES",
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

    pub fn issue_status(&self, issue: &Issue) -> Option<String> {
        issue
            .labels
            .iter()
            .find(|label| label.starts_with("status::"))
            .cloned()
    }

    pub fn available_statuses(&self) -> Vec<String> {
        let mut statuses = DEFAULT_STATUS_LABELS
            .iter()
            .map(|label| (*label).to_string())
            .collect::<Vec<_>>();
        for label in &self.labels {
            if label.starts_with("status::") && !statuses.contains(label) {
                statuses.push(label.clone());
            }
        }
        statuses.sort();
        statuses
    }

    pub fn project_label_options(&self) -> Vec<String> {
        let mut labels = self
            .labels
            .iter()
            .filter(|label| !label.starts_with("status::"))
            .cloned()
            .collect::<Vec<_>>();
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
            KeyCode::Char('S') => {
                self.pending_g = false;
                self.open_status_editor();
            }
            KeyCode::Char('d') => {
                self.pending_g = false;
                self.open_due_date_picker();
            }
            KeyCode::Tab | KeyCode::BackTab => {
                self.pending_g = false;
                self.cycle_state_filter()?;
            }
            KeyCode::Char('f') => {
                self.pending_g = false;
                self.cycle_state_filter()?;
            }
            KeyCode::Char('F') | KeyCode::Char('l') => {
                self.pending_g = false;
                self.open_label_filter();
            }
            KeyCode::Char('s') => {
                self.pending_g = false;
                self.open_status_filter();
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
            KeyCode::Char('S') => {
                self.pending_g = false;
                self.open_status_editor();
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
            KeyCode::Char('H') => {
                self.pending_g = false;
                self.move_selected_issue_status(-1)?;
            }
            KeyCode::Char('L') => {
                self.pending_g = false;
                self.move_selected_issue_status(1)?;
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
            KeyCode::Char('y') | KeyCode::Enter => self.confirm_delete_issue()?,
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
                self.config.save_theme(self.theme.name)?;
                self.status_line = format!("theme: {}", self.theme.name.display_name());
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.theme.next();
                self.config.save_theme(self.theme.name)?;
                self.status_line = format!("theme: {}", self.theme.name.display_name());
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
            KeyCode::Backspace => {
                if picker.query.is_empty() {
                    if let Some(last) = picker.selected.iter().last().cloned() {
                        picker.selected.remove(&last);
                    }
                } else {
                    picker.query.pop();
                }
                picker.cursor = 0;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let count = picker.filtered_labels(&self.labels).len().max(1);
                picker.cursor = (picker.cursor + 1).min(count - 1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                picker.cursor = picker.cursor.saturating_sub(1);
            }
            KeyCode::Char(' ') => {
                if let Some(label) = picker.current_choice(&self.labels) {
                    if picker.selected.contains(&label) {
                        picker.selected.remove(&label);
                    } else {
                        picker.selected.insert(label);
                    }
                }
            }
            KeyCode::Char(ch)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                picker.query.push(ch);
                picker.cursor = 0;
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
            KeyCode::Char('x') if selector.allow_clear => {
                selector.selected = None;
                return self.apply_selector();
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
            "status" => self.open_status_editor(),
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
            "filter status clear" => {
                self.filters.status = None;
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

    fn open_issue_view(&mut self) -> Result<()> {
        self.issue_view_scroll = 0;
        self.ensure_selected_notes_loaded()?;
        self.ensure_selected_issue_links_loaded()?;
        self.mode = Mode::IssueView;
        Ok(())
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

        match action {
            BlockerAction::Add => {
                self.client.add_blocker(issue_iid, candidate.iid)?;
                self.refresh_issue_links(issue_iid)?;
                self.status_line = format!("added blocker #{}", candidate.iid);
            }
            BlockerAction::Remove => {
                let link_id = candidate
                    .issue_link_id
                    .ok_or_else(|| anyhow!("missing blocker link id"))?;
                self.client.delete_issue_link(issue_iid, link_id)?;
                self.refresh_issue_links(issue_iid)?;
                self.status_line = format!("removed blocker #{}", candidate.iid);
            }
        }

        self.blocker_picker = None;
        self.restore_return_mode();
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
        let Some(confirm) = self.delete_confirmation.clone() else {
            self.restore_return_mode();
            return Ok(());
        };

        self.client.delete_issue(confirm.iid)?;
        self.delete_confirmation = None;
        self.remove_issue(confirm.iid);
        self.mode = Mode::Normal;
        self.return_mode = Mode::Normal;
        self.status_line = format!("deleted #{}", confirm.iid);
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

    fn move_selected_issue_status(&mut self, direction: isize) -> Result<()> {
        let statuses = self.available_statuses();
        let issue = self
            .selected_issue()
            .cloned()
            .ok_or_else(|| anyhow!("no issue selected"))?;
        let current = self
            .issue_status(&issue)
            .unwrap_or_else(|| self.default_status());
        let current_index = statuses
            .iter()
            .position(|status| status == &current)
            .unwrap_or(0);
        let next_index = (current_index as isize + direction)
            .clamp(0, (statuses.len().saturating_sub(1)) as isize)
            as usize;

        if next_index == current_index {
            return Ok(());
        }

        let next_status = statuses[next_index].clone();
        let mut labels = issue
            .labels
            .iter()
            .filter(|label| !label.starts_with("status::"))
            .cloned()
            .collect::<Vec<_>>();
        labels.push(next_status.clone());

        let updated = self.client.update_issue(
            issue.iid,
            &IssueUpdate {
                labels: Some(labels),
                ..IssueUpdate::default()
            },
        )?;

        self.replace_issue(updated);
        self.status_line = format!("moved #{} to {next_status}", issue.iid);
        Ok(())
    }

    fn default_status(&self) -> String {
        String::from(DEFAULT_STATUS_LABELS[0])
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
        let iid = self
            .selected_issue()
            .map(|issue| issue.iid)
            .ok_or_else(|| anyhow!("no issue selected"))?;
        let issue = self.client.update_issue(
            iid,
            &IssueUpdate {
                state_event: Some(StateEvent::Close),
                ..IssueUpdate::default()
            },
        )?;
        self.replace_issue(issue);
        self.status_line = format!("closed #{iid}");
        Ok(())
    }

    fn reopen_selected_issue(&mut self) -> Result<()> {
        let iid = self
            .selected_issue()
            .map(|issue| issue.iid)
            .ok_or_else(|| anyhow!("no issue selected"))?;
        let issue = self.client.update_issue(
            iid,
            &IssueUpdate {
                state_event: Some(StateEvent::Reopen),
                ..IssueUpdate::default()
            },
        )?;
        self.replace_issue(issue);
        self.status_line = format!("reopened #{iid}");
        Ok(())
    }

    fn ensure_selected_notes_loaded(&mut self) -> Result<()> {
        let Some(iid) = self.selected_issue().map(|issue| issue.iid) else {
            return Ok(());
        };
        if !self.notes_cache.contains_key(&iid) {
            let notes = self.client.list_notes(iid)?;
            self.notes_cache.insert(iid, notes);
        }
        Ok(())
    }

    fn ensure_selected_issue_links_loaded(&mut self) -> Result<()> {
        let Some(iid) = self.selected_issue().map(|issue| issue.iid) else {
            return Ok(());
        };
        if !self.issue_links_cache.contains_key(&iid) {
            let links = self.client.list_issue_links(iid)?;
            self.issue_links_cache.insert(iid, links);
        }
        Ok(())
    }

    fn refresh_issue_links(&mut self, issue_iid: u64) -> Result<()> {
        let links = self.client.list_issue_links(issue_iid)?;
        self.issue_links_cache.insert(issue_iid, links);
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

        let client = self.client.clone();
        let editor_snapshot = editor.clone();
        let return_mode = self.return_mode;
        let (sender, receiver) = mpsc::channel();
        let status_line = match editor.editing_iid {
            Some(iid) => format!("saving #{} in background", iid),
            None => String::from("creating issue in background"),
        };

        thread::spawn(move || {
            let result = (|| -> Result<Issue> {
                match editor_snapshot.editing_iid {
                    Some(iid) => client.update_issue(
                        iid,
                        &IssueUpdate {
                            title: Some(title),
                            description: Some(description),
                            ..IssueUpdate::default()
                        },
                    ),
                    None => client.create_issue(&IssueDraft {
                        title,
                        description,
                        labels: vec![String::from(DEFAULT_STATUS_LABELS[0])],
                        due_date: None,
                    }),
                }
            })()
            .map_err(|error| format!("{error:#}"));

            let _ = sender.send(result);
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

        let client = self.client.clone();
        let return_mode = self.return_mode;
        let (sender, receiver) = mpsc::channel();

        thread::spawn(move || {
            let result = client
                .add_note(iid, &body)
                .map(|note| (iid, note))
                .map_err(|error| format!("{error:#}"));
            let _ = sender.send(result);
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
        let Some(selected) = self
            .selected_issue()
            .map(|issue| issue.labels.iter().cloned().collect::<BTreeSet<_>>())
        else {
            return;
        };

        self.capture_return_mode();
        self.label_picker = Some(LabelPickerState {
            query: String::new(),
            selected,
            cursor: 0,
        });
        self.mode = Mode::LabelEditor;
    }

    fn save_label_picker(&mut self) -> Result<()> {
        let Some(picker) = self.label_picker.take() else {
            return Ok(());
        };
        let iid = self
            .selected_issue()
            .map(|issue| issue.iid)
            .ok_or_else(|| anyhow!("no issue selected"))?;
        let labels = picker.selected.into_iter().collect::<Vec<_>>();

        let issue = self.client.update_issue(
            iid,
            &IssueUpdate {
                labels: Some(labels),
                ..IssueUpdate::default()
            },
        )?;

        self.replace_issue(issue);
        self.restore_return_mode();
        self.status_line = format!("labels updated for #{iid}");
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

    fn open_status_filter(&mut self) {
        self.capture_return_mode();
        self.selector = Some(SelectorState {
            title: String::from("Filter by Status"),
            query: String::new(),
            options: self.available_statuses(),
            selected: self.filters.status.clone(),
            cursor: 0,
            allow_clear: true,
        });
        self.selector_kind = Some(SelectorKind::StatusFilter);
        self.mode = Mode::Selector;
    }

    fn open_status_editor(&mut self) {
        let Some(selected_status) = self.selected_issue().map(|issue| {
            self.issue_status(issue)
                .unwrap_or_else(|| self.default_status())
        }) else {
            return;
        };
        self.capture_return_mode();
        self.selector = Some(SelectorState {
            title: String::from("Set Issue Status"),
            query: String::new(),
            options: self.available_statuses(),
            selected: Some(selected_status),
            cursor: 0,
            allow_clear: true,
        });
        self.selector_kind = Some(SelectorKind::StatusEditor);
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
            SelectorKind::StatusFilter => {
                self.filters.status = choice;
                self.selected = 0;
                self.issue_view_scroll = 0;
                self.clamp_selection();
                self.ensure_selected_notes_loaded()?;
                self.ensure_selected_issue_links_loaded()?;
                self.status_line = match &self.filters.status {
                    Some(status) => format!("status filter: {status}"),
                    None => String::from("status filter cleared"),
                };
            }
            SelectorKind::StatusEditor => {
                let issue = self
                    .selected_issue()
                    .cloned()
                    .ok_or_else(|| anyhow!("no issue selected"))?;
                let mut labels = issue
                    .labels
                    .iter()
                    .filter(|label| !label.starts_with("status::"))
                    .cloned()
                    .collect::<Vec<_>>();
                if let Some(choice) = choice {
                    labels.push(choice.clone());
                }
                let updated = self.client.update_issue(
                    issue.iid,
                    &IssueUpdate {
                        labels: Some(labels),
                        ..IssueUpdate::default()
                    },
                )?;
                self.replace_issue(updated);
                self.status_line = format!("status updated for #{}", issue.iid);
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
        let iid = self
            .selected_issue()
            .map(|issue| issue.iid)
            .ok_or_else(|| anyhow!("no issue selected"))?;

        let issue = self.client.update_issue(
            iid,
            &IssueUpdate {
                due_date: Some(value.map(|date| date.format("%Y-%m-%d").to_string())),
                ..IssueUpdate::default()
            },
        )?;

        self.replace_issue(issue);
        self.due_date_picker = None;
        self.restore_return_mode();
        self.status_line = match value {
            Some(date) => format!("due date set to {}", date.format("%Y-%m-%d")),
            None => format!("due date cleared for #{iid}"),
        };
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

        if let Some(status) = &self.filters.status {
            if self.issue_status(issue).as_deref() != Some(status.as_str()) {
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
        for label in DEFAULT_STATUS_LABELS {
            let label = label.to_string();
            if !self.labels.contains(&label) {
                self.labels.push(label);
            }
        }

        for issue in &self.issues {
            for label in &issue.labels {
                if !self.labels.contains(label) {
                    self.labels.push(label.clone());
                }
            }
        }

        self.labels.sort();
        self.labels.dedup();
    }

    fn replace_issue(&mut self, issue: Issue) {
        let iid = issue.iid;
        if let Some(index) = self.issues.iter().position(|item| item.iid == iid) {
            self.issues[index] = issue;
        } else {
            self.issues.insert(0, issue);
        }

        self.issues
            .sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
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
        self.rebuild_label_catalog();
        self.restore_selection(selected_iid);
        self.status_line = format!(
            "{} issues loaded from {}",
            self.issues.len(),
            self.config.project
        );
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

    pub fn current_choice(&self, labels: &[String]) -> Option<String> {
        let filtered = self.filtered_labels(labels);
        if filtered.is_empty() {
            return (!self.query.trim().is_empty()).then(|| self.query.trim().to_string());
        }
        filtered.get(self.cursor.min(filtered.len() - 1)).cloned()
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
