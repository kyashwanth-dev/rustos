# RustOS v0.2

A hobby x86_64 kernel in Rust: interrupts, keyboard, paging, heap, block
storage, a filesystem persistence layer, a full-screen text editor, and a shell.

## Run it

**Fastest (no Rust needed):** install QEMU, then
- Windows: `powershell -ExecutionPolicy Bypass -File run-prebuilt.ps1`
- Mac/Linux: `./run-prebuilt.sh`

**From source** (needs the nightly toolchain, `cargo install bootimage`, QEMU):
```
cargo clean
cargo run
```

## Shell commands
Files: `ls [dir]  cat f  write f text  touch f  mkdir d  rm [-r] p  cp a b  mv a b  cd d  pwd  tree [dir]  echo text > f  (or >>)`
Editor: `edit f` (Ctrl+S save, Ctrl+X exit; press Ctrl+X twice to discard changes)
System: `uptime  tasks  sleep <seconds>  spawn <seconds>  cancel <task-id>  ps  proc <name>  kill <pid>  mem  history  run <script>  userdemo  color <name>  clear  gui  rev  about  reboot  panic  fault`

## Keys
Up/Down = history, Left/Right/Home/End = edit the line, Delete/Backspace,
PgUp/PgDn = scroll back, Ctrl+L clear, Ctrl+C cancel, Ctrl+U kill line,
Ctrl+A / Ctrl+E = start / end of line. Caps Lock and Shift work.

## Notes
- The filesystem probes the primary-master ATA disk at boot and falls back to
  the memory disk when no usable ATA device is present. The fallback resets on
  reboot.
- Filesystem snapshots use two alternating journal slots with generation
  numbers and checksums. An interrupted write falls back to the last complete
  snapshot.
- `sleep` and `spawn` create cooperative async tasks. `tasks` shows their
  sleeping, running, finished, or cancelled lifecycle state.
- `process.rs`, `memory.rs`, and `syscall.rs` provide validated process
  lifecycle, per-process user page tables, image loading, ring-3 launch/return
  contexts, and syscall ABI validation. `userdemo` allocates and maps a
  validated user image while preserving kernel mappings, activates its CR3,
  enters ring 3 with `iretq`, executes a user `int 0x80`, and returns through
  the IDT. The syscall ABI now reads `rax/rdi/rsi/rdx`, validates active
  process ranges, returns values in `rax`, and supports `getpid`, `write`,
  `yield`, and `exit`. The address-space manager also exposes the CR3
  activation primitive for the eventual ring-3 scheduler.
- `gui` opens a lightweight interactive text desktop. Use arrow keys or
  `Tab` to select Files, Tasks, System, or Shell; press `Enter` to open a
  panel and `Esc` to return to the command prompt.
- Source layout: `vga.rs` screen+scrollback, `keyboard.rs` scancodes->keys,
  `fs.rs` filesystem, `editor.rs` editor, `shell.rs` commands, `memory.rs` +
  `allocator.rs` paging and heap, `gdt.rs`/`interrupts.rs`/`pic.rs` CPU+hardware.
