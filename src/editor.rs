use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone)]
pub struct TextBuffer {
    lines: Vec<String>,
    row: usize,
    col: usize,
}

impl TextBuffer {
    pub fn new() -> Self {
        Self::from_text("")
    }

    pub fn from_text(text: &str) -> Self {
        let mut lines = text.lines().map(ToOwned::to_owned).collect::<Vec<_>>();
        if text.ends_with('\n') {
            lines.push(String::new());
        }
        if lines.is_empty() {
            lines.push(String::new());
        }

        Self {
            lines,
            row: 0,
            col: 0,
        }
    }

    pub fn to_text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn row(&self) -> usize {
        self.row
    }

    pub fn col(&self) -> usize {
        self.col
    }

    pub fn move_left(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = line_char_len(&self.lines[self.row]);
        }
    }

    pub fn move_right(&mut self) {
        let line_len = line_char_len(&self.lines[self.row]);
        if self.col < line_len {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
    }

    pub fn move_up(&mut self) {
        if self.row > 0 {
            self.row -= 1;
            self.col = self.col.min(line_char_len(&self.lines[self.row]));
        }
    }

    pub fn move_down(&mut self) {
        if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = self.col.min(line_char_len(&self.lines[self.row]));
        }
    }

    pub fn move_line_start(&mut self) {
        self.col = 0;
    }

    pub fn move_line_end(&mut self) {
        self.col = line_char_len(&self.lines[self.row]);
    }

    pub fn insert_char(&mut self, ch: char) {
        let idx = char_to_byte_idx(&self.lines[self.row], self.col);
        self.lines[self.row].insert(idx, ch);
        self.col += 1;
    }

    pub fn insert_str(&mut self, value: &str) {
        for ch in value.chars() {
            self.insert_char(ch);
        }
    }

    pub fn insert_newline(&mut self) {
        let idx = char_to_byte_idx(&self.lines[self.row], self.col);
        let tail = self.lines[self.row].split_off(idx);
        self.row += 1;
        self.col = 0;
        self.lines.insert(self.row, tail);
    }

    pub fn backspace(&mut self) {
        if self.col > 0 {
            self.col -= 1;
            remove_char_at(&mut self.lines[self.row], self.col);
        } else if self.row > 0 {
            let current = self.lines.remove(self.row);
            self.row -= 1;
            self.col = line_char_len(&self.lines[self.row]);
            self.lines[self.row].push_str(&current);
        }
    }

    pub fn delete(&mut self) {
        if self.col < line_char_len(&self.lines[self.row]) {
            remove_char_at(&mut self.lines[self.row], self.col);
        } else if self.row + 1 < self.lines.len() {
            let next = self.lines.remove(self.row + 1);
            self.lines[self.row].push_str(&next);
        }
    }

    pub fn handle_insert_key(&mut self, key: KeyEvent, multiline: bool) -> bool {
        match key.code {
            KeyCode::Char(ch)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.insert_char(ch);
                true
            }
            KeyCode::Enter if multiline => {
                self.insert_newline();
                true
            }
            KeyCode::Backspace => {
                self.backspace();
                true
            }
            KeyCode::Delete => {
                self.delete();
                true
            }
            KeyCode::Left => {
                self.move_left();
                true
            }
            KeyCode::Right => {
                self.move_right();
                true
            }
            KeyCode::Up if multiline => {
                self.move_up();
                true
            }
            KeyCode::Down if multiline => {
                self.move_down();
                true
            }
            KeyCode::Home => {
                self.move_line_start();
                true
            }
            KeyCode::End => {
                self.move_line_end();
                true
            }
            _ => false,
        }
    }
}

fn line_char_len(line: &str) -> usize {
    line.chars().count()
}

fn char_to_byte_idx(line: &str, char_idx: usize) -> usize {
    if char_idx == 0 {
        return 0;
    }

    line.char_indices()
        .map(|(idx, _)| idx)
        .nth(char_idx)
        .unwrap_or(line.len())
}

fn remove_char_at(line: &mut String, char_idx: usize) {
    let start = char_to_byte_idx(line, char_idx);
    let end = char_to_byte_idx(line, char_idx + 1);
    line.drain(start..end);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_str_appends_text_at_cursor() {
        let mut buffer = TextBuffer::from_text("hi");
        buffer.move_line_end();
        buffer.insert_str(" there");

        assert_eq!(buffer.to_text(), "hi there");
    }

    #[test]
    fn backspace_merges_lines() {
        let mut buffer = TextBuffer::from_text("hello\nworld");
        buffer.move_down();
        buffer.move_line_start();
        buffer.backspace();

        assert_eq!(buffer.to_text(), "helloworld");
    }

    #[test]
    fn supports_inserting_accented_characters() {
        let mut buffer = TextBuffer::new();
        buffer.insert_str("ação ç á ê ã");

        assert_eq!(buffer.to_text(), "ação ç á ê ã");
        assert_eq!(buffer.col(), 12);
    }

    #[test]
    fn backspace_handles_multibyte_characters() {
        let mut buffer = TextBuffer::from_text("ação");
        buffer.move_line_end();
        buffer.backspace();
        buffer.backspace();

        assert_eq!(buffer.to_text(), "aç");
        assert_eq!(buffer.col(), 2);
    }

    #[test]
    fn newline_split_uses_character_boundaries() {
        let mut buffer = TextBuffer::from_text("ação");
        buffer.move_right();
        buffer.move_right();
        buffer.insert_newline();

        assert_eq!(buffer.to_text(), "aç\não");
    }
}
