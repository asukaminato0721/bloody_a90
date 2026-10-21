#!/usr/bin/env bash
# Run the window-action test in a private D-Bus and virtual KWin session.
set -euo pipefail
a90_project="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
a90_check_root="$(mktemp -d /tmp/a90-window-check.XXXXXX)"
a90_session_pid=""
stop_session() {
    if [[ -z "$a90_session_pid" ]]; then return; fi
    # All signals target only the process group that this script created.
    kill -TERM -- "-$a90_session_pid" 2>/dev/null || true
    for a90_attempt in {1..20}; do
        if ! kill -0 -- "-$a90_session_pid" 2>/dev/null; then break; fi
        sleep 0.05
    done
    kill -KILL -- "-$a90_session_pid" 2>/dev/null || true
    a90_session_pid=""
}
trap stop_session EXIT
trap 'exit 130' INT TERM
mkdir -m 700 "$a90_check_root/runtime" "$a90_check_root/config" "$a90_check_root/cache" "$a90_check_root/data"
export A90_WINDOW_CHECK_PROJECT="$a90_project"
export A90_WINDOW_CHECK_RESULT="$a90_check_root/result"
export A90_TEST_KWIN_ISOLATED=1
export XDG_RUNTIME_DIR="$a90_check_root/runtime"
export XDG_CONFIG_HOME="$a90_check_root/config"
export XDG_CACHE_HOME="$a90_check_root/cache"
export XDG_DATA_HOME="$a90_check_root/data"
export LIBGL_ALWAYS_SOFTWARE=1
unset DISPLAY WAYLAND_DISPLAY WAYLAND_SOCKET QT_QPA_PLATFORM
cat > "$a90_check_root/session.sh" <<'SESSION'
#!/usr/bin/env bash
cd -- "$A90_WINDOW_CHECK_PROJECT" || exit 2
printf '%s\n' "$PPID" > "$A90_WINDOW_CHECK_RESULT.kwin-pid"
cargo test --locked --lib isolated_kwin_ -- --ignored --nocapture --test-threads=1
a90_test_status=$?
printf '%s\n' "$a90_test_status" > "$A90_WINDOW_CHECK_RESULT"
exit "$a90_test_status"
SESSION
chmod 700 "$a90_check_root/session.sh"
setsid --wait dbus-run-session -- kwin_wayland --virtual --width 1024 --height 768 --xwayland \
    --socket a90-private --no-lockscreen --no-global-shortcuts --no-kactivities \
    --exit-with-session "$a90_check_root/session.sh" > "$a90_check_root/kwin.log" 2>&1 &
a90_session_pid=$!
wait "$a90_session_pid" || true
stop_session
rg 'test |test result:|panicked|mismatch|^error|^    Finished|^   Compiling|^running|called .*Result' "$a90_check_root/kwin.log" || true
if [[ -f "$a90_check_root/result" && "$(cat "$a90_check_root/result")" == 0 ]]; then
    rm -rf -- "$a90_check_root"
else
    if [[ -r "$a90_check_root/result.kwin-pid" ]]; then
        journalctl --user "_PID=$(cat "$a90_check_root/result.kwin-pid")" --no-pager -n 40 >> "$a90_check_root/kwin.log" 2>&1 || true
    fi
    printf 'Window-action check failed. Logs: %s\n' "$a90_check_root" >&2
    exit 1
fi
