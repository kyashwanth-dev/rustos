//! Small graphical desktop for VGA-compatible BIOS boots.
//!
//! The bootloader starts RustOS in VGA text mode.  The desktop switches the
//! adapter to 320x200x256 mode so it is a real pixel UI rather than a box
//! drawn with terminal characters.

use x86_64::instructions::port::Port;

const WIDTH: usize = 320;
const HEIGHT: usize = 200;
const VRAM: *mut u8 = 0xa0000 as *mut u8;

const BG: u8 = 17;
const PANEL: u8 = 24;
const PANEL_SELECTED: u8 = 33;
const TEXT: u8 = 15;
const MUTED: u8 = 22;
const ACCENT: u8 = 51;

pub fn enter() {
    set_mode_13h();
    set_palette();
}

pub fn leave() {
    set_text_mode();
}

pub fn draw(cwd: &str) {
    draw_gui(cwd, 0, "READY");
}

pub fn draw_gui(cwd: &str, focus: usize, status: &str) {
    clear(BG);
    rect(0, 0, WIDTH, 18, 19);
    text(8, 5, "RUSTOS", ACCENT);
    text(215, 5, "F1-F4 SELECT", MUTED);
    text(8, 27, "DESKTOP", TEXT);
    text(8, 38, "A SIMPLE GRAPHICAL WORKSPACE", MUTED);

    panel(8, 58, 72, 66, focus == 0, "FILES", "BROWSE");
    panel(88, 58, 72, 66, focus == 1, "TASKS", "MONITOR");
    panel(168, 58, 72, 66, focus == 2, "SYSTEM", "STATUS");
    panel(248, 58, 64, 66, focus == 3, "SHELL", "EXIT");

    rect(8, 137, 304, 31, 19);
    text(16, 145, "STATUS", ACCENT);
    text(16, 156, status, TEXT);
    text(8, 177, "LOCATION", ACCENT);
    text(66, 177, truncate(cwd, 36), TEXT);
    rect(0, 188, WIDTH, 12, 19);
    text(8, 191, "ARROWS / TAB: SELECT    ENTER: OPEN    ESC: SHELL", MUTED);
}

fn panel(x: usize, y: usize, w: usize, h: usize, selected: bool, title: &str, subtitle: &str) {
    rect(x, y, w, h, if selected { PANEL_SELECTED } else { PANEL });
    rect(x, y, w, 3, if selected { ACCENT } else { MUTED });
    text(x + 8, y + 14, title, TEXT);
    text(x + 8, y + 31, subtitle, if selected { ACCENT } else { MUTED });
    if selected {
        rect(x + 8, y + h - 10, 7, 2, ACCENT);
    }
}

fn truncate(value: &str, max: usize) -> &str {
    if value.len() > max { &value[..max] } else { value }
}

fn clear(color: u8) {
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            pixel(x, y, color);
        }
    }
}

fn rect(x: usize, y: usize, w: usize, h: usize, color: u8) {
    for yy in y..core::cmp::min(y + h, HEIGHT) {
        for xx in x..core::cmp::min(x + w, WIDTH) {
            pixel(xx, yy, color);
        }
    }
}

fn pixel(x: usize, y: usize, color: u8) {
    if x < WIDTH && y < HEIGHT {
        unsafe { core::ptr::write_volatile(VRAM.add(y * WIDTH + x), color); }
    }
}

fn text(x: usize, y: usize, value: &str, color: u8) {
    let mut cursor = x;
    for byte in value.bytes() {
        if cursor + 5 >= WIDTH {
            break;
        }
        glyph(cursor, y, byte.to_ascii_uppercase(), color);
        cursor += 6;
    }
}

fn glyph(x: usize, y: usize, c: u8, color: u8) {
    let rows = glyph_rows(c);
    for (dy, row) in rows.iter().enumerate() {
        for dx in 0..5 {
            if row & (1 << (4 - dx)) != 0 {
                pixel(x + dx, y + dy, color);
            }
        }
    }
}

fn glyph_rows(c: u8) -> [u8; 7] {
    match c {
        b'A' => [14, 17, 17, 31, 17, 17, 17], b'B' => [30, 17, 17, 30, 17, 17, 30],
        b'C' => [14, 17, 16, 16, 16, 17, 14], b'D' => [30, 17, 17, 17, 17, 17, 30],
        b'E' => [31, 16, 16, 30, 16, 16, 31], b'F' => [31, 16, 16, 30, 16, 16, 16],
        b'G' => [14, 17, 16, 23, 17, 17, 14], b'H' => [17, 17, 17, 31, 17, 17, 17],
        b'I' => [31, 4, 4, 4, 4, 4, 31], b'J' => [7, 2, 2, 2, 18, 18, 12],
        b'K' => [17, 18, 20, 24, 20, 18, 17], b'L' => [16, 16, 16, 16, 16, 16, 31],
        b'M' => [17, 27, 21, 21, 17, 17, 17], b'N' => [17, 25, 21, 19, 17, 17, 17],
        b'O' => [14, 17, 17, 17, 17, 17, 14], b'P' => [30, 17, 17, 30, 16, 16, 16],
        b'Q' => [14, 17, 17, 17, 21, 18, 13], b'R' => [30, 17, 17, 30, 20, 18, 17],
        b'S' => [15, 16, 16, 14, 1, 1, 30], b'T' => [31, 4, 4, 4, 4, 4, 4],
        b'U' => [17, 17, 17, 17, 17, 17, 14], b'V' => [17, 17, 17, 17, 17, 10, 4],
        b'W' => [17, 17, 17, 21, 21, 27, 17], b'X' => [17, 17, 10, 4, 10, 17, 17],
        b'Y' => [17, 17, 10, 4, 4, 4, 4], b'Z' => [31, 1, 2, 4, 8, 16, 31],
        b'0' => [14, 17, 19, 21, 25, 17, 14], b'1' => [4, 12, 4, 4, 4, 4, 14],
        b'2' => [14, 17, 1, 2, 4, 8, 31], b'3' => [30, 1, 1, 14, 1, 1, 30],
        b'4' => [2, 6, 10, 18, 31, 2, 2], b'5' => [31, 16, 16, 30, 1, 1, 30],
        b'6' => [14, 16, 16, 30, 17, 17, 14], b'7' => [31, 1, 2, 4, 8, 8, 8],
        b'8' => [14, 17, 17, 14, 17, 17, 14], b'9' => [14, 17, 17, 15, 1, 1, 14],
        b'-' => [0, 0, 0, 31, 0, 0, 0], b'/' => [1, 2, 4, 8, 16, 0, 0],
        b':' => [0, 4, 0, 0, 0, 4, 0], b'.' => [0, 0, 0, 0, 0, 6, 6],
        b' ' => [0; 7], _ => [31, 1, 2, 4, 8, 0, 8],
    }
}

fn set_mode_13h() {
    // Standard VGA mode 13h register tables.
    unsafe {
        let mut misc: Port<u8> = Port::new(0x3c2);
        misc.write(0x63);
        for &(index, value) in &[(0, 3), (1, 1), (2, 0x0f), (3, 0), (4, 0x0e)] { seq(index, value); }
        for &(index, value) in &[
            (0, 0x5f), (1, 0x4f), (2, 0x50), (3, 0x82), (4, 0x54), (5, 0x80),
            (6, 0xbf), (7, 0x1f), (8, 0), (9, 0x41), (10, 0), (11, 0), (12, 0),
            (13, 0), (14, 0), (15, 0), (16, 0x9c), (17, 0x8e), (18, 0x8f),
            (19, 0x28), (20, 0x40), (21, 0x96), (22, 0xb9), (23, 0xa3), (24, 0xff),
        ] { crtc(index, value); }
        for &(index, value) in &[(0, 0), (1, 0), (2, 0), (3, 0), (4, 0), (5, 0x40), (6, 5), (7, 0x0f), (8, 0xff)] { gc(index, value); }
        for index in 0..16 { attr(index, index as u8); }
        for &(index, value) in &[(16, 0x41), (17, 0), (18, 0x0f), (19, 0), (20, 0)] { attr(index, value); }
        let mut mode: Port<u8> = Port::new(0x3c0);
        mode.write(0x20);
    }
}

fn set_text_mode() {
    // Mode 3 tables restore the text adapter used by the shell.
    unsafe {
        let mut misc: Port<u8> = Port::new(0x3c2);
        misc.write(0x67);
        for &(index, value) in &[(0, 3), (1, 0), (2, 3), (3, 0), (4, 2)] { seq(index, value); }
        for &(index, value) in &[
            (0, 0x5f), (1, 0x4f), (2, 0x50), (3, 0x82), (4, 0x55), (5, 0x81),
            (6, 0xbf), (7, 0x1f), (8, 0), (9, 0x4f), (10, 0x0d), (11, 0x0e),
            (12, 0), (13, 0), (14, 0), (15, 0), (16, 0x9c), (17, 0x8e), (18, 0x8f),
            (19, 0x28), (20, 0x1f), (21, 0x96), (22, 0xb9), (23, 0xa3), (24, 0xff),
        ] { crtc(index, value); }
        for &(index, value) in &[(0, 0), (1, 0), (2, 0), (3, 0), (4, 0), (5, 0x10), (6, 0x0e), (7, 0), (8, 0xff)] { gc(index, value); }
        let mut mode: Port<u8> = Port::new(0x3c0);
        mode.write(0x20);
    }
}

unsafe fn seq(index: u8, value: u8) {
    let mut i: Port<u8> = Port::new(0x3c4);
    let mut d: Port<u8> = Port::new(0x3c5);
    i.write(index); d.write(value);
}

unsafe fn crtc(index: u8, value: u8) {
    let mut i: Port<u8> = Port::new(0x3d4);
    let mut d: Port<u8> = Port::new(0x3d5);
    i.write(index); d.write(value);
}

unsafe fn gc(index: u8, value: u8) {
    let mut i: Port<u8> = Port::new(0x3ce);
    let mut d: Port<u8> = Port::new(0x3cf);
    i.write(index); d.write(value);
}

unsafe fn attr(index: u8, value: u8) {
    let mut status: Port<u8> = Port::new(0x3da);
    let mut port: Port<u8> = Port::new(0x3c0);
    status.read(); port.write(index); port.write(value);
}

fn set_palette() {
    unsafe {
        let mut index: Port<u8> = Port::new(0x3c8);
        let mut data: Port<u8> = Port::new(0x3c9);
        index.write(0);
        for &(r, g, b) in &[(0, 0, 0), (0, 0, 42), (0, 42, 0), (0, 42, 42), (42, 0, 0), (42, 0, 42), (42, 21, 0), (42, 42, 42), (21, 21, 21), (21, 21, 63), (21, 63, 21), (21, 63, 63), (63, 21, 21), (63, 21, 63), (63, 63, 21), (63, 63, 63)] {
            data.write(r); data.write(g); data.write(b);
        }
        for &(r, g, b) in &[(5, 9, 17), (8, 14, 25), (12, 21, 37), (20, 32, 52), (42, 54, 63), (24, 48, 63)] {
            data.write(r); data.write(g); data.write(b);
        }
    }
}
