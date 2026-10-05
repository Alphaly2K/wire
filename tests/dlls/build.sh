#!/bin/sh
set -eu
wire_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
mkdir -p "$wire_root/wire-out/dll-probe"
i686-w64-mingw32-gcc -Os -Wall -Wextra -fno-stack-protector \
    -fno-asynchronous-unwind-tables -shared -nostdlib \
    -Wl,--entry,_DllMain@12 -Wl,--kill-at -Wl,--image-base,0x10000000 \
    "$wire_root/tests/dlls/probe.c" -lkernel32 \
    -o "$wire_root/wire-out/dll-probe/probe.dll"
