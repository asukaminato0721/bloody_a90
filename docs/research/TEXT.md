# M 型宏的普通文本编译

`macro_text.py` 将已还原的普通 M 型文本编译为按下和释放程序。
程序使用小端字节序，并包含末尾的 `262f`。
该模块只处理内存中的文本和字节。它不访问鼠标。
它尚未接入 Piper 或 libratbag 驱动。

```python
from macro_text import compile_macro

program = compile_macro(
    "RightDown 1\ndelay 230\nLeftDownUp 1\ndelay 50\nRightUp 1",
    "dpi accurateout",
    repeat=0,
)
press_bytes = program.press
release_bytes = program.release
```

输入使用十进制整数。指令名称不区分大小写。
编译器统一行内空白，并报告出错的源代码行号。
未知指令、错误参数和越界跳转会产生 `MacroTextError`。
它不会模拟 EXE 对未知指令或越界参数的所有处理方式。

## 已实现的语法

| 语法 | 参数 |
| --- | --- |
| `keydown N`、`keyup N`、`keydownup N` | USB 键盘 usage。支持修饰键及旧别名 264 至 271。 |
| `modifierkeydown N`、`modifierkeyup N` | 1 至 255 的位掩码。 |
| `consumerkeydown N`、`consumerkeyup N` | 1 至 1023。 |
| `systemkeydown N`、`systemkeyup N` | 1 至 255。 |
| `leftdown`、`leftup` 等 | 支持 left、right、middle、button4、button5。可附加旧参数 `1`。 |
| `leftdownup`、`rightdownup` | 按下、延迟 16、释放。可附加 `1`。 |
| `wheelup`、`wheeldown` | 每行一次。可附加 `1`。 |
| `delay N`、`delays N`、`delaym N` | 分别输入毫秒、秒、分钟。见下文。 |
| `mover X Y` | 每个轴为 -32768 至 32767。 |
| `var VarA = RHS` | RHS 为 VarA 至 VarZ 或 0 至 255。 |
| `var VarA = VarB OP RHS` | OP 为 `+`、`-`、`&`、`|`、`^`。 |
| `core_var N` | 0 至 2。写入 VarH。物理状态含义尚未全部确认。 |
| `if VarA == RHS goto N` | 相等条件。也支持 `!=`。 |
| `if VarA == RHS rgoto_24bit D` | 条件相同，目标使用相对源代码行偏移。 |
| `goto N` | 从 1 开始的绝对源代码行号。 |
| `rgoto D` | 相对当前源代码行的偏移。使用短跳转。 |
| `rgoto_24bit_highlanguage D` | 同上，强制使用两个字的跳转。 |
| `dpi ACTION` | up、down、+、-、accuratein、accurateout。 |
| `profile ACTION` | up、down、recycle。 |
| `ledchange ACTION` | up、down、rotation。 |
| `shoot index N` | 1 至 8。 |
| `shoot ACTION` | exec_delay、enable_mover、move_recovery、paynxlimit。 |
| `stop`、`toggleaxisy` | 无参数。 |
| `null` 或空行 | 不生成指令字。仍占一个源代码行号。 |
| `moveto X Y` | `originaxis`（`2620`）、`delay 48`，再以 12 位或 16 位格式相对移动。 |
| `gowhile X N` | 用 VarE、VarF、VarG 轮流计数，把第 X 行至本行重复 N 次。最多嵌套三层。 |
| `ifkey K S L` | 按键 K（1 至 24）按住（S 非零）或松开（S 为零）时跳到第 L 行。 |
| `lockrightkey`、`toggleonoffjump`、`key7functiontoggle` | `260d`、`2621`、`261c`。 |
| `VarA = ...` | 省略 `var` 的赋值，EXE `006289a0` 会补上。 |

编译器拒绝原始 `dw` 和显式 `macroending`。
它自行生成操作数和末尾指令，以保持指令边界。
底层编码器支持 `reportrate` 和 `sensorlift`，但已检查的文本展开函数不保留它们。
因此本模块拒绝这两个文本指令。

## 延迟规则

EXE 的文本展开函数先将毫秒数分成 1024 单位的块和余数。
每个扩展延迟字保存 1 至 255 个块，编码为 `2700 + 块数`。
最终编码函数将余数限制为 999。

| 输入 | 指令字，不含末尾指令 |
| --- | --- |
| `delay 0` | 无。 |
| `delay 1000` | `2be7`，余数限制为 999。 |
| `delay 1023` | `2be7`。 |
| `delay 1024` | `2701`。 |
| `delay 2000` | `2701 2bd0`，一个块加 976。 |
| `delays 1` | `2be7`，先换算为 1000 毫秒。 |
| `delaym 1` | `273a 2a60`，58 个块加 608。 |

源代码中的 `delays N` 输入秒数。
EXE 展开后生成的内部 `delays N` 文本输入块数。
这两个阶段使用同名指令，但参数含义不同。
本模块的公开输入采用源代码阶段的含义。

C 普通事件编译器仍将长延迟分成最多 999 毫秒的指令，以保留输入时长。
本模块采用 EXE 文本路径的规则，因此两种 API 可以生成不同的字节。
这些规则描述编码参数，不证明实机计时精度。

## 位移、跳转和重复

文本路径使用严格边界选择位移格式。
两个轴的绝对值都小于 63 时使用 7 位格式。
否则，两个绝对值都小于 2047 时使用 12 位格式。
其余情况使用 16 位格式。`mover 0 0` 不生成指令。
C 位移编码器采用最短的无损格式，边界选择有所不同。

跳转输入使用源代码行号。编译器先展开各行，再计算指令字偏移。
目标可以是源代码末尾之后的一行。它指向后续重复指令或末尾指令。
短跳转必须落在 -4096 至 4095 个字的范围内。
条件和长跳转采用 24 位有符号偏移。
所有偏移均以跳转指令自身为起点。

| repeat | 按下程序 | 释放程序 |
| --- | --- | --- |
| 0 | 正文、末尾。 | 指定的释放正文、末尾。 |
| 1 | 正文、跳回正文、末尾。 | stop、末尾。忽略输入的释放正文。 |
| 2 | `2621`、正文、跳回正文、末尾。 | NOP、末尾。忽略输入的释放正文。 |

正文加前缀超过 1000 个字时，重复跳转使用长格式。
切换模式的跳转跳过 `2621` 前缀。
单个完整程序最多包含 16384 个字。
输入最多包含 16384 行和一 MiB 字符。

## 静态证据和检查

| EXE 地址 | 作用 |
| --- | --- |
| `006237f4` | 展开一行文本。 |
| `0062200c` | 记录源代码行与展开字的对应关系，并修正跳转。 |
| `0060c510` | 生成三种重复方式。 |
| `0060ca14` | 将内部文本编码为一个字。 |

`0062200c` 使用 120 页的栈探测循环。
Ghidra 脚本只在分析数据库中替换已检查的循环，再恢复并检查原始字节。
复现方法见 [DELPHI.md](DELPHI.md)。官方 EXE 文件保持原样。

`test_macro_text.py` 比较固定的预期字序列。
它还调用独立的 C 存储检查器，并通过 `flash.py` 解析生成的宏。
测试覆盖变长指令、跳转目标、延迟边界、变量、重复和宏池共用。

## Rust 版补齐的语法

`gowhile`、`ifkey`、`moveto` 和三个单字指令来自 EXE `006237f4` 与
`0060ca14` 的反编译。多语句展开中的跳转从跳转指令自身计算偏移，
因此 `gowhile` 的条件跳转位于展开后的第 2 个字，`ifkey` 位于第 4 个字。
`keydown` 等键盘指令忽略多余参数，官方文件中的 `KeyDownUp 32 1` 因此可以编译。

`moveto` 只在屏幕宽高超过 4096 像素时追加第二次 `originaxis`。
本工具不读取主机屏幕尺寸，不生成第二次回原点。

## HighLanguage

`src/highlanguage.rs` 按 EXE `006134b4` 把 HighLanguage 转为上述 M 文本：

| 语句 | 转换 |
| --- | --- |
| `keyboard down/up 名称` | `keydown`/`keyup` 加 USB usage。名称表来自 EXE `0159e8dc`。 |
| `mousebutton down/up left/right/middle/key4/key5` | `leftdown` 等。 |
| `mousewheel forward/backward N` | N 行 `wheelup` 或 `wheeldown`。 |
| `mousemove absolute/relative X Y` | `moveto` 或 `mover`。 |
| `delay N ms/s/min` | `delay`、`delays` 或 `delaym`。 |
| `math ...` | `var ...`。 |
| `if A op B { ... }` | 取反条件跳到块末标记。 |
| `ifkey K press { ... }` | `core_var`、按位与，再条件跳过块。 |
| `repeat N { ... }` | VarE、VarF、VarG 轮流计数，在块末跳回块首。 |
| `flag 名称`、`gotoflag 名称` | 相对行跳转，距离达到 1024 行时使用 24 位跳转。 |
| `comment ...` | 删除。 |

EXE 会把其他语句替换为空行。官方 Sniper AMC 文件在 HighLanguage 中混用
`RightDown 1` 等普通语句，按 EXE 规则会得到空程序。
本工具保留这些语句，交给 M 编译器处理。

`src/amc.rs` 读取和写出 UTF-16 的 AMC 文件，全部 12 个官方 Sniper 文件均可编译。

## 实机验证

宏 `KeyDownUp 4`、`delay 150`、`gowhile 1 3`、`ifkey 11 1 6`、`KeyDownUp 5`
存入“3”键后，短按输出 aaab，按住输出 aaa。
这验证了 `gowhile` 的计数和 `ifkey` 的两个分支。见 [WRITES.md](WRITES.md)。
