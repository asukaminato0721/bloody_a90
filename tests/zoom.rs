use bloody_a90::{
    gestures::Action,
    host_program::{self, Output, Vm},
};
use std::{
    fs,
    io::{BufRead, BufReader},
    os::unix::fs::DirBuilderExt,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use x11rb::{
    connection::Connection,
    protocol::{
        xproto::{AtomEnum, ConnectionExt as _, InputFocus},
        xtest::ConnectionExt as _,
    },
};

struct Files(std::path::PathBuf);
impl Drop for Files {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct Recipient(Child);
impl Drop for Recipient {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "requires isolated Xvfb, c++, pkg-config and Qt6Widgets; changes only its own widget"]
fn application_zoom_changes_qt_font_and_releases_control() {
    let (connection, screen) = x11rb::connect(None).unwrap();
    let root = connection.setup().roots[screen].root;
    let check = connection
        .intern_atom(false, b"_NET_SUPPORTING_WM_CHECK")
        .unwrap()
        .reply()
        .unwrap()
        .atom;
    let wm = connection
        .get_property(false, root, check, AtomEnum::WINDOW, 0, 1)
        .unwrap()
        .reply()
        .unwrap();
    assert!(
        wm.value.is_empty(),
        "run this test on an isolated Xvfb server"
    );
    let root = std::env::temp_dir().join(format!(
        "a90-zoom-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let files = Files(root);
    for directory in ["config", "cache", "data", "runtime"] {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(files.0.join(directory))
            .unwrap();
    }
    let flags = Command::new("pkg-config")
        .args(["--cflags", "--libs", "Qt6Widgets"])
        .output()
        .unwrap();
    assert!(
        flags.status.success(),
        "Qt6Widgets development files are required"
    );
    let executable = files.0.join("recipient");
    let compilation = Command::new("c++")
        .args(["-std=c++17", "-Wall", "-Wextra", "-Werror", "-fPIC"])
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/zoom_widget.cpp"
        ))
        .args(
            std::str::from_utf8(&flags.stdout)
                .unwrap()
                .split_whitespace(),
        )
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        compilation.status.success(),
        "{}",
        String::from_utf8_lossy(&compilation.stderr)
    );
    let log = files.0.join("wheel-events");
    let mut recipient = Recipient(
        Command::new(executable)
            .arg(&log)
            .env("QT_QPA_PLATFORM", "xcb")
            .env("QT_QPA_PLATFORMTHEME", "generic")
            .env("QT_NO_XDG_DESKTOP_PORTAL", "1")
            .env("QT_IM_MODULE", "compose")
            .env("XDG_CONFIG_HOME", files.0.join("config"))
            .env("XDG_CACHE_HOME", files.0.join("cache"))
            .env("XDG_DATA_HOME", files.0.join("data"))
            .env("XDG_RUNTIME_DIR", files.0.join("runtime"))
            .env_remove("AT_SPI_BUS_ADDRESS")
            .env_remove("WAYLAND_DISPLAY")
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let mut stdout = BufReader::new(recipient.0.stdout.take().unwrap());
    let mut ready = String::new();
    stdout.read_line(&mut ready).unwrap();
    let window: u32 = ready
        .strip_prefix("ready ")
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    connection
        .set_input_focus(InputFocus::PARENT, window, x11rb::CURRENT_TIME)
        .unwrap()
        .check()
        .unwrap();
    connection
        .warp_pointer(x11rb::NONE, window, 0, 0, 0, 0, 100, 100)
        .unwrap()
        .check()
        .unwrap();
    for object in [0, 1] {
        let now = Instant::now();
        let mut vm = Vm::new(
            host_program::prepare(&Action::new(22, object, 3, "缩放")).unwrap(),
            now,
            false,
        );
        let mut events = vm.tick(now, [0; 3]).unwrap();
        events.extend(vm.tick(now + Duration::from_millis(1), [0; 3]).unwrap());
        for event in events {
            match event {
                Output::Key(29, down) => connection
                    .xtest_fake_input(
                        if down { 2 } else { 3 },
                        37,
                        x11rb::CURRENT_TIME,
                        x11rb::NONE,
                        0,
                        0,
                        0,
                    )
                    .unwrap()
                    .check()
                    .unwrap(),
                Output::Wheel(0, y) if y == 1 || y == -1 => {
                    let button = if y == 1 { 4 } else { 5 };
                    for kind in [4, 5] {
                        connection
                            .xtest_fake_input(
                                kind,
                                button,
                                x11rb::CURRENT_TIME,
                                x11rb::NONE,
                                0,
                                0,
                                0,
                            )
                            .unwrap()
                            .check()
                            .unwrap();
                    }
                }
                unexpected => panic!("unexpected zoom output: {unexpected:?}"),
            }
        }
        connection.flush().unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let lines = fs::read_to_string(&log).unwrap_or_default();
            if lines.lines().count() == (object + 1) as usize {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "wheel input did not reach Qt: {lines:?}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    let log = fs::read_to_string(log).unwrap();
    let rows: Vec<Vec<f64>> = log
        .lines()
        .map(|line| {
            line.split_whitespace()
                .map(|v| v.parse().unwrap())
                .collect()
        })
        .collect();
    assert_eq!(rows.len(), 2, "{log}");
    assert_eq!(&rows[0][..2], [120., 1.]);
    assert_eq!(&rows[1][..2], [-120., 1.]);
    assert!(
        rows[0][3] > rows[0][2],
        "zoom in did not increase Qt font: {log}"
    );
    assert_eq!(rows[1][2], rows[0][3]);
    assert_eq!(
        rows[1][3], rows[0][2],
        "zoom out did not restore Qt font: {log}"
    );
    let keys = connection.query_keymap().unwrap().reply().unwrap().keys;
    assert_eq!(
        keys[37 / 8] & (1 << (37 % 8)),
        0,
        "Control must be released"
    );
}
