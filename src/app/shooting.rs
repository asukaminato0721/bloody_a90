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
    pub(super) fn shooting_page(&mut self, ui: &mut egui::Ui, sniper: bool) {
        let editor = &mut self.shooting;
        let slot = if sniper {
            &mut editor.sniper
        } else {
            &mut editor.slot
        };
        ui.horizontal(|ui| {
            ui.label("程序槽位：");
            for index in if sniper { 5..=7 } else { 1..=4 } {
                if skin::tool(ui, &index.to_string(), *slot == index).clicked() {
                    *slot = index;
                }
            }
        });
        let slot = *slot;
        ui.label("以下参数是本地草稿。连接鼠标不会覆盖此草稿。");
        if let Some(config) = &self.keys {
            ui.collapsing("查看已载入的槽位程序", |ui| {
                for (caption, action) in [
                    ("按下", &config.auxiliary[slot]),
                    ("弹起", &config.auxiliary[slot + 8]),
                ] {
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
        let draft = &mut editor.drafts[slot - 1];
        skin::sheet().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("射击程序").font(skin::bold(17.0)));
            ui.horizontal_wrapped(|ui| {
                for (kind, name) in [
                    (Kind::A, "A 连击"),
                    (Kind::B, "B 循环点击"),
                    (Kind::C, "C 持续按下"),
                    (Kind::D, "D 垂直循环"),
                    (Kind::Macro, "奥斯卡宏"),
                ] {
                    if skin::tool(ui, name, draft.kind == kind).clicked() {
                        draft.kind = kind;
                    }
                }
            });
            if draft.kind == Kind::Macro {
                ui.label("使用“奥斯卡II”页当前编译的按下和弹起程序。");
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
                ui.label("射击段数：");
                ui.add(egui::DragValue::new(&mut draft.count).range(1..=limit));
                if matches!(draft.kind, Kind::B | Kind::C) {
                    draft.loop_start = draft.loop_start.clamp(1, draft.count);
                    ui.label("循环起点：");
                    ui.add(egui::DragValue::new(&mut draft.loop_start).range(1..=draft.count));
                }
            });
            egui::Grid::new("shooting-shots")
                .striped(true)
                .show(ui, |ui| {
                    for caption in ["段", "间隔（ms）", "X 位移", "Y 位移"] {
                        ui.strong(caption);
                    }
                    ui.end_row();
                    for (i, shot) in draft.shots[..draft.count].iter_mut().enumerate() {
                        ui.label((i + 1).to_string());
                        ui.add(egui::DragValue::new(&mut shot.interval_ms).range(1..=999));
                        let movement = draft.kind != Kind::A || i + 1 < draft.count;
                        ui.add_enabled(
                            movement && draft.kind != Kind::D,
                            egui::DragValue::new(&mut shot.x).range(-126..=126),
                        );
                        ui.add_enabled(
                            movement,
                            egui::DragValue::new(&mut shot.y).range(-126..=126),
                        );
                        ui.end_row();
                    }
                });
            ui.label("X 向右为正。Y 向下为正。位移使用鼠标计数值。");
            ui.horizontal(|ui| {
                skin::check(ui, &mut draft.recover, "结束时恢复位移");
                skin::check(ui, &mut draft.special, "等待左键状态");
            });
        });
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
        let mut assign = false;
        ui.horizontal(|ui| {
            ui.label("切换按键：");
            egui::ComboBox::from_id_salt("shooting-trigger")
                .selected_text(trigger_name(editor.trigger))
                .show_ui(ui, |ui| {
                    for button in [9, 10, 11, 3, 4, 5] {
                        ui.selectable_value(&mut editor.trigger, button, trigger_name(button));
                    }
                });
            skin::check(ui, &mut editor.all_profiles, "全部配置");
            assign = skin::button(ui, "指定切换按键", 150.0, self.keys.is_some()).clicked();
        });
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
        ui.horizontal(|ui| {
            if skin::button(ui, "载入按键草稿", 160.0, config.is_ok()).clicked()
                && let Ok(config) = &config
            {
                self.keys = Some(config.clone());
                self.keys_dirty = true;
            }
            if skin::button(
                ui,
                "应用",
                140.0,
                config.is_ok() && self.device.is_some() && !self.pending,
            )
            .clicked()
                && let Ok(config) = &config
            {
                self.start(Job::SaveButtons(Box::new(config.clone())));
            }
        });
        if let Err(error) = config {
            ui.colored_label(skin::ERROR, error);
        }
    }
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
