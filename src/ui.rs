use chrono::{Datelike, Duration, Local, NaiveDate};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::app::{App, DueDatePickerState, EditorField, Mode, format_timestamp, parse_due_date};
use crate::markdown::render_markdown;

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
        Mode::IssueEditor => draw_issue_editor(frame, area, app),
        Mode::CommentEditor => draw_comment_editor(frame, area, app),
        Mode::LabelEditor => draw_label_editor(frame, area, app),
        Mode::Selector => draw_selector(frame, area, app),
        Mode::DueDatePicker => draw_due_date_picker(frame, area, app),
        Mode::Search | Mode::Command | Mode::Normal => {}
    }
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    let status = format!(
        " {}  {}  theme:{}  state:{}  label:{}  status:{}  search:{} ",
        app.mode_label(),
        app.config.project,
        app.config.theme.as_str(),
        app.state_label(),
        app.filters.label.as_deref().unwrap_or("any"),
        app.filters.status.as_deref().unwrap_or("any"),
        if app.filters.search.is_empty() {
            "off"
        } else {
            app.filters.search.as_str()
        },
    );

    let bar = Paragraph::new(status).style(
        Style::default()
            .bg(theme.panel_alt)
            .fg(theme.text)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(bar, area);
}

fn draw_sidebar(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    let today = Local::now().date_naive();
    let selected_issue = app.selected_issue().map(|issue| issue.iid);

    let lines = vec![
        Line::from(vec![Span::styled(
            "Views",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        )]),
        sidebar_line(theme, "All", app.issues.len(), app.state_label() == "all"),
        sidebar_line(theme, "Open", app.count_open(), app.state_label() == "open"),
        sidebar_line(
            theme,
            "Closed",
            app.count_closed(),
            app.state_label() == "closed",
        ),
        sidebar_line(theme, "Overdue", app.count_overdue(), false),
        Line::default(),
        Line::from(vec![Span::styled(
            "Scope",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled("Label  ", Style::default().fg(theme.muted)),
            Span::styled(
                app.filters.label.as_deref().unwrap_or("any"),
                Style::default().fg(theme.text),
            ),
        ]),
        Line::from(vec![
            Span::styled("Status ", Style::default().fg(theme.muted)),
            Span::styled(
                app.filters.status.as_deref().unwrap_or("any"),
                Style::default().fg(theme.text),
            ),
        ]),
        Line::from(vec![
            Span::styled("Search ", Style::default().fg(theme.muted)),
            Span::styled(
                if app.filters.search.is_empty() {
                    "off"
                } else {
                    app.filters.search.as_str()
                },
                Style::default().fg(theme.text),
            ),
        ]),
        Line::default(),
        Line::from(vec![Span::styled(
            "Agenda",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled("Today  ", Style::default().fg(theme.muted)),
            Span::styled(
                app.issues
                    .iter()
                    .filter(|issue| {
                        issue.due_date.as_deref().and_then(parse_due_date) == Some(today)
                    })
                    .count()
                    .to_string(),
                Style::default().fg(theme.text),
            ),
        ]),
        Line::from(vec![
            Span::styled("Picked ", Style::default().fg(theme.muted)),
            Span::styled(
                selected_issue
                    .map(|iid| format!("#{iid}"))
                    .unwrap_or_else(|| "none".to_string()),
                Style::default().fg(theme.text),
            ),
        ]),
    ];

    let sidebar = Paragraph::new(Text::from(lines))
        .style(Style::default().bg(theme.panel).fg(theme.text))
        .block(styled_block(theme, "glissues"));
    frame.render_widget(sidebar, area);
}

fn draw_issue_list(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
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
            Style::default().fg(theme.accent_alt)
        } else {
            Style::default().fg(theme.muted)
        };

        let status = app
            .issue_status(issue)
            .unwrap_or_else(|| "status::none".to_string());
        let due = issue
            .due_date
            .clone()
            .unwrap_or_else(|| "no due".to_string());
        let labels = issue
            .labels
            .iter()
            .filter(|label| !label.starts_with("status::"))
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");

        let meta = if labels.is_empty() {
            format!("{}  {}  {} notes", status, due, issue.user_notes_count)
        } else {
            format!(
                "{}  {}  {}  {} notes",
                status, due, labels, issue.user_notes_count
            )
        };

        items.push(ListItem::new(vec![
            Line::from(vec![
                Span::styled(format!("{state_marker} "), state_style),
                Span::styled(
                    format!("#{} {}", issue.iid, issue.title),
                    Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(Span::styled(meta, Style::default().fg(theme.muted))),
        ]));
    }

    if items.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            "No issues match the current filters.",
            Style::default().fg(theme.muted),
        ))));
    }

    let list = List::new(items)
        .block(pane_block(theme, "Issues", true))
        .highlight_style(
            Style::default()
                .bg(theme.panel_alt)
                .fg(theme.text)
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
    let theme = &app.theme;
    let content = preview_text(app);

    let preview = Paragraph::new(content)
        .block(styled_block(theme, "Preview"))
        .style(Style::default().bg(theme.panel).fg(theme.text))
        .wrap(Wrap { trim: false });
    frame.render_widget(preview, area);
}

fn preview_text(app: &App) -> Text<'static> {
    let theme = &app.theme;
    if let Some(issue) = app.selected_issue() {
        let mut lines = vec![
            Line::from(vec![
                Span::styled(
                    format!("#{}", issue.iid),
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(
                    issue.title.clone(),
                    Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("State  ", Style::default().fg(theme.muted)),
                Span::styled(issue.state.clone(), Style::default().fg(theme.text)),
                Span::raw("    "),
                Span::styled("Status  ", Style::default().fg(theme.muted)),
                Span::styled(
                    app.issue_status(issue)
                        .unwrap_or_else(|| "status::none".to_string()),
                    Style::default().fg(theme.text),
                ),
            ]),
            Line::from(vec![
                Span::styled("Due    ", Style::default().fg(theme.muted)),
                Span::styled(
                    issue.due_date.clone().unwrap_or_else(|| "none".to_string()),
                    due_style(issue, theme),
                ),
                Span::raw("    "),
                Span::styled("Updated  ", Style::default().fg(theme.muted)),
                Span::styled(
                    format_timestamp(&issue.updated_at),
                    Style::default().fg(theme.text),
                ),
            ]),
            Line::from(vec![
                Span::styled("Labels ", Style::default().fg(theme.muted)),
                Span::styled(
                    if issue.labels.is_empty() {
                        "none".to_string()
                    } else {
                        issue.labels.join(", ")
                    },
                    Style::default().fg(theme.text),
                ),
            ]),
            Line::from(vec![
                Span::styled("URL    ", Style::default().fg(theme.muted)),
                Span::styled(issue.web_url.clone(), Style::default().fg(theme.link)),
            ]),
            Line::default(),
            Line::from(Span::styled(
                "Description",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            )),
        ];

        let body = render_markdown(&issue.description, theme);
        lines.extend(body.lines);
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            "Comments",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        )));

        match app.selected_notes() {
            Some(notes) if notes.is_empty() => lines.push(Line::from(Span::styled(
                "No comments yet.",
                Style::default().fg(theme.muted),
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
                        .unwrap_or_else(|| "unknown".to_string());
                    lines.push(Line::from(vec![
                        Span::styled(
                            author,
                            Style::default().fg(theme.warn).add_modifier(Modifier::BOLD),
                        ),
                        Span::raw("  "),
                        Span::styled(
                            format_timestamp(&note.created_at),
                            Style::default().fg(theme.muted),
                        ),
                    ]));
                    let markdown = render_markdown(&note.body, theme);
                    lines.extend(markdown.lines);
                    lines.push(Line::default());
                }
            }
            None => lines.push(Line::from(Span::styled(
                "Comments are loading...",
                Style::default().fg(theme.muted),
            ))),
        }

        Text::from(lines)
    } else {
        Text::from(vec![Line::from(Span::styled(
            "Select an issue to inspect it.",
            Style::default().fg(theme.muted),
        ))])
    }
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    let prompt = match app.mode {
        Mode::Search => format!("/{}", app.search_input),
        Mode::Command => format!(":{}", app.command_input),
        _ => app.status_line.clone(),
    };

    let hints = match app.mode {
        Mode::Normal => {
            " Enter open issue  Tab state filter  j/k move  n new  e edit  l/s filters  : command  Ctrl-c quit "
        }
        Mode::IssueView => {
            " Esc close issue  j/k or arrows scroll  mouse wheel scroll  gg/G jump  Ctrl-u/Ctrl-d fast scroll "
        }
        Mode::Search => " Enter apply  Esc cancel ",
        Mode::Command => " Enter run command  Esc cancel ",
        _ => " Esc close overlay  Ctrl-s save while editing ",
    };

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(area);

    frame.render_widget(
        Paragraph::new(prompt).style(Style::default().bg(theme.panel_alt).fg(theme.text)),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(hints).style(Style::default().bg(theme.bg).fg(theme.muted)),
        rows[1],
    );
}

fn draw_help(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    let popup = centered_rect(72, 60, area);
    let text = Text::from(vec![
        Line::from(Span::styled(
            "glissues keymap",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        )),
        Line::default(),
        Line::from("j/k or arrows  move through the issue list"),
        Line::from("Enter          open the selected issue in a popup"),
        Line::from("Esc            leave the issue popup or close overlays"),
        Line::from("mouse wheel    move the list or scroll the open issue"),
        Line::from("gg / G         jump to top or bottom"),
        Line::from("Ctrl-u / Ctrl-d scroll the open issue faster"),
        Line::from("n              create a new issue"),
        Line::from("e              edit title and body"),
        Line::from("x              close or reopen the selected issue"),
        Line::from("c              add a comment"),
        Line::from("L              edit labels with autocomplete"),
        Line::from("S              set workflow status label"),
        Line::from("d              open the due date picker"),
        Line::from("Tab            cycle all/open/closed filters"),
        Line::from("l / s          filter by label or status"),
        Line::from("/              fuzzy-like text filter"),
        Line::from(":              run quick commands like :refresh or :filter open"),
        Line::from("Ctrl-c         quit instantly"),
        Line::default(),
        Line::from(Span::styled(
            "Editor mode",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("i              enter insert mode"),
        Line::from("Esc            leave insert mode"),
        Line::from("Tab            switch between title/body fields"),
        Line::from("Ctrl-s         save changes"),
        Line::from("q              cancel the current overlay"),
    ]);

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(text)
            .block(styled_block(theme, "Help"))
            .style(Style::default().bg(theme.panel).fg(theme.text))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn draw_issue_view(frame: &mut Frame, area: Rect, app: &mut App) {
    let theme = app.theme;
    let popup = centered_rect(82, 86, area);
    let content = preview_text(app);
    let inner = Block::default().borders(Borders::ALL).inner(popup);
    let content_height = wrapped_text_height(&content, inner.width);
    app.sync_issue_view_layout(popup, inner.height, content_height);

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(content)
            .block(pane_block(&theme, "Issue", true))
            .style(Style::default().bg(theme.panel).fg(theme.text))
            .wrap(Wrap { trim: false })
            .scroll((app.issue_view_scroll, 0)),
        popup,
    );
}

fn draw_issue_editor(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
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
            .style(Style::default().bg(theme.panel).fg(theme.text)),
        popup,
    );

    let title_style = if matches!(editor.focus, EditorField::Title) {
        Style::default().fg(theme.text).bg(theme.panel_alt)
    } else {
        Style::default().fg(theme.text).bg(theme.panel)
    };
    let body_style = if matches!(editor.focus, EditorField::Body) {
        Style::default().fg(theme.text).bg(theme.panel_alt)
    } else {
        Style::default().fg(theme.text).bg(theme.panel)
    };

    frame.render_widget(
        Paragraph::new(editor.title.to_text())
            .block(styled_block(theme, "Title"))
            .style(title_style),
        sections[0],
    );
    frame.render_widget(
        Paragraph::new(editor.body.to_text())
            .block(styled_block(theme, "Body (Markdown)"))
            .style(body_style)
            .wrap(Wrap { trim: false }),
        sections[1],
    );
    frame.render_widget(
        Paragraph::new(format!(
            "{}  i insert  Esc normal  Tab next field  Ctrl-s save  q cancel",
            if matches!(editor.mode, crate::editor::EditMode::Insert) {
                "INSERT"
            } else {
                "NORMAL"
            }
        ))
        .style(Style::default().fg(theme.muted).bg(theme.panel)),
        sections[2],
    );

    if matches!(editor.mode, crate::editor::EditMode::Insert) {
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
                frame.set_cursor_position((
                    inner.x + editor.body.col() as u16,
                    inner.y + editor.body.row() as u16,
                ));
            }
        }
    }
}

fn draw_comment_editor(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
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
            .style(Style::default().bg(theme.panel).fg(theme.text)),
        popup,
    );
    frame.render_widget(
        Paragraph::new(editor.body.to_text())
            .block(styled_block(theme, "Body"))
            .style(Style::default().bg(theme.panel_alt).fg(theme.text))
            .wrap(Wrap { trim: false }),
        inner[0],
    );
    frame.render_widget(
        Paragraph::new("i insert  Esc normal  Ctrl-s save  q cancel")
            .style(Style::default().fg(theme.muted).bg(theme.panel)),
        inner[1],
    );

    if matches!(editor.mode, crate::editor::EditMode::Insert) {
        let cursor = Block::default().borders(Borders::ALL).inner(inner[0]);
        frame.set_cursor_position((
            cursor.x + editor.body.col() as u16,
            cursor.y + editor.body.row() as u16,
        ));
    }
}

fn draw_label_editor(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
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
            .style(Style::default().bg(theme.panel).fg(theme.text)),
        popup,
    );
    frame.render_widget(
        Paragraph::new(picker.query.as_str())
            .block(styled_block(theme, "Search or Create"))
            .style(Style::default().bg(theme.panel_alt).fg(theme.text)),
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
        .block(styled_block(theme, "Selected"))
        .style(Style::default().bg(theme.panel).fg(theme.text))
        .wrap(Wrap { trim: false }),
        sections[1],
    );
    let list = List::new(items)
        .block(styled_block(theme, "Autocomplete"))
        .highlight_symbol("▎")
        .highlight_style(Style::default().bg(theme.panel_alt).fg(theme.text));
    let mut state = ListState::default();
    if !filtered.is_empty() {
        state.select(Some(picker.cursor.min(filtered.len() - 1)));
    }
    frame.render_stateful_widget(list, sections[2], &mut state);
    frame.render_widget(
        Paragraph::new("type to filter  Space toggle  Enter save  Esc cancel")
            .style(Style::default().fg(theme.muted).bg(theme.panel)),
        sections[3],
    );
}

fn draw_selector(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
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
            .style(Style::default().bg(theme.panel).fg(theme.text)),
        popup,
    );
    frame.render_widget(
        Paragraph::new(selector.query.as_str())
            .block(styled_block(theme, "Filter"))
            .style(Style::default().bg(theme.panel_alt).fg(theme.text)),
        sections[0],
    );
    let list = List::new(items)
        .block(styled_block(theme, "Options"))
        .highlight_symbol("▎")
        .highlight_style(Style::default().bg(theme.panel_alt).fg(theme.text));
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
        .style(Style::default().fg(theme.muted).bg(theme.panel)),
        sections[2],
    );
}

fn draw_due_date_picker(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
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
            .style(Style::default().bg(theme.panel).fg(theme.text)),
        popup,
    );

    let title = Paragraph::new(format!(
        "{} {}",
        month_name(picker.month.month()),
        picker.month.year()
    ))
    .style(
        Style::default()
            .fg(theme.accent)
            .bg(theme.panel)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(title, sections[0]);

    let calendar = Paragraph::new(calendar_text(picker, theme))
        .style(Style::default().fg(theme.text).bg(theme.panel))
        .wrap(Wrap { trim: false });
    frame.render_widget(calendar, sections[1]);

    let info = Paragraph::new(format!("Selected: {}", picker.selected.format("%Y-%m-%d")))
        .style(Style::default().fg(theme.text).bg(theme.panel_alt));
    frame.render_widget(info, sections[2]);
    frame.render_widget(
        Paragraph::new("h/j/k/l move  H/L month  t today  Enter save  x clear")
            .style(Style::default().fg(theme.muted).bg(theme.panel)),
        sections[3],
    );
}

fn calendar_text(picker: &DueDatePickerState, theme: &crate::theme::Theme) -> Text<'static> {
    let mut lines = vec![Line::from(vec![
        Span::styled(" Mo ", Style::default().fg(theme.muted)),
        Span::styled(" Tu ", Style::default().fg(theme.muted)),
        Span::styled(" We ", Style::default().fg(theme.muted)),
        Span::styled(" Th ", Style::default().fg(theme.muted)),
        Span::styled(" Fr ", Style::default().fg(theme.muted)),
        Span::styled(" Sa ", Style::default().fg(theme.muted)),
        Span::styled(" Su ", Style::default().fg(theme.muted)),
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
                    .bg(theme.accent)
                    .fg(theme.bg)
                    .add_modifier(Modifier::BOLD)
            } else if date == today {
                Style::default().fg(theme.warn).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text)
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

fn due_style(issue: &crate::model::Issue, theme: &crate::theme::Theme) -> Style {
    if issue.state != "opened" {
        return Style::default().fg(theme.muted);
    }

    let today = Local::now().date_naive();
    match issue.due_date.as_deref().and_then(parse_due_date) {
        Some(date) if date < today => Style::default()
            .fg(theme.danger)
            .add_modifier(Modifier::BOLD),
        Some(date) if date <= today + Duration::days(2) => {
            Style::default().fg(theme.warn).add_modifier(Modifier::BOLD)
        }
        Some(_) => Style::default().fg(theme.accent_alt),
        None => Style::default().fg(theme.muted),
    }
}

fn styled_block<'a>(theme: &crate::theme::Theme, title: &'a str) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .title(title)
        .style(Style::default().bg(theme.panel).fg(theme.muted))
}

fn pane_block<'a>(theme: &crate::theme::Theme, title: &'a str, active: bool) -> Block<'a> {
    let style = if active {
        Style::default().bg(theme.panel).fg(theme.accent)
    } else {
        Style::default().bg(theme.panel).fg(theme.muted)
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

fn sidebar_line(
    theme: &crate::theme::Theme,
    label: &str,
    count: usize,
    active: bool,
) -> Line<'static> {
    let style = if active {
        Style::default()
            .fg(theme.text)
            .bg(theme.panel_alt)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text)
    };
    Line::from(vec![
        Span::styled(format!("{label:<8}"), style),
        Span::styled(format!("{count:>4}"), Style::default().fg(theme.muted)),
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
