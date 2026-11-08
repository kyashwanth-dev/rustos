//! The shell: line editing, history, and all the commands.

use crate::editor::Editor;
use crate::fs::FileSystem;
use crate::interrupts::TICKS;
use crate::keyboard::Key;
use crate::drivers::{BlockDevice, MemoryDisk};
use crate::storage;
use crate::vga::{Color, WRITER};
use crate::{allocator, memory, print, println};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::boxed::Box;
use core::sync::atomic::Ordering;
use x86_64::instructions::port::Port;

const MAX_HISTORY: usize = 50;
const MAX_SCRIPT_DEPTH: usize = 3;
const FS_START_SECTOR: u64 = 1;
const FS_SECTORS: u64 = 2047;

pub struct Shell {
    fs: FileSystem,
    line: String,
    cursor: usize,
    history: Vec<String>,
    hist_pos: Option<usize>,
    saved_line: String,
    text_color: Color,
    editor: Option<Editor>,
    disk: Box<dyn BlockDevice>,
}

impl Shell {
    pub fn new() -> Self {
        Self::with_disk(MemoryDisk::new(FS_SECTORS + FS_START_SECTOR).expect("memory disk initialization failed"))
    }

    pub fn with_disk<D: BlockDevice + 'static>(disk: D) -> Self {
        Shell {
            fs: FileSystem::new(),
            line: String::new(),
            cursor: 0,
            history: Vec::new(),
            hist_pos: None,
            saved_line: String::new(),
            text_color: Color::White,
            editor: None,
            disk: Box::new(disk),
        }
    }

    /// Create the default files, run the startup script, show the first prompt.
    pub fn start(&mut self) {
        match storage::load_filesystem(&mut *self.disk, FS_START_SECTOR, FS_SECTORS) {
            Ok(Some(fs)) => {
                self.fs = fs;
                let _ = self.fs.chdir("/home");
            }
            Ok(None) => {
                self.seed_filesystem();
                self.persist();
            }
            Err(error) => {
                println!("storage: unable to load filesystem ({:?}); using a new filesystem", error);
                self.seed_filesystem();
                self.persist();
            }
        }
        set_color(self.text_color);
        self.run_script("/etc/startup.sh", 0);
        self.redraw_line();
    }

    fn seed_filesystem(&mut self) {
        let fs = &mut self.fs;
        for d in ["/etc", "/home", "/tmp"] {
            let _ = fs.mkdir(d);
        }
        let _ = fs.write(
            "/etc/motd",
            b"Welcome to RustOS! Type 'help' for commands.\nTry: ls, cat readme.txt, edit notes.txt, tree /\n".to_vec(),
            false,
        );
        let _ = fs.write(
            "/etc/startup.sh",
            b"# These commands run at every boot (edit me with: edit /etc/startup.sh).\n# The filesystem lives in RAM, so changes reset when you reboot.\ncat /etc/motd\n".to_vec(),
            false,
        );
        let _ = fs.write(
            "/home/readme.txt",
            b"RustOS filesystem\n-----------------\nFiles live in RAM (heap) and vanish on reboot.\nTry:  mkdir stuff   cd stuff   echo hello > hi.txt   cat hi.txt\n      edit notes.txt  (Ctrl+S saves, Ctrl+X exits)\n".to_vec(),
            false,
        );
        let _ = fs.chdir("/home");
    }

    // ---------- keyboard ----------

    pub fn handle_key(&mut self, key: Key) {
        if self.editor.is_some() {
            self.editor_key(key);
            return;
        }
        match key {
            Key::Char(c) => {
                if self.line.len() < self.max_line() {
                    self.line.insert(self.cursor, c);
                    self.cursor += 1;
                    self.hist_pos = None;
                    self.redraw_line();
                }
            }
            Key::Enter => {
                println!();
                let line = core::mem::take(&mut self.line);
                self.cursor = 0;
                self.hist_pos = None;
                self.execute(&line, true, 0);
                if self.editor.is_none() {
                    self.redraw_line();
                }
            }
            Key::Backspace => {
                if self.cursor > 0 {
                    self.line.remove(self.cursor - 1);
                    self.cursor -= 1;
                    self.hist_pos = None;
                    self.redraw_line();
                }
            }
            Key::Delete => {
                if self.cursor < self.line.len() {
                    self.line.remove(self.cursor);
                    self.hist_pos = None;
                    self.redraw_line();
                }
            }
            Key::Left => {
                self.cursor = self.cursor.saturating_sub(1);
                self.redraw_line();
            }
            Key::Right => {
                self.cursor = core::cmp::min(self.cursor + 1, self.line.len());
                self.redraw_line();
            }
            Key::Home | Key::Ctrl('a') => {
                self.cursor = 0;
                self.redraw_line();
            }
            Key::End | Key::Ctrl('e') => {
                self.cursor = self.line.len();
                self.redraw_line();
            }
            Key::Up => self.history_up(),
            Key::Down => self.history_down(),
            Key::PageUp => WRITER.lock().scroll_view(12),
            Key::PageDown => WRITER.lock().scroll_view(-12),
            Key::Ctrl('l') => {
                WRITER.lock().clear();
                self.redraw_line();
            }
            Key::Ctrl('u') => {
                self.line.clear();
                self.cursor = 0;
                self.hist_pos = None;
                self.redraw_line();
            }
            Key::Ctrl('c') => {
                println!("^C");
                self.line.clear();
                self.cursor = 0;
                self.hist_pos = None;
                self.redraw_line();
            }
            _ => {}
        }
    }

    fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let pos = match self.hist_pos {
            None => {
                self.saved_line = self.line.clone();
                self.history.len() - 1
            }
            Some(p) => p.saturating_sub(1),
        };
        self.hist_pos = Some(pos);
        self.line = self.history[pos].clone();
        self.cursor = self.line.len();
        self.redraw_line();
    }

    fn history_down(&mut self) {
        if let Some(p) = self.hist_pos {
            if p + 1 < self.history.len() {
                self.hist_pos = Some(p + 1);
                self.line = self.history[p + 1].clone();
            } else {
                self.hist_pos = None;
                self.line = core::mem::take(&mut self.saved_line);
            }
            self.cursor = self.line.len();
            self.redraw_line();
        }
    }

    fn editor_key(&mut self, key: Key) {
        let done = match self.editor.as_mut() {
            Some(ed) => ed.handle_key(key, &mut self.fs),
            None => return,
        };
        if done {
            self.editor = None;
            self.persist();
            WRITER.lock().redraw();
            self.redraw_line();
        } else if let Some(ed) = &self.editor {
            ed.draw();
        }
    }

    fn persist(&mut self) {
        if let Err(error) = storage::save_filesystem(&mut *self.disk, FS_START_SECTOR, FS_SECTORS, &self.fs) {
            println!("storage: unable to save filesystem ({:?})", error);
        }
    }

    // ---------- prompt ----------

    fn prompt_text(&self) -> String {
        let cwd = self.fs.cwd_string();
        let cwd = if cwd.len() > 40 {
            format!("...{}", &cwd[cwd.len() - 37..])
        } else {
            cwd
        };
        format!("rustos:{}> ", cwd)
    }

    fn max_line(&self) -> usize {
        78usize.saturating_sub(self.prompt_text().len())
    }

    fn redraw_line(&self) {
        let prompt = self.prompt_text();
        WRITER.lock().clear_current_line();
        set_color(Color::LightGreen);
        print!("{}", prompt);
        set_color(self.text_color);
        print!("{}", self.line);
        WRITER.lock().set_col(prompt.len() + self.cursor);
    }

    // ---------- commands ----------

    fn execute(&mut self, line: &str, record: bool, depth: usize) {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return;
        }
        if record {
            if self.history.last().map(|s| s.as_str()) != Some(line) {
                if self.history.len() >= MAX_HISTORY {
                    self.history.remove(0);
                }
                self.history.push(line.to_string());
            }
        }

        let (cmd, args) = match line.split_once(' ') {
            Some((c, a)) => (c, a.trim()),
            None => (line, ""),
        };

        match cmd {
            "help" => print_help(),
            "echo" => match split_redirect(args) {
                Some((text, file, append)) => {
                    let mut data = text.as_bytes().to_vec();
                    data.push(b'\n');
                    let r = self.fs.write(file, data, append);
                    let changed = r.is_ok();
                    ok(cmd, r);
                    if changed { self.persist(); }
                }
                None => println!("{}", args),
            },
            "clear" => WRITER.lock().clear(),
            "pwd" => println!("{}", self.fs.cwd_string()),
            "cd" => {
                let target = if args.is_empty() { "/" } else { args };
                let r = self.fs.chdir(target);
                ok(cmd, r);
            }
            "ls" => {
                let path = if args.is_empty() { "." } else { args };
                if let Some(entries) = ok(cmd, self.fs.list(path)) {
                    for e in entries {
                        if e.is_dir {
                            set_color(Color::LightCyan);
                            println!("{}/", e.name);
                            set_color(self.text_color);
                        } else {
                            println!("{:<24}{:>7} B", e.name, e.size);
                        }
                    }
                }
            }
            "cat" => {
                if args.is_empty() {
                    println!("usage: cat <file>");
                } else if let Some(data) = ok(cmd, self.fs.read(args)) {
                    let text: String = data
                        .iter()
                        .map(|&b| if (0x20..0x7f).contains(&b) || b == b'\n' { b as char } else { '?' })
                        .collect();
                    print!("{}", text);
                    if !text.ends_with('\n') {
                        println!();
                    }
                }
            }
            "write" => match args.split_once(' ') {
                Some((file, text)) => {
                    let mut data = text.as_bytes().to_vec();
                    data.push(b'\n');
                    let r = self.fs.write(file, data, false);
                    let changed = r.is_ok();
                    ok(cmd, r);
                    if changed { self.persist(); }
                }
                None => println!("usage: write <file> <text>"),
            },
            "touch" => {
                for path in args.split_whitespace() {
                    if !self.fs.exists(path) {
                        let r = self.fs.write(path, Vec::new(), false);
                        let changed = r.is_ok();
                        ok(cmd, r);
                        if changed { self.persist(); }
                    }
                }
            }
            "mkdir" => {
                for path in args.split_whitespace() {
                    let r = self.fs.mkdir(path);
                    let changed = r.is_ok();
                    ok(cmd, r);
                    if changed { self.persist(); }
                }
            }
            "rm" => {
                let recursive = args.split_whitespace().any(|a| a == "-r");
                for path in args.split_whitespace().filter(|a| *a != "-r") {
                    let r = self.fs.remove(path, recursive);
                    let changed = r.is_ok();
                    ok(cmd, r);
                    if changed { self.persist(); }
                }
            }
            "cp" | "mv" => {
                let mut it = args.split_whitespace();
                match (it.next(), it.next(), it.next()) {
                    (Some(a), Some(b), None) => {
                        let r = if cmd == "cp" { self.fs.copy(a, b) } else { self.fs.rename(a, b) };
                        let changed = r.is_ok();
                        ok(cmd, r);
                        if changed { self.persist(); }
                    }
                    _ => println!("usage: {} <from> <to>", cmd),
                }
            }
            "tree" => {
                let path = if args.is_empty() { "." } else { args };
                if let Some(rows) = ok(cmd, self.fs.walk(path)) {
                    println!("{}", self.fs.absolute_string(path));
                    for (depth, name, is_dir) in rows {
                        let indent = "  ".repeat(depth + 1);
                        if is_dir {
                            set_color(Color::LightCyan);
                            println!("{}{}/", indent, name);
                            set_color(self.text_color);
                        } else {
                            println!("{}{}", indent, name);
                        }
                    }
                }
            }
            "edit" => {
                if args.is_empty() {
                    println!("usage: edit <file>");
                } else if self.fs.is_dir(args) {
                    println!("edit: {}: is a directory", args);
                } else {
                    let content = if self.fs.exists(args) {
                        match ok(cmd, self.fs.read(args)) {
                            Some(c) => c,
                            None => return,
                        }
                    } else {
                        Vec::new()
                    };
                    let ed = Editor::open(self.fs.absolute_string(args), &content);
                    ed.draw();
                    self.editor = Some(ed);
                }
            }
            "run" | "source" => {
                if args.is_empty() {
                    println!("usage: {} <script>", cmd);
                } else {
                    self.run_script(args, depth);
                }
            }
            "uptime" => {
                // The timer ticks at ~18.2065 Hz (1193182 Hz / 65536).
                let ticks = TICKS.load(Ordering::Relaxed);
                let secs = ticks * 65536 / 1_193_182;
                println!("up {} seconds ({} ticks)", secs, ticks);
            }
            "mem" => {
                let (used, free) = allocator::stats();
                let ram = memory::USABLE_BYTES.load(Ordering::Relaxed);
                println!("usable RAM : {} KiB", ram / 1024);
                println!("heap used  : {} bytes", used);
                println!("heap free  : {} bytes", free);
                println!("files      : {} bytes", self.fs.total_bytes());
            }
            "history" => {
                for (i, entry) in self.history.iter().enumerate() {
                    println!("{:>3}  {}", i + 1, entry);
                }
            }
            "color" => {
                let c = match args {
                    "white" => Some(Color::White),
                    "green" => Some(Color::LightGreen),
                    "cyan" => Some(Color::LightCyan),
                    "pink" => Some(Color::Pink),
                    "yellow" => Some(Color::Yellow),
                    "red" => Some(Color::LightRed),
                    "blue" => Some(Color::LightBlue),
                    _ => None,
                };
                match c {
                    Some(c) => {
                        self.text_color = c;
                        set_color(c);
                    }
                    None => println!("usage: color <white|green|cyan|pink|yellow|red|blue>"),
                }
            }
            "rev" => {
                let reversed: String = args.chars().rev().collect();
                println!("{}", reversed);
            }
            "about" => {
                println!("RustOS - a hobby kernel written in Rust.");
                println!("GDT, IDT, PIC, keyboard, paging, heap, filesystem, editor, shell.");
            }
            "panic" => panic!("you asked for it"),
            "fault" => {
                // Write to an address that isn't mapped to anything.
                unsafe { (0xdead_beef as *mut u8).write_volatile(42) };
            }
            "reboot" => {
                // Ask the keyboard controller to pulse the CPU reset line.
                unsafe { Port::<u8>::new(0x64).write(0xFE) };
                loop {
                    x86_64::instructions::hlt();
                }
            }
            other => println!("unknown command: '{}' (try 'help')", other),
        }
    }

    fn run_script(&mut self, path: &str, depth: usize) {
        if depth >= MAX_SCRIPT_DEPTH {
            println!("source: scripts nested too deeply");
            return;
        }
        if let Some(data) = ok("source", self.fs.read(path)) {
            let text: String = data.iter().map(|&b| b as char).collect();
            for line in text.lines() {
                self.execute(line, false, depth + 1);
            }
        }
    }
}

/// Print an error (if any) and turn a Result into an Option.
fn ok<T>(cmd: &str, r: Result<T, String>) -> Option<T> {
    match r {
        Ok(v) => Some(v),
        Err(e) => {
            println!("{}: {}", cmd, e);
            None
        }
    }
}

/// Split "text > file" or "text >> file" into (text, file, append).
fn split_redirect(args: &str) -> Option<(&str, &str, bool)> {
    let i = args.find('>')?;
    let (text, rest) = (args[..i].trim_end(), &args[i..]);
    let (append, file) = match rest.strip_prefix(">>") {
        Some(f) => (true, f),
        None => (false, &rest[1..]),
    };
    let file = file.trim();
    if file.is_empty() {
        return None;
    }
    Some((text, file, append))
}

fn set_color(fg: Color) {
    WRITER.lock().set_color(fg, Color::Black);
}

fn print_help() {
    println!("Files:   ls [dir]   cat f   write f text   touch f   mkdir d   rm [-r] p");
    println!("         cp a b   mv a b   cd d   pwd   tree [dir]   echo text > f (or >>)");
    println!("Editor:  edit f      (Ctrl+S save, Ctrl+X exit)");
    println!("System:  uptime  mem  history  run <script>  color <name>  clear  rev");
    println!("         about  reboot  panic  fault");
    println!("Keys:    Up/Down history, Left/Right/Home/End edit the line,");
    println!("         PgUp/PgDn scroll back, Ctrl+L clear, Ctrl+C cancel, Ctrl+U kill line");
}
