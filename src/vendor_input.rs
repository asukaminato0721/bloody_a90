//! Vendor input notifications. EXE 0043dc38 / 0043e1b4, firmware 07 at 09f1.
//! Input reads use a separate descriptor. They do not send feature requests.
use crate::device::{Device, REPORT};
use std::{
    fs::{File, OpenOptions},
    io::Read,
    os::unix::fs::OpenOptionsExt,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State {
    pub buttons: [u8; 3],
    /// One wheel step. Positive means up. Zero means no wheel event.
    pub wheel: i8,
    /// Running configuration index, beginning at zero.
    pub profile: u8,
    /// Running DPI preset index, beginning at zero.
    pub sensitivity: u8,
}

impl State {
    pub fn held_buttons(&self) -> Vec<u8> {
        (0..24)
            .filter(|index| self.buttons[index / 8] & (1 << (index % 8)) != 0)
            .map(|index| index as u8 + 1)
            .collect()
    }
}

/// Report 07, subtype 01. Other reports include keyboard output and replies
/// to feature requests. They must not become button or wheel events.
pub fn parse(report: &[u8]) -> Result<Option<State>, String> {
    if report.first() != Some(&7) || report.get(1) != Some(&1) {
        return Ok(None);
    }
    if report.len() != REPORT {
        return Err(format!(
            "厂商状态通知长度应为 64 字节，实际为 {}",
            report.len()
        ));
    }
    Ok(Some(State {
        buttons: [report[3], report[4], report[5]],
        wheel: match report[2] & 0xf0 {
            0x10 => 1,
            0xf0 => -1,
            _ => 0,
        },
        profile: report[7] & 7,
        sensitivity: report[12] & 7,
    }))
}

pub struct Source {
    file: File,
}

impl Source {
    #[cfg(test)]
    pub(crate) fn fixture(file: File) -> Self {
        Self { file }
    }
    /// Open a passive reader. This does not enable notifications. Use Session
    /// when the caller must temporarily enable and restore notification mode.
    pub fn open() -> Result<Self, String> {
        let device = Device::open()?;
        Self::for_device(&device)
    }

    /// The Device has already checked the interface and installed firmware.
    /// Each hidraw open has its own input queue. The feature lock can remain
    /// held while this descriptor reads input. No ioctl or write is required.
    pub fn for_device(device: &Device) -> Result<Self, String> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(device.path())
            .map_err(|e| format!("无法读取 {} 的厂商通知：{e}", device.path().display()))?;
        Ok(Self { file })
    }

    /// Drain a bounded batch without waiting for hardware. Callers must stop
    /// their actions on an error. A full batch can indicate lost transitions.
    pub fn read(&mut self) -> Result<Vec<State>, String> {
        let mut states = Vec::new();
        let mut report = [0; REPORT];
        for _ in 0..256 {
            match self.file.read(&mut report) {
                Ok(0) => return Err("鼠标厂商通知接口已关闭".into()),
                Ok(size) => {
                    if let Some(state) = parse(&report[..size])? {
                        states.push(state);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(states),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(format!("读取鼠标厂商通知失败：{e}")),
            }
        }
        Err("鼠标厂商通知过多，无法确认全部按键变化".into())
    }
}

/// An exclusive diagnostic session. The borrowed Device keeps the feature
/// lock until the notification mode is restored. Regular GUI configuration
/// access is therefore unavailable for the duration of this session.
pub struct Session<'a> {
    device: &'a Device,
    source: Source,
    previous: bool,
    restored: bool,
}

impl<'a> Session<'a> {
    pub fn start(device: &'a Device) -> Result<Self, String> {
        let source = Source::for_device(device)?;
        let previous = device.ram(0x4b)? & 1 != 0;
        let session = Self {
            device,
            source,
            previous,
            restored: false,
        };
        // Construct the guard before the first write. A failed request may
        // already have changed the running mode.
        device.set_notification_mode(true)?;
        device.request_notification()?;
        Ok(session)
    }

    pub fn read(&mut self) -> Result<Vec<State>, String> {
        self.source.read()
    }

    pub fn restore(&mut self) -> Result<(), String> {
        self.device.set_notification_mode(self.previous)?;
        self.restored = true;
        Ok(())
    }
}

impl Drop for Session<'_> {
    fn drop(&mut self) {
        if !self.restored
            && let Err(error) = self.restore()
        {
            eprintln!("恢复鼠标通知模式失败：{error}");
        }
    }
}

/// One cooperating host-action runtime per user and device. Feature requests
/// use short-lived Device handles. Input reads retain their own descriptor.
pub(crate) struct Monitor {
    source: Source,
    path: std::path::PathBuf,
    previous: bool,
    active: bool,
    _owner: std::os::unix::net::UnixListener,
}

impl Monitor {
    pub(crate) fn start(device: &Device) -> Result<Self, String> {
        use std::os::linux::net::SocketAddrExt;
        let name = format!(
            "bloody-a90-host-{}-{}",
            unsafe { libc::geteuid() },
            device.path().display()
        );
        let address = std::os::unix::net::SocketAddr::from_abstract_name(name.as_bytes())
            .map_err(|e| e.to_string())?;
        let owner = std::os::unix::net::UnixListener::bind_addr(&address)
            .map_err(|e| format!("按键主机动作服务已被占用：{e}"))?;
        let source = Source::for_device(device)?;
        let previous = device.ram(0x4b)? & 1 != 0;
        if let Err(error) = device
            .set_notification_mode(true)
            .and_then(|()| device.request_notification())
        {
            return match device.set_notification_mode(previous) {
                Ok(()) => Err(error),
                Err(restore) => Err(format!("{error}；恢复通知模式失败：{restore}")),
            };
        }
        Ok(Self {
            source,
            path: device.path().to_owned(),
            previous,
            active: true,
            _owner: owner,
        })
    }

    pub(crate) fn read(&mut self) -> Result<Vec<State>, String> {
        self.source.read()
    }

    pub(crate) fn restore(&mut self) -> Result<(), String> {
        if self.active {
            Device::open_path(self.path.clone())?.set_notification_mode(self.previous)?;
            self.active = false;
        }
        Ok(())
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        if let Err(error) = self.restore() {
            eprintln!("恢复鼠标通知模式失败：{error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notifications_keep_physical_buttons_wheel_and_two_selections_separate() {
        let mut report = [0; REPORT];
        report[..6].copy_from_slice(&[7, 1, 0x18, 0x15, 0x07, 0x80]);
        report[12] = 0xf4;
        report[7] = 0x82;
        report[13] = 0x8b;
        // Generic EXE fields 14..15 are zero on this firmware. Never decode
        // arbitrary trailing data as an A90 extra-action notification.
        report[14..16].copy_from_slice(&[0xa1, 0xc0]);
        let state = parse(&report).unwrap().unwrap();
        assert_eq!(state.held_buttons(), [1, 3, 5, 9, 10, 11, 24]);
        assert_eq!((state.wheel, state.profile, state.sensitivity), (1, 2, 4));
        report[2] = 0xf7;
        assert_eq!(parse(&report).unwrap().unwrap().wheel, -1);
        report[2] = 7;
        assert_eq!(parse(&report).unwrap().unwrap().wheel, 0);
        assert!(parse(&report[..16]).is_err());
        for subtype in [0x04, 0x12, 0x1f, 0x81] {
            report[1] = subtype;
            assert_eq!(parse(&report).unwrap(), None);
        }
        assert_eq!(parse(&[1, 1]).unwrap(), None);
        assert_eq!(parse(&[]).unwrap(), None);
    }
}
