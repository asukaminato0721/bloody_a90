//! A90 vendor HID transport, firmware 07 and 0c. See docs/research/PROTOCOL.md
//! and docs/research/FIRMWARE-0C.md.
//!
//! Only requests verified on this mouse, or required by the audited save
//! sequence, can be built here. Command 0d has passed motion and RAM tests.
//! Commands 03 (sensor transfer), 1e (bootloader copy) and 2a are not sent.
use crate::backup::Layout;
use std::{
    fs::{self, File, OpenOptions},
    os::fd::AsRawFd,
    path::{Path, PathBuf},
    thread::sleep,
    time::{Duration, Instant},
};

pub const REPORT: usize = 64;
/// USB product IDs of the A90 (vendor 09da): firmware 07 enumerates as fafd,
/// firmware 0c as 5450.
pub const PRODUCTS: [u16; 2] = [0xfafd, 0x5450];
const ID: u8 = 7;
pub use crate::flash::{BUTTONS, BUTTONS_END, SENSITIVITY};
use crate::flash::WORDS as FLASH_WORDS;
// Command 25 magic. A wrong magic locks the command until the next reset.
const UNLOCK: [u8; 4] = [0x52, 0x49, 0x18, 0x22];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identity {
    pub model: u16,
    pub firmware: u8,
    pub core: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryInfo {
    pub rate_hz: u16,
    pub sensitivity: u16,
    pub buttons: u16,
    pub settings: u16,
    pub factory_sensitivity: u16,
    pub device_data: u16,
    pub factory_buttons: u16,
}

/// Command 13 returns the current cutoff and the calibration minimum.
#[derive(Clone, Copy, Debug)]
pub struct LiftStatus {
    pub cutoff: u8,
    pub minimum: u8,
}

impl LiftStatus {
    /// EXE 0044bc60 / 0044bf04. Bit 7 means that the sensor supplied a sample.
    pub fn calibrated_level(self) -> Result<u8, String> {
        if self.minimum & 0x80 == 0 {
            return Err("校准没有收到有效样本。请在使用表面上移动鼠标后重试。".into());
        }
        Ok(16 - (self.minimum & 0x1f).min(15))
    }
}

struct CalibrationMode<'a> {
    device: &'a Device,
    previous: u8,
    active: bool,
}

impl Drop for CalibrationMode<'_> {
    fn drop(&mut self) {
        if self.active {
            let _ = self.device.set_calibration_mode(self.previous);
        }
    }
}

pub struct Device {
    file: File,
    path: PathBuf,
    // One handle owns complete HID operations. Do not share it across threads.
    _single_thread: std::marker::PhantomData<std::cell::Cell<()>>,
}

/// Cooperating driver instances lock the hidraw inode for the handle's whole
/// lifetime. Locking each ioctl would still let request/reply pairs interleave.
fn lock_interface(file: &File, timeout: Duration) -> Result<bool, std::io::Error> {
    let deadline = Instant::now() + timeout;
    loop {
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(true);
        }
        let error = std::io::Error::last_os_error();
        if !matches!(
            error.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
        ) {
            return Err(error);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        sleep(Duration::from_millis(3));
    }
}

// _IOC(_IOC_READ | _IOC_WRITE, 'H', nr, REPORT)
const fn hid_ioctl(nr: u64) -> u64 {
    (3 << 30) | ((REPORT as u64) << 16) | ((b'H' as u64) << 8) | nr
}
const HIDIOCSFEATURE: u64 = hid_ioctl(6);
const HIDIOCGFEATURE: u64 = hid_ioctl(7);

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// Stop running macros. The official program tells users to switch on Caps
/// Lock, Num Lock and Scroll Lock together; firmware 1b0f stops macros when
/// the keyboard LED report reads 07. This sends that report to the mouse's
/// keyboard interface, then restores the lock state that the kernel holds.
pub fn interrupt_macros() -> Result<(), String> {
    use std::io::Write;
    for entry in fs::read_dir("/sys/class/hidraw")
        .map_err(|e| e.to_string())?
        .flatten()
    {
        let sys = entry.path();
        let uevent = fs::read_to_string(sys.join("device/uevent")).unwrap_or_default();
        if !is_a90(&uevent)
            || !uevent
                .lines()
                .any(|line| line.starts_with("HID_PHYS=") && line.ends_with("/input0"))
        {
            continue;
        }
        let mut leds = 0u8;
        for input in fs::read_dir(sys.join("device/input"))
            .into_iter()
            .flatten()
            .flatten()
        {
            let name = input.file_name().to_string_lossy().into_owned();
            for (bit, led) in [(1, "numlock"), (2, "capslock"), (4, "scrolllock")] {
                let path = input.path().join(format!("{name}::{led}/brightness"));
                if fs::read_to_string(path).is_ok_and(|value| value.trim() != "0") {
                    leds |= bit;
                }
            }
        }
        let node = Path::new("/dev").join(entry.file_name());
        let mut file = OpenOptions::new()
            .write(true)
            .open(&node)
            .map_err(|e| format!("无法打开 {}：{e}", node.display()))?;
        // Unnumbered output report: report number 0, then the LED byte.
        file.write_all(&[0, 0x07]).map_err(|e| e.to_string())?;
        sleep(Duration::from_millis(200));
        file.write_all(&[0, leds]).map_err(|e| e.to_string())?;
        return Ok(());
    }
    Err("未找到 A90 的键盘接口".into())
}

/// A hidraw uevent of a known A90 product.
fn is_a90(uevent: &str) -> bool {
    uevent.lines().any(|line| {
        PRODUCTS
            .iter()
            .any(|id| line == format!("HID_ID=0003:000009DA:0000{id:04X}"))
    })
}

/// Find the vendor interface: USB 09da:fafd or 5450, usage page ff32, usage
/// 04fc to 04ff.
pub fn find() -> Result<PathBuf, String> {
    let entries = fs::read_dir("/sys/class/hidraw").map_err(|e| format!("无法列出 hidraw：{e}"))?;
    let mut names: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
    names.sort();
    for sys in names {
        let uevent = fs::read_to_string(sys.join("device/uevent")).unwrap_or_default();
        if !is_a90(&uevent) {
            continue;
        }
        let descriptor = fs::read(sys.join("device/report_descriptor")).unwrap_or_default();
        // The vendor usage equals the model word: 04fc to 04ff for cores 1 to 4.
        if contains(&descriptor, &[0x06, 0x32, 0xff])
            && (0xfc..=0xff).any(|low| contains(&descriptor, &[0x0a, low, 0x04]))
            && let Some(name) = sys.file_name()
        {
            return Ok(Path::new("/dev").join(name));
        }
    }
    Err("未找到 A90 的厂商接口（USB 09da:fafd 或 5450）".into())
}

fn header(command: u8, bytes: [u8; 6]) -> [u8; REPORT] {
    let mut report = [0; REPORT];
    report[0] = ID;
    report[1] = command;
    report[2..8].copy_from_slice(&bytes);
    report
}

fn put_words(report: &mut [u8; REPORT], words: &[u16]) {
    for (index, word) in words.iter().enumerate() {
        report[8 + index * 2..10 + index * 2].copy_from_slice(&word.to_le_bytes());
    }
}

/// Header byte 2 encodes the word count, as in EXE 005a0938.
fn write_report(address: usize, words: &[u16], erase: bool) -> [u8; REPORT] {
    let mut report = header(
        0x2f,
        [
            ((words.len() - 1) * 8 + 1) as u8,
            (address >> 8) as u8,
            address as u8,
            if erase { 0x80 } else { 0 },
            0,
            0,
        ],
    );
    put_words(&mut report, words);
    report
}

/// Firmware 137a accepts 2400..=3e00. This tool narrows that to the
/// sensitivity page and the button area below the code page at 3d00.
fn writable(address: usize, count: usize) -> bool {
    let end = address + count;
    (address >= SENSITIVITY && end <= SENSITIVITY + 0x100)
        || (address >= BUTTONS && end <= BUTTONS_END)
}

impl Device {
    pub fn open() -> Result<Self, String> {
        let path = find()?;
        Self::open_path(path)
    }

    pub fn open_path(path: PathBuf) -> Result<Self, String> {
        Self::open_candidate(&path, Duration::from_secs(1))?.ok_or_else(|| {
            format!(
                "{} 正由其他操作使用。请等待该操作结束后重试。",
                path.display()
            )
        })
    }

    /// A background sampler must not delay input forwarding while a save or
    /// another process owns the interface. None means that the lock is busy.
    pub(crate) fn try_open() -> Result<Option<Self>, String> {
        Self::open_candidate(&find()?, Duration::ZERO)
    }

    fn open_candidate(path: &Path, wait: Duration) -> Result<Option<Self>, String> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| {
                format!(
                    "无法打开 {}：{e}。请安装 contrib/70-bloody-a90.rules 并重新插入鼠标。",
                    path.display()
                )
            })?;
        if !lock_interface(&file, wait).map_err(|e| format!("无法锁定 {}：{e}", path.display()))?
        {
            return Ok(None);
        }
        let device = Self {
            file,
            path: path.to_owned(),
            _single_thread: std::marker::PhantomData,
        };
        let identity = device.identity()?;
        if identity.model & 0xfffc != 0x04fc || Layout::of(identity.firmware).is_none() {
            return Err(format!(
                "设备型号 {:04x}、固件 {:02x} 不是已分析的 A90 固件 07 或 0c",
                identity.model, identity.firmware
            ));
        }
        Ok(Some(device))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn ioctl(&self, request: u64, report: &mut [u8; REPORT]) -> Result<(), String> {
        // SAFETY: the buffer has the length encoded in the request.
        let result =
            unsafe { libc::ioctl(self.file.as_raw_fd(), request as _, report.as_mut_ptr()) };
        if result < 0 {
            return Err(format!("HID 请求失败：{}", std::io::Error::last_os_error()));
        }
        if result as usize != REPORT {
            return Err(format!("HID 请求只传输了 {result} 字节"));
        }
        Ok(())
    }

    fn send(&self, report: &[u8; REPORT]) -> Result<(), String> {
        let mut buffer = *report;
        self.ioctl(HIDIOCSFEATURE, &mut buffer)
    }

    /// Send a request, then read a fresh response with the same header.
    fn query(&self, request: &[u8; REPORT]) -> Result<[u8; REPORT], String> {
        self.send(request)?;
        for _ in 0..20 {
            sleep(Duration::from_millis(3));
            let mut response = [0; REPORT];
            response[0] = ID;
            self.ioctl(HIDIOCGFEATURE, &mut response)?;
            if response[..8] == request[..8] {
                return Ok(response);
            }
        }
        Err(format!("命令 {:02x} 的响应头不匹配", request[1]))
    }

    pub fn identity(&self) -> Result<Identity, String> {
        let response = self.query(&header(0x1f, [0; 6]))?;
        Ok(Identity {
            model: u16::from_le_bytes([response[10], response[11]]),
            firmware: response[12],
            // All four official 0c core images answer 22 in byte 9 and differ
            // only in the model word, 04fc to 04ff (docs/research/CORES.md).
            core: (response[10] & 3) + 1,
        })
    }

    pub fn memory_info(&self) -> Result<MemoryInfo, String> {
        let r = self.query(&header(0x05, [0; 6]))?;
        let word = |offset: usize| u16::from_le_bytes([r[8 + offset], r[9 + offset]]);
        Ok(MemoryInfo {
            rate_hz: word(2),
            sensitivity: word(4),
            buttons: word(6),
            settings: word(8),
            factory_sensitivity: word(10),
            device_data: word(12),
            factory_buttons: word(14),
        })
    }

    /// Brightness level 0 to 3.
    pub fn brightness(&self) -> Result<u8, String> {
        let level = self.query(&header(0x11, [0; 6]))?[8];
        if level > 3 {
            return Err(format!("亮度响应 {level} 超出 0 至 3"));
        }
        Ok(level)
    }

    /// Set and read back the brightness. All four levels passed hardware readback.
    pub fn set_brightness(&self, level: u8) -> Result<(), String> {
        if level > 3 {
            return Err("亮度应为 0 至 3".into());
        }
        let mut report = header(0x11, [0, 0, 0x80, 0, 0, 0]);
        report[8] = level;
        self.send(&report)?;
        sleep(Duration::from_millis(30));
        match self.brightness()? {
            read if read == level => Ok(()),
            read => Err(format!("亮度读回 {read}，期望 {level}")),
        }
    }

    /// One RAM byte, firmware routine 3d23.
    pub fn ram(&self, address: u16) -> Result<u8, String> {
        let [high, low] = address.to_be_bytes();
        Ok(self.query(&header(0x81, [0, high, low, 0, 0, 0]))?[8])
    }

    /// Runtime profile index and maximum index, RAM 263 and 264 (firmware
    /// 0a6d and 069c). The index can exceed a new table's profile count.
    pub fn runtime_profile(&self) -> Result<(u8, u8), String> {
        Ok((self.ram(0x263)?, self.ram(0x264)?))
    }

    /// Command 0c, firmware 1322 and 0694. Set the pending-selection bit.
    /// The firmware applies the selection after all held buttons are released.
    /// Preserve the running profile limit and debounce byte. No flash write.
    pub fn select_profile(&self, index: u8) -> Result<(u8, u8), String> {
        let (_, maximum) = self.runtime_profile()?;
        if index > maximum || index >= 7 {
            return Err("配置序号超出鼠标的配置数量".into());
        }
        let pending = self.ram(0x262)?;
        if pending & 0x80 != 0 {
            return Err("鼠标仍在等待上一次配置切换。请松开全部按键后重试。".into());
        }
        let mut request = header(0x0c, [0; 6]);
        request[8] = maximum;
        request[9] = 0x80 | index;
        request[10] = self.ram(0x25)?;
        self.query(&request)?;
        // Also check the pending bit. The current index can already match
        // while a held button still prevents the requested reload.
        for _ in 0..50 {
            let current = self.runtime_profile()?;
            if current.0 == index && self.ram(0x262)? & 0x80 == 0 {
                return Ok(current);
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        Err("配置切换尚未完成。松开全部按键后，鼠标会执行待处理的切换。".into())
    }

    /// Runtime report-rate code (RAM 23) and the settings register (bank 1,
    /// 12). Firmware 0ab8 takes the rate from settings page word 6 when the
    /// register has bits c0 equal to 80, otherwise from sensitivity word 1e.
    pub fn runtime_rate(&self) -> Result<(u8, u8), String> {
        Ok((self.ram(0x23)?, self.ram(0x112)?))
    }

    pub fn runtime_sensitivity(&self) -> Result<crate::sensitivity::Runtime, String> {
        let mut payload = [0; 8];
        for (byte, address) in payload
            .iter_mut()
            .zip([0x26, 0x27, 0x28, 0x29, 0x2a, 0x23, 0x2d, 0x2e])
        {
            *byte = self.ram(address)?;
        }
        crate::sensitivity::Runtime::from_payload(payload)
    }

    /// EXE 005a1dc8, firmware 12e1. The sensor loop at 1981..1983 writes
    /// register 1b through 19bd when the sensor code changes. No flash write.
    pub fn apply_runtime_sensitivity(
        &self,
        runtime: crate::sensitivity::Runtime,
    ) -> Result<(), String> {
        let mut request = header(0x0d, [0; 6]);
        request[8..16].copy_from_slice(&runtime.payload()?);
        self.query(&request)?;
        for _ in 0..20 {
            if self.runtime_sensitivity()? == runtime
                && self.ram(0x162)? == runtime.record.sensor_code as u8
            {
                if self.ram(0x157)? & 1 == 0 {
                    return Err("传感器尚未完成初始化".into());
                }
                return Ok(());
            }
            sleep(Duration::from_millis(10));
        }
        Err("即时灵敏度的运行参数或传感器状态读回不一致".into())
    }

    /// Apply the saved preset, rate and lift cutoff without a flash write.
    pub fn apply_sensitivity(&self, words: &[u16]) -> Result<(), String> {
        let runtime = crate::sensitivity::Runtime::from_template(words)?;
        let level = crate::sensitivity::lift_height(words)?;
        self.apply_runtime_sensitivity(runtime)?;
        self.set_lift_height(level)
    }

    pub fn save_sensitivity(&self, words: &[u16]) -> Result<(), String> {
        crate::sensitivity::Runtime::from_template(words)?;
        crate::sensitivity::lift_height(words)?;
        self.save(SENSITIVITY, words)?;
        self.apply_sensitivity(words)
            .map_err(|e| format!("灵敏度已保存，但即时应用未完成：{e}"))
    }

    /// Preserve the preset currently running on the mouse. It can differ
    /// from the saved selection after a hardware DPI button was used.
    pub fn save_rate(&self, words: &[u16]) -> Result<(), String> {
        let code = *words.get(0x1e).ok_or("灵敏度模板长度无效")?;
        if ![1, 2, 4, 8].contains(&code) {
            return Err("回报率编码无效".into());
        }
        self.runtime_sensitivity()?;
        self.save(SENSITIVITY, words)?;
        (|| {
            let mut runtime = self.runtime_sensitivity()?;
            runtime.rate_code = code as u8;
            self.apply_runtime_sensitivity(runtime)
        })()
        .map_err(|e| format!("回报率已保存，但即时应用未完成：{e}"))
    }

    pub fn save_lift(&self, words: &[u16]) -> Result<(), String> {
        let level = crate::sensitivity::lift_height(words)?;
        self.save(SENSITIVITY, words)?;
        self.set_lift_height(level)
            .map_err(|e| format!("抬起高度已保存，但即时应用未完成：{e}"))
    }

    /// EXE 005a1d54, firmware 1322. Keep the running profile limit and
    /// selection register. Only the response-time byte changes.
    pub fn set_debounce_ms(&self, ms: u8) -> Result<(), String> {
        if ms > 16 {
            return Err("硬件按键响应时间应为 0 至 16 ms".into());
        }
        let mut request = header(0x0c, [0; 6]);
        request[8] = self.ram(0x264)?;
        request[9] = self.ram(0x262)?;
        request[10] = ms + 1;
        self.query(&request)?;
        if self.ram(0x25)? != ms + 1 {
            return Err("硬件按键响应时间的运行值读回不一致".into());
        }
        Ok(())
    }

    pub fn lift_status(&self) -> Result<LiftStatus, String> {
        let response = self.query(&header(0x13, [0; 6]))?;
        Ok(LiftStatus {
            cutoff: response[8],
            minimum: response[9],
        })
    }

    /// EXE 005a28c8, firmware 1202. This changes RAM 24, not sensor registers.
    pub fn set_lift_height(&self, level: Option<u8>) -> Result<(), String> {
        let code = match level {
            Some(level @ 1..=16) => 0xc0 | (16 - level),
            Some(_) => return Err("抬起高度应为 1 至 16 级".into()),
            None => self.lift_status()?.cutoff & !0x40,
        };
        let mut request = header(0x13, [0, 0, 0x80, 0, 0, 0]);
        request[8] = code;
        self.query(&request)?;
        if self.lift_status()?.cutoff != code {
            return Err("抬起高度的运行值读回不一致".into());
        }
        Ok(())
    }

    fn set_calibration_mode(&self, value: u8) -> Result<(), String> {
        let mut request = header(0x12, [0; 6]);
        request[8] = value;
        self.query(&request)?;
        if self.ram(0x111)? != value {
            return Err("校准状态读回不一致".into());
        }
        Ok(())
    }

    /// Collect the minimum surface quality, then restore the previous mode.
    /// Return the official 1..16 level without changing the saved cutoff.
    pub fn calibrate_lift(&self, seconds: u64) -> Result<u8, String> {
        if !(1..=30).contains(&seconds) {
            return Err("校准时间应为 1 至 30 秒".into());
        }
        let previous = self.ram(0x111)?;
        if previous & 0x80 != 0 {
            return Err("鼠标已经处于校准状态".into());
        }
        let mut guard = CalibrationMode {
            device: self,
            previous,
            active: true,
        };
        self.set_calibration_mode(previous)?;
        sleep(Duration::from_millis(30));
        self.set_calibration_mode(previous | 0x80)?;
        for _ in 0..seconds * 10 {
            sleep(Duration::from_millis(100));
            self.lift_status()?;
        }
        let result = self.lift_status()?.calibrated_level();
        self.set_calibration_mode(previous)?;
        guard.active = false;
        drop(guard);
        result
    }

    /// Buttons 1 to 24 currently held, from the state bytes that core_var
    /// reads (firmware 1126, bank 2 addresses 5e..60). Verified on the A90:
    /// buttons 1-5 are the mouse buttons, 9, 10 and 11 the "1", "C" and "3" keys.
    pub fn held_buttons(&self) -> Result<Vec<u8>, String> {
        let mut held = Vec::new();
        for (index, address) in (0x25e..=0x260).enumerate() {
            let byte = self.ram(address)?;
            held.extend(
                (0..8)
                    .filter(|bit| byte & 1 << bit != 0)
                    .map(|bit| index as u8 * 8 + bit + 1),
            );
        }
        Ok(held)
    }

    /// EXE 005a229c/005a2318, firmware 119c -> 12de. Only bit zero enables
    /// vendor notifications. Preserve all other running flags in RAM 004b.
    pub(crate) fn set_notification_mode(&self, enabled: bool) -> Result<(), String> {
        let current = self.ram(0x4b)?;
        let target = (current & !1) | u8::from(enabled);
        if current != target {
            let mut request = header(9, [0; 6]);
            request[8] = target;
            self.query(&request)?;
            if self.ram(0x4b)? != target {
                return Err("鼠标通知模式读回不一致".into());
            }
        }
        Ok(())
    }

    /// EXE 005a2394, firmware 1199 -> 1214. Queue one current state report.
    pub(crate) fn request_notification(&self) -> Result<(), String> {
        self.query(&header(6, [0; 6]))?;
        Ok(())
    }

    fn flash_access(&self, enabled: bool) -> Result<(), String> {
        let mut report = header(0x25, [0, 0, UNLOCK[0], UNLOCK[1], UNLOCK[2], UNLOCK[3]]);
        report[8] = enabled as u8;
        self.send(&report)?;
        sleep(Duration::from_millis(3));
        Ok(())
    }

    /// Read flash words. Reads are enabled for the call and disabled again.
    pub fn read_flash(
        &self,
        start: usize,
        count: usize,
        mut progress: impl FnMut(usize, usize),
    ) -> Result<Vec<u16>, String> {
        if start + count > FLASH_WORDS {
            return Err("读取范围超出 16384 个字".into());
        }
        self.flash_access(true)?;
        let result = (|| -> Result<Vec<u16>, String> {
            let mut words = Vec::with_capacity(count + 28);
            while words.len() < count {
                let address = start + words.len();
                let [high, low] = (address as u16).to_be_bytes();
                let response = self.query(&header(0x02, [0, high, low, 0x60, 0, 0]))?;
                words.extend(
                    response[8..]
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|pair| u16::from_le_bytes(*pair)),
                );
                progress(words.len().min(count), count);
            }
            words.truncate(count);
            Ok(words)
        })();
        let closed = self.flash_access(false);
        let words = result?;
        closed?;
        Ok(words)
    }

    pub fn read_backup(&self, progress: impl FnMut(usize, usize)) -> Result<Vec<u8>, String> {
        let words = self.read_flash(0, FLASH_WORDS, progress)?;
        Ok(words.iter().flat_map(|word| word.to_le_bytes()).collect())
    }

    /// Command 20 access, EXE 005a1fa8. Poll until response byte 8 is zero,
    /// waiting 3 ms after each response, as in the EXE.
    fn access(&self, open: bool) -> Result<(), String> {
        let request = header(0x20, [0, 0, 0, 0, 0, open as u8]);
        for _ in 0..1000 {
            let response = self.query(&request)?;
            sleep(Duration::from_millis(3));
            if response[8] == 0 {
                return Ok(());
            }
        }
        Err("设备在 1000 次查询后仍未就绪".into())
    }

    /// Write words exactly as given, for restoring a backup. The range must be
    /// page-aligned and inside an allowed area; each page is erased first.
    pub fn restore(&self, address: usize, words: &[u16]) -> Result<(), String> {
        if !address.is_multiple_of(0x100)
            || !words.len().is_multiple_of(16)
            || words.is_empty()
            || !writable(address, words.len())
        {
            return Err("恢复范围必须按页对齐，并位于允许写入的区域".into());
        }
        self.access(true)?;
        sleep(Duration::from_millis(3));
        let written = (|| {
            for (index, chunk) in words.chunks(16).enumerate() {
                self.send(&write_report(address + index * 16, chunk, true))?;
                sleep(Duration::from_millis(3));
            }
            Ok::<_, String>(())
        })();
        let closed = self.access(false);
        written?;
        closed?;
        if self.read_flash(address, words.len(), |_, _| {})? != words {
            return Err("恢复后读回不一致".into());
        }
        Ok(())
    }

    /// Save one complete block: a sensitivity block at 2400 or a button main
    /// vector at 2600. Sequence from EXE 0043c5d8 and 0043b1e8: open access,
    /// stage the header as ffff/ffff, write 16-word chunks with mode 80 (page
    /// erase at page starts), commit a4a4 alone, close access. Then read back.
    pub fn save(&self, address: usize, block: &[u16]) -> Result<(), String> {
        let valid_length = match address {
            SENSITIVITY => block.len() == 128,
            BUTTONS => (32..=BUTTONS_END - BUTTONS).contains(&block.len()),
            _ => false,
        };
        let padded = block.len().div_ceil(16) * 16;
        if !valid_length || !writable(address, padded) {
            return Err("保存地址或长度不在允许范围内".into());
        }
        if block[..2] != [0xa4a4, 0xffff]
            || crate::storage::checksums(block) != (block[2], block[3])
        {
            return Err("保存块的头部或校验值无效".into());
        }
        // Saved tables use the runtime profile index directly. An index past
        // the new profile list reads a garbage pointer and disables buttons.
        if address == BUTTONS {
            let (current, _) = self.runtime_profile()?;
            if usize::from(current) > usize::from(block[4] >> 8) {
                return Err(format!(
                    "鼠标正在使用配置 {}，新按键表只有 {} 个配置。请先切换到范围内的配置。",
                    current + 1,
                    (block[4] >> 8) + 1
                ));
            }
        }
        let mut staged = block.to_vec();
        staged[..2].copy_from_slice(&[0xffff, 0xffff]);
        staged.resize(padded, 0);

        self.access(true)?;
        sleep(Duration::from_millis(3));
        let written = (|| {
            for (index, chunk) in staged.chunks(16).enumerate() {
                self.send(&write_report(address + index * 16, chunk, true))?;
                sleep(Duration::from_millis(3));
            }
            self.send(&write_report(address, &[0xa4a4], false))?;
            sleep(Duration::from_millis(3));
            Ok::<_, String>(())
        })();
        let closed = self.access(false);
        written?;
        closed?;

        staged[0] = 0xa4a4;
        let read = self.read_flash(address, padded, |_, _| {})?;
        if read != staged {
            let first = read
                .iter()
                .zip(&staged)
                .position(|(a, b)| a != b)
                .unwrap_or(0);
            return Err(format!(
                "保存后读回不一致：字地址 {:04x} 读到 {:04x}，期望 {:04x}",
                address + first,
                read[first],
                staged[first]
            ));
        }
        if address == BUTTONS && (1..=17).contains(&(block[6] as u8)) {
            self.set_debounce_ms(block[6] as u8 - 1)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interface_lock_excludes_other_handles_and_releases_on_close() {
        let path = std::env::temp_dir().join(format!(
            "a90-lock-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let first = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        let second = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        // The inode remains alive through the open handles after unlinking.
        fs::remove_file(path).unwrap();
        assert!(lock_interface(&first, Duration::ZERO).unwrap());
        assert!(!lock_interface(&second, Duration::ZERO).unwrap());
        let acquired = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let observed = acquired.clone();
        let (started, waiting) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            started.send(()).unwrap();
            assert!(lock_interface(&second, Duration::from_secs(1)).unwrap());
            observed.store(true, std::sync::atomic::Ordering::Release);
            second
        });
        waiting.recv().unwrap();
        assert!(!acquired.load(std::sync::atomic::Ordering::Acquire));
        drop(first);
        let second = thread.join().unwrap();
        assert!(acquired.load(std::sync::atomic::Ordering::Acquire));
        drop(second);
    }

    #[test]
    #[ignore = "requires A90_TEST_RUNTIME_DPI=1 and movement; temporarily saves and restores sensitivity"]
    fn runtime_sensitivity_hardware() {
        use std::{io::Read, os::unix::fs::OpenOptionsExt, time::Instant};
        assert_eq!(std::env::var("A90_TEST_RUNTIME_DPI").as_deref(), Ok("1"));
        let node = fs::read_dir("/sys/class/input")
            .unwrap()
            .flatten()
            .find_map(|entry| {
                let dir = entry.path().join("device");
                let read = |name: &str| fs::read_to_string(dir.join(name)).unwrap_or_default();
                let rel = u64::from_str_radix(read("capabilities/rel").trim(), 16).unwrap_or(0);
                (entry.file_name().to_string_lossy().starts_with("event")
                    && read("id/vendor").trim() == "09da"
                    && PRODUCTS
                        .iter()
                        .any(|id| read("id/product").trim() == format!("{id:04x}"))
                    && rel & 3 == 3)
                    .then(|| Path::new("/dev/input").join(entry.file_name()))
            })
            .expect("A90 relative-motion input node");
        let mut input = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(node)
            .unwrap();
        fn motion(input: &mut File) -> (usize, Option<i64>) {
            let time_size = std::mem::size_of::<libc::timeval>();
            let mut bytes = vec![0; time_size + 8];
            // Discard events from before this sampling stage.
            while matches!(input.read(&mut bytes), Ok(n) if n > 0) {}
            let start = Instant::now();
            let mut moved = false;
            let mut times = Vec::new();
            while start.elapsed() < Duration::from_secs(3) {
                match input.read(&mut bytes) {
                    Ok(n) if n == bytes.len() => {
                        let kind =
                            u16::from_ne_bytes(bytes[time_size..time_size + 2].try_into().unwrap());
                        let code = u16::from_ne_bytes(
                            bytes[time_size + 2..time_size + 4].try_into().unwrap(),
                        );
                        let value = i32::from_ne_bytes(bytes[time_size + 4..].try_into().unwrap());
                        if kind == 2 && code <= 1 && value != 0 {
                            moved = true;
                        }
                        if kind == 0 && code == 0 {
                            if moved {
                                // input_event begins with the native timeval.
                                let time = unsafe {
                                    std::ptr::read_unaligned(bytes.as_ptr().cast::<libc::timeval>())
                                };
                                times.push(time.tv_sec as i64 * 1_000_000 + time.tv_usec as i64);
                            }
                            moved = false;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        sleep(Duration::from_millis(1))
                    }
                    result => panic!("input read: {result:?}"),
                }
            }
            let mut gaps: Vec<_> = times.windows(2).map(|pair| pair[1] - pair[0]).collect();
            gaps.sort_unstable();
            (times.len(), gaps.get(gaps.len() / 2).copied())
        }
        let device = Device::open().unwrap();
        let flash = device.read_backup(|_, _| {}).unwrap();
        let original = device.runtime_sensitivity().unwrap();
        struct Restore<'a>(&'a Device, crate::sensitivity::Runtime, bool);
        impl Drop for Restore<'_> {
            fn drop(&mut self) {
                if self.2 {
                    eprintln!(
                        "runtime restore: {:?}",
                        self.0.apply_runtime_sensitivity(self.1)
                    );
                }
            }
        }
        let mut restore = Restore(&device, original, true);
        device.apply_runtime_sensitivity(original).unwrap();
        println!("same-state payload: {:02x?}", original.payload().unwrap());
        let baseline = motion(&mut input);
        println!("same-state motion frames / median us: {baseline:?}");
        assert!(
            baseline.0 >= 10,
            "Move the mouse continuously during this test"
        );
        let mut alternate = original;
        alternate.record = crate::sensitivity::encode(
            if original.record.sensor_code == 0x11 {
                1600
            } else {
                800
            },
            if original.record.sensor_code == 0x11 {
                1600
            } else {
                800
            },
        )
        .unwrap();
        device.apply_runtime_sensitivity(alternate).unwrap();
        let changed = motion(&mut input);
        println!(
            "alternate DPI payload / motion: {:02x?} {changed:?}",
            alternate.payload().unwrap()
        );
        alternate.rate_code = if original.rate_code == 2 { 1 } else { 2 };
        device.apply_runtime_sensitivity(alternate).unwrap();
        let rate = motion(&mut input);
        println!(
            "alternate rate payload / motion: {:02x?} {rate:?}",
            alternate.payload().unwrap()
        );
        device.apply_runtime_sensitivity(original).unwrap();
        restore.2 = false;
        let recovered = motion(&mut input);
        println!(
            "restored payload / motion: {:02x?} {recovered:?}",
            device.runtime_sensitivity().unwrap().payload().unwrap()
        );
        assert!(
            changed.0 >= 10 && rate.0 >= 10 && recovered.0 >= 10,
            "Each stage needs mouse movement"
        );
        assert_eq!(device.runtime_sensitivity().unwrap(), original);
        assert_eq!(device.read_backup(|_, _| {}).unwrap(), flash);

        let backup = crate::backup::Backup::parse(&flash).unwrap();
        let original_cutoff = device.lift_status().unwrap().cutoff;
        let original_level = (original_cutoff & 0x40 != 0).then_some(16 - (original_cutoff & 0x1f));
        struct SavedSettings<'a> {
            device: &'a Device,
            page: Vec<u16>,
            runtime: crate::sensitivity::Runtime,
            lift: Option<u8>,
            active: bool,
        }
        impl SavedSettings<'_> {
            fn restore(&mut self) -> Result<(), String> {
                let flash = self.device.restore(SENSITIVITY, &self.page);
                let runtime = self.device.apply_runtime_sensitivity(self.runtime);
                let lift = self.device.set_lift_height(self.lift);
                flash?;
                runtime?;
                lift?;
                self.active = false;
                Ok(())
            }
        }
        impl Drop for SavedSettings<'_> {
            fn drop(&mut self) {
                if self.active {
                    eprintln!("saved sensitivity restore: {:?}", self.restore());
                }
            }
        }
        let page = flash[SENSITIVITY * 2..(SENSITIVITY + 0x100) * 2]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes(*b))
            .collect();
        let mut saved = SavedSettings {
            device: &device,
            page,
            runtime: original,
            lift: original_level,
            active: true,
        };
        let block = crate::sensitivity::block(
            &backup.sensitivity_template().unwrap(),
            &[[800; 2], [1600; 2]],
            1,
            Some(500),
        )
        .unwrap();
        let block = crate::sensitivity::with_lift_height(&block, original_level).unwrap();
        device.save_sensitivity(&block).unwrap();
        assert_eq!(
            device.runtime_sensitivity().unwrap(),
            crate::sensitivity::Runtime::from_template(&block).unwrap()
        );
        let saved_motion = motion(&mut input);
        println!("saved DPI immediate motion: {saved_motion:?}");
        let mut selected = device.runtime_sensitivity().unwrap();
        selected.selected = 0;
        selected.record = crate::sensitivity::encode(800, 800).unwrap();
        device.apply_runtime_sensitivity(selected).unwrap();
        let rate_block = crate::sensitivity::with_rate(&block, 1000).unwrap();
        device.save_rate(&rate_block).unwrap();
        selected.rate_code = 1;
        assert_eq!(
            device.runtime_sensitivity().unwrap(),
            selected,
            "rate save must preserve the running DPI selection"
        );
        let rate_motion = motion(&mut input);
        println!("saved rate immediate motion: {rate_motion:?}");
        device.save_lift(&rate_block).unwrap();
        assert_eq!(
            device.runtime_sensitivity().unwrap(),
            selected,
            "lift save must preserve the running DPI and rate"
        );
        saved.restore().unwrap();
        assert_eq!(device.runtime_sensitivity().unwrap(), original);
        assert_eq!(device.read_backup(|_, _| {}).unwrap(), flash);
        println!("saved settings restored; full flash unchanged");
        assert!(
            saved_motion.0 >= 10 && rate_motion.0 >= 10,
            "Saved settings also need mouse movement"
        );
    }

    #[test]
    fn calibration_sample_flag_and_official_clamp() {
        for (minimum, level) in [(0x80, 16), (0x87, 9), (0x8f, 1), (0x9f, 1)] {
            assert_eq!(
                LiftStatus { cutoff: 0, minimum }.calibrated_level(),
                Ok(level)
            );
        }
        assert!(
            LiftStatus {
                cutoff: 0,
                minimum: 0x1f
            }
            .calibrated_level()
            .is_err()
        );
    }

    #[test]
    fn report_layout() {
        let report = write_report(0x2410, &[0x1234; 16], true);
        assert_eq!(report[..8], [7, 0x2f, 0x79, 0x24, 0x10, 0x80, 0, 0]);
        assert_eq!(report[8..10], [0x34, 0x12]);
        assert_eq!(report[38..40], [0x34, 0x12]);
        assert_eq!(report[40..], [0; 24]);
        let commit = write_report(0x2600, &[0xa4a4], false);
        assert_eq!(
            commit[..10],
            [7, 0x2f, 0x01, 0x26, 0x00, 0, 0, 0, 0xa4, 0xa4]
        );
        assert_eq!(HIDIOCSFEATURE, 0xc0404806);
        assert_eq!(HIDIOCGFEATURE, 0xc0404807);
    }

    #[test]
    fn write_ranges() {
        assert!(writable(0x2400, 0x100));
        assert!(!writable(0x2400, 0x101));
        assert!(!writable(0x23f0, 16));
        assert!(!writable(0x2500, 16));
        assert!(writable(0x2600, 0x1700));
        assert!(!writable(0x2600, 0x1701));
        assert!(!writable(0x3d00, 16));
        assert!(!writable(0x3e00, 16));
    }
}
