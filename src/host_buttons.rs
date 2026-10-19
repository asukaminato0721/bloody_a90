//! Desktop actions attached to physical buttons and wheel directions.
//! The matching hardware slots must be no-operations. Host actions cannot
//! run unless the saved main-vector header matches the recorded signature.
use crate::{
    configuration::{self, Configuration},
    gestures::{Action, Document, Kind},
    host_program, storage,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const SLOTS: usize = configuration::BUTTONS + 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gestures {
    pub enabled: bool,
    pub documents: [Document; 2],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub enabled: bool,
    pub signature: Option<[u16; 16]>,
    pub profiles: Vec<[Option<Action>; SLOTS]>,
    pub gestures: Option<Box<Gestures>>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            signature: None,
            profiles: Vec::new(),
            gestures: None,
        }
    }
}

fn vacant(config: &Configuration, profile: usize, slot: usize) -> bool {
    use configuration::Action::Direct;
    let noop = Direct(storage::NO_OPERATION);
    let profile = &config.profiles[profile];
    if slot < configuration::BUTTONS {
        profile.buttons[slot].press == noop && profile.buttons[slot].release == noop
    } else {
        profile.wheel[slot - configuration::BUTTONS] == noop
    }
}

impl Settings {
    pub fn assigned_gesture_kinds(&self) -> [bool; 2] {
        let mut kinds = [false; 2];
        for action in self.profiles.iter().flatten().flatten() {
            if action.number("Simulator") == Ok(2)
                && let Ok(kind @ 0..=1) = action.number("Object")
            {
                kinds[kind as usize] = true;
            }
        }
        kinds
    }

    pub fn gesture_kinds(&self) -> [bool; 2] {
        if self.gestures.as_ref().is_some_and(|g| g.enabled) {
            self.assigned_gesture_kinds()
        } else {
            [false; 2]
        }
    }

    pub(crate) fn gesture_slots(&self, profile: u8) -> [Option<usize>; 24] {
        std::array::from_fn(|slot| {
            self.profiles
                .get(profile as usize)
                .and_then(|slots| slots[slot].as_ref())
                .and_then(|action| {
                    (action.number("Simulator") == Ok(2))
                        .then(|| {
                            action
                                .number("Object")
                                .ok()
                                .filter(|kind| *kind < 2)
                                .map(|kind| kind as usize)
                        })
                        .flatten()
                })
        })
    }

    pub fn resize(&mut self, count: usize) {
        self.profiles
            .resize_with(count, || std::array::from_fn(|_| None));
    }

    pub fn is_empty(&self) -> bool {
        self.profiles.iter().flatten().all(Option::is_none)
    }

    /// A hardware edit replaces the host assignment on the same slot.
    pub fn reconcile(&mut self, config: &Configuration) {
        self.resize(config.profiles.len());
        for (profile, slots) in self.profiles.iter_mut().enumerate() {
            for (slot, action) in slots.iter_mut().enumerate() {
                if !vacant(config, profile, slot) {
                    *action = None;
                }
            }
        }
    }

    pub fn bind(&mut self, config: &Configuration) -> Result<(), String> {
        self.validate_hardware(config)?;
        if self.profiles.len() != config.profiles.len() {
            return Err("主机动作的配置数量与鼠标不一致".into());
        }
        for (profile, slots) in self.profiles.iter().enumerate() {
            for (slot, action) in slots.iter().enumerate() {
                if let Some(action) = action {
                    if !vacant(config, profile, slot) {
                        return Err(format!(
                            "配置 {} 的记录 {} 仍有硬件动作",
                            profile + 1,
                            slot + 1
                        ));
                    }
                    if self.enabled && action.number("Simulator")? != 2 {
                        host_program::prepare_button(action, true)?;
                        host_program::prepare_button(action, false)?;
                    }
                }
            }
        }
        self.signature = Some(configuration::build(config)?[..16].try_into().unwrap());
        // Keep the stored representation within the parser's limit.
        if self.to_bytes().len() > 1024 * 1024 {
            return Err("按键主机动作设置超过 1 MiB".into());
        }
        Ok(())
    }

    pub(crate) fn validate_hardware(&self, config: &Configuration) -> Result<(), String> {
        if self.profiles.len() != config.profiles.len() {
            return Err("主机动作的配置数量与鼠标不一致".into());
        }
        self.validate_actions()?;
        for (profile, slots) in self.profiles.iter().enumerate() {
            for (slot, action) in slots.iter().enumerate() {
                if action.is_some() && !vacant(config, profile, slot) {
                    return Err(format!(
                        "配置 {} 的记录 {} 仍有硬件动作",
                        profile + 1,
                        slot + 1
                    ));
                }
            }
        }
        Ok(())
    }

    pub(crate) fn validate_actions(&self) -> Result<(), String> {
        for slots in &self.profiles {
            for (slot, action) in slots.iter().enumerate() {
                if let Some(action) = action {
                    if !matches!(action.number("Simulator")?, 2 | 20..=22 | 24)
                        || action.number("Action")? > 3
                    {
                        return Err("主机动作类型或执行阶段无效".into());
                    }
                    if action.number("Simulator")? == 2
                        && (slot >= 24 || action.number("Object")? > 1 || self.gestures.is_none())
                    {
                        return Err("按键手势需要完整的两套手势定义，且不能指定给滚轮".into());
                    }
                }
            }
        }
        if let Some(gestures) = &self.gestures
            && (gestures.documents[0].kind != Kind::Sixteen
                || gestures.documents[1].kind != Kind::Mouse)
        {
            return Err("按键手势的方向表类型无效".into());
        }
        Ok(())
    }

    pub fn matches(&self, header: &[u16]) -> bool {
        self.signature
            .as_ref()
            .is_some_and(|expected| header == expected)
    }

    /// Check actual direct slot words, including files edited outside the GUI.
    /// A matching header alone does not prove that a newly added host slot has
    /// no hardware output. Every read stays inside the saved configuration area.
    pub(crate) fn verify_device(&self, device: &crate::device::Device) -> Result<(), String> {
        let header = device.read_flash(crate::device::BUTTONS, 16, |_, _| {})?;
        if !self.matches(&header)
            || header[..2] != [0xa4a4, 0xffff]
            || (header[4] >> 8) as usize + 1 != self.profiles.len()
        {
            return Err("按键表与主机动作文件不一致。请重新读取并应用按键设置。".into());
        }
        let read = |offset: u16, count: usize| {
            if offset as usize + count > configuration::WORDS {
                return Err("主机动作的硬件表指针超出配置范围".into());
            }
            device.read_flash(crate::device::BUTTONS + offset as usize, count, |_, _| {})
        };
        let buttons = read(header[7], self.profiles.len())?;
        let wheels = read(header[8], self.profiles.len())?;
        for (index, slots) in self.profiles.iter().enumerate() {
            if slots[..24].iter().any(Option::is_some) {
                let words = read(buttons[index], 48)?;
                for (slot, action) in slots[..24].iter().enumerate() {
                    if action.is_some()
                        && words[slot * 2..slot * 2 + 2] != [storage::NO_OPERATION; 2]
                    {
                        return Err(format!(
                            "配置 {} 的记录 {} 仍有硬件动作",
                            index + 1,
                            slot + 1
                        ));
                    }
                }
            }
            if slots[24..].iter().any(Option::is_some) {
                let words = read(wheels[index], 2)?;
                for (slot, action) in slots[24..].iter().enumerate() {
                    if action.is_some() && words[slot] != storage::NO_OPERATION {
                        return Err(format!("配置 {} 的滚轮仍有硬件动作", index + 1));
                    }
                }
            }
        }
        Ok(())
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut xml = format!(
            "<HostButtons version=\"1\" enabled=\"{}\">",
            u8::from(self.enabled)
        );
        if let Some(signature) = self.signature {
            xml.push_str("<Signature>");
            xml.push_str(
                &signature
                    .iter()
                    .map(|word| format!("{word:04x}"))
                    .collect::<Vec<_>>()
                    .join(","),
            );
            xml.push_str("</Signature>");
        }
        for slots in &self.profiles {
            xml.push_str("<Profile>");
            for (slot, action) in slots.iter().enumerate() {
                if let Some(action) = action {
                    xml.push_str(&format!("<Slot index=\"{slot}\"><Action>"));
                    action.write_fields(&mut xml);
                    xml.push_str("</Action></Slot>");
                }
            }
            xml.push_str("</Profile>");
        }
        if let Some(gestures) = &self.gestures {
            xml.push_str(&format!(
                "<Gestures enabled=\"{}\">",
                u8::from(gestures.enabled)
            ));
            for (tag, document) in ["Sixteen", "Mouse"].into_iter().zip(&gestures.documents) {
                let bytes = document.to_bytes();
                let text = std::str::from_utf8(&bytes)
                    .unwrap()
                    .trim_start_matches('\u{feff}');
                xml.push_str(&format!("<{tag}>{text}</{tag}>"));
            }
            xml.push_str("</Gestures>");
        }
        xml.push_str("</HostButtons>");
        xml.into_bytes()
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 1024 * 1024 {
            return Err("按键主机动作设置超过 1 MiB".into());
        }
        let text = std::str::from_utf8(bytes).map_err(|_| "主机动作文件需要 UTF-8")?;
        let doc = roxmltree::Document::parse_with_options(
            text,
            roxmltree::ParsingOptions {
                nodes_limit: 16384,
                ..Default::default()
            },
        )
        .map_err(|e| format!("主机动作 XML 无效：{e}"))?;
        if doc
            .descendants()
            .filter(|n| n.is_element())
            .any(|n| n.tag_name().namespace().is_some())
        {
            return Err("主机动作不支持 XML 命名空间".into());
        }
        let root = doc.root_element();
        if root.tag_name().name() != "HostButtons"
            || root.attribute("version") != Some("1")
            || root.attributes().len() != 2
        {
            return Err("主机动作文件头无效".into());
        }
        let mut settings = Self {
            enabled: match root.attribute("enabled") {
                Some("0") => false,
                Some("1") => true,
                _ => return Err("主机动作开关无效".into()),
            },
            ..Default::default()
        };
        for node in root.children().filter(|n| n.is_element()) {
            match node.tag_name().name() {
                "Gestures" if settings.gestures.is_none() && node.attributes().len() == 1 => {
                    let enabled = match node.attribute("enabled") {
                        Some("0") => false,
                        Some("1") => true,
                        _ => return Err("按键手势开关无效".into()),
                    };
                    let mut documents = [None, None];
                    for wrapper in node.children().filter(|n| n.is_element()) {
                        let index = match wrapper.tag_name().name() {
                            "Sixteen" => 0,
                            "Mouse" => 1,
                            _ => return Err("按键手势包含未知方向表".into()),
                        };
                        let children: Vec<_> =
                            wrapper.children().filter(|n| n.is_element()).collect();
                        if wrapper.attributes().len() != 0
                            || documents[index].is_some()
                            || children.len() != 1
                        {
                            return Err("按键手势方向表重复或不完整".into());
                        }
                        let document = Document::parse(text[children[0].range()].as_bytes())?;
                        if document.kind != [Kind::Sixteen, Kind::Mouse][index] {
                            return Err("按键手势方向表类型不匹配".into());
                        }
                        documents[index] = Some(document);
                    }
                    let [Some(sixteen), Some(mouse)] = documents else {
                        return Err("按键手势需要两套方向表".into());
                    };
                    settings.gestures = Some(Box::new(Gestures {
                        enabled,
                        documents: [sixteen, mouse],
                    }));
                }
                "Signature"
                    if settings.signature.is_none()
                        && node.attributes().len() == 0
                        && !node.children().any(|n| n.is_element()) =>
                {
                    let signature = node
                        .text()
                        .unwrap_or("")
                        .split(',')
                        .map(|v| u16::from_str_radix(v, 16))
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|_| "主机动作签名无效")?;
                    settings.signature =
                        Some(signature.try_into().map_err(|_| "主机动作签名长度无效")?);
                }
                "Profile"
                    if node.attributes().len() == 0
                        && settings.profiles.len() < configuration::PROFILES =>
                {
                    let mut slots: [Option<Action>; SLOTS] = std::array::from_fn(|_| None);
                    for record in node.children().filter(|n| n.is_element()) {
                        if record.tag_name().name() != "Slot" || record.attributes().len() != 1 {
                            return Err("主机动作记录无效".into());
                        }
                        let index = record
                            .attribute("index")
                            .and_then(|s| s.parse::<usize>().ok())
                            .filter(|i| *i < SLOTS)
                            .ok_or("主机动作记录序号无效")?;
                        let children: Vec<_> =
                            record.children().filter(|n| n.is_element()).collect();
                        if slots[index].is_some()
                            || children.len() != 1
                            || children[0].tag_name().name() != "Action"
                        {
                            return Err("主机动作记录重复或不完整".into());
                        }
                        slots[index] = Some(Action::from_node(children[0])?);
                    }
                    settings.profiles.push(slots);
                }
                _ => return Err("主机动作包含未知或重复元素".into()),
            }
        }
        settings.validate_actions()?;
        Ok(settings)
    }
}

pub fn path() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))
        .map(|p| p.join("bloody-a90/host-buttons.xml"))
}

pub fn load(path: &Path) -> Result<(Settings, Option<Vec<u8>>), String> {
    match fs::read(path) {
        Ok(bytes) => Ok((Settings::parse(&bytes)?, Some(bytes))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok((Settings::default(), None)),
        Err(e) => Err(format!("无法读取 {}：{e}", path.display())),
    }
}

pub fn save(path: &Path, settings: &Settings, expected: Option<&[u8]>) -> Result<(), String> {
    let bytes = settings.to_bytes();
    Settings::parse(&bytes)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    crate::gesture_file::save_checked(path, &bytes, expected)
}

/// Persist metadata before changing hardware. A failed hardware operation
/// restores the exact previous file. If there was no previous file, leave an
/// empty disabled file. Rollback checks the current file before replacement.
pub fn commit(
    path: &Path,
    settings: &Settings,
    expected: Option<&[u8]>,
    write_hardware: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    save(path, settings, expected)?;
    if let Err(error) = write_hardware() {
        let new_bytes = settings.to_bytes();
        let disabled = Settings {
            enabled: false,
            ..Settings::default()
        }
        .to_bytes();
        let previous = expected.unwrap_or(&disabled);
        let restored = crate::gesture_file::save_checked(path, previous, Some(&new_bytes));
        return Err(match restored {
            Ok(()) => error,
            Err(restore) => format!("{error}；恢复主机动作文件失败：{restore}"),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn configuration() -> Configuration {
        Configuration {
            profiles: (0..2)
                .map(|_| configuration::Profile {
                    buttons: Default::default(),
                    wheel: std::array::from_fn(|_| {
                        configuration::Action::Direct(storage::NO_OPERATION)
                    }),
                    gestures: None,
                    metadata: [0x1ff, 0],
                })
                .collect(),
            selected: 0,
            auxiliary: std::array::from_fn(|_| {
                configuration::Action::Direct(storage::NO_OPERATION)
            }),
            parameters: [50; 6],
            flags: 0x10,
            header_word6: 9,
        }
    }

    #[test]
    fn host_file_keeps_profiles_unicode_arguments_and_bound_hardware_slots() {
        let mut config = configuration();
        let mut settings = Settings::default();
        settings.resize(2);
        settings.profiles[0][3] = Some(
            Action::open_target_with_arguments("/usr/bin/true", "a b\n中😀\n$()")
                .unwrap()
                .with_mode(0),
        );
        settings.profiles[1][10] = Some(Action::input_text("中😀\n<&").unwrap().with_mode(0));
        settings.profiles[1][24] = Some(Action::new(22, 0, 0, "放大"));
        settings.bind(&config).unwrap();
        let bytes = settings.to_bytes();
        assert_eq!(Settings::parse(&bytes).unwrap(), settings);
        assert!(settings.matches(&configuration::build(&config).unwrap()[..16]));
        config.profiles[0].buttons[3].press = configuration::Action::Direct(0x2104);
        assert!(settings.bind(&config).is_err());
        settings.reconcile(&config);
        assert!(settings.profiles[0][3].is_none());
        assert!(settings.profiles[1][10].is_some());
        settings.bind(&config).unwrap();
        let mut wrong = settings.signature.unwrap();
        wrong[3] ^= 1;
        assert!(!settings.matches(&wrong));
        let original = String::from_utf8(bytes).unwrap();
        for invalid in [
            original.replace("index=\"3\"", "index=\"26\""),
            original.replace("version=\"1\"", "version=\"2\""),
            original.replace("<Profile>", "<Unexpected>"),
            original.replace("<Signature>", "<Signature><Nested/>"),
        ] {
            assert!(Settings::parse(invalid.as_bytes()).is_err());
        }
    }

    #[test]
    fn native_gesture_assignments_keep_both_documents_and_reject_wheel_slots() {
        let config = configuration();
        let mut settings = Settings::default();
        settings.resize(config.profiles.len());
        let mut documents = [Document::new(Kind::Sixteen), Document::new(Kind::Mouse)];
        documents[0].actions[0] = Action::input_text("中😀<&").unwrap();
        documents[1].actions[3] = Action::keyboard(27, 1, "剪切");
        settings.gestures = Some(Box::new(Gestures {
            enabled: true,
            documents,
        }));
        settings.profiles[0][8] = Some(Action::new(2, 0, 0, "16 合一"));
        settings.profiles[1][10] = Some(Action::new(2, 1, 0, "鼠标手势"));
        settings.bind(&config).unwrap();
        assert_eq!(settings.gesture_kinds(), [true; 2]);
        assert_eq!(settings.gesture_slots(0)[8], Some(0));
        assert_eq!(settings.gesture_slots(0)[10], None);
        assert_eq!(settings.gesture_slots(1)[10], Some(1));
        assert_eq!(settings.gesture_slots(7), [None; 24]);
        assert_eq!(Settings::parse(&settings.to_bytes()).unwrap(), settings);
        settings.gestures.as_mut().unwrap().enabled = false;
        assert_eq!(settings.assigned_gesture_kinds(), [true; 2]);
        assert_eq!(settings.gesture_kinds(), [false; 2]);
        let bytes = settings.to_bytes();
        assert_eq!(Settings::parse(&bytes).unwrap(), settings);
        let text = String::from_utf8(bytes).unwrap();
        for invalid in [
            text.replace("<Mouse>", "<Sixteen>"),
            text.replace("<Gestures enabled=\"0\">", "<Gestures enabled=\"2\">"),
            text.replace("</Gestures>", "<Mouse/></Gestures>"),
        ] {
            assert!(Settings::parse(invalid.as_bytes()).is_err());
        }
        let action = settings.profiles[0][8].clone();
        settings.profiles[0][24] = action;
        assert!(settings.bind(&config).is_err());
        assert!(Settings::parse(&settings.to_bytes()).is_err());
        settings.profiles[0][24] = None;
        settings.profiles[0][8] = Some(Action::new(2, 2, 0, "未知手势"));
        assert!(settings.bind(&config).is_err());
        assert!(Settings::parse(&settings.to_bytes()).is_err());
        settings.profiles[0][8] = None;
        settings.gestures = None;
        assert!(settings.bind(&config).is_err());
        assert!(Settings::parse(&settings.to_bytes()).is_err());
    }

    #[test]
    fn seven_populated_profiles_and_gesture_documents_fit_the_file_parser() {
        let mut config = configuration();
        config.profiles.resize(7, config.profiles[0].clone());
        let mut settings = Settings::default();
        settings.resize(7);
        for slots in &mut settings.profiles {
            slots.fill(Some(
                Action::open_target_with_arguments("/usr/bin/true", "中😀\n<&\n$()")
                    .unwrap()
                    .with_mode(0),
            ));
            slots[8] = Some(Action::new(2, 0, 0, "16 合一"));
            slots[9] = Some(Action::new(2, 1, 0, "鼠标手势"));
        }
        settings.gestures = Some(Box::new(Gestures {
            enabled: true,
            documents: [Document::new(Kind::Sixteen), Document::new(Kind::Mouse)],
        }));
        settings.bind(&config).unwrap();
        assert_eq!(Settings::parse(&settings.to_bytes()).unwrap(), settings);
    }

    #[test]
    fn host_file_save_rejects_external_changes_and_keeps_the_previous_file() {
        let dir = std::env::temp_dir().join(format!("a90-host-file-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("buttons.xml");
        let mut settings = Settings::default();
        settings.resize(2);
        settings.bind(&configuration()).unwrap();
        save(&path, &settings, None).unwrap();
        let old = fs::read(&path).unwrap();
        settings.enabled = false;
        assert!(save(&path, &settings, None).is_err());
        save(&path, &settings, Some(&old)).unwrap();
        let new = fs::read(&path).unwrap();
        assert!(save(&path, &Settings::default(), Some(&old)).is_err());
        assert_eq!(fs::read(&path).unwrap(), new);
        assert_eq!(load(&path).unwrap().0, settings);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_hardware_commit_restores_metadata_and_preserves_concurrent_changes() {
        let dir = std::env::temp_dir().join(format!("a90-host-transaction-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("buttons.xml");
        let mut proposed = Settings::default();
        proposed.resize(2);
        proposed.bind(&configuration()).unwrap();
        let error = commit(&path, &proposed, None, || {
            assert_eq!(load(&path).unwrap().0, proposed);
            Err("HID write failed".into())
        })
        .unwrap_err();
        assert_eq!(error, "HID write failed");
        let (empty, _) = load(&path).unwrap();
        assert!(!empty.enabled && empty.is_empty());
        let old = fs::read(&path).unwrap();
        commit(&path, &proposed, Some(&old), || Ok(())).unwrap();
        let previous = fs::read(&path).unwrap();
        proposed.enabled = false;
        assert!(
            commit(&path, &proposed, Some(&previous), || Err(
                "readback failed".into()
            ))
            .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), previous);
        assert!(
            commit(&path, &proposed, Some(&previous), || {
                fs::write(&path, b"external change").unwrap();
                Err("readback failed".into())
            })
            .unwrap_err()
            .contains("恢复主机动作文件失败")
        );
        assert_eq!(fs::read(&path).unwrap(), b"external change");
        fs::remove_dir_all(dir).unwrap();
    }
}
