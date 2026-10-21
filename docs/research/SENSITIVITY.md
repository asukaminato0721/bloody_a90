# A90 sensitivity calculations

The code in `bloody-a90-sensitivity.h` performs offline calculations only.
It does not send a runtime report or write flash.
The current Rust driver exposes DPI settings and applies them immediately after saving.
See [RUNTIME-SENSITIVITY.md](RUNTIME-SENSITIVITY.md) for hardware checks.
The C helpers described below are the earlier offline implementation.

## Two conversion paths

EXE function `00459de0` initializes several sensor conversion tables.
The table at object byte offset `b0` has 63 entries.
An index represents nominal CPI divided by 100.
EXE function `0045b5d0` uses this table for sensor ID 3327 with separate X/Y values.
The same function uses a formula when X equals Y.

For X and Y from 100 through 6200 CPI:

| Condition | X scale | Y scale | Sensor code |
| --- | --- | --- | --- |
| X equals Y | 64 | 64 | `floor((X + 22) / 44) - 1` |
| X exceeds Y | 64 | `floor(64 * Y / X + 0.5)` | Table entry `X / 100` |
| Y exceeds X | `floor(64 * X / Y + 0.5)` | 64 | Table entry `Y / 100` |

The constants at `0045b728` and `0045b72c` are floats `1.0` and `0.5`.
Helper `013bb69c` changes the x87 rounding mode to truncation before integer conversion.
The offline encoder uses an equivalent integer ratio for the permitted input range.
It accepts a 100-CPI grid and rejects values outside 100 through 6200.
It does not implement the EXE's extrapolation path above 6200.

The EXE clamps each axis to a supplied device maximum before conversion.
The offline caller must select the correct sensor ID and check the device maximum.
The model mapping below establishes these values for this EXE.

## Factory agreement

The five active records in the saved firmware-07 backup have both scales equal to 64.
Their sensor codes match these entries in the constructor's table:

| Nominal CPI | Table index | Table code | Factory code |
| ---: | ---: | --- | --- |
| 1000 | 10 | `17` | `17` |
| 1200 | 12 | `1b` | `1b` |
| 1600 | 16 | `24` | `24` |
| 2000 | 20 | `2e` | `2e` |
| 6200 | 62 | `91` | `91` |

This is an independent match between the EXE table and five device records.
It explains why the equal-axis formula did not reproduce the factory codes.
The formula produces `16` for 1000 CPI and `8c` for 6200 CPI.
Do not replace the constructor table with that formula when interpreting factory data.

`bloody_3327_factory_cpi()` reads nominal CPI from a known factory record.
It requires X and Y scales of 64 and a code present in the constructor table.
It is not the inverse of `bloody_3327_encode_cpi()`.
Do not use it to interpret an arbitrary saved configuration.
`flash.py` continues to report raw scale and sensor fields.

The agreement does not identify the physical sensor or measure its CPI.
The catalog mapping establishes the EXE conversion choice.
The Rust transport passed hardware checks. Physical CPI calibration remains unmeasured.

## Model mapping

EXE function `00442950` builds a device descriptor from a packed identity value.
It extracts the high word and masks that model word with `fffc` at `00442977`.
For masked model `04fc`, its jump table at `00442d67` selects `00443f01`.
The table index is `04fc - 04f4`, which is 8.
The selected entry sets these descriptor fields:

| Byte offset | Value | Evidence |
| --- | --- | --- |
| `0c` | `0cff` (3327) | Store at `00443f18`. |
| `10` | `1838` (6200) | Store at `00443f26`. |
| `28` | `SN8F22E88B` | String at `0141c9f6`, assigned at `00443f62`. |

The observed model `04fd` selects that entry after masking.
`bloody_a90_encode_cpi()` uses this mapping and rejects other masked models.
The catalog also sets a rate field to 2000. The observed memory query reports 1000.
Do not treat the catalog rate as support measured on firmware 07.
The offline block patcher therefore accepts rates only through 1000 Hz.

## Save call site

Disassembly at `00450220` reads each pair of application CPI values.
It loads the sensor ID from global word `016127c0` before calling `0045bad8`.
That dispatcher selects `0045b5d0` for ID 3327.
The call site copies the resulting X scale, Y scale and sensor code into the settings block.
There are five iterations.

Global word `016127c0` is field `3c` of the settings object at `01612784`.
Function `00477dd4` assigns its sixth argument to that field.
Function `0047554c` restores the corresponding field from `Device.log`, section
`Device`, key `SensorType` (string address `01429110`).
The catalog mapping establishes the default sensor choice for model `04fc`.
An arbitrary existing log can contain a different value.

## Offline block update

`bloody-a90-settings.h` updates a caller-supplied 256-byte template.
It accepts firmware 07, one through five active CPI pairs and a selected index.
It checks every input before it changes the output.
It changes words 4 and 5 and the active records beginning at word 6.
It preserves inactive records and all unrelated fields.
An optional rate changes word `1e` to `1000 / rate_hz`.
A zero rate argument preserves the template's raw rate word.

The EXE's block construction at `00450189` clears 256 bytes before populating its fields.
The patcher preserves a supplied template instead. It does not reconstruct all application settings.
Do not copy 256 bytes from factory address `1806` to obtain that template.
The next factory table starts at `183c`, only 54 words later.

The patcher calculates the two checksums used by `0043c5d8`:

- Sum words 4 through 127, modulo 65536.
- Sum each word multiplied by its local index, modulo 65536.

It stores these values in words 2 and 3.
It sets word 0 to `a4a4` and word 1 to `ffff`, the final committed representation.
This buffer is not a write sequence. It does not implement unlock, staging or commit commands.
It does not update the separate general settings block or measure the report rate.
The Rust driver implements the equivalent calculations without calling the C helpers.

`flash.py` reports both checksums for an existing saved sensitivity block.
It reports a mismatch without discarding the raw fields.
It does not apply the 128-word check to the shorter factory table.

## Reproduce and check

```sh
/opt/ghidra/support/analyzeHeadless /tmp piper-a90-ghidra \
  -process BloodyWorkShop8.exe -noanalysis \
  -scriptPath contrib/bloody-a90 \
  -postScript A90Decompile.java /tmp/a90-sensitivity.c \
  442950 459de0 45bad8 45b5d0 47554c 477dd4 43c5d8
cc -std=c11 -Wall -Wextra -Werror -UNDEBUG \
  contrib/bloody-a90/test-sensitivity.c -o /tmp/a90-sensitivity-test
/tmp/a90-sensitivity-test
cc -std=c11 -Wall -Wextra -Werror -UNDEBUG \
  contrib/bloody-a90/test-settings.c -o /tmp/a90-settings-test
/tmp/a90-settings-test
python contrib/bloody-a90/test_flash.py
```

The tests check factory records, model selection, both conversion paths and invalid inputs.
The block tests check explicit expected bytes, checksum overflow and template preservation.
They also check selected indices, rates, in-place output and unchanged output after errors.
The tests do not check sensor operation or persistent writes.
