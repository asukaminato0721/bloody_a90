//! Global A90 gestures. Physical input is mirrored through uinput while active.
mod physical_buttons;
use crate::{
    gestures::{Document, Kind, Trace16, TraceMouse, direction8},
    host_program::{self, Output, Prepared, Vm},
    linux_input::{self as input, Event, Source, Virtual},
    macros::Repeat,
};
use std::{
    collections::{BTreeSet, VecDeque},
    sync::mpsc::{self, Receiver, SyncSender},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub struct Settings {
    pub documents: [Document; 2],
    pub triggers: [Option<u16>; 2],
}

#[cfg(test)]
fn prepare_actions(settings: &Settings) -> Result<Vec<Vec<Prepared>>, String> {
    prepare_external_actions(settings, [false; 2])
}

fn prepare_external_actions(
    settings: &Settings,
    external: [bool; 2],
) -> Result<Vec<Vec<Prepared>>, String> {
    let mut prepared = Vec::new();
    for (kind, external) in external.into_iter().enumerate() {
        let document = &settings.documents[kind];
        let mut actions = Vec::new();
        for (index, action) in document.actions.iter().enumerate() {
            // Instant 16-in-1 recognizes the first eight directions only.
            let enabled = (settings.triggers[kind].is_some() || external)
                && !(document.kind == Kind::Sixteen && document.realtime && index >= 8);
            let action = if enabled {
                host_program::prepare(action).map_err(|e| {
                    format!(
                        "{}，{}：{e}",
                        document.kind.name(),
                        document.kind.shapes()[index].name
                    )
                })?
            } else {
                host_program::prepare(&crate::gestures::Action::default())?
            };
            actions.push(action);
        }
        prepared.push(actions);
    }
    Ok(prepared)
}

#[derive(Clone, Debug)]
pub struct Status {
    pub running: bool,
    pub message: String,
}

#[derive(Debug)]
enum Decision {
    Forward(Event),
    Activate(usize, usize, bool),
    Released(usize),
    Click(u16),
    Cancel,
}

const NATIVE_BASE: u16 = 0x2c0;
fn native_slot(code: u16) -> Option<usize> {
    (NATIVE_BASE..NATIVE_BASE + 24)
        .contains(&code)
        .then(|| (code - NATIVE_BASE) as usize)
}

#[derive(Clone)]
pub(crate) struct Remote {
    sender: SyncSender<Native>,
    allowed: [bool; 2],
    control: mpsc::Sender<Control>,
}

#[derive(Clone, Copy)]
struct Native {
    state: crate::vendor_input::State,
    kinds: [Option<usize>; 24],
}

fn check_native_emergency(state: crate::vendor_input::State) -> Result<(), String> {
    if state.buttons[0] & 6 == 6 {
        return Err("已同时按下右键和中键。全局手势已停止。".into());
    }
    Ok(())
}

impl Remote {
    pub(crate) fn interrupt(&self) {
        let _ = self.control.send(Control::Interrupt);
    }
    pub(crate) fn update(
        &self,
        state: crate::vendor_input::State,
        mut kinds: [Option<usize>; 24],
    ) -> Result<(), String> {
        for kind in &mut kinds {
            if kind.is_some_and(|k| k >= 2 || !self.allowed[k]) {
                *kind = None;
            }
        }
        self.sender
            .try_send(Native { state, kinds })
            .map_err(|_| "按键手势通知队列已满或服务已停止".into())
    }
}

struct Stroke {
    kind: usize,
    button: u16,
    trace16: Trace16,
    mouse: TraceMouse,
    displacement: [i32; 2],
    realtime: [i32; 2],
    moved: bool,
    cancelled: bool,
    trajectory: Vec<[i32; 2]>,
}

struct Engine {
    settings: Settings,
    stroke: Option<Stroke>,
    delta: [i32; 2],
    held: BTreeSet<u16>,
    native_previous: Option<crate::vendor_input::State>,
    native_triggers: [Option<usize>; 24],
    native_blocked: [u8; 3],
}

impl Engine {
    fn new(settings: Settings) -> Self {
        Self {
            settings,
            stroke: None,
            delta: [0; 2],
            held: BTreeSet::new(),
            native_previous: None,
            native_triggers: [None; 24],
            native_blocked: [0; 3],
        }
    }

    fn native_baseline(&mut self, native: Native) {
        self.native_previous = Some(native.state);
        self.native_triggers = native.kinds;
        self.native_blocked = native.state.buttons;
        self.held.retain(|code| native_slot(*code).is_none());
        for (slot, kind) in native.kinds.iter().enumerate() {
            if kind.is_some() && native.state.buttons[slot / 8] & (1 << (slot % 8)) != 0 {
                self.held.insert(NATIVE_BASE + slot as u16);
            }
        }
    }

    fn native(&mut self, native: Native) -> Result<Vec<Decision>, String> {
        check_native_emergency(native.state)?;
        let previous = self.native_previous;
        if previous.is_none()
            || previous.is_some_and(|p| p.profile != native.state.profile)
            || self.native_triggers != native.kinds
        {
            let changed = previous.is_some();
            if changed {
                self.stroke = None;
                self.delta = [0; 2];
            }
            self.native_baseline(native);
            return Ok(if changed {
                vec![Decision::Cancel]
            } else {
                Vec::new()
            });
        }
        let previous = previous.unwrap();
        self.native_previous = Some(native.state);
        let mut output = Vec::new();
        for slot in 0..24 {
            let mask = 1 << (slot % 8);
            let byte = slot / 8;
            if (previous.buttons[byte] ^ native.state.buttons[byte]) & mask == 0 {
                continue;
            }
            let pressed = native.state.buttons[byte] & mask != 0;
            if self.native_blocked[byte] & mask != 0 {
                if !pressed {
                    self.native_blocked[byte] &= !mask;
                    self.held.remove(&(NATIVE_BASE + slot as u16));
                }
                continue;
            }
            if native.kinds[slot].is_some() {
                if pressed && native.state.held_buttons().len() > 1 {
                    if let Some(stroke) = &mut self.stroke {
                        stroke.cancelled = true;
                    }
                    self.native_blocked[byte] |= mask;
                    continue;
                }
                output.extend(self.event(Event::new(
                    input::KEY,
                    NATIVE_BASE + slot as u16,
                    i32::from(pressed),
                ))?);
            } else if pressed
                && let Some(stroke) = &mut self.stroke
                && native_slot(stroke.button).is_some()
            {
                stroke.cancelled = true;
            }
        }
        Ok(output)
    }

    fn motion(&mut self, output: &mut Vec<Decision>) {
        let delta = std::mem::take(&mut self.delta);
        let Some(stroke) = &mut self.stroke else {
            return;
        };
        if stroke.cancelled {
            return;
        }
        for (accumulated, value) in stroke.displacement.iter_mut().zip(delta) {
            *accumulated = accumulated.saturating_add(value);
        }
        if delta != [0; 2] && stroke.kind == 1 && self.settings.documents[1].tip {
            if stroke.trajectory.len() >= 2048 {
                stroke.trajectory = stroke.trajectory.iter().step_by(2).copied().collect();
            }
            stroke.trajectory.push(stroke.displacement);
        }
        if f64::from(stroke.displacement[0]).hypot(f64::from(stroke.displacement[1])) >= 10.0 {
            stroke.moved = true;
        }
        let doc = &self.settings.documents[stroke.kind];
        if doc.kind == Kind::Sixteen && doc.realtime {
            for (accumulated, value) in stroke.realtime.iter_mut().zip(delta) {
                *accumulated = accumulated.saturating_add(value);
            }
            let [x, y] = stroke.realtime;
            if f64::from(x).hypot(f64::from(y)) > doc.distance as f64 {
                stroke.realtime = [0; 2];
                let direction = direction8(x, y).to_string();
                if let Some(index) = Kind::Sixteen.index(&direction) {
                    stroke.moved = true;
                    output.push(Decision::Activate(stroke.kind, index, true));
                }
            }
        } else {
            stroke.trace16.feed(delta);
            stroke.mouse.feed(delta);
        }
    }

    fn event(&mut self, event: Event) -> Result<Vec<Decision>, String> {
        let mut output = Vec::new();
        match (event.kind, event.code) {
            (input::SYN, input::DROPPED) => {
                return Err("鼠标输入队列丢失事件。手势已停止，请重新启用。".into());
            }
            (input::REL, axis @ (input::X | input::Y)) if self.stroke.is_some() => {
                let value = &mut self.delta[axis as usize];
                *value = value.saturating_add(event.value);
            }
            (input::SYN, input::REPORT) => {
                self.motion(&mut output);
                output.push(Decision::Forward(event));
            }
            (input::KEY, code) => {
                self.motion(&mut output);
                if event.value == 1
                    && self.stroke.is_none()
                    && self.held.is_empty()
                    && let Some(kind) = self
                        .settings
                        .triggers
                        .iter()
                        .position(|trigger| *trigger == Some(code))
                        .or_else(|| native_slot(code).and_then(|slot| self.native_triggers[slot]))
                {
                    self.stroke = Some(Stroke {
                        kind,
                        button: code,
                        trace16: Trace16::default(),
                        mouse: TraceMouse::default(),
                        displacement: [0; 2],
                        realtime: [0; 2],
                        moved: false,
                        cancelled: false,
                        trajectory: vec![[0; 2]],
                    });
                } else if event.value == 0
                    && self
                        .stroke
                        .as_ref()
                        .is_some_and(|stroke| stroke.button == code)
                {
                    let stroke = self.stroke.take().unwrap();
                    output.push(Decision::Released(stroke.kind));
                    if !stroke.cancelled {
                        let doc = &self.settings.documents[stroke.kind];
                        let index = if doc.kind == Kind::Mouse {
                            stroke.mouse.index()
                        } else if !doc.realtime {
                            stroke.trace16.index()
                        } else {
                            None
                        };
                        if let Some(index) = index {
                            output.push(Decision::Activate(stroke.kind, index, false));
                        } else if !stroke.moved && native_slot(code).is_none() {
                            output.push(Decision::Click(code));
                        }
                    }
                } else if !self
                    .stroke
                    .as_ref()
                    .is_some_and(|stroke| stroke.button == code)
                {
                    if event.value == 1
                        && let Some(stroke) = &mut self.stroke
                    {
                        stroke.cancelled = true;
                    }
                    if native_slot(code).is_none() {
                        output.push(Decision::Forward(event));
                    }
                }
                if event.value == 0 {
                    self.held.remove(&code);
                } else {
                    self.held.insert(code);
                }
                if self.held.contains(&input::BUTTONS[1]) && self.held.contains(&input::BUTTONS[2])
                {
                    return Err("已同时按下右键和中键。全局手势已停止。".into());
                }
            }
            (input::REL, _) => output.push(Decision::Forward(event)),
            _ => {} // MSC_SCAN is metadata. The virtual device emits key codes.
        }
        Ok(output)
    }

    fn tip_visible(&self) -> bool {
        self.stroke.as_ref().is_some_and(|stroke| {
            stroke.kind == 1 && !stroke.cancelled && self.settings.documents[1].tip
        })
    }

    fn tip_frame(&self) -> Option<crate::gesture_tip::Frame> {
        if !self.tip_visible() {
            return None;
        }
        let stroke = self.stroke.as_ref()?;
        let document = &self.settings.documents[1];
        let (direction, action) = match stroke.mouse.index() {
            Some(index) => (
                Kind::Mouse.shapes()[index].name,
                document.actions[index].label(),
            ),
            None if !stroke.moved => ("等待方向", "按住触发键移动"),
            None => ("未匹配手势", "继续移动，松开结束"),
        };
        Some(crate::gesture_tip::Frame {
            points: stroke.trajectory.clone(),
            direction: direction.into(),
            action: action.chars().take(256).collect(),
        })
    }
}

/// Keep physical holds independent from macro holds on the same button.
#[derive(Default)]
struct Buttons {
    physical: BTreeSet<u16>,
    generated: BTreeSet<u16>,
}

impl Buttons {
    fn set(&mut self, code: u16, down: bool, generated: bool) -> Option<bool> {
        let before = self.physical.contains(&code) || self.generated.contains(&code);
        let owner = if generated {
            &mut self.generated
        } else {
            &mut self.physical
        };
        if down {
            owner.insert(code);
        } else {
            owner.remove(&code);
        }
        let after = self.physical.contains(&code) || self.generated.contains(&code);
        (before != after).then_some(after)
    }
}

pub(crate) struct Devices {
    pointer: Virtual,
    keyboard: Virtual,
    buttons: Buttons,
    axes: BTreeSet<u16>,
    opener: crate::host_open::Queue,
    text: Option<crate::host_text::Handle>,
    window: Option<crate::host_window::Handle>,
    physical: Option<physical_buttons::Reader>,
}

impl Devices {
    #[cfg(test)]
    pub(crate) fn fixture_sources(&self) -> Result<[(Source, String); 2], String> {
        Ok([
            (
                input::isolated_reader(&self.pointer)?,
                self.pointer.sys_name()?,
            ),
            (
                input::isolated_reader(&self.keyboard)?,
                self.keyboard.sys_name()?,
            ),
        ])
    }
    /// Host button actions generate their own output and never grab or mirror
    /// the physical pointer. The normal gesture runtime keeps its own source.
    pub(crate) fn for_host_actions(prepared: &[Prepared]) -> Result<Self, String> {
        let text = if prepared.iter().any(Prepared::needs_text) {
            Some(crate::host_text::Handle::start()?)
        } else {
            None
        };
        let window = if prepared.iter().any(Prepared::needs_window) {
            Some(crate::host_window::Handle::start()?)
        } else {
            None
        };
        let identity = input::Identity {
            bus: 6,
            vendor: 0x1d6b,
            product: 0xa091,
            version: 1,
        };
        let keys: BTreeSet<_> = (0..256)
            .filter_map(host_program::keyboard)
            .chain((0..1024).filter_map(host_program::consumer))
            .chain([116, 142, 143])
            .collect();
        let axes = [input::X, input::Y, input::WHEEL, input::HWHEEL, 11, 12];
        Ok(Self {
            pointer: Virtual::new("Bloody A90 host pointer", identity, &input::BUTTONS, &axes)?,
            keyboard: Virtual::new(
                "Bloody A90 host keyboard",
                identity,
                &keys.into_iter().collect::<Vec<_>>(),
                &[],
            )?,
            buttons: Buttons::default(),
            axes: axes.into_iter().collect(),
            opener: crate::host_open::Queue::default(),
            text,
            window,
            physical: None,
        })
    }

    pub(crate) fn poll(&mut self) -> Vec<String> {
        let mut messages = self.opener.poll();
        if let Some(text) = &self.text {
            messages.extend(text.poll());
        }
        if let Some(window) = &self.window {
            messages.extend(window.poll());
        }
        messages
    }

    pub(crate) fn cancel_pending(&self) {
        self.opener.cancel_pending();
        if let Some(text) = &self.text {
            text.cancel_pending();
        }
        if let Some(window) = &self.window {
            window.cancel_pending();
        }
    }

    fn forward(&mut self, event: Event) -> Result<(), String> {
        if event.kind == input::KEY {
            if let Some(down) = self.buttons.set(event.code, event.value != 0, false) {
                self.pointer
                    .event(input::KEY, event.code, i32::from(down))?;
            }
            Ok(())
        } else {
            self.pointer.event(event.kind, event.code, event.value)
        }
    }

    pub(crate) fn output(&mut self, output: Output) -> Result<(), String> {
        match output {
            Output::Open(plan) => self.opener.launch(plan),
            Output::Text(plan) => self.text.as_ref().ok_or("文字输入服务未启用")?.send(plan),
            Output::Window(operation) => self
                .window
                .as_ref()
                .ok_or("窗口动作服务未启用")?
                .execute(operation),
            Output::Key(code, down) => {
                self.keyboard.event(input::KEY, code, i32::from(down))?;
                self.keyboard.sync()
            }
            Output::Button(code, down) => {
                if let Some(down) = self.buttons.set(code, down, true) {
                    self.pointer.event(input::KEY, code, i32::from(down))?;
                }
                self.pointer.sync()
            }
            Output::Move(x, y) => {
                if x != 0 {
                    self.pointer.event(input::REL, input::X, x)?;
                }
                if y != 0 {
                    self.pointer.event(input::REL, input::Y, y)?;
                }
                self.pointer.sync()
            }
            Output::Wheel(x, y) => {
                if x != 0 {
                    self.pointer.event(input::REL, input::HWHEEL, x)?;
                    if self.axes.contains(&12) {
                        self.pointer.event(input::REL, 12, x.saturating_mul(120))?;
                    }
                }
                if y != 0 {
                    self.pointer.event(input::REL, input::WHEEL, y)?;
                    if self.axes.contains(&11) {
                        self.pointer.event(input::REL, 11, y.saturating_mul(120))?;
                    }
                }
                self.pointer.sync()
            }
        }
    }

    pub(crate) fn clear_generated(&mut self) -> Result<(), String> {
        self.keyboard.release_all()?;
        for code in self.buttons.generated.clone() {
            self.output(Output::Button(code, false))?;
        }
        Ok(())
    }
}

struct Active {
    id: (usize, usize),
    vm: Vm,
}

pub struct Handle {
    stop: mpsc::Sender<Control>,
    status: Receiver<Status>,
    thread: Option<JoinHandle<Result<(), String>>>,
    last: Status,
    remote: Option<Remote>,
}

enum Control {
    Stop,
    Interrupt,
}

impl Handle {
    pub fn start(settings: Settings) -> Result<Self, String> {
        Self::start_external(settings, [false; 2])
    }

    pub fn preflight(settings: &Settings) -> Result<(), String> {
        Self::preflight_external(settings, [false; 2])
    }

    pub(crate) fn preflight_external(
        settings: &Settings,
        external: [bool; 2],
    ) -> Result<(), String> {
        Self::validate(settings, external)?;
        let prepared = prepare_external_actions(settings, external)?;
        let _devices =
            Devices::for_host_actions(&prepared.into_iter().flatten().collect::<Vec<_>>())?;
        let info = input::find()?;
        let _source = Source::open(&info.path)?;
        Ok(())
    }

    pub(crate) fn remote(&self) -> Option<Remote> {
        self.remote.clone()
    }

    fn validate(settings: &Settings, external: [bool; 2]) -> Result<(), String> {
        if settings.documents[0].kind != Kind::Sixteen || settings.documents[1].kind != Kind::Mouse
        {
            return Err("两套手势的方向表无效".into());
        }
        if settings.triggers.iter().all(Option::is_none)
            && !external.into_iter().any(|enabled| enabled)
        {
            return Err("请至少启用一套手势".into());
        }
        for code in settings.triggers.iter().flatten() {
            if !input::BUTTONS.contains(code) {
                return Err("手势触发键必须是五个普通鼠标按键之一".into());
            }
        }
        if settings.triggers[0].is_some() && settings.triggers[0] == settings.triggers[1] {
            return Err("两套手势不能使用同一个触发键".into());
        }
        Ok(())
    }

    pub(crate) fn start_external(settings: Settings, external: [bool; 2]) -> Result<Self, String> {
        Self::validate(&settings, external)?;
        let prepared = prepare_external_actions(&settings, external)?;
        let physical = if prepared.iter().flatten().any(Prepared::needs_buttons) {
            Some(physical_buttons::Reader::start()?)
        } else {
            None
        };
        let text = if prepared.iter().flatten().any(Prepared::needs_text) {
            Some(crate::host_text::Handle::start()?)
        } else {
            None
        };
        let window = if prepared.iter().flatten().any(Prepared::needs_window) {
            Some(crate::host_window::Handle::start()?)
        } else {
            None
        };
        let info = input::find()?;
        let mut source = Source::open(&info.path)?;
        let mut axes = source.capabilities(input::REL, 15)?;
        for axis in [input::HWHEEL, input::WHEEL] {
            if !axes.contains(&axis) {
                axes.push(axis);
            }
        }
        let mut keys = source.capabilities(input::KEY, 767)?;
        for code in input::BUTTONS {
            if !keys.contains(&code) {
                keys.push(code);
            }
        }
        let pointer = Virtual::new(&info.name, info.identity, &keys, &axes)?;
        let keyboard_keys: BTreeSet<_> = (0..256)
            .filter_map(host_program::keyboard)
            .chain((0..1024).filter_map(host_program::consumer))
            .chain([116, 142, 143])
            .collect();
        let keyboard = Virtual::new(
            "Bloody A90 gesture keyboard",
            input::Identity {
                bus: 6,
                vendor: 0x1d6b,
                product: 0xa090,
                version: 1,
            },
            &keyboard_keys.into_iter().collect::<Vec<_>>(),
            &[],
        )?;
        // Give udev and the compositor time to open the mirrors before grabbing.
        let tip = if (settings.triggers[1].is_some() || external[1]) && settings.documents[1].tip {
            Some(crate::gesture_tip::Handle::start()?)
        } else {
            None
        };
        thread::sleep(Duration::from_millis(300));
        source.grab()?;
        let (stop, stopping) = mpsc::channel();
        let (updates, status) = mpsc::sync_channel(32);
        let (native_sender, native_receiver) = mpsc::sync_channel(64);
        let remote = external
            .into_iter()
            .any(|enabled| enabled)
            .then_some(Remote {
                sender: native_sender,
                allowed: external,
                control: stop.clone(),
            });
        let native = remote.as_ref().map(|_| native_receiver);
        let message = format!("全局手势已启用：{}", info.path.display());
        let thread = thread::spawn(move || {
            run(
                source,
                Devices {
                    pointer,
                    keyboard,
                    buttons: Buttons::default(),
                    axes: axes.into_iter().collect(),
                    opener: crate::host_open::Queue::default(),
                    text,
                    window,
                    physical,
                },
                settings,
                prepared,
                stopping,
                Extra {
                    status: updates,
                    tip,
                    native,
                },
            )
        });
        Ok(Self {
            stop,
            status,
            thread: Some(thread),
            last: Status {
                running: true,
                message,
            },
            remote,
        })
    }

    pub fn poll(&mut self) -> Status {
        if self.thread.is_none() {
            return self.last.clone();
        }
        while let Ok(status) = self.status.try_recv() {
            self.last = status;
        }
        if self
            .thread
            .as_ref()
            .is_some_and(|thread| thread.is_finished())
        {
            let result = self.thread.take().unwrap().join();
            self.last = Status {
                running: false,
                message: match result {
                    Ok(Ok(())) => "全局手势已停止".into(),
                    Ok(Err(e)) => e,
                    Err(_) => "全局手势线程异常退出，输入接管已释放".into(),
                },
            };
        }
        self.last.clone()
    }

    pub fn stop(&mut self) -> Result<(), String> {
        let _ = self.stop.send(Control::Stop);
        let result = if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| "全局手势线程异常退出".to_string())
                .and_then(|result| result)
        } else {
            Ok(())
        };
        self.last = Status {
            running: false,
            message: result
                .as_ref()
                .err()
                .cloned()
                .unwrap_or_else(|| "全局手势已停止".into()),
        };
        result
    }

    pub fn interrupt(&self) {
        let _ = self.stop.send(Control::Interrupt);
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

struct Extra {
    status: SyncSender<Status>,
    tip: Option<crate::gesture_tip::Handle>,
    native: Option<Receiver<Native>>,
}

fn run(
    mut source: Source,
    mut devices: Devices,
    settings: Settings,
    prepared: Vec<Vec<Prepared>>,
    stop: Receiver<Control>,
    extra: Extra,
) -> Result<(), String> {
    let Extra {
        status,
        tip,
        native,
    } = extra;
    let result = (|| {
        let mut engine = Engine::new(settings);
        let mut queue = VecDeque::new();
        let mut active: Option<Active> = None;
        let mut physical = [0; 3];
        let mut tip_visible = false;
        let mut last_tip = Instant::now() - Duration::from_secs(1);
        loop {
            match stop.try_recv() {
                Ok(Control::Stop) | Err(mpsc::TryRecvError::Disconnected) => break,
                Ok(Control::Interrupt) => {
                    active = None;
                    queue.clear();
                    devices.opener.cancel_pending();
                    if let Some(text) = &devices.text {
                        text.cancel_pending();
                    }
                    if let Some(window) = &devices.window {
                        window.cancel_pending();
                    }
                    devices.clear_generated()?;
                    let _ = status.try_send(Status {
                        running: true,
                        message: "主机手势宏已中断".into(),
                    });
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
            let now = Instant::now();
            let mut decisions = Vec::new();
            let mut released = None;
            if let Some(native) = &native {
                for _ in 0..64 {
                    match native.try_recv() {
                        Ok(snapshot) => {
                            // Do not let batch ambiguity discard the emergency
                            // chord while establishing a blocked baseline.
                            check_native_emergency(snapshot.state)?;
                            if let Some(stroke) = &engine.stroke
                                && let Some(slot) = native_slot(stroke.button)
                                && snapshot.state.buttons[slot / 8] & (1 << (slot % 8)) == 0
                                && engine
                                    .native_previous
                                    .is_some_and(|p| p.profile == snapshot.state.profile)
                            {
                                released = Some(snapshot);
                            } else if released.is_some() {
                                // Separate HID and evdev endpoints cannot order
                                // a second press inside one unread motion batch.
                                // Cancel the ambiguous stroke instead of guessing.
                                engine.stroke = None;
                                engine.delta = [0; 2];
                                engine.native_baseline(snapshot);
                                released = None;
                                decisions.push(Decision::Cancel);
                            } else {
                                let waiting = engine.stroke.is_none();
                                decisions.extend(engine.native(snapshot)?);
                                if waiting
                                    && let Some(stroke) = &engine.stroke
                                    && native_slot(stroke.button).is_some()
                                {
                                    let _ = status.try_send(Status {
                                        running: true,
                                        message: format!(
                                            "{}：等待方向",
                                            engine.settings.documents[stroke.kind].kind.name()
                                        ),
                                    });
                                }
                            }
                        }
                        Err(mpsc::TryRecvError::Empty) => break,
                        Err(mpsc::TryRecvError::Disconnected) => {
                            return Err("按键手势通知源已停止".into());
                        }
                    }
                }
            }
            for batch in 0..128 {
                let events = source.read()?;
                let finished = events.is_empty() || released.is_none();
                for event in events {
                    decisions.extend(engine.event(event)?);
                }
                if finished {
                    break;
                }
                if batch == 127 {
                    return Err("鼠标移动积压过多，按键手势已停止".into());
                }
            }
            if let Some(snapshot) = released {
                decisions.extend(engine.native(snapshot)?);
            }
            for decision in decisions {
                match decision {
                    Decision::Cancel => {
                        active = None;
                        queue.clear();
                        devices.cancel_pending();
                        devices.clear_generated()?;
                    }
                    Decision::Forward(event) => devices.forward(event)?,
                    Decision::Click(code) => {
                        // The click is generated only on release, so it cannot
                        // leave a physical trigger held during gesture capture.
                        devices.output(Output::Button(code, true))?;
                        devices.output(Output::Button(code, false))?;
                    }
                    Decision::Released(kind) => {
                        if let Some(active) = &mut active
                            && active.id.0 == kind
                        {
                            active.vm.released(now);
                        }
                        // An immediate action may still be queued when its
                        // trigger is released. It must not start a held loop.
                        for (queued_kind, _, held) in &mut queue {
                            if *queued_kind == kind {
                                *held = false;
                            }
                        }
                    }
                    Decision::Activate(kind, index, held) => {
                        if let Some(active) = &mut active
                            && active.id == (kind, index)
                            && active.vm.repeat() == Repeat::Toggle
                        {
                            active.vm.stop(now);
                        } else {
                            if let Some(active) = &mut active
                                && active.vm.repeat() != Repeat::Once
                            {
                                if active.id == (kind, index) && active.vm.repeat() == Repeat::Held
                                {
                                    continue;
                                }
                                active.vm.stop(now);
                            }
                            if queue.len() >= 128 {
                                return Err("手势动作队列已满。请增加即时响应距离。".into());
                            }
                            queue.push_back((kind, index, held));
                        }
                        let _ = status.try_send(Status {
                            running: true,
                            message: format!(
                                "{}：{}",
                                engine.settings.documents[kind].kind.shapes()[index].name,
                                engine.settings.documents[kind].actions[index].label()
                            ),
                        });
                    }
                }
            }
            if let Some(tip) = &tip
                && (tip_visible != engine.tip_visible()
                    || now.duration_since(last_tip) >= Duration::from_millis(33))
            {
                tip_visible = engine.tip_visible();
                tip.update(engine.tip_frame())?;
                last_tip = now;
            }
            if active.is_none()
                && let Some((kind, index, held)) = queue.pop_front()
            {
                active = Some(Active {
                    id: (kind, index),
                    vm: Vm::new(prepared[kind][index].clone(), now, held),
                });
            }
            if let Some(running) = &mut active
                && running.vm.ready(now)
            {
                let result = (|| {
                    if running.vm.needs_buttons() {
                        let state = devices
                            .physical
                            .as_mut()
                            .ok_or("缺少宏的按键状态读取服务")?
                            .sample()?;
                        let Some(state) = state else {
                            return Ok(Vec::new());
                        };
                        physical = state;
                    }
                    running.vm.tick(now, physical)
                })();
                match result {
                    Ok(output) => {
                        for event in output {
                            let desktop_action = matches!(
                                event,
                                Output::Open(_) | Output::Text(_) | Output::Window(_)
                            );
                            if let Err(message) = devices.output(event) {
                                if desktop_action {
                                    let _ = status.try_send(Status {
                                        running: true,
                                        message,
                                    });
                                } else {
                                    return Err(message);
                                }
                            }
                        }
                    }
                    Err(message) => {
                        let _ = status.try_send(Status {
                            running: true,
                            message: format!("手势宏已停止：{message}"),
                        });
                        devices.clear_generated()?;
                        active = None;
                    }
                }
            }
            if active.as_ref().is_some_and(|running| running.vm.finished()) {
                devices.clear_generated()?;
                active = None;
            }
            for message in devices.opener.poll() {
                let _ = status.try_send(Status {
                    running: true,
                    message,
                });
            }
            if let Some(text) = &devices.text {
                for message in text.poll() {
                    let _ = status.try_send(Status {
                        running: true,
                        message,
                    });
                }
            }
            if let Some(window) = &devices.window {
                for message in window.poll() {
                    let _ = status.try_send(Status {
                        running: true,
                        message,
                    });
                }
            }
            source.wait(if active.is_some() || !queue.is_empty() {
                1
            } else {
                10
            })?;
        }
        devices.clear_generated()?;
        // Both virtual devices and the source grab have Drop guards for errors.
        Ok(())
    })();
    // Release the physical input before waiting for the display thread.
    drop(source);
    drop(devices);
    drop(tip);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn settings(kind: usize) -> Settings {
        let mut triggers = [None; 2];
        triggers[kind] = Some(input::BUTTONS[3]);
        Settings {
            documents: [Document::new(Kind::Sixteen), Document::new(Kind::Mouse)],
            triggers,
        }
    }

    fn native_frame(profile: u8, buttons: [u8; 3], kinds: [Option<usize>; 24]) -> Native {
        Native {
            state: crate::vendor_input::State {
                profile,
                buttons,
                wheel: 0,
                sensitivity: 0,
            },
            kinds,
        }
    }

    #[test]
    fn native_top_keys_ignore_startup_holds_cancel_chords_and_follow_profiles() {
        let mut engine = Engine::new(settings(0));
        let mut kinds = [None; 24];
        kinds[8] = Some(0);
        kinds[9] = Some(1);
        kinds[10] = Some(0);
        assert!(
            engine
                .native(native_frame(0, [0, 1, 0], kinds))
                .unwrap()
                .is_empty()
        );
        assert!(
            engine
                .native(native_frame(0, [0; 3], kinds))
                .unwrap()
                .is_empty()
        );
        engine.native(native_frame(0, [0, 1, 0], kinds)).unwrap();
        assert!(engine.stroke.is_some());
        for _ in 0..5 {
            engine.event(Event::new(input::REL, input::Y, -21)).unwrap();
            engine
                .event(Event::new(input::SYN, input::REPORT, 0))
                .unwrap();
        }
        let result = engine.native(native_frame(0, [0; 3], kinds)).unwrap();
        assert!(
            result
                .iter()
                .any(|d| matches!(d, Decision::Activate(0, 0, false)))
        );
        assert!(
            !result
                .iter()
                .any(|d| matches!(d, Decision::Click(_) | Decision::Forward(_)))
        );
        engine.native(native_frame(0, [0, 4, 0], kinds)).unwrap();
        let changed = engine.native(native_frame(1, [0, 4, 0], kinds)).unwrap();
        assert!(matches!(changed.as_slice(), [Decision::Cancel]));
        assert!(engine.stroke.is_none());
        assert!(
            engine
                .native(native_frame(1, [0; 3], kinds))
                .unwrap()
                .is_empty()
        );
        engine.native(native_frame(1, [0, 1, 0], kinds)).unwrap();
        engine.native(native_frame(1, [0, 5, 0], kinds)).unwrap();
        assert!(engine.stroke.as_ref().unwrap().cancelled);
        assert!(engine.native(native_frame(1, [6, 0, 0], kinds)).is_err());
    }

    #[test]
    #[ignore = "requires /dev/uinput; native reports and ordinary triggers use one isolated input source"]
    fn isolated_native_top_keys_and_logical_trigger_share_one_source() {
        use crate::gestures::Action;
        let identity = input::Identity {
            bus: 6,
            vendor: 0x1d6b,
            product: 0xa090,
            version: 1,
        };
        let mut injected = Virtual::new(
            "A90 native gesture fixture",
            identity,
            &input::BUTTONS,
            &[input::X, input::Y],
        )
        .unwrap();
        let mut source = input::isolated_reader(&injected).unwrap();
        source.grab().unwrap();
        let mut config = settings(0);
        config.documents[1].tip = false;
        config.documents[0].actions[0] = Action::keyboard(27, 1, "剪切");
        config.documents[1].actions[0] = Action::keyboard(4, 0, "A");
        let prepared = prepare_external_actions(&config, [true; 2]).unwrap();
        let devices =
            Devices::for_host_actions(&prepared.iter().flatten().cloned().collect::<Vec<_>>())
                .unwrap();
        let [(mut pointer, pointer_sys), (mut keyboard, keyboard_sys)] =
            devices.fixture_sources().unwrap();
        pointer.grab().unwrap();
        keyboard.grab().unwrap();
        let (stop, stopping) = mpsc::channel();
        let (status, updates) = mpsc::sync_channel(32);
        let (native, received) = mpsc::sync_channel(64);
        let runtime = thread::spawn(move || {
            run(
                source,
                devices,
                config,
                prepared,
                stopping,
                Extra {
                    status,
                    tip: None,
                    native: Some(received),
                },
            )
        });
        struct Cleanup {
            stop: mpsc::Sender<Control>,
            thread: Option<JoinHandle<Result<(), String>>>,
        }
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = self.stop.send(Control::Stop);
                if let Some(thread) = self.thread.take() {
                    let _ = thread.join();
                }
            }
        }
        let mut cleanup = Cleanup {
            stop,
            thread: Some(runtime),
        };
        let collect = |source: &mut Source, code: u16| {
            let until = Instant::now() + Duration::from_secs(2);
            let mut events = Vec::new();
            loop {
                events.extend(source.read().unwrap());
                if events
                    .iter()
                    .any(|e| e.kind == input::KEY && e.code == code && e.value == 0)
                {
                    return events;
                }
                assert!(Instant::now() < until, "gesture key release did not arrive");
                thread::sleep(Duration::from_millis(1));
            }
        };
        let wait_stroke = || {
            loop {
                let status = updates.recv_timeout(Duration::from_secs(2)).unwrap();
                if status.message.ends_with("等待方向") {
                    break;
                }
            }
        };
        let draw = |injected: &mut Virtual| {
            for _ in 0..5 {
                injected.event(input::REL, input::Y, -21).unwrap();
                injected.sync().unwrap();
            }
        };
        let mut kinds = [None; 24];
        kinds[8] = Some(0);
        kinds[9] = Some(1);
        kinds[10] = Some(0);
        native.send(native_frame(0, [0; 3], kinds)).unwrap();
        native.send(native_frame(0, [0, 1, 0], kinds)).unwrap();
        wait_stroke();
        draw(&mut injected);
        // Queue release before the evdev batch is drained. Final motion must
        // belong to this stroke instead of leaking into normal pointer input.
        native.send(native_frame(0, [0; 3], kinds)).unwrap();
        let events = collect(&mut keyboard, 29);
        assert_eq!(
            events
                .iter()
                .filter(|e| e.kind == input::KEY)
                .map(|e| (e.code, e.value))
                .collect::<Vec<_>>(),
            [(29, 1), (45, 1), (45, 0), (29, 0)]
        );
        assert!(
            pointer
                .read()
                .unwrap()
                .iter()
                .all(|e| e.kind != input::REL && e.kind != input::KEY)
        );
        native.send(native_frame(0, [0, 2, 0], kinds)).unwrap();
        wait_stroke();
        draw(&mut injected);
        native.send(native_frame(0, [0; 3], kinds)).unwrap();
        let events = collect(&mut keyboard, 30);
        assert_eq!(
            events
                .iter()
                .filter(|e| e.kind == input::KEY)
                .map(|e| (e.code, e.value))
                .collect::<Vec<_>>(),
            [(30, 1), (30, 0)]
        );
        native.send(native_frame(0, [0, 4, 0], kinds)).unwrap();
        wait_stroke();
        let mut next = kinds;
        next[10] = Some(1);
        native.send(native_frame(1, [0, 4, 0], next)).unwrap();
        native.send(native_frame(1, [0; 3], next)).unwrap();
        // A complete ordinary trigger stroke remains valid with native input.
        // The profile reset must be processed first; its status provides the
        // following native stroke barrier without a timing sleep.
        native.send(native_frame(1, [0, 4, 0], next)).unwrap();
        wait_stroke();
        draw(&mut injected);
        native.send(native_frame(1, [0; 3], next)).unwrap();
        collect(&mut keyboard, 30);
        injected.event(input::KEY, input::BUTTONS[3], 1).unwrap();
        injected.sync().unwrap();
        draw(&mut injected);
        injected.event(input::KEY, input::BUTTONS[3], 0).unwrap();
        injected.sync().unwrap();
        collect(&mut keyboard, 29);
        native.send(native_frame(1, [0, 1, 0], next)).unwrap();
        wait_stroke();
        native.send(native_frame(1, [0; 3], next)).unwrap();
        native.send(native_frame(1, [6, 1, 0], next)).unwrap();
        let result = cleanup.thread.take().unwrap().join().unwrap();
        assert!(result.unwrap_err().contains("右键和中键"));
        for name in [pointer_sys, keyboard_sys] {
            assert!(
                !std::path::Path::new("/sys/devices/virtual/input")
                    .join(name)
                    .exists()
            );
        }
        let mut reclaimed = input::isolated_reader(&injected).unwrap();
        reclaimed.grab().unwrap();
    }

    #[test]
    fn preflight_checks_only_directions_that_can_execute() {
        let mut configuration = settings(0);
        configuration.documents[0].realtime = true;
        configuration.documents[0].actions[8] = crate::gestures::Action::new(24, 1, 3, "窗口");
        configuration.documents[0].actions[9] =
            crate::gestures::Action::input_text("文字").unwrap();
        configuration.documents[0].actions[10] = crate::gestures::Action::new(999, 0, 3, "未知");
        configuration.documents[1].actions[0] = crate::gestures::Action::new(999, 0, 3, "未知");
        let prepared = prepare_actions(&configuration).unwrap();
        assert!(!prepared.iter().flatten().any(Prepared::needs_window));
        assert!(!prepared.iter().flatten().any(Prepared::needs_text));
        // The imported values remain available when release recognition returns.
        assert_eq!(
            configuration.documents[0].actions[10].number("Simulator"),
            Ok(999)
        );
        configuration.documents[0].realtime = false;
        assert!(
            prepare_actions(&configuration)
                .unwrap_err()
                .contains("下 → 左")
        );
        configuration.documents[0].actions[10] = crate::gestures::Action::default();
        let prepared = prepare_actions(&configuration).unwrap();
        assert!(prepared[0][8].needs_window());
        assert!(prepared[0][9].needs_text());
        configuration.triggers[1] = Some(input::BUTTONS[4]);
        assert!(
            prepare_actions(&configuration)
                .unwrap_err()
                .contains("鼠标手势，上")
        );
    }

    #[test]
    fn mouse_diagonal_frames_activate_the_native_direction() {
        for (x, y, expected) in [
            (20, -20, 3),
            (20, 20, 1),
            (1999, -2000, 3),
            (1998, -2000, 0),
        ] {
            let mut engine = Engine::new(settings(1));
            let trigger = input::BUTTONS[3];
            engine.event(Event::new(input::KEY, trigger, 1)).unwrap();
            for _ in 0..5 {
                engine.event(Event::new(input::REL, input::X, x)).unwrap();
                engine.event(Event::new(input::REL, input::Y, y)).unwrap();
                engine
                    .event(Event::new(input::SYN, input::REPORT, 0))
                    .unwrap();
            }
            let result = engine.event(Event::new(input::KEY, trigger, 0)).unwrap();
            assert!(result.iter().any(|decision| matches!(decision, Decision::Activate(1, index, false) if *index == expected)), "{x},{y}: {result:?}");
        }
    }

    #[test]
    fn input_frames_capture_gestures_and_keep_ordinary_clicks() {
        for kind in 0..2 {
            let mut engine = Engine::new(settings(kind));
            let trigger = input::BUTTONS[3];
            assert!(
                engine
                    .event(Event::new(input::KEY, trigger, 1))
                    .unwrap()
                    .is_empty()
            );
            let click = engine.event(Event::new(input::KEY, trigger, 0)).unwrap();
            assert!(
                matches!(click.as_slice(), [Decision::Released(_), Decision::Click(code)] if *code == trigger)
            );
            engine.event(Event::new(input::KEY, trigger, 1)).unwrap();
            for _ in 0..5 {
                assert!(
                    engine
                        .event(Event::new(input::REL, input::Y, -21))
                        .unwrap()
                        .is_empty()
                );
                engine
                    .event(Event::new(input::SYN, input::REPORT, 0))
                    .unwrap();
            }
            let action = engine.event(Event::new(input::KEY, trigger, 0)).unwrap();
            assert!(
                matches!(action.as_slice(), [Decision::Released(_), Decision::Activate(k, 0, false)] if *k == kind)
            );
            let forwarded = engine.event(Event::new(input::REL, input::X, 25)).unwrap();
            assert!(matches!(
                forwarded.as_slice(),
                [Decision::Forward(Event { value: 25, .. })]
            ));
            assert!(
                engine
                    .event(Event::new(input::SYN, input::DROPPED, 0))
                    .is_err()
            );
        }
    }

    #[test]
    fn realtime_threshold_chords_and_button_ownership() {
        let mut config = settings(0);
        config.documents[0].realtime = true;
        config.documents[0].distance = 20;
        let mut engine = Engine::new(config);
        engine
            .event(Event::new(input::KEY, input::BUTTONS[3], 1))
            .unwrap();
        engine.event(Event::new(input::REL, input::X, 20)).unwrap();
        assert_eq!(
            engine
                .event(Event::new(input::SYN, input::REPORT, 0))
                .unwrap()
                .len(),
            1
        );
        engine.event(Event::new(input::REL, input::X, 1)).unwrap();
        assert!(matches!(
            engine
                .event(Event::new(input::SYN, input::REPORT, 0))
                .unwrap()
                .as_slice(),
            [Decision::Activate(0, 3, true), Decision::Forward(_)]
        ));
        engine
            .event(Event::new(input::KEY, input::BUTTONS[0], 1))
            .unwrap();
        assert!(matches!(
            engine
                .event(Event::new(input::KEY, input::BUTTONS[3], 0))
                .unwrap()
                .as_slice(),
            [Decision::Released(0)]
        ));
        let mut buttons = Buttons::default();
        assert_eq!(buttons.set(0x110, true, false), Some(true));
        assert_eq!(buttons.set(0x110, true, true), None);
        assert_eq!(buttons.set(0x110, false, true), None);
        assert_eq!(buttons.set(0x110, false, false), Some(false));
    }

    #[test]
    fn tip_tracks_mouse_stroke_and_respects_visibility_setting() {
        let mut config = settings(1);
        config.documents[1].actions[0] = crate::gestures::Action::keyboard(27, 1, "剪切");
        let mut engine = Engine::new(config);
        let trigger = input::BUTTONS[3];
        assert!(engine.tip_frame().is_none());
        engine.event(Event::new(input::KEY, trigger, 1)).unwrap();
        assert_eq!(engine.tip_frame().unwrap().points, [[0, 0]]);
        for _ in 0..5 {
            engine.event(Event::new(input::REL, input::Y, -21)).unwrap();
            engine
                .event(Event::new(input::SYN, input::REPORT, 0))
                .unwrap();
        }
        let frame = engine.tip_frame().unwrap();
        assert_eq!(frame.direction, "上");
        assert_eq!(frame.action, "剪切");
        assert_eq!(frame.points.last(), Some(&[0, -105]));
        engine.settings.documents[1].tip = false;
        assert!(engine.tip_frame().is_none());
        engine.settings.documents[1].tip = true;
        engine
            .event(Event::new(input::KEY, input::BUTTONS[0], 1))
            .unwrap();
        assert!(engine.tip_frame().is_none()); // a chord cancels the stroke
        engine.event(Event::new(input::KEY, trigger, 0)).unwrap();
        assert!(engine.tip_frame().is_none());
    }

    #[test]
    #[ignore = "requires the udev rule; creates and grabs only isolated virtual devices"]
    fn isolated_evdev_to_gesture_to_uinput() {
        use crate::{gestures::Action, linux_input::Identity};
        use std::os::unix::fs::PermissionsExt;
        struct Files(std::path::PathBuf);
        impl Drop for Files {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let files =
            Files(std::env::temp_dir().join(format!("a90-gesture-launch-{}", std::process::id())));
        std::fs::create_dir_all(&files.0).unwrap();
        let executable = files.0.join("program with spaces $(false).sh");
        let conditional = files.0.join("conditional.amc");
        std::fs::write(
            &conditional,
            crate::amc::Macro {
                description: "physical button branch".into(),
                high_language: false,
                repeat: Repeat::Once,
                press: "ifkey 11 0 3\nKeyDownUp 27".into(),
                release: String::new(),
            }
            .to_bytes(),
        )
        .unwrap();
        let marker = files.0.join("completed");
        std::fs::write(&executable, "#!/bin/sh\nprintf '%s' \"$1\" > \"$2\"\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let identity = Identity {
            bus: 6,
            vendor: 0x1d6b,
            product: 0xa090,
            version: 1,
        };
        let keys = input::BUTTONS;
        let axes = [input::X, input::Y, input::WHEEL, 11];
        let mut injected =
            Virtual::new("A90 isolated gesture source", identity, &keys, &axes).unwrap();
        let mut source = input::isolated_reader(&injected).unwrap();
        source.grab().unwrap();
        let pointer = Virtual::new("A90 isolated gesture pointer", identity, &keys, &axes).unwrap();
        let mut pointer_reader = input::isolated_reader(&pointer).unwrap();
        pointer_reader.grab().unwrap();
        let keyboard = Virtual::new(
            "A90 isolated gesture keyboard",
            identity,
            &[29, 45, 387, 144],
            &[],
        )
        .unwrap();
        let mut keyboard_reader = input::isolated_reader(&keyboard).unwrap();
        keyboard_reader.grab().unwrap();
        let pointer_sys = pointer.sys_name().unwrap();
        let keyboard_sys = keyboard.sys_name().unwrap();
        let mut config = settings(0);
        config.documents[0].actions[0] = Action::keyboard(27, 1, "剪切");
        config.documents[0].actions[1] = Action::new(9, 9, 3, "滚轮向上");
        config.documents[0].actions[2] = Action::new(22, 0, 3, "放大");
        config.documents[0].actions[3] = Action::open_target_with_arguments(
            executable.to_str().unwrap(),
            &format!("done\n{}", marker.display()),
        )
        .unwrap();
        config.documents[0].actions[4] = Action::macro_file(conditional.to_str().unwrap()).unwrap();
        config.documents[0].actions[5] = Action::new(7, 0x193, 3, "媒体播放器");
        config.documents[0].actions[6] = Action::new(7, 0x194, 3, "文件管理器");
        let (query_started, waiting_query) = mpsc::channel();
        let (release_query, query_gate) = mpsc::channel();
        let block_next_query = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let block_query = block_next_query.clone();
        let physical = physical_buttons::Reader::with_query(move |_| {
            if block_query.swap(false, std::sync::atomic::Ordering::AcqRel) {
                query_started.send(()).unwrap();
                query_gate
                    .recv_timeout(Duration::from_secs(2))
                    .map_err(|e| e.to_string())?;
            }
            Ok([0, 4, 0])
        });
        let prepared = config
            .documents
            .iter()
            .map(|doc| {
                doc.actions
                    .iter()
                    .map(host_program::prepare)
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap()
            })
            .collect();
        let (stop, stopping) = mpsc::channel();
        let (status, updates) = mpsc::sync_channel(32);
        let runtime = thread::spawn(move || {
            run(
                source,
                Devices {
                    pointer,
                    keyboard,
                    buttons: Buttons::default(),
                    axes: axes.into_iter().collect(),
                    opener: crate::host_open::Queue::default(),
                    text: None,
                    window: None,
                    physical: Some(physical),
                },
                config,
                prepared,
                stopping,
                Extra {
                    status,
                    tip: None,
                    native: None,
                },
            )
        });
        struct Stop {
            stop: mpsc::Sender<Control>,
            thread: Option<JoinHandle<Result<(), String>>>,
        }
        impl Drop for Stop {
            fn drop(&mut self) {
                let _ = self.stop.send(Control::Stop);
                if let Some(thread) = self.thread.take() {
                    let _ = thread.join();
                }
            }
        }
        let mut cleanup = Stop {
            stop,
            thread: Some(runtime),
        };
        let collect = |reader: &mut Source, kind: u16, code: u16, value: i32| {
            let until = Instant::now() + Duration::from_secs(2);
            let mut events = Vec::new();
            loop {
                reader.wait(25).unwrap();
                events.extend(reader.read().unwrap());
                if events
                    .iter()
                    .any(|event| event.kind == kind && event.code == code && event.value == value)
                {
                    break events;
                }
                assert!(
                    Instant::now() < until,
                    "input event timeout: {kind}, {code}, {value}, {events:?}"
                );
            }
        };
        // Ordinary movement passes through exactly once.
        injected.event(input::REL, input::X, 7).unwrap();
        injected.sync().unwrap();
        let events = collect(&mut pointer_reader, input::REL, input::X, 7);
        assert_eq!(
            events
                .iter()
                .filter(|e| e.kind == input::REL && e.code == input::X)
                .map(|e| e.value)
                .sum::<i32>(),
            7
        );
        // A stationary trigger remains an ordinary click.
        injected.event(input::KEY, input::BUTTONS[3], 1).unwrap();
        injected.sync().unwrap();
        injected.event(input::KEY, input::BUTTONS[3], 0).unwrap();
        injected.sync().unwrap();
        let events = collect(&mut pointer_reader, input::KEY, input::BUTTONS[3], 0);
        assert!(
            events
                .iter()
                .any(|e| e.kind == input::KEY && e.code == input::BUTTONS[3] && e.value == 1)
        );
        // A complete up stroke produces Ctrl+X and suppresses its motion.
        let draw = |injected: &mut Virtual, y| {
            injected.event(input::KEY, input::BUTTONS[3], 1).unwrap();
            injected.sync().unwrap();
            for _ in 0..5 {
                injected.event(input::REL, input::Y, y).unwrap();
                injected.sync().unwrap();
            }
            injected.event(input::KEY, input::BUTTONS[3], 0).unwrap();
            injected.sync().unwrap();
        };
        draw(&mut injected, -21);
        let events = collect(&mut keyboard_reader, input::KEY, 29, 0);
        let keys: Vec<_> = events
            .iter()
            .filter(|event| event.kind == input::KEY)
            .map(|event| (event.code, event.value))
            .collect();
        assert_eq!(keys, [(29, 1), (45, 1), (45, 0), (29, 0)]);
        assert!(keyboard_reader.held().unwrap().is_empty());
        assert!(
            !pointer_reader
                .read()
                .unwrap()
                .iter()
                .any(|event| event.kind == input::REL && event.code == input::Y)
        );
        // Consumer launch keys use the kernel's Linux codes, including the
        // extended KEY_PLAYER value above the ordinary keyboard range.
        for (x, y, code) in [(-21, 21, 387), (-21, -21, 144)] {
            injected.event(input::KEY, input::BUTTONS[3], 1).unwrap();
            injected.sync().unwrap();
            for _ in 0..5 {
                injected.event(input::REL, input::X, x).unwrap();
                injected.event(input::REL, input::Y, y).unwrap();
                injected.sync().unwrap();
            }
            injected.event(input::KEY, input::BUTTONS[3], 0).unwrap();
            injected.sync().unwrap();
            let events = collect(&mut keyboard_reader, input::KEY, code, 0);
            assert_eq!(
                events
                    .iter()
                    .filter(|event| event.kind == input::KEY)
                    .map(|event| (event.code, event.value))
                    .collect::<Vec<_>>(),
                [(code, 1), (code, 0)]
            );
            assert!(keyboard_reader.held().unwrap().is_empty());
        }
        // Generated scroll includes the high-resolution companion event.
        draw(&mut injected, 21);
        let events = collect(&mut pointer_reader, input::REL, 11, 120);
        assert!(events.iter().any(|event| event.kind == input::REL
            && event.code == input::WHEEL
            && event.value == 1));
        // A left stroke emits Ctrl + one wheel notch, then releases Ctrl.
        injected.event(input::KEY, input::BUTTONS[3], 1).unwrap();
        injected.sync().unwrap();
        for _ in 0..5 {
            injected.event(input::REL, input::X, -21).unwrap();
            injected.sync().unwrap();
        }
        injected.event(input::KEY, input::BUTTONS[3], 0).unwrap();
        injected.sync().unwrap();
        let key_events = collect(&mut keyboard_reader, input::KEY, 29, 0);
        let wheel_events = collect(&mut pointer_reader, input::REL, 11, 120);
        let key_events: Vec<_> = key_events
            .iter()
            .filter(|event| event.kind == input::KEY)
            .collect();
        assert_eq!(
            key_events
                .iter()
                .map(|event| (event.code, event.value))
                .collect::<Vec<_>>(),
            [(29, 1), (29, 0)]
        );
        let wheel = wheel_events
            .iter()
            .find(|event| event.kind == input::REL && event.code == input::WHEEL)
            .unwrap();
        assert_eq!(wheel.value, 1);
        assert!(key_events[0].timestamp_micros() < wheel.timestamp_micros());
        assert!(wheel.timestamp_micros() <= key_events[1].timestamp_micros());
        assert!(keyboard_reader.held().unwrap().is_empty());
        // A diagonal stroke runs ifkey. Keep the HID reply pending and prove
        // that ordinary pointer motion still reaches the mirrored device.
        injected.event(input::KEY, input::BUTTONS[3], 1).unwrap();
        injected.sync().unwrap();
        for _ in 0..5 {
            injected.event(input::REL, input::X, 21).unwrap();
            injected.event(input::REL, input::Y, -21).unwrap();
            injected.sync().unwrap();
        }
        injected.event(input::KEY, input::BUTTONS[3], 0).unwrap();
        injected.sync().unwrap();
        waiting_query.recv_timeout(Duration::from_secs(1)).unwrap();
        injected.event(input::REL, input::X, 9).unwrap();
        injected.sync().unwrap();
        collect(&mut pointer_reader, input::REL, input::X, 9);
        assert!(
            keyboard_reader
                .read()
                .unwrap()
                .iter()
                .all(|event| event.kind != input::KEY)
        );
        release_query.send(()).unwrap();
        let events = collect(&mut keyboard_reader, input::KEY, 45, 0);
        assert_eq!(
            events
                .iter()
                .filter(|e| e.kind == input::KEY)
                .map(|e| (e.code, e.value))
                .collect::<Vec<_>>(),
            [(45, 1), (45, 0)]
        );
        assert!(keyboard_reader.held().unwrap().is_empty());
        // A right stroke launches the literal target on the background thread.
        // The temporary program receives two literal arguments and only writes
        // a marker in its own test directory.
        injected.event(input::KEY, input::BUTTONS[3], 1).unwrap();
        injected.sync().unwrap();
        for _ in 0..5 {
            injected.event(input::REL, input::X, 21).unwrap();
            injected.sync().unwrap();
        }
        injected.event(input::KEY, input::BUTTONS[3], 0).unwrap();
        injected.sync().unwrap();
        let until = Instant::now() + Duration::from_secs(2);
        while std::fs::read(&marker).ok().as_deref() != Some(b"done") {
            assert!(Instant::now() < until, "gesture did not launch its target");
            thread::sleep(Duration::from_millis(5));
        }
        // A target can disappear after settings were prepared. Report that
        // launch error while continuing to forward ordinary pointer motion.
        std::fs::remove_file(&executable).unwrap();
        injected.event(input::KEY, input::BUTTONS[3], 1).unwrap();
        injected.sync().unwrap();
        for _ in 0..5 {
            injected.event(input::REL, input::X, 21).unwrap();
            injected.sync().unwrap();
        }
        injected.event(input::KEY, input::BUTTONS[3], 0).unwrap();
        injected.sync().unwrap();
        let until = Instant::now() + Duration::from_secs(2);
        loop {
            if updates
                .try_iter()
                .any(|update| update.message.starts_with("无法打开目标"))
            {
                break;
            }
            assert!(Instant::now() < until, "launch error was not reported");
            thread::sleep(Duration::from_millis(5));
        }
        injected.event(input::REL, input::X, 9).unwrap();
        injected.sync().unwrap();
        collect(&mut pointer_reader, input::REL, input::X, 9);
        // Stop while a second HID reply is pending. The runtime must release
        // the physical source before it waits for the HID worker to finish.
        block_next_query.store(true, std::sync::atomic::Ordering::Release);
        injected.event(input::KEY, input::BUTTONS[3], 1).unwrap();
        injected.sync().unwrap();
        for _ in 0..5 {
            injected.event(input::REL, input::X, 21).unwrap();
            injected.event(input::REL, input::Y, -21).unwrap();
            injected.sync().unwrap();
        }
        injected.event(input::KEY, input::BUTTONS[3], 0).unwrap();
        injected.sync().unwrap();
        waiting_query.recv_timeout(Duration::from_secs(1)).unwrap();
        cleanup.stop.send(Control::Stop).unwrap();
        let mut reclaimed = input::isolated_reader(&injected).unwrap();
        let until = Instant::now() + Duration::from_millis(500);
        while reclaimed.grab().is_err() {
            assert!(
                Instant::now() < until,
                "HID reply delayed the source release"
            );
            thread::sleep(Duration::from_millis(2));
        }
        release_query.send(()).unwrap();
        cleanup.thread.take().unwrap().join().unwrap().unwrap();
        assert!(
            !std::path::Path::new("/sys/devices/virtual/input")
                .join(pointer_sys)
                .exists()
        );
        assert!(
            !std::path::Path::new("/sys/devices/virtual/input")
                .join(keyboard_sys)
                .exists()
        );
    }

    #[test]
    #[ignore = "requires an A90; forwards input for one second and does not generate gesture actions"]
    fn physical_start_stop_releases_grab() {
        let info = input::find().unwrap();
        for kind in 0..2 {
            let mut handle = Handle::start(settings(kind)).unwrap();
            assert!(handle.poll().running);
            thread::sleep(Duration::from_secs(1));
            assert!(handle.poll().running);
            handle.interrupt();
            handle.stop().unwrap();
            assert!(!handle.poll().running);
            let mut reclaimed = Source::open(&info.path).unwrap();
            reclaimed.grab().unwrap();
        }
    }
}
