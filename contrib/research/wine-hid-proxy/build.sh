#!/bin/sh
# Build the 32-bit hid.dll proxy. Needs clang and lld; links against Wine's
# kernel32 import library. Install with:
#   cp hid.dll "$WINEPREFIX/drive_c/BloodyWorkShop8/"
#   wine reg add 'HKCU\Software\Wine\AppDefaults\BloodyWorkShop8.exe\DllOverrides' /v hid /d native,builtin /f
# Remove both to undo.
set -e
cd "$(dirname "$0")"
clang --target=i686-pc-windows-msvc -O2 -ffreestanding -fno-stack-protector \
    -fshort-wchar -c hid.c -o hid.obj
lld-link /dll /noentry /nodefaultlib /safeseh:no /machine:x86 /def:hid.def /out:hid.dll \
    hid.obj /usr/lib/wine/i386-windows/libkernel32.a
rm -f hid.obj hid.lib hid.exp
