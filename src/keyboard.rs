//! PS/2 keyboard: raw scancodes (set 1) -> `Key` events.
//! Handles Shift, Caps Lock, Ctrl and the extended (0xE0) keys: arrows,
//! Home/End, PageUp/PageDown, Delete.

use core::sync::atomic::{AtomicBool, Ordering};

// Index = scancode (0x00..=0x39). 0 means "no printable character".
const NORMAL: &[u8; 58] =
    b"\0\x1b1234567890-=\x08\tqwertyuiop[]\n\0asdfghjkl;'`\0\\zxcvbnm,./\0*\0 ";
const SHIFTED: &[u8; 58] =
    b"\0\x1b!@#$%^&*()_+\x08\tQWERTYUIOP{}\n\0ASDFGHJKL:\"~\0|ZXCVBNM<>?\0*\0 ";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Char(char),
    Enter,
    Backspace,
    Tab,
    Escape,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Ctrl(char), // Ctrl + a lowercase letter
}

impl Key {
    /// Pack into one byte, so keys can travel through the lock-free queue.
    pub fn to_byte(self) -> u8 {
        match self {
            Key::Char(c) => c as u8,
            Key::Enter => 0x80,
            Key::Backspace => 0x81,
            Key::Tab => 0x82,
            Key::Escape => 0x83,
            Key::Delete => 0x84,
            Key::Left => 0x85,
            Key::Right => 0x86,
            Key::Up => 0x87,
            Key::Down => 0x88,
            Key::Home => 0x89,
            Key::End => 0x8A,
            Key::PageUp => 0x8B,
            Key::PageDown => 0x8C,
            Key::Ctrl(c) => 0xA0 + (c as u8 - b'a'),
        }
    }

    pub fn from_byte(b: u8) -> Option<Key> {
        Some(match b {
            0x20..=0x7e => Key::Char(b as char),
            0x80 => Key::Enter,
            0x81 => Key::Backspace,
            0x82 => Key::Tab,
            0x83 => Key::Escape,
            0x84 => Key::Delete,
            0x85 => Key::Left,
            0x86 => Key::Right,
            0x87 => Key::Up,
            0x88 => Key::Down,
            0x89 => Key::Home,
            0x8A => Key::End,
            0x8B => Key::PageUp,
            0x8C => Key::PageDown,
            0xA0..=0xB9 => Key::Ctrl((b - 0xA0 + b'a') as char),
            _ => return None,
        })
    }
}

static SHIFT: AtomicBool = AtomicBool::new(false);
static CTRL: AtomicBool = AtomicBool::new(false);
static CAPS: AtomicBool = AtomicBool::new(false);
static EXTENDED: AtomicBool = AtomicBool::new(false);

/// Feed one raw scancode; returns a key press if it completes one.
pub fn decode(code: u8) -> Option<Key> {
    if code == 0xE0 {
        EXTENDED.store(true, Ordering::Relaxed);
        return None;
    }
    let extended = EXTENDED.swap(false, Ordering::Relaxed);
    let released = code & 0x80 != 0;
    let make = code & 0x7F;

    if extended {
        if make == 0x1D {
            CTRL.store(!released, Ordering::Relaxed); // right Ctrl
            return None;
        }
        if released {
            return None;
        }
        return match make {
            0x48 => Some(Key::Up),
            0x50 => Some(Key::Down),
            0x4B => Some(Key::Left),
            0x4D => Some(Key::Right),
            0x47 => Some(Key::Home),
            0x4F => Some(Key::End),
            0x49 => Some(Key::PageUp),
            0x51 => Some(Key::PageDown),
            0x53 => Some(Key::Delete),
            0x1C => Some(Key::Enter), // keypad Enter
            0x35 => Some(Key::Char('/')), // keypad /
            _ => None,
        };
    }

    match make {
        0x2A | 0x36 => {
            SHIFT.store(!released, Ordering::Relaxed);
            return None;
        }
        0x1D => {
            CTRL.store(!released, Ordering::Relaxed);
            return None;
        }
        0x3A => {
            if !released {
                CAPS.fetch_xor(true, Ordering::Relaxed); // Caps Lock toggles on press
            }
            return None;
        }
        _ => {}
    }

    if released || make as usize >= NORMAL.len() || NORMAL[make as usize] == 0 {
        return None;
    }

    let base = NORMAL[make as usize];
    let is_letter = base.is_ascii_lowercase();

    if CTRL.load(Ordering::Relaxed) {
        return if is_letter { Some(Key::Ctrl(base as char)) } else { None };
    }

    let shift = SHIFT.load(Ordering::Relaxed);
    let upper = if is_letter { shift ^ CAPS.load(Ordering::Relaxed) } else { shift };
    let b = if upper { SHIFTED[make as usize] } else { base };

    Some(match b {
        0x08 => Key::Backspace,
        b'\n' => Key::Enter,
        b'\t' => Key::Tab,
        0x1b => Key::Escape,
        0x20..=0x7e => Key::Char(b as char),
        _ => return None,
    })
}
