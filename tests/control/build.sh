#!/bin/sh
# Builds the 32-bit control program. Requires mingw-w64 (brew install mingw-w64).
set -e
cd "$(dirname "$0")"
i686-w64-mingw32-gcc -O1 -Wall -Wextra -o control.exe main.c -lgdi32 -mwindows
echo "built tests/control/control.exe"
