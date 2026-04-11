use chrono::{Datelike, Duration, Local, NaiveDate};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui_themes::{ThemePalette, ThemePicker};

use crate::app::{App, DueDatePickerState, EditorField, Mode, format_timestamp, parse_due_date};
use crate::editor::TextBuffer;
use crate::markdown::render_markdown;

#[derive(Clone, Copy)]
struct Colors {
    bg: Color,
    panel: Color,
    panel_alt: Color,
    text: Color,
    muted: Color,
    accent: Color,
    accent_alt: Color,
    warn: Color,
    danger: Color,
    iris: Color,
    rose: Color,
    info: Color,
}

fn colors(palette: ThemePalette) -> Colors {
    Colors {
        bg: palette.bg,
        panel: palette.bg,
        panel_alt: palette.selection,
        text: palette.fg,
        muted: palette.muted,
        accent: palette.accent,
        accent_alt: palette.success,
        warn: palette.warning,
        danger: palette.error,
        iris: palette.secondary,
        rose: palette.secondary,
        info: palette.info,
    }
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(10),
            Constraint::Length(2),
        ])
        .split(area);

    draw_header(frame, layout[0], app);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(24),
            Constraint::Percentage(36),
            Constraint::Percentage(64),
        ])
        .split(layout[1]);

    draw_sidebar(frame, body[0], app);
    draw_issue_list(frame, body[1], app);
    draw_preview(frame, body[2], app);
    draw_footer(frame, layout[2], app);

    match app.mode {
        Mode::Help => draw_help(frame, area, app),
        Mode::IssueView => draw_issue_view(frame, area, app),
        Mode::ConfirmDelete => draw_confirm_delete(frame, area, app),
        Mode::IssueEditor => draw_issue_editor(frame, area, app),
        Mode::CommentEditor => draw_comment_editor(frame, area, app),
        Mode::LabelEditor => draw_label_editor(frame, area, app),
        Mode::BlockerPicker => {}
        Mode::ThemePicker => {}
        Mode::ProjectPicker => {}
        Mode::StoreProjectPrompt => {}
        Mode::Selector => draw_selector(frame, area, app),
        Mode::DueDatePicker => draw_due_date_picker(frame, area, app),
        Mode::Search | Mode::Command | Mode::Normal => {}
    }

    if app.has_mention_picker() {
        draw_mention_picker(frame, area, app);
    }

    if app.has_blocker_picker() {
        draw_blocker_picker(frame, area, app);
    }

    if matches!(app.mode, Mode::ThemePicker) {
        draw_theme_picker(frame, area, app);
    }

    if app.is_loading() {
        draw_loading(frame, area, app);
    }

    if app.has_project_picker() {
        draw_project_picker(frame, area, app);
    }

    if app.has_store_project_prompt() {
        draw_store_project_prompt(frame, area, app);
    }

    if app.has_alert() {
        draw_alert(frame, area, app);
    }
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let c = colors(app.theme.palette());
    let status = format!(
        " {}  {}  projects:{}  theme:{}  open:{}  closed:{}  overdue:{}  state:{}  label:{}  search:{} ",
        app.mode_label(),
        app.config.project,
        app.projects.len(),
        app.theme.name.display_name(),
        app.count_open(),
        app.count_closed(),
        app.count_overdue(),
        app.state_label(),
        app.filters.label.as_deref().unwrap_or("any"),
        if app.filters.search.is_empty() {
            "off"
        } else {
            app.filters.search.as_str()
        },
    );

    frame.render_widget(
        Paragraph::new(status).style(
            Style::default()
                .bg(c.panel_alt)
                .fg(c.text)
                .add_modifier(Modifier::BOLD),
        ),
        area,
    );
}

fn draw_sidebar(frame: &mut Frame, area: Rect, app: &App) {
    let c = colors(app.theme.palette());
    let today = Local::now().date_naive();
    let selected_issue = app.selected_issue().map(|issue| issue.iid);

    let lines = vec![
        Line::from(vec![Span::styled(
            "Views",
            Style::default().fg(c.accent).add_modifier(Modifier::BOLD),
        )]),
        sidebar_line(c, "All", app.issues.len(), app.state_label() == "all"),
        sidebar_line(c, "Open", app.count_open(), app.state_label() == "open"),
        sidebar_line(
            c,
            "Closed",
            app.count_closed(),
            app.state_label() == "closed",
        ),
        sidebar_line(c, "Overdue", app.count_overdue(), false),
        Line::default(),
        Line::from(vec![Span::styled(
            "Scope",
            Style::default().fg(c.accent).add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled("Label  ", Style::default().fg(c.muted)),
            Span::styled(
                app.filters.label.as_deref().unwrap_or("any"),
                Style::default().fg(c.text),
            ),
        ]),
        Line::from(vec![
            Span::styled("Search ", Style::default().fg(c.muted)),
            Span::styled(
                if app.filters.search.is_empty() {
                    "off"
                } else {
                    app.filters.search.as_str()
                },
                Style::default().fg(c.text),
            ),
        ]),
        Line::default(),
        Line::from(vec![Span::styled(
            "Agenda",
            Style::default().fg(c.accent).add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled("Today  ", Style::default().fg(c.muted)),
            Span::styled(
                app.issues
                    .iter()
                    .filter(|issue| {
                        issue.due_date.as_deref().and_then(parse_due_date) == Some(today)
                    })
                    .count()
                    .to_string(),
                Style::default().fg(c.text),
            ),
        ]),
        Line::from(vec![
            Span::styled("Picked ", Style::default().fg(c.muted)),
            Span::styled(
                selected_issue
                    .map(|iid| format!("#{iid}"))
                    .unwrap_or_else(|| String::from("none")),
                Style::default().fg(c.text),
            ),
        ]),
    ];

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .style(Style::default().bg(c.panel).fg(c.text))
            .block(styled_block(c, "glissues")),
        area,
    );
}

fn draw_issue_list(frame: &mut Frame, area: Rect, app: &App) {
    let c = colors(app.theme.palette());
    let visible = app.visible_issue_indices();
    let mut items = Vec::new();

    for index in visible {
        let issue = &app.issues[index];
        let state_marker = if issue.state == "opened" {
            "●"
        } else {
            "○"
        };
        let state_style = if issue.state == "opened" {
            Style::default().fg(c.accent_alt)
        } else {
            Style::default().fg(c.muted)
        };

        let due = issue
            .due_date
            .clone()
            .unwrap_or_else(|| String::from("no due"));
        let labels = issue
            .labels
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");

        let meta = if labels.is_empty() {
            format!("{}  {} comments", due, issue.user_notes_count)
        } else {
            format!("{}  {}  {} comments", due, labels, issue.user_notes_count)
        };

        items.push(ListItem::new(vec![
            Line::from(vec![
                Span::styled(format!("{state_marker} "), state_style),
                Span::styled(
                    format!("#{} {}", issue.iid, issue.title),
                    Style::default().fg(c.text).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(Span::styled(meta, Style::default().fg(c.muted))),
        ]));
    }

    if items.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            "No issues match the current filters.",
            Style::default().fg(c.muted),
        ))));
    }

    let list = List::new(items)
        .block(pane_block(c, "Issues", true))
        .highlight_style(
            Style::default()
                .bg(c.panel_alt)
                .fg(c.text)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▎");

    let mut state = ListState::default();
    if app.visible_count() > 0 {
        state.select(Some(app.selected.min(app.visible_count() - 1)));
    }
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_preview(frame: &mut Frame, area: Rect, app: &App) {
    let c = colors(app.theme.palette());
    frame.render_widget(
        Paragraph::new(issue_text(app, false))
            .block(styled_block(c, "Preview"))
            .style(Style::default().bg(c.panel).fg(c.text))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    let c = colors(app.theme.palette());
    let prompt = match app.mode {
        Mode::Search => format!("/{}", app.search_input),
        Mode::Command => format!(":{}", app.command_input),
        _ => app.status_line.clone(),
    };

    let hints = match app.mode {
        Mode::Normal => {
            " j/k move  Tab state filter  Enter details  p projects  P cycle  b add blocker  t themes  n new  Ctrl-r refresh "
        }
        Mode::IssueView => {
            " Esc close  e edit  c comment  a labels  b add blocker  B remove blocker  p projects  P cycle  t themes "
        }
        Mode::ConfirmDelete => " y confirm delete  n or Esc cancel ",
        Mode::BlockerPicker => " type to search  Enter apply  Esc cancel ",
        Mode::ThemePicker => " h/Left prev  l/Right next  Enter or Esc close ",
        Mode::ProjectPicker => " type to filter  Enter open  Esc cancel ",
        Mode::StoreProjectPrompt => " y store project  n skip storing ",
        Mode::Search => " Enter apply  Esc cancel ",
        Mode::Command => " Enter run command  Esc cancel ",
        _ => " Esc close overlay  Ctrl-s save while editing ",
    };

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(area);

    frame.render_widget(
        Paragraph::new(prompt).style(Style::default().bg(c.panel_alt).fg(c.text)),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(hints).style(Style::default().bg(c.bg).fg(c.muted)),
        rows[1],
    );
}

fn draw_help(frame: &mut Frame, area: Rect, app: &App) {
    let c = colors(app.theme.palette());
    let popup = centered_rect(76, 68, area);
    let text = Text::from(vec![
        Line::from(Span::styled(
            "glissues keymap",
            Style::default().fg(c.accent).add_modifier(Modifier::BOLD),
        )),
        Line::default(),
        Line::from("j / k          move through the issue list"),
        Line::from("gg / G         jump to top or bottom"),
        Line::from("Enter          open the selected issue popup"),
        Line::from("Esc            close the issue popup or overlays"),
        Line::from("Tab            cycle all/open/closed filters"),
        Line::from("Ctrl-r         refresh from GitLab with spinner"),
        Line::from("p              open stored project picker"),
        Line::from("P              cycle to the next project"),
        Line::from("[ / ]          cycle previous or next project"),
        Line::from("t              open the theme selector"),
        Line::from("F or l         filter by label"),
        Line::from("/              fuzzy-like text filter"),
        Line::from("n              create a new issue"),
        Line::from("e              edit title and body"),
        Line::from("c              add a comment"),
        Line::from("a              edit labels"),
        Line::from("b / B          add or remove blockers"),
        Line::from("d              open the due date picker"),
        Line::from("x              close or reopen the selected issue"),
        Line::from("D              delete the selected issue after confirmation"),
        Line::from(
            "Inside popup   e edit, c comment, a labels, b/B blockers, d due, x close/reopen, D delete",
        ),
        Line::from(
            "Inside editors type # to mention an issue, Enter to insert #iid, or Esc to skip",
        ),
        Line::from(":              run commands like :refresh or :filter open"),
        Line::from("Ctrl-c         quit instantly"),
        Line::default(),
        Line::from(Span::styled(
            "Editors",
            Style::default().fg(c.accent).add_modifier(Modifier::BOLD),
        )),
        Line::from("Typing         always inserts text"),
        Line::from("Esc            close the editor or comment popup"),
        Line::from("Tab            switch between title/body fields"),
        Line::from("Ctrl-s         save changes"),
    ]);

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(text)
            .block(styled_block(c, "Help"))
            .style(Style::default().bg(c.panel).fg(c.text))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn draw_issue_view(frame: &mut Frame, area: Rect, app: &mut App) {
    let c = colors(app.theme.palette());
    let popup = centered_rect(86, 88, area);
    let content = issue_text(app, true);
    let inner = Block::default().borders(Borders::ALL).inner(popup);
    let content_height = wrapped_text_height(&content, inner.width);
    app.sync_issue_view_layout(inner.height, content_height);

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(content)
            .block(pane_block(c, "Issue", true))
            .style(Style::default().bg(c.panel).fg(c.text))
            .wrap(Wrap { trim: false })
            .scroll((app.issue_view_scroll, 0)),
        popup,
    );
}

fn draw_confirm_delete(frame: &mut Frame, area: Rect, app: &App) {
    let c = colors(app.theme.palette());
    let Some(confirm) = app.delete_confirmation.as_ref() else {
        return;
    };

    let popup = centered_rect(56, 28, area);
    let text = Text::from(vec![
        Line::from(Span::styled(
            "Delete issue?",
            Style::default().fg(c.danger).add_modifier(Modifier::BOLD),
        )),
        Line::default(),
        Line::from(vec![
            Span::styled(format!("#{} ", confirm.iid), Style::default().fg(c.accent)),
            Span::styled(confirm.title.clone(), Style::default().fg(c.text)),
        ]),
        Line::default(),
        Line::from(Span::styled(
            "This permanently removes the issue from GitLab.",
            Style::default().fg(c.warn),
        )),
        Line::default(),
        Line::from(Span::styled(
            "Press y to delete, n or Esc to cancel.",
            Style::default().fg(c.muted),
        )),
    ]);

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(text)
            .block(pane_block(c, "Confirm Delete", true))
            .style(Style::default().bg(c.panel).fg(c.text))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn draw_issue_editor(frame: &mut Frame, area: Rect, app: &App) {
    let c = colors(app.theme.palette());
    let Some(editor) = app.issue_editor.as_ref() else {
        return;
    };

    let popup = centered_rect(78, 78, area);
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(1),
        ])
        .margin(1)
        .split(popup);

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(if editor.editing_iid.is_some() {
                "Edit Issue"
            } else {
                "New Issue"
            })
            .style(Style::default().bg(c.panel).fg(c.text)),
        popup,
    );

    let title_style = if matches!(editor.focus, EditorField::Title) {
        Style::default().fg(c.text).bg(c.panel_alt)
    } else {
        Style::default().fg(c.text).bg(c.panel)
    };
    let body_style = if matches!(editor.focus, EditorField::Body) {
        Style::default().fg(c.text).bg(c.panel_alt)
    } else {
        Style::default().fg(c.text).bg(c.panel)
    };

    frame.render_widget(
        Paragraph::new(editor.title.to_text())
            .block(styled_block(c, "Title"))
            .style(title_style),
        sections[0],
    );
    frame.render_widget(
        Paragraph::new(editor.body.to_text())
            .block(styled_block(c, "Body (Markdown)"))
            .style(body_style)
            .wrap(Wrap { trim: false })
            .scroll((editor_scroll(&editor.body, sections[1]), 0)),
        sections[1],
    );
    frame.render_widget(
        Paragraph::new("Esc close draft  Tab next field  Ctrl-s save  # mention issue")
            .style(Style::default().fg(c.muted).bg(c.panel)),
        sections[2],
    );

    match editor.focus {
        EditorField::Title => {
            let inner = Block::default().borders(Borders::ALL).inner(sections[0]);
            frame.set_cursor_position((
                inner.x + editor.title.col() as u16,
                inner.y + editor.title.row() as u16,
            ));
        }
        EditorField::Body => {
            let inner = Block::default().borders(Borders::ALL).inner(sections[1]);
            let (_, cursor_x, cursor_y) = editor_viewport(&editor.body, inner.width, inner.height);
            frame.set_cursor_position((inner.x + cursor_x, inner.y + cursor_y));
        }
    }
}

fn draw_comment_editor(frame: &mut Frame, area: Rect, app: &App) {
    let c = colors(app.theme.palette());
    let Some(editor) = app.comment_editor.as_ref() else {
        return;
    };

    let popup = centered_rect(70, 54, area);
    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(8), Constraint::Length(1)])
        .margin(1)
        .split(popup);

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title("New Comment")
            .style(Style::default().bg(c.panel).fg(c.text)),
        popup,
    );
    frame.render_widget(
        Paragraph::new(editor.body.to_text())
            .block(styled_block(c, "Body"))
            .style(Style::default().bg(c.panel_alt).fg(c.text))
            .wrap(Wrap { trim: false })
            .scroll((editor_scroll(&editor.body, inner[0]), 0)),
        inner[0],
    );
    frame.render_widget(
        Paragraph::new("Esc close draft  Ctrl-s save  # mention issue")
            .style(Style::default().fg(c.muted).bg(c.panel)),
        inner[1],
    );

    let cursor = Block::default().borders(Borders::ALL).inner(inner[0]);
    let (_, cursor_x, cursor_y) = editor_viewport(&editor.body, cursor.width, cursor.height);
    frame.set_cursor_position((cursor.x + cursor_x, cursor.y + cursor_y));
}

fn draw_label_editor(frame: &mut Frame, area: Rect, app: &App) {
    let c = colors(app.theme.palette());
    let Some(picker) = app.label_picker.as_ref() else {
        return;
    };
    let popup = centered_rect(62, 64, area);
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(4),
            Constraint::Min(8),
            Constraint::Length(1),
        ])
        .margin(1)
        .split(popup);

    let filtered = picker.filtered_labels(&app.labels);
    let mut items = Vec::new();
    for label in &filtered {
        let mark = if picker.selected.contains(label) {
            "[x]"
        } else {
            "[ ]"
        };
        items.push(ListItem::new(Line::from(format!("{mark} {label}"))));
    }
    if items.is_empty() {
        items.push(ListItem::new(Line::from(
            "No labels yet. Type to create one.",
        )));
    }

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title("Edit Labels")
            .style(Style::default().bg(c.panel).fg(c.text)),
        popup,
    );
    frame.render_widget(
        Paragraph::new(picker.query.as_str())
            .block(styled_block(c, "Search or Create"))
            .style(Style::default().bg(c.panel_alt).fg(c.text)),
        sections[0],
    );
    frame.render_widget(
        Paragraph::new(
            picker
                .selected
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join("  "),
        )
        .block(styled_block(c, "Selected"))
        .style(Style::default().bg(c.panel).fg(c.text))
        .wrap(Wrap { trim: false }),
        sections[1],
    );
    let list = List::new(items)
        .block(styled_block(c, "Autocomplete"))
        .highlight_symbol("▎")
        .highlight_style(Style::default().bg(c.panel_alt).fg(c.text));
    let mut state = ListState::default();
    if !filtered.is_empty() {
        state.select(Some(picker.cursor.min(filtered.len() - 1)));
    }
    frame.render_stateful_widget(list, sections[2], &mut state);
    frame.render_widget(
        Paragraph::new("type to filter  Space toggle  Enter save  Esc cancel")
            .style(Style::default().fg(c.muted).bg(c.panel)),
        sections[3],
    );
}

fn draw_selector(frame: &mut Frame, area: Rect, app: &App) {
    let c = colors(app.theme.palette());
    let Some(selector) = app.selector.as_ref() else {
        return;
    };
    let popup = centered_rect(54, 56, area);
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(1),
        ])
        .margin(1)
        .split(popup);

    let filtered = selector.filtered_options();
    let mut items = Vec::new();
    for option in &filtered {
        let marker = if selector.selected.as_deref() == Some(option.as_str()) {
            "◉"
        } else {
            "○"
        };
        items.push(ListItem::new(Line::from(format!("{marker} {option}"))));
    }
    if items.is_empty() {
        items.push(ListItem::new(Line::from("No matches.")));
    }

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(selector.title.as_str())
            .style(Style::default().bg(c.panel).fg(c.text)),
        popup,
    );
    frame.render_widget(
        Paragraph::new(selector.query.as_str())
            .block(styled_block(c, "Filter"))
            .style(Style::default().bg(c.panel_alt).fg(c.text)),
        sections[0],
    );
    let list = List::new(items)
        .block(styled_block(c, "Options"))
        .highlight_symbol("▎")
        .highlight_style(Style::default().bg(c.panel_alt).fg(c.text));
    let mut state = ListState::default();
    if !filtered.is_empty() {
        state.select(Some(selector.cursor.min(filtered.len() - 1)));
    }
    frame.render_stateful_widget(list, sections[1], &mut state);
    frame.render_widget(
        Paragraph::new(if selector.allow_clear {
            "type to filter  Enter apply  x clear  Esc cancel"
        } else {
            "type to filter  Enter apply  Esc cancel"
        })
        .style(Style::default().fg(c.muted).bg(c.panel)),
        sections[2],
    );
}

fn draw_mention_picker(frame: &mut Frame, area: Rect, app: &App) {
    let c = colors(app.theme.palette());
    let Some(picker) = app.mention_picker.as_ref() else {
        return;
    };

    let popup = centered_rect(60, 52, area);
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(1),
        ])
        .margin(1)
        .split(popup);

    let candidates = app.mention_candidates();
    let mut items = Vec::new();
    for index in &candidates {
        if let Some(issue) = app.issues.get(*index) {
            items.push(ListItem::new(vec![
                Line::from(vec![
                    Span::styled(format!("#{} ", issue.iid), Style::default().fg(c.accent)),
                    Span::styled(issue.title.clone(), Style::default().fg(c.text)),
                ]),
                Line::from(Span::styled(
                    issue.state.clone(),
                    Style::default().fg(c.muted),
                )),
            ]));
        }
    }

    if items.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            "No matching issues.",
            Style::default().fg(c.muted),
        ))));
    }

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title("Mention Issue")
            .style(Style::default().bg(c.panel).fg(c.text)),
        popup,
    );
    frame.render_widget(
        Paragraph::new(format!("#{}", picker.query))
            .block(styled_block(c, "Query"))
            .style(Style::default().bg(c.panel_alt).fg(c.text)),
        sections[0],
    );

    let list = List::new(items)
        .block(styled_block(c, "Matches"))
        .highlight_symbol("▎")
        .highlight_style(
            Style::default()
                .bg(c.panel_alt)
                .fg(c.text)
                .add_modifier(Modifier::BOLD),
        );
    let mut state = ListState::default();
    if !candidates.is_empty() {
        state.select(Some(picker.cursor.min(candidates.len() - 1)));
    }
    frame.render_stateful_widget(list, sections[1], &mut state);
    frame.render_widget(
        Paragraph::new("type to search  Enter choose  Esc keep # and continue")
            .style(Style::default().fg(c.muted).bg(c.panel)),
        sections[2],
    );
}

fn draw_blocker_picker(frame: &mut Frame, area: Rect, app: &App) {
    let c = colors(app.theme.palette());
    let Some(picker) = app.blocker_picker.as_ref() else {
        return;
    };

    let popup = centered_rect(62, 56, area);
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(1),
        ])
        .margin(1)
        .split(popup);

    let candidates = app.blocker_candidates();
    let mut items = Vec::new();
    for candidate in &candidates {
        items.push(ListItem::new(vec![
            Line::from(vec![
                Span::styled(
                    format!("#{} ", candidate.iid),
                    Style::default().fg(c.accent),
                ),
                Span::styled(candidate.title.clone(), Style::default().fg(c.text)),
            ]),
            Line::from(Span::styled(
                candidate.state.clone(),
                Style::default().fg(c.muted),
            )),
        ]));
    }

    if items.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            if picker.action == crate::app::BlockerAction::Add {
                "No matching issues to add as blockers."
            } else {
                "No blockers to remove."
            },
            Style::default().fg(c.muted),
        ))));
    }

    let title = if picker.action == crate::app::BlockerAction::Add {
        "Add Blocker"
    } else {
        "Remove Blocker"
    };

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .style(Style::default().bg(c.panel).fg(c.text)),
        popup,
    );
    frame.render_widget(
        Paragraph::new(picker.query.as_str())
            .block(styled_block(c, "Search"))
            .style(Style::default().bg(c.panel_alt).fg(c.text)),
        sections[0],
    );

    let list = List::new(items)
        .block(styled_block(c, "Matches"))
        .highlight_symbol("▎")
        .highlight_style(
            Style::default()
                .bg(c.panel_alt)
                .fg(c.text)
                .add_modifier(Modifier::BOLD),
        );
    let mut state = ListState::default();
    if !candidates.is_empty() {
        state.select(Some(picker.cursor.min(candidates.len() - 1)));
    }
    frame.render_stateful_widget(list, sections[1], &mut state);
    frame.render_widget(
        Paragraph::new("type to search  Enter apply  Esc cancel")
            .style(Style::default().fg(c.muted).bg(c.panel)),
        sections[2],
    );
}

fn draw_theme_picker(frame: &mut Frame, area: Rect, app: &App) {
    let popup = centered_rect(56, 38, area);
    let picker = ThemePicker::new(app.theme.name)
        .title("Theme Selector")
        .instructions("Prev <h/Left> Next <l/Right> Close <Esc>");

    frame.render_widget(Clear, popup);
    frame.render_widget(picker, popup);
}

fn draw_project_picker(frame: &mut Frame, area: Rect, app: &App) {
    let c = colors(app.theme.palette());
    let Some(picker) = app.project_picker.as_ref() else {
        return;
    };

    let popup = centered_rect(74, 64, area);
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(1),
        ])
        .margin(1)
        .split(popup);

    let candidates = app.project_picker_candidates();
    let items = if candidates.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "No projects match the current filter.",
            Style::default().fg(c.muted),
        )))]
    } else {
        candidates
            .iter()
            .map(|project| {
                let (state, color) = if project.project_url == app.current_project_url {
                    (String::from("active"), c.muted)
                } else if let Some(state) = app.project_load_label(&project.project_url) {
                    (state, c.warn)
                } else if app.is_project_loaded(&project.project_url) {
                    (String::from("loaded"), c.muted)
                } else if project.stored {
                    (String::from("stored"), c.muted)
                } else {
                    (String::from("session"), c.muted)
                };
                ListItem::new(vec![
                    Line::from(vec![
                        Span::styled(project.project.clone(), Style::default().fg(c.accent)),
                        Span::raw("  "),
                        Span::styled(state, Style::default().fg(color)),
                    ]),
                    Line::from(Span::styled(
                        project.project_url.clone(),
                        Style::default().fg(c.text),
                    )),
                ])
            })
            .collect()
    };

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title("Projects")
            .style(Style::default().bg(c.panel).fg(c.text)),
        popup,
    );
    frame.render_widget(
        Paragraph::new(picker.query.as_str())
            .block(styled_block(c, "Filter"))
            .style(Style::default().bg(c.panel_alt).fg(c.text)),
        sections[0],
    );

    let list = List::new(items)
        .block(styled_block(c, "Available Projects"))
        .highlight_symbol("▎")
        .highlight_style(
            Style::default()
                .bg(c.panel_alt)
                .fg(c.text)
                .add_modifier(Modifier::BOLD),
        );
    let mut state = ListState::default();
    if !candidates.is_empty() {
        state.select(Some(picker.cursor.min(candidates.len() - 1)));
    }
    frame.render_stateful_widget(list, sections[1], &mut state);
    frame.render_widget(
        Paragraph::new("type to filter  Enter open  Esc cancel")
            .style(Style::default().fg(c.muted).bg(c.panel)),
        sections[2],
    );
}

fn draw_store_project_prompt(frame: &mut Frame, area: Rect, app: &App) {
    let c = colors(app.theme.palette());
    let Some(prompt) = app.store_project_prompt.as_ref() else {
        return;
    };
    let popup = centered_rect(58, 28, area);
    let text = Text::from(vec![
        Line::from(Span::styled(
            "Store this project for later?",
            Style::default().fg(c.accent).add_modifier(Modifier::BOLD),
        )),
        Line::default(),
        Line::from(Span::styled(
            prompt.project_url.clone(),
            Style::default().fg(c.text),
        )),
        Line::default(),
        Line::from(Span::styled(
            "The GitLab private token will be saved in plain text in the config file.",
            Style::default().fg(c.muted),
        )),
        Line::default(),
        Line::from(Span::styled(
            "Press y to store it or n to keep it only for this session.",
            Style::default().fg(c.warn),
        )),
    ]);

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(text)
            .block(pane_block(c, "Store Project", true))
            .style(Style::default().bg(c.panel).fg(c.text))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn draw_due_date_picker(frame: &mut Frame, area: Rect, app: &App) {
    let c = colors(app.theme.palette());
    let Some(picker) = app.due_date_picker.as_ref() else {
        return;
    };
    let popup = centered_rect(46, 50, area);
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(8),
            Constraint::Length(2),
            Constraint::Length(1),
        ])
        .margin(1)
        .split(popup);

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title("Due Date")
            .style(Style::default().bg(c.panel).fg(c.text)),
        popup,
    );

    frame.render_widget(
        Paragraph::new(format!(
            "{} {}",
            month_name(picker.month.month()),
            picker.month.year()
        ))
        .style(
            Style::default()
                .fg(c.accent)
                .bg(c.panel)
                .add_modifier(Modifier::BOLD),
        ),
        sections[0],
    );

    frame.render_widget(
        Paragraph::new(calendar_text(picker, c))
            .style(Style::default().fg(c.text).bg(c.panel))
            .wrap(Wrap { trim: false }),
        sections[1],
    );

    frame.render_widget(
        Paragraph::new(format!("Selected: {}", picker.selected.format("%Y-%m-%d")))
            .style(Style::default().fg(c.text).bg(c.panel_alt)),
        sections[2],
    );
    frame.render_widget(
        Paragraph::new("h/j/k/l move  H/L month  t today  Enter save  x clear")
            .style(Style::default().fg(c.muted).bg(c.panel)),
        sections[3],
    );
}

fn draw_loading(frame: &mut Frame, area: Rect, app: &App) {
    let c = colors(app.theme.palette());
    let popup = centered_rect(34, 16, area);
    let message = app.loading_message().unwrap_or("Loading GitLab data");
    let detail = app
        .loading_progress_label()
        .unwrap_or_else(|| String::from("Please wait..."));
    let text = Text::from(vec![
        Line::default(),
        Line::from(vec![
            Span::styled(
                app.spinner_frame(),
                Style::default().fg(c.accent).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                message,
                Style::default().fg(c.text).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::default(),
        Line::from(Span::styled(detail, Style::default().fg(c.muted))),
    ]);

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(text)
            .block(pane_block(c, "Loading", true))
            .style(Style::default().bg(c.panel).fg(c.text))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn draw_alert(frame: &mut Frame, area: Rect, app: &App) {
    let c = colors(app.theme.palette());
    let Some(alert) = app.alert.as_ref() else {
        return;
    };

    let popup = centered_rect(70, 38, area);
    let mut lines = vec![
        Line::from(Span::styled(
            alert.title.clone(),
            Style::default().fg(c.danger).add_modifier(Modifier::BOLD),
        )),
        Line::default(),
    ];
    for line in alert.message.lines() {
        lines.push(Line::from(line.to_string()));
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        "Press Enter, Esc, or q to dismiss.",
        Style::default().fg(c.muted),
    )));

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(pane_block(c, "Alert", true))
            .style(Style::default().bg(c.panel).fg(c.text))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn issue_text(app: &App, include_actions: bool) -> Text<'static> {
    let c = colors(app.theme.palette());
    if let Some(issue) = app.selected_issue() {
        let mut lines = vec![
            Line::from(vec![
                Span::styled(
                    format!("#{}", issue.iid),
                    Style::default().fg(c.accent).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(
                    issue.title.clone(),
                    Style::default().fg(c.text).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("State  ", Style::default().fg(c.muted)),
                Span::styled(issue.state.clone(), Style::default().fg(c.text)),
            ]),
            Line::from(vec![
                Span::styled("Due    ", Style::default().fg(c.muted)),
                Span::styled(
                    issue
                        .due_date
                        .clone()
                        .unwrap_or_else(|| String::from("none")),
                    due_style(issue, c),
                ),
                Span::raw("    "),
                Span::styled("Updated  ", Style::default().fg(c.muted)),
                Span::styled(
                    format_timestamp(&issue.updated_at),
                    Style::default().fg(c.text),
                ),
            ]),
            Line::from(vec![
                Span::styled("Labels ", Style::default().fg(c.muted)),
                Span::styled(
                    if issue.labels.is_empty() {
                        String::from("none")
                    } else {
                        issue.labels.join(", ")
                    },
                    Style::default().fg(c.text),
                ),
            ]),
            Line::from(vec![
                Span::styled("URL    ", Style::default().fg(c.muted)),
                Span::styled(issue.web_url.clone(), Style::default().fg(c.info)),
            ]),
        ];

        if include_actions {
            lines.push(Line::from(vec![
                Span::styled("Actions ", Style::default().fg(c.muted)),
                Span::styled(
                    "e edit  c comment  a labels  b add blocker  B remove blocker  d due  x close/reopen  D delete",
                    Style::default().fg(c.text),
                ),
            ]));
        }

        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            "Blockers",
            Style::default().fg(c.iris).add_modifier(Modifier::BOLD),
        )));
        let blockers = app.selected_blockers();
        if !app.selected_issue_links_loaded() {
            lines.push(Line::from(Span::styled(
                "Blockers are loading...",
                Style::default().fg(c.muted),
            )));
        } else if blockers.is_empty() {
            lines.push(Line::from(Span::styled(
                "No blockers.",
                Style::default().fg(c.muted),
            )));
        } else {
            for blocker in blockers {
                lines.push(Line::from(vec![
                    Span::styled("- ", Style::default().fg(c.muted)),
                    Span::styled(format!("#{}", blocker.iid), Style::default().fg(c.rose)),
                    Span::raw(" "),
                    Span::styled(blocker.title.clone(), Style::default().fg(c.text)),
                ]));
            }
        }

        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            "Description",
            Style::default().fg(c.accent).add_modifier(Modifier::BOLD),
        )));

        let body = render_markdown(&issue.description, app.theme.palette());
        lines.extend(body.lines);
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            "Comments",
            Style::default().fg(c.accent).add_modifier(Modifier::BOLD),
        )));

        match app.selected_notes() {
            Some(notes) if notes.is_empty() => lines.push(Line::from(Span::styled(
                "No comments yet.",
                Style::default().fg(c.muted),
            ))),
            Some(notes) => {
                for note in notes {
                    let author = note
                        .author
                        .as_ref()
                        .map(|author| {
                            if author.name.is_empty() {
                                author.username.clone()
                            } else {
                                author.name.clone()
                            }
                        })
                        .unwrap_or_else(|| String::from("unknown"));
                    lines.push(Line::from(vec![
                        Span::styled(
                            author,
                            Style::default().fg(c.warn).add_modifier(Modifier::BOLD),
                        ),
                        Span::raw("  "),
                        Span::styled(
                            format_timestamp(&note.created_at),
                            Style::default().fg(c.muted),
                        ),
                    ]));
                    let markdown = render_markdown(&note.body, app.theme.palette());
                    lines.extend(markdown.lines);
                    lines.push(Line::default());
                }
            }
            None => lines.push(Line::from(Span::styled(
                "Comments are loading...",
                Style::default().fg(c.muted),
            ))),
        }

        Text::from(lines)
    } else {
        Text::from(vec![Line::from(Span::styled(
            "Select an issue to inspect it.",
            Style::default().fg(c.muted),
        ))])
    }
}

fn calendar_text(picker: &DueDatePickerState, c: Colors) -> Text<'static> {
    let mut lines = vec![Line::from(vec![
        Span::styled(" Mo ", Style::default().fg(c.muted)),
        Span::styled(" Tu ", Style::default().fg(c.muted)),
        Span::styled(" We ", Style::default().fg(c.muted)),
        Span::styled(" Th ", Style::default().fg(c.muted)),
        Span::styled(" Fr ", Style::default().fg(c.muted)),
        Span::styled(" Sa ", Style::default().fg(c.muted)),
        Span::styled(" Su ", Style::default().fg(c.muted)),
    ])];

    let first_weekday = picker.month.weekday().number_from_monday() - 1;
    let last_day = last_day_of_month(picker.month.year(), picker.month.month());
    let today = Local::now().date_naive();

    let mut day = 1_u32;
    for week in 0..6 {
        let mut spans = Vec::new();
        for weekday in 0..7 {
            let cell_index = week * 7 + weekday;
            if cell_index < first_weekday || day > last_day {
                spans.push(Span::raw("    "));
                continue;
            }

            let date = NaiveDate::from_ymd_opt(picker.month.year(), picker.month.month(), day)
                .expect("valid calendar date");
            let style = if date == picker.selected {
                Style::default()
                    .bg(c.accent)
                    .fg(c.bg)
                    .add_modifier(Modifier::BOLD)
            } else if date == today {
                Style::default().fg(c.warn).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(c.text)
            };
            spans.push(Span::styled(format!(" {day:>2} "), style));
            day += 1;
        }
        lines.push(Line::from(spans));
        if day > last_day {
            break;
        }
    }

    Text::from(lines)
}

fn due_style(issue: &crate::model::Issue, c: Colors) -> Style {
    if issue.state != "opened" {
        return Style::default().fg(c.muted);
    }

    let today = Local::now().date_naive();
    match issue.due_date.as_deref().and_then(parse_due_date) {
        Some(date) if date < today => Style::default().fg(c.danger).add_modifier(Modifier::BOLD),
        Some(date) if date <= today + Duration::days(2) => {
            Style::default().fg(c.warn).add_modifier(Modifier::BOLD)
        }
        Some(_) => Style::default().fg(c.accent_alt),
        None => Style::default().fg(c.muted),
    }
}

fn styled_block<'a>(c: Colors, title: &'a str) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .title(title)
        .style(Style::default().bg(c.panel).fg(c.muted))
}

fn pane_block<'a>(c: Colors, title: &'a str, active: bool) -> Block<'a> {
    let style = if active {
        Style::default().bg(c.panel).fg(c.accent)
    } else {
        Style::default().bg(c.panel).fg(c.muted)
    };

    Block::default()
        .borders(Borders::ALL)
        .title(title)
        .style(style)
}

fn wrapped_text_height(text: &Text<'_>, width: u16) -> u16 {
    let width = width.max(1) as usize;
    text.lines
        .iter()
        .map(|line| {
            let line_width = line.width();
            let wrapped = if line_width == 0 {
                1
            } else {
                ((line_width - 1) / width) + 1
            };
            wrapped as u16
        })
        .sum()
}

fn editor_scroll(buffer: &TextBuffer, area: Rect) -> u16 {
    let inner = Block::default().borders(Borders::ALL).inner(area);
    let (scroll, _, _) = editor_viewport(buffer, inner.width, inner.height);
    scroll
}

fn editor_viewport(buffer: &TextBuffer, width: u16, height: u16) -> (u16, u16, u16) {
    let width = width.max(1) as usize;
    let height = height.max(1) as usize;

    let mut visual_row = 0usize;
    for line in buffer.lines().iter().take(buffer.row()) {
        visual_row += wrapped_line_rows(line, width);
    }

    let current_line = &buffer.lines()[buffer.row()];
    let current_col = buffer.col().min(current_line.chars().count());
    visual_row += current_col / width;
    let visual_col = (current_col % width) as u16;

    let scroll = visual_row.saturating_sub(height.saturating_sub(1));
    let cursor_y = (visual_row - scroll) as u16;

    (scroll as u16, visual_col, cursor_y)
}

fn wrapped_line_rows(line: &str, width: usize) -> usize {
    let chars = line.chars().count();
    if chars == 0 {
        1
    } else {
        ((chars - 1) / width) + 1
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

fn sidebar_line(c: Colors, label: &str, count: usize, active: bool) -> Line<'static> {
    let style = if active {
        Style::default()
            .fg(c.text)
            .bg(c.panel_alt)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(c.text)
    };
    Line::from(vec![
        Span::styled(format!("{label:<8}"), style),
        Span::styled(format!("{count:>4}"), Style::default().fg(c.muted)),
    ])
}

fn month_name(month: u32) -> &'static str {
    match month {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        _ => "December",
    }
}

fn last_day_of_month(year: i32, month: u32) -> u32 {
    let next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1).expect("valid date")
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1).expect("valid date")
    };
    (next - Duration::days(1)).day()
}
