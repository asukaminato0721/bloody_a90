#!/usr/bin/env python3
"""Compare local A90 core images. Read files only. Never access USB.

EXE 0045c5ac skips a 256-byte header after 013c792c decodes the file.
013c7814 XORs the ciphertext with the four-byte trailer.
013c6498/013c6504 obtain 16 AES key bytes from the local EXE's DWORD table.
013c7744 decodes each 16-byte block independently.
The script emits metadata and instruction differences, not decoded firmware.
"""

import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess


EXE_HASH = "4a76f6ef4b6e7da699afbd8d3d780ae9747b599af795531a39e3eb5d93c93162"
FILE_HASHES = [
    "8d8b74fd483892341dac9e47e60c1f9cb35d9890c3d46d245007acb021ca642d",
    "45d8a8e7daada2a41bc8ec0beae8284ac32e6036561bd5bbd333a1565634c425",
    "af97bbd4d8a51eece4a7b29aa1939c73e96bc603b0435abc17e140307821872d",
    "08fe3c9cf85b92675b8958ff75ec49a4111e952820a4356c9a9bb193df63d670",
]


def sha(data):
    return hashlib.sha256(data).hexdigest()


def pe_bytes(exe, address, size):
    """Resolve a virtual address through the PE section table."""
    pe = struct.unpack_from("<I", exe, 0x3C)[0]
    if exe[pe:pe + 4] != b"PE\0\0":
        raise ValueError("Invalid PE signature")
    count = struct.unpack_from("<H", exe, pe + 6)[0]
    optional_size = struct.unpack_from("<H", exe, pe + 20)[0]
    base = struct.unpack_from("<I", exe, pe + 52)[0]
    for i in range(count):
        section = pe + 24 + optional_size + i * 40
        _, rva, raw_size, raw = struct.unpack_from("<IIII", exe, section + 8)
        offset = address - base - rva
        if 0 <= offset and offset + size <= raw_size:
            return exe[raw + offset:raw + offset + size]
    raise ValueError("Virtual address has no complete file-backed section")


def decode(data, key):
    if len(data) != 0x8104:
        raise ValueError("A90 image must contain 0x8104 bytes")
    mask = data[-4:]
    ciphertext = bytes(value ^ mask[i % 4] for i, value in enumerate(data[:-4]))
    result = subprocess.run(
        ["openssl", "enc", "-aes-128-ecb", "-d", "-nopad", "-K", key.hex()],
        input=ciphertext, capture_output=True, check=True,
    ).stdout
    if len(result) != 0x8100 or result[:0x100] != bytes(0x100):
        raise ValueError("Decoded header does not match the checked A90 files")
    return result[0x100:]


def hooks(words, press, release):
    return {
        "press_address": f"{press:04x}",
        "press_words": [f"{word:04x}" for word in words[press:press + 2]],
        "release_address": f"{release:04x}",
        "release_words": [f"{word:04x}" for word in words[release:release + 2]],
    }


def compare(exe_path, firmware_dir, backup_path):
    exe = exe_path.read_bytes()
    if sha(exe) != EXE_HASH:
        raise ValueError("EXE hash differs from the analyzed program")
    key = pe_bytes(exe, 0x015FCAE8, 64)[::4]
    images = []
    packaged = []
    for index, expected in enumerate(FILE_HASHES):
        path = firmware_dir / f"A90cir_P3332A_4FC_{index}_0C.sn8encode5"
        data = path.read_bytes()
        if sha(data) != expected:
            raise ValueError(f"Firmware file hash differs: {path.name}")
        image = decode(data, key)
        words = struct.unpack("<16384H", image)
        if (words[0x100], words[0x101], words[0x601]) != (0xA4A4, 0x0C, 0x04FC + index):
            raise ValueError(f"Decoded identity differs: {path.name}")
        images.append(words)
        packaged.append({
            "file": path.name,
            "file_sha256": sha(data),
            "image_sha256": sha(image),
            "core": index + 1,
            "model": f"{words[0x601]:04x}",
            "revision": f"{words[0x101]:02x}",
            "left_dispatch": hooks(words, 0x717, 0x739),
        })
    data = backup_path.read_bytes()
    if len(data) != 32768:
        raise ValueError("Installed backup must contain 32768 bytes")
    words = struct.unpack("<16384H", data)
    if words[0x1804:0x1806] != (0x04FD, 7):
        raise ValueError("Installed backup must match model 04fd, firmware 07")
    differences = [
        {"address": f"{address:04x}", "core_words": [f"{image[address]:04x}" for image in images]}
        for address in range(16384)
        if len({image[address] for image in images}) > 1
    ]
    return {
        "executable_sha256": sha(exe),
        "packaged": packaged,
        "differences": differences,
        "installed": {
            "sha256": sha(data),
            "model": "04fd",
            "revision": "07",
            "left_dispatch": hooks(words, 0x708, 0x72A),
        },
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("executable", type=Path)
    parser.add_argument("firmware_dir", type=Path)
    parser.add_argument("backup", type=Path)
    args = parser.parse_args()
    print(json.dumps(compare(args.executable, args.firmware_dir, args.backup), indent=2))
