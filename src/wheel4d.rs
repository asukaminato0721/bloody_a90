//! Evolutionary Wheel: the right part of the pointer window scrolls horizontally.
//! The EXE's 013df608 uses a strict divider boundary and a default ratio of 0.9.
mod direction;
mod divider;
mod kwin;

use crate::linux_input::{self as input, Event};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// The EXE saves one switch for each kernel1..kernel4 mode.
    pub cores: [bool; 4],
    /// Width of the left region, in thousandths of the window width.
    pub divider: u16,
    pub reverse_regions: bool,
    pub applications: Vec<Application>,
    pub direction_icon: bool,
    pub divider_icon: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Application {
    pub app: String,
    pub divider: u16,
}

/// A runtime configuration selected for one confirmed device core.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Active {
    pub(crate) settings: Settings,
    core: u8,
}

impl Active {
    pub fn core(&self) -> u8 {
        self.core
    }
    pub fn settings(&self) -> &Settings {
        &self.settings
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            cores: [false; 4],
            divider: 900,
            reverse_regions: false,
            applications: Vec::new(),
            direction_icon: true,
            divider_icon: true,
        }
    }
}

impl Settings {
    pub fn enabled_for(&self, core: u8) -> bool {
        core.checked_sub(1)
            .and_then(|index| self.cores.get(index as usize))
            .copied()
            .unwrap_or(false)
    }

    pub fn active_for(&self, core: u8) -> Option<Active> {
        self.enabled_for(core).then(|| Active {
            settings: self.clone(),
            core,
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        if !(100..=900).contains(&self.divider) {
            return Err("滚轮分界位置必须在 10% 到 90% 之间".into());
        }
        if self.applications.len() > 128 {
            return Err("滚轮应用规则最多为 128 条".into());
        }
        for (index, rule) in self.applications.iter().enumerate() {
            if rule.app.trim().is_empty()
                || rule.app.len() > 512
                || rule.app.chars().any(char::is_control)
            {
                return Err("滚轮应用标识不能为空，不能包含控制字符，且最多为 512 字节".into());
            }
            if !(100..=900).contains(&rule.divider) {
                return Err("应用滚轮分界必须在 10% 到 90% 之间".into());
            }
            if self.applications[..index]
                .iter()
                .any(|r| r.app.trim().eq_ignore_ascii_case(rule.app.trim()))
            {
                return Err(format!("滚轮应用标识重复：{}", rule.app.trim()));
            }
        }
        Ok(())
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 256 * 1024 {
            return Err("滚轮设置文件过大".into());
        }
        let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
        let document = roxmltree::Document::parse(text).map_err(|e| e.to_string())?;
        let root = document.root_element();
        let version = root.attribute("version");
        if root.tag_name().name() != "A90Wheel" || !matches!(version, Some("1" | "2")) {
            return Err("滚轮设置文件版本无效".into());
        }
        if root.attributes().any(|a| {
            !matches!(
                a.name(),
                "version" | "divider" | "reverse" | "directionIcon" | "dividerIcon"
            ) && !(version == Some("1") && a.name() == "enabled")
                && !(version == Some("2")
                    && matches!(a.name(), "kernel1" | "kernel2" | "kernel3" | "kernel4"))
        }) {
            return Err("滚轮设置文件包含未知字段".into());
        }
        let boolean = |name| match root.attribute(name) {
            Some("0") => Ok(false),
            Some("1") => Ok(true),
            _ => Err(format!("滚轮设置 {name} 无效")),
        };
        let result = Self {
            cores: if version == Some("1") {
                [boolean("enabled")?; 4]
            } else {
                [
                    boolean("kernel1")?,
                    boolean("kernel2")?,
                    boolean("kernel3")?,
                    boolean("kernel4")?,
                ]
            },
            direction_icon: if root.attribute("directionIcon").is_some() {
                boolean("directionIcon")?
            } else {
                true
            },
            divider: root
                .attribute("divider")
                .ok_or("缺少滚轮分界位置")?
                .parse::<u16>()
                .map_err(|e| e.to_string())?,
            divider_icon: if root.attribute("dividerIcon").is_some() {
                boolean("dividerIcon")?
            } else {
                true
            },
            reverse_regions: boolean("reverse")?,
            applications: root
                .children()
                .filter(|n| n.is_element())
                .map(|node| {
                    if node.tag_name().name() != "Application"
                        || node
                            .attributes()
                            .any(|a| !matches!(a.name(), "app" | "divider"))
                        || node.children().any(|n| n.is_element())
                    {
                        return Err("滚轮设置文件包含未知应用字段".into());
                    }
                    Ok(Application {
                        app: node.attribute("app").ok_or("缺少滚轮应用标识")?.into(),
                        divider: node
                            .attribute("divider")
                            .ok_or("缺少应用滚轮分界")?
                            .parse::<u16>()
                            .map_err(|e| e.to_string())?,
                    })
                })
                .collect::<Result<_, String>>()?,
        };
        result.validate()?;
        Ok(result)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut text = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<A90Wheel version=\"2\" kernel1=\"{}\" kernel2=\"{}\" kernel3=\"{}\" kernel4=\"{}\" divider=\"{}\" reverse=\"{}\" directionIcon=\"{}\" dividerIcon=\"{}\">\n",
            u8::from(self.cores[0]),
            u8::from(self.cores[1]),
            u8::from(self.cores[2]),
            u8::from(self.cores[3]),
            self.divider,
            u8::from(self.reverse_regions),
            u8::from(self.direction_icon),
            u8::from(self.divider_icon)
        );
        for rule in &self.applications {
            let app = rule
                .app
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
                .replace('\'', "&apos;");
            text.push_str(&format!(
                "  <Application app=\"{app}\" divider=\"{}\"/>\n",
                rule.divider
            ));
        }
        text.push_str("</A90Wheel>\n");
        text.into_bytes()
    }

    pub fn divider_for(&self, app: &str) -> u16 {
        self.applications
            .iter()
            .find(|r| r.app.trim().eq_ignore_ascii_case(app))
            .map_or(self.divider, |r| r.divider)
    }

    #[cfg(test)]
    pub(crate) fn horizontal(&self, point: [f64; 2], rect: [f64; 4], app: &str) -> bool {
        self.horizontal_at(point, rect, self.divider_for(app))
    }

    pub(crate) fn horizontal_at(&self, point: [f64; 2], rect: [f64; 4], divider: u16) -> bool {
        let [x, y] = point;
        let [left, top, width, height] = rect;
        if !point.into_iter().chain(rect).all(f64::is_finite)
            || width <= 0.0
            || height <= 0.0
            || x < left
            || x >= left + width
            || y < top
            || y >= top + height
        {
            return false;
        }
        // Win32 GetWindowRect uses integer coordinates. KWin uses logical
        // fractional coordinates, so keep those coordinates at mixed DPI.
        let right = x > left + (width * f64::from(divider) / 1000.0).trunc();
        right != self.reverse_regions
    }
}

pub fn path() -> Option<PathBuf> {
    crate::host_buttons::path().map(|p| p.with_file_name("wheel.xml"))
}

pub fn load(path: &Path) -> Result<(Settings, Option<Vec<u8>>), String> {
    match fs::read(path) {
        Ok(bytes) => Ok((Settings::parse(&bytes)?, Some(bytes))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok((Settings::default(), None)),
        Err(e) => Err(format!("无法读取滚轮设置：{e}")),
    }
}

pub fn save(path: &Path, settings: &Settings, expected: Option<&[u8]>) -> Result<(), String> {
    settings.validate()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    crate::gesture_file::save_checked(path, &settings.to_bytes(), expected)
}

pub(crate) use divider::Observer;
pub(crate) use kwin::Monitor;

/// Keep both wheel representations on the same axis for a whole evdev frame.
#[derive(Default)]
pub(crate) struct Route {
    horizontal: Option<bool>,
}

impl Route {
    pub(crate) fn event(&mut self, event: Event, horizontal: bool) -> Event {
        if event.kind == input::SYN && event.code == input::REPORT {
            self.horizontal = None;
        }
        if event.kind == input::REL
            && matches!(event.code, input::WHEEL | 11)
            && *self.horizontal.get_or_insert(horizontal)
        {
            Event::new(
                input::REL,
                if event.code == 11 { 12 } else { input::HWHEEL },
                event.value.saturating_neg(),
            )
        } else {
            event
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_switches_are_independent_and_legacy_global_switch_migrates() {
        let settings = Settings {
            cores: [false, true, false, true],
            ..Default::default()
        };
        for core in 0..=5 {
            let expected = matches!(core, 2 | 4);
            assert_eq!(settings.enabled_for(core), expected);
            assert_eq!(settings.active_for(core).is_some(), expected);
            if let Some(active) = settings.active_for(core) {
                assert_eq!(active.core(), core);
            }
        }
        assert_eq!(Settings::parse(&settings.to_bytes()).unwrap(), settings);
        let legacy = b"<A90Wheel version='1' enabled='1' divider='900' reverse='0'/>";
        assert_eq!(Settings::parse(legacy).unwrap().cores, [true; 4]);
        let mixed = b"<A90Wheel version='2' kernel1='0' kernel2='1' kernel3='0' kernel4='yes' divider='900' reverse='0'/>";
        assert!(Settings::parse(mixed).is_err());
    }

    #[test]
    fn original_regions_boundaries_negative_outputs_and_reverse() {
        let mut settings = Settings::default();
        for (point, expected) in [
            ([100.0, 50.0], false),
            ([280.0, 50.0], false),
            ([281.0, 50.0], true),
            ([300.0, 50.0], false),
            ([281.0, 100.0], false),
            ([281.0, -1.0], false),
        ] {
            assert_eq!(
                settings.horizontal(point, [100.0, 0.0, 200.0, 100.0], ""),
                expected
            );
        }
        assert!(settings.horizontal([-119.0, -50.0], [-300.0, -100.0, 200.0, 100.0], ""));
        settings.reverse_regions = true;
        assert!(settings.horizontal([280.0, 50.0], [100.0, 0.0, 200.0, 100.0], ""));
        assert!(!settings.horizontal([281.0, 50.0], [100.0, 0.0, 200.0, 100.0], ""));
        assert!(!settings.horizontal([301.0, 50.0], [100.0, 0.0, 200.0, 100.0], ""));
        assert!(!settings.horizontal([f64::NAN, 50.0], [100.0, 0.0, 200.0, 100.0], ""));
    }

    #[test]
    fn application_rules_change_only_matching_windows_and_round_trip_literals() {
        let mut settings = Settings {
            applications: vec![
                Application {
                    app: " org.kde.kwrite ".into(),
                    divider: 200,
                },
                Application {
                    app: "特殊<&\"'应用>".into(),
                    divider: 500,
                },
            ],
            ..Default::default()
        };
        settings.validate().unwrap();
        assert_eq!(Settings::parse(&settings.to_bytes()).unwrap(), settings);
        assert_eq!(settings.divider_for("ORG.KDE.KWRITE"), 200);
        assert_eq!(settings.divider_for("org.kde.konsole"), 900);
        assert!(settings.horizontal([150.0, 50.0], [100.0, 0.0, 200.0, 100.0], "org.kde.kwrite"));
        assert!(!settings.horizontal([150.0, 50.0], [100.0, 0.0, 200.0, 100.0], "org.kde.konsole"));
        settings.applications.push(Application {
            app: "ORG.KDE.KWRITE".into(),
            divider: 600,
        });
        assert!(settings.validate().is_err());
        assert!(Settings::parse(&settings.to_bytes()).is_err());
        let legacy = b"<A90Wheel version='1' enabled='0' divider='900' reverse='0'/>";
        assert_eq!(Settings::parse(legacy).unwrap(), Settings::default());
    }

    #[test]
    fn frame_routes_coarse_and_fine_once_and_preserves_other_input() {
        let mut route = Route::default();
        for (code, value, horizontal, expected_code, expected_value) in [
            (11, 120, true, 12, -120),
            (input::WHEEL, 1, false, input::HWHEEL, -1),
            (input::X, 7, false, input::X, 7),
            (input::HWHEEL, 1, true, input::HWHEEL, 1),
        ] {
            let result = route.event(Event::new(input::REL, code, value), horizontal);
            assert_eq!((result.code, result.value), (expected_code, expected_value));
        }
        route.event(Event::new(input::SYN, input::REPORT, 0), true);
        let result = route.event(Event::new(input::REL, input::WHEEL, -1), false);
        assert_eq!((result.code, result.value), (input::WHEEL, -1));
    }

    #[test]
    fn persistence_rejects_invalid_and_stale_settings() {
        let settings = Settings {
            cores: [true, false, true, false],
            divider: 723,
            reverse_regions: true,
            applications: Vec::new(),
            direction_icon: false,
            divider_icon: false,
        };
        assert_eq!(Settings::parse(&settings.to_bytes()).unwrap(), settings);
        for invalid in [
            b"<A90Wheel version='2'/>".as_slice(),
            b"<A90Wheel version='1' enabled='yes' divider='900' reverse='0'/>",
            b"<A90Wheel version='1' enabled='1' divider='0' reverse='0'/>",
        ] {
            assert!(Settings::parse(invalid).is_err());
        }
        let path =
            std::env::temp_dir().join(format!("a90-wheel-settings-{}.xml", std::process::id()));
        save(&path, &settings, None).unwrap();
        assert!(save(&path, &Settings::default(), Some(b"stale")).is_err());
        assert_eq!(load(&path).unwrap().0, settings);
        fs::remove_file(path).unwrap();
    }
}
