# 原程序按键菜单核对

此记录使用 PROTOCOL.md 中的 EXE，目标为当前 A90 和 KDE Wayland。
语言文件为 `Data/Texts/LanguageIni/English/BloodyWorkShop8_English.ini`。
`MainApp.TriggerSetPopupMenu` 列出菜单名称。
名称不能单独证明型号支持，必须同时检查赋值转换、分派和固件路径。

## 当前实现

| 原菜单类别 | Linux 实现 | 当前检查 |
| --- | --- | --- |
| 键盘及修饰键 | 按键页支持普通键，以及八个左右 Ctrl、Shift、Alt、Super 选项。也可只按修饰键。 | 独立预期字、保存表往返、界面重新选择及隔离 uinput 释放检查通过。 |
| 鼠标五键、上下滚轮 | 保留硬件映射。 | 既有实机结果见 WRITES.md。 |
| 左右滚轮 | 按键和两个滚轮方向均可指定主机水平滚动。每次按下或每格滚动执行一格。 | 通知解析、配置选择、普通及高分辨率水平事件的隔离路径通过。 |
| 办公快捷键 | 剪切、复制、粘贴、删除、查找、全选、重做、撤销、新建、打开、保存、打印。 | 两阶段键盘编码、组合键恢复和配置往返检查通过。 |
| 双击、三击 | 完整硬件宏，采用原程序的 64 ms 按住时间和间隔。释放记录为空操作，避免提前停止。 | 独立预期字及配置往返检查通过。 |
| 多媒体和浏览器 | 20 项 Consumer 选项，含七个播放和音量键、浏览器、邮件、播放器、计算器、文件管理器及媒体控制设置。 | 编码检查、播放器和文件管理器的 VM 及隔离手势路径通过。 |
| 电源、睡眠、唤醒 | 使用 System usage `81/82/83`。 | 仅检查编码和界面，不执行这些系统动作。 |
| DPI、配置、灯光和 Y 轴 | 保留原硬件指令，补充灯光循环和锁定右键选项。 | 指令来自 EXE，宏存储检查通过。新增功能键的实体效果仍需检查。 |
| 打开目标、文字、缩放、窗口 | 使用已有主机服务。新增当前桌面及活动的全部最小化。 | 全部最小化的独立 KWin 窗口状态检查通过。 |
| 手势、宏、射击切换 | 使用既有两套手势、AMC/M 编译器和七槽射击设置。 | 验证范围见对应记录。 |

普通按键的组合键分成按下和释放程序。
修饰键按位从低到高按下，主键先释放，修饰键从高到低释放。
主键本身为修饰键时只生成一次按下。
滚轮把两阶段合并，并在键盘释放前等待 16 ms。
选中已保存的键盘组合时，编辑器会恢复主键和修饰键。
其他宏只有在完整匹配已知结构时才显示为快捷操作，不会据部分字值推断用途。

## 静态依据

`00814aa8` 把 Simulator、Object 转换为界面文字。
它确认 Consumer `0183` 在原程序中显示为媒体播放器，`0194` 显示为“我的电脑”。
`0062f6c8` 的 `6/7/8` 分支分别生成键盘、Consumer 和 System 指令。
Simulator `23` 的 Object `0/1` 生成双击和三击。
该分支的 Object `2` 包含压枪位移，受当前设备的位移授权限制。
Object `3` 是输入厂商网站地址的 Windows 宏，可用 Linux 的打开目标代替。

Simulator `24` 的 Object `0` 在转换器中生成以下 Windows 宏：

```text
modifierkeydown 8
Delay 10 ms
KeyDown 16
Delay 150 ms
KeyUp 16
Delay 10 ms
modifierkeyup 8
MacroEnding
```

它表示 Win+M。文字格式器将该值标为“全部最小化”。
主机分派器 `0045cd4c` 的 Object `0` 分支不执行窗口动作，因为 Windows 路径使用上述硬件宏。
Linux 的主机动作使用相同的 `Simulator=24, Object=0` 表示对应桌面操作。
它不向 KDE 发送 Windows 的 Win+M。

KWin 实现检查每个窗口的可最小化能力、虚拟桌面和活动。
只有当前桌面、当前活动及跨桌面窗口会被处理。
它直接设置最小化状态，不切换“显示桌面”状态，也不恢复已最小化的窗口。
独立测试覆盖 XWayland 和原生 Wayland 窗口、其他桌面排除及跨桌面窗口。
KWin 属性依据见 [脚本 API](https://develop.kde.org/docs/plasma/kwin/api/)。
X11 的全部最小化尚未实现，会报告桌面不支持。当前范围为 KDE Wayland。

Linux 的 HID Consumer 映射将 `0183` 解释为 `KEY_CONFIG`。
新建的播放器设置使用 `0193`，对应 `KEY_PLAYER=387`。
原值仍可选择为“媒体控制设置”，不会改写已有宏或手势文件。
`0194` 对应 `KEY_FILE=144`。
映射依据见 [Linux hid-input.c](https://github.com/torvalds/linux/blob/master/drivers/hid/hid-input.c)，数值来自本机 `linux/input-event-codes.h`。
实际启动哪一个应用，由桌面的快捷键和默认应用设置决定。

复现静态分析：

```sh
/opt/ghidra/support/analyzeHeadless /tmp bloody-a90-research \
  -process BloodyWorkShop8.exe -noanalysis -readOnly \
  -scriptPath contrib/research \
  -postScript A90Decompile.java /tmp/a90-key-menu.c 814aa8 62f6c8 45cd4c
```

## 检查与剩余范围

2026-10-21，108 项常规检查通过。
Clippy、格式和构建检查通过。
隔离 uinput 检查确认 Ctrl、Alt、右 Super 和 X 均按预期顺序释放，主键至少按住 16 ms。
隔离厂商通知检查确认两个配置可以分别把滚轮映射为向左和向右。
新功能没有在本轮写入实体鼠标，也没有执行电源或睡眠。
实体按键、滚轮和用户选定应用的组合效果仍需人工检查。

菜单核对还发现 Evolutionary Wheel / 4-Way Wheel 设置。
其配置包含竖直整页滚动、水平字符数和浮动图标。
这些功能在 A90 上的启用条件及 KDE 对应实现仍需继续核对。
不能把普通滚轮和左右滚动已经可用，当作这组功能全部完成的证据。
162 对附加动作的型号适用范围也仍待确认。

```sh
cargo test --locked --lib button_actions::tests::wheel_keyboard_combination_emits_and_releases_all_uinput_keys -- --ignored --nocapture
cargo test --locked --lib gesture_runtime::tests::isolated_evdev_to_gesture_to_uinput -- --ignored --nocapture
cargo test --locked --lib host_button_runtime::tests::native_frames_select_profiles_generate_zoom_and_launch_literal_arguments -- --ignored --nocapture
bash contrib/research/check-window-actions.sh
```
