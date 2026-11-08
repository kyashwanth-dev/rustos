//! Minimal text-mode desktop for the RustOS shell.

use crate::vga::{Color, WRITER};
use crate::println;

pub fn draw(cwd: &str) {
    draw_gui(cwd, 0, "Ready");
}

pub fn draw_gui(cwd: &str, focus: usize, status: &str) {
    let cwd = if cwd.len() > 64 { &cwd[..64] } else { cwd };
    WRITER.lock().clear();

    color(Color::LightCyan);
    println!("+==============================================================================+");
    color(Color::White);
    println!("|  RUSTOS DESKTOP                                      F1-F4 SELECT  ESC SHELL |");
    color(Color::LightCyan);
    println!("+==============================================================================+");
    color(Color::White);
    println!("|                                                                              |");
    panel(0, focus == 0, "FILES", "F1", "Browse current directory");
    panel(1, focus == 1, "TASKS", "F2", "View cooperative tasks");
    panel(2, focus == 2, "SYSTEM", "F3", "Memory and kernel status");
    panel(3, focus == 3, "SHELL", "F4", "Return to command line");
    println!("|                                                                              |");
    color(Color::LightGreen);
    println!("|  STATUS: {:<68}|", truncate(status, 68));
    color(Color::Yellow);
    println!("|  LOCATION: {:<66}|", truncate(cwd, 66));
    color(Color::Yellow);
    println!("|  ENTER open   LEFT/RIGHT change focus   UP/DOWN navigate   ESC return       |");
    color(Color::White);
    println!("|                                                                              |");
    color(Color::LightCyan);
    println!("+==============================================================================+");
    color(Color::White);
    WRITER.lock().update_cursor();
}

fn panel(row: usize, selected: bool, title: &str, key: &str, subtitle: &str) {
    let marker = if selected { ">" } else { " " };
    let fg = if selected { Color::LightGreen } else { Color::White };
    color(fg);
    match row {
        0 => println!("|  {} [{}] {:<12}  {}                                               |", marker, key, title, subtitle),
        1 => println!("|  {} [{}] {:<12}  {}                                               |", marker, key, title, subtitle),
        2 => println!("|  {} [{}] {:<12}  {}                                               |", marker, key, title, subtitle),
        _ => println!("|  {} [{}] {:<12}  {}                                               |", marker, key, title, subtitle),
    }
}

fn truncate(value: &str, max: usize) -> &str {
    if value.len() > max { &value[..max] } else { value }
}

fn color(fg: Color) {
    WRITER.lock().set_color(fg, Color::Black);
}
