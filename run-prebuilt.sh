#!/bin/sh
# Boots the prebuilt RustOS image in QEMU. No Rust needed - only QEMU.
exec qemu-system-x86_64 -drive format=raw,file="$(dirname "$0")/prebuilt/rustos.img"
