# A90 macro format

This analysis uses the EXE and firmware hashes in [PROTOCOL.md](PROTOCOL.md).
It does not send commands to the mouse.
The native driver does not yet expose macro or button settings.

## Instruction encoding

EXE function `0060ca14` compiles one internal text instruction to one word.
It trims the text and compares names without case sensitivity.
Firmware routine `0d92` dispatches these words.
Words use little-endian byte order in the saved flash image.

`libratbag/src/bloody-a90-macro.h` implements these instructions:

| Action | Word | Input |
| --- | --- | --- |
| Key down | `2100 + usage` | USB keyboard usage, excluding modifier usages. |
| Key up | `2000 + usage` | USB keyboard usage, excluding modifier usages. |
| Modifier down | `2300 + mask` | Usages `e0` through `e7` select bits 0 through 7. |
| Modifier up | `2200 + mask` | Same mask. |
| Mouse button down | `2500 + mask` | Buttons 1 through 5 select bits 0 through 4. |
| Mouse button up | `2400 + mask` | Same mask. |
| Consumer key down/up | `3400 + usage` / `3000 + usage` | USB consumer usage, 1 through `3ff`. |
| System key down/up | `3900 + usage` / `3800 + usage` | USB system control usage, 1 through `ff`. |
| Delay | `2800 + milliseconds` | One instruction contains 1 through 999 milliseconds. |
| Delay blocks | `2700 + blocks` | One instruction contains 1 through 255 blocks of 1024 counter units. |
| Wheel up | `262b` | No argument. |
| Wheel down | `262c` | No argument. |
| No operation | `262e` | No argument. A zero delay also produces this word. |
| End macro | `262f` | No argument. |
| DPI preset up/down | `2626` / `2624` | No argument. |
| DPI increment/decrement | `2627` / `2625` | No argument. |
| Accurate mode enter/leave | `2628` / `2629` | No argument. |
| Profile up/down/cycle | `2622` / `2623` / `262a` | No argument. |
| LED up/down/cycle | `2618` / `2619` / `261a` | No argument. |
| Report rate cycle | `260e` | No argument. |
| Sensor lift change | `260f` | No argument. |
| Toggle Y axis | `260b` | No argument. |
| Stop macro | `262d` | No argument. |
| Shoot index | `2610` through `2617` | EXE indices 1 through 8. These are not physical button numbers. |
| Shoot controls | `2608`, `2609`, `260a`, `260c` | No argument. See [SHOOTING.md](SHOOTING.md) for the exact internal names. |

Mouse button order is left, right, middle, back, forward.
This order differs from Linux button code order.
The keyboard input is a USB usage, not a Linux input key code.
The encoder does not translate Linux key codes.

The EXE also accepts legacy modifier aliases `108` through `10f`.
The encoder accepts only USB usages `e0` through `e7` for these modifiers.
The EXE replaces unknown instructions with a no-operation word.
The encoder returns an error for unsupported actions.

The compiler checks all events before it changes the output buffer.
It divides a long delay into instructions of at most 999 milliseconds.
This preserves the requested duration instead of applying the EXE's single-line clamp.
The caller must supply the end instruction and select any repeat behavior.
The compiler does not select a flash address or construct a settings header.
Device action execution depends on the current core and configuration.
The encoded word alone does not establish the resulting device behavior.

## Relative movement

`bloody_a90_macro_motion` selects the shortest lossless format for signed X/Y values.
The units are device motion counts. They are not screen pixels.
The encoder does not append an end instruction.

| Range of both axes | Words |
| --- | --- |
| -64 through 63 | `4000 OR ((x AND 7f) << 7) OR (y AND 7f)` |
| -2048 through 2047 | `3b00 OR (((x AND fff) >> 8) << 4) OR ((y AND fff) >> 8)`, then `((x AND ff) << 8) OR (y AND ff)` |
| -32768 through 32767 | `2600`, then signed X, then signed Y |

All words use little-endian byte order.
The 12-bit data word places the low Y byte before the low X byte.
The 16-bit format places the complete X word before the complete Y word.
Firmware routine `066e` reads the operand words before dispatch.
Handlers `0e24` and `0e3a` establish these different operand orders.
The encoder checks negative values and range limits before changing output.
Macro execution on the mouse remains untested.

## Relative branches

`bloody_a90_macro_jump` encodes a signed displacement in words.
Firmware handlers `0e47` and `116c` add it to the current instruction address.
The origin is the branch instruction, not the following word.

| Displacement range | Words |
| --- | --- |
| -4096 through 4095 | `displacement AND 1fff` |
| -8388608 through 8388607 | `3a00 OR ((displacement >> 16) AND ff)`, then `displacement AND ffff` |

The encoder selects the shorter format when possible.
It does not check the target against a complete macro.
Zero displacement encodes a branch to itself.
Macro inspection reports the displacement and target address but continues in storage order.
It does not follow branches or prove that a macro terminates.

Firmware handlers `1083` and `1090` preserve ten bits for consumer usages.
Handlers `109f` and `10a6` preserve eight bits for system usages.
The encoder rejects values that would overlap the opcode.
These width checks do not establish that the device or host implements every usage.

## Button macro repetition

EXE function `0060c510` constructs three modes from field `+8` of an assignment.
`bloody_a90_compile_button_macro` implements these modes for ordinary event bodies.
The caller supplies the body without end or stop events.
The result contains press bytes and one direct release instruction.

| Mode | Press sequence | Release action |
| --- | --- | --- |
| Once (`0`) | Body, end. | No operation. |
| While held (`1`) | Body, branch to body start, end. | Stop. |
| Toggle (`2`) | `2621`, body, branch to body start, end. | No operation. |

The toggle branch skips the `2621` prefix.
Firmware handler `0fff` records the first prefix address and stops on a repeated prefix at the same address.
This is the instruction used for the second trigger to stop repetition.
The firmware stop path also clears the active macro state.
The device execution and interruption timing remain untested.

The compiler counts encoded words after it divides long delays.
It uses that count for the branch displacement.
The EXE selects the 24-bit branch above 1,000 internal lines.
The offline compiler instead uses the shortest format whose signed range contains the displacement.
These choices can produce different bytes for long bodies.
The displacement still follows the firmware's instruction format.
The compiler does not allocate button storage or prove that the result fits the device's available macro area.

## Independent fixture

Factory words `18a7` through `18b6` contain this macro:

```text
2501 2819 2401 2855
2501 2819 2401 2855
2501 2819 2401 2855
2501 2819 2401 262f
```

These words press and release the left button four times.
Each press interval is 25 milliseconds.
The first three releases have an 85-millisecond delay.
`test-macro.c` compares the compiler output with these saved bytes.
The backup SHA-256 is:

```text
725528a1554745777b37e7ce8059080e622c55c7b7e0a38fbecae0b01924bf6c
```

This comparison checks instruction encoding against independent device data.
It does not check macro execution, timer accuracy or a configuration write.

## Text programs and further instructions

The offline text compiler supports a subset of the ordinary M-type source format.
It expands clicks, relocates source-line branches and applies the EXE's repeat threshold.
See [TEXT.md](TEXT.md) for supported syntax and the separate delay rules.
HighLanguage and AMC import remain incomplete.

The offline encoder now supports variable assignment, arithmetic, bit operations and conditional branches.
See [VARIABLES.md](VARIABLES.md) for the operand layouts and firmware handlers.
These instructions require more than a list of ordinary key events.
Their presence in the EXE does not establish support on every firmware revision.
The encoder also supports shooting instruction words and ordinary and special A/B/C/D templates.
The remaining M syntax and complete shooting configurations need further work.
Instruction widths do not establish valid state combinations or execution timing.

## Button storage

Function `00619b60` builds button tables and macro data.
Function `0062f6c8` converts application assignments to internal macro text.
Function `006112e0` selects a direct instruction or a macro reference.
Function `0060ed38` allocates and deduplicates multiword macros.

The compiler has seven profile slots and 24 button trigger records per slot.
Each trigger has separate press and release instructions.
The program also allocates wheel and gesture records.
These are compiler limits. They do not establish the A90's physical button count.

On installed firmware 07, routine `1153` handles macro references.
It adds `(instruction & 3fff)` to the selected button table base.
Routine `1185` selects base `2600` or factory base `183c`.
A reference therefore contains a relative word offset, not a byte address.

The factory header contains pointer words with high bits set, such as `4036`.
Firmware helper `0244` masks the high byte with `3f` before adding the pointer.
This establishes address calculation. The meanings of the high flags remain unresolved.
Do not combine the instruction encoder with an assumed P85 flash layout.

For firmware 07, header word 7 points to the button profile pointer list.
Header word 8 points to the wheel profile pointer list.
Each list entry points to its action table, relative to the same table base.
Button actions contain 24 consecutive press/release pairs.
Wheel actions contain two direction words. They are not a press/release pair.
Header word 11 points to 16 auxiliary actions, including the factory click shortcuts.
These actions occupy the first 16 words of a 22-word auxiliary block.
The remaining six words contain parameters and flags, not action instructions.
Header word 10 points to two metadata words per profile.
The editor preserves these fields as described in [PROFILES.md](PROFILES.md).

In the saved-table case, header word 4 stores selected index and maximum index.
The low byte is the selected index. The high byte is the maximum index.
Firmware routine `080c` forces profile index zero when the saved marker is absent.
The factory header says seven slots, but its lists contain one entry each.
Do not read seven entries from those factory lists.

`flash.py` follows these pointers in an existing firmware-07 backup.
It checks the size, model, firmware revision, profile counts and address ranges.
It preserves raw instruction values and pointer flags in its JSON output.
Macro inspection stops at unknown instructions instead of interpreting possible operand words.
It does not execute jumps or emulate the macro virtual machine.

The actual backup produced one factory profile, 24 trigger pairs and three macros.
References `c073`, `c06f` and `c06b` resolve to `18af`, `18ab` and `18a7`.
These are the two-, three- and four-click suffixes of the same instruction sequence.
The parsed sensor codes remain raw values. The parser does not label them as DPI.

### Allocate macro storage offline

`bloody-a90-storage.h` allocates macros in a caller-reserved byte buffer.
The caller supplies its start offset in words, relative to the button table.
The allocator returns `c000 OR (start offset + allocated word offset)`.
It checks the 14-bit pointer limit and the supplied byte capacity.
The caller must separately limit the area to the device's available storage.
The allocator does not select an unused flash area or move existing tables.

Each input macro must end with an END instruction.
The validator checks instruction widths and ordinary and conditional branch targets.
A branch must target an instruction boundary within that macro.
The validator accepts loops. It does not prove termination or execution timing.
Unknown instructions receive an error before storage changes.

The allocator reuses equal bytes at instruction boundaries, including suffixes.
An operand that happens to equal an opcode cannot supply a reusable entry point.
The four-click fixture at relative offset `006b` produces reference `c06b`.
Its three-click and two-click suffixes reuse `c06f` and `c073` without more storage.
These references match the factory backup.
This optimization does not claim to reproduce the EXE's allocation algorithm.
An error preserves the pool bytes, allocation count and output reference.

### Edit an existing saved record

`bloody-a90-settings.h` provides `bloody_a90_patch_trigger` for offline buffers.
The input must be an existing saved template with header words `a4a4, ffff`.
It does not convert the factory table to a saved table.
The caller supplies the local word index of an existing press/release pair.
Both replacement actions must encode to one word.
The function preserves all bytes except that pair and the two checksum words.

EXE function `00619b60` adds three vectors to the button checksums.
The main vector excludes header words 0 through 3.
The other vectors start their weighted index at zero.
The saved checksum therefore cannot be reconstructed by summing an assumed continuous flash range.
The patch function instead adds each changed word's value difference.
It multiplies that difference by the local main-vector index for the weighted checksum.
All arithmetic wraps to 16 bits in the result.
Unchanged vectors retain their original contributions.

The function checks model, firmware, buffer bounds and action encodings before changing output.
It supports an output buffer that is also the input buffer.
It does not establish that the original checksum was valid.
The caller must validate the source template and resolve its profile pointers.
Shared action tables need special handling because one record can serve multiple profiles.
This function does not allocate new macros or determine physical button numbers.
The driver does not call it or write its output to the mouse.

`bloody_a90_finalize_buttons` updates checksums after several main-vector edits.
It compares equal-length original and modified vectors and applies each word's difference.
This includes changed macro words and their new button references.
It preserves checksum contributions from unchanged auxiliary vectors.
The caller must supply the complete main vector and a valid original checksum.
The input must exclude auxiliary vectors and unused flash padding.
The caller must also check shared tables, pointer targets and reserved storage.
The function does not infer those properties or convert a factory table.

`test_storage.py` compiles and runs the C storage tests in a temporary directory.
The C test builds a synthetic saved main vector with five macro assignments.
It includes two-, three-, four-click, toggle and held-repeat assignments.
The held-repeat body shares the toggle macro's suffix.
The Python parser independently checks the resulting pointers, actions, branches and checksums.
This verifies an offline buffer. It does not verify a saved configuration on the mouse.

Factory trigger 14 contains `2628` on press and `2629` on release.
This provides an independent fixture for the accurate-mode words.
Triggers 15 and 16 contain rate-cycle and lift-change instructions.
These trigger indices do not establish the physical button mapping.

Macro inspection recognizes all three relative movement formats and the device actions above.
It consumes the full instruction before inspecting the next opcode.
An operand equal to `262f` therefore does not terminate inspection.
An instruction that exceeds the inspection limit receives an incomplete-instruction result.

```sh
python contrib/bloody-a90/flash.py \
  ~/.cache/a90-unlocked-flash-backup.bin > /tmp/a90-flash-decoded.json
python contrib/bloody-a90/test_flash.py
```

## Check the encoder

Run these commands from the Piper repository:

```sh
cc -std=c11 -Wall -Wextra -Werror -UNDEBUG \
  contrib/bloody-a90/test-macro.c -o /tmp/a90-macro-test
/tmp/a90-macro-test
cc -std=c11 -Wall -Wextra -Werror -UNDEBUG \
  contrib/bloody-a90/test-buttons.c -o /tmp/a90-buttons-test
/tmp/a90-buttons-test
python contrib/bloody-a90/test_storage.py
```

The test checks the factory fixture, modifier masks, button order and long delays.
It checks movement formats, signed limits and device action words.
It checks consumer usages, system usages and both branch formats.
It checks all three repeat modes and a long delay that requires a 24-bit branch.
It also checks invalid input, capacity limits and unchanged output after errors.
The record test checks independent checksum values, subtraction, wrap and byte preservation.
The storage test checks suffix reuse, operand boundaries and branch targets.
It checks allocation limits and the complete synthetic fixture with the independent parser.
