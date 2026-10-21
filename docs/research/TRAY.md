# 托盘图标

本记录使用 PROTOCOL.md 中的 EXE。

## 官方程序

托盘窗体为 `TTrayForm`（资源 `MAIN_SystemTray`）。`FormCreate` `0083f630` 载入七个图标：
`Data/Mouse/Common/Profile` 的 `1`…`5`、`Disable`、`Enable` 等，鼠标可用时为红色手形，不可用时为灰色。

菜单由 `0083b764` 按字符串表建立 17 项，每项格式为
`名称,图标,语言节,标题键`，语言节为 `MainApp.TrayIconPopupMenu`。顺序为：

| 序号 | 名称 | 显示条件（`0083ce9c` 的标志位） |
| ---: | --- | --- |
| 1 | `mi_LaunchOnLogin` 开机自动执行 | `1` |
| 2 | `mi_KeyboardControlPanel` | `400`，且有键盘 |
| 3 | `mi_MouseControlPanel` | `2` |
| 4 | `mi_MultiDeviceManage` 多装置管理 | `4` |
| 5 | `mi_PowerManagement` | `800` |
| 6 | `mi_ChannelSetting` | 不显示 |
| 7 | `mi_DevicePair` 对码程序 | `80` |
| 8 | `mi_MouseLed` 开启功能显示 | `100` |
| 9 | `mi_W4DSetting` 进化轮设定 | `200` |
| 10 | `mi_TroubleShooting` 故障排除 | `8` |
| 11 | `mi_UpdaterSetting` 更新设定 | `1000` |
| 12 | `mi_AdvancedFeatures` 进阶功能 | `2000` |
| 13 | `mi_AdvancedSettings` 进阶设定 | 视条件 |
| 14 | `mi_FactoryDefault` 恢复出厂默认设置 | 不显示 |
| 15 | `mi_TrayLaunch` 开启 %P | `10000` |
| 16 | `mi_ProgramExit` 离开 | `20` |
| 17 | `mi_ProgramClose` 游戏防打扰 | `40` |

`0083c7e4` 为“开启功能显示”建立子菜单（永远显示、改变时显示、不显示），见 [STATUS-WINDOW.md](STATUS-WINDOW.md)。

## 本工具的实现

`src/app/tray.rs` 通过 StatusNotifierItem（`ksni`，使用 zbus 的 async-io）提供托盘图标。
菜单按官方顺序保留适用于有线 A90 和 Linux 的项目：开机自动执行、开启功能显示、进化轮设定、开启 Bloody A90、离开。
无线、多装置、更新、故障排除、游戏防打扰和恢复出厂设置不适用或没有对应功能，未加入。
图标由代码绘制（鼠标轮廓），连接鼠标时为红色，未连接时为灰色；左键单击打开主窗口。

关闭主窗口时程序留在托盘。窗口真正关闭，`App` 由 `main` 持有并继续运行：
工作线程、按键主机动作、手势和状态浮窗不受影响，应用关联等轮询在无窗口循环中继续。
“开启”或“进化轮设定”在同一个 `App` 上重新创建窗口（eframe 复用 winit 事件循环），
新窗口重新应用样式和字体，工作线程通过共享的 `Wake` 唤醒当前窗口。
“离开”保存状态并结束程序。没有 StatusNotifierWatcher 时不创建托盘，关闭窗口即退出，与之前相同。

## 检查

2026-10-21，KDE Plasma Wayland：

- 托盘项注册到 `org.kde.StatusNotifierWatcher`，dbusmenu 布局与上表顺序一致，勾选和单选状态正确。
- 用 KWin 脚本关闭窗口后进程保留，窗口列表中不再有主窗口。
- D-Bus `Activate` 重新打开窗口，样式、字体和已连接的鼠标状态保留。
- 窗口关闭时选择“永远显示”立即显示状态浮窗并写入状态文件；“进化轮设定”打开 灵敏度 → 滚轮 页。
- 窗口打开和关闭时选择“离开”都会结束进程。
