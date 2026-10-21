#!/usr/bin/env bash
# Test text input in a private headless Sway session. No physical input devices.
set -euo pipefail
a90_project="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
a90_sway="${A90_SWAY_BIN:-sway}"
a90_swaymsg="${A90_SWAYMSG_BIN:-swaymsg}"
a90_sway="$(command -v -- "$a90_sway")"
a90_swaymsg="$(command -v -- "$a90_swaymsg")"
a90_root="$(mktemp -d /tmp/a90-virtual-keyboard-XXXXXX)"
a90_pid=""
stop_session() {
    if [[ -z "$a90_pid" ]]; then return; fi
    kill -TERM -- "-$a90_pid" 2>/dev/null || true
    for a90_attempt in {1..20}; do
        if ! kill -0 -- "-$a90_pid" 2>/dev/null; then break; fi
        sleep 0.05
    done
    kill -KILL -- "-$a90_pid" 2>/dev/null || true
    a90_pid=""
}
trap stop_session EXIT
trap 'exit 130' INT TERM
mkdir -m 700 "$a90_root/runtime" "$a90_root/config" "$a90_root/cache" "$a90_root/data"
export A90_VK_PROJECT="$a90_project" A90_VK_RESULT="$a90_root/result"
export A90_TEST_SWAY_ISOLATED=1 A90_TEST_SWAYMSG="$a90_swaymsg"
export XDG_RUNTIME_DIR="$a90_root/runtime" XDG_CONFIG_HOME="$a90_root/config"
export XDG_CACHE_HOME="$a90_root/cache" XDG_DATA_HOME="$a90_root/data"
export XDG_CURRENT_DESKTOP=sway XDG_SESSION_TYPE=wayland
export WLR_BACKENDS=headless WLR_HEADLESS_OUTPUTS=1 WLR_RENDERER=pixman
export QT_QPA_PLATFORMTHEME=generic QT_NO_XDG_DESKTOP_PORTAL=1
unset DISPLAY WAYLAND_DISPLAY WAYLAND_SOCKET SWAYSOCK AT_SPI_BUS_ADDRESS
cat > "$a90_root/session.sh" <<'SESSION'
#!/usr/bin/env bash
cd -- "$A90_VK_PROJECT" || exit 2
cargo test --locked --lib host_text::virtual_keyboard::tests::isolated_sway_types_unicode_into_qt_and_cancels -- --ignored --nocapture
a90_status=$?
printf '%s\n' "$a90_status" > "$A90_VK_RESULT"
"$A90_TEST_SWAYMSG" exit >/dev/null
exit "$a90_status"
SESSION
chmod 700 "$a90_root/session.sh"
cat > "$a90_root/sway.conf" <<CONFIG
xwayland disable
output HEADLESS-1 resolution 1024x768
seat seat0 fallback true
default_border none
focus_follows_mouse no
exec $a90_root/session.sh
CONFIG
setsid --wait timeout --foreground 60s dbus-run-session -- "$a90_sway" --config "$a90_root/sway.conf" > "$a90_root/sway.log" 2>&1 &
a90_pid=$!
wait "$a90_pid" || true
stop_session
rg 'test |test result:|panicked|mismatch|^error|^    Finished|^   Compiling|^running|called .*Result' "$a90_root/sway.log" || true
if [[ -f "$a90_root/result" && "$(cat "$a90_root/result")" == 0 ]]; then
    rm -rf -- "$a90_root"
else
    printf 'Virtual keyboard check failed. Logs: %s\n' "$a90_root" >&2
    exit 1
fi
