//! A small full-screen text editor (nano-style). Draws straight to the screen,
//! then hands the screen back to the shell when it exits.

use crate::fs::FileSystem;
use crate::keyboard::Key;
use crate::vga::{put_cell, set_hw_cursor, HEIGHT, WIDTH};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

const VIEW_H: usize = HEIGHT - 2; // header + status bar take 2 rows
const MAX_COLS: usize = WIDTH - 2;
const MAX_LINES: usize = 2000;

pub struct Editor {
    path: String,
    lines: Vec<String>,
    cx: usize,
    cy: usize,
    top: usize,
    modified: bool,
    quit_armed: bool,
    status: String,
}

impl Editor {
    pub fn open(path: String, content: &[u8]) -> Self {
        let text: String = content
            .iter()
            .map(|&b| match b {
                0x20..=0x7e | b'\n' => b as char,
                b'\t' => ' ',
                _ => '?',
            })
            .collect();
        let mut lines: Vec<String> = text.split('\n').map(|s| s.to_string()).collect();
        // A trailing newline leaves one empty piece at the end; we re-add it on save.
        if lines.len() > 1 && lines.last().map_or(false, |l| l.is_empty()) {
            lines.pop();
        }
        for l in lines.iter_mut() {
            l.truncate(MAX_COLS);
        }
        Editor {
            path,
            lines,
            cx: 0,
            cy: 0,
            top: 0,
            modified: false,
            quit_armed: false,
            status: String::new(),
        }
    }

    /// Returns true when the editor should close.
    pub fn handle_key(&mut self, key: Key, fs: &mut FileSystem) -> bool {
        let was_armed = self.quit_armed;
        self.quit_armed = false;
        self.status.clear();

        match key {
            Key::Ctrl('s') => self.save(fs),
            Key::Ctrl('x') => {
                if self.modified && !was_armed {
                    self.quit_armed = true;
                    self.status = "Unsaved changes! Ctrl+X again to discard, Ctrl+S to save.".to_string();
                } else {
                    return true;
                }
            }
            Key::Char(c) => self.insert_char(c),
            Key::Tab => {
                self.insert_char(' ');
                self.insert_char(' ');
            }
            Key::Enter => {
                if self.lines.len() < MAX_LINES {
                    let rest = self.lines[self.cy].split_off(self.cx);
                    self.lines.insert(self.cy + 1, rest);
                    self.cy += 1;
                    self.cx = 0;
                    self.modified = true;
                }
            }
            Key::Backspace => {
                if self.cx > 0 {
                    self.lines[self.cy].remove(self.cx - 1);
                    self.cx -= 1;
                    self.modified = true;
                } else if self.cy > 0 {
                    let prev_len = self.lines[self.cy - 1].len();
                    if prev_len + self.lines[self.cy].len() <= MAX_COLS {
                        let cur = self.lines.remove(self.cy);
                        self.lines[self.cy - 1].push_str(&cur);
                        self.cy -= 1;
                        self.cx = prev_len;
                        self.modified = true;
                    }
                }
            }
            Key::Delete => {
                if self.cx < self.lines[self.cy].len() {
                    self.lines[self.cy].remove(self.cx);
                    self.modified = true;
                } else if self.cy + 1 < self.lines.len()
                    && self.lines[self.cy].len() + self.lines[self.cy + 1].len() <= MAX_COLS
                {
                    let next = self.lines.remove(self.cy + 1);
                    self.lines[self.cy].push_str(&next);
                    self.modified = true;
                }
            }
            Key::Left => {
                if self.cx > 0 {
                    self.cx -= 1;
                } else if self.cy > 0 {
                    self.cy -= 1;
                    self.cx = self.lines[self.cy].len();
                }
            }
            Key::Right => {
                if self.cx < self.lines[self.cy].len() {
                    self.cx += 1;
                } else if self.cy + 1 < self.lines.len() {
                    self.cy += 1;
                    self.cx = 0;
                }
            }
            Key::Up => self.cy = self.cy.saturating_sub(1),
            Key::Down => self.cy = core::cmp::min(self.cy + 1, self.lines.len() - 1),
            Key::PageUp => self.cy = self.cy.saturating_sub(VIEW_H - 1),
            Key::PageDown => self.cy = core::cmp::min(self.cy + VIEW_H - 1, self.lines.len() - 1),
            Key::Home => self.cx = 0,
            Key::End => self.cx = self.lines[self.cy].len(),
            _ => {}
        }

        self.cx = core::cmp::min(self.cx, self.lines[self.cy].len());
        if self.cy < self.top {
            self.top = self.cy;
        } else if self.cy >= self.top + VIEW_H {
            self.top = self.cy + 1 - VIEW_H;
        }
        false
    }

    fn insert_char(&mut self, c: char) {
        if self.lines[self.cy].len() < MAX_COLS {
            self.lines[self.cy].insert(self.cx, c);
            self.cx += 1;
            self.modified = true;
        }
    }

    fn save(&mut self, fs: &mut FileSystem) {
        let data: Vec<u8> = if self.lines.len() == 1 && self.lines[0].is_empty() {
            Vec::new()
        } else {
            let mut s = self.lines.join("\n");
            s.push('\n');
            s.into_bytes()
        };
        let n = data.len();
        match fs.write(&self.path, data, false) {
            Ok(()) => {
                self.modified = false;
                self.status = format!("Saved {} bytes", n);
            }
            Err(e) => self.status = format!("Save failed: {}", e),
        }
    }

    pub fn draw(&self) {
        let title = format!(
            " RustOS edit: {}{}",
            self.path,
            if self.modified { " [modified]" } else { "" }
        );
        draw_row(0, &title, 0x70); // black on light gray

        for r in 0..VIEW_H {
            match self.lines.get(self.top + r) {
                Some(line) => draw_row(r + 1, line, 0x07),
                None => draw_row(r + 1, "~", 0x08),
            }
        }

        let status = if self.status.is_empty() {
            format!(" ^S Save   ^X Exit   Ln {}, Col {}", self.cy + 1, self.cx + 1)
        } else {
            format!(" {}", self.status)
        };
        draw_row(HEIGHT - 1, &status, 0x70);

        set_hw_cursor(self.cy - self.top + 1, self.cx);
    }
}

fn draw_row(row: usize, text: &str, attr: u8) {
    let bytes = text.as_bytes();
    for col in 0..WIDTH {
        let b = bytes.get(col).copied().unwrap_or(b' ');
        put_cell(row, col, b, attr);
    }
}
