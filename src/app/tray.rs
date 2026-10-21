//! Tray icon and its menu, like the official TTrayForm (MAIN_SystemTray).
//! EXE 0083b764 creates 17 items from a string table; 0083ce9c shows them by
//! device flags. This keeps the ones that apply to a wired A90 on Linux, in
//! the official order: mi_LaunchOnLogin, mi_MouseLed, mi_W4DSetting,
//! mi_TrayLaunch and mi_ProgramExit. The icon is drawn here: red with a
//! mouse available, grey without, as the official Enable.ico / Disable.ico.
use super::{App, Main, Page, other};
use crate::worker::Wake;
use bloody_a90::{i18n::tr, status_window::Display, tf};
use eframe::egui;
use ksni::{
    Icon, MenuItem, ToolTip, Tray,
    blocking::{Handle, TrayMethods},
    menu::{CheckmarkItem, RadioGroup, RadioItem, StandardItem, SubMenu},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, RecvTimeoutError, Sender, channel},
};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};

pub(super) enum Command {
    Show,
    Wheel4D,
    Autostart(bool),
    Status(Option<Display>),
    Quit,
}

/// What the menu shows. The App pushes a new copy when one changes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct View {
    pub connected: bool,
    pub autostart: bool,
    pub display: Option<Display>,
    pub language: bloody_a90::i18n::Language,
}

const DISPLAYS: [Option<Display>; 3] = [Some(Display::Always), Some(Display::Changed), None];

struct TrayState {
    view: View,
    commands: Sender<Command>,
    wake: Wake,
    /// A StatusNotifierWatcher shows the icon. Without one the window
    /// must not hide, or nothing could bring it back.
    online: Arc<AtomicBool>,
}

impl TrayState {
    fn send(&self, command: Command) {
        let _ = self.commands.send(command);
        self.wake.request_repaint();
    }
}

impl Tray for TrayState {
    fn id(&self) -> String {
        "bloody-a90".into()
    }

    fn title(&self) -> String {
        "Bloody A90".into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        [22, 32, 64]
            .into_iter()
            .map(|size| draw(size, self.view.connected))
            .collect()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: "Bloody A90".into(),
            description: if self.view.connected {
                tr("已连接鼠标")
            } else {
                tr("请插入血手系列鼠标!")
            }
            .into(),
            ..Default::default()
        }
    }

    /// A left click opens the main window, as the official double click.
    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(Command::Show);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let selected = DISPLAYS
            .iter()
            .position(|d| *d == self.view.display)
            .unwrap_or(2);
        vec![
            CheckmarkItem {
                label: tr("开机自动执行").into(),
                checked: self.view.autostart,
                activate: Box::new(|tray: &mut Self| {
                    tray.view.autostart = !tray.view.autostart;
                    tray.send(Command::Autostart(tray.view.autostart));
                }),
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: tr("开启功能显示").into(),
                submenu: vec![
                    RadioGroup {
                        selected,
                        select: Box::new(|tray: &mut Self, index| {
                            tray.view.display = DISPLAYS[index];
                            tray.send(Command::Status(DISPLAYS[index]));
                        }),
                        options: ["永远显示", "改变时显示", "不显示"]
                            .into_iter()
                            .map(|label| RadioItem {
                                label: tr(label).into(),
                                ..Default::default()
                            })
                            .collect(),
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: tr("进化轮设定").into(),
                activate: Box::new(|tray: &mut Self| tray.send(Command::Wheel4D)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: tf!("开启 {}", "Bloody A90"),
                icon_name: "window-new".into(),
                activate: Box::new(|tray: &mut Self| tray.send(Command::Show)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: tr("离开").into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|tray: &mut Self| tray.send(Command::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }

    fn watcher_online(&self) {
        self.online.store(true, Ordering::Release);
    }

    /// Keep the service. The icon returns when a watcher starts again.
    fn watcher_offline(&self, _reason: ksni::OfflineReason) -> bool {
        self.online.store(false, Ordering::Release);
        true
    }
}

pub(super) struct TrayIcon {
    handle: Handle<TrayState>,
    commands: Receiver<Command>,
    online: Arc<AtomicBool>,
    view: View,
}

impl TrayIcon {
    pub(super) fn start(view: View, wake: Wake) -> Result<Self, String> {
        let (sender, commands) = channel();
        // spawn fails without a watcher; watcher_online only reports a return.
        let online = Arc::new(AtomicBool::new(true));
        let handle = TrayState {
            view,
            commands: sender,
            wake,
            online: online.clone(),
        }
        .spawn()
        .map_err(|e| tf!("无法创建托盘图标：{e}"))?;
        Ok(Self {
            handle,
            commands,
            online,
            view,
        })
    }

    pub(super) fn online(&self) -> bool {
        self.online.load(Ordering::Acquire) && !self.handle.is_closed()
    }

    pub(super) fn sync(&mut self, view: View) {
        if view != self.view {
            self.view = view;
            self.handle.update(|tray| tray.view = view);
        }
    }

    pub(super) fn commands(&self) -> Vec<Command> {
        self.commands.try_iter().collect()
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        self.handle.shutdown().wait();
    }
}

impl App {
    pub(super) fn tray_view(&self) -> View {
        View {
            connected: self.connected(),
            autostart: other::autostart_file().is_some_and(|path| path.exists()),
            display: self.status_display,
            language: bloody_a90::i18n::language(),
        }
    }

    pub(super) fn start_tray(&mut self) {
        match TrayIcon::start(self.tray_view(), self.wake.clone()) {
            Ok(tray) => self.tray = Some(tray),
            // Without a tray, closing the window ends the program as before.
            Err(error) => eprintln!("{error}"),
        }
    }

    /// Closing the window keeps the program in the tray while the icon is
    /// shown. 离开 ends it. The smoke test never stays.
    pub fn stays_in_tray(&self) -> bool {
        !self.quitting
            && self.smoke_frames.is_none()
            && self.tray.as_ref().is_some_and(TrayIcon::online)
    }

    /// Run one menu command. True asks for the main window.
    fn tray_command(&mut self, command: Command) -> bool {
        match command {
            Command::Show => return true,
            Command::Wheel4D => {
                self.open(Main::Sensitivity);
                self.page = Page::Wheel;
                return true;
            }
            Command::Autostart(on) => {
                let result = other::set_autostart(on);
                self.report(result);
            }
            Command::Status(display) => {
                self.set_status_display(display);
                self.save_state();
            }
            Command::Quit => self.quitting = true,
        }
        false
    }

    /// Menu commands while the window is open.
    pub(super) fn tray_logic(&mut self, ctx: &egui::Context) {
        let Some(tray) = &mut self.tray else {
            return;
        };
        let view = View {
            connected: self.device.is_some(),
            autostart: other::autostart_file().is_some_and(|path| path.exists()),
            display: self.status_display,
            language: bloody_a90::i18n::language(),
        };
        tray.sync(view);
        for command in tray.commands() {
            if self.tray_command(command) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            if self.quitting {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    /// Keep services and polls running with no window. Returns true when
    /// the window should open again, false when the program should end.
    pub fn run_in_tray(&mut self) -> bool {
        loop {
            let delay = self
                .tick()
                .unwrap_or(std::time::Duration::from_secs(1))
                .min(std::time::Duration::from_secs(1));
            let Some(tray) = &mut self.tray else {
                return false;
            };
            if !tray.online() {
                // The icon is gone; nothing else could reopen the window.
                return true;
            }
            let view = self.tray_view();
            let tray = self.tray.as_mut().unwrap();
            tray.sync(view);
            match tray.commands.recv_timeout(delay) {
                Ok(command) => {
                    if self.tray_command(command) {
                        return true;
                    }
                    if self.quitting {
                        return false;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return false,
            }
        }
    }
}

/// A mouse outline with a split and a wheel on a rounded tile.
fn draw(size: u32, connected: bool) -> Icon {
    let mut image = Pixmap::new(size, size).expect("tray icon size");
    let s = size as f32 / 32.0;
    let (tile, ink) = if connected {
        ([200, 24, 32], [255, 255, 255])
    } else {
        ([92, 92, 96], [200, 200, 200])
    };
    let paint = |[r, g, b]: [u8; 3]| {
        let mut paint = Paint::default();
        paint.set_color_rgba8(r, g, b, 255);
        paint.anti_alias = true;
        paint
    };
    let rounded = |x: f32, y: f32, w: f32, h: f32, r: f32| {
        let mut path = PathBuilder::new();
        path.move_to(x + r, y);
        path.line_to(x + w - r, y);
        path.quad_to(x + w, y, x + w, y + r);
        path.line_to(x + w, y + h - r);
        path.quad_to(x + w, y + h, x + w - r, y + h);
        path.line_to(x + r, y + h);
        path.quad_to(x, y + h, x, y + h - r);
        path.line_to(x, y + r);
        path.quad_to(x, y, x + r, y);
        path.close();
        path.finish().unwrap()
    };
    let scale = Transform::from_scale(s, s);
    image.fill_path(
        &rounded(1.0, 1.0, 30.0, 30.0, 7.0),
        &paint(tile),
        FillRule::Winding,
        scale,
        None,
    );
    let stroke = Stroke {
        width: 2.4,
        ..Default::default()
    };
    image.stroke_path(
        &rounded(10.0, 5.0, 12.0, 22.0, 6.0),
        &paint(ink),
        &stroke,
        scale,
        None,
    );
    let mut split = PathBuilder::new();
    split.move_to(10.0, 14.0);
    split.line_to(22.0, 14.0);
    split.move_to(16.0, 5.0);
    split.line_to(16.0, 14.0);
    image.stroke_path(&split.finish().unwrap(), &paint(ink), &stroke, scale, None);
    // ARGB32 in network byte order, not premultiplied.
    let data = image
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let c = pixel.demultiply();
            [c.alpha(), c.red(), c.green(), c.blue()]
        })
        .collect();
    Icon {
        width: size as i32,
        height: size as i32,
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_have_argb_pixels_for_each_size() {
        for connected in [true, false] {
            for size in [22, 32, 64] {
                let icon = draw(size, connected);
                assert_eq!(icon.data.len(), (size * size * 4) as usize);
                // Opaque tile in the middle, transparent corner.
                let at = |x: u32, y: u32| ((y * size + x) * 4) as usize;
                assert_eq!(icon.data[at(size / 2, size - 3)], 255);
                assert_eq!(icon.data[at(0, 0)], 0);
                // A standalone image for inspecting the drawing.
                let mut png = Pixmap::new(size, size).unwrap();
                for (pixel, argb) in png.pixels_mut().iter_mut().zip(icon.data.chunks(4)) {
                    *pixel = tiny_skia::ColorU8::from_rgba(argb[1], argb[2], argb[3], argb[0])
                        .premultiply();
                }
                png.save_png(std::env::temp_dir().join(format!("a90-tray-{size}-{connected}.png")))
                    .unwrap();
            }
        }
    }
}
