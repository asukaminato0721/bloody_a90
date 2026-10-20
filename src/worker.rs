//! Background thread for files and the mouse. The interface thread never
//! blocks on HID requests or flash reads.
use bloody_a90::{
    associations::Rules,
    backup::Backup,
    configuration::{self, Configuration},
    device::{self, Device, Identity, MemoryInfo},
};
use eframe::egui;
use std::{
    fs,
    io::Write,
    path::PathBuf,
    sync::mpsc::{Receiver, Sender},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy)]
pub enum Purpose {
    Amc,
    Profile,
    Gestures,
}

#[derive(Clone, Copy)]
pub enum Refresh {
    All,
    Sensitivity,
    Rate,
    Lift,
    Buttons,
}

pub enum Job {
    Backup(String),
    Read(String, Purpose),
    Export(String, Vec<u8>),
    SaveGesture(String, Vec<u8>, Option<Vec<u8>>),
    Connect,
    Brightness(u8),
    SaveSensitivity(Vec<u16>),
    SaveRate(Vec<u16>),
    SaveLift(Vec<u16>),
    CalibrateLift,
    Shutdown,
    SaveButtons(Box<Configuration>),
    SaveHostButtons(
        Box<Configuration>,
        Box<bloody_a90::host_buttons::Settings>,
        Option<Vec<u8>>,
    ),
    HostButtonsEnabled(bool, Option<Vec<u8>>),
    HostButtonsStatus,
    Held,
    HostRead,
    Interrupt,
    HostWrite(bloody_a90::host::Settings),
    Associations(Rules),
    Foreground,
    Library(PathBuf, Box<bloody_a90::library::Command>),
    Gestures(Option<Box<bloody_a90::gesture_runtime::Settings>>),
    GestureStatus,
    WheelRead,
    WheelWrite(bloody_a90::wheel4d::Settings, Option<Vec<u8>>),
}

pub enum Reply {
    Backup(Result<Backup, String>),
    File(Purpose, Result<Vec<u8>, String>),
    Export(Result<String, String>),
    GestureSaved(String, Result<Vec<u8>, String>),
    /// A fresh device snapshot and a status message.
    Device(Refresh, Result<(Box<Snapshot>, String), String>),
    Brightness(Result<u8, String>),
    Held(Result<Vec<u8>, String>),
    Host(Result<(bloody_a90::host::Settings, String), String>),
    CalibratedLift(Result<u8, String>),
    Associations(Result<(), String>),
    Foreground(Result<Foreground, String>),
    Library(Result<Box<bloody_a90::library::Update>, String>),
    Gestures(Result<GestureUpdate, String>),
    GestureStatus(bloody_a90::gesture_runtime::Status, bool, bool),
    Wheel(Result<WheelUpdate, String>),
    HostButtons(Result<bloody_a90::gesture_runtime::Status, String>),
    HostButtonsEnabled(
        Result<
            (
                bloody_a90::host_buttons::Settings,
                Vec<u8>,
                bloody_a90::gesture_runtime::Status,
            ),
            String,
        >,
    ),
}

pub struct Foreground {
    pub app: Option<String>,
    pub profile: Option<(u8, u8)>,
}

pub struct GestureUpdate {
    pub requested: bool,
    pub status: bloody_a90::gesture_runtime::Status,
    pub host: Option<(bloody_a90::host_buttons::Settings, Option<Vec<u8>>)>,
}

pub struct WheelUpdate {
    pub settings: bloody_a90::wheel4d::Settings,
    pub bytes: Option<Vec<u8>>,
    pub status: bloody_a90::gesture_runtime::Status,
    pub runtime_enabled: bool,
}

pub struct Snapshot {
    pub path: String,
    pub identity: Identity,
    pub info: MemoryInfo,
    pub brightness: u8,
    pub profile: (u8, u8),
    pub rate: (u8, u8),
    pub backup: Backup,
    pub host_buttons: Result<bloody_a90::host_buttons::Settings, String>,
    pub host_button_file: Option<Vec<u8>>,
}

pub fn write_new(path: &PathBuf, bytes: &[u8]) -> Result<(), String> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("无法创建 {}：{e}", path.display()))?;
    file.write_all(bytes)
        .map_err(|e| format!("写入 {} 未完成：{e}", path.display()))
}

fn cache_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
        .join("bloody-a90")
}

fn snapshot(device: &Device) -> Result<Box<Snapshot>, String> {
    let bytes = device.read_backup(|_, _| {})?;
    let host_buttons = host_button_settings();
    Ok(Box::new(Snapshot {
        path: device.path().display().to_string(),
        identity: device.identity()?,
        info: device.memory_info()?,
        brightness: device.brightness()?,
        profile: device.runtime_profile()?,
        rate: device.runtime_rate()?,
        backup: Backup::parse(&bytes)?,
        host_buttons: host_buttons
            .as_ref()
            .map(|(_, settings, _)| settings.clone())
            .map_err(Clone::clone),
        host_button_file: host_buttons.ok().and_then(|(_, _, bytes)| bytes),
    }))
}

struct Worker {
    // The first read of each session is kept as a file before any write.
    backup_file: Option<PathBuf>,
    monitor: Option<bloody_a90::foreground::Monitor>,
    associations: Rules,
    last_application: Option<Option<String>>,
    gestures: Option<bloody_a90::gesture_runtime::Handle>,
    gesture_settings: Option<bloody_a90::gesture_runtime::Settings>,
    gesture_status: bloody_a90::gesture_runtime::Status,
    capture_suppressed: bool,
    wheel: bloody_a90::wheel4d::Settings,
    core: Option<u8>,
    native_gestures: bool,
    host_buttons: Option<bloody_a90::host_button_runtime::Handle>,
    host_button_status: bloody_a90::gesture_runtime::Status,
}

type HostButtonSettings = (PathBuf, bloody_a90::host_buttons::Settings, Option<Vec<u8>>);

fn host_button_settings() -> Result<HostButtonSettings, String> {
    let path = bloody_a90::host_buttons::path().ok_or("无法确定主机动作文件路径")?;
    let (settings, bytes) = bloody_a90::host_buttons::load(&path)?;
    Ok((path, settings, bytes))
}

impl Worker {
    fn new() -> Self {
        Self {
            backup_file: None,
            monitor: None,
            associations: Rules::default(),
            last_application: None,
            gestures: None,
            gesture_settings: None,
            gesture_status: bloody_a90::gesture_runtime::Status {
                running: false,
                message: "全局手势未启用".into(),
            },
            capture_suppressed: false,
            wheel: bloody_a90::wheel4d::path()
                .and_then(|p| bloody_a90::wheel4d::load(&p).ok())
                .map(|(settings, _)| settings)
                .unwrap_or_default(),
            native_gestures: false,
            core: None,
            host_buttons: None,
            host_button_status: bloody_a90::gesture_runtime::Status {
                running: false,
                message: "按键主机动作未启用".into(),
            },
        }
    }

    fn foreground(&mut self) -> Result<Foreground, String> {
        if self.monitor.is_none() {
            self.monitor = Some(bloody_a90::foreground::Monitor::connect()?);
        }
        let app = match self.monitor.as_mut().unwrap().current() {
            Ok(app) => app,
            Err(error) => {
                self.monitor = None;
                return Err(error);
            }
        };
        let mut profile = None;
        if self.last_application.as_ref() != Some(&app)
            && let Some(target) = self.associations.target(app.as_deref())
        {
            let device = Device::open()?;
            let current = device.runtime_profile()?;
            profile = Some(if current.0 == target {
                current
            } else {
                device.select_profile(target)?
            });
        }
        self.last_application = Some(app.clone());
        Ok(Foreground { app, profile })
    }

    fn connect(&mut self) -> Result<(Box<Snapshot>, String), String> {
        self.last_application = None;
        let device = Device::open()?;
        let snapshot = snapshot(&device)?;
        if self.backup_file.is_none() {
            let dir = cache_dir();
            fs::create_dir_all(&dir).map_err(|e| format!("无法创建 {}：{e}", dir.display()))?;
            let seconds = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |time| time.as_secs());
            let path = dir.join(format!("backup-{seconds}.bin"));
            write_new(&path, &snapshot.backup.to_bytes())?;
            self.backup_file = Some(path);
        }
        let path = self.backup_file.as_ref().map(|p| p.display().to_string());
        drop(device);
        self.restart_host_buttons(true);
        Ok((
            snapshot,
            format!("已连接并读取闪存 · 本次备份 {}", path.unwrap_or_default()),
        ))
    }

    fn stop_host_buttons(&mut self) -> Result<(), String> {
        if let Some(mut handle) = self.host_buttons.take() {
            handle.poll();
            let stopped = handle.stop();
            if let Some(settings) = handle.wheel_settings() {
                self.wheel = settings;
            }
            stopped?;
        }
        self.host_button_status = bloody_a90::gesture_runtime::Status {
            running: false,
            message: "按键主机动作已停止".into(),
        };
        Ok(())
    }

    fn poll_gestures(&mut self) -> bloody_a90::gesture_runtime::Status {
        if let Some(settings) = self
            .host_buttons
            .as_ref()
            .and_then(|h| h.wheel_settings())
            .or_else(|| self.gestures.as_ref().and_then(|h| h.wheel_settings()))
        {
            self.wheel = settings;
        }
        if let Some(host) = &self.host_buttons
            && host.owns_gestures()
        {
            self.gesture_status = host.gesture_status();
            if !self.gesture_status.running {
                self.capture_suppressed = true;
            }
        } else if let Some(gestures) = &mut self.gestures {
            self.gesture_status = gestures.poll();
            if !self.gesture_status.running {
                self.capture_suppressed = true;
            }
        }
        self.gesture_status.clone()
    }

    fn stop_input_runtimes(&mut self) -> Result<(), String> {
        self.native_gestures = false;
        // Attempt both stops even if the native notification service failed.
        // A separate legacy capture must never retain its physical input grab.
        let host = self.stop_host_buttons();
        let capture = self.gestures.take().map_or(Ok(()), |mut handle| {
            handle.poll();
            let stopped = handle.stop();
            if let Some(settings) = handle.wheel_settings() {
                self.wheel = settings;
            }
            stopped
        });
        self.gesture_status = bloody_a90::gesture_runtime::Status {
            running: false,
            message: "全局手势已停止".into(),
        };
        match (host, capture) {
            (Err(host), Err(capture)) => Err(format!("{host}；{capture}")),
            (Err(error), _) | (_, Err(error)) => Err(error),
            _ => Ok(()),
        }
    }

    fn restart_host_buttons(&mut self, resume_capture: bool) {
        self.poll_gestures();
        if resume_capture {
            self.capture_suppressed = false;
        }
        let result = (|| {
            self.stop_input_runtimes()?;
            self.core = None;
            self.core = Some(Device::open()?.identity()?.core);
            let (_, mut settings, _) = host_button_settings()?;
            if self.capture_suppressed
                && let Some(gestures) = &mut settings.gestures
            {
                gestures.enabled = false;
            }
            if settings.enabled && !settings.is_empty() {
                let native_gestures = settings.gesture_kinds().into_iter().any(|kind| kind);
                if settings.gesture_kinds().into_iter().any(|kind| kind)
                    && let Some(global) = &mut self.gesture_settings
                {
                    global.documents = settings.gestures.as_ref().unwrap().documents.clone();
                }
                let global = (!self.capture_suppressed)
                    .then(|| self.capture_settings())
                    .flatten();
                let mut handle =
                    bloody_a90::host_button_runtime::Handle::start_merged(settings, global)?;
                self.host_button_status = handle.poll();
                self.native_gestures = native_gestures;
                self.host_buttons = Some(handle);
            }
            Ok::<_, String>(())
        })();
        if let Err(message) = result {
            self.host_button_status = bloody_a90::gesture_runtime::Status {
                running: false,
                message,
            };
        }
        if !self.capture_suppressed
            && !self
                .host_buttons
                .as_ref()
                .is_some_and(|host| host.owns_gestures())
            && let Some(settings) = self.capture_settings()
        {
            match bloody_a90::gesture_runtime::Handle::start(settings) {
                Ok(mut handle) => {
                    self.gesture_status = handle.poll();
                    self.gestures = Some(handle);
                }
                Err(message) => {
                    self.gesture_status = bloody_a90::gesture_runtime::Status {
                        running: false,
                        message,
                    };
                }
            }
        }
    }

    fn capture_settings(&self) -> Option<bloody_a90::gesture_runtime::Settings> {
        let mut settings = self.gesture_settings.clone();
        if let Some(settings) = &mut settings {
            settings.wheel = None;
        }
        if let Some(wheel) = self.core.and_then(|core| self.wheel.active_for(core)) {
            settings
                .get_or_insert_with(|| bloody_a90::gesture_runtime::Settings {
                    documents: [
                        bloody_a90::gestures::Document::new(bloody_a90::gestures::Kind::Sixteen),
                        bloody_a90::gestures::Document::new(bloody_a90::gestures::Kind::Mouse),
                    ],
                    triggers: [None; 2],
                    wheel: None,
                })
                .wheel = Some(wheel);
        }
        settings
    }

    fn wheel_enabled(&self) -> bool {
        self.core.is_some_and(|core| self.wheel.enabled_for(core))
    }

    fn save_buttons(
        &mut self,
        config: &Configuration,
        proposed: Option<(bloody_a90::host_buttons::Settings, Option<Vec<u8>>)>,
    ) -> Result<(Box<Snapshot>, String), String> {
        let (path, old_settings, old_bytes) = host_button_settings()?;
        let explicit = proposed.is_some();
        if let Some((_, expected)) = &proposed
            && *expected != old_bytes
        {
            return Err("主机动作文件已改变。请重新连接并读取设置后重试。".into());
        }
        let mut settings = proposed
            .map(|(settings, _)| settings)
            .unwrap_or_else(|| old_settings.clone());
        if !explicit && !settings.is_empty() {
            let current = Device::open()?.read_flash(device::BUTTONS, 16, |_, _| {})?;
            if !settings.matches(&current) {
                settings.profiles.clear();
            }
        }
        settings.reconcile(config);
        settings.bind(config)?;
        bloody_a90::host_button_runtime::preflight(&settings)?;
        let words = configuration::build(config)?;
        if self.backup_file.is_none() {
            self.connect()?;
        }
        self.stop_input_runtimes()?;
        let mut result = (|| {
            let device = Device::open()?;
            bloody_a90::host_buttons::commit(&path, &settings, old_bytes.as_deref(), || {
                device.save(device::BUTTONS, &words)
            })?;
            Ok((
                snapshot(&device)?,
                "按键设置及主机动作已保存，设备读回一致。".into(),
            ))
        })();
        if result.is_ok() {
            self.restart_host_buttons(explicit);
            if settings.enabled
                && !settings.is_empty()
                && !self.host_button_status.running
                && let Ok((_, message)) = &mut result
            {
                *message = format!(
                    "按键设置已保存，但主机动作未启动：{}",
                    self.host_button_status.message
                );
            }
        } else {
            self.host_button_status = bloody_a90::gesture_runtime::Status {
                running: false,
                message: "按键保存失败，主机动作未启动。请检查错误后重新应用。".into(),
            };
        }
        result
    }

    fn update(
        &mut self,
        operation: impl FnOnce(&Device) -> Result<(), String>,
        message: &str,
    ) -> Result<(Box<Snapshot>, String), String> {
        if self.backup_file.is_none() {
            self.connect()?;
        }
        let device = Device::open()?;
        operation(&device)?;
        Ok((snapshot(&device)?, message.into()))
    }

    fn run(&mut self, job: Job) -> Reply {
        match job {
            Job::Backup(path) => Reply::Backup(Backup::read(path)),
            Job::Read(path, purpose) => Reply::File(
                purpose,
                fs::read(&path).map_err(|e| format!("无法读取 {path}：{e}")),
            ),
            Job::Export(path, bytes) => Reply::Export(
                write_new(&PathBuf::from(&path), &bytes)
                    .map(|()| format!("已导出 {} 字节到 {path}", bytes.len())),
            ),
            Job::SaveGesture(path, bytes, expected) => {
                let result = bloody_a90::gesture_file::save(
                    std::path::Path::new(&path),
                    &bytes,
                    expected.as_deref(),
                )
                .map(|()| bytes);
                Reply::GestureSaved(path, result)
            }
            Job::Connect => Reply::Device(Refresh::All, self.connect()),
            Job::Brightness(level) => Reply::Brightness(
                Device::open()
                    .and_then(|device| device.set_brightness(level))
                    .map(|()| level),
            ),
            Job::SaveSensitivity(words) => Reply::Device(
                Refresh::Sensitivity,
                self.update(
                    |device| device.save_sensitivity(&words),
                    "灵敏度已保存并立即应用。运行参数已读回一致。",
                ),
            ),
            Job::SaveRate(words) => Reply::Device(
                Refresh::Rate,
                self.update(
                    |device| device.save_rate(&words),
                    "回报率已保存并立即应用。运行参数已读回一致。",
                ),
            ),
            Job::SaveLift(words) => Reply::Device(
                Refresh::Lift,
                self.update(
                    |device| device.save_lift(&words),
                    "抬起高度已保存。运行值已读回一致。",
                ),
            ),
            Job::CalibrateLift => {
                Reply::CalibratedLift(Device::open().and_then(|device| device.calibrate_lift(5)))
            }
            Job::Shutdown => unreachable!("shutdown is handled by the receive loop"),
            Job::SaveButtons(config) => {
                Reply::Device(Refresh::Buttons, self.save_buttons(&config, None))
            }
            Job::SaveHostButtons(config, settings, expected) => Reply::Device(
                Refresh::Buttons,
                self.save_buttons(&config, Some((*settings, expected))),
            ),
            Job::HostButtonsEnabled(enabled, expected) => Reply::HostButtonsEnabled((|| {
                if !enabled {
                    self.stop_host_buttons()?;
                }
                let (path, mut settings, bytes) = host_button_settings()?;
                if expected != bytes {
                    return Err("主机动作文件已改变。请重新连接并读取设置后重试。".into());
                }
                settings.enabled = enabled;
                if enabled {
                    bloody_a90::host_button_runtime::preflight(&settings)?;
                }
                self.stop_host_buttons()?;
                bloody_a90::host_buttons::save(&path, &settings, bytes.as_deref())?;
                self.restart_host_buttons(enabled);
                let saved = settings.to_bytes();
                Ok((settings, saved, self.host_button_status.clone()))
            })(
            )),
            Job::HostButtonsStatus => {
                if let Some(handle) = &mut self.host_buttons {
                    self.host_button_status = handle.poll();
                }
                Reply::HostButtons(Ok(self.host_button_status.clone()))
            }
            Job::Held => Reply::Held(Device::open().and_then(|device| device.held_buttons())),
            Job::Interrupt => {
                if let Some(gestures) = &self.gestures {
                    gestures.interrupt();
                }
                if let Some(buttons) = &self.host_buttons {
                    buttons.interrupt();
                }
                let interrupted = device::interrupt_macros();
                // Firmware 1b0f can reload its state and clear notification
                // mode. Reconnect the host service after the hardware stop.
                self.restart_host_buttons(false);
                Reply::Export(interrupted.map(|()| {
                    if !self.host_button_status.running
                        && self.host_button_status.message != "按键主机动作已停止"
                    {
                        format!(
                            "设备宏和主机动作已中断。{}",
                            self.host_button_status.message
                        )
                    } else {
                        "设备宏和主机动作已中断".into()
                    }
                }))
            }
            Job::HostRead => Reply::Host(Ok((bloody_a90::host::read(), "已读取桌面设置".into()))),
            Job::HostWrite(settings) => Reply::Host(
                bloody_a90::host::write(&settings)
                    .map(|message| (bloody_a90::host::read(), message)),
            ),
            Job::Associations(rules) => Reply::Associations(rules.validate().map(|()| {
                self.associations = rules;
                self.last_application = None;
            })),
            Job::Foreground => Reply::Foreground(self.foreground()),
            Job::Library(root, command) => Reply::Library(
                bloody_a90::library::Store::new(root)
                    .run(*command)
                    .map(Box::new),
            ),
            Job::Gestures(settings) => Reply::Gestures((|| {
                let apply = settings.is_some();
                if !apply {
                    // Stop physical capture before reading persisted settings.
                    // A damaged file or missing application cannot block stop.
                    self.capture_suppressed = true;
                    self.gesture_settings = None;
                    self.stop_input_runtimes()?;
                }
                let (path, mut host, bytes) = host_button_settings()?;
                if apply
                    && !host.enabled
                    && host.assigned_gesture_kinds().into_iter().any(|kind| kind)
                    && settings
                        .as_ref()
                        .is_some_and(|s| s.triggers.iter().all(Option::is_none))
                {
                    return Err("请先在按键页启用已保存的主机动作".into());
                }
                // Check all logical and native directions before persisting
                // their shared documents or replacing an active capture.
                if let Some(settings) = &settings {
                    let mut proposed = host.clone();
                    if let Some(gestures) = &mut proposed.gestures {
                        gestures.enabled = true;
                        gestures.documents = settings.documents.clone();
                    }
                    bloody_a90::host_button_runtime::preflight_merged(&proposed, Some(settings))?;
                }
                let mut updated = None;
                if host.assigned_gesture_kinds().into_iter().any(|kind| kind) {
                    if let Some(gestures) = &mut host.gestures {
                        gestures.enabled = apply;
                        if let Some(settings) = &settings {
                            gestures.documents = settings.documents.clone();
                        }
                    }
                    bloody_a90::host_buttons::save(&path, &host, bytes.as_deref())?;
                    updated = Some((host.clone(), Some(host.to_bytes())));
                }
                self.gesture_settings = settings
                    .map(|s| *s)
                    .filter(|settings| settings.triggers.iter().any(Option::is_some));
                self.capture_suppressed = !apply && !self.wheel_enabled();
                self.restart_host_buttons(apply || self.wheel_enabled());
                let mut status = self.poll_gestures();
                if !apply && status.running {
                    status.running = false;
                    status.message = "全局手势已停止，区域滚动继续运行".into();
                }
                Ok(GestureUpdate {
                    requested: apply,
                    status,
                    host: updated,
                })
            })()),
            Job::GestureStatus => {
                let status = self.poll_gestures();
                Reply::GestureStatus(
                    status,
                    self.native_gestures || self.gesture_settings.is_some(),
                    self.wheel_enabled(),
                )
            }
            Job::WheelRead => Reply::Wheel((|| {
                let path = bloody_a90::wheel4d::path().ok_or("无法确定滚轮设置文件路径")?;
                let (settings, bytes) = bloody_a90::wheel4d::load(&path)?;
                Ok(WheelUpdate {
                    settings,
                    bytes,
                    status: self.poll_gestures(),
                    runtime_enabled: self.wheel_enabled(),
                })
            })()),
            Job::WheelWrite(settings, expected) => Reply::Wheel((|| {
                settings.validate()?;
                let path = bloody_a90::wheel4d::path().ok_or("无法确定滚轮设置文件路径")?;
                if settings.cores.into_iter().any(|enabled| enabled) {
                    self.core = Some(Device::open()?.identity()?.core);
                }
                if self.core.is_some_and(|core| settings.enabled_for(core)) {
                    let previous = std::mem::replace(&mut self.wheel, settings.clone());
                    let capture = self.capture_settings().unwrap();
                    self.wheel = previous;
                    let (_, native, _) = host_button_settings()?;
                    bloody_a90::host_button_runtime::preflight_merged(&native, Some(&capture))?;
                }
                bloody_a90::wheel4d::save(&path, &settings, expected.as_deref())?;
                self.wheel = settings.clone();
                // An explicit apply can resume capture after the emergency chord.
                self.restart_host_buttons(self.wheel_enabled());
                Ok(WheelUpdate {
                    bytes: Some(settings.to_bytes()),
                    settings,
                    status: self.poll_gestures(),
                    runtime_enabled: self.wheel_enabled(),
                })
            })()),
        }
    }
}

pub fn spawn(
    jobs: Receiver<Job>,
    results: Sender<Reply>,
    wake: egui::Context,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut worker = Worker::new();
        while let Ok(job) = jobs.recv() {
            if matches!(job, Job::Shutdown) {
                break;
            }
            if results.send(worker.run(job)).is_err() {
                break;
            }
            wake.request_repaint();
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_capture_settings_keep_gesture_documents_and_allow_wheel_only() {
        let mut worker = Worker::new();
        worker.wheel = bloody_a90::wheel4d::Settings::default();
        worker.core = Some(2);
        assert!(worker.capture_settings().is_none());
        worker.wheel.cores[1] = true;
        let wheel_only = worker.capture_settings().unwrap();
        assert_eq!(wheel_only.triggers, [None; 2]);
        assert_eq!(wheel_only.wheel.as_ref().unwrap().settings(), &worker.wheel);
        let mut combined = wheel_only;
        combined.triggers[1] = Some(bloody_a90::linux_input::BUTTONS[4]);
        combined.documents[1].actions[0] = bloody_a90::gestures::Action::keyboard(27, 1, "剪切");
        worker.gesture_settings = Some(combined.clone());
        let capture = worker.capture_settings().unwrap();
        assert_eq!(capture.triggers, combined.triggers);
        assert_eq!(capture.documents, combined.documents);
        worker.wheel.cores[1] = false;
        assert!(worker.capture_settings().unwrap().wheel.is_none());
        worker.gesture_settings = None;
        worker.wheel.cores[2] = true;
        assert!(worker.capture_settings().is_none());
        worker.core = Some(3);
        assert_eq!(worker.capture_settings().unwrap().wheel.unwrap().core(), 3);
        worker.core = None;
        assert!(worker.capture_settings().is_none());
    }

    #[test]
    #[ignore = "requires A90_TEST_ASSOCIATIONS=1, a mouse, and a stable foreground window"]
    fn associations_switch_and_restore() {
        assert_eq!(std::env::var("A90_TEST_ASSOCIATIONS").as_deref(), Ok("1"));
        let device = Device::open().unwrap();
        let initial = device.runtime_profile().unwrap();
        assert!(initial.1 >= 1);
        struct Restore(u8);
        impl Drop for Restore {
            fn drop(&mut self) {
                if let Ok(device) = Device::open() {
                    let _ = device.select_profile(self.0);
                }
            }
        }
        let _restore = Restore(initial.0);
        let debounce = device.ram(0x25).unwrap();
        drop(device);
        let target = if initial.0 == 0 { 1 } else { 0 };
        let mut worker = Worker::new();
        let app = worker.foreground().unwrap().app.unwrap();
        let rules = Rules {
            enabled: true,
            fallback: initial.0,
            entries: vec![bloody_a90::associations::Rule {
                app: app.clone(),
                profile: target,
            }],
        };
        assert!(matches!(
            worker.run(Job::Associations(rules)),
            Reply::Associations(Ok(()))
        ));
        let result = worker.foreground().unwrap();
        assert_eq!(
            result.app.as_deref(),
            Some(app.as_str()),
            "foreground changed during test"
        );
        assert_eq!(result.profile, Some((target, initial.1)));
        let device = Device::open().unwrap();
        assert_eq!(device.runtime_profile().unwrap(), (target, initial.1));
        assert_eq!(device.ram(0x25).unwrap(), debounce);
        // A manual change must persist while the foreground app stays the same.
        device.select_profile(initial.0).unwrap();
        drop(device);
        assert_eq!(worker.foreground().unwrap().profile, None);
        assert_eq!(Device::open().unwrap().runtime_profile().unwrap(), initial);
        assert!(matches!(
            worker.run(Job::Associations(Rules::default())),
            Reply::Associations(Ok(()))
        ));
        assert_eq!(worker.foreground().unwrap().profile, None);
        assert_eq!(Device::open().unwrap().runtime_profile().unwrap(), initial);
    }
}
