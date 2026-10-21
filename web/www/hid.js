// A90 vendor HID transport over WebHID, firmware 07 and 0c. A port of
// src/device.rs: the same requests, sequences and read-back checks.
// See docs/research/PROTOCOL.md.

export const REPORT = 64;
export const VENDOR = 0x09da;
/// Firmware 07 enumerates as fafd, firmware 0c as 5450.
export const PRODUCTS = [0xfafd, 0x5450];
export const USAGE_PAGE = 0xff32;
const ID = 7;
export const SENSITIVITY = 0x2400;
export const BUTTONS = 0x2600;
/// Command handlers live in page 3d00; mode-80 writes erase whole pages.
export const BUTTONS_END = 0x3d00;
export const FLASH_WORDS = 0x4000;
/// Firmware versions whose layout the tool knows (backup::Layout).
const FIRMWARES = [0x07, 0x0c];
// Command 25 magic. A wrong magic locks the command until the next reset.
const UNLOCK = [0x52, 0x49, 0x18, 0x22];

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const hex = (value, width) => value.toString(16).padStart(width, "0");

export function supported() {
  return "hid" in navigator;
}

function header(command, bytes = [0, 0, 0, 0, 0, 0]) {
  const report = new Uint8Array(REPORT);
  report[0] = ID;
  report[1] = command;
  report.set(bytes, 2);
  return report;
}

function putWords(report, words) {
  const view = new DataView(report.buffer);
  words.forEach((word, index) => view.setUint16(8 + index * 2, word, true));
}

/// Header byte 2 encodes the word count, as in EXE 005a0938.
function writeReport(address, words, erase) {
  const report = header(0x2f, [
    ((words.length - 1) * 8 + 1) & 0xff,
    address >> 8,
    address & 0xff,
    erase ? 0x80 : 0,
    0,
    0,
  ]);
  putWords(report, words);
  return report;
}

/// Firmware 137a accepts 2400..=3e00. Narrowed to the sensitivity page and
/// the button area below the code page at 3d00.
function writable(address, count) {
  const end = address + count;
  return (
    (address >= SENSITIVITY && end <= SENSITIVITY + 0x100) ||
    (address >= BUTTONS && end <= BUTTONS_END)
  );
}

function equalWords(a, b) {
  return a.length === b.length && a.every((word, index) => word === b[index]);
}

/// Saved blocks begin with a4a4 and carry two checksums over words 4..
/// (storage::checksums). The caller passes the wasm implementation.
let checksums = null;
export function setChecksums(fn) {
  checksums = fn;
}

/// Find the vendor collection: usage page ff32, usage 04fc..04ff.
function vendorCollection(device) {
  return device.collections.find(
    (c) => c.usagePage === USAGE_PAGE && (c.usage & 0xfffc) === 0x04fc,
  );
}

export async function pairedDevices() {
  const devices = await navigator.hid.getDevices();
  return devices.filter(
    (d) =>
      d.vendorId === VENDOR &&
      PRODUCTS.includes(d.productId) &&
      vendorCollection(d),
  );
}

export async function requestDevice() {
  const devices = await navigator.hid.requestDevice({
    filters: PRODUCTS.map((productId) => ({
      vendorId: VENDOR,
      productId,
      usagePage: USAGE_PAGE,
    })),
  });
  return devices.find(vendorCollection) ?? null;
}

export class Device {
  constructor(hid) {
    this.hid = hid;
    // One page owns complete request/reply pairs. The Web Lock also keeps
    // other tabs of this site from interleaving with them.
    this.queue = Promise.resolve();
  }

  /// Open the device and check that it is an analyzed A90 firmware.
  static async open(hid) {
    if (!hid.opened) await hid.open();
    const device = new Device(hid);
    const identity = await device.identity();
    if (
      (identity.model & 0xfffc) !== 0x04fc ||
      !FIRMWARES.includes(identity.firmware)
    ) {
      throw new Error(
        `设备型号 ${hex(identity.model, 4)}、固件 ${hex(identity.firmware, 2)} 不是已分析的 A90 固件 07 或 0c`,
      );
    }
    return device;
  }

  async close() {
    await this.queue.catch(() => {});
    if (this.hid.opened) await this.hid.close();
  }

  /// Run one complete operation exclusively.
  exclusive(operation) {
    const run = () =>
      "locks" in navigator
        ? navigator.locks.request("bloody-a90-hid", () => operation())
        : operation();
    const result = this.queue.then(run, run);
    this.queue = result.catch(() => {});
    return result;
  }

  async send(report) {
    await this.hid.sendFeatureReport(report[0], report.subarray(1));
  }

  async receive() {
    const view = await this.hid.receiveFeatureReport(ID);
    let bytes = new Uint8Array(view.buffer, view.byteOffset, view.byteLength);
    // Chromium returns the report ID as the first byte; be tolerant of a
    // platform that leaves it out.
    if (bytes.length === REPORT - 1) {
      const full = new Uint8Array(REPORT);
      full[0] = ID;
      full.set(bytes, 1);
      bytes = full;
    }
    if (bytes.length !== REPORT) {
      throw new Error(`HID 请求只传输了 ${bytes.length} 字节`);
    }
    return bytes;
  }

  /// Send a request, then read a fresh response with the same header.
  async query(request) {
    await this.send(request);
    for (let attempt = 0; attempt < 20; attempt++) {
      await sleep(3);
      const response = await this.receive();
      if (response.subarray(0, 8).every((byte, i) => byte === request[i])) {
        return response;
      }
    }
    throw new Error(`命令 ${hex(request[1], 2)} 的响应头不匹配`);
  }

  async identity() {
    const r = await this.query(header(0x1f));
    const model = r[10] | (r[11] << 8);
    // All four official 0c core images differ only in the model word.
    return { model, firmware: r[12], core: (r[10] & 3) + 1 };
  }

  async memoryInfo() {
    const r = await this.query(header(0x05));
    const word = (offset) => r[8 + offset] | (r[9 + offset] << 8);
    return {
      rateHz: word(2),
      sensitivity: word(4),
      buttons: word(6),
      settings: word(8),
      factorySensitivity: word(10),
      deviceData: word(12),
      factoryButtons: word(14),
    };
  }

  /// Brightness level 0 to 3.
  async brightness() {
    const level = (await this.query(header(0x11)))[8];
    if (level > 3) throw new Error(`亮度响应 ${level} 超出 0 至 3`);
    return level;
  }

  /// Set and read back the brightness.
  setBrightness(level) {
    return this.exclusive(async () => {
      if (!(level >= 0 && level <= 3)) throw new Error("亮度应为 0 至 3");
      const report = header(0x11, [0, 0, 0x80, 0, 0, 0]);
      report[8] = level;
      await this.send(report);
      await sleep(30);
      const read = await this.brightness();
      if (read !== level) throw new Error(`亮度读回 ${read}，期望 ${level}`);
    });
  }

  /// One RAM byte, firmware routine 3d23.
  async ram(address) {
    return (await this.query(header(0x81, [0, address >> 8, address & 0xff, 0, 0, 0])))[8];
  }

  /// Runtime profile index and maximum index, RAM 263 and 264.
  async runtimeProfile() {
    return [await this.ram(0x263), await this.ram(0x264)];
  }

  /// Command 0c, firmware 1322 and 0694. The firmware applies the selection
  /// after all held buttons are released. No flash write.
  async selectProfile(index) {
    const [, maximum] = await this.runtimeProfile();
    if (index > maximum || index >= 7) {
      throw new Error("配置序号超出鼠标的配置数量");
    }
    if ((await this.ram(0x262)) & 0x80) {
      throw new Error("鼠标仍在等待上一次配置切换。请松开全部按键后重试。");
    }
    const request = header(0x0c);
    request[8] = maximum;
    request[9] = 0x80 | index;
    request[10] = await this.ram(0x25);
    await this.query(request);
    for (let attempt = 0; attempt < 50; attempt++) {
      const current = await this.runtimeProfile();
      if (current[0] === index && ((await this.ram(0x262)) & 0x80) === 0) {
        return current;
      }
      await sleep(20);
    }
    throw new Error("配置切换尚未完成。松开全部按键后，鼠标会执行待处理的切换。");
  }

  /// Runtime report-rate code (RAM 23) and the settings register (RAM 112).
  async runtimeRate() {
    return [await this.ram(0x23), await this.ram(0x112)];
  }

  /// The eight bytes of command 0d, read from RAM 26..2a, 23, 2d, 2e.
  async runtimeSensitivity() {
    const payload = new Uint8Array(8);
    const addresses = [0x26, 0x27, 0x28, 0x29, 0x2a, 0x23, 0x2d, 0x2e];
    for (const [index, address] of addresses.entries()) {
      payload[index] = await this.ram(address);
    }
    return payload;
  }

  /// EXE 005a1dc8, firmware 12e1. `validate` checks a payload
  /// (sensitivity::Runtime::from_payload). No flash write.
  async applyRuntimeSensitivity(payload, validate) {
    validate(payload);
    const request = header(0x0d);
    request.set(payload, 8);
    await this.query(request);
    for (let attempt = 0; attempt < 20; attempt++) {
      const current = await this.runtimeSensitivity();
      if (
        current.every((byte, i) => byte === payload[i]) &&
        (await this.ram(0x162)) === payload[4]
      ) {
        if (((await this.ram(0x157)) & 1) === 0) {
          throw new Error("传感器尚未完成初始化");
        }
        return;
      }
      await sleep(10);
    }
    throw new Error("即时灵敏度的运行参数或传感器状态读回不一致");
  }

  /// EXE 005a1d54. Only the response-time byte changes.
  async setDebounceMs(ms) {
    if (!(ms >= 0 && ms <= 16)) throw new Error("硬件按键响应时间应为 0 至 16 ms");
    const request = header(0x0c);
    request[8] = await this.ram(0x264);
    request[9] = await this.ram(0x262);
    request[10] = ms + 1;
    await this.query(request);
    if ((await this.ram(0x25)) !== ms + 1) {
      throw new Error("硬件按键响应时间的运行值读回不一致");
    }
  }

  async liftStatus() {
    const response = await this.query(header(0x13));
    return { cutoff: response[8], minimum: response[9] };
  }

  /// EXE 005a28c8, firmware 1202. Level 1..16, or null for the preset height.
  async setLiftHeight(level) {
    let code;
    if (level === null || level === undefined) {
      code = (await this.liftStatus()).cutoff & ~0x40 & 0xff;
    } else if (level >= 1 && level <= 16) {
      code = 0xc0 | (16 - level);
    } else {
      throw new Error("抬起高度应为 1 至 16 级");
    }
    const request = header(0x13, [0, 0, 0x80, 0, 0, 0]);
    request[8] = code;
    await this.query(request);
    if ((await this.liftStatus()).cutoff !== code) {
      throw new Error("抬起高度的运行值读回不一致");
    }
  }

  async setCalibrationMode(value) {
    const request = header(0x12);
    request[8] = value;
    await this.query(request);
    if ((await this.ram(0x111)) !== value) throw new Error("校准状态读回不一致");
  }

  /// Collect the minimum surface quality, then restore the previous mode.
  /// Returns the official 1..16 level without changing the saved cutoff.
  calibrateLift(seconds) {
    return this.exclusive(async () => {
      if (!(seconds >= 1 && seconds <= 30)) throw new Error("校准时间应为 1 至 30 秒");
      const previous = await this.ram(0x111);
      if (previous & 0x80) throw new Error("鼠标已经处于校准状态");
      let active = true;
      try {
        await this.setCalibrationMode(previous);
        await sleep(30);
        await this.setCalibrationMode(previous | 0x80);
        for (let tick = 0; tick < seconds * 10; tick++) {
          await sleep(100);
          await this.liftStatus();
        }
        const { minimum } = await this.liftStatus();
        await this.setCalibrationMode(previous);
        active = false;
        // EXE 0044bc60 / 0044bf04. Bit 7 means the sensor supplied a sample.
        if ((minimum & 0x80) === 0) {
          throw new Error("校准没有收到有效样本。请在使用表面上移动鼠标后重试。");
        }
        return 16 - Math.min(minimum & 0x1f, 15);
      } finally {
        if (active) await this.setCalibrationMode(previous).catch(() => {});
      }
    });
  }

  /// Buttons 1 to 24 currently held (firmware 1126, RAM 25e..260).
  heldButtons() {
    return this.exclusive(async () => {
      const held = [];
      for (let index = 0; index < 3; index++) {
        const byte = await this.ram(0x25e + index);
        for (let bit = 0; bit < 8; bit++) {
          if (byte & (1 << bit)) held.push(index * 8 + bit + 1);
        }
      }
      return held;
    });
  }

  async flashAccess(enabled) {
    const report = header(0x25, [0, 0, ...UNLOCK]);
    report[8] = enabled ? 1 : 0;
    await this.send(report);
    await sleep(3);
  }

  /// Read flash words. Reads are enabled for the call and disabled again.
  async readFlash(start, count, progress = () => {}) {
    if (start + count > FLASH_WORDS) throw new Error("读取范围超出 16384 个字");
    await this.flashAccess(true);
    try {
      const words = new Uint16Array(count + 28);
      let length = 0;
      while (length < count) {
        const address = start + length;
        const response = await this.query(
          header(0x02, [0, address >> 8, address & 0xff, 0x60, 0, 0]),
        );
        const view = new DataView(response.buffer);
        for (let offset = 8; offset < REPORT; offset += 2) {
          words[length++] = view.getUint16(offset, true);
        }
        progress(Math.min(length, count), count);
      }
      return words.slice(0, count);
    } finally {
      await this.flashAccess(false);
    }
  }

  /// The complete flash as 32768 little-endian bytes.
  readBackup(progress) {
    return this.exclusive(async () => {
      const words = await this.readFlash(0, FLASH_WORDS, progress);
      return new Uint8Array(words.buffer, words.byteOffset, words.byteLength).slice();
    });
  }

  /// Command 20 access, EXE 005a1fa8. Poll until response byte 8 is zero.
  async access(open) {
    const request = header(0x20, [0, 0, 0, 0, 0, open ? 1 : 0]);
    for (let attempt = 0; attempt < 1000; attempt++) {
      const response = await this.query(request);
      await sleep(3);
      if (response[8] === 0) return;
    }
    throw new Error("设备在 1000 次查询后仍未就绪");
  }

  /// Write `staged` in 16-word chunks with page erase, inside one access
  /// window. `commit` writes the a4a4 marker alone at the end.
  async writeChunks(address, staged, commit) {
    await this.access(true);
    await sleep(3);
    let failure = null;
    try {
      for (let index = 0; index * 16 < staged.length; index++) {
        const chunk = staged.subarray(index * 16, index * 16 + 16);
        await this.send(writeReport(address + index * 16, chunk, true));
        await sleep(3);
      }
      if (commit) {
        await this.send(writeReport(address, [0xa4a4], false));
        await sleep(3);
      }
    } catch (error) {
      failure = error;
    }
    // Always close the access window, then report the first failure.
    await this.access(false).catch((error) => {
      failure ??= error;
    });
    if (failure) throw failure;
  }

  /// Write words exactly as given, for restoring a backup. The range must
  /// be page-aligned and inside an allowed area.
  async restore(address, words) {
    if (
      address % 0x100 !== 0 ||
      words.length % 16 !== 0 ||
      words.length === 0 ||
      !writable(address, words.length)
    ) {
      throw new Error("恢复范围必须按页对齐，并位于允许写入的区域");
    }
    await this.writeChunks(address, Uint16Array.from(words), false);
    const read = await this.readFlash(address, words.length);
    if (!equalWords(read, words)) throw new Error("恢复后读回不一致");
  }

  /// Save one complete block: a sensitivity block at 2400 or a button main
  /// vector at 2600. Sequence from EXE 0043c5d8 and 0043b1e8: open access,
  /// stage the header as ffff/ffff, write 16-word chunks with page erase,
  /// commit a4a4 alone, close access. Then read back.
  async save(address, block) {
    const validLength =
      address === SENSITIVITY
        ? block.length === 128
        : address === BUTTONS
          ? block.length >= 32 && block.length <= BUTTONS_END - BUTTONS
          : false;
    const padded = Math.ceil(block.length / 16) * 16;
    if (!validLength || !writable(address, padded)) {
      throw new Error("保存地址或长度不在允许范围内");
    }
    const [a, b] = checksums(block);
    if (block[0] !== 0xa4a4 || block[1] !== 0xffff || a !== block[2] || b !== block[3]) {
      throw new Error("保存块的头部或校验值无效");
    }
    // Saved tables use the runtime profile index directly. An index past
    // the new profile list reads a garbage pointer and disables buttons.
    if (address === BUTTONS) {
      const [current] = await this.runtimeProfile();
      if (current > block[4] >> 8) {
        throw new Error(
          `鼠标正在使用配置 ${current + 1}，新按键表只有 ${(block[4] >> 8) + 1} 个配置。请先切换到范围内的配置。`,
        );
      }
    }
    const staged = new Uint16Array(padded);
    staged.set(block);
    staged[0] = 0xffff;
    staged[1] = 0xffff;
    await this.writeChunks(address, staged, true);

    staged[0] = 0xa4a4;
    const read = await this.readFlash(address, padded);
    const first = read.findIndex((word, i) => word !== staged[i]);
    if (first >= 0) {
      throw new Error(
        `保存后读回不一致：字地址 ${hex(address + first, 4)} 读到 ${hex(read[first], 4)}，期望 ${hex(staged[first], 4)}`,
      );
    }
    if (address === BUTTONS && (block[6] & 0xff) >= 1 && (block[6] & 0xff) <= 17) {
      await this.setDebounceMs((block[6] & 0xff) - 1);
    }
  }

  // ---- Complete operations, each holding the lock ----

  /// Identity, memory map, brightness, running state and the whole flash.
  snapshot(progress) {
    return this.exclusive(async () => {
      const identity = await this.identity();
      const info = await this.memoryInfo();
      const brightness = await this.brightness();
      const profile = await this.runtimeProfile();
      const rate = await this.runtimeRate();
      const words = await this.readFlash(0, FLASH_WORDS, progress);
      const bytes = new Uint8Array(words.buffer, words.byteOffset, words.byteLength).slice();
      return { identity, info, brightness, profile, rate, bytes };
    });
  }

  /// Apply the saved preset, rate and lift cutoff without a flash write.
  async applySensitivity(block, runtime) {
    await this.applyRuntimeSensitivity(runtime.payload(block), runtime.validate);
    await this.setLiftHeight(runtime.lift(block));
  }

  saveSensitivity(block, runtime) {
    return this.exclusive(async () => {
      runtime.payload(block);
      runtime.lift(block);
      await this.save(SENSITIVITY, block);
      try {
        await this.applySensitivity(block, runtime);
      } catch (error) {
        throw new Error(`灵敏度已保存，但即时应用未完成：${error.message}`);
      }
    });
  }

  /// Preserve the preset currently running on the mouse. It can differ
  /// from the saved selection after a hardware DPI button was used.
  saveRate(block, runtime) {
    return this.exclusive(async () => {
      const code = block[0x1e];
      if (![1, 2, 4, 8].includes(code)) throw new Error("回报率编码无效");
      runtime.validate(await this.runtimeSensitivity());
      await this.save(SENSITIVITY, block);
      try {
        const current = await this.runtimeSensitivity();
        current[5] = code;
        await this.applyRuntimeSensitivity(current, runtime.validate);
      } catch (error) {
        throw new Error(`回报率已保存，但即时应用未完成：${error.message}`);
      }
    });
  }

  saveLift(block, runtime) {
    return this.exclusive(async () => {
      const level = runtime.lift(block);
      await this.save(SENSITIVITY, block);
      try {
        await this.setLiftHeight(level);
      } catch (error) {
        throw new Error(`抬起高度已保存，但即时应用未完成：${error.message}`);
      }
    });
  }

  saveButtons(block) {
    return this.exclusive(() => this.save(BUTTONS, block));
  }

  /// Save a key table whose change is the light effect, then reload the
  /// running profile, which reads its metadata again.
  saveLighting(block) {
    return this.exclusive(async () => {
      await this.save(BUTTONS, block);
      const [current] = await this.runtimeProfile();
      await this.selectProfile(current);
    });
  }

  /// Restore the sensitivity page and the button pages that differ from a
  /// backup image, as `bloody_a90 --restore` does. Returns the pages written.
  restoreBackup(bytes, runtime, debounceMs) {
    return this.exclusive(async () => {
      const words = new Uint16Array(bytes.buffer, bytes.byteOffset, FLASH_WORDS);
      const current = await this.readFlash(0, FLASH_WORDS);
      const pages = [SENSITIVITY];
      for (let page = BUTTONS; page < BUTTONS_END; page += 0x100) pages.push(page);
      const restored = [];
      for (const page of pages) {
        const wanted = words.subarray(page, page + 0x100);
        if (!equalWords(current.subarray(page, page + 0x100), wanted)) {
          await this.restore(page, wanted);
          restored.push(page);
        }
      }
      if (debounceMs !== undefined && debounceMs !== null) {
        await this.setDebounceMs(debounceMs);
      }
      const template = runtime.template();
      if (template) await this.applySensitivity(template, runtime);
      return restored;
    });
  }
}
