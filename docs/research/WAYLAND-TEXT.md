# Wayland 虚拟键盘文字输入

`host_text::virtual_keyboard` 实现 `Simulator=21` 的第二条文字输出路径。
程序先检查 KWin EIS。该接口不可用时，再检查 `zwp_virtual_keyboard_manager_v1`。
这条路径不需要 libei，不使用剪贴板。

## 输入方式

每段文字生成一份 XKB 映射，每个不同的 keysym 使用一个键码。
映射属于新建的虚拟键盘。它不替换物理键盘的映射。
键类型只有一个级别，按键重复关闭，修饰状态为零。
文字的大小写和字符不依赖物理布局或 Caps Lock。
换行发送 Return，制表符发送 Tab，CR 继续按官方行为跳过。

映射使用匿名 memfd 传递。客户端按协议先发送映射，再发送按键。
每个字符发送按下和释放，使用同一个单调时钟。
每段文字开始前更新映射，所以多段文字可使用不同字符。
Wayland 的键码不受 X11 核心协议的 255 上限限制。
当前文字仍受 64 KiB 的动作文本上限约束。

输入线程的队列和中断机制与 KWin 路径相同。
线程在字符之间检查中断，每次发送都包含按下和释放。
退出时销毁虚拟键盘。连接断开或座席移除会结束文字线程，并报告错误。
初始化用两次有期限的同步请求检查注册表、创建权限和键盘映射。
每次同步最多等待两秒。初始化不发送按键，也不接管鼠标。
当前实现只接受一个座席，多座席会在启用前报告错误。

协议依据为 [Virtual keyboard XML](https://github.com/Smithay/wayland-rs/blob/master/wayland-protocols-misc/protocols/virtual-keyboard-unstable-v1.xml)。
服务端按键与销毁行为参考 [wlroots 实现](https://github.com/swaywm/wlroots/blob/master/types/wlr_virtual_keyboard_v1.c)。

## 实际编辑器检查

2026-10-20，独立 Sway 1.12、wlroots 0.20.2 和原生 Wayland Qt 编辑器检查通过。
测试链路为动作、主机 VM、文字线程、虚拟键盘协议、Sway 和 `QPlainTextEdit`。
编辑器只由真实按键事件修改文字，测试代码读取它保存的 UTF-8 文件。

检查包含：

- `A中😀`、Return、Tab 和后续字符的完整内容。
- 第二段文字的 300 个不同汉字，验证大于 255 的键码和映射切换。
- 开始输入 65536 个字符后中断。已输入内容停止增长，新请求仍可输入。
- 退出后 Sway 的输入设备列表恢复为空，确认虚拟键盘已销毁。
- libxkbcommon 编译生成的映射，并逐项检查超过 1024 个符号的查找结果。
- 服务端不响应时按期限返回错误。连接断开后不继续等待。

运行方式：

```sh
bash contrib/research/check-virtual-keyboard.sh
```

脚本需要 Sway、swaymsg、Qt6Widgets 开发文件、C++ 编译器和 `dbus-run-session`。
它建立独立总线、临时配置和 headless 显示，使用 pixman 渲染，不连接物理输入设备。
脚本最多运行 60 秒。成功时清理进程和临时目录，失败时保留日志。
可通过 `A90_SWAY_BIN` 和 `A90_SWAYMSG_BIN` 指定测试程序路径。

本机测试把官方 Arch 软件包解压到缓存目录，没有安装或替换系统组件。
软件包 SHA-256 与本机仓库数据库一致：

| 软件包 | SHA-256 |
| --- | --- |
| sway 1:1.12-4 | `5704cfc9eba9804116c665cb4450352bf5a240c1a8300b36aca3218116648443` |
| wlroots0.20 0.20.2-1 | `8b1da3fbf29cc45908d8de771bd22a5813b298a9049462df5b8952f23c3ffb8f` |
| libliftoff 0.5.0-1 | `59cf08c21500673a14287b83b89b6db5389134cc24c97d9222c7cddefa639ccf` |

## 仍未支持的接口

X11 XTEST 只发送键码，没有直接提交任意 Unicode 字符的请求。
动态改写公共键盘映射存在客户端读取映射的时序问题，当前没有采用此路径。
接口说明见 [XTEST 协议](https://xorg.freedesktop.org/archive/X11R7.7/doc/xextproto/xtest.html)。

RemoteDesktop 的 `NotifyKeyboardKeysym` 也不能直接证明任意 Unicode 输入可用。
Mutter 的实现会在当前键盘布局查找键码，找不到时记录警告并跳过。
因此不能把成功的 D-Bus 调用当作中文或表情符号已输入的证据。
见 [Mutter 按键实现](https://github.com/GNOME/mutter/blob/main/src/backends/native/meta-virtual-input-device-native.c)。
GNOME 和其他未提供上述文字接口的桌面仍需单独实现和验证。
