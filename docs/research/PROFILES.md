# A90 saved profiles

This analysis uses the EXE and firmware hashes in [PROTOCOL.md](PROTOCOL.md).
All functions in `bloody-a90-profiles.h` operate on offline buffers.
The native driver does not expose these functions or save their output.
The separate [configuration builder](CONFIGURATION.md) now creates new main
vectors with one through seven profiles and allocates their tables and macros.

## Table structure

These word indices are relative to the button table base.
On installed firmware 07, the saved base is `2600` and the factory base is `183c`.

| Header word | Meaning established by the current evidence |
| --- | --- |
| 4 | Selected profile in the low byte. Maximum profile index in the high byte. |
| 7 | Pointer to the button profile pointer list. Each action table has 24 press/release pairs. |
| 8 | Pointer to the wheel profile pointer list. Each action table has down/up words. |
| 9 | Pointer to a gesture profile pointer list in the EXE compiler. Complete firmware use remains under analysis. |
| 10 | Pointer to two metadata words per profile. Their complete field meanings remain unresolved. |
| 11 | Pointer to a 22-word auxiliary block. Its first 16 words are action instructions. |

Firmware routines `07fe`, `080c` and `0812` resolve profile pointers.
Helper `0244` masks pointer bits 14 and 15 before adding the table base.
The editor preserves these flag bits when it replaces a pointer.
Their complete meaning remains unresolved.
The saved format supports one through seven profiles.
The factory fallback forces profile zero and uses shorter pointer lists.
The saved editor therefore rejects factory headers.

EXE function `00619b60` shares equal button tables between profiles.
Its `00633d7c` comparison selects whether to reuse an earlier table offset.
The compiler also shares gesture tables through comparison `00633de8`.
An edit to shared action bytes can therefore change more than one profile.

The constructor at `00617c8c` creates 22 auxiliary records.
The compiler places the auxiliary block before the metadata block.
It writes two metadata words for each profile before allocating action tables.
Firmware routine `0a76` reads these metadata words through header word 10.
Routine `0ad9` reads fields beyond auxiliary action index 15.
These fields must remain separate from action instruction parsing.

The actual factory backup contains these independent excerpts:

```text
auxiliary words 16..21: 0032 0032 0032 0032 0032 0101

metadata word pairs:
01ff 0000
0100 ff00
01ff ff00
0100 00ff
0100 ffff
01ff 00ff
01ff ffff
```

The editor preserves these raw fields. It does not assign guessed parameter names.
The EXE stores `ffff` in header word 12, while the factory backup contains `4034`.
Do not assume that copying a factory header creates an EXE-compatible saved header.

## Offline edits

`bloody_a90_read_profile_map` resolves button or wheel tables in a saved main vector.
It checks model, firmware, profile count, selection, list bounds and table bounds.
It returns relative word offsets and pointer flags separately.
It supports shared tables without treating them as an error.
It does not validate a complete device configuration or its checksum.

`bloody_a90_copy_profile_table` copies one action table to a caller-reserved area.
It updates the selected profile's list entry and both checksum contributions.
It preserves the original table bytes and the pointer's high flag bits.
The function always copies the table, including when its known reference is unique.
It checks destination conflicts with these known regions:

- The header and both profile pointer lists.
- Every button and wheel action table in those lists.
- The complete 22-word auxiliary block.
- Two metadata words per profile.

The caller must separately prove that the destination is unused by gestures, macros and other allocations.
Zero bytes alone do not establish free storage.
The function does not discover a free area or increase the main-vector length.
The caller must supply a complete main vector with a valid original checksum.
Both auxiliary checksum vectors must remain unchanged.
The checksum update follows [MACROS.md](MACROS.md#edit-an-existing-saved-record).

After copying a button table, `bloody_a90_patch_trigger` can change a direct action pair there.
After copying a wheel table, its two words retain down/up direction semantics.
They do not become press/release events.
`bloody_a90_select_profile` changes the saved selected index and updates its checksum contributions.
It preserves the maximum index and checks the new selected index against the profile count.
It does not change runtime state on the device.

## Checks

Run this command from the Piper repository:

```sh
python contrib/bloody-a90/test_storage.py
```

The script compiles the C storage and profile tests in temporary directories.
The profile fixture starts with two profiles that share button and wheel tables.
It copies the second profile's tables and changes only its first button assignment.
The Python backup parser independently checks the resulting profile pointers and actions.
The C tests also check one and seven profiles, preserved flags, overlap errors and selection reversal.
Error cases must leave output unchanged.
AddressSanitizer and UndefinedBehaviorSanitizer also passed the C profile tests.
These checks do not establish device execution or a safe configuration write sequence.
