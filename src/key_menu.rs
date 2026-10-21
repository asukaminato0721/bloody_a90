//! Menu entries of the official KeySet window, shared by the desktop editor
//! and the browser version.
use crate::{i18n::tr, storage::NO_OPERATION, tf};

/// Action types of the KeySet window.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Mouse,
    Keyboard,
    Media,
    System,
    Shortcut,
    Function,
    Shoot,
    Macro,
    Host,
    Disabled,
}

pub const CHOICES: [(Choice, &str); 10] = [
    (Choice::Mouse, "鼠标按键"),
    (Choice::Keyboard, "键盘按键"),
    (Choice::Media, "多媒体"),
    (Choice::System, "系统按键"),
    (Choice::Shortcut, "快捷操作"),
    (Choice::Function, "功能"),
    (Choice::Shoot, "射击模式"),
    (Choice::Macro, "宏"),
    (Choice::Host, "主机动作"),
    (Choice::Disabled, "禁用"),
];

pub const MOUSE: [(&str, u16); 5] = [
    ("左键", 1),
    ("右键", 2),
    ("中键", 4),
    ("后退键（四键）", 8),
    ("前进键（五键）", 16),
];

// USB consumer usages.
pub const MEDIA: [(&str, u16); 20] = [
    ("播放/暂停", 0xcd),
    ("停止", 0xb7),
    ("上一曲", 0xb6),
    ("下一曲", 0xb5),
    ("静音", 0xe2),
    ("音量加", 0xe9),
    ("音量减", 0xea),
    ("浏览器后退", 0x224),
    ("浏览器前进", 0x225),
    ("停止加载", 0x226),
    ("刷新", 0x227),
    ("搜索", 0x221),
    ("收藏夹", 0x22a),
    ("浏览器主页", 0x223),
    ("打开浏览器", 0x196),
    ("电子邮件", 0x18a),
    ("媒体播放器", 0x193),
    ("媒体控制设置", 0x183),
    ("计算器", 0x192),
    ("文件管理器", 0x194),
];

pub const SYSTEM: [(&str, u16); 3] = [("电源", 0x81), ("睡眠", 0x82), ("唤醒", 0x83)];

#[derive(Clone, Copy)]
pub enum Shortcut {
    Keyboard(u16, u8),
    Clicks(usize),
}

pub const SHORTCUTS: [(&str, Shortcut); 14] = [
    ("剪切 Ctrl+X", Shortcut::Keyboard(27, 1)),
    ("复制 Ctrl+C", Shortcut::Keyboard(6, 1)),
    ("粘贴 Ctrl+V", Shortcut::Keyboard(25, 1)),
    ("删除 Delete", Shortcut::Keyboard(76, 0)),
    ("查找 Ctrl+F", Shortcut::Keyboard(9, 1)),
    ("全选 Ctrl+A", Shortcut::Keyboard(4, 1)),
    ("重做 Ctrl+Y", Shortcut::Keyboard(28, 1)),
    ("撤销 Ctrl+Z", Shortcut::Keyboard(29, 1)),
    ("新建 Ctrl+N", Shortcut::Keyboard(17, 1)),
    ("打开 Ctrl+O", Shortcut::Keyboard(18, 1)),
    ("保存 Ctrl+S", Shortcut::Keyboard(22, 1)),
    ("打印 Ctrl+P", Shortcut::Keyboard(19, 1)),
    ("左键双击", Shortcut::Clicks(2)),
    ("左键三击", Shortcut::Clicks(3)),
];

// Press and release words, as in the factory table.
pub const FUNCTIONS: [(&str, u16, u16); 17] = [
    ("滚轮向上", 0x262b, NO_OPERATION),
    ("滚轮向下", 0x262c, NO_OPERATION),
    ("DPI 档位上移", 0x2626, NO_OPERATION),
    ("DPI 档位下移", 0x2624, NO_OPERATION),
    // Official mi_PlusMouseSensitivity and mi_MinusMouseSensitivity.
    ("CPI+", 0x2627, NO_OPERATION),
    ("CPI-", 0x2625, NO_OPERATION),
    ("精确模式（按住生效）", 0x2628, 0x2629),
    ("配置上移", 0x2622, NO_OPERATION),
    ("配置下移", 0x2623, NO_OPERATION),
    ("配置循环", 0x262a, NO_OPERATION),
    ("灯光上移", 0x2618, NO_OPERATION),
    ("灯光下移", 0x2619, NO_OPERATION),
    ("灯光循环", 0x261a, NO_OPERATION),
    ("回报率循环", 0x260e, NO_OPERATION),
    ("抬起高度切换", 0x260f, NO_OPERATION),
    ("切换 Y 轴", 0x260b, NO_OPERATION),
    ("锁定右键", 0x260d, NO_OPERATION),
];

/// Record N belongs to button N of the firmware state bits. The A90 names
/// were found with the button query on the mouse.
pub fn record_name(index: usize) -> String {
    match index {
        0 => tr("左键").into(),
        1 => tr("右键").into(),
        2 => tr("中键").into(),
        3 => tr("后侧键").into(),
        4 => tr("前侧键").into(),
        8 => tr("“1”键").into(),
        9 => tr("“C”键").into(),
        10 => tr("“3”键").into(),
        24 => tr("滚轮向下").into(),
        25 => tr("滚轮向上").into(),
        _ => tf!("记录 {}", index + 1),
    }
}

/// Records with a physical button on the A90.
pub const PHYSICAL: [usize; 8] = [0, 1, 2, 3, 4, 8, 9, 10];

