//! Host gesture documents and recognition. EXE 0045cd4c, 013eb354,
//! 013eaf80 and 013efa38. These functions never access input devices.
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Sixteen,
    Mouse,
}

#[derive(Clone, Copy, Debug)]
pub struct Shape {
    pub tag: &'static str,
    pub name: &'static str,
    pub code: &'static str,
}

const COMMON: [Shape; 4] = [
    Shape {
        tag: "Gesture-Up",
        name: "上",
        code: "3",
    },
    Shape {
        tag: "Gesture-Down",
        name: "下",
        code: "7",
    },
    Shape {
        tag: "Gesture-Left",
        name: "左",
        code: "5",
    },
    Shape {
        tag: "Gesture-Right",
        name: "右",
        code: "1",
    },
];
const CORNERS: [Shape; 8] = [
    Shape {
        tag: "Gesture-Up-Left",
        name: "上 → 左",
        code: "35",
    },
    Shape {
        tag: "Gesture-Up-Right",
        name: "上 → 右",
        code: "31",
    },
    Shape {
        tag: "Gesture-Down-Left",
        name: "下 → 左",
        code: "75",
    },
    Shape {
        tag: "Gesture-Down-Right",
        name: "下 → 右",
        code: "71",
    },
    Shape {
        tag: "Gesture-Left-Up",
        name: "左 → 上",
        code: "53",
    },
    Shape {
        tag: "Gesture-Left-Down",
        name: "左 → 下",
        code: "57",
    },
    Shape {
        tag: "Gesture-Right-Up",
        name: "右 → 上",
        code: "13",
    },
    Shape {
        tag: "Gesture-Right-Down",
        name: "右 → 下",
        code: "17",
    },
];
const SIXTEEN: [Shape; 16] = [
    COMMON[0],
    COMMON[1],
    COMMON[2],
    COMMON[3],
    Shape {
        tag: "Gesture-Right-Slant-Up",
        name: "右上斜向",
        code: "2",
    },
    Shape {
        tag: "Gesture-Left-Slant-Down",
        name: "左下斜向",
        code: "6",
    },
    Shape {
        tag: "Gesture-Left-Slant-Up",
        name: "左上斜向",
        code: "4",
    },
    Shape {
        tag: "Gesture-Right-Slant-Down",
        name: "右下斜向",
        code: "8",
    },
    CORNERS[0],
    CORNERS[1],
    CORNERS[2],
    CORNERS[3],
    CORNERS[4],
    CORNERS[5],
    CORNERS[6],
    CORNERS[7],
];
const MOUSE: [Shape; 16] = [
    COMMON[0],
    COMMON[1],
    COMMON[2],
    COMMON[3],
    Shape {
        tag: "Gesture-Up-Down",
        name: "上 → 下",
        code: "37",
    },
    Shape {
        tag: "Gesture-Down-Up",
        name: "下 → 上",
        code: "73",
    },
    Shape {
        tag: "Gesture-Right-Left",
        name: "右 → 左",
        code: "15",
    },
    Shape {
        tag: "Gesture-Left-Right",
        name: "左 → 右",
        code: "51",
    },
    CORNERS[0],
    CORNERS[1],
    CORNERS[2],
    CORNERS[3],
    CORNERS[4],
    CORNERS[5],
    CORNERS[6],
    CORNERS[7],
];

impl Kind {
    pub fn shapes(self) -> &'static [Shape; 16] {
        match self {
            Self::Sixteen => &SIXTEEN,
            Self::Mouse => &MOUSE,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Sixteen => "16 合一手势",
            Self::Mouse => "鼠标手势",
        }
    }

    pub fn file_name(self) -> &'static str {
        match self {
            Self::Sixteen => "16in1.gestures",
            Self::Mouse => "Mouse.gestures",
        }
    }

    pub fn index(self, code: &str) -> Option<usize> {
        self.shapes().iter().position(|shape| shape.code == code)
    }
}

const ACTION_FIELDS: [&str; 12] = [
    "Simulator",
    "Object",
    "Action",
    "HintText",
    "MacroName",
    "MacroPath",
    "Text",
    "Comment",
    "Append",
    "Reserve1",
    "Reserve2",
    "FileFormat",
];

/// Keep text and unknown scalar properties when editing a known property.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    fields: BTreeMap<String, String>,
}

impl Default for Action {
    fn default() -> Self {
        Self::new(0, 0, 3, "未设置")
    }
}

impl Action {
    pub fn with_mode(mut self, mode: u32) -> Self {
        self.fields.insert("Action".into(), mode.to_string());
        self
    }

    pub(crate) fn from_node(node: roxmltree::Node<'_, '_>) -> Result<Self, String> {
        if node.attributes().len() != 0 {
            return Err("动作元素不能包含属性".into());
        }
        let mut fields = BTreeMap::new();
        for field in node.children().filter(|n| n.is_element()) {
            let key = field.tag_name().name();
            if fields.insert(key.to_owned(), scalar(field)?).is_some() {
                return Err(format!("动作属性 {key} 重复"));
            }
        }
        let action = Self { fields };
        for property in ["Simulator", "Object", "Action"] {
            action.number(property)?;
        }
        Ok(action)
    }

    pub(crate) fn write_fields(&self, output: &mut String) {
        for (name, value) in &self.fields {
            output.push_str(&format!("<{name}>{}</{name}>", escape(value)));
        }
    }

    pub fn new(simulator: u32, object: u32, action: u32, label: &str) -> Self {
        let mut fields: BTreeMap<_, _> = ACTION_FIELDS[..11]
            .iter()
            .map(|name| (name.to_string(), String::new()))
            .collect();
        for (name, value) in [
            ("Simulator", simulator),
            ("Object", object),
            ("Action", action),
        ] {
            fields.insert(name.into(), value.to_string());
        }
        fields.insert("Text".into(), label.into());
        fields.insert("Comment".into(), label.into());
        Self { fields }
    }

    pub fn get(&self, name: &str) -> &str {
        self.fields.get(name).map_or("", String::as_str)
    }

    pub fn number(&self, name: &str) -> Result<u32, String> {
        self.get(name)
            .trim()
            .parse()
            .map_err(|_| format!("手势动作的 {name} 不是非负整数"))
    }

    pub fn label(&self) -> &str {
        if !self.get("Text").is_empty() {
            self.get("Text")
        } else {
            self.get("Comment")
        }
    }

    /// Object = modifier mask * 1000 + USB usage. EXE 0062f6c8, case 6.
    pub fn keyboard(usage: u8, modifiers: u8, label: &str) -> Self {
        Self::new(6, modifiers as u32 * 1000 + usage as u32, 3, label)
    }

    pub fn macro_file(path: &str) -> Result<Self, String> {
        if path.trim().is_empty() {
            return Err("请填写 AMC 文件路径".into());
        }
        let mut action = Self::new(1, 0, 3, "奥斯卡宏");
        action.fields.insert("MacroPath".into(), path.into());
        action.fields.insert(
            "MacroName".into(),
            std::path::Path::new(path)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        );
        Ok(action)
    }

    pub fn open_target(target: &str) -> Result<Self, String> {
        if target.trim().is_empty() {
            return Err("请填写打开目标的路径或 URL".into());
        }
        let mut action = Self::new(20, 0, 3, "打开文件或程序");
        action.fields.insert("MacroName".into(), target.into());
        Ok(action)
    }

    /// Linux-only arguments remain an optional scalar XML extension.
    pub fn open_target_with_arguments(target: &str, arguments: &str) -> Result<Self, String> {
        crate::host_open::Plan::prepare_with_arguments(target, arguments)?;
        let mut action = Self::open_target(target)?;
        if !arguments.is_empty() {
            action
                .fields
                .insert("LinuxArguments".into(), arguments.into());
        }
        Ok(action)
    }

    pub fn input_text(text: &str) -> Result<Self, String> {
        crate::host_text::Plan::prepare(text)?;
        let mut action = Self::new(21, 0, 3, "输入文字");
        action.fields.insert("HintText".into(), text.into());
        Ok(action)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    pub kind: Kind,
    pub realtime: bool,
    pub distance: u32,
    pub tip: bool,
    pub actions: [Action; 16],
    extra: BTreeMap<String, String>,
}

impl Document {
    pub fn new(kind: Kind) -> Self {
        Self {
            kind,
            realtime: false,
            distance: 3,
            tip: true,
            actions: std::array::from_fn(|_| Action::default()),
            extra: BTreeMap::new(),
        }
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 1024 * 1024 {
            return Err("手势文件超过 1 MiB".into());
        }
        let text = decode(bytes)?;
        let xml = roxmltree::Document::parse_with_options(
            &text,
            roxmltree::ParsingOptions {
                nodes_limit: 4096,
                ..Default::default()
            },
        )
        .map_err(|e| format!("手势 XML 无效：{e}"))?;
        if xml
            .descendants()
            .filter(|node| node.is_element())
            .any(|node| node.tag_name().namespace().is_some())
        {
            return Err("手势文件不支持 XML 命名空间".into());
        }
        let root = xml.root_element();
        if root.tag_name().name() != "Root" || root.attributes().len() != 0 {
            return Err("手势文件需要无属性的 Root 元素".into());
        }
        let mut properties = BTreeMap::new();
        let mut gesture = None;
        for child in root.children().filter(|n| n.is_element()) {
            let name = child.tag_name().name();
            if name == "Gesture" {
                if gesture.replace(child).is_some() {
                    return Err("Gesture 元素重复".into());
                }
            } else if properties.insert(name.to_owned(), scalar(child)?).is_some() {
                return Err(format!("手势属性 {name} 重复"));
            }
        }
        let kind = match (
            properties.contains_key("Mode"),
            properties.contains_key("EnabledTipBoard"),
        ) {
            (true, false) => Kind::Sixteen,
            (false, true) => Kind::Mouse,
            _ => return Err("无法区分两套手势格式：需要 Mode 或 EnabledTipBoard".into()),
        };
        let mut document = Self::new(kind);
        let flag = |value: Option<String>, name| match value.as_deref().map(str::trim) {
            Some("0") => Ok(false),
            Some("1") => Ok(true),
            _ => Err(format!("手势属性 {name} 应为 0 或 1")),
        };
        match kind {
            Kind::Sixteen => {
                document.realtime = flag(properties.remove("Mode"), "Mode")?;
                document.distance = properties
                    .remove("Sensitivity")
                    .and_then(|v| v.trim().parse().ok())
                    .filter(|v| *v > 0)
                    .ok_or("手势距离应为正整数")?;
            }
            Kind::Mouse => {
                document.tip = flag(properties.remove("EnabledTipBoard"), "EnabledTipBoard")?
            }
        }
        document.extra = properties;
        let gesture = gesture.ok_or("缺少 Gesture 元素")?;
        if gesture.attributes().len() != 0 {
            return Err("Gesture 元素不能包含属性".into());
        }
        let mut found = [false; 16];
        for child in gesture.children().filter(|n| n.is_element()) {
            if child.attributes().len() != 0 {
                return Err("方向元素不能包含属性".into());
            }
            let name = child.tag_name().name();
            let index = kind
                .shapes()
                .iter()
                .position(|shape| shape.tag == name)
                .ok_or_else(|| format!("该手势格式不包含方向 {name}"))?;
            if std::mem::replace(&mut found[index], true) {
                return Err(format!("方向 {name} 重复"));
            }
            let mut fields = BTreeMap::new();
            for field in child.children().filter(|n| n.is_element()) {
                let key = field.tag_name().name();
                if fields.insert(key.to_owned(), scalar(field)?).is_some() {
                    return Err(format!("方向 {name} 的属性 {key} 重复"));
                }
            }
            let action = Action { fields };
            for property in ["Simulator", "Object", "Action"] {
                action.number(property)?;
            }
            document.actions[index] = action;
        }
        if found.iter().any(|found| !found) {
            return Err("手势文件必须包含全部 16 个方向".into());
        }
        Ok(document)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut output = "\u{feff}<Root>\r\n".to_string();
        let mut properties = self.extra.clone();
        match self.kind {
            Kind::Sixteen => {
                properties.insert("Mode".into(), u8::from(self.realtime).to_string());
                properties.insert("Sensitivity".into(), self.distance.to_string());
            }
            Kind::Mouse => {
                properties.insert("EnabledTipBoard".into(), u8::from(self.tip).to_string());
            }
        }
        let property = |out: &mut String, depth: &str, name: &str, value: &str| {
            out.push_str(&format!("{depth}<{name}>{}</{name}>\r\n", escape(value)));
        };
        for (name, value) in properties {
            property(&mut output, "\t", &name, &value);
        }
        output.push_str("\t<Gesture>\r\n");
        for (shape, action) in self.kind.shapes().iter().zip(&self.actions) {
            output.push_str(&format!("\t\t<{}>\r\n", shape.tag));
            for name in ACTION_FIELDS {
                if let Some(value) = action.fields.get(name) {
                    property(&mut output, "\t\t\t", name, value);
                }
            }
            for (name, value) in &action.fields {
                if !ACTION_FIELDS.contains(&name.as_str()) {
                    property(&mut output, "\t\t\t", name, value);
                }
            }
            output.push_str(&format!("\t\t</{}>\r\n", shape.tag));
        }
        output.push_str("\t</Gesture>\r\n</Root>\r\n");
        output.into_bytes()
    }
}

fn scalar(node: roxmltree::Node<'_, '_>) -> Result<String, String> {
    if node.attributes().len() != 0 || node.children().any(|n| n.is_element()) {
        return Err(format!("属性 {} 需要纯文本内容", node.tag_name().name()));
    }
    Ok(node
        .children()
        .filter(|n| n.is_text())
        .filter_map(|n| n.text())
        .collect())
}

fn decode(bytes: &[u8]) -> Result<String, String> {
    let utf16 = |data: &[u8], big| {
        if !data.len().is_multiple_of(2) {
            return Err("UTF-16 手势文件的字节数无效".into());
        }
        let units: Vec<_> = data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| {
                if big {
                    u16::from_be_bytes(*b)
                } else {
                    u16::from_le_bytes(*b)
                }
            })
            .collect();
        String::from_utf16(&units).map_err(|_| "手势文件不是有效的 UTF-16".into())
    };
    match bytes {
        [0xff, 0xfe, data @ ..] => utf16(data, false),
        [0xfe, 0xff, data @ ..] => utf16(data, true),
        _ => std::str::from_utf8(bytes)
            .map(|s| s.trim_start_matches('\u{feff}').to_owned())
            .map_err(|_| "手势文件需要 UTF-8 或带 BOM 的 UTF-16".into()),
    }
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\r', "&#13;")
}

/// EXE 013eb354. Integer half-division and strict comparisons matter near
/// sector boundaries. Widen first so i32::MIN does not overflow abs().
pub fn direction8(x: i32, y: i32) -> u8 {
    let (x, y) = (i64::from(x), i64::from(y));
    if x.abs() < (y / 2).abs() {
        return if y > 0 { 7 } else { 3 };
    }
    if y.abs() < (x / 2).abs() {
        return if x > 0 { 1 } else { 5 };
    }
    match (x.signum(), y.signum()) {
        (1, 1) => 8,
        (1, -1) => 2,
        (-1, 1) => 6,
        (-1, -1) => 4,
        _ => 0,
    }
}

/// Sectors from EXE 013ef8a4 / 013ef954. The native degrees factor is
/// 180 / 3.14, not 180 / pi. This changes some diagonal classifications.
pub fn direction4(x: i32, y: i32) -> u8 {
    if x == 0 && y == 0 {
        return 0;
    }
    const DEGREES_FACTOR: f64 = f64::from_bits(0x404c_a994_61d5_9ae7);
    let (dx, dy) = (f64::from(x), f64::from(y));
    let magnitude = (dx * dx + dy * dy).sqrt();
    let angle = (-dy / magnitude).acos() * DEGREES_FACTOR;
    let angle = if x < 0 { -angle } else { angle };
    if -45.0 < angle && angle <= 45.0 {
        3
    } else if 45.0 < angle && angle <= 135.0 {
        1
    } else if -135.0 < angle && angle <= -45.0 {
        5
    } else {
        7
    }
}

/// Release recognition for 16-in-1, EXE 013eaf80 at 13eb0ff..13eb269.
/// The first segment stays. A later segment needs three equal samples to
/// survive another turn. Samples occur only beyond the 20-unit threshold.
#[derive(Clone, Debug, Default)]
pub struct Trace16 {
    accumulated: [i32; 2],
    path: String,
    repeats: usize,
}

impl Trace16 {
    pub fn feed(&mut self, delta: [i32; 2]) {
        for (value, delta) in self.accumulated.iter_mut().zip(delta) {
            *value = value.saturating_add(delta);
        }
        let [x, y] = self.accumulated;
        if f64::from(x).hypot(f64::from(y)) <= 20.0 {
            return;
        }
        self.accumulated = [0; 2];
        let direction = direction8(x, y);
        if direction == 0 {
            return;
        }
        let code = (b'0' + direction) as char;
        if self.path.ends_with(code) {
            self.repeats = self.repeats.saturating_add(1);
        } else {
            if self.repeats < 2 && self.path.len() > 1 {
                self.path.pop();
            }
            if self.path.len() < 100 {
                self.path.push(code);
            }
            self.repeats = 0;
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn index(&self) -> Option<usize> {
        Kind::Sixteen.index(&self.path)
    }
}

/// Mouse.gestures recognition, EXE 013ef1a0. The native substring wrapper
/// 0040f0c0 / 013c5360 uses one-based indices and clamps zero to one.
/// Both native filtering loops stop before length - 1, leaving two samples
/// of lookahead. Keep that behavior rather than reducing it to run lengths.
#[derive(Clone, Debug, Default)]
pub struct TraceMouse {
    accumulated: [i32; 2],
    samples: Vec<u8>,
    path: String,
    overflow: bool,
}

impl TraceMouse {
    pub fn feed(&mut self, delta: [i32; 2]) {
        if self.overflow {
            return;
        }
        for (value, delta) in self.accumulated.iter_mut().zip(delta) {
            *value = value.saturating_add(delta);
        }
        let [x, y] = self.accumulated;
        let distance = f64::from(x).hypot(f64::from(y)).floor();
        if distance < 10.0 {
            return;
        }
        self.accumulated = [0; 2];
        let direction = b'0' + direction4(x, y);
        let count = if distance > 30.0 {
            (distance / 10.0) as usize
        } else {
            1
        };
        // A long or corrupt input stream must not allocate without a bound.
        // It invalidates this stroke instead of matching a truncated prefix.
        if count > 8192usize.saturating_sub(self.samples.len()) {
            self.overflow = true;
            self.path.clear();
            return;
        }
        self.samples.extend(std::iter::repeat_n(direction, count));
        self.filter();
    }

    fn filter(&mut self) {
        let mut index = 1;
        while index < self.samples.len().saturating_sub(1) {
            let previous = self.samples[index.saturating_sub(2)];
            let current = self.samples[index - 1];
            let next = self.samples[index];
            if previous != current && next != current {
                self.samples.remove(index - 1);
            }
            index += 1;
        }
        self.path.clear();
        let mut previous = None;
        let mut repeats = 0;
        for index in 1..self.samples.len().saturating_sub(1) {
            let current = self.samples[index - 1];
            if previous != Some(current) {
                previous = Some(current);
                repeats = 0;
            }
            repeats += 1;
            if repeats >= 3 && self.path.as_bytes().last() != Some(&current) {
                self.path.push(current as char);
                previous = None;
                repeats = 0;
            }
            if self.path.len() > 3 {
                break;
            }
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn index(&self) -> Option<usize> {
        if self.overflow {
            None
        } else {
            Kind::Mouse.index(&self.path)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_sectors_boundaries_and_limits() {
        for (x, y, eight, four) in [
            (0, -100, 3, 3),
            (0, 100, 7, 7),
            (-100, 0, 5, 5),
            (100, 0, 1, 1),
            (100, -100, 2, 1),
            (-100, 100, 6, 7),
            (-100, -100, 4, 5),
            (100, 100, 8, 7),
            (49, -100, 3, 3),
            (50, -100, 2, 3),
            (100, 49, 1, 1),
            (100, 50, 8, 1),
            (0, 0, 0, 0),
            (1, 0, 0, 1),
            (2, 0, 1, 1),
            (i32::MIN, 0, 5, 5),
        ] {
            assert_eq!(
                (direction8(x, y), direction4(x, y)),
                (eight, four),
                "{x}, {y}"
            );
        }
    }

    #[test]
    fn sixteen_paths_threshold_and_short_turn_filter() {
        let vectors = [
            (3, [0, -21]),
            (7, [0, 21]),
            (5, [-21, 0]),
            (1, [21, 0]),
            (2, [21, -21]),
            (6, [-21, 21]),
            (4, [-21, -21]),
            (8, [21, 21]),
        ];
        for (index, shape) in SIXTEEN.iter().enumerate() {
            let mut trace = Trace16::default();
            for digit in shape.code.bytes() {
                let delta = vectors
                    .iter()
                    .find(|(number, _)| *number == digit - b'0')
                    .unwrap()
                    .1;
                for _ in 0..3 {
                    trace.feed(delta);
                }
            }
            assert_eq!(trace.index(), Some(index), "{}", shape.tag);
        }
        let mut trace = Trace16::default();
        trace.feed([0, -20]);
        assert!(trace.path().is_empty());
        trace.feed([0, -1]);
        assert_eq!(trace.path(), "3");
        trace.feed([21, 0]);
        trace.feed([0, -21]);
        assert_eq!(trace.path(), "33"); // short second segment was removed
        assert_eq!(trace.index(), None);
    }

    #[test]
    fn mouse_paths_lookahead_isolated_noise_and_distance_weights() {
        let vectors = [
            (b'3', [0, -50]),
            (b'7', [0, 50]),
            (b'5', [-50, 0]),
            (b'1', [50, 0]),
        ];
        for (index, shape) in MOUSE.iter().enumerate() {
            let mut trace = TraceMouse::default();
            for digit in shape.code.bytes() {
                trace.feed(
                    vectors
                        .iter()
                        .find(|(number, _)| *number == digit)
                        .unwrap()
                        .1,
                );
            }
            assert_eq!(trace.index(), Some(index), "{}", shape.tag);
        }
        let mut trace = TraceMouse::default();
        trace.feed([0, -9]);
        assert!(trace.samples.is_empty());
        trace.feed([0, -1]);
        assert_eq!(trace.samples, b"3");
        trace.feed([0, -30]);
        assert_eq!(trace.samples, b"33");
        trace.feed([0, -31]);
        assert_eq!(trace.samples, b"33333");
        assert_eq!(trace.path(), "3");
        for (samples, filtered, path) in [
            ("3333", "3333", ""),
            ("33333", "33333", "3"),
            ("3331333", "333333", "3"),
            ("3333311111", "3333311111", "31"),
            ("3333377777", "3333377777", "37"),
            ("333331111155555", "333331111155555", "315"),
        ] {
            let mut trace = TraceMouse {
                samples: samples.as_bytes().to_vec(),
                ..Default::default()
            };
            trace.filter();
            assert_eq!(trace.samples, filtered.as_bytes());
            assert_eq!(trace.path(), path);
            if path.len() > 2 {
                assert_eq!(trace.index(), None);
            }
        }
        let mut trace = TraceMouse::default();
        trace.feed([i32::MAX, i32::MAX]);
        trace.feed([0, -100]);
        assert!(trace.overflow);
        assert_eq!(trace.index(), None);
    }

    #[test]
    fn files_preserve_actions_extras_and_separate_directions() {
        for kind in [Kind::Sixteen, Kind::Mouse] {
            let mut doc = Document::new(kind);
            doc.actions[0] = Action::keyboard(27, 1, "剪切 <Ctrl+X> & 测试");
            doc.actions[0]
                .fields
                .insert("FutureField".into(), "保留".into());
            doc.extra.insert("FutureOption".into(), "& < >".into());
            let bytes = doc.to_bytes();
            assert_eq!(Document::parse(&bytes).unwrap(), doc);
            let text = std::str::from_utf8(&bytes)
                .unwrap()
                .trim_start_matches('\u{feff}');
            let mut utf16 = vec![0xff, 0xfe];
            utf16.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
            assert_eq!(Document::parse(&utf16).unwrap(), doc);
            assert_eq!(doc.actions[0].number("Object"), Ok(1027));
        }
        assert_eq!(Kind::Sixteen.index("2"), Some(4));
        assert_eq!(Kind::Mouse.index("2"), None);
        assert_eq!(Kind::Mouse.index("37"), Some(4));
    }

    #[test]
    fn malformed_files_are_rejected() {
        let valid = String::from_utf8(Document::new(Kind::Sixteen).to_bytes()).unwrap();
        for invalid in [
            valid.replace("<Mode>0</Mode>", "<Mode>0</Mode><Mode>1</Mode>"),
            valid.replace("<Mode>0</Mode>", "<Mode>2</Mode>"),
            valid.replace("<Root>", "<Root xmlns=\"urn:other\">"),
            valid.replace(
                "<Sensitivity>3</Sensitivity>",
                "<Sensitivity>0</Sensitivity>",
            ),
            valid.replace("<Gesture-Up>", "<Gesture-Up x=\"1\">"),
            valid.replace("<Simulator>0</Simulator>", "<Simulator>bad</Simulator>"),
            valid.replace("<Gesture-Up>", "<Gesture-Down>"),
            valid.replace(
                "<Mode>0</Mode>",
                "<Mode>0</Mode><EnabledTipBoard>1</EnabledTipBoard>",
            ),
            "<!DOCTYPE Root [<!ENTITY x 'abc'>]><Root>&x;</Root>".into(),
        ] {
            assert!(Document::parse(invalid.as_bytes()).is_err());
        }
        assert!(Document::parse(&[0xff, 0xfe, 0]).is_err());
    }

    #[test]
    #[ignore = "requires A90_GESTURE_DIR with the official gesture library"]
    fn official_files_round_trip() {
        let dir = std::path::PathBuf::from(std::env::var_os("A90_GESTURE_DIR").unwrap());
        for kind in [Kind::Sixteen, Kind::Mouse] {
            let doc = Document::parse(&std::fs::read(dir.join(kind.file_name())).unwrap()).unwrap();
            assert_eq!(doc.kind, kind);
            assert_eq!(Document::parse(&doc.to_bytes()).unwrap(), doc);
            if kind == Kind::Mouse {
                assert_eq!(doc.actions[2].number("Object"), Ok(1027));
            }
        }
    }
}
