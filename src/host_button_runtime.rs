//! Run desktop actions from vendor button notifications. Native gesture slots
//! share one physical input capture with optional ordinary gesture triggers.
//! The hardware table contains no-operations for all host action slots.
use crate::{
    device::{BUTTONS, Device},
    gesture_runtime::{Devices, Status},
    host_buttons::{SLOTS, Settings},
    host_program::{self, Prepared, Vm},
    vendor_input::{Monitor, State},
};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

fn prepare(settings: &Settings) -> Result<Vec<[Option<Prepared>; 2]>, String> {
    settings.validate_actions()?;
    settings
        .profiles
        .iter()
        .flatten()
        .map(|action| {
            if action
                .as_ref()
                .is_some_and(|a| a.number("Simulator") == Ok(2))
            {
                return Ok([None, None]);
            }
            action
                .as_ref()
                .map(|action| {
                    Ok([
                        Some(host_program::prepare_button(action, true)?),
                        Some(host_program::prepare_button(action, false)?),
                    ])
                })
                .unwrap_or(Ok([None, None]))
        })
        .collect()
}

/// Do all desktop and action checks before a hardware save starts.
pub fn preflight(settings: &Settings) -> Result<(), String> {
    preflight_merged(settings, None)
}

pub fn preflight_merged(
    settings: &Settings,
    global: Option<&crate::gesture_runtime::Settings>,
) -> Result<(), String> {
    if settings.enabled && !settings.is_empty() {
        let prepared = prepare(settings)?;
        let all = prepared.into_iter().flatten().flatten().collect::<Vec<_>>();
        let _devices = Devices::for_host_actions(&all)?;
        if let Some((capture, kinds)) = capture_settings(settings, global.cloned()) {
            crate::gesture_runtime::Handle::preflight_external(&capture, kinds)?;
        } else if let Some(global) = global {
            crate::gesture_runtime::Handle::preflight(global)?;
        }
    } else if let Some(global) = global {
        crate::gesture_runtime::Handle::preflight(global)?;
    }
    Ok(())
}

#[derive(Default)]
struct Edges {
    previous: Option<State>,
    blocked: [u8; 3],
}

impl Edges {
    fn events(&mut self, state: State) -> Vec<(usize, usize)> {
        let previous = self.previous.replace(state);
        let Some(previous) = previous else {
            self.blocked = state.buttons;
            return Vec::new();
        };
        // A configuration change establishes a new baseline. It must not
        // interpret a button held in the old profile as a new press.
        if previous.profile != state.profile {
            self.blocked = state.buttons;
            return Vec::new();
        }
        let blocked = self.blocked;
        for (target, value) in self.blocked.iter_mut().zip(state.buttons) {
            *target &= value;
        }
        let mut events = Vec::new();
        for slot in 0..24 {
            let mask = 1 << (slot % 8);
            if (previous.buttons[slot / 8] ^ state.buttons[slot / 8]) & mask != 0
                && blocked[slot / 8] & mask == 0
            {
                events.push((slot, usize::from(state.buttons[slot / 8] & mask == 0)));
            }
        }
        if state.wheel != 0 {
            events.push((if state.wheel < 0 { 24 } else { 25 }, 0));
        }
        events
    }
}

pub struct Handle {
    stop: Arc<AtomicBool>,
    interrupt: Arc<AtomicBool>,
    status: Receiver<String>,
    thread: Option<JoinHandle<Result<(), String>>>,
    last: Status,
    capture_status: Arc<Mutex<Status>>,
    capture_remote: Option<crate::gesture_runtime::Remote>,
}

fn capture_settings(
    settings: &Settings,
    global: Option<crate::gesture_runtime::Settings>,
) -> Option<(crate::gesture_runtime::Settings, [bool; 2])> {
    let kinds = settings.gesture_kinds();
    if !kinds.into_iter().any(|kind| kind) {
        return global.map(|settings| (settings, kinds));
    }
    let documents = settings.gestures.as_ref()?.documents.clone();
    Some((
        crate::gesture_runtime::Settings {
            documents,
            triggers: global.as_ref().map_or([None; 2], |g| g.triggers),
            wheel: global.and_then(|g| g.wheel),
        },
        kinds,
    ))
}

impl Handle {
    pub fn start(settings: Settings) -> Result<Self, String> {
        Self::start_merged(settings, None)
    }

    pub fn owns_gestures(&self) -> bool {
        self.capture_remote.is_some()
    }

    pub fn wheel_settings(&self) -> Option<crate::wheel4d::Settings> {
        self.capture_remote.as_ref()?.wheel_settings()
    }

    pub fn gesture_status(&self) -> Status {
        self.capture_status
            .lock()
            .map(|s| s.clone())
            .unwrap_or(Status {
                running: false,
                message: "手势状态读取失败".into(),
            })
    }

    pub fn start_merged(
        settings: Settings,
        global: Option<crate::gesture_runtime::Settings>,
    ) -> Result<Self, String> {
        if !settings.enabled || settings.is_empty() {
            return Err("没有启用按键主机动作".into());
        }
        let signature = settings.signature.ok_or("主机动作尚未绑定保存的按键表")?;
        let prepared = prepare(&settings)?;
        let all = prepared
            .iter()
            .flatten()
            .flatten()
            .cloned()
            .collect::<Vec<_>>();
        let devices = Devices::for_host_actions(&all)?;
        let device = Device::open()?;
        settings.verify_device(&device)?;
        // Allow udev and the compositor to open the generated devices before
        // vendor notifications can start a shortcut or zoom action.
        thread::sleep(Duration::from_millis(300));
        let mut monitor = Monitor::start(&device)?;
        drop(device);
        let mut capture = if let Some((capture, kinds)) = capture_settings(&settings, global) {
            Some(crate::gesture_runtime::Handle::start_external(
                capture, kinds,
            )?)
        } else {
            None
        };
        // Ignore notifications collected while desktop services and evdev were
        // starting. Request a fresh baseline before accepting a physical press.
        let _ = monitor.read()?;
        Device::open()?.request_notification()?;
        let capture_remote = capture.as_ref().and_then(|handle| handle.remote());
        let bridge = capture_remote.clone();
        let capture_status = Arc::new(Mutex::new(Status {
            running: capture.is_some(),
            message: if capture.is_some() {
                "按键手势已启用"
            } else {
                "按键手势未启用"
            }
            .into(),
        }));
        let shared_capture = capture_status.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let interrupt = Arc::new(AtomicBool::new(false));
        let interrupted = interrupt.clone();
        let (messages, status) = mpsc::sync_channel(32);
        let thread = thread::spawn(move || {
            // Feature queries never delay the notification reader. A changed
            // main-vector header stops this runtime instead of using old rules.
            let audit_stop = Arc::new(AtomicBool::new(false));
            let audit_stopping = audit_stop.clone();
            let (failed, failure) = mpsc::sync_channel(1);
            let audit = thread::spawn(move || {
                while !audit_stopping.load(Ordering::Acquire) {
                    let checked = (|| {
                        if let Some(device) = Device::try_open()? {
                            if device.read_flash(BUTTONS, 16, |_, _| {})? != signature {
                                return Err(
                                    "鼠标按键表已改变，按键主机动作已停止。请重新读取并应用设置。"
                                        .into(),
                                );
                            }
                            if device.ram(0x4b)? & 1 == 0 {
                                return Err("鼠标通知模式已关闭，按键主机动作已停止。".into());
                            }
                        }
                        Ok::<_, String>(())
                    })();
                    if let Err(error) = checked {
                        let _ = failed.try_send(error);
                        break;
                    }
                    for _ in 0..50 {
                        if audit_stopping.load(Ordering::Acquire) {
                            return;
                        }
                        thread::sleep(Duration::from_millis(10));
                    }
                }
            });
            struct Audit {
                stop: Arc<AtomicBool>,
                thread: Option<JoinHandle<()>>,
            }
            impl Drop for Audit {
                fn drop(&mut self) {
                    self.stop.store(true, Ordering::Release);
                    if let Some(thread) = self.thread.take() {
                        let _ = thread.join();
                    }
                }
            }
            let audit = Audit {
                stop: audit_stop,
                thread: Some(audit),
            };
            let result = run(
                || {
                    let states = monitor.read()?;
                    if let Some(capture) = &mut capture {
                        let state = capture.poll();
                        if state.running
                            && let Some(bridge) = &bridge
                        {
                            for snapshot in &states {
                                bridge
                                    .update(*snapshot, settings.gesture_slots(snapshot.profile))?;
                            }
                        }
                        if let Ok(mut shared) = shared_capture.lock() {
                            *shared = state;
                        }
                    }
                    Ok(states)
                },
                devices,
                prepared,
                &stopping,
                &interrupted,
                &failure,
                |message| {
                    let _ = messages.try_send(message);
                },
            );
            // Restore physical input before waiting for HID access or audit.
            drop(capture);
            if let Ok(mut status) = shared_capture.lock() {
                status.running = false;
            }
            drop(audit);
            let restored = monitor.restore();
            match (result, restored) {
                (Err(error), Err(restore)) => Err(format!("{error}；恢复通知模式失败：{restore}")),
                (Err(error), _) | (_, Err(error)) => Err(error),
                _ => Ok(()),
            }
        });
        Ok(Self {
            stop,
            interrupt,
            status,
            thread: Some(thread),
            last: Status {
                running: true,
                message: "按键主机动作已启用".into(),
            },
            capture_status,
            capture_remote,
        })
    }

    pub fn interrupt(&self) {
        self.interrupt.store(true, Ordering::Release);
        if let Some(remote) = &self.capture_remote {
            remote.interrupt();
        }
    }

    pub fn poll(&mut self) -> Status {
        for message in self.status.try_iter() {
            self.last.message = message;
        }
        if self
            .thread
            .as_ref()
            .is_some_and(|thread| thread.is_finished())
        {
            let result = self
                .thread
                .take()
                .unwrap()
                .join()
                .map_err(|_| "按键主机动作线程异常退出".to_string())
                .and_then(|r| r);
            self.last = Status {
                running: false,
                message: result.err().unwrap_or_else(|| "按键主机动作已停止".into()),
            };
        }
        self.last.clone()
    }

    pub fn stop(&mut self) -> Result<(), String> {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| "按键主机动作线程异常退出".to_string())??;
        }
        self.last = Status {
            running: false,
            message: "按键主机动作已停止".into(),
        };
        Ok(())
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn run(
    mut read: impl FnMut() -> Result<Vec<State>, String>,
    mut devices: Devices,
    prepared: Vec<[Option<Prepared>; 2]>,
    stop: &AtomicBool,
    interrupt: &AtomicBool,
    failure: &Receiver<String>,
    mut report: impl FnMut(String),
) -> Result<(), String> {
    let result = (|| {
        let mut edges = Edges::default();
        let mut queue = VecDeque::new();
        let mut active: Option<Vm> = None;
        let mut physical = [0; 3];
        while !stop.load(Ordering::Acquire) {
            if let Ok(error) = failure.try_recv() {
                return Err(error);
            }
            if interrupt.swap(false, Ordering::AcqRel) {
                queue.clear();
                active = None;
                devices.cancel_pending();
                devices.clear_generated()?;
                // Notifications queued before the interrupt must not start a
                // fresh action. Retain their latest state as a blocked baseline.
                let latest = read()?.last().copied().or(edges.previous);
                edges = Edges::default();
                if let Some(state) = latest {
                    let _ = edges.events(state);
                    physical = state.buttons;
                }
            }
            for state in read()? {
                physical = state.buttons;
                let changed = edges
                    .previous
                    .is_some_and(|previous| previous.profile != state.profile);
                if changed {
                    queue.clear();
                    active = None;
                    devices.cancel_pending();
                    devices.clear_generated()?;
                }
                for (slot, phase) in edges.events(state) {
                    if let Some(Some(action)) = prepared
                        .get(state.profile as usize * SLOTS + slot)
                        .map(|pair| &pair[phase])
                    {
                        if queue.len() >= 128 {
                            return Err("按键主机动作队列已满".into());
                        }
                        queue.push_back(action.clone());
                    }
                }
            }
            let now = Instant::now();
            if active.is_none()
                && let Some(action) = queue.pop_front()
            {
                active = Some(Vm::new(action, now, false));
            }
            if let Some(vm) = &mut active
                && vm.ready(now)
            {
                for output in vm.tick(now, physical)? {
                    let desktop = matches!(
                        output,
                        host_program::Output::Open(_)
                            | host_program::Output::Text(_)
                            | host_program::Output::Window(_)
                    );
                    if let Err(error) = devices.output(output) {
                        if desktop {
                            report(error);
                        } else {
                            return Err(error);
                        }
                    }
                }
                if vm.finished() {
                    devices.clear_generated()?;
                    active = None;
                }
            }
            for error in devices.poll() {
                report(error);
            }
            thread::sleep(Duration::from_millis(1));
        }
        devices.clear_generated()
    })();
    // Release generated keys before feature access or desktop worker cleanup.
    drop(devices);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_capture_keeps_wheel_with_and_without_native_gestures() {
        let mut settings = Settings::default();
        settings.resize(1);
        let global = crate::gesture_runtime::Settings {
            documents: [
                crate::gestures::Document::new(crate::gestures::Kind::Sixteen),
                crate::gestures::Document::new(crate::gestures::Kind::Mouse),
            ],
            triggers: [Some(crate::linux_input::BUTTONS[3]), None],
            wheel: crate::wheel4d::Settings {
                cores: [true; 4],
                ..Default::default()
            }
            .active_for(2),
        };
        let (capture, kinds) = capture_settings(&settings, Some(global.clone())).unwrap();
        assert_eq!(kinds, [false; 2]);
        assert_eq!(capture.wheel, global.wheel);
        assert_eq!(capture.triggers, global.triggers);
        settings.gestures = Some(Box::new(crate::host_buttons::Gestures {
            enabled: true,
            documents: global.documents.clone(),
        }));
        settings.profiles[0][8] = Some(crate::gestures::Action::new(2, 0, 0, "手势"));
        let (capture, kinds) = capture_settings(&settings, Some(global.clone())).unwrap();
        assert_eq!(kinds, [true, false]);
        assert_eq!(capture.wheel, global.wheel);
        assert_eq!(capture.triggers, global.triggers);
    }

    fn state(profile: u8, buttons: [u8; 3]) -> State {
        State {
            profile,
            buttons,
            wheel: 0,
            sensitivity: 0,
        }
    }

    #[test]
    #[ignore = "requires A90_TEST_HOST_BUTTONS=1 and the A90; no flash writes or generated input"]
    fn native_runtime_keeps_feature_access_excludes_duplicate_owners_and_restores_mode() {
        assert_eq!(std::env::var("A90_TEST_HOST_BUTTONS").as_deref(), Ok("1"));
        let device = Device::open().unwrap();
        let backup = crate::backup::Backup::parse(&device.read_backup(|_, _| {}).unwrap()).unwrap();
        let config = backup.configuration().unwrap();
        let original_profile = device.runtime_profile().unwrap();
        let original_mode = device.ram(0x4b).unwrap();
        let signature: [u16; 16] = device
            .read_flash(BUTTONS, 16, |_, _| {})
            .unwrap()
            .try_into()
            .unwrap();
        assert_eq!(
            signature[0], 0xa4a4,
            "test requires an existing saved main vector"
        );
        let slot = (0..24)
            .find(|slot| {
                ![0, 1, 2, 3, 4, 8, 9, 10].contains(slot)
                    && config.profiles.iter().all(|profile| {
                        profile.buttons[*slot] == crate::configuration::Binding::default()
                    })
            })
            .expect("no unused hardware slot available");
        let mut settings = Settings::default();
        settings.resize(config.profiles.len());
        for profile in &mut settings.profiles {
            profile[slot] = Some(
                crate::gestures::Action::open_target("/usr/bin/true")
                    .unwrap()
                    .with_mode(0),
            );
        }
        settings.bind(&config).unwrap();
        settings.signature = Some(signature);
        drop(device);
        let mut handle = Handle::start(settings.clone()).unwrap();
        assert!(handle.poll().running);
        assert_eq!(
            Device::open().unwrap().ram(0x4b).unwrap(),
            original_mode | 1
        );
        match Handle::start(settings.clone()) {
            Err(error) => assert!(error.contains("占用")),
            Ok(_) => panic!("a second native runtime acquired the same notification mode"),
        }
        assert_eq!(Device::open().unwrap().identity().unwrap().firmware, 7);
        thread::sleep(Duration::from_millis(600));
        assert!(handle.poll().running);
        // A hardware interrupt can clear notification mode. The service must
        // detect this state change and release its devices and owner token.
        Device::open()
            .unwrap()
            .set_notification_mode(false)
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let stopped = loop {
            let status = handle.poll();
            if !status.running {
                break status;
            }
            assert!(
                Instant::now() < deadline,
                "closed notification mode was not detected"
            );
            thread::sleep(Duration::from_millis(5));
        };
        assert!(stopped.message.contains("通知模式已关闭"));
        handle.stop().unwrap();
        assert_eq!(Device::open().unwrap().ram(0x4b).unwrap(), original_mode);
        // The stopped owner must release its abstract socket and all handles.
        let mut documents = [
            crate::gestures::Document::new(crate::gestures::Kind::Sixteen),
            crate::gestures::Document::new(crate::gestures::Kind::Mouse),
        ];
        documents[1].tip = false;
        settings.gestures = Some(Box::new(crate::host_buttons::Gestures {
            enabled: true,
            documents,
        }));
        for profile in &mut settings.profiles {
            profile[slot] = Some(crate::gestures::Action::new(2, 0, 0, "空手势"));
        }
        settings.bind(&config).unwrap();
        settings.signature = Some(signature);
        let mut restarted = Handle::start(settings).unwrap();
        assert!(restarted.owns_gestures());
        assert!(restarted.gesture_status().running);
        thread::sleep(Duration::from_millis(600));
        assert!(restarted.poll().running);
        restarted.stop().unwrap();
        assert!(!restarted.gesture_status().running);
        let info = crate::linux_input::find().unwrap();
        let mut source = crate::linux_input::Source::open(&info.path).unwrap();
        source.grab().unwrap();
        drop(source);
        let device = Device::open().unwrap();
        assert_eq!(device.ram(0x4b).unwrap(), original_mode);
        assert_eq!(device.runtime_profile().unwrap(), original_profile);
        assert_eq!(device.read_backup(|_, _| {}).unwrap(), backup.to_bytes());
    }

    #[test]
    fn native_edges_ignore_startup_holds_profile_changes_and_repeated_states() {
        let mut edges = Edges::default();
        assert!(edges.events(state(0, [0, 4, 0])).is_empty());
        assert!(edges.events(state(0, [0; 3])).is_empty());
        assert_eq!(edges.events(state(0, [0, 4, 0])), [(10, 0)]);
        assert!(edges.events(state(0, [0, 4, 0])).is_empty());
        assert!(edges.events(state(1, [0, 4, 0])).is_empty());
        assert!(edges.events(state(1, [0; 3])).is_empty());
        assert_eq!(edges.events(state(1, [0, 4, 0])), [(10, 0)]);
        assert_eq!(edges.events(state(1, [0; 3])), [(10, 1)]);
        let mut wheel = state(1, [0; 3]);
        wheel.wheel = 1;
        assert_eq!(edges.events(wheel), [(25, 0)]);
        assert_eq!(edges.events(wheel), [(25, 0)]);
        wheel.wheel = -1;
        assert_eq!(edges.events(wheel), [(24, 0)]);
    }

    #[test]
    #[ignore = "requires /dev/uinput; reads synthetic vendor frames and grabs only its own generated devices"]
    fn native_frames_select_profiles_generate_zoom_and_launch_literal_arguments() {
        use crate::linux_input as input;
        use std::{fs, io::Write, os::fd::FromRawFd};
        let dir = std::env::temp_dir().join(format!("a90-native-loop-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        struct Directory(std::path::PathBuf);
        impl Drop for Directory {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _directory = Directory(dir.clone());
        let executable = dir.join("touch $() tool");
        fs::copy("/usr/bin/touch", &executable).unwrap();
        let marker = dir.join("参数 中😀 $()");
        let mut settings = Settings::default();
        settings.resize(2);
        settings.profiles[0][10] = Some(crate::gestures::Action::new(22, 0, 0, "放大"));
        settings.profiles[1][10] = Some(
            crate::gestures::Action::open_target_with_arguments(
                executable.to_str().unwrap(),
                marker.to_str().unwrap(),
            )
            .unwrap()
            .with_mode(0),
        );
        settings.profiles[1][3] = settings.profiles[0][10].clone();
        settings.profiles[0][24] = Some(crate::gestures::Action::new(9, 9, 5, "滚轮向左"));
        settings.profiles[1][25] = Some(crate::gestures::Action::new(9, 9, 6, "滚轮向右"));
        let mut prepared = prepare(&settings).unwrap();
        // Keep Ctrl held long enough to check interruption deterministically.
        let mut delayed = prepared[10][0].clone().unwrap();
        delayed.press = vec![0x2301, 0x2be7, 0x262b, 0x2201, crate::storage::END].into();
        prepared[3][0] = Some(delayed);
        let all = prepared
            .iter()
            .flatten()
            .flatten()
            .cloned()
            .collect::<Vec<_>>();
        let devices = Devices::for_host_actions(&all).unwrap();
        let [(mut pointer, pointer_sys), (mut keyboard, keyboard_sys)] =
            devices.fixture_sources().unwrap();
        pointer.grab().unwrap();
        keyboard.grab().unwrap();
        let mut descriptors = [0; 2];
        assert_eq!(
            unsafe { libc::pipe2(descriptors.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) },
            0
        );
        let mut source = crate::vendor_input::Source::fixture(unsafe {
            std::fs::File::from_raw_fd(descriptors[0])
        });
        let mut sender = unsafe { std::fs::File::from_raw_fd(descriptors[1]) };
        let send = |sender: &mut std::fs::File, state: State| {
            let mut report = [0; crate::device::REPORT];
            report[0] = 7;
            report[1] = 1;
            report[2] = match state.wheel {
                -1 => 0xf0,
                1 => 0x10,
                _ => 0,
            };
            report[3..6].copy_from_slice(&state.buttons);
            report[7] = state.profile;
            sender.write_all(&report).unwrap();
        };
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let interrupt = Arc::new(AtomicBool::new(false));
        let interrupted = interrupt.clone();
        let (errors, error) = mpsc::channel();
        let (_failure, failure) = mpsc::channel();
        let thread = thread::spawn(move || {
            run(
                || source.read(),
                devices,
                prepared,
                &stopping,
                &interrupted,
                &failure,
                |message| {
                    let _ = errors.send(message);
                },
            )
        });
        struct Cleanup {
            stop: Arc<AtomicBool>,
            thread: Option<JoinHandle<Result<(), String>>>,
        }
        impl Drop for Cleanup {
            fn drop(&mut self) {
                self.stop.store(true, Ordering::Release);
                if let Some(thread) = self.thread.take() {
                    let _ = thread.join();
                }
            }
        }
        let mut cleanup = Cleanup {
            stop,
            thread: Some(thread),
        };
        let collect = |source: &mut input::Source, kind, code, value| {
            let deadline = Instant::now() + Duration::from_secs(2);
            let mut events = Vec::new();
            loop {
                events.extend(source.read().unwrap());
                if events
                    .iter()
                    .any(|e| (e.kind, e.code, e.value) == (kind, code, value))
                {
                    return events;
                }
                assert!(
                    Instant::now() < deadline,
                    "expected generated event did not arrive"
                );
                thread::sleep(Duration::from_millis(1));
            }
        };
        send(&mut sender, state(0, [0; 3]));
        send(&mut sender, state(0, [0, 4, 0]));
        send(&mut sender, state(0, [0; 3]));
        let events = collect(&mut keyboard, input::KEY, 29, 0);
        assert_eq!(
            events
                .iter()
                .filter(|e| e.kind == input::KEY)
                .map(|e| (e.code, e.value))
                .collect::<Vec<_>>(),
            [(29, 1), (29, 0)]
        );
        collect(&mut pointer, input::REL, input::WHEEL, 1);
        assert!(keyboard.held().unwrap().is_empty());
        assert!(!marker.exists());
        let mut horizontal = state(0, [0; 3]);
        horizontal.wheel = -1;
        send(&mut sender, horizontal);
        let events = collect(&mut pointer, input::REL, 12, -120);
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == input::REL && event.code == input::HWHEEL)
                .map(|event| event.value)
                .collect::<Vec<_>>(),
            [-1]
        );
        send(&mut sender, state(0, [8, 0, 0]));
        collect(&mut keyboard, input::KEY, 29, 1);
        assert!(keyboard.held().unwrap().contains(&29));
        interrupt.store(true, Ordering::Release);
        collect(&mut keyboard, input::KEY, 29, 0);
        assert!(keyboard.held().unwrap().is_empty());
        send(&mut sender, state(0, [0; 3]));
        send(&mut sender, state(1, [0; 3]));
        send(&mut sender, state(1, [0, 4, 0]));
        send(&mut sender, state(1, [0; 3]));
        let until = Instant::now() + Duration::from_secs(2);
        while !marker.exists() {
            assert!(
                Instant::now() < until,
                "profile action did not create its literal marker"
            );
            thread::sleep(Duration::from_millis(2));
        }
        // A prepared target can disappear. Its error must reach the UI while
        // later button actions continue to execute.
        let mut horizontal = state(1, [0; 3]);
        horizontal.wheel = 1;
        send(&mut sender, horizontal);
        let events = collect(&mut pointer, input::REL, 12, 120);
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == input::REL && event.code == input::HWHEEL)
                .map(|event| event.value)
                .collect::<Vec<_>>(),
            [1]
        );
        fs::remove_file(&executable).unwrap();
        send(&mut sender, state(1, [0, 4, 0]));
        send(&mut sender, state(1, [0; 3]));
        assert!(
            error
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .starts_with("无法打开目标")
        );
        send(&mut sender, state(1, [8, 0, 0]));
        send(&mut sender, state(1, [0; 3]));
        collect(&mut keyboard, input::KEY, 29, 0);
        assert!(keyboard.held().unwrap().is_empty());
        cleanup.stop.store(true, Ordering::Release);
        cleanup.thread.take().unwrap().join().unwrap().unwrap();
        for name in [pointer_sys, keyboard_sys] {
            assert!(
                !std::path::Path::new("/sys/devices/virtual/input")
                    .join(name)
                    .exists()
            );
        }
    }
}
