#!/usr/bin/env bash
# Compare Rust's direction classifier against two pure functions in the EXE.
set -euo pipefail
if [[ $# != 1 ]]; then
    printf 'Usage: bash %s /path/to/BloodyWorkShop8.exe\n' "$0" >&2
    exit 2
fi
a90_project="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
a90_exe="$(realpath -- "$1")"
a90_hash="$(sha256sum -- "$a90_exe")"
if [[ "${a90_hash%% *}" != 4a76f6ef4b6e7da699afbd8d3d780ae9747b599af795531a39e3eb5d93c93162 ]]; then
    printf 'The EXE hash does not match the verified function addresses.\n' >&2
    exit 2
fi
a90_probe_root="$(mktemp -d /tmp/a90-direction-check.XXXXXX)"
mkdir -m 700 "$a90_probe_root/wine"
stop_wine() {
    env WINEPREFIX="$a90_probe_root/wine" wineserver -k 2>/dev/null || true
    timeout 10s env WINEPREFIX="$a90_probe_root/wine" wineserver -w 2>/dev/null || true
}
trap stop_wine EXIT
llvm-dlltool -m i386 -k -d "$a90_project/contrib/research/a90-direction-probe.def" -l "$a90_probe_root/kernel32.a"
clang --target=i686-pc-windows-gnu -std=c11 -O1 -Wall -Wextra -Werror \
    -ffreestanding -fno-builtin -fno-stack-protector \
    -c "$a90_project/contrib/research/a90-direction-probe.c" -o "$a90_probe_root/probe.o"
ld -mi386pe --subsystem console --image-base 0x30000000 --entry _probe_start \
    -o "$a90_probe_root/probe.exe" "$a90_probe_root/probe.o" "$a90_probe_root/kernel32.a"
a90_windows_exe="Z:${a90_exe//\//\\}"
if ! xvfb-run -a env WINEPREFIX="$a90_probe_root/wine" WINEDEBUG=-all \
    WINEDLLOVERRIDES=winemenubuilder.exe=d wine "$a90_probe_root/probe.exe" "$a90_windows_exe" \
    > "$a90_probe_root/reference.tsv" 2> "$a90_probe_root/wine.log"; then
    printf 'Native math probe failed. Results: %s\n' "$a90_probe_root" >&2
    exit 1
fi
stop_wine
cd -- "$a90_project"
if ! A90_DIRECTION_REFERENCE="$a90_probe_root/reference.tsv" cargo test --locked --test gesture_direction \
    full_original_direction_reference -- --ignored --nocapture; then
    printf 'Direction comparison failed. Reference: %s\n' "$a90_probe_root/reference.tsv" >&2
    exit 1
fi
a90_after="$(sha256sum -- "$a90_exe")"
[[ "$a90_after" == "$a90_hash" ]]
trap - EXIT
rm -rf -- "$a90_probe_root"
