#!/usr/bin/env python3
"""Read-only uhid bridge that lets the official program under Wine see the A90.

Wine's winebus ignores hidraw devices whose first collection is a mouse or a
keyboard. The A90 vendor interface (usage page ff32) starts with a keyboard
collection, so Wine hides it. This bridge creates a virtual HID device with
the same USB ID and only the vendor collection, then forwards feature report 7
to the real interface.

Only read commands reach the mouse. Every other command is answered with an
empty report and logged, so the official program cannot save, erase or send
the commands listed in docs/research/WRITES.md.

Needs contrib/research/71-a90-uhid-bridge.rules and the uhid module
(`sudo modprobe uhid`). Pass -v to log forwarded commands too. Run it while neither this
tool nor another program uses the A90, start Wine, and stop it with Ctrl+C.
"""

import fcntl
import os
import select
import struct
import sys
import time

UHID_DESTROY = 1
UHID_START = 2
UHID_OPEN = 4
UHID_CLOSE = 5
UHID_OUTPUT = 6
UHID_GET_REPORT = 9
UHID_GET_REPORT_REPLY = 10
UHID_CREATE2 = 11
UHID_INPUT2 = 12
UHID_SET_REPORT = 13
UHID_SET_REPORT_REPLY = 14
UHID_FEATURE_REPORT = 0
EVENT_SIZE = 4 + 128 + 64 + 64 + 2 + 2 + 4 * 4 + 4096
BUS_USB = 3
EIO = 5

REPORT = 64
ID = 7
UNLOCK = bytes([0x52, 0x49, 0x18, 0x22])

# Vendor collection of the A90 interface 2: input and feature report 7.
DESCRIPTOR = bytes.fromhex(
    "0632ff0afd04a1018507"
    "1901293f150026ff007508953f8100"
    "1901293f150026ff007508953fb102"
    "c0"
)


def hidioc(direction, nr, size):
    return (direction << 30) | (size << 16) | (ord("H") << 8) | nr


HIDIOCSFEATURE = hidioc(3, 6, REPORT)
HIDIOCGFEATURE = hidioc(3, 7, REPORT)


def allowed(report):
    """Commands that only read state. Byte 1 is the command."""
    command = report[1]
    if command in (0x1F, 0x05, 0x06, 0x02, 0x81):
        return True
    # 11 brightness and 13 lift height write when byte 4 bit 7 is set.
    if command in (0x11, 0x13):
        return report[4] & 0x80 == 0
    # 2f writes flash when byte 2 bit 0 is set (firmware 137a).
    if command == 0x2F:
        return report[2] & 1 == 0
    # 29 sub-command 0 copies device data 3ff4..3ff7 (firmware 13c0);
    # other sub-commands go to the 13d6 path.
    if command == 0x29:
        return report[2] == 0
    # 28 reads word 0100 when byte 2 bit 0 is clear; set, it calls 1245
    # with 01c0 (firmware 136d). 00 jumps to the reset path 0268.
    if command == 0x28:
        return report[2] & 1 == 0
    # 2a reads word 3ff3 when byte 2 bit 0 is clear (firmware 13ed). Set,
    # it programs 3ff3 after a key check, or rewrites page 3f00.
    if command == 0x2A:
        return report[2] & 1 == 0
    # 25 with another magic locks the command until reset.
    if command == 0x25:
        return report[4:8] == UNLOCK
    return False


# The official program programs a body ID into 3ff4..3ff7 when those words
# are blank. That page holds boot code and device data, so the bridge never
# forwards the write. Instead it answers 29/0 with this placeholder ID; the
# mouse keeps its blank words.
PLACEHOLDER_ID = bytes.fromhex("a9 0b 1d 00 a9 0b 1d 00")


# After identifying the mouse, the official program decides at EXE 0044cfde
# to post message 0x41b, which sends 28/1 (programs the boot marker at 0100,
# firmware 136d -> 1245) and 00 (reset path 0268). Without the marker the
# reset path at 0017 starts the bootloader at 3f00. The bridge blocks both;
# the program then waits at 0% and does not show its pages.


def find_vendor():
    for name in sorted(os.listdir("/sys/class/hidraw")):
        base = f"/sys/class/hidraw/{name}/device"
        try:
            uevent = open(f"{base}/uevent").read()
            descriptor = open(f"{base}/report_descriptor", "rb").read()
        except OSError:
            continue
        if "HID_ID=0003:000009DA:0000FAFD" not in uevent:
            continue
        # Skip this bridge's own device: it has no keyboard collection.
        if b"\x06\x32\xff" in descriptor and descriptor != DESCRIPTOR:
            return f"/dev/{name}"
    sys.exit("找不到 A90 的厂商接口")


def log(text):
    print(time.strftime("%H:%M:%S"), text, flush=True)


def main():
    verbose = "-v" in sys.argv[1:]
    path = find_vendor()
    device = os.open(path, os.O_RDWR | os.O_NONBLOCK)
    uhid = os.open("/dev/uhid", os.O_RDWR)
    name = b"COMPANY  USB Device (A90 bridge)"
    create = struct.pack(
        "<I128s64s64sHHIIII4096s",
        UHID_CREATE2,
        name,
        b"a90-uhid-bridge",
        b"",
        len(DESCRIPTOR),
        BUS_USB,
        0x09DA,
        0xFAFD,
        0x0007,
        0,
        DESCRIPTOR,
    )
    os.write(uhid, create)
    log(f"桥接 {path}，只放行读取命令")
    pending = None
    last = None
    try:
        while True:
            ready, _, _ = select.select([uhid, device], [], [])
            if device in ready:
                try:
                    data = os.read(device, REPORT)
                except BlockingIOError:
                    data = b""
                # Forward status notifications of report 7 only.
                if data and data[0] == ID:
                    os.write(
                        uhid,
                        struct.pack("<IH4096s", UHID_INPUT2, len(data), data).ljust(
                            EVENT_SIZE, b"\0"
                        ),
                    )
            if uhid not in ready:
                continue
            event = os.read(uhid, EVENT_SIZE)
            kind = struct.unpack_from("<I", event)[0]
            if kind == UHID_SET_REPORT:
                request, number, rtype, size = struct.unpack_from("<IBBH", event, 4)
                data = bytes(event[12 : 12 + size])
                report = bytearray(REPORT)
                report[: len(data)] = data[:REPORT]
                report[0] = number or ID
                error = 0
                if rtype != UHID_FEATURE_REPORT or report[0] != ID:
                    error = EIO
                elif allowed(report):
                    last = report[1], report[2]
                    if verbose:
                        log(f"放行命令 {report[1]:02x}：{report[:12].hex(' ')}")
                    try:
                        fcntl.ioctl(device, HIDIOCSFEATURE, bytes(report))
                        pending = None
                    except OSError as failure:
                        error = failure.errno
                else:
                    log(f"拦截命令 {report[1]:02x}：{report[:12].hex(' ')}")
                    pending = bytes([ID, report[1]]) + bytes(REPORT - 2)
                os.write(
                    uhid,
                    struct.pack("<IIH", UHID_SET_REPORT_REPLY, request, error).ljust(
                        EVENT_SIZE, b"\0"
                    ),
                )
            elif kind == UHID_GET_REPORT:
                request, number, rtype = struct.unpack_from("<IBB", event, 4)
                error, data = 0, b""
                if rtype != UHID_FEATURE_REPORT or (number or ID) != ID:
                    error = EIO
                elif pending is not None:
                    data, pending = pending, None
                else:
                    buffer = bytearray(REPORT)
                    buffer[0] = ID
                    try:
                        fcntl.ioctl(device, HIDIOCGFEATURE, buffer)
                        if last == (0x29, 0) and buffer[1] == 0x29 and buffer[8:16] == b"\xff" * 8:
                            buffer[8:16] = PLACEHOLDER_ID
                            log("机身码ID 为空，回复占位 ID")
                        data = bytes(buffer)
                    except OSError as failure:
                        error = failure.errno
                os.write(
                    uhid,
                    struct.pack(
                        "<IIHH4096s",
                        UHID_GET_REPORT_REPLY,
                        request,
                        error,
                        len(data),
                        data,
                    ),
                )
            elif kind == UHID_OUTPUT:
                log("拦截输出报告")
            elif kind in (UHID_START, UHID_OPEN, UHID_CLOSE):
                log({UHID_START: "已创建", UHID_OPEN: "已打开", UHID_CLOSE: "已关闭"}[kind])
    except KeyboardInterrupt:
        pass
    finally:
        os.write(uhid, struct.pack("<I", UHID_DESTROY).ljust(EVENT_SIZE, b"\0"))
        log("已移除虚拟设备")


if __name__ == "__main__":
    main()
