use std::collections::{BTreeSet, HashMap};

use anyhow::{Result, anyhow};
use chrono::{Datelike, Duration, Local, NaiveDate};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::config::AppConfig;
use crate::editor::{EditMode, TextBuffer};
use crate::gitlab::{GitLabClient, IssueDraft, IssueUpdate, StateEvent};
use crate::model::{Issue, Note};
use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    IssueView,
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
    pub mode: EditMode,
}

#[derive(Debug, Clone)]
pub struct CommentEditorState {
    pub body: TextBuffer,
    pub mode: EditMode,
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

#[derive(Debug)]
pub struct App {
    pub config: AppConfig,
    pub theme: Theme,
    client: GitLabClient,
    pub issues: Vec<Issue>,
    pub labels: Vec<String>,
    pub notes_cache: HashMap<u64, Vec<Note>>,
    pub filters: Filters,
    pub selected: usize,
    pub issue_view_scroll: u16,
    pub issue_view_rect: Rect,
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
    pending_g: bool,
}

impl App {
    pub fn new(config: AppConfig, client: GitLabClient) -> Result<Self> {
        let mut app = Self {
            theme: Theme::from_name(config.theme),
            config,
            client,
            issues: Vec::new(),
            labels: Vec::new(),
            notes_cache: HashMap::new(),
            filters: Filters {
                state: StateFilter::Open,
                label: None,
                status: None,
                search: String::new(),
            },
            selected: 0,
            issue_view_scroll: 0,
            issue_view_rect: Rect::default(),
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
            pending_g: false,
        };

        app.refresh()?;
        Ok(app)
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Result<()> {
        match self.mode {
            Mode::Normal => self.handle_normal_mode(key),
            Mode::IssueView => self.handle_issue_view_mode(key),
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

    pub fn on_mouse(&mut self, mouse: MouseEvent) -> Result<()> {
        match self.mode {
            Mode::Normal => match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    self.pending_g = false;
                }
                MouseEventKind::ScrollDown => {
                    self.pending_g = false;
                    self.move_selection(1)?;
                }
                MouseEventKind::ScrollUp => {
                    self.pending_g = false;
                    self.move_selection(-1)?;
                }
                _ => {}
            },
            Mode::IssueView => match mouse.kind {
                MouseEventKind::ScrollDown
                    if rect_contains(self.issue_view_rect, mouse.column, mouse.row) =>
                {
                    self.scroll_issue_view_by(3);
                }
                MouseEventKind::ScrollUp
                    if rect_contains(self.issue_view_rect, mouse.column, mouse.row) =>
                {
                    self.scroll_issue_view_by(-3);
                }
                _ => {}
            },
            _ => {}
        }

        Ok(())
    }

    pub fn sync_issue_view_layout(
        &mut self,
        issue_view_rect: Rect,
        issue_view_view_height: u16,
        issue_view_content_height: u16,
    ) {
        self.issue_view_rect = issue_view_rect;
        self.issue_view_view_height = issue_view_view_height;
        self.issue_view_content_height = issue_view_content_height;
        self.issue_view_scroll = self.issue_view_scroll.min(self.max_issue_view_scroll());
    }

    pub fn refresh(&mut self) -> Result<()> {
        let selected_iid = self.selected_issue().map(|issue| issue.iid);

        self.issues = self.client.list_issues()?;
        self.issues
            .sort_by(|left, right| right.updated_at.cmp(&left.updated_at));

        self.labels = self.client.list_labels()?;
        self.rebuild_label_catalog();
        self.restore_selection(selected_iid);
        self.ensure_selected_notes_loaded()?;
        self.status_line = format!(
            "{} issues loaded from {}",
            self.issues.len(),
            self.config.project
        );
        Ok(())
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
        let mut statuses = self.config.status_labels.clone();
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
            KeyCode::Char('r') => self.refresh()?,
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
            KeyCode::Char('c') => {
                self.pending_g = false;
                self.open_comment_editor();
            }
            KeyCode::Char('L') => {
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
            KeyCode::Char('l') => {
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
            }
            KeyCode::Char('g') => self.pending_g = true,
            KeyCode::Char('G') => {
                self.selected = self.visible_count().saturating_sub(1);
                self.pending_g = false;
                self.ensure_selected_notes_loaded()?;
            }
            KeyCode::Enter => {
                self.pending_g = false;
                self.open_issue_view();
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

    fn handle_search_mode(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.filters.search = self.search_backup.clone();
                self.search_input = self.search_backup.clone();
                self.mode = Mode::Normal;
                self.selected = 0;
                self.clamp_selection();
                self.ensure_selected_notes_loaded()?;
            }
            KeyCode::Enter => {
                self.mode = Mode::Normal;
                self.selected = 0;
                self.issue_view_scroll = 0;
                self.clamp_selection();
                self.ensure_selected_notes_loaded()?;
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
        let Some(editor) = self.issue_editor.as_mut() else {
            self.mode = Mode::Normal;
            return Ok(());
        };

        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('s')) {
            return self.save_issue_editor();
        }

        match editor.mode {
            EditMode::Normal => match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.issue_editor = None;
                    self.mode = Mode::Normal;
                }
                KeyCode::Char('i') => editor.mode = EditMode::Insert,
                KeyCode::Tab => editor.focus = next_field(editor.focus),
                KeyCode::BackTab => editor.focus = previous_field(editor.focus),
                _ => {
                    let multiline = matches!(editor.focus, EditorField::Body);
                    let buffer = active_issue_buffer(editor);
                    buffer.handle_normal_motion(key, multiline);
                }
            },
            EditMode::Insert => match key.code {
                KeyCode::Esc => editor.mode = EditMode::Normal,
                KeyCode::Tab => editor.focus = next_field(editor.focus),
                KeyCode::Enter if matches!(editor.focus, EditorField::Title) => {
                    editor.focus = EditorField::Body;
                }
                _ => {
                    let multiline = matches!(editor.focus, EditorField::Body);
                    let buffer = active_issue_buffer(editor);
                    buffer.handle_insert_key(key, multiline);
                }
            },
        }

        Ok(())
    }

    fn handle_comment_editor_mode(&mut self, key: KeyEvent) -> Result<()> {
        let Some(editor) = self.comment_editor.as_mut() else {
            self.mode = Mode::Normal;
            return Ok(());
        };

        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('s')) {
            return self.save_comment_editor();
        }

        match editor.mode {
            EditMode::Normal => match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.comment_editor = None;
                    self.mode = Mode::Normal;
                }
                KeyCode::Char('i') => editor.mode = EditMode::Insert,
                _ => {
                    editor.body.handle_normal_motion(key, true);
                }
            },
            EditMode::Insert => match key.code {
                KeyCode::Esc => editor.mode = EditMode::Normal,
                _ => {
                    editor.body.handle_insert_key(key, true);
                }
            },
        }

        Ok(())
    }

    fn handle_label_editor_mode(&mut self, key: KeyEvent) -> Result<()> {
        let Some(picker) = self.label_picker.as_mut() else {
            self.mode = Mode::Normal;
            return Ok(());
        };

        match key.code {
            KeyCode::Esc => {
                self.label_picker = None;
                self.mode = Mode::Normal;
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
            self.mode = Mode::Normal;
            return Ok(());
        };

        match key.code {
            KeyCode::Esc => {
                self.selector = None;
                self.selector_kind = None;
                self.mode = Mode::Normal;
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
            self.mode = Mode::Normal;
            return Ok(());
        };

        match key.code {
            KeyCode::Esc => {
                self.due_date_picker = None;
                self.mode = Mode::Normal;
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
            "refresh" | "r" => self.refresh()?,
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
            }
            "filter status clear" => {
                self.filters.status = None;
                self.selected = 0;
                self.clamp_selection();
                self.ensure_selected_notes_loaded()?;
            }
            _ => {
                self.status_line = format!("unknown command: {command}");
            }
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
        }
        Ok(())
    }

    fn open_issue_view(&mut self) {
        self.issue_view_scroll = 0;
        self.mode = Mode::IssueView;
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

    fn set_state_filter(&mut self, state: StateFilter) -> Result<()> {
        self.filters.state = state;
        self.selected = 0;
        self.issue_view_scroll = 0;
        self.clamp_selection();
        self.ensure_selected_notes_loaded()?;
        self.status_line = format!("state filter: {}", self.state_label());
        Ok(())
    }

    fn open_issue_editor(&mut self, issue: Option<Issue>) {
        self.issue_editor = Some(match issue {
            Some(issue) => IssueEditorState {
                editing_iid: Some(issue.iid),
                title: TextBuffer::from_text(&issue.title),
                body: TextBuffer::from_text(&issue.description),
                focus: EditorField::Title,
                mode: EditMode::Normal,
            },
            None => IssueEditorState {
                editing_iid: None,
                title: TextBuffer::new(),
                body: TextBuffer::new(),
                focus: EditorField::Title,
                mode: EditMode::Insert,
            },
        });
        self.mode = Mode::IssueEditor;
    }

    fn save_issue_editor(&mut self) -> Result<()> {
        let Some(editor) = self.issue_editor.take() else {
            return Ok(());
        };

        let title = editor.title.to_text().trim().to_string();
        if title.is_empty() {
            self.issue_editor = Some(editor);
            self.status_line = String::from("title cannot be empty");
            return Ok(());
        }

        let description = editor.body.to_text();
        let issue = match editor.editing_iid {
            Some(iid) => self.client.update_issue(
                iid,
                &IssueUpdate {
                    title: Some(title),
                    description: Some(description),
                    ..IssueUpdate::default()
                },
            )?,
            None => self.client.create_issue(&IssueDraft {
                title,
                description,
                labels: Vec::new(),
                due_date: None,
            })?,
        };

        let iid = issue.iid;
        self.replace_issue(issue);
        self.mode = Mode::Normal;
        self.status_line = format!("saved #{iid}");
        Ok(())
    }

    fn open_comment_editor(&mut self) {
        self.comment_editor = Some(CommentEditorState {
            body: TextBuffer::new(),
            mode: EditMode::Insert,
        });
        self.mode = Mode::CommentEditor;
    }

    fn save_comment_editor(&mut self) -> Result<()> {
        let Some(editor) = self.comment_editor.take() else {
            return Ok(());
        };
        let iid = self
            .selected_issue()
            .map(|issue| issue.iid)
            .ok_or_else(|| anyhow!("no issue selected"))?;

        let body = editor.body.to_text().trim().to_string();
        if body.is_empty() {
            self.comment_editor = Some(editor);
            self.status_line = String::from("comment cannot be empty");
            return Ok(());
        }

        let note = self.client.add_note(iid, &body)?;
        self.notes_cache.entry(iid).or_default().push(note);
        self.mode = Mode::Normal;
        self.status_line = format!("comment added to #{iid}");
        Ok(())
    }

    fn open_label_editor(&mut self) {
        let Some(issue) = self.selected_issue() else {
            return;
        };

        self.label_picker = Some(LabelPickerState {
            query: String::new(),
            selected: issue.labels.iter().cloned().collect(),
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
        self.mode = Mode::Normal;
        self.status_line = format!("labels updated for #{iid}");
        Ok(())
    }

    fn open_label_filter(&mut self) {
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
        let Some(issue) = self.selected_issue() else {
            return;
        };
        self.selector = Some(SelectorState {
            title: String::from("Set Issue Status"),
            query: String::new(),
            options: self.available_statuses(),
            selected: self.issue_status(issue),
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
            self.mode = Mode::Normal;
            return Ok(());
        };
        let Some(selector) = selector else {
            self.mode = Mode::Normal;
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

        self.mode = Mode::Normal;
        Ok(())
    }

    fn open_due_date_picker(&mut self) {
        let selected = self
            .selected_issue()
            .and_then(|issue| issue.due_date.as_deref())
            .and_then(parse_due_date)
            .unwrap_or_else(|| Local::now().date_naive());

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
        self.mode = Mode::Normal;
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
        for label in &self.config.status_labels {
            if !self.labels.contains(label) {
                self.labels.push(label.clone());
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

        let visible = self.visible_issue_indices();
        if let Some(position) = visible
            .iter()
            .position(|index| self.issues[*index].iid == iid)
        {
            self.selected = position;
        } else {
            self.clamp_selection();
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

fn rect_contains(rect: Rect, column: u16, row: u16) -> bool {
    column >= rect.x
        && column < rect.x.saturating_add(rect.width)
        && row >= rect.y
        && row < rect.y.saturating_add(rect.height)
}
