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
System: `uptime  tasks  sleep <seconds>  spawn <seconds>  cancel <task-id>  ps  proc <name>  kill <pid>  mem  history  run <script>  color <name>  clear  ui  rev  about  reboot  panic  fault`

## Keys
Up/Down = history, Left/Right/Home/End = edit the line, Delete/Backspace,
PgUp/PgDn = scroll back, Ctrl+L clear, Ctrl+C cancel, Ctrl+U kill line,
Ctrl+A / Ctrl+E = start / end of line. Caps Lock and Shift work.

## Notes
- The filesystem probes the primary-master ATA disk at boot and falls back to
  the memory disk when no usable ATA device is present. The fallback resets on
  reboot.
- `sleep` and `spawn` create cooperative async tasks. `tasks` shows their
  sleeping, running, finished, or cancelled lifecycle state.
- `process.rs` and `syscall.rs` provide validated process lifecycle and syscall
  ABI groundwork. `ps`, `proc`, and `kill` expose the kernel process table;
  user-mode page-table switching and a real syscall entry point are not
  implemented yet.
- Source layout: `vga.rs` screen+scrollback, `keyboard.rs` scancodes->keys,
  `fs.rs` filesystem, `editor.rs` editor, `shell.rs` commands, `memory.rs` +
  `allocator.rs` paging and heap, `gdt.rs`/`interrupts.rs`/`pic.rs` CPU+hardware.
