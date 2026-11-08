//! Minimal text-mode desktop for the RustOS shell.

use crate::vga::{Color, WRITER};
use crate::println;

pub fn draw(cwd: &str) {
    let cwd = if cwd.len() > 64 { &cwd[..64] } else { cwd };
    WRITER.lock().clear();

    color(Color::LightCyan);
    println!("+------------------------------------------------------------------------------+");
    color(Color::White);
    println!("|  RUSTOS                                                   MINIMAL DESKTOP    |");
    color(Color::LightCyan);
    println!("+------------------------------------------------------------------------------+");
    color(Color::White);
    println!("|                                                                              |");
    println!("|  Welcome back.                                                              |");
    println!("|  A small, keyboard-first operating system written in Rust.                  |");
    println!("|                                                                              |");
    color(Color::LightGreen);
    println!("|  SYSTEM                                                                      |");
    color(Color::White);
    println!("|  [OK] Kernel services online       [OK] Filesystem mounted                  |");
    println!("|  [OK] Keyboard input ready         [OK] Cooperative tasks ready              |");
    println!("|                                                                              |");
    color(Color::LightCyan);
    println!("|  QUICK COMMANDS                                                              |");
    color(Color::White);
    println!("|  help       list available commands                                          |");
    println!("|  ls         browse the current directory                                    |");
    println!("|  edit <file> open the full-screen editor                                    |");
    println!("|  mem        inspect memory and filesystem usage                             |");
    println!("|  clear      clear the screen                                                |");
    println!("|                                                                              |");
    color(Color::Yellow);
    println!("|  Location: {:<64}|", cwd);
    color(Color::White);
    println!("|  Type a command below. Press Ctrl+L to clear the screen.                    |");
    color(Color::LightCyan);
    println!("+------------------------------------------------------------------------------+");
    color(Color::White);
}

fn color(fg: Color) {
    WRITER.lock().set_color(fg, Color::Black);
}
