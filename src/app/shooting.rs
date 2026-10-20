use super::App;
use crate::{skin, worker::Job};
use bloody_a90::{
    configuration::{self, Action, Binding},
    shooting::{self, Cycle, Shot},
    storage::{self, END},
};
use eframe::egui::{self, RichText};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    A,
    B,
    C,
    D,
    Macro,
}

struct Draft {
    kind: Kind,
    shots: [Shot; 8],
    count: usize,
    loop_start: usize,
    recover: bool,
    special: bool,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            kind: Kind::A,
            shots: [Shot {
                interval_ms: 100,
                x: 0,
                y: 0,
            }; 8],
            count: 2,
            loop_start: 1,
            recover: false,
            special: false,
        }
    }
}

pub(super) struct Editor {
    drafts: [Draft; 7],
    slot: usize,
    sniper: usize,
    trigger: usize,
    all_profiles: bool,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            drafts: std::array::from_fn(|_| Draft::default()),
            slot: 1,
            sniper: 5,
            trigger: 9,
            all_profiles: true,
        }
    }
}

impl Editor {
    pub(super) fn state(&self) -> String {
        self.drafts
            .iter()
            .map(|draft| {
                let kind = match draft.kind {
                    Kind::A => 0,
                    Kind::B => 1,
                    Kind::C => 2,
                    Kind::D => 3,
                    Kind::Macro => 4,
                };
                let mut values = vec![
                    kind,
                    draft.count as i32,
                    draft.loop_start as i32,
                    i32::from(draft.recover),
                    i32::from(draft.special),
                ];
                for shot in draft.shots {
                    values.extend([shot.interval_ms as i32, shot.x, shot.y]);
                }
                values
                    .iter()
                    .map(i32::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .collect::<Vec<_>>()
            .join("|")
    }

    pub(super) fn load(&mut self, text: &str) {
        for (draft, entry) in self.drafts.iter_mut().zip(text.split('|')) {
            let Some(values) = entry
                .split(',')
                .map(|v| v.parse::<i32>().ok())
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            if values.len() != 29
                || !(0..=4).contains(&values[0])
                || !(1..=8).contains(&values[1])
                || !(1..=values[1]).contains(&values[2])
                || !(0..=1).contains(&values[3])
                || !(0..=1).contains(&values[4])
                || values[5..].as_chunks::<3>().0.iter().any(|s| {
                    !(1..=999).contains(&s[0])
                        || !(-126..=126).contains(&s[1])
                        || !(-126..=126).contains(&s[2])
                })
            {
                continue;
            }
            draft.kind = match values[0] {
                0 => Kind::A,
                1 => Kind::B,
                2 => Kind::C,
                3 => Kind::D,
                _ => Kind::Macro,
            };
            draft.count = values[1] as usize;
            draft.loop_start = values[2] as usize;
            draft.recover = values[3] != 0;
            draft.special = values[4] != 0;
            for (shot, values) in draft.shots.iter_mut().zip(values[5..].as_chunks::<3>().0) {
                *shot = Shot {
                    interval_ms: values[0] as u32,
                    x: values[1],
                    y: values[2],
                };
            }
        }
    }
}

impl App {
    /// WeaponProperty (左键编程) and ShootMode (狙击键): ① slots and the
    /// switch key, ② the shooting type, ③ the shots of the program.
    pub(super) fn shooting_page(&mut self, ui: &mut egui::Ui, sniper: bool) {
        let titles = if sniper {
            ["狙击设置", "狙击类别", "编程区"]
        } else {
            ["左键编程", "编程文件", "编程区"]
        };
        let column = egui::Color32::from_rgb(29, 30, 32);
        let mut assign = false;
        ui.add_space(-24.0);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            ui.add_space(-20.0);
            // ① 304 px: program slots and the switch key.
            egui::Frame::new()
                .fill(column)
                .inner_margin(egui::Margin {
                    left: 13,
                    right: 13,
                    top: 13,
                    bottom: 20,
                })
                .show(ui, |ui| {
                    ui.set_width(278.0);
                    ui.set_min_height(560.0);
                    ui.vertical(|ui| {
                        skin::step(ui, "①", titles[0]);
                        ui.add_space(40.0);
                        ui.label(
                            RichText::new("程序槽位")
                                .font(skin::regular(16.0))
                                .color(skin::DISABLED),
                        );
                        let editor = &mut self.shooting;
                        let slot = if sniper {
                            &mut editor.sniper
                        } else {
                            &mut editor.slot
                        };
                        for index in if sniper { 5..=7 } else { 1..=4 } {
                            let caption = if sniper {
                                format!("狙击键 {}", index - 4)
                            } else {
                                format!("程序槽位 {index}")
                            };
                            if skin::list_item(ui, &caption, *slot == index, 273.0).clicked() {
                                *slot = index;
                            }
                        }
                        ui.add_space(30.0);
                        ui.label(
                            RichText::new("切换按键")
                                .font(skin::regular(16.0))
                                .color(skin::DISABLED),
                        );
                        egui::ComboBox::from_id_salt("shooting-trigger")
                            .selected_text(trigger_name(editor.trigger))
                            .width(260.0)
                            .show_ui(ui, |ui| {
                                for button in [9, 10, 11, 3, 4, 5] {
                                    ui.selectable_value(
                                        &mut editor.trigger,
                                        button,
                                        trigger_name(button),
                                    );
                                }
                            });
                        skin::check(ui, &mut editor.all_profiles, "全部配置");
                        assign = skin::button(ui, "指定切换按键", 273.0, self.keys.is_some())
                            .on_hover_text("把切换按键设为执行此槽位，点击“应用”后写入")
                            .clicked();
                    });
                });
            let slot = if sniper {
                self.shooting.sniper
            } else {
                self.shooting.slot
            };
            let draft = &mut self.shooting.drafts[slot - 1];
            // ② 205 px: shooting type.
            egui::Frame::new()
                .fill(column)
                .inner_margin(egui::Margin {
                    left: 12,
                    right: 12,
                    top: 13,
                    bottom: 20,
                })
                .show(ui, |ui| {
                    ui.set_width(181.0);
                    ui.set_min_height(560.0);
                    ui.vertical(|ui| {
                        skin::step(ui, "②", titles[1]);
                        ui.add_space(46.0);
                        ui.spacing_mut().item_spacing.y = 20.0;
                        for (kind, name) in [
                            (Kind::A, "A-多连"),
                            (Kind::B, "B-循环"),
                            (Kind::C, "C-机枪"),
                            (Kind::D, "D-临阵"),
                            (Kind::Macro, "M-宏枪"),
                        ] {
                            if mode(ui, name, draft.kind == kind).clicked() {
                                draft.kind = kind;
                            }
                        }
                    });
                });
            // ③ the rest: the shots.
            egui::Frame::new()
                .fill(column)
                .inner_margin(egui::Margin {
                    left: 20,
                    right: 20,
                    top: 13,
                    bottom: 20,
                })
                .show(ui, |ui| {
                    ui.set_width(ui.available_width() - 40.0);
                    ui.set_min_height(560.0);
                    ui.vertical(|ui| {
                        skin::step(ui, "③", titles[2]);
                        ui.add_space(14.0);
                        shots(ui, draft);
                    });
                });
        });
        let editor = &mut self.shooting;
        let slot = if sniper { editor.sniper } else { editor.slot };
        let draft = &mut editor.drafts[slot - 1];
        let program = match draft.kind {
            Kind::Macro => self
                .compiled
                .as_ref()
                .map(|p| shooting::Program {
                    press: p.press.clone(),
                    release: p.release.clone(),
                })
                .map_err(Clone::clone),
            Kind::A => shooting::burst(&draft.shots[..draft.count], draft.recover, draft.special)
                .map(|press| shooting::Program {
                    press,
                    release: vec![END],
                }),
            kind => shooting::cycle(
                match kind {
                    Kind::B => Cycle::B,
                    Kind::C => Cycle::C,
                    _ => Cycle::D,
                },
                &draft.shots[..draft.count],
                draft.loop_start.min(draft.count) - 1,
                draft.recover,
                draft.special,
            ),
        };
        ui.add_space(12.0);
        let vertical = (draft.kind == Kind::D).then_some(draft.shots[0]);
        let binding = program.map(|p| Binding {
            press: Action::from_program(p.press),
            release: Action::from_program(p.release),
        });
        match &binding {
            Ok(binding) => {
                ui.collapsing("查看编译程序", |ui| {
                    for (caption, action) in [("按下", &binding.press), ("弹起", &binding.release)]
                    {
                        ui.label(caption);
                        match action {
                            Action::Direct(word) => {
                                ui.monospace(storage::describe(*word));
                            }
                            Action::Macro(words) => {
                                for instruction in storage::list(words).instructions {
                                    ui.monospace(format!(
                                        "{:04}  {}",
                                        instruction.offset, instruction.text
                                    ));
                                }
                            }
                        }
                    }
                });
            }
            Err(error) => {
                ui.colored_label(skin::ERROR, error);
            }
        }
        if assign && let Some(config) = &mut self.keys {
            for (i, profile) in config.profiles.iter_mut().enumerate() {
                if editor.all_profiles || i == self.profile {
                    profile.buttons[editor.trigger - 1] = Binding {
                        press: Action::Direct(0x2610 + slot as u16),
                        release: Action::Direct(storage::NO_OPERATION),
                    };
                }
            }
            self.keys_dirty = true;
        }
        let route = self
            .device
            .as_ref()
            .map(|device| &device.backup)
            .or(self.source.as_ref())
            .map_or(bloody_a90::backup::ShootingRoute::Unknown, |backup| {
                backup.shooting_route()
            });
        ui.colored_label(skin::ERROR, route.description());
        let config = binding.and_then(|binding| {
            let mut config = self.keys.clone().ok_or("请先连接鼠标或读取备份")?;
            config.set_shooting(slot, binding)?;
            config.set_shooting_parameters(slot, vertical)?;
            configuration::build(&config)?;
            Ok(config)
        });
        let clicked = skin::footer(
            ui,
            self.footer_area,
            &[
                ("载入按键草稿", config.is_ok()),
                (
                    "应用",
                    config.is_ok() && self.device.is_some() && !self.pending,
                ),
            ],
        );
        if clicked[0]
            && let Ok(config) = &config
        {
            self.keys = Some(config.clone());
            self.keys_dirty = true;
        }
        if clicked[1]
            && let Ok(config) = &config
        {
            self.start(Job::SaveButtons(Box::new(config.clone())));
        }
        if let Err(error) = config {
            ui.colored_label(skin::ERROR, error);
        }
    }
}

/// rb_AMode…rb_MMode: a 177 × 40 type button, red when chosen.
fn mode(ui: &mut egui::Ui, caption: &str, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(177.0, 40.0), egui::Sense::click());
    let fill = if selected {
        skin::RED
    } else if response.hovered() {
        egui::Color32::from_rgb(60, 61, 66)
    } else {
        egui::Color32::from_rgb(44, 45, 49)
    };
    ui.painter().rect_filled(rect, 0, fill);
    let color = if selected || response.hovered() {
        skin::TEXT
    } else {
        egui::Color32::from_rgb(138, 138, 138)
    };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        caption,
        skin::bold(19.0),
        color,
    );
    response
}

/// ③ 编程区 of the Weapon form: count, recentering and cycle on top, then
/// one column per shot with interval and offset.
fn shots(ui: &mut egui::Ui, draft: &mut Draft) {
    let gray = |text: &str| {
        RichText::new(text)
            .font(skin::regular(16.0))
            .color(skin::LIGHT_GRAY)
    };
    if draft.kind == Kind::Macro {
        ui.label(gray("M-宏枪：使用“奥斯卡II”页当前编译的按下和弹起程序。"));
        statement(ui, draft.kind);
        return;
    }
    let limit = match draft.kind {
        Kind::A => 5,
        Kind::D => 1,
        _ => 8,
    };
    draft.count = draft.count.clamp(1, limit);
    draft.loop_start = draft.loop_start.clamp(1, draft.count);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(gray("设定弹数"));
        ui.add(egui::DragValue::new(&mut draft.count).range(1..=limit));
        ui.add_space(14.0);
        ui.label(gray("准心回位"));
        skin::check(ui, &mut draft.recover, "");
        if matches!(draft.kind, Kind::B | Kind::C) {
            ui.add_space(14.0);
            ui.label(gray("次循环 从"));
            ui.add(egui::DragValue::new(&mut draft.loop_start).range(1..=draft.count));
            ui.label(gray(&format!("到 {} 发", draft.count)));
        }
        ui.add_space(14.0);
        skin::check(ui, &mut draft.special, "等待左键状态");
    });
    ui.add_space(18.0);
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(40, 41, 45))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::Grid::new("shooting-shots")
                .spacing(egui::vec2(12.0, 14.0))
                .min_col_width(60.0)
                .show(ui, |ui| {
                    ui.label(gray(""));
                    for i in 0..draft.count {
                        ui.label(RichText::new(format!("第 {} 发", i + 1)).font(skin::bold(15.0)));
                    }
                    ui.end_row();
                    ui.label(gray("连击间歇\n(毫秒)"));
                    for shot in &mut draft.shots[..draft.count] {
                        ui.add(egui::DragValue::new(&mut shot.interval_ms).range(1..=999));
                    }
                    ui.end_row();
                    let vertical = draft.kind == Kind::D;
                    ui.label(gray(if vertical {
                        "修正Y轴  Y="
                    } else {
                        "修正(X,Y)  Y="
                    }));
                    let count = draft.count;
                    for (i, shot) in draft.shots[..count].iter_mut().enumerate() {
                        let movement = draft.kind != Kind::A || i + 1 < count;
                        ui.add_enabled(
                            movement,
                            egui::DragValue::new(&mut shot.y).range(-126..=126),
                        );
                    }
                    ui.end_row();
                    ui.label(gray("偏移量  X="));
                    for (i, shot) in draft.shots[..count].iter_mut().enumerate() {
                        let movement = draft.kind != Kind::A || i + 1 < count;
                        ui.add_enabled(
                            movement && !vertical,
                            egui::DragValue::new(&mut shot.x).range(-126..=126),
                        );
                    }
                    ui.end_row();
                });
        });
    statement(ui, draft.kind);
}

/// htmlb_ModeStatement: what the chosen type is for.
fn statement(ui: &mut egui::Ui, kind: Kind) {
    let text = match kind {
        Kind::A => "A-多连：限使用于 [手枪与连狙及机枪类] 的多连发压枪射击。",
        Kind::B => "B-循环：限使用于 [手枪与连狙及机枪类] 的循环压枪射击。",
        Kind::C => "C-机枪：限使用于 [连狙及机枪类] 的机枪压枪射击。",
        Kind::D => "D-临阵：限使用于 [手枪与连狙及机枪类] 临阵可调的循环压枪射击。",
        Kind::Macro => "M-宏枪：限转换 [奥斯卡宏指令] 角色动作与射击合并于左键。",
    };
    ui.add_space(20.0);
    ui.label(
        RichText::new("说明>>")
            .font(skin::regular(15.0))
            .color(skin::CAPTION_RED),
    );
    ui.label(
        RichText::new(text)
            .font(skin::regular(15.0))
            .color(egui::Color32::from_rgb(138, 138, 138)),
    );
    ui.label(
        RichText::new("X 向右为正，Y 向下为正，位移使用鼠标计数值。")
            .font(skin::regular(13.0))
            .color(egui::Color32::from_rgb(138, 138, 138)),
    );
}

fn trigger_name(button: usize) -> &'static str {
    match button {
        9 => "“1”键",
        10 => "“C”键",
        11 => "“3”键",
        3 => "中键",
        4 => "后侧键",
        _ => "前侧键",
    }
}
