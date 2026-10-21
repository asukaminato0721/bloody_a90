//! Linux evdev and uinput transport. ABI: linux/input.h and linux/uinput.h.
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::{Path, PathBuf},
};

pub const SYN: u16 = 0;
pub const KEY: u16 = 1;
pub const REL: u16 = 2;
pub const REPORT: u16 = 0;
pub const DROPPED: u16 = 3;
pub const X: u16 = 0;
pub const Y: u16 = 1;
pub const HWHEEL: u16 = 6;
pub const WHEEL: u16 = 8;
pub const BUTTONS: [u16; 5] = [0x110, 0x111, 0x112, 0x113, 0x114];

const fn ioctl_code(direction: u64, group: u8, number: u8, size: usize) -> libc::c_ulong {
    ((direction << 30) | (size as u64) << 16 | (group as u64) << 8 | number as u64) as _
}

fn check(result: libc::c_int) -> Result<(), String> {
    if result < 0 {
        Err(std::io::Error::last_os_error().to_string())
    } else {
        Ok(())
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Event {
    time: libc::timeval,
    pub kind: u16,
    pub code: u16,
    pub value: i32,
}

impl Event {
    #[cfg(test)]
    pub(crate) fn timestamp_micros(&self) -> i128 {
        i128::from(self.time.tv_sec) * 1_000_000 + i128::from(self.time.tv_usec)
    }
    pub fn new(kind: u16, code: u16, value: i32) -> Self {
        Self {
            time: libc::timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
            kind,
            code,
            value,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Identity {
    pub bus: u16,
    pub vendor: u16,
    pub product: u16,
    pub version: u16,
}

#[derive(Clone, Debug)]
pub struct PointerInfo {
    pub path: PathBuf,
    pub name: String,
    pub identity: Identity,
}

/// Require a physical USB device, so a virtual mirror cannot feed itself.
pub fn find() -> Result<PointerInfo, String> {
    let entries = fs::read_dir("/sys/class/input").map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with("event") {
            continue;
        }
        let sys = entry.path().join("device");
        if fs::canonicalize(&sys).is_ok_and(|p| p.starts_with("/sys/devices/virtual")) {
            continue;
        }
        let number = |name| {
            fs::read_to_string(sys.join(format!("id/{name}")))
                .ok()
                .and_then(|s| u16::from_str_radix(s.trim(), 16).ok())
        };
        let Some(product) = number("product").filter(|id| crate::device::PRODUCTS.contains(id))
        else {
            continue;
        };
        if number("vendor") != Some(0x09da) {
            continue;
        }
        // Firmware 0c adds a second relative pointer (report 4) to interface 2.
        // Ordinary motion and buttons stay on interface 1.
        if !fs::canonicalize(&sys).is_ok_and(|p| p.to_string_lossy().contains(":1.1/")) {
            continue;
        }
        let rel = fs::read_to_string(sys.join("capabilities/rel"))
            .ok()
            .and_then(|s| {
                s.split_whitespace()
                    .last()
                    .and_then(|s| u64::from_str_radix(s, 16).ok())
            });
        if rel.is_none_or(|bits| bits & 3 != 3) {
            continue;
        }
        return Ok(PointerInfo {
            path: Path::new("/dev/input").join(entry.file_name()),
            name: fs::read_to_string(sys.join("name"))
                .unwrap_or_else(|_| "Bloody A90".into())
                .trim_end_matches(['\r', '\n'])
                .into(),
            identity: Identity {
                bus: number("bustype").unwrap_or(3),
                vendor: 0x09da,
                product,
                version: number("version").unwrap_or(1),
            },
        });
    }
    Err("未找到 A90 的鼠标输入接口".into())
}

pub struct Source {
    file: File,
    grabbed: bool,
}

impl Source {
    pub fn open(path: &Path) -> Result<Self, String> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(path)
            .map_err(|e| format!("无法读取 {}：{e}。请检查 input 设备权限。", path.display()))?;
        Ok(Self {
            file,
            grabbed: false,
        })
    }

    pub fn capabilities(&self, kind: u16, max: usize) -> Result<Vec<u16>, String> {
        let mut bits = vec![0u8; (max + 1).div_ceil(8)];
        let request = ioctl_code(2, b'E', 0x20 + kind as u8, bits.len());
        check(unsafe { libc::ioctl(self.file.as_raw_fd(), request, bits.as_mut_ptr()) })?;
        Ok((0..=max)
            .filter(|n| bits[n / 8] & (1 << (n % 8)) != 0)
            .map(|n| n as u16)
            .collect())
    }

    pub fn held(&self) -> Result<Vec<u16>, String> {
        let mut bits = [0u8; 96];
        check(unsafe {
            libc::ioctl(
                self.file.as_raw_fd(),
                ioctl_code(2, b'E', 0x18, bits.len()),
                bits.as_mut_ptr(),
            )
        })?;
        Ok((0..768)
            .filter(|n| bits[n / 8] & (1 << (n % 8)) != 0)
            .map(|n| n as u16)
            .collect())
    }

    pub fn grab(&mut self) -> Result<(), String> {
        // Discard events which the desktop already received before the grab.
        for _ in 0..128 {
            if self.read()?.is_empty() {
                break;
            }
        }
        if !self.held()?.is_empty() {
            return Err("请松开所有鼠标按键后再启用手势".into());
        }
        check(unsafe { libc::ioctl(self.file.as_raw_fd(), ioctl_code(1, b'E', 0x90, 4), 1) })
            .map_err(|e| format!("无法接管鼠标输入：{e}"))?;
        self.grabbed = true;
        if !self.held()?.is_empty() {
            return Err("鼠标按键状态改变。请松开按键后重试。".into());
        }
        Ok(())
    }

    pub fn read(&mut self) -> Result<Vec<Event>, String> {
        let mut bytes = [0u8; size_of::<Event>() * 64];
        match self.file.read(&mut bytes) {
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(Vec::new()),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => Ok(Vec::new()),
            Err(e) => Err(format!("鼠标输入读取失败：{e}")),
            Ok(0) => Err("鼠标输入接口已断开".into()),
            Ok(length) if length.is_multiple_of(size_of::<Event>()) => Ok(bytes[..length]
                .as_chunks::<{ size_of::<Event>() }>()
                .0
                .iter()
                .map(|chunk| unsafe { std::ptr::read_unaligned(chunk.as_ptr().cast::<Event>()) })
                .collect()),
            Ok(_) => Err("鼠标输入事件的长度无效".into()),
        }
    }

    pub fn wait(&self, milliseconds: i32) -> Result<(), String> {
        let mut fd = libc::pollfd {
            fd: self.file.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let result = unsafe { libc::poll(&mut fd, 1, milliseconds) };
        if result < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            return Ok(());
        }
        check(result)?;
        if fd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err("鼠标输入接口已断开".into());
        }
        Ok(())
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        if self.grabbed {
            unsafe {
                libc::ioctl(self.file.as_raw_fd(), ioctl_code(1, b'E', 0x90, 4), 0);
            }
        }
    }
}

#[repr(C)]
struct Setup {
    identity: Identity,
    name: [u8; 80],
    effects: u32,
}

pub struct Virtual {
    file: File,
    held: BTreeSet<u16>,
    created: bool,
}

impl Virtual {
    pub fn new(name: &str, identity: Identity, keys: &[u16], axes: &[u16]) -> Result<Self, String> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_CLOEXEC)
            .open("/dev/uinput")
            .map_err(|e| format!("无法打开 /dev/uinput：{e}。请检查 uinput 设备权限。"))?;
        let mut result = Self {
            file,
            held: BTreeSet::new(),
            created: false,
        };
        let set = |number, value: u16| {
            check(unsafe {
                libc::ioctl(
                    result.file.as_raw_fd(),
                    ioctl_code(1, b'U', number, 4),
                    value as libc::c_int,
                )
            })
        };
        set(100, KEY)?;
        for key in keys {
            set(101, *key)?;
        }
        if !axes.is_empty() {
            set(100, REL)?;
            for axis in axes {
                set(102, *axis)?;
            }
        }
        let mut setup = Setup {
            identity,
            name: [0; 80],
            effects: 0,
        };
        let length = name.len().min(79);
        setup.name[..length].copy_from_slice(&name.as_bytes()[..length]);
        check(unsafe {
            libc::ioctl(
                result.file.as_raw_fd(),
                ioctl_code(1, b'U', 3, size_of::<Setup>()),
                &setup,
            )
        })?;
        check(unsafe { libc::ioctl(result.file.as_raw_fd(), ioctl_code(0, b'U', 1, 0)) })?;
        result.created = true;
        Ok(result)
    }

    pub fn event(&mut self, kind: u16, code: u16, value: i32) -> Result<(), String> {
        let event = Event::new(kind, code, value);
        let bytes = unsafe {
            std::slice::from_raw_parts((&event as *const Event).cast::<u8>(), size_of::<Event>())
        };
        self.file
            .write_all(bytes)
            .map_err(|e| format!("虚拟输入写入失败：{e}"))?;
        if kind == KEY {
            if value == 0 {
                self.held.remove(&code);
            } else {
                self.held.insert(code);
            }
        }
        Ok(())
    }

    pub fn sync(&mut self) -> Result<(), String> {
        self.event(SYN, REPORT, 0)
    }

    pub fn release_all(&mut self) -> Result<(), String> {
        for code in self.held.clone() {
            self.event(KEY, code, 0)?;
        }
        self.sync()
    }

    pub fn sys_name(&self) -> Result<String, String> {
        let mut bytes = [0u8; 128];
        check(unsafe {
            libc::ioctl(
                self.file.as_raw_fd(),
                ioctl_code(2, b'U', 44, bytes.len()),
                bytes.as_mut_ptr(),
            )
        })?;
        Ok(String::from_utf8_lossy(&bytes)
            .trim_end_matches('\0')
            .into())
    }
}

impl Drop for Virtual {
    fn drop(&mut self) {
        if self.created {
            let _ = self.release_all();
            unsafe {
                libc::ioctl(self.file.as_raw_fd(), ioctl_code(0, b'U', 2, 0));
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn isolated_reader(device: &Virtual) -> Result<Source, String> {
    let directory = Path::new("/sys/devices/virtual/input").join(device.sys_name()?);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let mut error = "虚拟输入设备尚未出现".to_string();
    loop {
        for entry in fs::read_dir(&directory).into_iter().flatten().flatten() {
            if !entry.file_name().to_string_lossy().starts_with("event") {
                continue;
            }
            match Source::open(&Path::new("/dev/input").join(entry.file_name())) {
                Ok(reader) => return Ok(reader),
                Err(message) => error = message,
            }
        }
        if std::time::Instant::now() >= deadline {
            return Err(error);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn linux_abi_layout_and_ioctl_numbers() {
        assert_eq!(size_of::<Identity>(), 8);
        assert_eq!(size_of::<Setup>(), 92);
        assert_eq!(size_of::<Event>(), size_of::<libc::timeval>() + 8);
        assert_eq!(ioctl_code(1, b'E', 0x90, 4), 0x40044590);
        assert_eq!(ioctl_code(1, b'U', 3, 92), 0x405c5503);
    }

    #[test]
    #[ignore = "requires /dev/uinput and /dev/input access; grabs only its own virtual keyboard"]
    fn virtual_device_event_round_trip() {
        let mut virtual_device = Virtual::new(
            "A90 isolated input test",
            Identity {
                bus: 6,
                vendor: 0x1d6b,
                product: 0xa090,
                version: 1,
            },
            &[30],
            &[],
        )
        .unwrap();
        let sys_name = virtual_device.sys_name().unwrap();
        let directory = Path::new("/sys/devices/virtual/input").join(&sys_name);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let path = loop {
            if let Some(path) = fs::read_dir(&directory)
                .into_iter()
                .flatten()
                .flatten()
                .find(|entry| entry.file_name().to_string_lossy().starts_with("event"))
                .map(|entry| Path::new("/dev/input").join(entry.file_name()))
                && path.exists()
            {
                break path;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "virtual device node did not appear"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        let mut reader = loop {
            match Source::open(&path) {
                Ok(reader) => break reader,
                Err(error) => {
                    assert!(std::time::Instant::now() < deadline, "{error}");
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
        };
        reader.grab().unwrap();
        virtual_device.event(KEY, 30, 1).unwrap();
        virtual_device.sync().unwrap();
        reader.wait(1000).unwrap();
        let events = reader.read().unwrap();
        assert!(
            events
                .iter()
                .any(|event| event.kind == KEY && event.code == 30 && event.value == 1)
        );
        assert_eq!(reader.held().unwrap(), [30]);
        virtual_device.release_all().unwrap();
        reader.wait(1000).unwrap();
        assert!(
            reader
                .read()
                .unwrap()
                .iter()
                .any(|event| event.kind == KEY && event.code == 30 && event.value == 0)
        );
        assert!(reader.held().unwrap().is_empty());
        // Destroy the virtual device while its exclusive reader still exists.
        drop(virtual_device);
        assert!(!directory.exists());
    }
}
