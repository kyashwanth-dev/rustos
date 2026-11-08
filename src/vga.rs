//! VGA text-mode driver with scrollback and a hardware cursor.
//!
//! The Writer keeps the last 300 lines in a buffer (in static memory, no heap).
//! The screen is a 25-line "window" onto that buffer. New output always lives on
//! the bottom line; PageUp/PageDown slide the window into the past.

use core::fmt;
use core::ptr::write_volatile;
use spin::Mutex;
use x86_64::instructions::port::Port;

pub const WIDTH: usize = 80;
pub const HEIGHT: usize = 25;
const HISTORY_LINES: usize = 300;

#[allow(dead_code)]
#[derive(Clone, Copy)]
#[repr(u8)]
pub enum Color {
    Black = 0, Blue = 1, Green = 2, Cyan = 3, Red = 4, Magenta = 5,
    Brown = 6, LightGray = 7, DarkGray = 8, LightBlue = 9, LightGreen = 10,
    LightCyan = 11, LightRed = 12, Pink = 13, Yellow = 14, White = 15,
}

const fn attr_of(fg: Color, bg: Color) -> u8 {
    ((bg as u8) << 4) | (fg as u8)
}

const fn cell(attr: u8, byte: u8) -> u16 {
    ((attr as u16) << 8) | byte as u16
}

pub struct Writer {
    lines: [[u16; WIDTH]; HISTORY_LINES],
    head: usize,    // index of the live (bottom) line
    created: usize, // lines created since the last clear
    offset: usize,  // how far the view is scrolled into the past (0 = live)
    col: usize,
    attr: u8,
}

/// The one global screen writer. A `const` initializer keeps the big buffer
/// out of the stack.
pub static WRITER: Mutex<Writer> = Mutex::new(Writer::new(Color::White, Color::Black));

fn vga_ptr(row: usize, col: usize) -> *mut u16 {
    (0xb8000 as *mut u16).wrapping_add(row * WIDTH + col)
}

impl Writer {
    pub const fn new(fg: Color, bg: Color) -> Self {
        let attr = attr_of(fg, bg);
        Writer {
            lines: [[cell(attr, b' '); WIDTH]; HISTORY_LINES],
            head: 0,
            created: 0,
            offset: 0,
            col: 0,
            attr,
        }
    }

    pub fn set_color(&mut self, fg: Color, bg: Color) {
        self.attr = attr_of(fg, bg);
    }

    #[allow(dead_code)]
    pub fn col(&self) -> usize {
        self.col
    }

    pub fn set_col(&mut self, col: usize) {
        self.col = core::cmp::min(col, WIDTH - 1);
        self.update_cursor();
    }

    fn line_index(&self, screen_row: usize) -> usize {
        (self.head + HISTORY_LINES * 2 - (HEIGHT - 1 - screen_row) - self.offset) % HISTORY_LINES
    }

    fn draw_row(&self, row: usize) {
        let idx = self.line_index(row);
        for col in 0..WIDTH {
            unsafe { write_volatile(vga_ptr(row, col), self.lines[idx][col]) };
        }
    }

    fn draw_screen(&self) {
        for row in 0..HEIGHT {
            self.draw_row(row);
        }
    }

    /// Repaint everything (used after the full-screen editor exits).
    pub fn redraw(&self) {
        self.draw_screen();
        self.update_cursor();
    }

    fn snap_to_bottom(&mut self) {
        if self.offset != 0 {
            self.offset = 0;
            self.draw_screen();
        }
    }

    pub fn clear(&mut self) {
        let blank = [cell(self.attr, b' '); WIDTH];
        for line in self.lines.iter_mut() {
            *line = blank;
        }
        self.head = 0;
        self.created = 0;
        self.offset = 0;
        self.col = 0;
        self.draw_screen();
        self.update_cursor();
    }

    /// Blank the live line and put the cursor at its start.
    pub fn clear_current_line(&mut self) {
        self.snap_to_bottom();
        self.lines[self.head] = [cell(self.attr, b' '); WIDTH];
        self.col = 0;
        self.draw_row(HEIGHT - 1);
        self.update_cursor();
    }

    /// Scroll the view: positive = into the past, negative = back toward live.
    pub fn scroll_view(&mut self, delta: isize) {
        let available = core::cmp::min(self.created + 1, HISTORY_LINES);
        let max_offset = available.saturating_sub(HEIGHT) as isize;
        let new = (self.offset as isize + delta).clamp(0, max_offset);
        if new as usize != self.offset {
            self.offset = new as usize;
            self.draw_screen();
            self.update_cursor();
        }
    }

    fn new_line(&mut self) {
        self.head = (self.head + 1) % HISTORY_LINES;
        self.lines[self.head] = [cell(self.attr, b' '); WIDTH];
        self.created = core::cmp::min(self.created + 1, HISTORY_LINES);
        self.col = 0;
        self.draw_screen();
    }

    #[allow(dead_code)]
    pub fn backspace(&mut self) {
        if self.col > 0 {
            self.snap_to_bottom();
            self.col -= 1;
            self.lines[self.head][self.col] = cell(self.attr, b' ');
            self.draw_row(HEIGHT - 1);
            self.update_cursor();
        }
    }

    pub fn write_byte(&mut self, byte: u8) {
        self.snap_to_bottom();
        match byte {
            b'\n' => self.new_line(),
            byte => {
                if self.col >= WIDTH {
                    self.new_line();
                }
                let c = cell(self.attr, byte);
                self.lines[self.head][self.col] = c;
                unsafe { write_volatile(vga_ptr(HEIGHT - 1, self.col), c) };
                self.col += 1;
            }
        }
    }

    pub fn update_cursor(&self) {
        if self.offset == 0 {
            set_hw_cursor(HEIGHT - 1, core::cmp::min(self.col, WIDTH - 1));
        } else {
            set_cursor_pos((WIDTH * HEIGHT) as u16); // off-screen = hidden
        }
    }
}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            match byte {
                0x20..=0x7e | b'\n' => self.write_byte(byte),
                _ => self.write_byte(0xfe),
            }
        }
        self.update_cursor();
        Ok(())
    }
}

// ---- Low-level helpers (used by the full-screen editor) ----

fn set_cursor_pos(pos: u16) {
    unsafe {
        let mut index: Port<u8> = Port::new(0x3D4);
        let mut data: Port<u8> = Port::new(0x3D5);
        index.write(0x0F);
        data.write((pos & 0xFF) as u8);
        index.write(0x0E);
        data.write((pos >> 8) as u8);
    }
}

pub fn set_hw_cursor(row: usize, col: usize) {
    set_cursor_pos((row * WIDTH + col) as u16);
}

/// Turn on the blinking underline cursor (scanlines 13..15 of the 16-line cell).
pub fn enable_cursor() {
    unsafe {
        let mut index: Port<u8> = Port::new(0x3D4);
        let mut data: Port<u8> = Port::new(0x3D5);
        index.write(0x0A);
        data.write(13);
        index.write(0x0B);
        data.write(15);
    }
}

/// Write one cell straight to the screen, bypassing the scrollback buffer.
pub fn put_cell(row: usize, col: usize, byte: u8, attr: u8) {
    unsafe { write_volatile(vga_ptr(row, col), cell(attr, byte)) };
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::vga::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    // Interrupts off while holding the lock, so a handler can't deadlock against us.
    x86_64::instructions::interrupts::without_interrupts(|| {
        WRITER.lock().write_fmt(args).unwrap();
    });
}
