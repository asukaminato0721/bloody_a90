# 手势应用缩放

## 原程序证据

EXE 的 SHA-256：`4a76f6ef4b6e7da699afbd8d3d780ae9747b599af795531a39e3eb5d93c93162`。
缩放 DLL 的 SHA-256：`0d094f59da17e3c66c4d31d536b58c01548b2edb3105eda75290580ab87a1e8f`。
本次只做静态分析。项目不包含官方 EXE 或 DLL。

动作分发器 `0045cd4c` 的 `Simulator=22` 分支调用 `00465770`。
`Object=0` 传入方向 `+1`，其他值传入 `-1`。次数为 1。
构造函数 `00465628` 加载 `DLL\DLL_ZoomControl.dll`。
`00465770` 查找导出函数 `GoZoomEvent`，然后传入方向和次数。

DLL 为 32 位 PE，映像基址为 `10000000`。
唯一导出 `GoZoomEvent` 的 RVA 为 `48d0`。
它先读取前台窗口，再按 Windows 窗口类别选择处理函数。
不匹配已知类别时不输出事件。

| 地址 | 已确认行为 |
| --- | --- |
| `100017a0` | Ctrl 按下、`Sleep(1)`、每次输出一格 `±120` 滚轮、Ctrl 释放 |
| `10001990` | 向指定窗口发送 `WM_MOUSEWHEEL=0x20a`，步长 `±120` |
| `100029c0` | 按方向发送数字小键盘加号 `VK_ADD=0x6b` 或减号 `VK_SUBTRACT=0x6d` |
| `10001ec0` | 向指定窗口发送 `WM_COMMAND`，命令为 `0x22e` 或 `0x22f` |
| `10001f10` | Ctrl 配合滚轮，每次输出三格 |

`100017a0` 的次数从 EAX 传入。Ghidra 默认原型把它显示为 `in_EAX`，不能将其当作未初始化值。
EXE 对两种手势结果都传入阶段 2。因此此类动作只有 `Action=3` 会执行。

## Linux 动作

Linux 使用 Ctrl 配合一格滚轮，保留方向、次数和 1 ms 的等待。
主机 VM 使用现有键盘、延迟和滚轮指令执行，输出经 uinput 进入桌面。
高分辨率滚轮同时生成 `±120` 的伴随事件。
正常结束、中断和退出都使用现有的按键释放路径。

Windows 专用窗口消息没有直接对应的 Linux 接口。
此实现不复制 Windows 的窗口类别表。应用必须支持 Ctrl+滚轮缩放。
使用其他绑定的应用可设置现有键盘动作或 AMC 宏。
此动作的作用范围是应用内容，不是显示器分辨率或桌面放大镜。

## 验证

- 单元检查覆盖 `Object=0`、其他非零值、`Action=0..3` 和错误阶段值。
- 时间检查确认 Ctrl 先按下，VM 等待 1 ms 后输出滚轮并释放 Ctrl。
- 完整隔离输入链路检查实际 uinput 事件，确认 Ctrl 下、滚轮、Ctrl 上的时间顺序，以及最终释放状态。
- 独立 Xvfb 中的 Qt `QPlainTextEdit` 收到 `+120` 和 `-120`，两次均带 Ctrl。
- Qt 自身完成字体缩放。测试只记录变化，确认字体增大后恢复原值，并检查 Ctrl 已释放。

Qt 对 Ctrl+滚轮的行为见 [QPlainTextEdit 文档](https://doc.qt.io/qt-6/qplaintextedit.html#read-only-key-bindings)。
窗口检查需要 C++ 编译器、`pkg-config`、Qt6Widgets 开发文件和 Xvfb。
测试只操作自身窗口，不连接用户桌面窗口。

```sh
cargo test --locked --lib host_program::tests::zoom_direction_delay_and_result_phase
cargo test --locked --lib gesture_runtime::tests::isolated_evdev_to_gesture_to_uinput -- --ignored
dbus-run-session -- xvfb-run -a cargo test --locked --test zoom -- --ignored --nocapture
```

尚未由人工使用物理鼠标在实际桌面应用中检查缩放效果。
