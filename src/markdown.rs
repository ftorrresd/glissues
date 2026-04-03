use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui_themes::ThemePalette;
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Style as SyntectStyle, ThemeSet};
use syntect::parsing::SyntaxSet;

pub fn render_markdown(input: &str, palette: ThemePalette) -> Text<'static> {
    let mut lines = Vec::new();
    let mut code_block = None::<CodeBlockState<'static>>;

    for raw_line in input.lines() {
        let trimmed = raw_line.trim_end();

        if let Some(rest) = trimmed.strip_prefix("```") {
            if code_block.is_some() {
                code_block = None;
            } else {
                code_block = Some(CodeBlockState::new(rest.trim()));
            }

            lines.push(Line::from(Span::styled(
                trimmed.to_string(),
                Style::default().fg(palette.info),
            )));
            continue;
        }

        if let Some(state) = code_block.as_mut() {
            lines.push(render_code_line(state, raw_line));
            continue;
        }

        if trimmed.is_empty() {
            lines.push(Line::default());
            continue;
        }

        let (prefix, content, base_style) = classify_line(trimmed, palette);
        let mut spans = Vec::new();

        if !prefix.is_empty() {
            spans.push(Span::styled(prefix.to_string(), base_style));
        }

        spans.extend(render_inline(content, base_style, palette));
        lines.push(Line::from(spans));
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "(empty)",
            Style::default().fg(Color::DarkGray),
        )));
    }

    Text::from(lines)
}

struct CodeBlockState<'a> {
    highlighter: HighlightLines<'a>,
}

impl CodeBlockState<'static> {
    fn new(language: &str) -> Self {
        let syntax_set = syntax_set();
        let theme = code_theme();
        let token = language
            .split(|ch: char| ch.is_whitespace() || ch == '{')
            .find(|part| !part.is_empty())
            .unwrap_or_default();
        let syntax = syntax_set
            .find_syntax_by_token(token)
            .or_else(|| syntax_set.find_syntax_by_extension(token))
            .unwrap_or_else(|| syntax_set.find_syntax_plain_text());

        Self {
            highlighter: HighlightLines::new(syntax, theme),
        }
    }
}

fn render_code_line(state: &mut CodeBlockState<'static>, line: &str) -> Line<'static> {
    match state.highlighter.highlight_line(line, syntax_set()) {
        Ok(ranges) => Line::from(
            ranges
                .into_iter()
                .map(|(style, segment)| Span::styled(segment.to_string(), syntect_style(style)))
                .collect::<Vec<_>>(),
        ),
        Err(_) => Line::from(Span::raw(line.to_string())),
    }
}

fn syntect_style(style: SyntectStyle) -> Style {
    let mut modifiers = Modifier::empty();

    if style.font_style.contains(FontStyle::BOLD) {
        modifiers |= Modifier::BOLD;
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        modifiers |= Modifier::ITALIC;
    }
    if style.font_style.contains(FontStyle::UNDERLINE) {
        modifiers |= Modifier::UNDERLINED;
    }

    Style::default()
        .fg(Color::Rgb(
            style.foreground.r,
            style.foreground.g,
            style.foreground.b,
        ))
        .add_modifier(modifiers)
}

fn syntax_set() -> &'static SyntaxSet {
    static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn code_theme() -> &'static syntect::highlighting::Theme {
    static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();
    let theme_set = THEME_SET.get_or_init(ThemeSet::load_defaults);
    theme_set
        .themes
        .get("base16-ocean.dark")
        .or_else(|| theme_set.themes.values().next())
        .expect("syntect themes should include at least one theme")
}

fn classify_line<'a>(line: &'a str, palette: ThemePalette) -> (&'a str, &'a str, Style) {
    let heading = [
        ("###### ", Style::default().fg(palette.accent)),
        ("##### ", Style::default().fg(palette.accent)),
        (
            "#### ",
            Style::default()
                .fg(palette.secondary)
                .add_modifier(Modifier::BOLD),
        ),
        (
            "### ",
            Style::default()
                .fg(palette.secondary)
                .add_modifier(Modifier::BOLD),
        ),
        (
            "## ",
            Style::default()
                .fg(palette.warning)
                .add_modifier(Modifier::BOLD),
        ),
        (
            "# ",
            Style::default()
                .fg(palette.warning)
                .add_modifier(Modifier::BOLD),
        ),
    ];

    for (marker, style) in heading {
        if let Some(rest) = line.strip_prefix(marker) {
            return ("", rest, style);
        }
    }

    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(marker) {
            return ("• ", rest, Style::default().fg(palette.fg));
        }
    }

    if let Some(rest) = line.strip_prefix("> ") {
        return ("│ ", rest, Style::default().fg(palette.muted));
    }

    if let Some((prefix, rest)) = split_ordered_prefix(line) {
        return (prefix, rest, Style::default().fg(palette.fg));
    }

    ("", line, Style::default().fg(palette.fg))
}

fn split_ordered_prefix(line: &str) -> Option<(&str, &str)> {
    let bytes = line.as_bytes();
    let mut digits = 0;
    while digits < bytes.len() && bytes[digits].is_ascii_digit() {
        digits += 1;
    }

    if digits == 0
        || digits + 1 >= bytes.len()
        || bytes[digits] != b'.'
        || bytes[digits + 1] != b' '
    {
        return None;
    }

    Some((&line[..digits + 2], &line[digits + 2..]))
}

fn render_inline(content: &str, base_style: Style, palette: ThemePalette) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut cursor = 0;

    while cursor < content.len() {
        let tail = &content[cursor..];

        if let Some(rest) = tail.strip_prefix("**") {
            if let Some(end) = rest.find("**") {
                let text = &rest[..end];
                spans.push(Span::styled(
                    text.to_string(),
                    base_style.add_modifier(Modifier::BOLD),
                ));
                cursor += 2 + end + 2;
                continue;
            }
        }

        if let Some(rest) = tail.strip_prefix('`') {
            if let Some(end) = rest.find('`') {
                let text = &rest[..end];
                spans.push(Span::styled(
                    text.to_string(),
                    Style::default().fg(palette.info),
                ));
                cursor += 1 + end + 1;
                continue;
            }
        }

        if let Some(rest) = tail.strip_prefix('*') {
            if let Some(end) = rest.find('*') {
                let text = &rest[..end];
                spans.push(Span::styled(
                    text.to_string(),
                    base_style.add_modifier(Modifier::ITALIC),
                ));
                cursor += 1 + end + 1;
                continue;
            }
        }

        if let Some(rest) = tail.strip_prefix("~~") {
            if let Some(end) = rest.find("~~") {
                let text = &rest[..end];
                spans.push(Span::styled(
                    text.to_string(),
                    base_style.add_modifier(Modifier::CROSSED_OUT),
                ));
                cursor += 2 + end + 2;
                continue;
            }
        }

        if let Some(rest) = tail.strip_prefix('[') {
            if let Some(text_end) = rest.find("](") {
                let label = &rest[..text_end];
                let url_part = &rest[text_end + 2..];
                if let Some(url_end) = url_part.find(')') {
                    let url = &url_part[..url_end];
                    spans.push(Span::styled(
                        label.to_string(),
                        Style::default()
                            .fg(palette.secondary)
                            .add_modifier(Modifier::UNDERLINED),
                    ));
                    spans.push(Span::styled(
                        format!(" <{}>", url),
                        Style::default().fg(palette.muted),
                    ));
                    cursor += 1 + text_end + 2 + url_end + 1;
                    continue;
                }
            }
        }

        if let Some(ch) = tail.chars().next() {
            spans.push(Span::styled(ch.to_string(), base_style));
            cursor += ch.len_utf8();
        } else {
            break;
        }
    }

    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui_themes::{Theme, ThemeName};

    #[test]
    fn highlights_rust_code_blocks() {
        let palette = Theme::new(ThemeName::RosePine).palette();
        let rendered = render_markdown("```rust\nlet answer = 42;\n```", palette);

        assert_eq!(rendered.lines.len(), 3);
        assert!(rendered.lines[1].spans.len() > 1);
    }
}
