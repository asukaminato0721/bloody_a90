//! Unicode text on compositors that expose zwp_virtual_keyboard_manager_v1.
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs::File,
    io::Write as _,
    os::fd::{AsFd, AsRawFd, FromRawFd},
    time::{Duration, Instant},
};
use wayland_client::{
    Connection, Dispatch, EventQueue, QueueHandle, delegate_noop,
    protocol::{wl_callback, wl_registry, wl_seat},
};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1 as manager, zwp_virtual_keyboard_v1 as keyboard,
};

#[derive(Default)]
struct State {
    manager: Option<(u32, manager::ZwpVirtualKeyboardManagerV1)>,
    seats: BTreeMap<u32, wl_seat::WlSeat>,
    selected: Option<u32>,
    lost: bool,
    synced: u64,
}

fn dispatch(
    connection: &Connection,
    queue: &mut EventQueue<State>,
    state: &mut State,
    timeout: i32,
) -> Result<(), String> {
    connection.flush().map_err(|e| e.to_string())?;
    if let Some(guard) = queue.prepare_read() {
        let mut descriptor = libc::pollfd {
            fd: guard.connection_fd().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        if unsafe { libc::poll(&mut descriptor, 1, timeout) } < 0
            && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        if descriptor.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err("Wayland 文字输入连接已断开".into());
        }
        if descriptor.revents & libc::POLLIN != 0 {
            guard.read().map_err(|e| e.to_string())?;
        }
    }
    queue.dispatch_pending(state).map_err(|e| e.to_string())?;
    if state.lost {
        return Err("Wayland 文字输入设备已移除".into());
    }
    Ok(())
}

fn synchronize(
    connection: &Connection,
    queue: &mut EventQueue<State>,
    state: &mut State,
    timeout: Duration,
) -> Result<(), String> {
    let serial = state.synced + 1;
    connection.display().sync(&queue.handle(), serial);
    let deadline = Instant::now() + timeout;
    while state.synced != serial {
        if Instant::now() >= deadline {
            return Err("Wayland 文字输入接口检查超时".into());
        }
        dispatch(connection, queue, state, 5)?;
    }
    Ok(())
}

pub(super) struct Keyboard {
    connection: Connection,
    queue: EventQueue<State>,
    state: State,
    keyboard: keyboard::ZwpVirtualKeyboardV1,
    codes: BTreeMap<u32, u32>,
    started: Instant,
}

/// One level per key prevents Shift, Caps Lock and the physical layout from
/// changing text. The keymap belongs only to this virtual keyboard.
fn keymap(symbols: &[u32]) -> Result<(String, BTreeMap<u32, u32>), String> {
    if symbols.len() > 65536 {
        return Err("虚拟键盘文字超过 65536 个字符".into());
    }
    let mut codes = BTreeMap::new();
    for &symbol in symbols {
        let next = codes.len() as u32;
        codes.entry(symbol).or_insert(next);
    }
    let mut text = format!(
        "xkb_keymap {{\nxkb_keycodes \"a90\" {{ minimum=8; maximum={};\n",
        (codes.len() as u32 + 7).max(255)
    );
    for code in codes.values() {
        writeln!(text, "<{code:04X}> = {};", code + 8).unwrap();
    }
    text.push_str("};\nxkb_types \"a90\" { type \"ONE_LEVEL\" { modifiers=None; level_name[Level1]=\"Any\"; }; };\nxkb_compatibility \"a90\" {};\nxkb_symbols \"a90\" {\n");
    for (symbol, code) in &codes {
        writeln!(
            text,
            "key <{code:04X}> {{ type=\"ONE_LEVEL\", repeat=no, [ 0x{symbol:08x} ] }};"
        )
        .unwrap();
    }
    text.push_str("};\n};\0");
    Ok((text, codes))
}

impl Keyboard {
    pub(super) fn connect() -> Result<Self, String> {
        let connection =
            Connection::connect_to_env().map_err(|e| format!("无法连接 Wayland 文字输入：{e}"))?;
        let mut queue = connection.new_event_queue();
        connection.display().get_registry(&queue.handle(), ());
        let mut state = State::default();
        synchronize(&connection, &mut queue, &mut state, Duration::from_secs(2))?;
        let manager = &state
            .manager
            .as_ref()
            .ok_or("桌面未提供 Wayland 虚拟键盘接口")?
            .1;
        if state.seats.len() != 1 {
            return Err("Wayland 文字输入需要一个座席。当前未找到座席，或存在多个座席。".into());
        }
        let (&name, seat) = state.seats.first_key_value().unwrap();
        let keyboard = manager.create_virtual_keyboard(seat, &queue.handle(), ());
        state.selected = Some(name);
        let mut result = Self {
            connection,
            queue,
            state,
            keyboard,
            codes: BTreeMap::new(),
            started: Instant::now(),
        };
        // Validate keyboard creation and keymap access before grabbing input.
        // No key event is sent during this check.
        result.install_mapping(&[0x20])?;
        synchronize(
            &result.connection,
            &mut result.queue,
            &mut result.state,
            Duration::from_secs(2),
        )
        .map_err(|e| format!("桌面拒绝了虚拟文字键盘：{e}"))?;
        Ok(result)
    }

    pub(super) fn begin(&mut self, symbols: &[u32]) -> Result<(), String> {
        self.pump()?;
        self.install_mapping(symbols)?;
        self.keyboard.modifiers(0, 0, 0, 0);
        self.connection.flush().map_err(|e| e.to_string())
    }

    fn install_mapping(&mut self, symbols: &[u32]) -> Result<(), String> {
        let (text, codes) = keymap(symbols)?;
        let fd = unsafe { libc::memfd_create(c"a90-text-keymap".as_ptr(), libc::MFD_CLOEXEC) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let mut file = unsafe { File::from_raw_fd(fd) };
        file.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        self.keyboard.keymap(1, file.as_fd(), text.len() as u32);
        self.codes = codes;
        Ok(())
    }

    pub(super) fn send(&mut self, symbol: u32) -> Result<(), String> {
        if self.state.lost {
            return Err("Wayland 文字输入设备已移除".into());
        }
        let code = *self
            .codes
            .get(&symbol)
            .ok_or("文字不在当前虚拟键盘映射中")?;
        let time = self.started.elapsed().as_millis() as u32;
        self.keyboard.key(time, code, 1);
        self.keyboard.key(time, code, 0);
        self.connection.flush().map_err(|e| e.to_string())
    }

    pub(super) fn pump(&mut self) -> Result<(), String> {
        dispatch(&self.connection, &mut self.queue, &mut self.state, 0)
    }
}

impl Drop for Keyboard {
    fn drop(&mut self) {
        self.keyboard.destroy();
        let _ = self.connection.flush();
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name, interface, ..
            } => match interface.as_str() {
                "zwp_virtual_keyboard_manager_v1" => {
                    state.manager = Some((name, registry.bind(name, 1, qh, ())))
                }
                "wl_seat" => {
                    state.seats.insert(name, registry.bind(name, 1, qh, ()));
                }
                _ => {}
            },
            wl_registry::Event::GlobalRemove { name } => {
                if state.selected == Some(name)
                    || state.manager.as_ref().is_some_and(|(id, _)| *id == name)
                {
                    state.lost = true;
                }
                state.seats.remove(&name);
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_callback::WlCallback, u64> for State {
    fn event(
        state: &mut Self,
        _: &wl_callback::WlCallback,
        _: wl_callback::Event,
        serial: &u64,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        state.synced = *serial;
    }
}

delegate_noop!(State: ignore wl_seat::WlSeat);
delegate_noop!(State: ignore manager::ZwpVirtualKeyboardManagerV1);
delegate_noop!(State: ignore keyboard::ZwpVirtualKeyboardV1);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stalled_server_and_disconnect_do_not_block_initialization() {
        let (client, server) = std::os::unix::net::UnixStream::pair().unwrap();
        let connection = Connection::from_socket(client).unwrap();
        let mut queue = connection.new_event_queue();
        let mut state = State::default();
        let now = Instant::now();
        assert!(
            synchronize(
                &connection,
                &mut queue,
                &mut state,
                Duration::from_millis(20)
            )
            .unwrap_err()
            .contains("超时")
        );
        assert!(now.elapsed() < Duration::from_secs(1));
        drop(server);
        assert!(dispatch(&connection, &mut queue, &mut state, 0).is_err());
    }

    #[test]
    fn unicode_keymap_compiles_and_preserves_symbols_above_keycode_255() {
        let mut symbols = crate::host_text::Plan::prepare("Aa中😀\r\n\t")
            .unwrap()
            .0
            .to_vec();
        symbols.extend((0x4e00..0x5200).map(|code| code | 0x0100_0000));
        let (text, codes) = keymap(&symbols).unwrap();
        assert!(codes.len() > 255);
        unsafe {
            type Ptr = *mut std::ffi::c_void;
            let library = libloading::Library::new("libxkbcommon.so.0").unwrap();
            let context_new = library
                .get::<unsafe extern "C" fn(u32) -> Ptr>(b"xkb_context_new\0")
                .unwrap();
            let compile = library
                .get::<unsafe extern "C" fn(Ptr, *const u8, u32, u32) -> Ptr>(
                    b"xkb_keymap_new_from_string\0",
                )
                .unwrap();
            let lookup = library
                .get::<unsafe extern "C" fn(Ptr, u32, u32, u32, *mut *const u32) -> i32>(
                    b"xkb_keymap_key_get_syms_by_level\0",
                )
                .unwrap();
            let unref_map = library
                .get::<unsafe extern "C" fn(Ptr)>(b"xkb_keymap_unref\0")
                .unwrap();
            let unref_context = library
                .get::<unsafe extern "C" fn(Ptr)>(b"xkb_context_unref\0")
                .unwrap();
            let context = context_new(0);
            assert!(!context.is_null());
            let map = compile(context, text.as_ptr(), 1, 0);
            assert!(!map.is_null(), "generated XKB keymap did not compile");
            for (symbol, code) in &codes {
                let mut actual = std::ptr::null();
                assert_eq!(lookup(map, code + 8, 0, 0, &mut actual), 1);
                assert_eq!(*actual, *symbol);
            }
            unref_map(map);
            unref_context(context);
        }
    }

    #[test]
    #[ignore = "requires private headless Sway, c++, pkg-config and Qt6Widgets"]
    fn isolated_sway_types_unicode_into_qt_and_cancels() {
        use crate::{
            gestures::Action,
            host_program::{self, Output, Vm},
            host_text::{Handle, Plan},
        };
        use std::{
            io::{BufRead, BufReader},
            process::{Child, Command, Stdio},
            sync::Arc,
            thread,
            time::Duration,
        };
        assert_eq!(std::env::var("A90_TEST_SWAY_ISOLATED").as_deref(), Ok("1"));
        let root = std::path::PathBuf::from(std::env::var_os("XDG_CACHE_HOME").unwrap());
        let session_root = root.parent().unwrap();
        assert_eq!(session_root.parent().unwrap(), std::path::Path::new("/tmp"));
        assert!(
            session_root
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("a90-virtual-keyboard-")
        );
        struct Recipient(Child);
        impl Drop for Recipient {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let inputs = || {
            let result = Command::new(std::env::var_os("A90_TEST_SWAYMSG").unwrap())
                .args(["-r", "-t", "get_inputs"])
                .output()
                .unwrap();
            assert!(result.status.success());
            String::from_utf8(result.stdout).unwrap()
        };
        assert_eq!(inputs().trim(), "[]");
        let flags = Command::new("pkg-config")
            .args(["--cflags", "--libs", "Qt6Widgets"])
            .output()
            .unwrap();
        assert!(flags.status.success());
        let executable = root.join("text-recipient");
        let build = Command::new("c++")
            .args(["-std=c++17", "-Wall", "-Wextra", "-Werror", "-fPIC"])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/text_widget.cpp"
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
            build.status.success(),
            "{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let file = root.join("typed-text");
        let mut recipient = Recipient(
            Command::new(executable)
                .arg(&file)
                .env("QT_QPA_PLATFORM", "wayland")
                .env("QT_QPA_PLATFORMTHEME", "generic")
                .env("QT_NO_XDG_DESKTOP_PORTAL", "1")
                .env("QT_IM_MODULE", "compose")
                .env_remove("DISPLAY")
                .env_remove("AT_SPI_BUS_ADDRESS")
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let mut ready = String::new();
        BufReader::new(recipient.0.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready, "ready\n");
        let handle = Handle::start().unwrap();
        assert!(inputs().contains("keyboard"));
        let emit = |text: &str| {
            let now = Instant::now();
            let mut vm = Vm::new(
                host_program::prepare(&Action::input_text(text).unwrap()).unwrap(),
                now,
                false,
            );
            let output = vm.tick(now, [0; 3]).unwrap();
            let [Output::Text(plan)] = output.as_slice() else {
                panic!("{output:?}");
            };
            handle.send(plan.clone()).unwrap();
            assert!(vm.finished());
        };
        let wait_text = |expected: &str| {
            let deadline = Instant::now() + Duration::from_secs(3);
            while std::fs::read(&file).ok().as_deref() != Some(expected.as_bytes()) {
                assert!(handle.poll().is_empty());
                assert!(
                    Instant::now() < deadline,
                    "text mismatch: {:?}",
                    std::fs::read_to_string(&file)
                );
                thread::sleep(Duration::from_millis(5));
            }
        };
        emit("A中😀\r\n\tz");
        let mut expected = "A中😀\n\tz".to_string();
        wait_text(&expected);
        // More than 255 distinct symbols verifies that this path does not
        // inherit X11's core keycode limit. Replacing the map preserves order.
        let second: String = (0x4e00..0x4f2c)
            .map(|code| char::from_u32(code).unwrap())
            .collect();
        emit(&second);
        expected.push_str(&second);
        wait_text(&expected);
        handle
            .send(Arc::new(Plan::prepare(&"a".repeat(65536)).unwrap()))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if std::fs::read_to_string(&file).is_ok_and(|text| text.ends_with("aaaa")) {
                break;
            }
            assert!(handle.poll().is_empty());
            assert!(Instant::now() < deadline, "long text did not start");
            thread::sleep(Duration::from_millis(5));
        }
        handle.cancel_pending();
        thread::sleep(Duration::from_millis(40));
        let cancelled = std::fs::read_to_string(&file).unwrap();
        assert!(cancelled.len() < expected.len() + 65536);
        thread::sleep(Duration::from_millis(40));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), cancelled);
        emit("Z");
        wait_text(&(cancelled + "Z"));
        assert!(handle.poll().is_empty());
        drop(handle);
        let deadline = Instant::now() + Duration::from_secs(2);
        while inputs().trim() != "[]" {
            assert!(
                Instant::now() < deadline,
                "virtual keyboard survived shutdown"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }
}
