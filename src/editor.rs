use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditMode {
    Normal,
    Insert,
}

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
            self.col = self.lines[self.row].len();
        }
    }

    pub fn move_right(&mut self) {
        let line_len = self.lines[self.row].len();
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
            self.col = self.col.min(self.lines[self.row].len());
        }
    }

    pub fn move_down(&mut self) {
        if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = self.col.min(self.lines[self.row].len());
        }
    }

    pub fn move_line_start(&mut self) {
        self.col = 0;
    }

    pub fn move_line_end(&mut self) {
        self.col = self.lines[self.row].len();
    }

    pub fn insert_char(&mut self, ch: char) {
        self.lines[self.row].insert(self.col, ch);
        self.col += 1;
    }

    pub fn insert_newline(&mut self) {
        let tail = self.lines[self.row].split_off(self.col);
        self.row += 1;
        self.col = 0;
        self.lines.insert(self.row, tail);
    }

    pub fn backspace(&mut self) {
        if self.col > 0 {
            self.col -= 1;
            self.lines[self.row].remove(self.col);
        } else if self.row > 0 {
            let current = self.lines.remove(self.row);
            self.row -= 1;
            self.col = self.lines[self.row].len();
            self.lines[self.row].push_str(&current);
        }
    }

    pub fn delete(&mut self) {
        if self.col < self.lines[self.row].len() {
            self.lines[self.row].remove(self.col);
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

    pub fn handle_normal_motion(&mut self, key: KeyEvent, multiline: bool) -> bool {
        match key.code {
            KeyCode::Char('h') | KeyCode::Left => {
                self.move_left();
                true
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.move_right();
                true
            }
            KeyCode::Char('k') | KeyCode::Up if multiline => {
                self.move_up();
                true
            }
            KeyCode::Char('j') | KeyCode::Down if multiline => {
                self.move_down();
                true
            }
            KeyCode::Char('0') | KeyCode::Home => {
                self.move_line_start();
                true
            }
            KeyCode::Char('$') | KeyCode::End => {
                self.move_line_end();
                true
            }
            _ => false,
        }
    }
}
