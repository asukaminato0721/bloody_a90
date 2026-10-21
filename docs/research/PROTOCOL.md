# A90 protocol notes

These notes separate observations from unverified interpretations.
No firmware image or decompiled vendor source is included in this directory.

## Inputs

- Device: USB `09da:fafd`, A90 model word `04fd`.
- Installed firmware: `07`. Active core: `2`.
- On 2026-10-21 the official program updated the mouse to firmware `0c`, USB `09da:5450`.
  See [FIRMWARE-0C.md](FIRMWARE-0C.md).
- Official program: `/home/w/gitproject/BloodyWorkShop8/BloodyWorkShop8.exe`.
- Program SHA-256:
  `4a76f6ef4b6e7da699afbd8d3d780ae9747b599af795531a39e3eb5d93c93162`.
- The package contains A90 P3332A firmware files with revision `0C`.
  Those files were not installed on the mouse.
- Ghidra: x86, little endian, 32 bits, `borlandcpp` compiler specification.
- Firmware disassembly used [sn8dis](https://github.com/Marisa-Chan/sn8dis).
  The SN8F2288 instruction set decoded the relevant routines.
  This does not establish the exact MCU part number.
- Initial read-unlock information came from
  [the P85 driver](https://gitlab.com/C0rn3j/a4tech_bloody_p85_driver/).
  Other P85 settings were not assumed to apply to A90.

The local flash backup is at
`/home/w/.cache/a90-unlocked-flash-backup.bin`.
It contains 0x4000 words, with two little-endian bytes per word.
The backup was read after enabling the read interface.
An earlier backup without that operation contains protected zero responses.
Its SHA-256 is in [MACROS.md](MACROS.md).
`flash.py` inspects this existing backup without a USB transport.

## Transport

The vendor HID interface has usage page `ff32`, usage `04fd` and feature
report ID `7`. Reports contain 64 bytes, including the report ID.
USB identity alone cannot distinguish all Bloody models.
The native driver also checks the interface and model word.

The first eight report bytes contain the command header.
For queries, send the request with `HIDIOCSFEATURE`.
Then read a fresh buffer with `HIDIOCGFEATURE` and report ID 7.
Check the response header before using its payload.

## Verified commands

| Command | Request prefix | Result |
| --- | --- | --- |
| Identity | `07 1f 00 00 00 00 00 00` | Model at report bytes 10–11. Firmware byte at 12. Core is model minus `04fb` (`04fc` to `04ff` for cores 1 to 4). Byte 9 is `22` in every core image, not the core. |
| Memory information | `07 05 00 00 00 00 00 00` | 40-byte payload. See the table below. |
| Read brightness | `07 11 00 00 00 00 00 00` | Report byte 8 is 0–3. |
| Set brightness | `07 11 00 00 80 00 00 00 LL` | `LL` is 0–3. All four levels passed hardware readback. |
| Read one RAM byte | `07 81 00 HH LL 00 00 00` | Report byte 8 contains RAM at `HHLL`. Verified on firmware 07, core 2. |
| Set runtime sensitivity | `07 0d 00 00 00 00 00 00 ...` | Applies DPI and rate. RAM, sensor update and movement checks passed. See [RUNTIME-SENSITIVITY.md](RUNTIME-SENSITIVITY.md). |
| Enable flash reads | `07 25 00 00 52 49 18 22 01` | Allows the firmware read command below to return program data. |
| Read flash | `07 02 00 HH LL 60 00 00` | 28 little-endian words, beginning at word address `HHLL`. |

All requests require zero padding to 64 bytes.
Command `0x25` does not write the program image.
The Rust driver uses bounded flash writes and reads runtime state through RAM queries.

## Memory information

These offsets start at report byte 8. Values are little endian.

| Payload offset | Observed value | Evidence |
| --- | --- | --- |
| 2 | `03e8` | Official program treats this value as a report rate of 1000 Hz. |
| 4 | `2400` | Writable sensitivity address used by the official program. |
| 6 | `2600` | Writable button/configuration address used by the official program. |
| 8 | `3e00` | General settings address. Firmware reads rate and brightness here. |
| 10 | `1806` | Factory sensitivity table address. |
| 12 | `3ff0` | Device data address. Further interpretation is incomplete. |
| 14 | `183c` | Factory button table address. |

The initial word at `0x2400` is `0x2218`, followed by a BOOT HEAD marker.
This initially appeared to conflict with the sensitivity address.
Firmware routine `0x117c` explains the fallback:

1. Read the low byte at word address `0x2400`.
2. Return that table if its marker byte is `a4`.
3. Otherwise select the factory table at `0x1806`.

Routine `0x1185` makes the equivalent choice between `0x2600` and `0x183c`.
The mouse therefore uses factory tables when the writable markers are absent.
This resolves the read-address question. Persistent writes still require
page preservation, checksum checks and hardware validation.

## Runtime DPI

Firmware dispatch starts at `0x118e`. Command `0x0d` reaches `0x12e1`.
That routine copies the payload to RAM. The sensor loop applies a changed sensor code.
It does not write flash.

| Payload offset | RAM address | Meaning |
| --- | --- | --- |
| 0 | `0026` | Preset count minus one |
| 1 | `0027` | Active preset index, beginning at zero |
| 2 | `0028` | X scale, low byte |
| 3 | `0029` | Y scale, low byte |
| 4 | `002a` | Sensor code |
| 5 | `0023` | Rate code |
| 6 | `002d` | X scale, high byte |
| 7 | `002e` | Y scale, high byte |

An earlier test changed `04 00 40 40 17 02 00 00` to
`04 01 40 40 1b 02 00 00`, then restored the original RAM values.
The mouse could not move afterward. That session also sent command `03`.
The earlier test did not isolate the failing command.

On 2026-10-20, isolated command `0d` tests passed with continuous mouse movement.
The tests checked RAM, sensor state and evdev events at each stage.
They also tested the production DPI and rate save methods.
The rate save preserved the current DPI selection.
The tests restored the original runtime state and all flash bytes.
See [RUNTIME-SENSITIVITY.md](RUNTIME-SENSITIVITY.md) for payloads and measured event intervals.

RAM command `0x81` reaches firmware routine `0x3d23`.
That routine reads one byte through the indirect RAM address registers.
The Rust driver uses this command for runtime readback.

The factory table contains X/Y scales of 64 and sensor codes
`17`, `1b`, `24`, `2e`, `91` for its five active entries.
Those codes are not DPI values.
For sensor ID 3327, the current EXE uses a formula when X equals Y.
It uses a lookup table when X and Y differ.
Five constructor table entries match the five active factory codes exactly.
The equal-axis formula produces different codes.
See [SENSITIVITY.md](SENSITIVITY.md) for the calculations and offline implementation.
The EXE catalog maps masked model `04fc` to ID 3327 and a 6200-CPI maximum.
Runtime updates and persistent writes passed hardware checks on this firmware.
The nominal CPI values have not been measured against physical travel distance.

The rate code is `02`, while memory information reports 1000 Hz.
Do not present the rate code as a measured USB report rate.

## Official program functions

Addresses apply only to the program hash above.
Borland passes the first arguments in EAX, EDX and ECX.
Later arguments use the stack.

| Address | Observed operation |
| --- | --- |
| `0059eddc` | Build and send a payload report. |
| `0059ee98` | Build a query, read a response and compare its eight-byte header. |
| `005a01d4` | Read flash with command `02`. This is not the runtime DPI setter. |
| `005a0804` | Read four words with command `2f`. Address bytes are at report offsets 3–4. |
| `005a0938` | Write words with command `2f`. Header byte 2 is `(word_count - 1) * 8 + 1`. |
| `005a09f4` | Same write format, with header byte 5 set to `80`. |
| `005a1b6c` | Read the 40-byte memory information payload with command `05`. |
| `005a1be0` | Send command `07` with a zero payload during device setup. |
| `005a1dc8` | Send the eight-byte runtime sensitivity payload with command `0d`. |
| `0043c5d8` | Update runtime sensitivity, then save a 256-byte sensitivity block if its checksums differ. |
| `0043c90c` | Similar save flow using a different memory-access format. |
| `0043b1e8` | Compile button/macro data, compare checksums, write storage vectors, then commit the `a4a4` marker. The Rust implementation passed hardware checks. |
| `0045bad8` | Select the DPI conversion function by sensor ID. |
| `0045b5d0` | Encode X/Y settings for sensor ID 3327. |

The generic query helper reverses its six final arguments into header
bytes 7 through 2. Reading those arguments in stack order gives incorrect
addresses and mode bits.

The sensitivity save routine calculates two 16-bit checksums over words
4 through 127: a sum and a sum weighted by word index.
It opens access with command `20`, writes the block in 16-word chunks,
then writes the `a4a4` marker. It closes access afterward.
The Rust implementation passed persistent-write and runtime-update checks.
The earlier offline packet planner describes the internal main-vector sequence.
It includes staged markers, final-chunk padding, access polling and EXE delays.
See [SAVING.md](SAVING.md) for that planner and [WRITES.md](WRITES.md) for hardware results.

The button compiler `00619b60` calculates checksums across three separate vectors.
It excludes the first four words of the main vector.
Each auxiliary vector uses its own zero-based weighted index.
The main header stores the resulting sums in words 2 and 3.
The offline trigger patcher uses word differences to preserve unchanged vector contributions.
See [MACROS.md](MACROS.md) for its source-template and pointer requirements.

The alternative memory reader `005a0bec` places a 32-bit word count at
report offset 24 and an address at offset 28. Returned data begins at 32.
The count field is not a memory bank number.
The tested A90 did not return useful data through this format.

The executable also uses command `03` to read nine-byte settings.
Firmware 07 dispatches command `03` through a different table.
That table does not implement the same query format.
Do not send those executable-derived requests to this firmware.

### Command 03 and the movement failure

An earlier temporary test script sent `07 03 03 03 00 00 00 00` with a
zero payload. It treated this packet as a settings query. That classification
was incorrect for the installed firmware.

Static disassembly gives this path:

1. Handler `14f4` tests report byte 2. A nonzero value selects `1aab`.
2. The table at `1aab` dispatches on report byte 3.
3. Value 3 selects `1ab8`, a sensor serial transfer routine.
4. That routine loads its byte counter from report byte 7.
5. It transfers one byte before it decrements the eight-bit counter.

The test supplied a zero counter. Under this instruction decoding, that
counter produces 256 transfers rather than zero transfers. This can read
beyond the 56-byte report payload. The code path shares transfer functions
with sensor setup.

This is a possible cause of the movement failure. It does not establish
the cause. A later isolated runtime sensitivity test passed movement checks.
See [RUNTIME-SENSITIVITY.md](RUNTIME-SENSITIVITY.md) for that result.
The SN8F2288 decoder has incorrect peripheral names for this firmware.
The interpretation above uses instruction flow and raw register addresses.

## Sensitivity table layout

The EXE save function `0043c5d8` selects a runtime payload from this layout.
The saved factory bytes at word address `1806` agree with the main fields.
Offsets below are byte offsets from the start of the table.

| Offset | Size | Interpretation |
| --- | --- | --- |
| `00` | 2 | Valid marker `a4a4`. |
| `02` | 2 | Second header word. Factory value `a4a4`. The save routine stages `ffff`. |
| `04` | 2 | Sum checksum for a saved block. |
| `06` | 2 | Weighted checksum for a saved block. |
| `08` | 2 | Preset count minus one. Factory value 4. |
| `0a` | 2 | Selected preset index. Factory value 0. |
| `0c` | 48 | Eight records. Each record contains 16-bit X scale, Y scale and sensor code. |
| `3c` | 2 | Rate code. Factory value 2. |

Only the low bytes of count, index, sensor code and rate enter the runtime
payload. X/Y scales use both bytes. The factory block has zero checksum
words. Do not reject the factory block using the saved-block checksum rule.
Do not copy the next 256 bytes at `1806` into a writable sensitivity block.
Other factory tables follow this data before that size boundary.

Delphi method extraction and UI callback analysis are in [DELPHI.md](DELPHI.md).

## Repeat the executable analysis

Use a writable Ghidra project directory. Select the `borlandcpp` compiler
specification when importing the executable.
The script contains known function starts and restores the memcpy/memset
signatures needed by the decompiler.

```sh
/opt/ghidra/support/analyzeHeadless /tmp a90-analysis \
  -import /home/w/gitproject/BloodyWorkShop8/BloodyWorkShop8.exe \
  -processor x86:LE:32:default -cspec borlandcpp \
  -scriptPath contrib/bloody-a90 \
  -postScript A90Decompile.java /tmp/a90-decompiled.c
```

For an existing project, use `-process BloodyWorkShop8.exe -noanalysis`
instead of the import options. Additional hexadecimal function addresses
can follow the output path.

## Remaining work

The Rust driver implements and checks the settings listed in [FEATURES.md](FEATURES.md).
Remaining work includes gesture editing and its host execution path.
The installed core-2 firmware omits the left-button shooting dispatch.
The official core-3/4 files include it. See [CORES.md](CORES.md) for the comparison.
The current driver does not update firmware or change authorization data.
