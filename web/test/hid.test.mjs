// Drives www/hid.js against a simulated A90 that holds a real flash image.
// Build first (web/build.sh), then:
//   A90_BACKUP=/path/to/backup.bin node --test web/test/
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import init, * as wasm from "../dist/pkg/bloody_a90_web.js";
import * as hid from "../dist/hid.js";

const path = process.env.A90_BACKUP;
const image = path ? new Uint8Array(readFileSync(path)) : null;

/// Feature-report behaviour of firmware 07/0c as documented in
/// docs/research/PROTOCOL.md, reduced to what hid.js uses.
class FakeHid {
  constructor(bytes, model, firmware) {
    this.flash = new Uint16Array(bytes.slice().buffer);
    this.ram = new Uint8Array(0x300);
    this.model = model;
    this.firmware = firmware;
    this.brightness = 2;
    this.minimum = 0x87;
    this.opened = false;
    this.reads = false;
    this.access = false;
    this.writes = [];
    this.productName = "Simulated A90";
    this.collections = [{ usagePage: 0xff32, usage: model }];
  }
  async open() {
    this.opened = true;
  }
  async close() {
    this.opened = false;
  }
  async receiveFeatureReport(id) {
    assert.equal(id, 7);
    return new DataView(this.response.slice().buffer);
  }
  async sendFeatureReport(id, data) {
    assert.equal(data.length, 63);
    const r = new Uint8Array(64);
    r[0] = id;
    r.set(data, 1);
    const response = r.slice();
    const address = (r[3] << 8) | r[4];
    const ram = this.ram;
    switch (r[1]) {
      case 0x1f:
        response[10] = this.model & 0xff;
        response[11] = this.model >> 8;
        response[12] = this.firmware;
        break;
      case 0x05:
        response.fill(0, 8);
        break;
      case 0x11:
        if (r[4] === 0x80) this.brightness = r[8];
        response[8] = this.brightness;
        break;
      case 0x81:
        response[8] = ram[address];
        break;
      case 0x0c:
        ram[0x264] = r[8];
        if (r[9] & 0x80) ram[0x263] = r[9] & 0x7f;
        ram[0x262] = r[9] & 0x7f;
        ram[0x25] = r[10];
        break;
      case 0x0d:
        [0x26, 0x27, 0x28, 0x29, 0x2a, 0x23, 0x2d, 0x2e].forEach((a, i) => (ram[a] = r[8 + i]));
        ram[0x162] = r[12];
        ram[0x157] |= 1;
        break;
      case 0x12:
        ram[0x111] = r[8];
        break;
      case 0x13:
        if (r[4] === 0x80) ram[0x24] = r[8];
        response[8] = ram[0x24];
        response[9] = this.minimum;
        break;
      case 0x25:
        assert.deepEqual([...r.subarray(4, 8)], [0x52, 0x49, 0x18, 0x22], "unlock magic");
        this.reads = r[8] === 1;
        return;
      case 0x02:
        assert.ok(this.reads, "flash read without command 25");
        for (let i = 0; i < 28; i++) {
          const word = this.flash[address + i] ?? 0;
          response[8 + i * 2] = word & 0xff;
          response[9 + i * 2] = word >> 8;
        }
        break;
      case 0x20:
        this.access = r[7] === 1;
        response[8] = 0;
        break;
      case 0x2f: {
        assert.ok(this.access, "write outside an access window");
        const count = (r[2] - 1) / 8 + 1;
        const end = address + count;
        assert.ok(
          (address >= 0x2400 && end <= 0x2500) || (address >= 0x2600 && end <= 0x3d00),
          `write at ${address.toString(16)}`,
        );
        // Mode 80 erases the page when the address starts one.
        if (r[5] & 0x80 && address % 0x100 === 0) this.flash.fill(0xffff, address, address + 0x100);
        for (let i = 0; i < count; i++) {
          this.flash[address + i] &= r[8 + i * 2] | (r[9 + i * 2] << 8);
        }
        this.writes.push(address);
        return;
      }
      default:
        throw new Error(`unexpected command ${r[1].toString(16)}`);
    }
    this.response = response;
  }
}

const runtime = (template) => ({
  payload: (block) => wasm.runtimePayload(block),
  validate: (payload) => wasm.checkRuntimePayload(payload),
  lift: (block) => wasm.liftHeight(block) ?? null,
  template: () => template,
});

const outside = (flash) => [
  ...flash.subarray(0, 0x2400),
  ...flash.subarray(0x2500, 0x2600),
  ...flash.subarray(0x3d00),
];

test("read, save, restore on a simulated mouse", { skip: !image && "set A90_BACKUP" }, async () => {
  await init({ module_or_path: readFileSync(new URL("../dist/pkg/bloody_a90_web_bg.wasm", import.meta.url)) });
  hid.setChecksums((words) => wasm.checksums(words));
  const original = new wasm.Editor(image);
  const info = original.info();
  const fake = new FakeHid(image, info.model, info.firmware);
  const template = original.sensitivityTemplate();
  const payload = wasm.runtimePayload(template);
  [0x26, 0x27, 0x28, 0x29, 0x2a, 0x23, 0x2d, 0x2e].forEach((a, i) => (fake.ram[a] = payload[i]));
  fake.ram[0x157] = 1;
  fake.ram[0x264] = original.profileCount() - 1;
  fake.ram[0x25] = 1;
  const untouched = outside(fake.flash);

  const device = await hid.Device.open(fake);
  const snapshot = await device.snapshot();
  assert.deepEqual(snapshot.bytes, image);
  assert.equal(snapshot.identity.firmware, info.firmware);

  // DPI: two presets, the second selected, applied at runtime.
  const editor = new wasm.Editor(snapshot.bytes);
  const dpi = editor.sensitivityBlock([[800, 800], [1600, 1600]], 1);
  await device.saveSensitivity(dpi, runtime(dpi));
  assert.deepEqual([...fake.flash.subarray(0x2400, 0x2480)], [...dpi]);
  assert.deepEqual([...(await device.runtimeSensitivity())], [...wasm.runtimePayload(dpi)]);

  // Report rate keeps the running preset.
  editor.reload(new Uint8Array(fake.flash.slice().buffer), false);
  const rate = editor.rateBlock(500);
  const before = await device.runtimeSensitivity();
  await device.saveRate(rate, runtime(rate));
  const after = await device.runtimeSensitivity();
  assert.equal(after[5], 2);
  assert.deepEqual([...after.subarray(0, 5)], [...before.subarray(0, 5)]);

  // Lift height and calibration.
  editor.reload(new Uint8Array(fake.flash.slice().buffer), false);
  const lift = editor.liftBlock(10);
  await device.saveLift(lift, runtime(lift));
  assert.equal(fake.ram[0x24], 0xc0 | 6);
  assert.equal(await device.calibrateLift(1), 9);
  assert.equal(fake.ram[0x111] & 0x80, 0);

  // A keyboard assignment on the rear side key, then the light effect.
  editor.reload(new Uint8Array(fake.flash.slice().buffer), false);
  editor.assign(0, 3, "keyboard", 0, 0x01);
  const keys = editor.buttonsBlock();
  await device.saveButtons(keys);
  const saved = new wasm.Editor(new Uint8Array(fake.flash.slice().buffer));
  assert.match(saved.records(0)[3].caption, /^键盘：Ctrl\+/);
  const lighting = (() => {
    saved.setLightEffect(3);
    return saved.buttonsBlock();
  })();
  await device.saveLighting(lighting);
  assert.equal(new wasm.Editor(new Uint8Array(fake.flash.slice().buffer)).lightEffect(), 3);

  // A shooting slot with a trigger in all profiles.
  editor.reload(new Uint8Array(fake.flash.slice().buffer), false);
  const shot = { kind: "b", shots: [{ intervalMs: 100, x: 0, y: 5 }], loopStart: 1, recover: false, special: false };
  editor.setShooting(2, shot, 8, undefined);
  await device.saveButtons(editor.buttonsBlock());

  // Restoring the original image writes back only the pages that differ.
  const pages = await device.restoreBackup(image, runtime(template), original.debounce());
  assert.ok(pages.includes(0x2400) && pages.includes(0x2600));
  assert.deepEqual(new Uint8Array(fake.flash.slice().buffer), image);
  assert.deepEqual(outside(fake.flash), untouched);
  assert.deepEqual([...(await device.runtimeSensitivity())], [...payload]);
  assert.ok(fake.writes.every((a) => (a >= 0x2400 && a < 0x2500) || (a >= 0x2600 && a < 0x3d00)));
});

test("save refuses a damaged block before any write", { skip: !image && "set A90_BACKUP" }, async () => {
  await init({ module_or_path: readFileSync(new URL("../dist/pkg/bloody_a90_web_bg.wasm", import.meta.url)) });
  hid.setChecksums((words) => wasm.checksums(words));
  const editor = new wasm.Editor(image);
  const info = editor.info();
  const fake = new FakeHid(image, info.model, info.firmware);
  const device = await hid.Device.open(fake);
  const block = editor.buttonsBlock();
  block[20] ^= 1;
  await assert.rejects(device.saveButtons(block), /校验值/);
  // A running profile index past the new table (at most 7 profiles).
  fake.ram[0x263] = 7;
  await assert.rejects(device.saveButtons(editor.buttonsBlock()), /新按键表只有/);
  assert.equal(fake.writes.length, 0);
});
