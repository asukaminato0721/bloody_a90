# 状态浮窗

本记录使用 PROTOCOL.md 中的 EXE。只读取鼠标 RAM，不写闪存，也不改变通知模式。

## 官方程序

窗体为 `TformMouseStatusDialog`，皮肤为 `Data/Mouse/Forms/MouseLEDEx`。
嵌入的 DFM 为无边框、置顶窗口。控件包括：

| 控件 | 内容 |
| --- | --- |
| `im_Profile` | 配置图标。皮肤中有红、绿、黄三色和默认白色手形图。 |
| `im_ShootIndexKey` | 射击模式图标。 |
| `lb_CPIX`、`lb_CPIY` | 当前 X/Y CPI，颜色为 `CPI_FontColor`（红）。 |
| `lb_AccurateCPI` | 精确模式 CPI。 |
| `tm_CPI`、`tm_Profile`、`tm_Shoot` 等 | 变化后的显示定时器。 |

托盘菜单 `mi_MouseLed`“开启功能显示”有三项：永远显示、改变时显示、不显示。
`00502384` 根据状态字决定显示或隐藏窗口。状态字位 1 表示启用，位 2 表示永远显示。
改变时显示模式下，它把各定时器间隔设为 3000 ms。DFM 中写的是 5000。
定时器处理函数（如 `tm_CPITimer` `00504744`、`tm_ProfileTimer` `005046dc`）
清除对应的变化标志后再次调用 `00502384`。没有剩余标志时窗口隐藏。

## 本工具的实现

`src/status_window.rs` 显示配置序号、该配置保存的灯光颜色和当前档位的 X/Y CPI。
图标由代码绘制，不使用官方图片。颜色取自按键表配置元数据（见 PROFILES.md）。
选项位于 其他设置 → 关于软件 →“开启功能显示”，并保存在状态文件中。

通知模式由按键主机动作服务管理，固件重新加载时也可能关闭该模式。
因此浮窗不启用通知，而是每 300 ms 用非阻塞句柄读取 RAM `0263`、`0027`、`002a`。
接口被保存操作或其他进程占用时跳过本次读取，并保留显示值。
这三个字节变化时，或距上次完整读取超过 3 秒时，再读取完整运行灵敏度。
通知模式已经开启时，收到的配置或档位变化会让下一次读取提前进行。

窗口使用 `src/overlay` 的点击穿透表面：Wayland 用 layer-shell Overlay 层锚定右下角，
X11 用 override-redirect 窗口。官方窗口可以拖动和右键关闭，此实现暂不支持。
射击模式图标和精确模式 CPI 尚未显示：A90 状态报文第 13 字节的射击状态与官方图标的对应关系仍未验证。

## 实机检查

2026-10-21，固件 `0c`、核心 1，KDE Wayland：

```sh
A90_TEST_STATUS_WINDOW=1 A90_TEST_STATUS_SECONDS=20 \
  cargo test --locked --lib status_window_follows_the_mouse -- --ignored --nocapture
```

浮窗显示在右下角面板上方，内容为配置 4（蓝色）和 3200 × 3200，
与 `--runtime-sensitivity` 和 `--ram 263 1` 的结果一致。测试期间没有报错。
同一次检查中，用户按下 DPI 键和配置切换键，但鼠标灯色和速度都没有变化。
另行每 0.1 秒读取 RAM，`0027` 和 `0263` 在 30 秒内保持不变，`025e..0262` 没有按住或待切换的状态。
因此本次未能实机验证浮窗随按键更新。
