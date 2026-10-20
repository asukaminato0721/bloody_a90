use super::{App, path_field};
use crate::{
    skin,
    worker::{Job, Purpose},
};
use bloody_a90::profile_file;
use bloody_a90::{
    i18n::{self, tr, tr_text},
    tf,
};
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
                    .ok_or(tr("请先连接鼠标或读取备份，导入需要以它为底"))?;
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
                Ok(tr("已导入配置文件。请在“分辨率”和“按键”页点击“应用”写入鼠标。").into())
            }
        });
        self.report(result);
    }

    pub(super) fn backup(&mut self, ui: &mut egui::Ui) {
        skin::sheet().show(ui, |ui| {
            ui.set_width(ui.available_width());
            self.backup_body(ui);
        });
    }

    fn backup_body(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            path_field(
                ui,
                &mut self.backup_path,
                tr("32768 字节备份的文件路径"),
                480.0,
            );
            let enabled = !self.pending && !self.backup_path.trim().is_empty();
            if skin::button(ui, tr("读取文件"), 140.0, enabled).clicked() {
                self.start(Job::Backup(self.backup_path.trim().to_owned()));
            }
            let device = self.device.as_ref().map(|device| device.backup.summary());
            if skin::button(ui, tr("解析鼠标闪存"), 140.0, device.is_some())
                .on_disabled_hover_text(tr("请先连接鼠标"))
                .clicked()
                && let Some(text) = device
            {
                self.backup_text = text;
            }
        });
        if self.backup_text.is_empty() {
            ui.label(RichText::new(tr("尚未读取文件。")).color(skin::GRAY));
        } else {
            if skin::button(ui, tr("复制解析结果"), 140.0, true).clicked() {
                ui.ctx().copy_text(self.backup_text.clone());
            }
            skin::sheet().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.monospace(tr_text(&self.backup_text));
            });
        }
        ui.label(
            RichText::new(tr(
                "未连接鼠标时，读取的备份会载入分辨率和按键页供查看与编辑。",
            ))
            .color(skin::GRAY),
        );
    }

    pub(super) fn profile_file(&mut self, ui: &mut egui::Ui) {
        skin::sheet().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(tr("配置文件 :")).font(skin::bold(17.0)));
            ui.horizontal(|ui| {
                path_field(ui, &mut self.profile_path, tr("配置文件路径，例如 a90.cfg"), 480.0);
            });
            let path = self.profile_path.trim().to_owned();
            let source = self.device.as_ref().map(|device| &device.backup).or(self.source.as_ref());
            let export = source.map(|source| profile_file::export_all(source, &self.host_buttons.applied));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                let ready = !self.pending && !path.is_empty();
                if skin::button(ui, tr("导出"), 140.0, ready && export.is_some())
                    .on_disabled_hover_text(tr("需要文件路径，以及已连接的鼠标或已读取的备份"))
                    .clicked()
                {
                    match export {
                        Some(Ok(bytes)) => self.start(Job::Export(path.clone(), bytes)),
                        Some(Err(error)) => self.report(Err(error)),
                        None => {}
                    }
                }
                if skin::button(ui, tr("导入"), 140.0, ready).clicked() {
                    self.start(Job::Read(path.clone(), Purpose::Profile));
                }
            });
            ui.label(
                RichText::new(tr("文件保存当前生效的灵敏度、全部按键配置、宏和已保存的主机动作。导入不会直接写入鼠标。"))
                    .font(skin::regular(13.0))
                    .color(skin::GRAY),
            );
        });
    }

    /// AboutHardware: the 硬件信息 panel with value boxes on the right.
    pub(super) fn hardware(&mut self, ui: &mut egui::Ui) {
        ui.add_space(36.0);
        let help = tr("硬件信息：设备型号、固件和存储区地址。");
        skin::card(ui, Some(skin::Icon::Card), tr("硬件信息"), help, |ui| {
            let Some(device) = &self.device else {
                ui.label(RichText::new(tr("请先连接鼠标。")).color(skin::GRAY));
                return;
            };
            let info = &device.info;
            let rows = [
                (tr("机身码ID"), format!("{:04x}", device.identity.model)),
                (tr("固件"), format!("{:02x}", device.identity.firmware)),
                (tr("核心"), device.identity.core.to_string()),
                (tr("接口"), device.path.clone()),
                (tr("回报率"), format!("{} Hz", info.rate_hz)),
                (
                    tr("当前配置"),
                    format!("{} / {}", device.profile.0 + 1, device.profile.1 + 1),
                ),
                (
                    tr("灵敏度区"),
                    tf!(
                        "{:04x}（出厂 {:04x}）",
                        info.sensitivity,
                        info.factory_sensitivity
                    ),
                ),
                (
                    tr("按键区"),
                    tf!("{:04x}（出厂 {:04x}）", info.buttons, info.factory_buttons),
                ),
                (tr("设置区"), format!("{:04x}", info.settings)),
                (tr("设备数据"), format!("{:04x}", info.device_data)),
            ];
            ui.add_space(-56.0);
            ui.horizontal_top(|ui| {
                ui.add_space((ui.available_width() - 2.0 * 360.0 - 40.0).max(0.0));
                for column in rows.chunks(5) {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 12.0;
                        for (name, value) in column {
                            skin::field(ui, name, value, 240.0);
                        }
                    });
                    ui.add_space(40.0);
                }
            });
            ui.add_space(8.0);
            ui.label(
                RichText::new(tr_text(device.backup.shooting_route().description()))
                    .font(skin::regular(13.0))
                    .color(skin::GRAY),
            );
        });
    }

    /// AboutSoftware: option tiles, then what this tool covers. Tiles for
    /// vendor services (更新通知, 新品通知, 模式管理, 游戏防打扰) are left
    /// out; they have no counterpart on Linux.
    pub(super) fn about(&mut self, ui: &mut egui::Ui) {
        ui.add_space(-10.0);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 15.0;
            ui.add_space(-4.0);
            let width = ((ui.available_width() - 15.0) / 2.0).min(690.0);
            tile(
                ui,
                width,
                skin::Icon::Startup,
                tr("开机自动启动"),
                |ui| {
                    let mut on = autostart_file().is_some_and(|path| path.exists());
                    if skin::toggle(ui, &mut on).changed() {
                        let result = set_autostart(on);
                        self.report(result);
                    }
                },
            );
            tile(ui, width, skin::Icon::Language, tr("显示语言"), |ui| {
                let mut current = i18n::language();
                egui::ComboBox::from_id_salt("language")
                    .selected_text(current.name())
                    .width(120.0)
                    .show_ui(ui, |ui| {
                        for language in i18n::Language::ALL {
                            ui.selectable_value(&mut current, language, language.name());
                        }
                    });
                if current != i18n::language() {
                    i18n::set_language(current);
                    self.save_state();
                }
            });
        });
        ui.add_space(15.0);
        skin::sheet().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(tr("Bloody A90 配置工具")).font(skin::bold(22.0)));
            ui.label(
                RichText::new(tf!(
                    "独立 Rust 工具 · egui / eframe 0.37 · 版本 {}",
                    env!("CARGO_PKG_VERSION")
                ))
                .color(skin::GRAY),
            );
            ui.add_space(8.0);
            for text in [
                tr("已接入：设备识别、闪存读取、灯光亮度、DPI 和回报率保存、按键与宏保存、射击编程、抬起高度、表面校准、硬件响应时间、配置文件、AMC 宏文件、HighLanguage。"),
                tr("写入只使用与官方程序相同的保存流程，并限制在灵敏度页和 2600 至 3cff 的按键区。每次连接都会先保存完整备份。"),
                tr("即时 DPI 和回报率使用已验证的命令 0d。不发送传感器传输、引导程序覆盖或授权写入命令。"),
                tr("桌面设置：双击速度、滚轮行数、指针速度。应用关联支持 KDE Wayland 和 X11。手势支持文件编辑、全局触发和主机宏。A90 不支持 RGB 灯效。"),
                tr("研究记录见 docs/research。"),
            ] {
                ui.label(text);
            }
            ui.add_space(8.0);
            ui.hyperlink_to(tr("egui 文档"), "https://docs.rs/egui/0.37.0/egui/");
        });
    }
}

/// One 690 × 100 option tile of AboutSoftware.
fn tile(
    ui: &mut egui::Ui,
    width: f32,
    icon: skin::Icon,
    caption: &str,
    control: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::new()
        .fill(skin::PANEL)
        .inner_margin(egui::Margin::symmetric(32, 15))
        .show(ui, |ui| {
            ui.set_width(width - 64.0);
            ui.set_height(70.0);
            ui.horizontal_centered(|ui| {
                skin::icon(ui, icon);
                ui.add_space(14.0);
                ui.label(RichText::new(caption).font(skin::regular(17.0)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(10.0);
                    control(ui);
                });
            });
        });
}

/// XDG autostart entry that starts this program at login.
fn autostart_file() -> Option<std::path::PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".config"))
        })
        .map(|dir| dir.join("autostart").join("bloody-a90.desktop"))
}

fn set_autostart(on: bool) -> Result<String, String> {
    let path = autostart_file().ok_or(tr("找不到配置目录"))?;
    if !on {
        return match std::fs::remove_file(&path) {
            Ok(()) => Ok(tr("已关闭开机自动启动").into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(tr("已关闭开机自动启动").into())
            }
            Err(error) => Err(tf!("无法删除 {}：{error}", path.display())),
        };
    }
    let exe = std::env::current_exe().map_err(|error| tf!("找不到程序路径：{error}"))?;
    let text = format!(
        "[Desktop Entry]\nType=Application\nName=Bloody A90\nExec=\"{}\"\nX-GNOME-Autostart-enabled=true\n",
        exe.display()
    );
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|error| tf!("无法创建 {}：{error}", dir.display()))?;
    }
    std::fs::write(&path, text).map_err(|error| tf!("无法写入 {}：{error}", path.display()))?;
    Ok(tf!("已写入 {}", path.display()))
}
