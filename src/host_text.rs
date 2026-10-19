//! Simulator 21. Text keysyms do not depend on the user's layout.
mod virtual_keyboard;
use std::{
    collections::VecDeque,
    ffi::{c_char, c_void},
    os::fd::{IntoRawFd, OwnedFd},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan(Arc<[u32]>);

impl Plan {
    pub fn prepare(text: &str) -> Result<Self, String> {
        if text.len() > 65536 || text.contains('\0') {
            return Err("输入文字不能超过 64 KiB，也不能包含空字符".into());
        }
        if text.chars().any(|ch| matches!(ch, '\u{1}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' | '\u{fffe}' | '\u{ffff}')) {
            return Err("输入文字包含手势文件不支持的控制字符".into());
        }
        // 00460054 skips CR and sends LF as Return. Use scalar values on Linux.
        Ok(Self(
            text.chars()
                .filter_map(|ch| match ch {
                    '\r' => None,
                    '\n' => Some(0xff0d),
                    '\t' => Some(0xff09),
                    '\u{7f}' => Some(0xffff),
                    ' '..='~' | '\u{a0}'..='\u{ff}' => Some(u32::from(ch)),
                    ch => Some(0x0100_0000 | u32::from(ch)),
                })
                .collect(),
        ))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

type Ptr = *mut c_void;
const TEXT: u32 = 1 << 6;

// Signatures and enum values come from libei 1.6's public libei.h.
// The library remains loaded until every context, event and device is released.
struct Api {
    new: unsafe extern "C" fn(Ptr) -> Ptr,
    unref: unsafe extern "C" fn(Ptr) -> Ptr,
    name: unsafe extern "C" fn(Ptr, *const c_char),
    setup: unsafe extern "C" fn(Ptr, i32) -> i32,
    dispatch: unsafe extern "C" fn(Ptr),
    event: unsafe extern "C" fn(Ptr) -> Ptr,
    event_type: unsafe extern "C" fn(Ptr) -> u32,
    event_unref: unsafe extern "C" fn(Ptr) -> Ptr,
    event_seat: unsafe extern "C" fn(Ptr) -> Ptr,
    event_device: unsafe extern "C" fn(Ptr) -> Ptr,
    bind: unsafe extern "C" fn(Ptr, ...),
    has: unsafe extern "C" fn(Ptr, u32) -> bool,
    device_ref: unsafe extern "C" fn(Ptr) -> Ptr,
    device_unref: unsafe extern "C" fn(Ptr) -> Ptr,
    start: unsafe extern "C" fn(Ptr, u32),
    stop: unsafe extern "C" fn(Ptr),
    key: unsafe extern "C" fn(Ptr, u32, bool),
    frame: unsafe extern "C" fn(Ptr, u64),
    now: unsafe extern "C" fn(Ptr) -> u64,
    _library: libloading::Library,
}

impl Api {
    fn load() -> Result<Self, String> {
        // Loading is optional. Other driver functions do not need libei.
        unsafe {
            let library = libloading::Library::new("libei.so.1")
                .map_err(|e| format!("文字输入需要 libei 1.6 或更新版本：{e}"))?;
            macro_rules! symbol {
                ($name:literal) => {
                    *library
                        .get(concat!($name, "\0").as_bytes())
                        .map_err(|e| format!("文字输入需要 libei 1.6 或更新版本：{e}"))?
                };
            }
            Ok(Self {
                new: symbol!("ei_new_sender"),
                unref: symbol!("ei_unref"),
                name: symbol!("ei_configure_name"),
                setup: symbol!("ei_setup_backend_fd"),
                dispatch: symbol!("ei_dispatch"),
                event: symbol!("ei_get_event"),
                event_type: symbol!("ei_event_get_type"),
                event_unref: symbol!("ei_event_unref"),
                event_seat: symbol!("ei_event_get_seat"),
                event_device: symbol!("ei_event_get_device"),
                bind: symbol!("ei_seat_bind_capabilities"),
                has: symbol!("ei_device_has_capability"),
                device_ref: symbol!("ei_device_ref"),
                device_unref: symbol!("ei_device_unref"),
                start: symbol!("ei_device_start_emulating"),
                stop: symbol!("ei_device_stop_emulating"),
                key: symbol!("ei_device_text_keysym"),
                frame: symbol!("ei_device_frame"),
                now: symbol!("ei_now"),
                _library: library,
            })
        }
    }
}

struct Backend {
    api: Api,
    context: Ptr,
    devices: Vec<(Ptr, bool)>,
    // KWin ties the EIS connection to this D-Bus connection's lifetime.
    _bus: Option<zbus::blocking::Connection>,
    sequence: u32,
}

impl Backend {
    fn connect() -> Result<Self, String> {
        let bus = zbus::blocking::connection::Builder::session()
            .and_then(|builder| builder.method_timeout(Duration::from_secs(3)).build())
            .map_err(|e| format!("无法连接文字输入服务：{e}"))?;
        let proxy = zbus::blocking::Proxy::new(
            &bus,
            "org.kde.KWin",
            "/org/kde/KWin/EIS/RemoteDesktop",
            "org.kde.KWin.EIS.RemoteDesktop",
        )
        .map_err(|e| e.to_string())?;
        let (fd, _cookie): (zbus::zvariant::OwnedFd, i32) = proxy
            .call("connectToEIS", &1i32)
            .map_err(|e| format!("文字输入需要支持 EIS 文字设备的 KWin Wayland：{e}"))?;
        Self::from_fd(fd.into(), Some(bus))
    }

    fn from_fd(fd: OwnedFd, bus: Option<zbus::blocking::Connection>) -> Result<Self, String> {
        let api = Api::load()?;
        let context = unsafe { (api.new)(std::ptr::null_mut()) };
        if context.is_null() {
            return Err("无法创建文字输入连接".into());
        }
        let mut backend = Self {
            api,
            context,
            devices: Vec::new(),
            _bus: bus,
            sequence: 0,
        };
        unsafe {
            (backend.api.name)(context, c"Bloody A90 gesture text".as_ptr());
            // libei takes ownership of this descriptor, including on setup failure.
            let result = (backend.api.setup)(context, fd.into_raw_fd());
            if result != 0 {
                return Err(format!("无法初始化文字输入连接：{result}"));
            }
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            backend.pump()?;
            if backend.devices.iter().any(|(_, resumed)| *resumed) {
                return Ok(backend);
            }
            thread::sleep(Duration::from_millis(5));
        }
        Err("桌面未提供可用的 EIS 文字设备。文字输入需要 libei 1.6 和支持此接口的 KWin。".into())
    }

    fn pump(&mut self) -> Result<(), String> {
        unsafe {
            (self.api.dispatch)(self.context);
            loop {
                let event = (self.api.event)(self.context);
                if event.is_null() {
                    break;
                }
                let kind = (self.api.event_type)(event);
                match kind {
                    3 => (self.api.bind)(
                        (self.api.event_seat)(event),
                        TEXT,
                        std::ptr::null::<c_void>(),
                    ),
                    5 => {
                        let device = (self.api.event_device)(event);
                        if (self.api.has)(device, TEXT) {
                            self.devices.push(((self.api.device_ref)(device), false));
                        }
                    }
                    6 => {
                        let device = (self.api.event_device)(event);
                        self.devices.retain(|(existing, _)| {
                            if *existing == device {
                                (self.api.device_unref)(*existing);
                                false
                            } else {
                                true
                            }
                        });
                    }
                    7 | 8 => {
                        let device = (self.api.event_device)(event);
                        for (existing, resumed) in &mut self.devices {
                            if *existing == device {
                                *resumed = kind == 8;
                            }
                        }
                    }
                    _ => {}
                }
                (self.api.event_unref)(event);
                if kind == 2 {
                    return Err("桌面已断开文字输入连接".into());
                }
            }
        }
        Ok(())
    }

    fn send(&mut self, keysym: u32) -> Result<(), String> {
        let device = self
            .devices
            .iter()
            .find(|(_, ready)| *ready)
            .map(|(device, _)| *device)
            .ok_or("文字输入设备当前不可用")?;
        self.sequence = self.sequence.wrapping_add(1);
        unsafe {
            (self.api.start)(device, self.sequence);
            (self.api.key)(device, keysym, true);
            (self.api.frame)(device, (self.api.now)(self.context));
            (self.api.key)(device, keysym, false);
            (self.api.frame)(device, (self.api.now)(self.context));
            (self.api.stop)(device);
        }
        Ok(())
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        unsafe {
            for (device, _) in self.devices.drain(..) {
                (self.api.device_unref)(device);
            }
            (self.api.unref)(self.context);
        }
    }
}

enum OutputBackend {
    Eis(Backend),
    Virtual(virtual_keyboard::Keyboard),
}

impl OutputBackend {
    fn connect() -> Result<Self, String> {
        if std::env::var_os("WAYLAND_DISPLAY").is_none()
            && std::env::var_os("WAYLAND_SOCKET").is_none()
        {
            return Err(
                "文字输入需要 KWin EIS 或 Wayland 虚拟键盘接口。当前不支持 X11 文字输入。".into(),
            );
        }
        match Backend::connect() {
            Ok(backend) => Ok(Self::Eis(backend)),
            Err(eis_error) => virtual_keyboard::Keyboard::connect()
                .map(Self::Virtual)
                .map_err(|error| format!("{eis_error}；{error}")),
        }
    }

    fn begin(&mut self, plan: &Plan) -> Result<(), String> {
        match self {
            Self::Eis(_) => Ok(()),
            Self::Virtual(backend) => backend.begin(&plan.0),
        }
    }

    fn pump(&mut self) -> Result<(), String> {
        match self {
            Self::Eis(backend) => backend.pump(),
            Self::Virtual(backend) => backend.pump(),
        }
    }

    fn send(&mut self, keysym: u32) -> Result<(), String> {
        match self {
            Self::Eis(backend) => backend.send(keysym),
            Self::Virtual(backend) => backend.send(keysym),
        }
    }
}

pub struct Handle {
    sender: SyncSender<(u64, Arc<Plan>)>,
    results: Receiver<String>,
    stop: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
    thread: Option<JoinHandle<()>>,
}

impl Handle {
    pub fn start() -> Result<Self, String> {
        Self::start_with(OutputBackend::connect)
    }

    fn start_with(
        connect: impl FnOnce() -> Result<OutputBackend, String> + Send + 'static,
    ) -> Result<Self, String> {
        let (sender, requests) = mpsc::sync_channel::<(u64, Arc<Plan>)>(8);
        let (replies, results) = mpsc::sync_channel(16);
        let (ready, started) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let generation = Arc::new(AtomicU64::new(0));
        let stopping = stop.clone();
        let current = generation.clone();
        let thread = thread::spawn(move || {
            let mut backend = match connect() {
                Ok(backend) => {
                    let _ = ready.send(Ok(()));
                    backend
                }
                Err(error) => {
                    let _ = ready.send(Err(error));
                    return;
                }
            };
            let mut pending = VecDeque::new();
            let mut active: Option<(u64, Arc<Plan>, usize)> = None;
            while !stopping.load(Ordering::Acquire) {
                if let Err(error) = backend.pump() {
                    let _ = replies.try_send(error);
                    break;
                }
                // Both queues have an upper bound. Do not drain into an unbounded queue.
                while pending.len() < 8 {
                    match requests.try_recv() {
                        Ok(request) => pending.push_back(request),
                        Err(_) => break,
                    }
                }
                let generation = current.load(Ordering::Acquire);
                if active
                    .as_ref()
                    .is_some_and(|(old, _, _)| *old != generation)
                {
                    active = None;
                }
                if active.is_none() {
                    while let Some((old, plan)) = pending.pop_front() {
                        if old == generation {
                            if let Err(error) = backend.begin(&plan) {
                                let _ = replies.try_send(error);
                                continue;
                            }
                            active = Some((old, plan, 0));
                            break;
                        }
                    }
                }
                if let Some((_, plan, index)) = active.as_mut() {
                    if let Some(keysym) = plan.0.get(*index) {
                        if let Err(error) = backend.send(*keysym) {
                            let _ = replies.try_send(error);
                            active = None;
                        } else {
                            *index += 1;
                        }
                    } else {
                        active = None;
                    }
                }
                // Process commands between characters. Every character releases its key.
                thread::sleep(Duration::from_millis(if active.is_some() { 1 } else { 5 }));
            }
        });
        match started.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(())) => Ok(Self {
                sender,
                results,
                stop,
                generation,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => {
                stop.store(true, Ordering::Release);
                Err("文字输入服务初始化超时".into())
            }
        }
    }

    pub fn send(&self, plan: Arc<Plan>) -> Result<(), String> {
        self.sender
            .try_send((self.generation.load(Ordering::Acquire), plan))
            .map_err(|e| format!("无法提交文字输入：{e}"))
    }
    pub fn cancel_pending(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
    }
    pub fn poll(&self) -> Vec<String> {
        self.results.try_iter().collect()
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Only the private KWin integration test may send the save shortcut.
#[cfg(test)]
pub(crate) fn input_and_save_private_editor_for_test(plan: &Plan) -> Result<(), String> {
    if std::env::var("A90_TEST_KWIN_ISOLATED").as_deref() != Ok("1") {
        return Err("文字保存检查需要独立 KWin 会话".into());
    }
    let mut backend = Backend::connect()?;
    for &keysym in plan.0.iter() {
        backend.send(keysym)?;
    }
    let device = backend
        .devices
        .iter()
        .find(|(_, ready)| *ready)
        .map(|(device, _)| *device)
        .ok_or("测试文字设备不可用")?;
    unsafe {
        (backend.api.start)(device, backend.sequence + 1);
        for (key, down) in [(0xffe3, true), (0x73, true), (0x73, false), (0xffe3, false)] {
            (backend.api.key)(device, key, down);
            (backend.api.frame)(device, (backend.api.now)(backend.context));
        }
        (backend.api.stop)(device);
    }
    backend.pump()?;
    thread::sleep(Duration::from_millis(20));
    backend.pump()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_and_native_line_breaks() {
        assert_eq!(
            &*Plan::prepare("A中😀\r\n\t").unwrap().0,
            &[65, 0x0100_4e2d, 0x0101_f600, 0xff0d, 0xff09]
        );
        assert!(Plan::prepare("\0").is_err());
        assert!(Plan::prepare("\u{1}").is_err());
        assert!(Plan::prepare("\u{ffff}").is_err());
        assert!(Plan::prepare(&"a".repeat(65537)).is_err());
        assert!(Plan::prepare("").unwrap().0.is_empty());
    }

    #[test]
    #[ignore = "requires KWin Wayland and libei 1.6; sends no input"]
    fn kwin_text_device_connects_without_input() {
        let mut backend = Backend::connect().unwrap();
        assert!(backend.devices.iter().any(|(_, resumed)| *resumed));
        backend.pump().unwrap();
        drop(backend);
        let handle = Handle::start().unwrap();
        assert!(handle.poll().is_empty());
        drop(handle);
    }

    #[test]
    #[ignore = "requires cc, pkg-config and libeis 1.6; uses a private EIS socket"]
    fn isolated_unicode_frames_removal_and_disconnect() {
        use std::{
            io::{BufRead, BufReader, Read},
            os::unix::{fs::DirBuilderExt, net::UnixStream},
            process::{Command, Stdio},
        };
        let root = std::env::temp_dir().join(format!(
            "a90-eis-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .unwrap();
        let flags = Command::new("pkg-config")
            .args(["--cflags", "--libs", "libeis-1.0"])
            .output()
            .unwrap();
        assert!(flags.status.success());
        let executable = root.join("server");
        let output = Command::new("cc")
            .args(["-std=c11", "-Wall", "-Wextra", "-Werror"])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/eis_text_server.c"
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
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let socket = root.join("socket");
        let mut child = Command::new(executable)
            .arg(&socket)
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        assert_eq!(line, "ready\n");
        let stream = UnixStream::connect(&socket).unwrap();
        let fd: OwnedFd = stream.into();
        let handle =
            Handle::start_with(move || Backend::from_fd(fd, None).map(OutputBackend::Eis)).unwrap();
        let plan = Plan::prepare("A中😀\r\n\t").unwrap();
        let action = crate::gestures::Action::input_text("A中😀\r\n\t").unwrap();
        let now = Instant::now();
        let mut vm = crate::host_program::Vm::new(
            crate::host_program::prepare(&action).unwrap(),
            now,
            false,
        );
        let outputs = vm.tick(now, [0; 3]).unwrap();
        assert_eq!(outputs.len(), 1);
        let crate::host_program::Output::Text(output) = &outputs[0] else {
            panic!("expected text output")
        };
        handle.send(output.clone()).unwrap();
        let mut report = String::new();
        loop {
            line.clear();
            assert!(
                stdout.read_line(&mut line).unwrap() > 0,
                "server ended before device removal"
            );
            report.push_str(&line);
            if line == "removed\n" {
                break;
            }
        }
        handle.send(Arc::new(Plan::prepare("B").unwrap())).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut errors = Vec::new();
        while errors.is_empty() && Instant::now() < deadline {
            errors.extend(handle.poll());
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(errors, ["文字输入设备当前不可用"]);
        handle.cancel_pending();
        drop(handle);
        stdout.read_to_string(&mut report).unwrap();
        assert!(child.wait().unwrap().success(), "{report}");
        let expected: String = plan
            .0
            .iter()
            .flat_map(|key| [format!("key {key} 1\n"), format!("key {key} 0\n")])
            .collect();
        assert_eq!(report, expected + "removed\nframes 10\ndisconnected 1\n");
        std::fs::remove_dir_all(root).unwrap();
    }
}
