# Delphi / VCL analysis

The analysis reads the official EXE. It does not execute the EXE or access USB.
The EXE hash is in [PROTOCOL.md](PROTOCOL.md).

## Tools

[undelphi](https://github.com/ATRAPSLLC/undelphi) extracts class metadata and
embedded forms on Linux. The checked version is `0.4.0`, commit
`e910452a27dfeccf85f215c3b09a07c1691224cb`.
Ghidra then decompiles the code at the recovered addresses.
Metadata extraction does not restore the original Delphi source.

Detect It Easy identifies Borland Delphi. undelphi identifies C++Builder
from an Embarcadero build string. Both tools find the Delphi/VCL runtime.
These results do not establish the application's source language or exact
compiler version. Use the recovered VCL metadata with the `borlandcpp`
calling convention in Ghidra.

The extraction found:

| Record | Count |
| --- | ---: |
| Classes | 3,848 |
| Published methods | 1,009 |
| Forms | 140 |
| Form event bindings | 1,529 |
| Event bindings with resolved code addresses | 1,527 |
| Virtual method slots, including inherited slots | 72,254 |
| Dynamic dispatch slots | 2,378 |

`delphi-index.rs` resolves child events against the root form's class.
The event handler belongs to the form, even when a button owns the event
property. The upstream dump example instead searches the button's class.
That search misses many event targets in this EXE.

## Repeat the extraction

Run these commands from this repository. Use a separate checkout for
the analysis library.

```sh
git clone https://github.com/ATRAPSLLC/undelphi ~/.cache/undelphi
git -C ~/.cache/undelphi checkout e910452a27dfeccf85f215c3b09a07c1691224cb
cp contrib/research/delphi-index.rs ~/.cache/undelphi/examples/a90_index.rs
cargo build --release --locked \
  --manifest-path ~/.cache/undelphi/Cargo.toml --example a90_index
~/.cache/undelphi/target/release/examples/a90_index \
  /home/w/gitproject/BloodyWorkShop8/BloodyWorkShop8.exe \
  > /tmp/a90-delphi-index.tsv
```

Skip the clone command if the checkout already exists.
The TSV columns are `kind`, `va`, `owner`, `name` and `detail`.
Addresses are hexadecimal virtual addresses. Zero means no code address.
CLASS addresses identify the start of VMT metadata, including its negative
header. Do not treat these addresses as function pointers.
VIRTUAL rows contain code addresses, slot offsets and pointer locations.
DYNAMIC rows contain code addresses and dispatch indexes.
The importer below only imports published METHOD names.
The exporter escapes backslashes, tabs and line breaks in text cells.

Import the method names into an existing Ghidra project:

```sh
/opt/ghidra/support/analyzeHeadless /tmp piper-a90-ghidra \
  -process BloodyWorkShop8.exe -noanalysis \
  -scriptPath contrib/research \
  -postScript A90ImportDelphi.java /tmp/a90-delphi-index.tsv
```

The import adds labels. It does not change function signatures.
The actual EXE extraction and all 1,009 label imports passed locally.
A second import added zero labels, as expected.

## Useful entry points

| Class and method | Address |
| --- | --- |
| `TtabSensitivityCPI.FormCreate` | `0072f2b4` |
| `TtabSensitivityReportRate.FormCreate` | `0073f1b4` |
| `TtabKeySet.FormCreate` | `006d86f8` |
| `TtabBrightness.FormCreate` | `006f161c` |
| `TtabMacroEdit3.FormCreate` | `006f9790` |
| `TtabMacroEdit3.CategoryBtn_SaveClick` | `007116e4` |
| `TtabMacroEdit3.CategoryBtn_SaveAsClick` | `007125b4` |

The CPI form resource contains form events but no child controls.
Function `0072fc68`, called by FormCreate, creates the controls in code.
It also assigns event handler addresses and the form instance pointer.
The control layout metadata identifies `FOnClick` at instance offset
`0x120`. Its companion instance pointer is at `0x124`.

| Callback address | Operation found in code |
| --- | --- |
| `00733608` | Change the X/Y link controls. |
| `007337cc` | Enable or disable consecutive DPI presets. |
| `00735880` | Select a preset and update the X/Y text controls. |
| `00734f10` | Restore controls from the current settings buffer. |
| `007353f0` | Apply the form values through `00733ac0`, then refresh the form. |
| `00733ac0` | Collect preset count, X/Y values and selected values in the application settings buffer. |

These callback names describe observations. They are not recovered source
names. The code collects up to five visible presets. The device's factory
table has eight entry slots. These are different limits.
The Apply callback does not directly call the USB helper. Further analysis
must follow the application's message and save dispatch.

To reproduce the callback decompilation:

```sh
/opt/ghidra/support/analyzeHeadless /tmp piper-a90-ghidra \
  -process BloodyWorkShop8.exe -noanalysis \
  -scriptPath contrib/research \
  -postScript A90Decompile.java /tmp/a90-delphi-cpi.c \
  72f2b4 72fc68 733608 7337cc 735880 734f10 7353f0 733ac0 5a1dc8 43c5d8
```

## Macro compiler and large stack frame

`A90Decompile.java` also applies observed signatures for string helpers.
These include concatenation, integer conversion, trimming and lowercase conversion.
These helpers use stack arguments and caller cleanup, so their convention is `__cdecl`.
The script names describe observed operations. They are not original symbols.
The corrected signatures expose instruction names and arguments in `0060ca14`.
See [MACROS.md](MACROS.md) for the recovered instruction format.

Function `0060c510` adds the control flow for once, held and toggle modes.
Its assignment field `+8` selects values 0, 1 and 2.
Function `0060ed38` allocates or reuses multiword press and release sequences.
Function `006112e0` converts them to references or direct instruction words.
Single-word sequences remain direct assignments.
This separates repeat behavior from the later flash allocation.

Function `0043b1e8` reserves 64 KiB through a page-touch loop at `0043b1ec`.
Function `0062200c` reserves 480 KiB through the same loop at `00622010`.
Ghidra does not recover that function's stack frame correctly from the loop.
`A90DecompileProbe.java` checks its 15 original bytes before any change.
It temporarily replaces the loop with an equivalent stack-size adjustment and NOPs.
It then decompiles the function and explicitly restores the original bytes.
The script checks the restored bytes before it returns.

This change applies only to the Ghidra database during analysis.
It does not modify the official EXE file.
The replacement does not preserve page-touch behavior. Do not export it as executable code.

```sh
/opt/ghidra/support/analyzeHeadless /tmp piper-a90-ghidra \
  -process BloodyWorkShop8.exe -noanalysis \
  -scriptPath contrib/research \
  -postScript A90Decompile.java /tmp/a90-opcode-typed.c 60ca14 619b60 \
  -postScript A90DecompileProbe.java /tmp/a90-button-save-typed.c
```

Supply `62200c` as the optional second argument to analyze the macro expansion function:

```sh
/opt/ghidra/support/analyzeHeadless /tmp piper-a90-ghidra \
  -process BloodyWorkShop8.exe -noanalysis \
  -scriptPath contrib/research \
  -postScript A90DecompileProbe.java /tmp/a90-expand-macro.c 62200c
```

The script accepts only these two verified function addresses.

The local run decompiled both compiler functions and the save function.
The restored probe bytes matched the original bytes after each successful run.

`A90References.java` exports references and containing function addresses as TSV.
It reads references already present in the database.
Missing references can indicate incomplete disassembly or indirect calls.
Do not interpret an empty result as proof that code is unused.

```sh
/opt/ghidra/support/analyzeHeadless /tmp piper-a90-ghidra \
  -process BloodyWorkShop8.exe -noanalysis \
  -scriptPath contrib/research \
  -postScript A90References.java /tmp/a90-references.tsv 45bad8 60b498
```
