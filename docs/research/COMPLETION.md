# 当前 KDE Wayland 功能检查

用户确定先完成本机 KDE Wayland 上的全部 A90 适用功能。
配置、按键、宏和手势属于本次范围。
截图、桌面画笔和语音可调用 Linux 应用，官方附带工具不要求重做。
X11 和 GNOME 的进一步适配留待后续。
本文记录当前证据和剩余项。完成状态仍待验证。

## 当前设备

2026-10-20，生产命令 `--device` 读到 USB `09da:fafd`、型号 `04fd`、固件 `07`、核心 `2`。
灵敏度区为 `2400`，按键区为 `2600`，常规设置区为 `3e00`。
`--host` 读到 KDE 桌面设置，`--runtime-sensitivity` 可读取当前档位和回报率。
完成验证必须使用当前状态，不能从早期备份推断用户现在的设置。

## 功能与证据

| 要求 | 当前证据 | 剩余检查 |
| --- | --- | --- |
| 设备识别与备份 | 生产命令、完整备份解析及保存前备份路径。 | 检查最终版本的完整流程。 |
| 五档 DPI 和独立 X/Y | 公式与表来自 EXE，保存和即时应用已实现。等轴实机测试见 RUNTIME-SENSITIVITY.md。 | 五档独立 X/Y 的组合尚未实机验证。 |
| 回报率 | 125、500、1000 Hz 实机事件间隔及恢复检查通过。 | 250 Hz 的实际事件间隔未测量。 |
| 按键与七个配置 | 物理按键查询、保存表重建、配置循环和普通映射实机通过。主机动作、两套手势和顶键触发已接入。组合键、办公快捷键、双击三击、媒体和系统键已补齐。左右滚动及新映射检查见 KEY-MENU.md、WHEEL.md。 | 新映射的实体效果尚未实机检查。4-Way Wheel 等菜单项的型号适用条件仍需核对。 |
| 普通宏、重复与条件 | 独立预期字节、实机按键输出、停止和 `ifkey` 两分支记录。 | 汇总最终版本的回归结果。 |
| HighLanguage 与 AMC | 十二个官方文件可编译，读写和语义检查。 | 保留官方兼容性差异说明。 |
| 宏库 | 分类、文件操作、冲突和恢复检查见 LIBRARY.md。 | 无已知实现缺项。 |
| 射击与狙击程序 | 七槽编译和保存已实现，固件入口静态检查见 CORES.md。 | 本机核心 2 缺少左键入口。用户尚未在官方软件中测试。 |
| 抬起高度和校准 | 保存、运行值、校准状态实机读回见 CALIBRATION.md。 | 没有测量物理抬起距离。 |
| 硬件响应时间 | 保存和运行值实机读回。 | 没有测量物理按键延迟。 |
| 灯光亮度 | 关闭和三档实机读回。 | 无已知实现缺项。 |
| 桌面指针、双击与滚轮 | KDE 属性、配置键值、独立 Qt 程序读值及恢复检查通过。已修正错误读取键盘接口速度的问题，见 KDE-SETTINGS.md。 | 未测量人工双击和滚动距离。 |
| 4-Way Wheel | KDE Wayland 窗口区域转换、每应用分界、反向区域、每核心开关、方向图标和统一输入读取器。独立 KWin 完整路径、图标行为及实机启停检查通过，见 WHEEL4D.md。 | 整页滚动、横向字符数和可拖动分隔图标。实际应用中的人工滚动效果。 |
| 应用配置关联 | 前台应用识别、切换及恢复实机通过。独占锁修改后的实机回归也通过。 | 无已知实现缺项。 |
| 配置文件与编辑草稿 | 导入导出、完整配置重建、手势草稿和外部修改冲突测试。版本 2 保留按键主机动作，版本 1 继续可用。 | 无已知实现缺项。 |
| 两套全局手势 | 原数学函数参考、隔离完整路径、实机启停、浮动提示。 | 人工物理画图效果尚未检查。 |
| 手势文字和窗口动作 | 私有 KWin 中的 KWrite 输入保存及窗口状态检查通过。全部最小化、跨桌面窗口及其他桌面排除检查也通过。 | 物理触发键与实际前台应用的组合效果尚未由人工检查。 |
| Linux 附带工具调用 | 独立参数、XML 往返、主机 VM、子进程实际参数和错误转发检查通过。 | 所选外部工具由用户配置。 |
| 通信互斥与输入响应 | 真实 HID 的跨进程只读检查通过。隔离 `ifkey` 等待期间仍转发指针，停止时先释放输入接管。常规检查通过 116 项，Clippy 检查通过。 | 无已知通信互斥缺项。 |
| 厂商状态通知 | 初始状态、独立读取器、配置变化、特征查询并行和模式恢复通过实机检查，见 VENDOR-INPUT.md。 | 实体按键触发实际前台主机动作尚未由人工检查。 |
| 按键主机动作保存与执行 | 文件冲突、硬件失败恢复、配置选择、Ctrl 释放、目标参数和错误恢复检查通过。两套手势和三枚顶键的隔离路径通过。实机启停后闪存及运行配置一致，见 HOST-BUTTONS.md。 | 顶键配合实际应用的人工物理绘图尚未检查。 |

## 仍需分析

官方分发器 `004611a4` 的 `401/402/403/404/406` 分支从配置记录读取主机动作。
`406` 使用 `Object + bec + index × 74` 附加记录区域。
这些分支与现有两套主机手势的 `408/409` 结果消息有不同来源。
当前不能仅凭全局手势已经可用，就把厂商通知触发路径列为完成。
已确认普通按键状态通知可用，必须先临时启用通知模式。
固件 `0a2d..0a31` 把通用附加动作通知字段写零，不能直接采用其他型号的 `406` 路径。
普通按键的打开目标、文字、缩放和窗口动作已实现。
按键直接指定两套手势及顶键触发已实现。
仍需核对其他官方按键菜单及 162 对附加动作的适用范围。
已确认的普通菜单及 Windows 与 Linux 的差异见 KEY-MENU.md。
Evolutionary Wheel / 4-Way Wheel 已接入主机区域转换。
整页滚动、水平字符数和可拖动分隔图标仍需实现，见 WHEEL4D.md。
当前保存器继续保留这些附加动作。

RGB 灯效和无线设置不适用于本机。
位移宏的授权限制和核心 2 的左键入口缺失属于已发现的设备限制，见 WRITES.md 和 CORES.md。
不能通过未验证的固件或授权写入来代替驱动功能验证。

## 当前回归命令

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --check
cargo test --locked --test device_access -- --ignored
cargo test --locked --lib gesture_runtime::tests::isolated_evdev_to_gesture_to_uinput -- --ignored
cargo test --locked --lib button_actions::tests::wheel_keyboard_combination_emits_and_releases_all_uinput_keys -- --ignored --nocapture
cargo test --locked --lib gesture_runtime::tests::isolated_native_top_keys_and_logical_trigger_share_one_source -- --ignored --nocapture
cargo test --locked --lib host_button_runtime::tests::native_frames_select_profiles_generate_zoom_and_launch_literal_arguments -- --ignored --nocapture
env A90_TEST_HOST_BUTTONS=1 cargo test --locked --lib host_button_runtime::tests::native_runtime_keeps_feature_access_excludes_duplicate_owners_and_restores_mode -- --ignored --nocapture
env A90_TEST_KDE_HOST=1 cargo test --locked --lib host::tests::kde_settings_write_read_and_restore -- --ignored --nocapture
env A90_TEST_NATIVE_INPUT=1 cargo test --locked --test vendor_input -- --ignored --nocapture
env A90_TEST_ASSOCIATIONS=1 cargo test --locked --bin bloody_a90 worker::tests::associations_switch_and_restore -- --ignored --nocapture
bash contrib/research/check-window-actions.sh
env A90_TEST_WHEEL_CAPTURE=1 cargo test --locked --lib gesture_runtime::tests::physical_wheel_only_start_stop_keeps_device_settings -- --ignored --nocapture
```

实机写入检查必须先读取新备份，并在结束时恢复闪存及运行参数。
不应把纯编译结果、单元测试或旧的测试说明当作整体验证完成的证据。
