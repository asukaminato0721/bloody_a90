# Bloody A90

使用 Rust、egui 和 eframe 开发的独立 A90 配置工具。
它读取并写入 A90（USB `09da:fafd`，型号 `04fd`，固件 07）的灵敏度、按键和宏，
并控制灯光亮度。协议来自此前对官方 EXE 和固件的分析，写入流程已在实机上验证。
当前完成目标是本机 KDE Wayland 上的全部 A90 适用功能。
范围和剩余验证见 [功能检查](docs/research/COMPLETION.md)。

## 设备权限

工具通过 hidraw 访问鼠标的厂商接口。安装 udev 规则后，当前桌面用户即可访问：

```sh
run0 install -m644 contrib/70-bloody-a90.rules /etc/udev/rules.d/
run0 udevadm control --reload
run0 udevadm trigger --action=change --subsystem-match=hidraw
run0 udevadm trigger --action=change --subsystem-match=input
run0 modprobe uinput
run0 udevadm trigger --action=change --subsystem-match=misc
```

全局手势还需要读取 A90 的鼠标事件，并通过 uinput 生成输入。
上述规则把这些权限授予当前本地桌面用户。
uinput 允许该用户生成桌面按键和鼠标事件。
手势文字输入支持 KWin EIS，或提供虚拟键盘接口的 Wayland 桌面。
KWin EIS 路径需要 libei 1.6。虚拟键盘路径已在独立 Sway 会话中验证，不需要 libei。
启用前会检查文字接口。当前 X11 后端尚不支持文字动作。

## 运行

```sh
cargo run
```

界面沿用官方 Bloody 8 的布局和配色：左侧为设备与六个主选项卡，
右侧为子选项卡、功能说明和设置页。尺寸、颜色和中文标签取自官方
`Data/Mouse/Forms/*/UI.txt` 与语言文件。
A90 不适用的无线设置和 RGB 灯效保持禁用。

启动时若找到鼠标，程序会读取整片闪存，并把本次会话的第一份读取结果保存到
`~/.cache/bloody-a90/backup-<时间>.bin`，然后再允许写入。

| 页面 | 功能 |
| --- | --- |
| 按键 | 七个模式的按键动作：鼠标按键、键盘按键、多媒体、功能、射击模式、宏、主机动作和禁用。主机动作支持打开目标、文字、缩放、窗口操作和两套手势。三枚顶键可直接触发手势。滚轮方向可指定其他主机动作。按键查询会在按住物理按键时选中对应记录。 |
| 灵敏度 → 分辨率 | 1 至 5 档、独立 X/Y 轴，100 至 6200 DPI。保存后立即生效。 |
| 灵敏度 → 回报率 | 125、250、500、1000 Hz。保存后立即生效，并保留当前 DPI 档位。 |
| 灵敏度 → 微精校准 | 抬起高度 1 至 16 级、启用开关、五秒表面校准。应用后立即生效。 |
| 灵敏度 → 按键响应 | 硬件响应时间 0 至 16 ms，保存并立即应用。桌面双击速度单独保存。 |
| 灵敏度 → 滚轮、指针 | 滚动行数、指针速度。官方程序写入 Windows，这里写入 KDE Plasma 或 GNOME 的设置。 |
| 编程设置 → 左键编程、狙击键 | 七个射击槽位，A 连击、B/C/D 循环或奥斯卡宏。可保存并指定切换按键。界面检查固件入口，本机核心 2 缺少左键射击入口。见 [CORES.md](docs/research/CORES.md)。 |
| 其他设置 → 应用关联 | 按 KDE Wayland 或 X11 前台应用切换配置，支持默认配置。关闭工具后停止自动切换。 |
| 其他设置 → 手势 | 编辑两套官方文件，预览识别结果，配置全局触发键。支持键盘、常用媒体键、鼠标按键、滚轮、主机 AMC 宏、应用缩放、打开目标和指针下窗口的最小化、最大化。KWin EIS 和 Wayland 虚拟键盘后端支持文字输入，已分别在 KWin 和 Sway 会话中检查。普通手势可显示 Wayland / X11 浮动提示。完整隔离路径和实机启停检查通过。见 [GESTURE-RUNTIME.md](docs/research/GESTURE-RUNTIME.md)。 |
| 编程设置 → 奥斯卡II | M 型宏文本编辑和编译，导入导出官方 AMC 文件（含 HighLanguage）。“测试”写入后侧键，“中断宏”停止正在运行的宏。 |
| 编程设置 → 奥斯卡III | 按分类管理 AMC 宏。支持新建、保存、另存为、重命名、删除及恢复上次删除。下方编辑器可修改宏。 |
| 灯光设置 → 灯光亮度 | 关闭和三档亮度，立即生效。 |
| 其他设置 | 关于软件、关于硬件、备份解析，以及配置文件导入导出。 |

编辑器状态（DPI 草稿、宏文本、射击参数、手势草稿、文件路径）在关闭时保存到
`~/.config/bloody-a90/state`。
宏库默认位于 `~/.local/share/bloody-a90/macros`。可在奥斯卡III 页面选择其他目录。
分类使用子目录，宏使用 AMC 文件，删除项保留在该宏库的 `.trash` 目录。
手势文件载入后可直接保存。改变路径可另存为新文件。
保存会拒绝外部已修改的原文件，也不会覆盖未载入的现有文件。
关闭程序后会保留手势草稿。再次修改原文件前，需要重新载入该文件。
普通触发键需点击“应用全局设置”启用。已保存的按键手势随主机动作服务启用。
关闭工具会解除输入接管。
同时按下右键和中键可停止全局手势。

在按键页选择“主机动作”，填写内容后点击“设定此按键”，再点击“应用”。
程序把对应硬件记录设为空操作，并通过厂商通知执行主机动作。
选择手势后，按住该键绘图。设定此按键时载入手势页中的两套方向动作。
“应用全局设置”会更新已保存的按键手势。“停止全局手势”会停止两种触发方式。
主机动作保存在 `~/.config/bloody-a90/host-buttons.xml`，需要本程序保持运行。
设置可随版本 2 配置文件导入和导出。详细行为与检查结果见
[按键主机动作](docs/research/HOST-BUTTONS.md)。

滚轮方向也可选择硬件键盘、鼠标、多媒体、功能或单次宏。
滚轮宏每格执行一次按下程序和释放程序，不使用按住或开关重复。
编码依据和检查范围见 [滚轮动作](docs/research/WHEEL.md)。

截图、桌面画笔和语音功能可调用已安装的 Linux 应用。
在手势页选择“打开文件或程序”，填写可执行程序的完整路径。
在参数框中每行填写一个参数。空格和引号按原文传递，不需要添加外层引号。
空行表示空参数，末尾换行只结束上一行。参数框留空时保留原来的打开行为。
设置后点击“替换当前方向的动作”，再点击“应用全局设置”。
载入的官方截图、画笔和语音动作需按此方法替换。
工具不会自动选择应用，也不会改写这些官方动作。
替换后的动作在识别时启动一次。区域选择、画笔开关和语音录制由所调用的应用处理。
官方附带的编辑器和语音程序不在本项目中重做。

## 命令行

命令行功能不创建图形窗口：

```sh
cargo run -- --help
cargo run -- --device                         # 身份、内存信息和亮度
cargo run -- --device-backup PATH              # 读取整片闪存
cargo run -- --save-dpi 800,1600:1200,3200 2   # 写入三档，当前第 2 档
cargo run -- --save-rate 1000                  # 写入回报率
cargo run -- --save-debounce 8                 # 保存并应用硬件响应时间
cargo run -- --calibrate-lift 5                # 移动鼠标五秒，读取校准等级
cargo run -- --save-lift 12                    # 保存并应用抬起高度等级
cargo run -- --foreground                     # 读取前台应用标识
cargo run -- --select-profile 2               # 切换运行配置，不写闪存
cargo run -- --save-lift off                   # 关闭抬起高度限制
cargo run -- --set-key 11 A                    # 把“3”键设为键盘 A（全部模式）
cargo run -- --set-macro 11 macro.txt          # 把“3”键设为 M 文本宏
cargo run -- --save-buttons                    # 以当前生效的按键表重建并写入
cargo run -- --interrupt                       # 停止正在运行的宏
cargo run -- --host                            # 显示桌面的指针、双击和滚轮设置
cargo run -- --restore PATH                    # 恢复与备份不同的可写页
cargo run -- --watch-buttons 30                # 显示按住的按键编号
cargo run -- --watch-native-input 30           # 监听厂商按键、滚轮和配置通知
cargo run -- --amc FILE.amc                    # 编译官方宏文件
cargo run -- --inspect BACKUP                  # 解析备份
```

`--watch-native-input` 可监听 1 至 300 秒。命令会临时启用厂商通知，结束时恢复原模式。
监听期间独占配置接口。其他配置操作需等到监听结束。
通知使用独立的只读句柄，不使用连续 RAM 查询。详见
[厂商通知记录](docs/research/VENDOR-INPUT.md)。

## 写入安全

写入只使用官方程序的保存流程：打开访问、按 16 字分块写入、单独提交 `a4a4`、
关闭访问，然后读回整个范围核对。传输层只允许写入：

- 灵敏度页 `2400` 至 `24ff`；
- 按键区 `2600` 至 `3cff`。`3d00` 页含固件代码，按页擦除时不能触及。

保存按键表前会检查鼠标运行中的配置序号，防止新表的配置数量少于它。
本工具用独占锁保护厂商 HID 的特征请求，覆盖整个配置读取或保存操作。
其他操作最多等待一秒。接口仍被占用时，操作会报告错误，不发送查询。
含 `ifkey` 的主机宏使用独立线程读取物理按键，等待期间继续转发普通鼠标输入。
命令 `03`、`1e`、`2a` 不会发送。原因和实机测试记录见
[docs/research/WRITES.md](docs/research/WRITES.md)。
命令 `0d` 用于即时应用 DPI 和回报率，已检查移动事件、运行值和恢复结果。
鼠标的硬件回报率设置仍可能在重新插入后覆盖保存值，见
[RUNTIME-SENSITIVITY.md](docs/research/RUNTIME-SENSITIVITY.md)。

## 已迁入的实现

| 模块 | 内容 |
| --- | --- |
| `device` | hidraw 传输、身份、内存信息、亮度、RAM 和闪存读取、受限的保存与恢复。 |
| `vendor_input` | 厂商状态通知解析、独立读取器，以及临时启用后恢复的诊断会话。 |
| `host_buttons`、`host_button_runtime` | 按键及滚轮主机动作、按配置选择、文件保存检查、通知执行和模式恢复。 |
| `sensitivity` | 传感器 ID 3327 的 DPI 换算、灵敏度块构建和反向查找。 |
| `macros` | 普通 M 型文本：按键、延迟、位移、绝对移动、变量、条件、`gowhile`、`ifkey`、跳转和重复方式。 |
| `highlanguage` | HighLanguage 到 M 文本的转换，含 `if`、`ifkey`、`repeat` 块和标记跳转。 |
| `amc` | 官方 AMC 宏文件的读取、编译和写出。 |
| `library` | AMC 宏库分类、文件保存、重命名和删除恢复。 |
| `gestures`、`gesture_file` | 两套手势 XML 的读写、动作设置、释放识别，以及文件保存检查。 |
| `gesture_runtime`、`linux_input`、`host_program` | 全局触发、输入转发、uinput 输出和主机宏执行。 |
| `gesture_tip`、`host_open` | 浮动手势提示，以及文件、程序和 URL 的异步打开。 |
| `host_text` | KWin EIS 和 Wayland 虚拟键盘文字输入、输入队列和中断处理。libei 按需加载。 |
| `host_window` | 指针下窗口的最小化、最大化。使用 KWin 脚本或 X11 窗口管理协议。 |
| `storage` | 宏指令宽度、边界与跳转检查、宏池、指令列表和校验和。 |
| `configuration` | 1 至 7 个配置的主向量构建，分配顺序与 EXE `00619b60` 一致。 |
| `shooting` | 射击程序 A 类连击和 B、C、D 类循环，含特殊序列。 |
| `backup` | 备份解析，以及从出厂表或保存表转换为可编辑配置。 |
| `profile_file` | 配置文件：灵敏度块和按键主向量。 |
| `host` | 桌面的指针速度、双击时间和滚动行数（KDE Plasma、GNOME）。 |
| `app`、`skin`、`worker` | 仿官方界面和后台设备线程。 |

## 检查

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --check
```

宏测试包含原 Python 编译器生成的独立预期字节，见 [tests/fixtures](tests/fixtures)。
配置测试逐字比较旧 Python 检查中的 821 字主向量，并用备份解析器读回。
`A90_BACKUP=路径 cargo test --locked --test configuration real_backup_round_trip -- --ignored`
用真实备份检查出厂表的转换和重建。
图形启动检查依次绘制全部页面，再退出。

```sh
cargo run -- --smoke-test
```

Linux 中文显示优先使用系统的 Noto Sans CJK 字体。
静态研究记录位于 [docs/research](docs/research)。
