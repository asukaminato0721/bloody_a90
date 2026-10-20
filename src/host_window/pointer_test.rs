//! Pointer input for the private KWin fixture. No kernel device is created.
use std::{
    ffi::{c_char, c_void},
    os::fd::IntoRawFd,
    time::{Duration, Instant},
};
use zbus::blocking::{Connection, Proxy};
type Ptr = *mut c_void;
const ABSOLUTE: u32 = 1 << 1;
const BUTTON: u32 = 1 << 5;

struct Api {
    new: unsafe extern "C" fn(Ptr) -> Ptr,
    unref: unsafe extern "C" fn(Ptr) -> Ptr,
    name: unsafe extern "C" fn(Ptr, *const c_char),
    setup: unsafe extern "C" fn(Ptr, i32) -> i32,
    dispatch: unsafe extern "C" fn(Ptr),
    event: unsafe extern "C" fn(Ptr) -> Ptr,
    event_type: unsafe extern "C" fn(Ptr) -> u32,
    event_unref: unsafe extern "C" fn(Ptr) -> Ptr,
    seat: unsafe extern "C" fn(Ptr) -> Ptr,
    device: unsafe extern "C" fn(Ptr) -> Ptr,
    bind: unsafe extern "C" fn(Ptr, ...),
    has: unsafe extern "C" fn(Ptr, u32) -> bool,
    device_ref: unsafe extern "C" fn(Ptr) -> Ptr,
    device_unref: unsafe extern "C" fn(Ptr) -> Ptr,
    start: unsafe extern "C" fn(Ptr, u32),
    stop: unsafe extern "C" fn(Ptr),
    motion: unsafe extern "C" fn(Ptr, f64, f64),
    button: unsafe extern "C" fn(Ptr, u32, bool),
    frame: unsafe extern "C" fn(Ptr, u64),
    now: unsafe extern "C" fn(Ptr) -> u64,
    _library: libloading::Library,
}

impl Api {
    fn load() -> Result<Self, String> {
        unsafe {
            let library = libloading::Library::new("libei.so.1").map_err(|e| e.to_string())?;
            macro_rules! symbol {
                ($name:literal) => {
                    *library
                        .get(concat!($name, "\0").as_bytes())
                        .map_err(|e| e.to_string())?
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
                seat: symbol!("ei_event_get_seat"),
                device: symbol!("ei_event_get_device"),
                bind: symbol!("ei_seat_bind_capabilities"),
                has: symbol!("ei_device_has_capability"),
                device_ref: symbol!("ei_device_ref"),
                device_unref: symbol!("ei_device_unref"),
                start: symbol!("ei_device_start_emulating"),
                stop: symbol!("ei_device_stop_emulating"),
                motion: symbol!("ei_device_pointer_motion_absolute"),
                button: symbol!("ei_device_button_button"),
                frame: symbol!("ei_device_frame"),
                now: symbol!("ei_now"),
                _library: library,
            })
        }
    }
}

pub(super) struct Pointer {
    api: Api,
    context: Ptr,
    devices: Vec<(Ptr, bool)>,
    sequence: u32,
    pressed: bool,
    _bus: Connection,
}

impl Pointer {
    pub(super) fn connect() -> Result<Self, String> {
        if std::env::var("A90_TEST_KWIN_ISOLATED").as_deref() != Ok("1")
            || std::env::var("WAYLAND_DISPLAY").as_deref() != Ok("a90-private")
            || !std::env::var("XDG_RUNTIME_DIR")
                .unwrap_or_default()
                .starts_with("/tmp/a90-window-check.")
        {
            return Err("指针检查必须使用独立 KWin 会话".into());
        }
        let bus = zbus::blocking::connection::Builder::session()
            .and_then(|b| b.method_timeout(Duration::from_secs(2)).build())
            .map_err(|e| e.to_string())?;
        let proxy = Proxy::new(
            &bus,
            "org.kde.KWin",
            "/org/kde/KWin/EIS/RemoteDesktop",
            "org.kde.KWin.EIS.RemoteDesktop",
        )
        .map_err(|e| e.to_string())?;
        let (fd, _cookie): (zbus::zvariant::OwnedFd, i32) = proxy
            .call("connectToEIS", &2i32)
            .map_err(|e| e.to_string())?;
        let api = Api::load()?;
        let context = unsafe { (api.new)(std::ptr::null_mut()) };
        if context.is_null() {
            return Err("无法创建测试指针".into());
        }
        drop(proxy);
        let mut pointer = Self {
            api,
            context,
            devices: Vec::new(),
            sequence: 0,
            pressed: false,
            _bus: bus,
        };
        let fd: std::os::fd::OwnedFd = fd.into();
        unsafe {
            (pointer.api.name)(context, c"A90 private divider check".as_ptr());
            if (pointer.api.setup)(context, fd.into_raw_fd()) != 0 {
                return Err("无法初始化测试指针".into());
            }
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            pointer.pump();
            if pointer.ready(ABSOLUTE).is_some() && pointer.ready(BUTTON).is_some() {
                return Ok(pointer);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Err("独立 KWin 没有提供指针设备".into())
    }

    fn ready(&self, capability: u32) -> Option<Ptr> {
        self.devices
            .iter()
            .find(|(d, ready)| *ready && unsafe { (self.api.has)(*d, capability) })
            .map(|(d, _)| *d)
    }

    fn pump(&mut self) {
        unsafe {
            (self.api.dispatch)(self.context);
            loop {
                let event = (self.api.event)(self.context);
                if event.is_null() {
                    break;
                }
                match (self.api.event_type)(event) {
                    3 => (self.api.bind)(
                        (self.api.seat)(event),
                        ABSOLUTE,
                        BUTTON,
                        std::ptr::null::<c_void>(),
                    ),
                    5 => {
                        self.devices
                            .push(((self.api.device_ref)((self.api.device)(event)), false));
                    }
                    6 => {
                        let removed = (self.api.device)(event);
                        self.devices.retain(|(d, _)| {
                            if *d == removed {
                                (self.api.device_unref)(*d);
                                false
                            } else {
                                true
                            }
                        });
                    }
                    7 | 8 => {
                        let kind = (self.api.event_type)(event);
                        let device = (self.api.device)(event);
                        for (d, ready) in &mut self.devices {
                            if *d == device {
                                *ready = kind == 8;
                                if *ready {
                                    self.sequence += 1;
                                    (self.api.start)(*d, self.sequence);
                                }
                            }
                        }
                    }
                    _ => {}
                }
                (self.api.event_unref)(event);
            }
        }
    }

    pub(super) fn motion(&mut self, x: f64, y: f64) {
        self.pump();
        let device = self
            .ready(ABSOLUTE)
            .expect("private absolute pointer is unavailable");
        unsafe {
            (self.api.motion)(device, x, y);
            (self.api.frame)(device, (self.api.now)(self.context));
        }
        self.pump();
        std::thread::sleep(Duration::from_millis(20));
    }

    pub(super) fn button(&mut self, down: bool) {
        self.pump();
        let device = self
            .ready(BUTTON)
            .expect("private pointer buttons are unavailable");
        unsafe {
            (self.api.button)(device, 0x110, down);
            (self.api.frame)(device, (self.api.now)(self.context));
        }
        self.pressed = down;
        self.pump();
        std::thread::sleep(Duration::from_millis(20));
    }
}

impl Drop for Pointer {
    fn drop(&mut self) {
        unsafe {
            if self.pressed
                && let Some(device) = self.ready(BUTTON)
            {
                (self.api.button)(device, 0x110, false);
                (self.api.frame)(device, (self.api.now)(self.context));
            }
            for (device, ready) in self.devices.drain(..) {
                if ready {
                    (self.api.stop)(device);
                }
                (self.api.device_unref)(device);
            }
            (self.api.dispatch)(self.context);
            (self.api.unref)(self.context);
        }
    }
}
