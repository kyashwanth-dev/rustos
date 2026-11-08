//! The shell: line editing, history, and all the commands.

use crate::editor::Editor;
use crate::fs::FileSystem;
use crate::interrupts::TICKS;
use crate::keyboard::Key;
use crate::drivers::{ata::AtaPio, BlockDevice, MemoryDisk};
use crate::storage;
use crate::task::{Scheduler, TaskState};
use crate::process::{enter_user_mode, AddressSpace, ProcessState, ProcessTable, UserImage};
use crate::ui;
use crate::vga::{Color, WRITER};
use crate::{allocator, memory, print, println};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::boxed::Box;
use core::sync::atomic::Ordering;
use x86_64::instructions::port::Port;
use x86_64::VirtAddr;

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
    scheduler: Scheduler,
    processes: ProcessTable,
    shell_process: u64,
    user_memory: Option<UserMemory>,
    user_spaces: Vec<(u64, memory::UserAddressSpace)>,
    gui_mode: bool,
    gui_focus: usize,
    gui_status: String,
}

struct UserMemory {
    physical_memory_offset: VirtAddr,
    frame_allocator: memory::BootInfoFrameAllocator,
}

impl Shell {
    pub fn new() -> Self {
        let disk: Box<dyn BlockDevice> = unsafe { AtaPio::detect_primary_master() }
            .map(|disk| Box::new(disk) as Box<dyn BlockDevice>)
            .unwrap_or_else(|| {
                Box::new(MemoryDisk::new(FS_SECTORS + FS_START_SECTOR)
                    .expect("memory disk initialization failed"))
            });
        Self::with_boxed_disk(disk)
    }

    pub fn with_memory(
        physical_memory_offset: VirtAddr,
        frame_allocator: memory::BootInfoFrameAllocator,
    ) -> Self {
        let disk: Box<dyn BlockDevice> = unsafe { AtaPio::detect_primary_master() }
            .map(|disk| Box::new(disk) as Box<dyn BlockDevice>)
            .unwrap_or_else(|| {
                Box::new(MemoryDisk::new(FS_SECTORS + FS_START_SECTOR)
                    .expect("memory disk initialization failed"))
            });
        let mut shell = Self::with_boxed_disk(disk);
        shell.user_memory = Some(UserMemory { physical_memory_offset, frame_allocator });
        shell
    }

    pub fn with_disk<D: BlockDevice + 'static>(disk: D) -> Self {
        Self::with_boxed_disk(Box::new(disk))
    }

    fn with_boxed_disk(disk: Box<dyn BlockDevice>) -> Self {
        let mut processes = ProcessTable::new();
        let shell_process = processes.create("shell", AddressSpace::kernel_placeholder());
        let _ = processes.switch_to(shell_process);
        Shell {
            fs: FileSystem::new(),
            line: String::new(),
            cursor: 0,
            history: Vec::new(),
            hist_pos: None,
            saved_line: String::new(),
            text_color: Color::White,
            editor: None,
            disk,
            scheduler: Scheduler::new(),
            processes,
            shell_process,
            user_memory: None,
            user_spaces: Vec::new(),
            gui_mode: false,
            gui_focus: 0,
            gui_status: String::from("Ready"),
        }
    }

    /// Advance shell-owned cooperative tasks after each timer tick.
    pub fn poll_tasks(&mut self) {
        self.scheduler.poll();
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
        ui::draw(&self.fs.cwd_string());
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
            b"# These commands run at every boot (edit me with: edit /etc/startup.sh).\n# Files are restored from storage when available.\ncat /etc/motd\n".to_vec(),
            false,
        );
        let _ = fs.write(
            "/home/readme.txt",
            b"RustOS filesystem\n-----------------\nFiles are persisted to the boot disk when available.\nTry:  mkdir stuff   cd stuff   echo hello > hi.txt   cat hi.txt\n      edit notes.txt  (Ctrl+S saves, Ctrl+X exits)\n".to_vec(),
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
        if self.gui_mode {
            self.handle_gui_key(key);
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
                if self.editor.is_none() && !self.gui_mode {
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

    fn handle_gui_key(&mut self, key: Key) {
        match key {
            Key::Escape | Key::Ctrl('c') => self.leave_gui(),
            Key::Left | Key::Up => {
                self.gui_focus = self.gui_focus.saturating_sub(1);
                self.draw_gui();
            }
            Key::Right | Key::Down | Key::Tab => {
                self.gui_focus = (self.gui_focus + 1) % 4;
                self.draw_gui();
            }
            Key::Enter => self.activate_gui_panel(),
            Key::Char('1') | Key::Ctrl('f') => {
                self.gui_focus = 0;
                self.draw_gui();
            }
            Key::Char('2') => {
                self.gui_focus = 1;
                self.draw_gui();
            }
            Key::Char('3') => {
                self.gui_focus = 2;
                self.draw_gui();
            }
            Key::Char('4') => {
                self.gui_focus = 3;
                self.draw_gui();
            }
            _ => {}
        }
    }

    fn activate_gui_panel(&mut self) {
        match self.gui_focus {
            0 => {
                self.gui_status = String::from("Opening file browser...");
                self.draw_gui();
                self.gui_mode = false;
                self.execute("ls", false, 0);
                self.redraw_line();
            }
            1 => {
                self.gui_status = String::from("Opening task monitor...");
                self.draw_gui();
                self.gui_mode = false;
                self.execute("tasks", false, 0);
                self.redraw_line();
            }
            2 => {
                self.gui_status = String::from("Opening system monitor...");
                self.draw_gui();
                self.gui_mode = false;
                self.execute("mem", false, 0);
                self.redraw_line();
            }
            _ => self.leave_gui(),
        }
    }

    fn draw_gui(&self) {
        ui::draw_gui(&self.fs.cwd_string(), self.gui_focus, &self.gui_status);
    }

    fn leave_gui(&mut self) {
        self.gui_mode = false;
        self.gui_status = String::from("Returned to shell");
        ui::leave();
        WRITER.lock().clear();
        self.redraw_line();
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
            "ui" | "desktop" | "gui" => {
                self.gui_mode = true;
                self.gui_focus = 0;
                self.gui_status = String::from("Ready");
                ui::enter();
                self.draw_gui();
            }
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
            "tasks" => {
                if self.scheduler.list().next().is_none() {
                    println!("no tasks");
                } else {
                    for task in self.scheduler.list() {
                        let state = match task.state {
                            TaskState::Sleeping => "sleeping",
                            TaskState::Ready => "ready",
                            TaskState::Running => "running",
                            TaskState::Finished => "finished",
                            TaskState::Cancelled => "cancelled",
                        };
                        println!("[{}] {:<10} {} (wake {})", task.id, state, task.name, task.wake_at);
                    }
                }
            }
            "ps" | "processes" => {
                for process in self.processes.list() {
                    let state = match process.state {
                        ProcessState::Ready => "ready",
                        ProcessState::Running => "running",
                        ProcessState::Exited(code) => {
                            println!("[{}] exited({}) {}", process.id, code, process.name);
                            continue;
                        }
                    };
                    println!("[{}] {:<7} {}", process.id, state, process.name);
                }
            }
            "proc" => {
                if args.is_empty() {
                    println!("usage: proc <name>");
                } else {
                    let id = self.processes.create(args, AddressSpace::kernel_placeholder());
                    println!("process {} created: {}", id, args);
                }
            }
            "kill" => match args.parse::<u64>() {
                Ok(id) if id == self.shell_process => println!("kill: cannot terminate shell"),
                Ok(id) if self.processes.exit(id, 0) => println!("process {} exited", id),
                Ok(_) => println!("kill: no such process"),
                Err(_) => println!("usage: kill <process-id>"),
            },
            "sleep" => match args.parse::<u64>() {
                Ok(seconds) => {
                    let id = self.scheduler.spawn_sleep("shell sleep", seconds);
                    println!("task {} sleeping for {} second(s)", id, seconds);
                }
                Err(_) => println!("usage: sleep <seconds>"),
            },
            "spawn" => match args.parse::<u64>() {
                Ok(seconds) => {
                    let id = self.scheduler.spawn_sleep("async shell task", seconds);
                    println!("task {} spawned for {} second(s)", id, seconds);
                }
                Err(_) => println!("usage: spawn <seconds>"),
            },
            "cancel" => match args.parse::<u64>() {
                Ok(id) if self.scheduler.cancel(id) => println!("task {} cancelled", id),
                Ok(_) => println!("cancel: no such task"),
                Err(_) => println!("usage: cancel <task-id>"),
            },
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
            "userdemo" => {
                let image = UserImage::demo();
                match self.processes.create_user("user-demo", image) {
                    Some(id) => {
                        let process = self.processes.get(id).expect("new process missing");
                        let data_start = process.address_space.data_start;
                        let address_space = process.address_space;
                        let entry_point = process.entry_point;
                        let stack_pointer = process.stack_pointer;
                        let process_name = process.name.clone();
                        let mapped = self.map_user_process(id, address_space, image);
                        println!("user process {} prepared: {}", id, process_name);
                        println!(" entry  {:#x}", entry_point);
                        println!(" stack  {:#x}", stack_pointer);
                        println!(" context: ring3 entry/return frame ready");
                        println!(" page table: {}", if mapped { "mapped" } else { "unavailable" });
                        println!(" syscall gate: int {:#x}, {} entries", crate::interrupts::SYSCALL_VECTOR, crate::syscall::entry_count());
                        println!(" getpid: {:?}", crate::syscall::dispatch(
                            &mut self.processes, id, crate::syscall::SYS_GETPID, 0, 0, 0
                        ));
                        println!(" write(data): {:?}", crate::syscall::dispatch(
                            &mut self.processes, id, crate::syscall::SYS_WRITE,
                            data_start, 1, 0
                        ));
                        if mapped {
                            let context = self.processes.user_context(id).expect("user context missing");
                            crate::syscall::activate_process(id, address_space);
                            let _ = self.processes.switch_to(id);
                            let user_space = self.user_spaces.iter()
                                .find(|(process_id, _)| *process_id == id)
                                .map(|(_, space)| space)
                                .expect("user address space missing");
                            println!(" launching ring-3 image; syscall and timer interrupts are active");
                            unsafe {
                                user_space.activate();
                                enter_user_mode(context);
                            }
                        }
                    }
                    None => println!("userdemo: invalid image"),
                }
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

    fn map_user_process(
        &mut self,
        id: u64,
        address_space: AddressSpace,
        image: UserImage,
    ) -> bool {
        let memory = match self.user_memory.as_mut() {
            Some(memory) => memory,
            None => return false,
        };
        let mut user_space = match memory::UserAddressSpace::new(
            memory.physical_memory_offset,
            &mut memory.frame_allocator,
        ) {
            Some(space) => space,
            None => return false,
        };
        if !user_space.map_image(
            address_space,
            image.code,
            image.data,
            &mut memory.frame_allocator,
        ) {
            return false;
        }
        let root = user_space.root_address();
        if !self.processes.set_page_table_root(id, root) {
            return false;
        }
        self.user_spaces.push((id, user_space));
        true
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
    println!("System:  uptime  tasks  sleep <seconds>  spawn <seconds>  cancel <task-id>  ps  proc <name>  kill <pid>  userdemo");
    println!("         run <script>  color <name>  clear  gui  ui  rev");
    println!("         about  reboot  panic  fault");
    println!("Keys:    Up/Down history, Left/Right/Home/End edit the line,");
    println!("         PgUp/PgDn scroll back, Ctrl+L clear, Ctrl+C cancel, Ctrl+U kill line");
}
