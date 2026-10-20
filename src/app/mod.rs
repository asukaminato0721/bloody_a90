mod associations;
mod gestures;
mod host;
mod host_buttons;
mod keys;
mod library;
mod oscar;
mod other;
mod sensitivity;
mod shooting;
mod wheel4d;

use crate::{
    skin,
    worker::{self, Job, Refresh, Reply, Snapshot},
};
use bloody_a90::{
    associations::Rules,
    backup::Backup,
    configuration::Configuration,
    macros::{self, Program, Repeat},
};
use eframe::egui::{self, Align2, RichText, vec2};
use std::{
    fs,
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
};

/// Dashboard main tabs, in the order of rb_MainTab1..6.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Main {
    Keys,
    Sensitivity,
    Programming,
    Lighting,
    Wireless,
    Other,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Gestures,
    Keys,
    Resolution,
    Response,
    Rate,
    Lift,
    Wheel,
    Pointer,
    Oscar,
    Library,
    Shooting,
    Sniper,
    Brightness,
    About,
    Hardware,
    Backup,
    Profile,
    Associations,
}

// Captions from BloodyWorkShop8_ChineseS.ini. None marks functions that this
// tool has not migrated; they stay visible but disabled. "配置文件" and
// "已有备份" are additions of this tool.
fn sub_tabs(main: Main) -> &'static [(&'static str, Option<Page>)] {
    match main {
        Main::Keys | Main::Wireless => &[],
        Main::Sensitivity => &[
            ("分辨率", Some(Page::Resolution)),
            ("按键响应", Some(Page::Response)),
            ("回报率", Some(Page::Rate)),
            ("微精校准", Some(Page::Lift)),
            ("滚轮", Some(Page::Wheel)),
            ("指针", Some(Page::Pointer)),
        ],
        // Kernel 3 order: Weapon, ShootMode, MacroII, MacroEdit3.
        Main::Programming => &[
            ("左键编程", Some(Page::Shooting)),
            ("狙击键", Some(Page::Sniper)),
            ("奥斯卡II", Some(Page::Oscar)),
            ("奥斯卡III", Some(Page::Library)),
        ],
        Main::Lighting => &[("灯光亮度", Some(Page::Brightness)), ("灯光控制", None)],
        Main::Other => &[
            ("关于软件", Some(Page::About)),
            ("关于硬件", Some(Page::Hardware)),
            ("已有备份", Some(Page::Backup)),
            ("配置文件", Some(Page::Profile)),
            ("应用关联", Some(Page::Associations)),
            ("手势", Some(Page::Gestures)),
        ],
    }
}

fn description(page: Page) -> &'static str {
    match page {
        Page::Gestures => "手势：配置全局触发键、编辑动作和文件，并在绘图区域预览识别结果。",
        Page::Keys => {
            "按键设置：选择记录并指定动作，点击“应用”写入鼠标。按键查询可找出物理按键对应的记录。"
        }
        Page::Resolution => "分辨率：设置各档 X / Y 轴 CPI。点击“应用”保存并立即生效。",
        Page::Oscar => {
            "奥斯卡宏编辑器：编辑 M 型文本，导入或导出 AMC 文件。在“按键”页把宏指定给按键。"
        }
        Page::Library => "奥斯卡III：按分类管理 AMC 宏，编辑后可保存、另存为或指定给按键。",
        Page::Shooting | Page::Sniper => {
            "射击编程：编辑连击或循环程序，保存到鼠标，并指定切换按键。"
        }
        Page::Response => "按键响应：硬件响应时间写入鼠标。双击速度写入 KDE 或 GNOME。",
        Page::Wheel => "滚轮：一次滚动的行数，写入桌面设置。",
        Page::Pointer => "指针：指针移动速度，写入桌面中这只鼠标的加速度设置。",
        Page::Rate => "回报率：保存所选回报率，并立即应用。",
        Page::Lift => "微精校准：设置抬起高度，或在使用表面上移动鼠标进行校准。应用后立即生效。",
        Page::Brightness => "灯光亮度：调整鼠标灯光亮度，立即生效。",
        Page::About => "关于软件：独立 Rust 工具的适配进度。",
        Page::Hardware => "关于硬件：设备型号、固件和存储区地址。",
        Page::Backup => "读取已有的 A90 固件 07 备份，查看灵敏度、按键表和宏程序。",
        Page::Profile => "配置文件：导出当前灵敏度和按键设置，或导入后在各页应用。",
        Page::Associations => "应用关联：按前台应用切换鼠标配置。关闭本工具后停止自动切换。",
    }
}

const DEFAULT_DPI: [[u32; 2]; 5] = [[1000; 2], [1200; 2], [1600; 2], [2000; 2], [6200; 2]];

pub struct App {
    main: Main,
    page: Page,
    dpi: [[u32; 2]; 5],
    enabled: [bool; 5],
    selected: usize,
    separate: bool,
    lift_enabled: bool,
    lift_level: u8,
    sensitivity_template: Option<Vec<u16>>,
    press: String,
    release: String,
    repeat: Repeat,
    compiled: Result<Program, String>,
    shooting: shooting::Editor,
    library: library::Editor,
    gestures: gestures::Editor,
    delay: u32,
    motion: [i32; 2],
    goto: u32,
    export_path: String,
    backup_path: String,
    backup_text: String,
    profile_path: String,
    /// The latest device read. Writes always start from it.
    device: Option<Box<Snapshot>>,
    /// Editable button configuration and the image it came from.
    keys: Option<Configuration>,
    source: Option<Backup>,
    keys_dirty: bool,
    profile: usize,
    record: usize,
    choice: keys::Choice,
    choice_value: usize,
    key_modifiers: u8,
    watching: bool,
    held: Vec<u8>,
    /// Desktop settings as read and as edited.
    host: Option<bloody_a90::host::Settings>,
    host_draft: Option<bloody_a90::host::Settings>,
    wheel4d: wheel4d::Editor,
    host_buttons: host_buttons::Editor,
    associations: Rules,
    associations_draft: Rules,
    foreground: Option<String>,
    association_error: Option<String>,
    last_association_poll: std::time::Instant,
    status: String,
    failed: bool,
    pending: bool,
    sender: Sender<Job>,
    receiver: Receiver<Reply>,
    worker: Option<std::thread::JoinHandle<()>>,
    smoke_frames: Option<u8>,
}

fn load_fonts(ctx: &egui::Context) {
    // Load system fonts. The project does not redistribute font files.
    let mut fonts = egui::FontDefinitions::default();
    let fallback = fonts.families[&egui::FontFamily::Proportional].clone();
    let mut bold = fallback.clone();
    for (name, paths) in [
        (
            "cjk",
            [
                "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
                "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            ],
        ),
        (
            "cjk-bold",
            [
                "/usr/share/fonts/noto-cjk/NotoSansCJK-Bold.ttc",
                "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc",
            ],
        ),
    ] {
        let Some(bytes) = paths.iter().find_map(|path| fs::read(path).ok()) else {
            continue;
        };
        fonts
            .font_data
            .insert(name.into(), egui::FontData::from_owned(bytes).into());
        if name == "cjk" {
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                let list = fonts.families.entry(family.clone()).or_default();
                if family == egui::FontFamily::Proportional {
                    list.insert(0, name.into());
                } else {
                    list.push(name.into());
                }
            }
            if !bold.contains(&"cjk-bold".to_owned()) {
                bold.insert(0, name.into());
            }
        } else {
            bold.retain(|font| font != "cjk");
            bold.insert(0, name.into());
        }
    }
    fonts
        .families
        .insert(egui::FontFamily::Name("bold".into()), bold);
    ctx.set_fonts(fonts);
}

fn style(ctx: &egui::Context) {
    ctx.set_visuals(egui::Visuals::dark());
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style.spacing.item_spacing = vec2(10.0, 10.0);
        style.spacing.button_padding = vec2(12.0, 6.0);
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
        style
            .text_styles
            .insert(egui::TextStyle::Monospace, egui::FontId::monospace(14.0));
        let visuals = &mut style.visuals;
        visuals.panel_fill = skin::BACKGROUND;
        visuals.window_fill = skin::BACKGROUND;
        visuals.extreme_bg_color = skin::FIELD;
        visuals.override_text_color = Some(skin::TEXT);
        visuals.selection.bg_fill = skin::RED;
        visuals.selection.stroke.color = skin::TEXT;
        visuals.hyperlink_color = skin::CAPTION_RED;
        for widget in [
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            widget.corner_radius = 0.into();
        }
        visuals.widgets.inactive.weak_bg_fill = egui::Color32::from_rgb(52, 53, 57);
        visuals.widgets.hovered.weak_bg_fill = skin::RED;
        visuals.widgets.active.weak_bg_fill = skin::RED;
        visuals.widgets.noninteractive.bg_stroke.color = egui::Color32::from_rgb(60, 61, 64);
    });
}

// Editor state survives restarts in a small key-value file.
fn state_file() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .map(|dir| dir.join("bloody-a90").join("state"))
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('\n', "\\n")
}

fn unescape(text: &str) -> String {
    let mut output = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match (c, c == '\\') {
            (_, true) => match chars.next() {
                Some('n') => output.push('\n'),
                Some(other) => output.push(other),
                None => {}
            },
            (c, false) => output.push(c),
        }
    }
    output
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut app = Self::from_context(&cc.egui_ctx);
        app.load_state();
        if app.associations.enabled {
            app.start(Job::Associations(app.associations.clone()));
        }
        app.recompile();
        app
    }

    fn from_context(ctx: &egui::Context) -> Self {
        style(ctx);
        load_fonts(ctx);
        let (sender, jobs) = mpsc::channel();
        let (results, receiver) = mpsc::channel();
        let worker = worker::spawn(jobs, results, ctx.clone());
        let press = "LeftDown\ndelay 25\nLeftUp\ndelay 85\nLeftDown\ndelay 25\nLeftUp".to_owned();
        Self {
            main: Main::Sensitivity,
            page: Page::Resolution,
            dpi: DEFAULT_DPI,
            enabled: [true; 5],
            selected: 0,
            separate: false,
            lift_enabled: false,
            lift_level: 16,
            sensitivity_template: None,
            compiled: macros::compile(&press, "", Repeat::Once),
            shooting: shooting::Editor::default(),
            library: library::Editor::default(),
            gestures: gestures::Editor::default(),
            press,
            release: String::new(),
            repeat: Repeat::Once,
            delay: 25,
            motion: [10, 0],
            goto: 1,
            export_path: String::new(),
            backup_path: String::new(),
            backup_text: String::new(),
            profile_path: String::new(),
            device: None,
            keys: None,
            source: None,
            keys_dirty: false,
            profile: 0,
            record: 0,
            choice: keys::Choice::Mouse,
            choice_value: 0,
            key_modifiers: 0,
            watching: false,
            held: Vec::new(),
            host: None,
            host_draft: None,
            wheel4d: wheel4d::Editor::default(),
            host_buttons: host_buttons::Editor::default(),
            associations: Rules::default(),
            associations_draft: Rules::default(),
            foreground: None,
            association_error: None,
            last_association_poll: std::time::Instant::now(),
            status: "就绪".into(),
            failed: false,
            pending: false,
            sender,
            receiver,
            worker: Some(worker),
            smoke_frames: None,
        }
    }

    pub fn smoke_test(mut self) -> Self {
        self.smoke_frames = Some(0);
        self
    }

    /// Connect at startup when the vendor interface exists.
    pub fn connect_at_start(mut self) -> Self {
        if bloody_a90::device::find().is_ok() {
            self.start(Job::Connect);
        }
        self
    }

    fn load_state(&mut self) {
        let Some(text) = state_file().and_then(|path| fs::read_to_string(path).ok()) else {
            return;
        };
        for line in text.lines() {
            let Some((key, value)) = line.split_once('\t') else {
                continue;
            };
            let value = unescape(value);
            let numbers: Vec<u32> = value.split(',').filter_map(|v| v.parse().ok()).collect();
            match key {
                "dpi" if numbers.len() == 10 => {
                    for (index, axes) in self.dpi.iter_mut().enumerate() {
                        *axes = [numbers[index * 2], numbers[index * 2 + 1]];
                    }
                }
                "enabled" if numbers.len() == 5 && numbers.contains(&1) => {
                    for (flag, number) in self.enabled.iter_mut().zip(&numbers) {
                        *flag = *number == 1;
                    }
                }
                "selected" if numbers.len() == 1 && (numbers[0] as usize) < 5 => {
                    self.selected = numbers[0] as usize;
                }
                "separate" => self.separate = value == "1",
                "press" => self.press = value,
                "release" => self.release = value,
                "repeat" => {
                    self.repeat = match value.as_str() {
                        "held" => Repeat::Held,
                        "toggle" => Repeat::Toggle,
                        _ => Repeat::Once,
                    }
                }
                "export_path" => self.export_path = value,
                "backup_path" => self.backup_path = value,
                "profile_path" => self.profile_path = value,
                "shooting" => self.shooting.load(&value),
                "macro_library" => self.library.set_root(value),
                "gesture_16" => self.gestures.load_state(0, &value),
                "gesture_mouse" => self.gestures.load_state(1, &value),
                "gesture_triggers" => {
                    if numbers.len() == 2 {
                        for (target, value) in self.gestures.triggers.iter_mut().zip(&numbers) {
                            *target = bloody_a90::linux_input::BUTTONS
                                .get(value.saturating_sub(1) as usize)
                                .copied()
                                .filter(|_| *value != 0);
                        }
                    }
                }
                "associations" => {
                    if let Ok(rules) = Rules::decode(&value) {
                        self.associations = rules.clone();
                        self.associations_draft = rules;
                    }
                }
                _ => {}
            }
        }
        if !self.enabled[self.selected] {
            self.selected = self.enabled.iter().position(|e| *e).unwrap_or(0);
        }
    }

    fn save_state(&self) {
        let Some(path) = state_file() else {
            return;
        };
        let join = |values: Vec<String>| values.join(",");
        let entries = [
            (
                "dpi",
                join(self.dpi.iter().flatten().map(u32::to_string).collect()),
            ),
            (
                "enabled",
                join(
                    self.enabled
                        .iter()
                        .map(|e| (*e as u8).to_string())
                        .collect(),
                ),
            ),
            ("selected", self.selected.to_string()),
            ("separate", (self.separate as u8).to_string()),
            ("press", self.press.clone()),
            ("release", self.release.clone()),
            (
                "repeat",
                match self.repeat {
                    Repeat::Once => "once",
                    Repeat::Held => "held",
                    Repeat::Toggle => "toggle",
                }
                .into(),
            ),
            ("export_path", self.export_path.clone()),
            ("backup_path", self.backup_path.clone()),
            ("profile_path", self.profile_path.clone()),
            ("shooting", self.shooting.state()),
            ("macro_library", self.library.root.clone()),
            ("gesture_16", self.gestures.state(0)),
            ("gesture_mouse", self.gestures.state(1)),
            (
                "gesture_triggers",
                self.gestures
                    .triggers
                    .iter()
                    .map(|code| {
                        code.and_then(|code| {
                            bloody_a90::linux_input::BUTTONS
                                .iter()
                                .position(|button| *button == code)
                        })
                        .map_or(0, |index| index + 1)
                        .to_string()
                    })
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            ("associations", self.associations.encode()),
        ];
        let text: String = entries
            .iter()
            .map(|(key, value)| format!("{key}\t{}\n", escape(value)))
            .collect();
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let _ = fs::write(path, text);
    }

    fn start(&mut self, job: Job) {
        let quiet = matches!(job, Job::Held | Job::Foreground);
        match self.sender.send(job) {
            Ok(()) => {
                self.pending = true;
                if !quiet {
                    self.failed = false;
                    self.status = "正在处理…".into();
                }
            }
            Err(_) => self.report(Err("工作线程已关闭".into())),
        }
    }

    fn report(&mut self, result: Result<String, String>) {
        self.failed = result.is_err();
        self.status = result.unwrap_or_else(|error| error);
    }

    /// Load the DPI and button editors from an image.
    fn load_source(&mut self, backup: Backup) {
        self.load_sensitivity(&backup);
        self.load_keys(&backup);
        self.source = Some(backup);
    }

    fn load_sensitivity(&mut self, backup: &Backup) {
        self.sensitivity_template = backup.sensitivity_template().ok();
        self.load_lift(backup);
        self.load_dpi(backup);
    }

    fn load_lift(&mut self, backup: &Backup) {
        if let Ok(level) = backup
            .sensitivity_template()
            .and_then(|words| bloody_a90::sensitivity::lift_height(&words))
        {
            self.lift_enabled = level.is_some();
            self.lift_level = level.unwrap_or(16);
        }
    }

    fn load_dpi(&mut self, backup: &Backup) {
        if let Ok(data) = backup.sensitivity() {
            let count = data.count.min(5);
            for (index, record) in data.records.iter().take(5).enumerate() {
                if let Some(axes) = bloody_a90::sensitivity::decode(*record) {
                    self.dpi[index] = axes;
                }
            }
            for (index, flag) in self.enabled.iter_mut().enumerate() {
                *flag = index < count;
            }
            self.selected = data.selected.min(count - 1);
            self.separate = self.dpi[..count].iter().any(|[x, y]| x != y);
        }
    }

    fn load_keys(&mut self, backup: &Backup) {
        match backup.configuration() {
            Ok(config) => {
                self.profile = self.profile.min(config.profiles.len() - 1);
                let same = bloody_a90::configuration::build(&config)
                    .is_ok_and(|words| self.host_buttons.applied.matches(&words[..16]));
                self.host_buttons.rules = if same {
                    self.host_buttons.applied.clone()
                } else {
                    bloody_a90::host_buttons::Settings::default()
                };
                self.host_buttons.rules.resize(config.profiles.len());
                self.keys = Some(config);
            }
            Err(error) => {
                self.keys = None;
                self.report(Err(format!("无法转换按键表：{error}")));
            }
        }
        self.keys_dirty = false;
    }

    fn refresh_editors(&mut self, scope: Refresh, backup: Backup) {
        match scope {
            Refresh::All => {
                self.load_sensitivity(&backup);
                self.load_keys(&backup);
            }
            Refresh::Sensitivity => {
                self.sensitivity_template = backup.sensitivity_template().ok();
                self.load_dpi(&backup);
            }
            Refresh::Rate | Refresh::Lift => {
                if let Ok(saved) = backup.sensitivity_template() {
                    match &mut self.sensitivity_template {
                        Some(draft) => {
                            let offset = if matches!(scope, Refresh::Rate) {
                                0x1e
                            } else {
                                0x1f
                            };
                            draft[offset] = saved[offset];
                            *draft = bloody_a90::sensitivity::finish(std::mem::take(draft));
                        }
                        None => self.sensitivity_template = Some(saved),
                    }
                }
                if matches!(scope, Refresh::Lift) {
                    self.load_lift(&backup);
                }
            }
            Refresh::Buttons => self.load_keys(&backup),
        }
        self.source = Some(backup);
    }

    fn open(&mut self, main: Main) {
        self.main = main;
        self.page = match main {
            Main::Keys | Main::Wireless => Page::Keys,
            _ => sub_tabs(main)
                .iter()
                .find_map(|(_, page)| *page)
                .unwrap_or(Page::Keys),
        };
    }

    fn connected(&self) -> bool {
        self.device.is_some()
    }

    fn dashboard(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        let state = match &self.device {
            Some(device) => format!("已连接 · 固件 {:02x}", device.identity.firmware),
            None => "未连接".into(),
        };
        skin::device(ui, "设备一", "A90", &state);
        ui.add_space(12.0);
        for (main, caption, enabled) in [
            (Main::Keys, "按键", true),
            (Main::Sensitivity, "灵敏度", true),
            (Main::Programming, "编程设置", true),
            (Main::Lighting, "灯光设置", true),
            // A90 is a wired mouse.
            (Main::Wireless, "无线设置", false),
            (Main::Other, "其他设置", true),
        ] {
            if skin::main_tab(ui, caption, self.main == main, enabled).clicked() {
                self.open(main);
            }
        }
        // lb_VersionInfo and lb_Version sit at the bottom of the dashboard.
        let bottom = ui.max_rect().left_bottom();
        ui.painter().text(
            bottom + vec2(14.0, -52.0),
            Align2::LEFT_BOTTOM,
            "版本信息",
            skin::bold(22.0),
            skin::TEXT,
        );
        ui.painter().text(
            bottom + vec2(14.0, -24.0),
            Align2::LEFT_BOTTOM,
            concat!("v", env!("CARGO_PKG_VERSION")),
            skin::regular(14.0),
            skin::GRAY,
        );
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        let painter = ui.painter();
        painter.rect_filled(
            egui::Rect::from_min_size(rect.min, vec2(175.0, rect.height())),
            0,
            skin::DASHBOARD,
        );
        painter.text(
            rect.left_center() + vec2(88.0, -8.0),
            Align2::CENTER_CENTER,
            "BLOODY",
            skin::bold(30.0),
            skin::CAPTION_RED,
        );
        painter.text(
            rect.left_center() + vec2(88.0, 22.0),
            Align2::CENTER_CENTER,
            "A90 配置工具",
            skin::regular(13.0),
            skin::LIGHT_GRAY,
        );
        let mut right = rect.right_center() - vec2(24.0, 0.0);
        let button = egui::Rect::from_min_size(right - vec2(120.0, 18.0), vec2(120.0, 36.0));
        let caption = if self.connected() {
            "重新读取"
        } else {
            "连接鼠标"
        };
        if ui
            .put(button, |ui: &mut egui::Ui| {
                skin::button(ui, caption, 120.0, !self.pending)
            })
            .clicked()
        {
            self.start(Job::Connect);
        }
        right.x -= 140.0;
        let text = match &self.device {
            Some(device) => format!(
                "{} · 型号 {:04x} · 核心 {}",
                device.path, device.identity.model, device.identity.core
            ),
            None => "未连接鼠标 · 可离线编辑".into(),
        };
        ui.painter().text(
            right,
            Align2::RIGHT_CENTER,
            text,
            skin::regular(14.0),
            skin::GRAY,
        );
    }

    fn sub_strip(&mut self, ui: &mut egui::Ui) {
        let tabs = sub_tabs(self.main);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            if tabs.is_empty() {
                ui.allocate_space(vec2(1.0, 51.0));
            }
            for (caption, page) in tabs {
                let selected = *page == Some(self.page);
                if skin::sub_tab(ui, caption, selected, page.is_some())
                    .on_disabled_hover_text("A90 为单色灯光，不使用 RGB 灯效控制。")
                    .clicked()
                    && let Some(page) = page
                {
                    self.page = *page;
                }
            }
        });
        // gl_SeparateLine sits directly below the tabs.
        ui.add_space(-ui.spacing().item_spacing.y);
        let (rect, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), 3.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 0, skin::RED);
        ui.add_space(2.0);
        ui.label(
            RichText::new(description(self.page))
                .font(skin::regular(14.0))
                .color(skin::LIGHT_GRAY),
        );
        ui.add_space(14.0);
    }

    fn receive(&mut self) {
        while let Ok(reply) = self.receiver.try_recv() {
            self.pending = false;
            match reply {
                Reply::Backup(Ok(backup)) => {
                    self.backup_text = backup.summary();
                    self.report(Ok("备份读取完成".into()));
                    if !self.connected() {
                        self.load_source(backup);
                    }
                }
                Reply::Backup(Err(error)) => {
                    self.backup_text.clear();
                    self.report(Err(error));
                }
                Reply::Export(result) => self.report(result),
                Reply::GestureSaved(path, result) => self.gesture_saved(path, result),
                Reply::Gestures(result) => {
                    let mut failed = None;
                    let result = result.map(|update| {
                        if update.requested && !update.status.running {
                            failed = Some(update.status.message.clone());
                        }
                        if let Some((settings, bytes)) = update.host {
                            self.host_buttons.expected_file = bytes;
                            self.host_buttons.rules.gestures = settings.gestures.clone();
                            self.host_buttons.applied = settings;
                        }
                        update.status
                    });
                    self.gesture_status(result, true);
                    if let Some(error) = failed {
                        self.report(Err(error));
                    }
                }
                Reply::GestureStatus(mut status, gestures_enabled, wheel_enabled) => {
                    self.wheel_runtime_status(&status, wheel_enabled);
                    if !gestures_enabled && status.running {
                        status.running = false;
                        status.message = "全局手势未启用".into();
                    }
                    self.gesture_status(Ok(status), false);
                }
                Reply::Wheel(result) => self.wheel_reply(result),
                Reply::HostButtons(result) => self.host_button_status(result),
                Reply::HostButtonsEnabled(result) => match result {
                    Ok((settings, bytes, status)) => {
                        self.host_buttons.rules.enabled = settings.enabled;
                        self.host_buttons.applied.enabled = settings.enabled;
                        self.host_buttons.expected_file = Some(bytes);
                        if settings.enabled && !settings.is_empty() && !status.running {
                            self.report(Err(status.message.clone()));
                        }
                        self.host_button_status(Ok(status));
                    }
                    Err(error) => self.host_button_status(Err(error)),
                },
                Reply::Device(scope, Ok((snapshot, message))) => {
                    self.refresh_editors(scope, snapshot.backup.clone());
                    if matches!(scope, Refresh::All | Refresh::Buttons)
                        && let Some(config) = &self.keys
                    {
                        self.host_buttons
                            .load(snapshot.host_buttons.clone(), config);
                        self.host_buttons.expected_file = snapshot.host_button_file.clone();
                        self.host_key_selected();
                    }
                    self.device = Some(snapshot);
                    self.report(Ok(message));
                }
                Reply::Device(_, Err(error)) => self.report(Err(error)),
                Reply::Brightness(Ok(level)) => {
                    if let Some(device) = &mut self.device {
                        device.brightness = level;
                    }
                    self.report(Ok(format!("亮度已设为 {level}")));
                }
                Reply::Brightness(Err(error)) => self.report(Err(error)),
                Reply::Held(Ok(held)) => self.held = held,
                Reply::Held(Err(error)) => {
                    self.watching = false;
                    self.report(Err(error));
                }
                Reply::File(purpose, result) => self.file_read(purpose, result),
                Reply::Host(Ok((settings, message))) => {
                    self.host_draft = Some(settings.clone());
                    self.host = Some(settings);
                    self.report(Ok(message));
                }
                Reply::Host(Err(error)) => self.report(Err(error)),
                Reply::CalibratedLift(Ok(level)) => {
                    self.lift_enabled = true;
                    self.lift_level = level;
                    self.report(Ok(format!("校准结果：{level} 级。点击“应用”保存。")));
                }
                Reply::CalibratedLift(Err(error)) => self.report(Err(error)),
                Reply::Associations(result) => {
                    self.report(result.map(|()| "应用关联设置已应用".into()));
                }
                Reply::Foreground(Ok(foreground)) => {
                    self.foreground = foreground.app;
                    self.association_error = None;
                    if let Some(profile) = foreground.profile
                        && let Some(device) = &mut self.device
                    {
                        device.profile = profile;
                    }
                }
                Reply::Foreground(Err(error)) => self.association_error = Some(error),
                Reply::Library(result) => self.library_reply(result),
            }
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = self.sender.send(Job::Shutdown);
        if let Some(worker) = self.worker.take() {
            // Let pending flash writes and calibration cleanup finish.
            let _ = worker.join();
        }
    }
}

// Oscar memo: a light editor with a line-number gutter.
fn memo(ui: &mut egui::Ui, id: &str, text: &mut String, rows: usize, enabled: bool) -> bool {
    let font = egui::FontId::monospace(14.0);
    let row = ui.fonts_mut(|fonts| fonts.row_height(&font));
    let lines = text.split('\n').count().max(rows);
    let mut changed = false;
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let numbers: String = (1..=lines).map(|n| format!("{n:>4}\n")).collect();
            egui::Frame::new()
                .fill(skin::GUTTER)
                .inner_margin(egui::Margin::symmetric(4, 2))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(numbers.trim_end())
                            .font(font.clone())
                            .color(egui::Color32::from_rgb(90, 90, 90))
                            .line_height(Some(row)),
                    );
                });
            changed = ui
                .add(
                    egui::TextEdit::multiline(text)
                        .id_salt(id)
                        .font(font.clone())
                        .background_color(skin::MEMO)
                        .text_color(egui::Color32::BLACK)
                        .desired_rows(rows)
                        .desired_width(f32::INFINITY),
                )
                .changed();
        });
    });
    changed
}

fn byte_hex(words: &[u16]) -> String {
    macros::bytes(words)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A single-line path field in the official dark style.
fn path_field(ui: &mut egui::Ui, text: &mut String, hint: &str, width: f32) {
    ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(hint)
            .background_color(skin::FIELD)
            .desired_width(width),
    );
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.receive();
        if self.associations.enabled && self.connected() && self.association_error.is_none() {
            if !self.pending && self.last_association_poll.elapsed().as_millis() >= 750 {
                self.last_association_poll = std::time::Instant::now();
                self.start(Job::Foreground);
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        // Give a due foreground poll priority over the faster button query.
        if self.gestures.running
            || self.wheel4d.running
            || (self.wheel4d.loaded
                && self
                    .device
                    .as_ref()
                    .is_some_and(|d| self.wheel4d.applied.enabled_for(d.identity.core)))
            || (self.host_buttons.running
                && self
                    .host_buttons
                    .applied
                    .assigned_gesture_kinds()
                    .into_iter()
                    .any(|kind| kind))
        {
            if !self.pending && self.gestures.last_poll.elapsed().as_millis() >= 250 {
                self.gestures.last_poll = std::time::Instant::now();
                self.start(Job::GestureStatus);
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if self.connected() {
            if !self.pending && self.host_buttons.last_poll.elapsed().as_millis() >= 500 {
                self.host_buttons.last_poll = std::time::Instant::now();
                self.start(Job::HostButtonsStatus);
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        if self.watching && self.page == Page::Keys {
            if !self.pending {
                self.start(Job::Held);
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
    }

    fn on_exit(&mut self) {
        if self.smoke_frames.is_none() {
            self.save_state();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(frames) = self.smoke_frames.as_mut() {
            *frames += 1;
            // Render each page before the smoke test closes the window.
            let pages = [
                (Main::Keys, Page::Keys),
                (Main::Sensitivity, Page::Resolution),
                (Main::Sensitivity, Page::Response),
                (Main::Sensitivity, Page::Rate),
                (Main::Sensitivity, Page::Lift),
                (Main::Sensitivity, Page::Wheel),
                (Main::Sensitivity, Page::Pointer),
                (Main::Programming, Page::Oscar),
                (Main::Programming, Page::Library),
                (Main::Programming, Page::Shooting),
                (Main::Programming, Page::Sniper),
                (Main::Lighting, Page::Brightness),
                (Main::Other, Page::About),
                (Main::Other, Page::Hardware),
                (Main::Other, Page::Backup),
                (Main::Other, Page::Profile),
                (Main::Other, Page::Associations),
                (Main::Other, Page::Gestures),
            ];
            let index = (*frames as usize / 2).min(pages.len() - 1);
            (self.main, self.page) = pages[index];
            if *frames as usize >= pages.len() * 2 + 5 {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ui.ctx().request_repaint();
        }
        let plain = egui::Frame::new().fill(skin::BACKGROUND);
        egui::Panel::top("title")
            .exact_size(84.0)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(skin::BACKGROUND))
            .show(ui, |ui| self.header(ui));
        egui::Panel::bottom("status")
            .show_separator_line(false)
            .frame(plain.inner_margin(egui::Margin::symmetric(20, 6)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(175.0);
                    if self.pending && !self.watching {
                        ui.spinner();
                    }
                    ui.label(
                        RichText::new(&self.status)
                            .font(skin::regular(14.0))
                            .color(if self.failed { skin::ERROR } else { skin::GRAY }),
                    );
                });
            });
        egui::Panel::left("dashboard")
            .exact_size(175.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(skin::DASHBOARD))
            .show(ui, |ui| self.dashboard(ui));
        egui::CentralPanel::default()
            .frame(plain.inner_margin(egui::Margin {
                left: 20,
                right: 20,
                top: 3,
                bottom: 0,
            }))
            .show(ui, |ui| {
                self.sub_strip(ui);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    match self.page {
                        Page::Keys => self.keys_page(ui),
                        Page::Resolution => self.resolution(ui),
                        Page::Rate => self.rate(ui),
                        Page::Lift => self.lift(ui),
                        Page::Response => self.response(ui),
                        Page::Wheel => self.wheel(ui),
                        Page::Pointer => self.pointer(ui),
                        Page::Oscar => self.oscar(ui),
                        Page::Library => self.library_page(ui),
                        Page::Shooting => self.shooting_page(ui, false),
                        Page::Sniper => self.shooting_page(ui, true),
                        Page::Brightness => self.brightness(ui),
                        Page::About => self.about(ui),
                        Page::Hardware => self.hardware(ui),
                        Page::Backup => self.backup(ui),
                        Page::Profile => self.profile_file(ui),
                        Page::Associations => self.associations_page(ui),
                        Page::Gestures => self.gestures_page(ui),
                    }
                    ui.add_space(16.0);
                });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bloody_a90::{
        configuration::{self, Action, Profile},
        sensitivity,
    };

    fn image(dpi: u32, key: u16, lift: Option<u8>) -> Backup {
        let mut bytes = vec![0; 32768];
        bytes[0x1804 * 2..0x1806 * 2].copy_from_slice(&[0xfd, 4, 7, 0]);
        let rate = if dpi == 1600 { 250 } else { 1000 };
        let words = sensitivity::block(&[0; 128], &[[dpi; 2]], 0, Some(rate)).unwrap();
        let sensitivity = sensitivity::with_lift_height(&words, lift).unwrap();
        let mut profile = Profile {
            buttons: Default::default(),
            wheel: [Action::Direct(0x262c), Action::Direct(0x262b)],
            gestures: None,
            metadata: [0x01ff, 0],
        };
        profile.buttons[0].press = Action::Direct(key);
        let config = Configuration {
            profiles: vec![profile],
            selected: 0,
            auxiliary: std::array::from_fn(|_| Action::Direct(0x262e)),
            parameters: [50; 6],
            flags: 0x10,
            header_word6: 1,
        };
        let buttons = configuration::build(&config).unwrap();
        for (base, words) in [(0x2400, sensitivity), (0x2600, buttons)] {
            for (i, word) in words.iter().enumerate() {
                bytes[(base + i) * 2..][..2].copy_from_slice(&word.to_le_bytes());
            }
        }
        Backup::parse(&bytes).unwrap()
    }

    #[test]
    fn imported_drafts_survive_either_save_order() {
        for buttons_first in [false, true] {
            let mut app = App::from_context(&egui::Context::default());
            app.load_source(image(1600, 0x2104, Some(9)));
            app.keys_dirty = true;
            if buttons_first {
                app.refresh_editors(Refresh::Buttons, image(800, 0x2104, None));
            } else {
                app.refresh_editors(Refresh::Sensitivity, image(1600, 0x2105, Some(9)));
            }
            // The other half of the import must remain editable after a save.
            assert_eq!(app.dpi[0], [1600; 2]);
            assert_eq!(
                app.keys.as_ref().unwrap().profiles[0].buttons[0].press,
                Action::Direct(0x2104)
            );
            assert_eq!(app.keys_dirty, !buttons_first);
            let template = app.sensitivity_template.as_ref().unwrap();
            assert_eq!(template[0x1e], 4);
            assert_eq!(sensitivity::lift_height(template), Ok(Some(9)));
            app.refresh_editors(
                if buttons_first {
                    Refresh::Sensitivity
                } else {
                    Refresh::Buttons
                },
                image(1600, 0x2104, Some(9)),
            );
            assert!(!app.keys_dirty);
            assert_eq!(app.dpi[0], [1600; 2]);
            assert_eq!(
                app.keys.as_ref().unwrap().profiles[0].buttons[0].press,
                Action::Direct(0x2104)
            );
        }
    }

    #[test]
    fn imported_host_actions_survive_dpi_refresh_and_render_button_and_wheel_editors() {
        use crate::worker::Purpose;
        let context = egui::Context::default();
        let mut app = App::from_context(&context);
        let backup = image(1600, 0x2104, None);
        app.load_source(backup.clone());
        let mut settings = bloody_a90::host_buttons::Settings::default();
        settings.resize(1);
        let action = bloody_a90::gestures::Action::input_text("中😀\n<&")
            .unwrap()
            .with_mode(0);
        settings.profiles[0][3] = Some(action.clone());
        settings.profiles[0][4] = Some(bloody_a90::gestures::Action::new(24, 0, 0, "全部最小化"));
        settings.profiles[0][8] = Some(bloody_a90::gestures::Action::new(2, 0, 0, "16 合一手势"));
        settings.profiles[0][9] = Some(bloody_a90::gestures::Action::new(2, 1, 0, "鼠标手势"));
        let mut documents = [
            bloody_a90::gestures::Document::new(bloody_a90::gestures::Kind::Sixteen),
            bloody_a90::gestures::Document::new(bloody_a90::gestures::Kind::Mouse),
        ];
        documents[0].actions[0] = action.clone();
        settings.gestures = Some(Box::new(bloody_a90::host_buttons::Gestures {
            enabled: true,
            documents: documents.clone(),
        }));
        settings.bind(&backup.configuration().unwrap()).unwrap();
        let bytes = bloody_a90::profile_file::export_all(&backup, &settings).unwrap();
        app.file_read(Purpose::Profile, Ok(bytes));
        assert!(!app.failed);
        assert!(app.keys_dirty);
        assert_eq!(app.host_buttons.rules, settings);
        assert_eq!(app.gestures.documents, documents);
        app.refresh_editors(Refresh::Sensitivity, image(800, 0x2104, None));
        assert_eq!(app.host_buttons.rules, settings);
        assert_eq!(app.gestures.documents, documents);
        app.record = 3;
        app.host_key_selected();
        assert!(app.choice == keys::Choice::Host);
        assert_eq!(app.host_buttons.action().unwrap(), action);
        let mut output = context.run_ui(Default::default(), |ui| app.keys_page(ui));
        output.textures_delta.clear();
        for slot in [4, 8, 9] {
            app.record = slot;
            app.host_key_selected();
            assert!(app.choice == keys::Choice::Host);
            assert_eq!(
                app.host_buttons.action().unwrap(),
                settings.profiles[0][slot].clone().unwrap()
            );
            let mut output = context.run_ui(Default::default(), |ui| app.keys_page(ui));
            output.textures_delta.clear();
        }
        app.record = 24;
        app.host_key_selected();
        let mut output = context.run_ui(Default::default(), |ui| app.keys_page(ui));
        output.textures_delta.clear();
        assert_eq!(
            app.keys.as_ref().unwrap().profiles[0].wheel[0],
            Action::Direct(0x262c)
        );
        app.file_read(
            Purpose::Profile,
            Ok(bloody_a90::profile_file::export(&backup).unwrap()),
        );
        assert!(app.host_buttons.rules.is_empty());
    }

    #[test]
    fn sensitivity_pages_keep_other_drafts_in_every_save_order() {
        use bloody_a90::device::{Identity, MemoryInfo};
        for order in [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ] {
            let mut app = App::from_context(&egui::Context::default());
            app.device = Some(Box::new(Snapshot {
                path: "test device".into(),
                identity: Identity {
                    model: 0x04fd,
                    firmware: 7,
                    core: 2,
                },
                info: MemoryInfo {
                    rate_hz: 1000,
                    sensitivity: 0x2400,
                    buttons: 0x2600,
                    settings: 0x3e00,
                    factory_sensitivity: 0x1806,
                    device_data: 0x1800,
                    factory_buttons: 0x183c,
                },
                brightness: 3,
                profile: (0, 0),
                rate: (1, 0),
                backup: image(800, 0x2105, None),
                host_buttons: Ok(bloody_a90::host_buttons::Settings::default()),
                host_button_file: None,
            }));
            app.load_source(image(1600, 0x2104, Some(9)));
            app.keys_dirty = true;
            app.dpi[0] = [3200; 2];
            app.lift_level = 7;
            for page in order {
                let before = app
                    .device
                    .as_ref()
                    .unwrap()
                    .backup
                    .sensitivity_template()
                    .unwrap();
                let (scope, words) = match page {
                    0 => (Refresh::Sensitivity, app.sensitivity_block().unwrap()),
                    1 => (Refresh::Lift, app.lift_block().unwrap()),
                    _ => (Refresh::Rate, app.rate_block(500).unwrap()),
                };
                // A component save must not silently apply the pending DPI
                // import or any other component from that import.
                if page != 0 {
                    let offset = if page == 1 { 0x1f } else { 0x1e };
                    for i in 4..words.len() {
                        if i != offset {
                            assert_eq!(words[i], before[i], "{order:?}, page {page}, word {i}");
                        }
                    }
                }
                let mut bytes = app.device.as_ref().unwrap().backup.to_bytes();
                for (i, word) in words.iter().enumerate() {
                    bytes[(0x2400 + i) * 2..][..2].copy_from_slice(&word.to_le_bytes());
                }
                let saved = Backup::parse(&bytes).unwrap();
                app.refresh_editors(scope, saved.clone());
                app.device.as_mut().unwrap().backup = saved;
                assert_eq!(app.dpi[0], [3200; 2], "{order:?}");
                assert_eq!((app.lift_enabled, app.lift_level), (true, 7), "{order:?}");
                assert!(app.keys_dirty);
                assert_eq!(
                    app.keys.as_ref().unwrap().profiles[0].buttons[0].press,
                    Action::Direct(0x2104)
                );
            }
            let saved = &app.device.as_ref().unwrap().backup;
            assert_eq!(
                sensitivity::decode(saved.sensitivity().unwrap().records[0]),
                Some([3200; 2])
            );
            let template = saved.sensitivity_template().unwrap();
            assert_eq!(template[0x1e], 2);
            assert_eq!(sensitivity::lift_height(&template), Ok(Some(7)));
        }
    }
}
