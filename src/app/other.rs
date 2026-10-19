use super::{App, path_field};
use crate::{
    skin,
    worker::{Job, Purpose},
};
use bloody_a90::profile_file;
use eframe::egui::{self, RichText};

impl App {
    pub(super) fn file_read(&mut self, purpose: Purpose, result: Result<Vec<u8>, String>) {
        let result = result.and_then(|bytes| match purpose {
            Purpose::Amc => self.import_amc(&bytes),
            Purpose::Gestures => self.gestures.imported(&bytes),
            Purpose::Profile => {
                let base = self
                    .device
                    .as_ref()
                    .map(|device| device.backup.clone())
                    .or_else(|| self.source.clone())
                    .ok_or("请先连接鼠标或读取备份，导入需要以它为底")?;
                let (imported, host) = profile_file::import_all(&bytes, &base)?;
                self.load_source(imported);
                self.host_buttons.rules = host.unwrap_or_default();
                if let Some(gestures) = &self.host_buttons.rules.gestures {
                    self.gestures.documents = gestures.documents.clone();
                }
                self.host_buttons
                    .rules
                    .resize(self.keys.as_ref().map_or(0, |config| config.profiles.len()));
                self.keys_dirty = true;
                Ok("已导入配置文件。请在“分辨率”和“按键”页点击“应用”写入鼠标。".into())
            }
        });
        self.report(result);
    }

    pub(super) fn backup(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            path_field(ui, &mut self.backup_path, "32768 字节备份的文件路径", 480.0);
            let enabled = !self.pending && !self.backup_path.trim().is_empty();
            if skin::button(ui, "读取文件", 140.0, enabled).clicked() {
                self.start(Job::Backup(self.backup_path.trim().to_owned()));
            }
            let device = self.device.as_ref().map(|device| device.backup.summary());
            if skin::button(ui, "解析鼠标闪存", 140.0, device.is_some())
                .on_disabled_hover_text("请先连接鼠标")
                .clicked()
                && let Some(text) = device
            {
                self.backup_text = text;
            }
        });
        if self.backup_text.is_empty() {
            ui.label(RichText::new("尚未读取文件。").color(skin::GRAY));
        } else {
            if skin::button(ui, "复制解析结果", 140.0, true).clicked() {
                ui.ctx().copy_text(self.backup_text.clone());
            }
            skin::sheet().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.monospace(&self.backup_text);
            });
        }
        ui.label(
            RichText::new("未连接鼠标时，读取的备份会载入分辨率和按键页供查看与编辑。")
                .color(skin::GRAY),
        );
    }

    pub(super) fn profile_file(&mut self, ui: &mut egui::Ui) {
        skin::sheet().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("配置文件 :").font(skin::bold(17.0)));
            ui.horizontal(|ui| {
                path_field(ui, &mut self.profile_path, "配置文件路径，例如 a90.cfg", 480.0);
            });
            let path = self.profile_path.trim().to_owned();
            let source = self.device.as_ref().map(|device| &device.backup).or(self.source.as_ref());
            let export = source.map(|source| profile_file::export_all(source, &self.host_buttons.applied));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                let ready = !self.pending && !path.is_empty();
                if skin::button(ui, "导出", 140.0, ready && export.is_some())
                    .on_disabled_hover_text("需要文件路径，以及已连接的鼠标或已读取的备份")
                    .clicked()
                {
                    match export {
                        Some(Ok(bytes)) => self.start(Job::Export(path.clone(), bytes)),
                        Some(Err(error)) => self.report(Err(error)),
                        None => {}
                    }
                }
                if skin::button(ui, "导入", 140.0, ready).clicked() {
                    self.start(Job::Read(path.clone(), Purpose::Profile));
                }
            });
            ui.label(
                RichText::new("文件保存当前生效的灵敏度、全部按键配置、宏和已保存的主机动作。导入不会直接写入鼠标。")
                    .font(skin::regular(13.0))
                    .color(skin::GRAY),
            );
        });
    }

    pub(super) fn hardware(&mut self, ui: &mut egui::Ui) {
        skin::sheet().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("关于硬件").font(skin::bold(22.0)));
            let Some(device) = &self.device else {
                ui.label(RichText::new("请先连接鼠标。").color(skin::GRAY));
                return;
            };
            let info = &device.info;
            egui::Grid::new("hardware").striped(true).show(ui, |ui| {
                for (name, value) in [
                    ("接口", device.path.clone()),
                    ("型号", format!("{:04x}", device.identity.model)),
                    ("固件", format!("{:02x}", device.identity.firmware)),
                    ("核心", device.identity.core.to_string()),
                    ("回报率", format!("{} Hz", info.rate_hz)),
                    (
                        "灵敏度区",
                        format!(
                            "{:04x}（出厂 {:04x}）",
                            info.sensitivity, info.factory_sensitivity
                        ),
                    ),
                    (
                        "按键区",
                        format!("{:04x}（出厂 {:04x}）", info.buttons, info.factory_buttons),
                    ),
                    ("设置区", format!("{:04x}", info.settings)),
                    ("设备数据", format!("{:04x}", info.device_data)),
                    (
                        "当前配置",
                        format!("{} / {}", device.profile.0 + 1, device.profile.1 + 1),
                    ),
                ] {
                    ui.label(name);
                    ui.label(value);
                    ui.end_row();
                }
            });
            ui.label(device.backup.shooting_route().description());
        });
    }

    pub(super) fn about(&mut self, ui: &mut egui::Ui) {
        skin::sheet().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Bloody A90 配置工具").font(skin::bold(22.0)));
            ui.label(
                RichText::new(concat!(
                    "独立 Rust 工具 · egui / eframe 0.37 · 版本 ",
                    env!("CARGO_PKG_VERSION")
                ))
                .color(skin::GRAY),
            );
            ui.add_space(8.0);
            for text in [
                "已接入：设备识别、闪存读取、灯光亮度、DPI 和回报率保存、按键与宏保存、射击编程、抬起高度、表面校准、硬件响应时间、配置文件、AMC 宏文件、HighLanguage。",
                "写入只使用与官方程序相同的保存流程，并限制在灵敏度页和 2600 至 3cff 的按键区。每次连接都会先保存完整备份。",
                "即时 DPI 和回报率使用已验证的命令 0d。不发送传感器传输、引导程序覆盖或授权写入命令。",
                "桌面设置：双击速度、滚轮行数、指针速度。应用关联支持 KDE Wayland 和 X11。手势支持文件编辑、全局触发和主机宏。A90 不支持 RGB 灯效。",
                "研究记录见 docs/research。",
            ] {
                ui.label(text);
            }
            ui.add_space(8.0);
            ui.hyperlink_to("egui 文档", "https://docs.rs/egui/0.37.0/egui/");
        });
    }
}
