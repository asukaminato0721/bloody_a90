#!/usr/bin/env python3
"""Compare an A90 firmware-07 flash backup with an official 0C core image.

Usage: compare-firmware.py EXE FW_USB_DIR BACKUP_07 [CORE]

CORE is 1 to 4 (default 2, the core the 07 mouse ran). The script decodes the
official image with compare-cores.py, aligns both code areas (0100..1bff) by
instruction opcodes and prints only addresses, words and short descriptor
excerpts. It writes no firmware to disk and sends no device commands.
"""
import difflib
import importlib.util
import pathlib
import struct
import sys

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("cores", HERE / "compare-cores.py")
cores = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cores)


def words(data):
    return struct.unpack("<%dH" % (len(data) // 2), data)


def opcode(word):
    """Opcode class of an SN8F2288 word with its operand removed."""
    if word & 0x8000:
        return word & 0xc000
    if word & 0x4000:
        return word & 0x7800
    if word >= 0x0d00 and word < 0x1000:
        return word & 0xff00 if word & 0xff else word
    return word & 0xff00


def dispatch(w):
    """Command table: the 0393 B0ADD after BTS0 B9.7 / JMP 3D00."""
    for i in range(0x100, 0x1c00):
        if w[i : i + 4] == (0x57B9, 0xBD00, 0x1EB9, 0x0393):
            base = i + 4
            return base, [None if w[base + n] == 0x0E00 else w[base + n] & 0x3FFF for n in range(0x30)]
    raise SystemExit("command table not found")


def main():
    if len(sys.argv) not in (4, 5):
        raise SystemExit(__doc__)
    exe = pathlib.Path(sys.argv[1]).read_bytes()
    key = cores.pe_bytes(exe, 0x015FCAE8, 64)[::4]
    core = int(sys.argv[4]) if len(sys.argv) == 5 else 2
    path = sorted(pathlib.Path(sys.argv[2]).glob("A90cir_P3332A_4FC_*_0C.sn8encode5"))[core - 1]
    new = words(cores.decode(path.read_bytes(), key))
    old = words(pathlib.Path(sys.argv[3]).read_bytes())
    if (old[0x1804], old[0x1805]) != (0x04FD, 7):
        raise SystemExit("backup is not model 04fd firmware 07")
    print(f"core {core}: {path.name} sha256 {cores.sha(path.read_bytes())}")

    print("\nflash pages (0000 and 3f00 come from staging pages 2400 and 2500 of the image)")
    for page in range(0, 0x4000, 0x100):
        diff = sum(old[page + i] != new[page + i] for i in range(0x100))
        if diff:
            print(f"  {page:04x}: {diff} words differ")

    base_old, table_old = dispatch(old)
    base_new, table_new = dispatch(new)
    print(f"\ncommand table 07 {base_old:04x}, 0C {base_new:04x}")
    for n, (a, b) in enumerate(zip(table_old, table_new)):
        if (a is None) != (b is None):
            print(f"  command {n:02x}: present in only one image")
    if all((a is None) == (b is None) for a, b in zip(table_old, table_new)):
        print("  same commands present")

    a = list(range(0x100, 0x1C00))
    sm = difflib.SequenceMatcher(None, [opcode(old[i]) for i in a], [opcode(new[i]) for i in a], autojunk=False)
    mapping = {}
    print("\ninserted or removed code")
    for tag, i1, i2, j1, j2 in sm.get_opcodes():
        if tag == "equal":
            mapping.update({a[i1 + k]: a[j1 + k] for k in range(i2 - i1)})
        else:
            print(f"  {tag}: 07 {0x100 + i1:04x}+{i2 - i1}  0C {0x100 + j1:04x}+{j2 - j1}")
    print("\naligned words whose operand differs (jumps to aligned targets omitted)")
    for x, y in sorted(mapping.items()):
        u, v = old[x], new[y]
        if u == v:
            continue
        if u & 0x8000 and (u & 0xC000) == (v & 0xC000) and mapping.get(u & 0x3FFF) == v & 0x3FFF:
            continue
        print(f"  07 {x:04x} {u:04x}  0C {y:04x} {v:04x}")

    print("\nfactory headers (RIDATA, sensitivity address, model, firmware)")
    for name, w in (("07", old), ("0C", new)):
        at = next(i for i in range(0x1700, 0x1C00) if w[i : i + 2] == (0x2218, 0x4952))
        print(f"  {name}: header {at:04x}, words {' '.join('%04x' % v for v in w[at + 7 : at + 12])}")
    sens_old, sens_new = 0x1806, new[next(i for i in range(0x1700, 0x1C00) if new[i : i + 2] == (0x2218, 0x4952)) + 8]
    for i in range(54):
        if old[sens_old + i] != new[sens_new + i]:
            print(f"  factory sensitivity +{i:02x}: 07 {old[sens_old + i]:04x}  0C {new[sens_new + i]:04x}")
    print("  factory key table 07 183c / 0C 187a:",
          "same" if old[0x183C:0x18BB] == new[0x187A:0x18F9] else "different")

    print("\ndevice data 3ff0..3fff (07 flash) and 25f0..25ff (0C staging)")
    print("  07", " ".join("%04x" % v for v in old[0x3FF0:0x4000]))
    print("  0C", " ".join("%04x" % v for v in new[0x25F0:0x2600]))


if __name__ == "__main__":
    main()
