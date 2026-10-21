// Browser version of the A90 configuration tool. The layout, widgets and
// captions follow the desktop tool (src/app, src/skin.rs). The wasm module
// (web/src) parses flash images and builds blocks; hid.js talks to the mouse.
import init, * as wasm from "./pkg/bloody_a90_web.js";
import * as hid from "./hid.js";

const $ = (id) => document.getElementById(id);
const SVG = "http://www.w3.org/2000/svg";

// Text is written in Simplified Chinese, as in the desktop tool, and shown
// through its translation tables (src/i18n) in the chosen language.
const CJK = /[\u3000-\u9fff\uff00-\uffef]/;
const t = (text) => (typeof text === "string" && CJK.test(text) ? wasm.tr(text) : text);

/// Element builder: h("div", { class: "x", onclick }, ...children).
function h(tag, props, ...children) {
  const el = document.createElement(tag);
  for (const [key, value] of Object.entries(props ?? {})) {
    if (value === undefined || value === null || value === false) continue;
    if (key.startsWith("on")) el.addEventListener(key.slice(2), value);
    else if (key === "class") el.className = value;
    else if (key === "style" && typeof value === "object") {
      for (const [name, v] of Object.entries(value)) {
        if (name.startsWith("--")) el.style.setProperty(name, v);
        else el.style[name] = v;
      }
    }
    else if (key === "dataset") Object.assign(el.dataset, value);
    else if (key === "title" || key === "placeholder") el[key] = t(value);
    else if (key in el && key !== "list") el[key] = value;
    else el.setAttribute(key, value === true ? "" : value);
  }
  for (const child of children.flat(Infinity)) {
    if (child === null || child === undefined || child === false) continue;
    el.append(child instanceof Node ? child : t(String(child)));
  }
  return el;
}

/// SVG from markup; the drawings are fixed, so a template is clearest.
function svg(width, height, body, attrs = "") {
  const template = document.createElement("template");
  template.innerHTML = `<svg xmlns="${SVG}" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}" ${attrs}>${body}</svg>`;
  return template.content.firstChild;
}

const REPOSITORY = "https://github.com/asukaminato0721/bloody_a90";
const hex = (value, width) => value.toString(16).padStart(width, "0");
const message = (error) => (error instanceof Error ? error.message : String(error));
const px = (value) => `${value}px`;

// ---- State ----

const DEFAULT_DPI = [[1000, 1000], [1200, 1200], [1600, 1600], [2000, 2000], [6200, 6200]];
const RATES = [125, 250, 500, 1000];
const WHEEL_DOWN = 24;

const state = {
  editor: null,
  /// "device" or "file".
  source: null,
  fileName: "",
  bytes: null,
  device: null,
  snapshot: null,
  /// The first flash read of this page session, kept before any write.
  sessionBackup: null,
  busy: false,
  status: "",
  failed: false,
  main: "keys",
  page: "keys",
  templateMenu: false,
  profile: 0,
  record: 1,
  popup: null,
  keysDirty: false,
  key: { choice: "mouse", value: 0, modifiers: 0 },
  watching: null,
  held: [],
  hover: null,
  sens: null,
  rateDraft: null,
  macro: { press: "LeftDown\ndelay 25\nLeftUp\ndelay 85\nLeftDown\ndelay 25\nLeftUp", release: "", repeat: 0, file: "" },
  oscar: { category: 0, physical: false, keyDelay: 64, motion: [10, 0], position: [400, 300], delay: 25, goto: 1, count: 3, key: 4, undo: [], redo: [], notes: false },
  shoot: {
    slot: 1,
    sniper: 5,
    trigger: 8,
    allProfiles: true,
    drafts: Array.from({ length: 7 }, () => ({
      kind: "a",
      count: 2,
      loopStart: 1,
      recover: false,
      special: false,
      shots: Array.from({ length: 8 }, () => ({ intervalMs: 100, x: 0, y: 0 })),
    })),
  },
  lightEffect: null,
  debounce: null,
};

// The sensitivity helpers that hid.js needs from the wasm module.
function runtimeFor(template) {
  return {
    payload: (block) => wasm.runtimePayload(block),
    validate: (payload) => wasm.checkRuntimePayload(payload),
    lift: (block) => wasm.liftHeight(block) ?? null,
    template: () => template,
  };
}

// ---- Navigation, from src/app/mod.rs ----

const MAIN_TABS = [
  ["keys", "按键"],
  ["sensitivity", "灵敏度"],
  ["programming", "编程设置"],
  ["lighting", "灯光设置"],
  ["other", "其他设置"],
];

const HOST_ONLY = "网页版不能修改桌面设置，请使用桌面版";
/// Captions of BloodyWorkShop8_ChineseS.ini. A tooltip marks the pages that
/// need the desktop tool.
const SUB_TABS = {
  keys: [],
  sensitivity: [
    ["resolution", "分辨率"],
    ["response", "按键响应"],
    ["rate", "回报率"],
    ["lift", "微精校准"],
    [null, "滚轮", HOST_ONLY],
    [null, "指针", HOST_ONLY],
  ],
  programming: [
    ["shooting", "左键编程"],
    ["sniper", "狙击键"],
    ["oscar", "奥斯卡II"],
    [null, "奥斯卡III", "宏库位于本机目录，请使用桌面版"],
  ],
  lighting: [["brightness", "灯光亮度"]],
  other: [
    ["about", "关于软件"],
    ["hardware", "关于硬件"],
    ["backup", "已有备份"],
    [null, "配置文件", "配置文件含主机动作，请使用桌面版"],
    [null, "应用关联", HOST_ONLY],
    [null, "手势", HOST_ONLY],
  ],
};

const BANNERS = {
  keys: "全键编程模板 : 提供MMORPG等游戏所需的多按键自定义编程功能，若FPS/TPS射击类游戏玩家可选用更专业的左键编程模板进行设置。",
  sensitivity: "灵敏度设置 : 提供鼠标的分辨率CPI，按键响应，报告率，微精校准LOD，滚轮及指针等灵敏度的功能设置。",
  programming: "编程设置 : 提供多种编程平台的“组合键”“左键编程”“奥斯卡II”“狙击键”等编程组合设置，并在按键设置中绑定编程按键使用。",
  lighting: "灯光设置：提供鼠标灯光亮度，灯效控制及RGB编辑等设置。（因产品功能不同，灯光设置会有所差异)",
  other: "其他设置 : 提供鼠标软件与硬件等设备设置。",
};

const DESCRIPTIONS = {
  keys: "按键设置：选择记录并指定动作，点击“应用”写入鼠标。按键查询可找出物理按键对应的记录。",
  resolution: "分辨率：设置各档 X / Y 轴 CPI。点击“应用”保存并立即生效。",
  oscar: "奥斯卡宏编辑器：编辑 M 型文本，导入或导出 AMC 文件。在“按键”页把宏指定给按键。",
  backup: "读取已有的 A90 固件 07 或 0c 备份，查看灵敏度、按键表和宏程序。",
};

function openMain(main) {
  state.main = main;
  state.page = main === "keys" ? "keys" : SUB_TABS[main].find(([page]) => page)[0];
  state.popup = null;
  state.templateMenu = false;
}

// ---- Status, progress and persistence ----

function report(text, failed = false) {
  state.status = text;
  state.failed = failed;
  const el = $("status");
  el.textContent = t(text);
  el.title = t(text);
  el.className = `status${failed ? " error" : ""}${state.busy ? " busy" : ""}`;
}

function progress(done, total, label) {
  const box = $("progress");
  if (done === null) {
    box.hidden = true;
    return;
  }
  box.hidden = false;
  $("progress-bar").style.width = `${(100 * done) / total}%`;
  $("progress-text").textContent = `${t(label)} ${Math.round((100 * done) / total)}%`;
}

function download(bytes, name, type = "application/octet-stream") {
  const url = URL.createObjectURL(new Blob([bytes], { type }));
  const link = h("a", { href: url, download: name });
  document.body.append(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

const timestamp = () => new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);

/// Keep the last five session backups in this browser, as the desktop tool
/// keeps one under ~/.cache. Storage can be unavailable; downloads work.
function rememberBackup(bytes) {
  try {
    const list = JSON.parse(localStorage.getItem("a90-backups") ?? "[]");
    let binary = "";
    for (const byte of bytes) binary += String.fromCharCode(byte);
    list.unshift({ time: new Date().toISOString(), data: btoa(binary) });
    localStorage.setItem("a90-backups", JSON.stringify(list.slice(0, 5)));
    return true;
  } catch {
    return false;
  }
}

function storedBackups() {
  try {
    return JSON.parse(localStorage.getItem("a90-backups") ?? "[]");
  } catch {
    return [];
  }
}

function decodeStored(entry) {
  return Uint8Array.from(atob(entry.data), (c) => c.charCodeAt(0));
}

function wordsToBytes(words) {
  const bytes = new Uint8Array(words.length * 2);
  const view = new DataView(bytes.buffer);
  words.forEach((word, index) => view.setUint16(index * 2, word, true));
  return bytes;
}

function byteHex(words) {
  return [...wordsToBytes(words)].map((byte) => hex(byte, 2)).join(" ");
}

// ---- Loading images ----

function loadSensitivity() {
  const sens = state.editor.sensitivity();
  state.sens = {
    presets: sens.presets.map((p) => [...p]),
    enabled: [0, 1, 2, 3, 4].map((i) => i < sens.count),
    selected: sens.selected,
    separate: sens.presets.slice(0, sens.count).some(([x, y]) => x !== y),
    rate: sens.rate_hz ?? null,
    liftEnabled: sens.lift !== undefined && sens.lift !== null,
    lift: sens.lift ?? 16,
  };
}

function loadKeys() {
  try {
    state.profile = Math.min(state.profile, state.editor.profileCount() - 1);
    state.debounce = state.editor.debounce() ?? null;
    state.lightEffect = state.editor.lightEffect() ?? null;
  } catch {
    state.debounce = null;
    state.lightEffect = null;
  }
  state.keysDirty = false;
  selectRecord(state.record);
}

function openImage(bytes, source, name = "") {
  const editor = new wasm.Editor(bytes);
  state.editor?.free();
  state.editor = editor;
  state.source = source;
  state.fileName = name;
  state.bytes = bytes;
  state.rateDraft = null;
  loadSensitivity();
  loadKeys();
  compileMacro();
}

async function readDevice() {
  const snapshot = await state.device.snapshot((done, total) => progress(done, total, "正在读取闪存"));
  progress(null);
  state.snapshot = snapshot;
  return snapshot.bytes;
}

async function connect(hidDevice) {
  if (state.busy) return;
  state.busy = true;
  report("正在连接…");
  render();
  try {
    const previous = state.device;
    const device = await hid.Device.open(hidDevice);
    if (previous && previous.hid !== hidDevice) await previous.close().catch(() => {});
    state.device = device;
    const bytes = await readDevice();
    openImage(bytes, "device");
    let note = "";
    if (!state.sessionBackup) {
      state.sessionBackup = bytes;
      note = rememberBackup(bytes)
        ? " · 本次备份已保存在浏览器中，可在 其他设置 → 已有备份 下载"
        : " · 浏览器存储不可用，请在 其他设置 → 已有备份 下载本次备份";
    }
    state.busy = false;
    report(`已连接并读取闪存${note}`);
  } catch (error) {
    progress(null);
    state.busy = false;
    report(`连接失败：${message(error)}`, true);
  }
  render();
}

/// Re-read the flash after a write. `keepKeys` keeps an unsaved key draft.
async function refresh(keepKeys) {
  const bytes = await readDevice();
  state.bytes = bytes;
  const keep = keepKeys && state.keysDirty;
  state.editor.reload(bytes, keep);
  loadSensitivity();
  if (!keep) loadKeys();
}

/// Run one device job, then re-read the flash, as the desktop worker does.
async function run(job, done, { refreshAfter = true, keepKeys = true } = {}) {
  if (state.busy) return;
  if (!state.device) return report("鼠标未连接", true);
  if (!state.sessionBackup) return report("尚未读取本次会话的闪存备份，不能写入。", true);
  state.busy = true;
  state.popup = null;
  report("正在写入…");
  render();
  try {
    const result = await job(state.device);
    if (refreshAfter) await refresh(keepKeys);
    state.busy = false;
    report(typeof done === "function" ? done(result) : done);
  } catch (error) {
    progress(null);
    state.busy = false;
    report(message(error), true);
  }
  render();
}

const connected = () => !!state.device && !!state.sessionBackup && !state.busy;

// ---- Skin widgets (src/skin.rs) ----

function skinButton(label, width, enabled, onclick, title) {
  return h("button", { class: "skin-button", style: { width: px(width) }, disabled: !enabled, onclick, title }, label);
}

function tool(label, active, onclick, title) {
  return h("button", { class: `tool${active ? " active" : ""}`, onclick, title }, label);
}

function check(label, checked, onchange, disabled = false) {
  return h(
    "label",
    { class: `check${disabled ? " disabled" : ""}` },
    h("input", { type: "checkbox", checked, disabled, onchange: (e) => onchange(e.target.checked) }),
    label,
  );
}

function option(label, active, onclick) {
  return h("span", { class: `option${active ? " active" : ""}`, onclick }, label);
}

function info(help) {
  return h("span", { class: "info", title: help }, "i");
}

function step(number, title) {
  return h("div", { class: "step" }, `${number} ${t(title)}`);
}

function valueBox(text, selected, [width, height], onclick, title) {
  return h(
    onclick ? "button" : "span",
    { class: `value${selected ? " selected" : ""}`, style: { width: px(width), height: px(height) }, onclick, title },
    text,
  );
}

function select(options, selected, onchange, width, disabled = false) {
  return h(
    "select",
    { style: { width: px(width) }, disabled, onchange: (e) => onchange(Number(e.target.value)) },
    options.map((label, index) => h("option", { value: index, selected: index === selected }, label)),
  );
}

function number(value, min, max, onchange, width = 64, disabled = false) {
  return h("input", {
    type: "number",
    value,
    min,
    max,
    disabled,
    style: { width: px(width) },
    onchange: (e) => {
      const v = Math.round(Number(e.target.value));
      onchange(Number.isFinite(v) ? Math.min(max, Math.max(min, v)) : min);
    },
  });
}

/// TrackBar of the sensitivity pages. `oninput` gets snapped values while
/// dragging; the page renders again when the drag ends.
function trackbar({ value, min, max, step = 1, width, disabled = false, oninput }) {
  const fraction = (v) => (v - min) / Math.max(1, max - min);
  const fill = h("div", { class: "fill" });
  const thumb = h("div", { class: "thumb" });
  const bar = h("div", { class: `trackbar${disabled ? " disabled" : ""}`, style: { width: px(width) } }, h("div", { class: "groove" }), fill, thumb);
  const place = (v) => {
    const x = 5 + fraction(v) * (width - 10);
    thumb.style.left = px(x);
    fill.style.width = px(Math.max(0, x - 1));
  };
  place(value);
  if (!disabled) {
    let current = value;
    const at = (event) => {
      const rect = bar.getBoundingClientRect();
      const f = Math.min(1, Math.max(0, (event.clientX - rect.left - (5 * rect.width) / width) / ((rect.width * (width - 10)) / width)));
      const v = Math.min(max, Math.max(min, Math.round((f * (max - min)) / step) * step + min));
      if (v !== current) {
        current = v;
        place(v);
        oninput(v);
      }
    };
    bar.addEventListener("pointerdown", (event) => {
      bar.setPointerCapture(event.pointerId);
      at(event);
    });
    bar.addEventListener("pointermove", (event) => {
      if (bar.hasPointerCapture(event.pointerId)) at(event);
    });
    bar.addEventListener("pointerup", () => render());
  }
  return bar;
}

function ticks(width, marks) {
  return h(
    "div",
    { class: "ticks", style: { width: px(width) } },
    marks.map(([f, text]) => h("span", { style: { left: px(5 + f * (width - 10)) } }, text)),
  );
}

const RED = "rgb(220,20,30)";
const ICONS = {
  mouseX: `<rect x="23.25" y="18.25" width="19.5" height="29.5" rx="9.75" fill="none" stroke="${RED}" stroke-width="2.5"/><path d="M33 17v9" stroke="${RED}" stroke-width="2.5"/><path d="M13 29l-4 4 4 4M53 29l4 4-4 4" fill="none" stroke="${RED}" stroke-width="2"/>`,
  mouseY: `<rect x="23.25" y="18.25" width="19.5" height="29.5" rx="9.75" fill="none" stroke="${RED}" stroke-width="2.5"/><path d="M33 17v9" stroke="${RED}" stroke-width="2.5"/><path d="M29 11l4-4 4 4M29 55l4 4 4-4" fill="none" stroke="${RED}" stroke-width="2"/>`,
  speed: `<path d="M13 41a20 20 0 0 1 40 0" fill="none" stroke="${RED}" stroke-width="2.5"/><path d="M33 41l6-18" stroke="${RED}" stroke-width="3.5"/><circle cx="33" cy="41" r="3.5" fill="${RED}"/>`,
  rate: `<circle cx="33" cy="33" r="18.75" fill="none" stroke="${RED}" stroke-width="2.5"/><circle cx="33" cy="33" r="9" fill="none" stroke="${RED}" stroke-width="2"/><path d="M9 33h48M33 9v48" stroke="${RED}" stroke-width="1.5"/>`,
  light: `<circle cx="33" cy="31" r="11" fill="none" stroke="${RED}" stroke-width="2.5"/><text x="33" y="36" text-anchor="middle" font-size="14" font-weight="700" fill="${RED}">A</text><path d="M48 31h6M18 31h-6M22.4 20.4l-4.2-4.2M43.6 20.4l4.2-4.2M33 16v-6M27 46h12M29 51h8" stroke="${RED}" stroke-width="2"/>`,
  language: `<rect x="14" y="16" width="22" height="20" rx="3" fill="none" stroke="${RED}" stroke-width="2"/><text x="25" y="31" text-anchor="middle" font-size="13" font-weight="700" fill="${RED}">A</text><rect x="30" y="30" width="22" height="20" rx="3" fill="#460e12" stroke="${RED}" stroke-width="2"/><text x="41" y="45" text-anchor="middle" font-size="13" font-weight="700" fill="${RED}">&#x4e2d;</text>`,
  card: `<rect x="13" y="19" width="40" height="28" rx="3" fill="${RED}"/><circle cx="23" cy="29" r="5" fill="#460e12"/><rect x="16" y="36" width="14" height="8" rx="3" fill="#460e12"/><path d="M36 27h12M36 33h12M36 39h12" stroke="#460e12" stroke-width="2"/>`,
};

function icon(name) {
  return h("div", { class: "icon" }, svg(66, 66, ICONS[name]));
}

/// A group panel: icon tile, title and info badge, then the body.
function card({ icon: name, title, help, style }, ...body) {
  return h(
    "div",
    { class: "card", style },
    info(help),
    h("div", { class: "card-title" }, name ? icon(name) : null, h("span", null, title)),
    body,
  );
}

function needsImage() {
  return h("p", { class: "gray", style: { margin: "40px 0 0 20px" } }, "请连接鼠标，或在 其他设置 → 已有备份 中读取文件。");
}

// ---- 按键 (src/app/keys.rs) ----

/// Rows of the KeySet sheet. The left button is reached from the diagram.
const ROWS = [
  ["R", 1],
  ["M", 2],
  ["UP", 25],
  ["DN", 24],
  ["→", 4],
  ["←", 3],
  ["1", 8],
  ["N", 9],
  ["3", 10],
];

/// Caption, record, caption box and leader dot, relative to the drawing.
const DIAGRAM = [
  ["L", 0, [38, 103], [214, 118]],
  ["→", 4, [38, 179], [153, 193]],
  ["←", 3, [38, 247], [153, 261]],
  ["R", 1, [486, 79], [347, 94]],
  ["M", 2, [486, 120], [285, 134]],
  ["1", 8, [486, 183], [274, 198]],
  ["N", 9, [486, 219], [274, 230]],
  ["3", 10, [486, 255], [274, 262]],
];

/// Top view of the mouse (skin::mouse_top), 560 × 520.
function mouseTop() {
  const line = `fill="none" stroke="#828286" stroke-width="1.5"`;
  const leaders = DIAGRAM.map(([, , [x, y], [dx]]) => {
    const edgeX = x < 100 ? x + 38 : x;
    const edgeY = y + 15;
    return `<path d="M${edgeX} ${edgeY}H${dx}" ${line} stroke-width="1"/><circle cx="${dx}" cy="${edgeY}" r="5" ${line} stroke-width="1"/>`;
  }).join("");
  const wheel = Array.from({ length: 9 }, (_, i) => {
    const y = 100 + i * 6;
    return `<path d="M265 ${y}L272 ${y + 3}L279 ${y}" fill="none" stroke="#96969a"/>`;
  }).join("");
  return svg(
    560,
    520,
    `<path d="M272 48C230 50 190 58 175 80C150 160 155 200 140 250C115 330 120 400 180 450C220 480 250 500 272 508C294 500 324 480 364 450C424 400 429 330 404 250C389 200 394 160 369 80C354 58 314 50 272 48Z" fill="#2c2d31" stroke="#828286" stroke-width="1.5"/>
    <path d="M268 14V50M276 14V50" stroke="#6e6e72" stroke-width="2"/>
    <path d="M255 50C250 120 252 200 250 270M289 50C294 120 292 200 294 270M175 80C190 140 215 230 250 270M369 80C354 140 329 230 294 270M250 270C255 310 265 320 272 330M294 270C289 310 279 320 272 330M160 420C210 440 240 455 272 465M384 420C334 440 304 455 272 465" ${line}/>
    <rect x="258.75" y="88.75" width="26.5" height="72.5" rx="13" fill="#46474b" stroke="#828286" stroke-width="1.5"/>${wheel}
    <path d="M256 182L272 194L288 182M256 214L272 226L288 214M256 246L272 258L288 246" ${line}/>
    <rect x="147.75" y="178.75" width="8.5" height="28.5" rx="4" ${line}/><rect x="147.75" y="246.75" width="8.5" height="28.5" rx="4" ${line}/>
    <text x="272" y="400" text-anchor="middle" font-size="15" font-weight="700" fill="#6e6e72">bloody</text>${leaders}`,
  );
}

function selectRecord(record) {
  state.record = record;
  if (!state.editor) return;
  try {
    const view = state.editor.records(state.profile)[record];
    state.key = { choice: view.choice, value: view.value, modifiers: view.modifiers };
  } catch {
    // The page shows the conversion error.
  }
}

/// Where the editor opens: below the record's row. The left button and
/// the records without a row open below the first row.
function popupAt(record) {
  const row = Math.max(0, ROWS.findIndex(([, r]) => r === record));
  return [33 + 64, 18 + 56 + row * 32 + 25];
}

function openPopup(record) {
  if (state.popup && state.popup.record === record) {
    state.popup = null;
  } else {
    selectRecord(record);
    state.popup = { record, at: popupAt(record) };
  }
  render();
}

function keysPage() {
  const editor = state.editor;
  let records = null;
  let error = null;
  if (editor) {
    try {
      records = editor.records(state.profile);
    } catch (e) {
      error = message(e);
    }
  }
  const held = (record) => state.held.includes(record + 1);
  const sheet = h(
    "div",
    { class: "sheet" },
    h("div", { class: "sheet-header" }, "按键设置"),
    records
      ? ROWS.map(([label, record], row) =>
          h(
            "div",
            {
              class: "sheet-row",
              style: { top: px(56 + row * 32) },
              onmouseenter: () => setHover(record),
              onmouseleave: () => setHover(null),
            },
            h("span", { class: `caption${held(record) ? " held" : ""}` }, label),
            h(
              "button",
              { class: `combo${state.popup?.record === record ? " open" : ""}`, title: records[record].caption, onclick: () => openPopup(record) },
              records[record].caption,
            ),
          ),
        )
      : h(
          "div",
          { class: error ? "error" : "gray", style: { position: "absolute", left: "34px", top: "60px", width: "470px" } },
          error ?? "请连接鼠标，或在 其他设置 → 已有备份 中读取文件。",
        ),
  );
  const diagram = h(
    "div",
    { class: "diagram" },
    mouseTop(),
    DIAGRAM.map(([caption, record, [x, y]]) =>
      h(
        "button",
        {
          class: `key-box${state.hover === record || state.popup?.record === record || held(record) ? " lit" : ""}`,
          dataset: { record },
          style: { left: px(x), top: px(y) },
          onmouseenter: () => setHover(record),
          onmouseleave: () => setHover(null),
          onclick: () => records && openPopup(record),
        },
        caption,
      ),
    ),
  );
  const keyset = h("div", { class: "keyset" }, sheet, diagram);
  if (state.popup && records) {
    const [left, top] = state.popup.at;
    keyset.append(h("div", { class: "key-popup", style: { left: px(left), top: px(top) } }, keyEditor(records[state.record])));
  }
  const others = records
    ? h(
        "details",
        { open: state.record < 24 && !ROWS.some(([, r]) => r === state.record) && state.record !== 0 },
        h("summary", null, "其他记录（A90 没有对应的物理按键）"),
        h(
          "div",
          { class: "record-chips" },
          records
            .filter((view) => !view.physical)
            .map((view) =>
              h(
                "button",
                {
                  class: `chip${state.record === view.record ? " active" : ""}`,
                  title: view.caption,
                  onclick: () => {
                    selectRecord(view.record);
                    state.popup = { record: view.record, at: [97, 520] };
                    render();
                  },
                },
                view.name,
              ),
            ),
        ),
      )
    : null;
  return {
    body: [keyset, others],
    footer: [
      [
        "恢复默认",
        !!records,
        () => {
          try {
            editor.resetKeys(true);
            state.keysDirty = true;
            loadKeysDraftOnly();
          } catch (e) {
            report(message(e), true);
          }
          render();
        },
      ],
      [
        "取消",
        state.keysDirty,
        () => {
          editor.resetKeys(false);
          loadKeys();
          render();
        },
      ],
      [
        "应用",
        connected() && state.keysDirty && !!records,
        () => {
          let block;
          try {
            block = editor.buttonsBlock();
          } catch (e) {
            return report(message(e), true);
          }
          run((device) => device.saveButtons(block), "按键设置已保存，设备读回一致。", { keepKeys: false });
        },
      ],
    ],
  };
}

/// After a draft change that did not come from the saved table.
function loadKeysDraftOnly() {
  try {
    state.debounce = state.editor.debounce() ?? null;
  } catch {
    state.debounce = null;
  }
  selectRecord(state.record);
}

function setHover(record) {
  if (state.hover === record) return;
  state.hover = record;
  for (const box of document.querySelectorAll(".key-box")) {
    const r = Number(box.dataset.record);
    box.classList.toggle("lit", r === record || state.popup?.record === r || state.held.includes(r + 1));
  }
}

function keyEditor(view) {
  const editor = state.editor;
  const key = state.key;
  const wheel = view.record >= 24;
  const options = wasm.choiceOptions(key.choice, view.record);
  const gray = (text, size) => h("div", { class: "gray", style: size ? { fontSize: px(size) } : null }, text);
  const parts = [
    h("h2", null, view.name),
    gray(`当前：${t(view.press)} / ${t(view.release)}`),
    h(
      "div",
      { class: "wrap", style: { marginTop: "6px" } },
      wasm.choices().map(({ code, label }) =>
        tool(label, key.choice === code, () => {
          if (key.choice !== code) {
            key.choice = code;
            key.value = 0;
            render();
          }
        }),
      ),
    ),
  ];
  if (options.length) {
    parts.push(h("div", null, select(options, Math.min(key.value, options.length - 1), (v) => (key.value = v), 260)));
  }
  if (key.choice === "keyboard") {
    for (const [start, labels] of [
      [0, ["左 Ctrl", "左 Shift", "左 Alt", "左 Super"]],
      [4, ["右 Ctrl", "右 Shift", "右 Alt", "右 Super"]],
    ]) {
      parts.push(
        h(
          "div",
          { class: "row", style: { gap: "14px" } },
          labels.map((label, index) => {
            const mask = 1 << (start + index);
            return check(label, (key.modifiers & mask) !== 0, (on) => (key.modifiers = on ? key.modifiers | mask : key.modifiers & ~mask));
          }),
        ),
      );
    }
  }
  if (key.choice === "media" || key.choice === "system") {
    parts.push(h("div", null, "动作由桌面处理。实际效果取决于桌面快捷键和默认应用设置。"));
  }
  if (key.choice === "macro") {
    const macro = state.macro;
    parts.push(
      gray(
        macro.error
          ? `奥斯卡II中的程序无法编译：${macro.error}`
          : wheel
            ? "使用奥斯卡II中的单次滚轮程序"
            : `使用奥斯卡II中的程序：按下 ${macro.program.press.length} 字，弹起 ${macro.program.release.length} 字`,
      ),
    );
    if (wheel) parts.push(h("div", null, "每格滚动执行一次按下程序，再执行释放程序。滚轮不使用按住或开关重复。"));
  }
  if (key.choice === "shoot") {
    const absent = editor.info().shooting_route === "absent";
    parts.push(
      gray(absent ? "此固件没有左键射击入口：射击模式只切换滚轮灯颜色（模式 1 红、2 绿）。" : "射击模式索引由固件解释，效果取决于射击设置。", 13),
    );
  }
  const assign = () => {
    if (view.record === 0 && !(key.choice === "mouse" && key.value === 0)) {
      if (!confirm(t("左键将不再执行鼠标左键点击。确定要修改左键吗？"))) return;
    }
    try {
      editor.assign(state.profile, view.record, key.choice, key.value, key.modifiers);
      state.keysDirty = true;
      state.popup = null;
      selectRecord(view.record);
    } catch (e) {
      report(message(e), true);
    }
    render();
  };
  parts.push(
    h(
      "div",
      { class: "row", style: { gap: "10px", marginTop: "6px" } },
      skinButton("设定此按键", 130, true, assign),
      skinButton(state.watching ? "停止查询" : "按键查询", 130, !!state.device && (!state.busy || !!state.watching), toggleWatch, "按住鼠标上的按键，自动选中对应记录"),
    ),
  );
  if (wheel) {
    parts.push(
      skinButton("恢复此方向滚动", 160, true, () => {
        editor.restoreWheel(state.profile, view.record);
        state.keysDirty = true;
        selectRecord(view.record);
        render();
      }),
    );
  }
  if (state.watching) {
    parts.push(h("div", { class: "red" }, state.held.length ? `按住：[${state.held.join(", ")}]` : "请按下鼠标按键"));
  }
  parts.push(gray("修改只影响当前模式。点击“应用”后写入鼠标并读回核对。", 13));
  return parts;
}

function toggleWatch() {
  if (state.watching) {
    clearInterval(state.watching);
    state.watching = null;
    state.held = [];
    render();
    return;
  }
  state.watching = setInterval(async () => {
    if (state.busy || !state.device) return;
    try {
      const held = await state.device.heldButtons();
      const changed = held.join() !== state.held.join();
      state.held = held;
      if (held.length) {
        const record = Math.min(held[0] - 1, 23);
        if (record !== state.record) {
          selectRecord(record);
          if (state.popup) state.popup = { record, at: popupAt(record) };
        }
      }
      if (changed) render();
    } catch (error) {
      clearInterval(state.watching);
      state.watching = null;
      report(message(error), true);
      render();
    }
  }, 150);
  render();
}

function profileStrip() {
  const strip = $("profile-strip");
  if (state.page !== "keys") {
    strip.className = "profile-strip";
    strip.replaceChildren();
    return;
  }
  strip.className = "profile-strip tabs";
  let count = 0;
  try {
    count = state.editor?.profileCount() ?? 0;
  } catch {
    count = 0;
  }
  strip.replaceChildren(
    ...Array.from({ length: Math.max(count, 3) }, (_, index) =>
      h(
        "button",
        {
          class: `profile-tab${state.profile === index ? " active" : ""}`,
          disabled: index >= count,
          title: state.snapshot?.profile[0] === index ? "鼠标正在使用此模式" : null,
          onclick: () => {
            state.profile = index;
            state.popup = null;
            selectRecord(state.record);
            render();
          },
        },
        `模式${index + 1}`,
      ),
    ),
  );
}

// ---- 灵敏度 (src/app/sensitivity.rs, src/app/host.rs) ----

function activePresets() {
  const sens = state.sens;
  const presets = sens.presets.filter((_, i) => sens.enabled[i]);
  const selected = sens.enabled.slice(0, sens.selected).filter(Boolean).length;
  return [presets, selected];
}

// At least one preset stays enabled. The current preset must be enabled.
function togglePreset(index, enabled) {
  const sens = state.sens;
  if (!enabled && sens.enabled.filter(Boolean).length === 1) return;
  sens.enabled[index] = enabled;
  if (!sens.enabled[sens.selected]) sens.selected = sens.enabled.indexOf(true);
}

function axisSheet(axis) {
  const sens = state.sens;
  const bar = 534;
  return h(
    "div",
    { class: "axis-sheet" },
    h(
      "div",
      { class: "row", style: { gap: "18px", marginBottom: "14px", height: "70px" } },
      icon(axis === 0 ? "mouseX" : "mouseY"),
      sens.separate ? h("span", { style: { fontSize: "16px" } }, axis === 0 ? "X轴 设定 :" : "Y轴 设定 :") : null,
      axis === 0
        ? h(
            "span",
            { style: { marginLeft: sens.separate ? "auto" : "353px", marginRight: sens.separate ? "150px" : "0" } },
            check("启用X/Y", sens.separate, (on) => {
              sens.separate = on;
              if (!on) for (const p of sens.presets) p[1] = p[0];
              render();
            }),
          )
        : null,
    ),
    sens.presets.map((preset, index) => {
      const active = sens.enabled[index];
      const valueEl = valueBox(preset[axis], sens.selected === index, [76, 30], active ? () => ((sens.selected = index), render()) : null, "点击设为当前档位");
      return h(
        "div",
        { class: "preset" },
        check("", active, (on) => {
          togglePreset(index, on);
          render();
        }),
        h(
          "div",
          { style: { opacity: active ? 1 : 0.5 } },
          trackbar({
            value: preset[axis],
            min: 100,
            max: 6200,
            step: 100,
            width: bar,
            disabled: !active,
            oninput: (v) => {
              preset[axis] = v;
              if (!sens.separate) preset[1] = v;
              valueEl.textContent = v;
            },
          }),
          h("div", { class: "range-labels", style: { width: px(bar) } }, h("span", null, "100"), h("span", null, "6200")),
        ),
        h("span", { style: { width: "12px" } }),
        valueEl,
      );
    }),
  );
}

function resolutionPage() {
  if (!state.sens) return { body: needsImage(), footer: [] };
  const sens = state.sens;
  const current = sens.presets[sens.selected];
  let block = null;
  let blockError = null;
  try {
    const [presets, selected] = activePresets();
    block = state.editor.sensitivityBlock(presets, selected);
  } catch (error) {
    blockError = message(error);
  }
  return {
    body: [
      h(
        "div",
        { class: "row", style: { gap: "15px", marginLeft: "95px", marginBottom: "14px" } },
        h("span", { style: { fontSize: "16px", marginRight: "20px" } }, "目前 CPI :"),
        valueBox(current[0], true, [100, 24]),
        valueBox(current[1], true, [100, 24]),
      ),
      h("div", { class: "row", style: { gap: "16px", alignItems: "flex-start" } }, axisSheet(0), sens.separate ? axisSheet(1) : null),
      h(
        "details",
        { style: { marginTop: "8px" } },
        h("summary", null, "查看编码参数"),
        h(
          "table",
          { class: "mono", style: { borderSpacing: "24px 4px" } },
          h("tr", null, ["档位", "X 比例", "Y 比例", "传感器编码"].map((t) => h("th", { style: { textAlign: "left" } }, t))),
          sens.presets.map(([x, y], index) => {
            if (!sens.enabled[index]) return null;
            try {
              const [xs, ys, code] = wasm.encodeDpi(x, y);
              return h("tr", null, h("td", null, index + 1), h("td", null, xs), h("td", null, ys), h("td", null, hex(code, 4)));
            } catch (error) {
              return h("tr", null, h("td", null, index + 1), h("td", { class: "error", colSpan: 3 }, message(error)));
            }
          }),
        ),
      ),
      blockError ? h("div", { class: "gray small" }, blockError) : null,
    ],
    footer: [
      [
        "恢复默认",
        true,
        () => {
          sens.presets = DEFAULT_DPI.map((p) => [...p]);
          sens.enabled = [true, true, true, true, true];
          sens.selected = 0;
          sens.separate = false;
          render();
        },
      ],
      ["取消", true, () => (loadSensitivity(), render())],
      [
        "应用",
        connected() && !!block,
        () => run((device) => device.saveSensitivity(block, runtimeFor(block)), "灵敏度已保存并立即应用。运行参数已读回一致。"),
      ],
    ],
  };
}

function responsePage() {
  const editorReady = state.editor && state.debounce !== undefined;
  let body;
  if (!state.editor) {
    body = h("div", { class: "gray" }, "请先连接鼠标或读取备份。");
  } else {
    const width = 1176 - 42 - 160;
    const ms = state.debounce ?? 0;
    const valueEl = valueBox(ms, false, [76, 30]);
    body = [
      h(
        "div",
        { class: "row", style: { marginTop: "10px", paddingLeft: "42px", alignItems: "flex-start" } },
        h(
          "div",
          null,
          trackbar({
            value: ms,
            min: 0,
            max: 16,
            width,
            oninput: (v) => {
              try {
                state.editor.setDebounce(v);
                state.debounce = v;
                state.keysDirty = true;
                valueEl.textContent = v;
              } catch (error) {
                report(message(error), true);
              }
            },
          }),
          ticks(width, Array.from({ length: 17 }, (_, i) => [i / 16, String(i)])),
        ),
        h("div", { style: { marginLeft: "42px", marginTop: "-6px" } }, valueEl),
      ),
      state.debounce === null ? h("div", { class: "gray" }, "当前编码不在已确认的范围内。选择时间后可替换此设置。") : null,
    ];
  }
  return {
    body: [
      h("div", { style: { height: "66px" } }),
      card(
        {
          icon: "speed",
          title: "按键响应 (ms)",
          help: "按键响应：较长的响应时间可以过滤按键触点的抖动。此设置写入鼠标，并同时保存“按键”页中尚未应用的修改。",
        },
        body,
      ),
      h(
        "p",
        { class: "gray small", style: { width: "1200px", margin: "16px auto" } },
        "双击速度写入桌面设置，网页版不能修改。",
      ),
    ],
    footer: [
      [
        "恢复默认",
        !!editorReady,
        () => {
          state.editor.setDebounce(0);
          state.debounce = 0;
          state.keysDirty = true;
          render();
        },
      ],
      [
        "取消",
        state.keysDirty,
        () => {
          state.editor.resetKeys(false);
          loadKeys();
          render();
        },
      ],
      [
        "应用",
        connected() && state.keysDirty,
        () => {
          let block;
          try {
            block = state.editor.buttonsBlock();
          } catch (error) {
            return report(message(error), true);
          }
          run((device) => device.saveButtons(block), `硬件按键响应时间 ${state.debounce} ms 已保存并读回一致。`, { keepKeys: false });
        },
      ],
    ],
  };
}

function ratePage() {
  const saved = state.sens?.rate ?? null;
  const shown = state.rateDraft ?? saved;
  const notes = [];
  if (state.snapshot) {
    const [code, register] = state.snapshot.rate;
    notes.push(h("div", { class: "gray small" }, `当前运行：${[1, 2, 4, 8].includes(code) ? `${1000 / code} Hz` : `编码 ${hex(code, 2)}`}`));
    if ((register & 0xc0) === 0x80) {
      notes.push(h("div", { class: "red small" }, "硬件切换键的回报率设置仍启用。重新插入后，鼠标会使用该设置。"));
    }
  } else {
    notes.push(h("div", { class: "gray small" }, "请先连接鼠标。"));
  }
  const width = 1176 - 30 - 150;
  const valueEl = valueBox(shown ?? "—", false, [76, 30]);
  const index = Math.max(0, RATES.indexOf(shown ?? 1000));
  return {
    body: [
      h("div", { style: { height: "66px" } }),
      card(
        { icon: "rate", title: "报告率（Hz)", help: "报告率：鼠标每秒向电脑报告位置的次数。点击“应用”保存并立即生效。" },
        h(
          "div",
          { class: "row", style: { marginTop: "10px", paddingLeft: "30px", alignItems: "flex-start" } },
          h(
            "div",
            null,
            trackbar({
              value: index,
              min: 0,
              max: 3,
              width,
              disabled: !state.device,
              oninput: (v) => {
                state.rateDraft = RATES[v];
                valueEl.textContent = RATES[v];
              },
            }),
            ticks(width, [
              [0, "125"],
              [1 / 3, "250"],
              [2 / 3, "500"],
              [1, "1000"],
            ]),
          ),
          h("div", { style: { marginLeft: "40px" } }, valueEl),
        ),
        notes,
      ),
    ],
    footer: [
      ["恢复默认", !!state.device, () => ((state.rateDraft = 1000), render())],
      ["取消", state.rateDraft !== null, () => ((state.rateDraft = null), render())],
      [
        "应用",
        connected() && state.rateDraft !== null && state.rateDraft !== saved,
        () => {
          let block;
          try {
            block = state.editor.rateBlock(state.rateDraft);
          } catch (error) {
            return report(message(error), true);
          }
          state.rateDraft = null;
          run((device) => device.saveRate(block, runtimeFor(block)), "回报率已保存并立即应用。运行参数已读回一致。");
        },
      ],
    ],
  };
}

/// The glowing sensor picture of SensitivitySensorLOD, 164 × 158.
function sensorPicture() {
  const grid = Array.from({ length: 15 }, (_, i) => {
    const t = (i + 1) / 16;
    return `<path d="M${2 + 160 * t} 2V156M2 ${2 + 154 * t}H162" stroke="#3c1214"/>`;
  }).join("");
  return svg(
    164,
    158,
    `<rect width="164" height="158" rx="10" fill="#3c3d41"/><rect x="2" y="2" width="160" height="154" rx="9" fill="#280e10"/>${grid}
    <circle cx="82" cy="79" r="70" fill="rgb(220 20 30 / .12)"/><circle cx="82" cy="79" r="52" fill="rgb(220 20 30 / .2)"/>
    <circle cx="82" cy="79" r="40" fill="rgb(220 20 30 / .31)"/><circle cx="82" cy="79" r="30" fill="rgb(220 20 30 / .55)" stroke="#f03c46" stroke-width="1.5"/>
    <circle cx="82" cy="79" r="10" fill="#eb141e"/>`,
  );
}

/// Vertical LOD bar: 16 rungs, level 16 at the top.
function ladder(level, enabled, onchange) {
  const top = 12;
  const span = 336 - 24;
  const red = enabled ? "#780a10" : "#461e20";
  const rungs = Array.from({ length: 16 }, (_, i) => `<path d="M14 ${top + (span * i) / 15}H122" stroke="${red}" stroke-width="1.5"/>`).join("");
  const draw = (v) =>
    `<rect width="136" height="336" fill="#2c2d31"/>${rungs}<rect x="63" y="${top - 6}" width="10" height="${span + 12}" rx="5" fill="#19191b"/>
    <rect x="63.5" y="${top + (span * (16 - v)) / 15 - 6}" width="9" height="12" rx="1" fill="${enabled ? "#d2171e" : "#5a282a"}"/>`;
  const el = svg(136, 336, draw(level), `style="cursor:${enabled ? "pointer" : "default"};touch-action:none"`);
  if (enabled) {
    let current = level;
    const at = (event) => {
      const rect = el.getBoundingClientRect();
      const f = Math.min(1, Math.max(0, (event.clientY - rect.top - (top * rect.height) / 336) / ((span * rect.height) / 336)));
      const v = 16 - Math.round(f * 15);
      if (v !== current) {
        current = v;
        el.innerHTML = draw(v);
        onchange(v);
      }
    };
    el.addEventListener("pointerdown", (e) => (el.setPointerCapture(e.pointerId), at(e)));
    el.addEventListener("pointermove", (e) => el.hasPointerCapture(e.pointerId) && at(e));
    el.addEventListener("pointerup", () => render());
  }
  return el;
}

/// Side view of a mouse above the surface lines; the lit band marks the
/// chosen lift height.
function sideView(level) {
  const lines = Array.from({ length: 9 }, (_, i) => {
    const lit = level !== null && i === 8 - Math.floor((level - 1) / 2);
    return `<path d="M45 ${196 + i * 20}H420" stroke="${lit ? "#c80a14" : "#46474b"}" stroke-width="${lit ? 2 : 1}"/>`;
  }).join("");
  const o = `fill="none" stroke="#bebec0" stroke-width="1.5"`;
  return svg(
    560,
    370,
    `<rect width="560" height="370" fill="#2c2d31"/><path d="M45 16V360" stroke="#46474b"/>${lines}
    <path d="M80 110C160 20 380 10 490 130C510 160 480 185 420 190H150C110 185 70 140 80 110Z" ${o}/>
    <path d="M140 100C220 70 300 80 360 100M200 125C240 110 300 110 340 125M20 152H80M20 160H78" ${o}/>`,
  );
}

function liftPage() {
  if (!state.sens) return { body: needsImage(), footer: [] };
  const sens = state.sens;
  const valueEl = valueBox(sens.liftEnabled ? sens.lift : "—", false, [60, 24]);
  return {
    body: [
      h("div", { style: { height: "66px" } }),
      card(
        {
          title: "传感器 [微精校准LOD]",
          help: "鼠标抬起后，传感器停止报告位移。等级不是毫米数值。\n校准时在使用表面上持续移动鼠标，保持鼠标接触表面。校准结果先载入此页，再点击“应用”保存。",
          style: { width: "1230px" },
        },
        h(
          "div",
          { class: "row", style: { alignItems: "flex-start", marginTop: "-20px", paddingLeft: "20px" } },
          h(
            "div",
            { style: { paddingTop: "28px" } },
            sensorPicture(),
            h(
              "div",
              { style: { margin: "16px 0 0 25px" } },
              skinButton("⇣  校准", 114, connected(), () =>
                run(
                  async (device) => {
                    report("正在校准。请在使用表面上持续移动鼠标，保持鼠标接触表面。");
                    const level = await device.calibrateLift(5);
                    sens.liftEnabled = true;
                    sens.lift = level;
                    return level;
                  },
                  (level) => `校准结果：${level} 级。点击“应用”保存。`,
                  { refreshAfter: false },
                ),
              ),
            ),
          ),
          h(
            "div",
            { style: { margin: "96px 0 0 24px", display: "flex", flexDirection: "column", gap: "24px" } },
            option("预设高度", !sens.liftEnabled, () => ((sens.liftEnabled = false), render())),
            option("微精校准", sens.liftEnabled, () => ((sens.liftEnabled = true), render())),
          ),
          h(
            "div",
            { style: { margin: "4px 0 0 50px" } },
            ladder(sens.lift, sens.liftEnabled, (v) => {
              sens.lift = v;
              valueEl.textContent = v;
            }),
          ),
          h("div", { style: { margin: "-4px 0 0 64px" } }, sideView(sens.liftEnabled ? sens.lift : null)),
          h("div", { style: { margin: "332px 0 0 10px" } }, valueEl),
        ),
      ),
    ],
    footer: [
      ["恢复默认", true, () => ((sens.liftEnabled = false), (sens.lift = 16), render())],
      ["取消", true, () => (loadSensitivity(), render())],
      [
        "应用",
        connected(),
        () => {
          let block;
          try {
            block = state.editor.liftBlock(sens.liftEnabled ? sens.lift : undefined);
          } catch (error) {
            return report(message(error), true);
          }
          run((device) => device.saveLift(block, runtimeFor(block)), "抬起高度已保存。运行值已读回一致。");
        },
      ],
    ],
  };
}

// ---- 编程设置 (src/app/shooting.rs, src/app/oscar.rs) ----

function compileMacro() {
  const macro = state.macro;
  if (!state.editor) return;
  try {
    macro.program = state.editor.setMacro(macro.press, macro.release, macro.repeat);
    macro.error = null;
  } catch (error) {
    macro.program = null;
    macro.error = message(error);
  }
}

const KINDS = [
  ["a", "A-多连"],
  ["b", "B-循环"],
  ["c", "C-机枪"],
  ["d", "D-临阵"],
  ["m", "M-宏枪"],
];

const STATEMENTS = {
  a: "A-多连：限使用于 [手枪与连狙及机枪类] 的多连发压枪射击。",
  b: "B-循环：限使用于 [手枪与连狙及机枪类] 的循环压枪射击。",
  c: "C-机枪：限使用于 [连狙及机枪类] 的机枪压枪射击。",
  d: "D-临阵：限使用于 [手枪与连狙及机枪类] 临阵可调的循环压枪射击。",
  m: "M-宏枪：限转换 [奥斯卡宏指令] 角色动作与射击合并于左键。",
};

/// Switch keys (1-based buttons, as trigger_name) and their records.
const TRIGGERS = [
  [9, "“1”键"],
  [10, "“C”键"],
  [11, "“3”键"],
  [3, "中键"],
  [4, "后侧键"],
  [5, "前侧键"],
];

function shootingInput(draft) {
  const count = draft.kind === "d" ? 1 : draft.count;
  return {
    kind: draft.kind,
    shots: draft.shots.slice(0, count),
    loopStart: Math.min(draft.loopStart, count),
    recover: draft.recover,
    special: draft.special,
  };
}

function shotsArea(draft) {
  const gray = (text) => h("span", { class: "light-gray", style: { fontSize: "16px" } }, text);
  const statement = h(
    "div",
    { style: { marginTop: "20px" } },
    h("div", { class: "red", style: { fontSize: "15px" } }, "说明>>"),
    h("div", { style: { fontSize: "15px", color: "#8a8a8a", marginTop: "6px" } }, STATEMENTS[draft.kind]),
    h("div", { style: { fontSize: "13px", color: "#8a8a8a", marginTop: "6px" } }, "X 向右为正，Y 向下为正，位移使用鼠标计数值。"),
  );
  if (draft.kind === "m") {
    return [gray("M-宏枪：使用“奥斯卡II”页当前编译的按下和弹起程序。"), statement];
  }
  const limit = draft.kind === "a" ? 5 : draft.kind === "d" ? 1 : 8;
  draft.count = Math.min(Math.max(draft.count, 1), limit);
  draft.loopStart = Math.min(Math.max(draft.loopStart, 1), draft.count);
  const shots = draft.shots.slice(0, draft.count);
  const vertical = draft.kind === "d";
  const movement = (i) => draft.kind !== "a" || i + 1 < draft.count;
  return [
    h(
      "div",
      { class: "row" },
      gray("设定弹数"),
      number(draft.count, 1, limit, (v) => ((draft.count = v), render()), 40),
      h("span", { style: { width: "14px" } }),
      gray("准心回位"),
      check("", draft.recover, (on) => ((draft.recover = on), render())),
      draft.kind === "b" || draft.kind === "c"
        ? [h("span", { style: { width: "14px" } }), gray("次循环 从"), number(draft.loopStart, 1, draft.count, (v) => ((draft.loopStart = v), render()), 40), gray(`到 ${draft.count} 发`)]
        : null,
      h("span", { style: { width: "14px" } }),
      check("等待左键状态", draft.special, (on) => ((draft.special = on), render())),
    ),
    h(
      "table",
      { class: "shots" },
      h("tr", null, h("td"), shots.map((_, i) => h("th", null, `第 ${i + 1} 发`))),
      h("tr", null, h("td", null, "连击间歇\n(毫秒)"), shots.map((shot) => h("td", null, number(shot.intervalMs, 1, 999, (v) => ((shot.intervalMs = v), render()), 50)))),
      h(
        "tr",
        null,
        h("td", null, vertical ? "修正Y轴  Y=" : "修正(X,Y)  Y="),
        shots.map((shot, i) => h("td", null, number(shot.y, -126, 126, (v) => ((shot.y = v), render()), 50, !movement(i)))),
      ),
      h(
        "tr",
        null,
        h("td", null, "偏移量  X="),
        shots.map((shot, i) => h("td", null, number(shot.x, -126, 126, (v) => ((shot.x = v), render()), 50, !movement(i) || vertical))),
      ),
    ),
    statement,
  ];
}

function shootingPage(sniper) {
  if (!state.editor) return { body: needsImage(), footer: [] };
  const shoot = state.shoot;
  const titles = sniper ? ["狙击设置", "狙击类别", "编程区"] : ["左键编程", "编程文件", "编程区"];
  const slot = sniper ? shoot.sniper : shoot.slot;
  const draft = shoot.drafts[slot - 1];
  const slots = sniper ? [5, 6, 7] : [1, 2, 3, 4];
  let preview = null;
  let previewError = null;
  try {
    preview = state.editor.shootingPreview(shootingInput(draft));
  } catch (error) {
    previewError = message(error);
  }
  const listing = (lines) => lines.map((line) => `${String(line.offset).padStart(4, "0")}  ${t(line.text)}`).join("\n");
  const columns = h(
    "div",
    { class: "columns" },
    h(
      "div",
      { class: "column", style: { width: "304px" } },
      step("①", titles[0]),
      h("div", { class: "label", style: { margin: "40px 0 8px" } }, "程序槽位"),
      slots.map((index) =>
        h(
          "button",
          {
            class: `list-item${slot === index ? " active" : ""}`,
            onclick: () => {
              if (sniper) shoot.sniper = index;
              else shoot.slot = index;
              render();
            },
          },
          sniper ? `狙击键 ${index - 4}` : `程序槽位 ${index}`,
        ),
      ),
      h("div", { class: "label", style: { margin: "30px 0 8px" } }, "切换按键"),
      h(
        "div",
        { style: { display: "flex", flexDirection: "column", gap: "10px", alignItems: "flex-start" } },
        select(
          TRIGGERS.map(([, name]) => name),
          Math.max(0, TRIGGERS.findIndex(([button]) => button - 1 === shoot.trigger)),
          (i) => (shoot.trigger = TRIGGERS[i][0] - 1),
          260,
        ),
        check("全部配置", shoot.allProfiles, (on) => (shoot.allProfiles = on)),
        skinButton(
          "指定切换按键",
          273,
          true,
          () => {
            try {
              state.editor.assignShootingTrigger(slot, shoot.trigger, shoot.allProfiles ? undefined : state.profile);
              state.keysDirty = true;
              loadKeysDraftOnly();
              report("已指定切换按键。点击“应用”后写入。");
            } catch (error) {
              report(message(error), true);
            }
            render();
          },
          "把切换按键设为执行此槽位，点击“应用”后写入",
        ),
      ),
    ),
    h(
      "div",
      { class: "column", style: { width: "205px", padding: "13px 12px" } },
      step("②", titles[1]),
      h(
        "div",
        { style: { marginTop: "46px" } },
        KINDS.map(([code, name]) => h("button", { class: `mode${draft.kind === code ? " active" : ""}`, onclick: () => ((draft.kind = code), render()) }, name)),
      ),
    ),
    h("div", { class: "column", style: { flex: "1", padding: "13px 20px 20px" } }, step("③", titles[2]), h("div", { style: { height: "14px" } }), shotsArea(draft)),
  );
  const config = () => {
    state.editor.setShooting(slot, shootingInput(draft), undefined, undefined);
    state.keysDirty = true;
    loadKeysDraftOnly();
  };
  return {
    body: [
      columns,
      h(
        "details",
        { style: { marginTop: "12px" } },
        h("summary", null, "查看编译程序"),
        preview
          ? h(
              "div",
              { style: { paddingLeft: "14px" } },
              h("div", null, "按下"),
              h("div", { class: "mono" }, listing(preview.press_lines)),
              h("div", null, "弹起"),
              h("div", { class: "mono" }, listing(preview.release_lines)),
            )
          : null,
      ),
      previewError ? h("div", { class: "error" }, previewError) : null,
      h("div", { class: "error" }, state.editor.info().shooting_note),
    ],
    footer: [
      [
        "载入按键草稿",
        !previewError,
        () => {
          try {
            config();
            report("已载入按键草稿。可在“按键”页继续修改后点击“应用”。");
          } catch (error) {
            report(message(error), true);
          }
          render();
        },
      ],
      [
        "应用",
        connected() && !previewError,
        () => {
          let block;
          try {
            config();
            block = state.editor.buttonsBlock();
          } catch (error) {
            report(message(error), true);
            return render();
          }
          run((device) => device.saveButtons(block), "射击设置已保存，设备读回一致。", { keepKeys: false });
        },
      ],
    ],
  };
}

/// 指令区 categories and their colour marks.
const CATEGORIES = [
  ["键盘", "rgb(28,160,110)"],
  ["鼠标", "rgb(150,40,200)"],
  ["动作", "rgb(60,70,220)"],
  ["控制", "rgb(240,140,20)"],
  ["运算", "rgb(40,180,60)"],
];

// On-screen keyboard rows: caption, HID usage and width in key units.
const KEY_ROWS = [
  [["ESC", 41, 1], ["", 0, 1], ["F1", 58, 1], ["F2", 59, 1], ["F3", 60, 1], ["F4", 61, 1], ["", 0, 0.3], ["F5", 62, 1], ["F6", 63, 1], ["F7", 64, 1], ["F8", 65, 1], ["", 0, 0.3], ["F9", 66, 1], ["F10", 67, 1], ["F11", 68, 1], ["F12", 69, 1]],
  [],
  [["~ `", 53, 1], ["1", 30, 1], ["2", 31, 1], ["3", 32, 1], ["4", 33, 1], ["5", 34, 1], ["6", 35, 1], ["7", 36, 1], ["8", 37, 1], ["9", 38, 1], ["0", 39, 1], ["_ -", 45, 1], ["+ =", 46, 1], ["back", 42, 1.75]],
  [["Tab", 43, 1.3], ["Q", 20, 1], ["W", 26, 1], ["E", 8, 1], ["R", 21, 1], ["T", 23, 1], ["Y", 28, 1], ["U", 24, 1], ["I", 12, 1], ["O", 18, 1], ["P", 19, 1], ["{ [", 47, 1], ["} ]", 48, 1], ["| \\", 49, 1.45]],
  [["Caps", 57, 1.6], ["A", 4, 1], ["S", 22, 1], ["D", 7, 1], ["F", 9, 1], ["G", 10, 1], ["H", 11, 1], ["J", 13, 1], ["K", 14, 1], ["L", 15, 1], [": ;", 51, 1], ["\" '", 52, 1], ["Enter", 40, 2.15]],
  [["Shift", 225, 2], ["Z", 29, 1], ["X", 27, 1], ["C", 6, 1], ["V", 25, 1], ["B", 5, 1], ["N", 17, 1], ["M", 16, 1], ["< ,", 54, 1], ["> .", 55, 1], ["? /", 56, 1], ["RShift", 229, 2.75]],
  [["Ctrl", 224, 1.2], ["LW", 227, 1.1], ["Alt", 226, 1.1], ["Space", 44, 6.4], ["RAlt", 230, 1.1], ["RW", 231, 1.1], ["MENU", 101, 1.2], ["RCtrl", 228, 1.2]],
];
const EDIT_KEYS = [
  [["Print", 70], ["Scroll", 71], ["Pause", 72]],
  [["Insert", 73], ["Home", 74], ["Up", 75]],
  [["Delete", 76], ["End", 77], ["Down", 78]],
  [["", 0], ["^", 82], ["", 0]],
  [["<", 80], ["v", 81], [">", 79]],
];
const PAD_KEYS = [
  [["LOCK", 83], ["/", 84], ["*", 85], ["-", 86]],
  [["7", 95], ["8", 96], ["9", 97], ["+", 87]],
  [["4", 92], ["5", 93], ["6", 94], ["", 0]],
  [["1", 89], ["2", 90], ["3", 91], ["Enter", 88]],
  [["0", 98], ["", 0], ["Del", 99], ["", 0]],
];

/// HID usage of a key typed on the physical keyboard (KeyboardEvent.code).
const CODE_USAGE = (() => {
  const map = { Enter: 40, Escape: 41, Backspace: 42, Tab: 43, Space: 44, Minus: 45, Equal: 46, BracketLeft: 47, BracketRight: 48, Backslash: 49, Semicolon: 51, Quote: 52, Backquote: 53, Comma: 54, Period: 55, Slash: 56, CapsLock: 57, PrintScreen: 70, ScrollLock: 71, Pause: 72, Insert: 73, Home: 74, PageUp: 75, Delete: 76, End: 77, PageDown: 78, ArrowRight: 79, ArrowLeft: 80, ArrowDown: 81, ArrowUp: 82, NumLock: 83, NumpadDivide: 84, NumpadMultiply: 85, NumpadSubtract: 86, NumpadAdd: 87, NumpadEnter: 88, Numpad0: 98, NumpadDecimal: 99, ContextMenu: 101, ControlLeft: 224, ShiftLeft: 225, AltLeft: 226, MetaLeft: 227, ControlRight: 228, ShiftRight: 229, AltRight: 230, MetaRight: 231 };
  for (let i = 0; i < 26; i++) map[`Key${String.fromCharCode(65 + i)}`] = 4 + i;
  for (let i = 1; i <= 9; i++) map[`Digit${i}`] = 29 + i;
  map.Digit0 = 39;
  for (let i = 1; i <= 12; i++) map[`F${i}`] = 57 + i;
  for (let i = 1; i <= 9; i++) map[`Numpad${i}`] = 88 + i;
  return map;
})();

function editPress(text) {
  const macro = state.macro;
  if (text === macro.press) return;
  state.oscar.undo.push(macro.press);
  state.oscar.redo = [];
  macro.press = text;
  compileMacro();
}

function insertLines(text) {
  let press = state.macro.press;
  if (press && !press.endsWith("\n")) press += "\n";
  editPress(press + text);
  render();
}

function insertKey(usage) {
  const delay = state.oscar.keyDelay;
  insertLines(`KeyDown ${usage}\nDelay ${delay}\nKeyUp ${usage}`);
}

function onScreenKeyboard() {
  const unit = 30;
  const height = 26;
  const y = (row) => row * (height + 4) - (row > 1 ? 26 : 0);
  const cap = (caption, usage, left, top, width, tall = height) =>
    h("button", { class: "key-cap", style: { left: px(left), top: px(top), width: px(width), height: px(tall) }, onclick: () => insertKey(usage) }, caption);
  const keys = [];
  KEY_ROWS.forEach((row, index) => {
    let x = 0;
    for (const [caption, usage, width] of row) {
      if (usage) keys.push(cap(caption, usage, x, y(index), unit * width - 2));
      x += unit * width;
    }
  });
  const editLeft = unit * 15.2;
  EDIT_KEYS.forEach((row, index) =>
    row.forEach(([caption, usage], column) => usage && keys.push(cap(caption, usage, editLeft + column * (unit + 10), y(index + 2), unit + 8))),
  );
  const padLeft = editLeft + 3 * (unit + 10) + 12;
  PAD_KEYS.forEach((row, index) =>
    row.forEach(([caption, usage], column) => {
      if (!usage) return;
      const tall = usage === 87 || usage === 88;
      keys.push(cap(caption, usage, padLeft + column * unit, y(index + 2), usage === 98 ? unit * 2 - 2 : unit - 2, tall ? height * 2 + 4 : height));
    }),
  );
  return h("div", { class: "keyboard", style: { height: "182px" } }, keys);
}

function emulation() {
  const o = state.oscar;
  const gray = (text) => h("span", { class: "gray" }, text);
  const insert = (text) => tool("插入", false, () => insertLines(typeof text === "function" ? text() : text));
  switch (o.category) {
    case 0:
      return [
        onScreenKeyboard(),
        h("div", { style: { height: "24px" } }),
        h("span", { title: "开启后，在本页按下的键盘按键会插入编程区。" }, check("实体键盘输入", o.physical, (on) => ((o.physical = on), render()))),
        h("div", { class: "row", style: { marginTop: "10px" } }, number(o.keyDelay, 0, 999999, (v) => (o.keyDelay = v), 64), "：预设延迟时间(毫秒)"),
      ];
    case 1:
      return [
        gray("鼠标按键"),
        h(
          "div",
          { style: { display: "grid", gridTemplateColumns: "auto auto auto auto", gap: "8px", justifyContent: "start", alignItems: "center", margin: "8px 0" } },
          [
            ["左键", "Left"],
            ["右键", "Right"],
            ["中键", "Middle"],
            ["四键", "Button4"],
            ["五键", "Button5"],
          ].map(([caption, name]) => [
            h("span", null, caption),
            tool("按下", false, () => insertLines(`${name}Down`)),
            tool("弹起", false, () => insertLines(`${name}Up`)),
            tool("单击", false, () => insertLines(`${name}Down\nDelay ${o.keyDelay}\n${name}Up`)),
          ]),
        ),
        h("div", { class: "row" }, gray("滚轮"), tool("前滚", false, () => insertLines("WheelUp")), tool("后滚", false, () => insertLines("WheelDown"))),
      ];
    case 2:
      return [
        gray("鼠标相对移动坐标(X,Y)"),
        h(
          "div",
          { class: "row", style: { margin: "6px 0 14px" } },
          number(o.motion[0], -32768, 32767, (v) => (o.motion[0] = v)),
          number(o.motion[1], -32768, 32767, (v) => (o.motion[1] = v)),
          insert(() => `MoveR ${o.motion[0]} ${o.motion[1]}`),
        ),
        gray("鼠标绝对坐标(X,Y)"),
        h(
          "div",
          { class: "row", style: { marginTop: "6px" } },
          number(o.position[0], 0, 65535, (v) => (o.position[0] = v)),
          number(o.position[1], 0, 65535, (v) => (o.position[1] = v)),
          insert(() => `MoveTo ${o.position[0]} ${o.position[1]}`),
        ),
      ];
    case 3:
      return h(
        "div",
        { style: { display: "flex", flexDirection: "column", gap: "10px" } },
        h("div", { class: "row" }, gray("延时"), number(o.delay, 0, 999999, (v) => (o.delay = v), 80), gray("ms"), insert(() => `Delay ${o.delay}`)),
        h("div", { class: "row" }, gray("跳跃前往第"), number(o.goto, 1, 16384, (v) => ((o.goto = v), render())), gray("行"), insert(() => `Goto ${o.goto}`)),
        h(
          "div",
          { class: "row" },
          gray("从第"),
          number(o.goto, 1, 16384, (v) => ((o.goto = v), render())),
          gray("行起循环"),
          number(o.count, 1, 65535, (v) => (o.count = v)),
          gray("次"),
          insert(() => `gowhile ${o.goto} ${o.count}`),
        ),
        h(
          "div",
          { class: "row" },
          gray("按键"),
          number(o.key, 1, 255, (v) => (o.key = v)),
          gray("按住时前往第"),
          number(o.goto, 1, 16384, (v) => ((o.goto = v), render())),
          gray("行"),
          insert(() => `ifkey ${o.key} 1 ${o.goto}`),
        ),
      );
    default:
      return h(
        "div",
        { style: { display: "flex", flexDirection: "column", gap: "6px", alignItems: "flex-start" } },
        gray("变量 VarA ~ VarZ"),
        [
          ["定义变量 VarA = 0", "var VarA = 0"],
          ["VarA 加 1", "VarA = VarA + 1"],
          ["VarA 减 1", "VarA = VarA - 1"],
          ["VarA 不等于 3 时前往第 1 行", "if VarA != 3 goto 1"],
        ].map(([caption, text]) => tool(caption, false, () => insertLines(text))),
      );
  }
}

/// Text editor with line numbers, as the desktop memo.
function memo(text, rows, enabled, oninput) {
  const lines = Math.max(rows, text.split("\n").length);
  const gutter = h("div", { class: "gutter" }, Array.from({ length: lines }, (_, i) => String(i + 1).padStart(4)).join("\n"));
  const area = h("textarea", {
    value: text,
    rows,
    spellcheck: false,
    disabled: !enabled,
    style: { height: px(rows * 18 + 4) },
    oninput: (e) => {
      oninput(e.target.value);
      const count = Math.max(rows, e.target.value.split("\n").length);
      gutter.textContent = Array.from({ length: count }, (_, i) => String(i + 1).padStart(4)).join("\n");
    },
    onscroll: (e) => (gutter.scrollTop = e.target.scrollTop),
  });
  gutter.style.height = px(rows * 18 + 4);
  return h("div", { class: "memo" }, gutter, area);
}

function oscarPage() {
  const macro = state.macro;
  const o = state.oscar;
  const full = 1389;
  const left = 140;
  const right = Math.min(450, Math.max(360, full * 0.32));
  const middle = full - left - right - 32;
  const importInput = h("input", {
    type: "file",
    accept: ".amc",
    hidden: true,
    onchange: async (event) => {
      const file = event.target.files[0];
      event.target.value = "";
      if (!file) return;
      try {
        const amc = wasm.parseAmc(new Uint8Array(await file.arrayBuffer()));
        editPress(amc.press);
        macro.release = amc.release;
        macro.repeat = amc.repeat;
        macro.file = file.name.replace(/\.amc$/i, "");
        compileMacro();
        report(`已导入 AMC：${amc.description}${amc.high_language ? "（HighLanguage 已转换为 M 文本）" : ""}`);
      } catch (error) {
        report(message(error), true);
      }
      render();
    },
  });
  const result = h("div", { id: "macro-result" });
  const showResult = () =>
    result.replaceChildren(
      macro.error
        ? h("div", { class: "error" }, macro.error)
        : h("div", { class: "gray small" }, `编译通过 · 按下 ${macro.program.press.length} 字 · 弹起 ${macro.program.release.length} 字 · 在“按键”页选择“宏”即可指定给按键`),
    );
  showResult();
  const tools = [
    ["💾", "导出 AMC", true, () => download(wasm.exportAmc(macro.file, macro.press, macro.release, macro.repeat), `${macro.file || "macro"}.amc`)],
    ["📂", "导入 AMC", true, () => importInput.click()],
    [
      "↶",
      "撤销",
      o.undo.length > 0,
      () => {
        o.redo.push(macro.press);
        macro.press = o.undo.pop();
        compileMacro();
        render();
      },
    ],
    [
      "↷",
      "重做",
      o.redo.length > 0,
      () => {
        o.undo.push(macro.press);
        macro.press = o.redo.pop();
        compileMacro();
        render();
      },
    ],
    ["🗑", "清空编程区", macro.press !== "", () => (editPress(""), render())],
  ];
  const details = macro.program
    ? h(
        "details",
        { style: { marginTop: "14px" } },
        h("summary", null, "编码与导出"),
        h(
          "div",
          { class: "row", style: { gap: "10px", margin: "6px 0 8px 14px" } },
          skinButton("复制按下字节", 150, true, () => navigator.clipboard.writeText(byteHex(macro.program.press))),
          skinButton("复制弹起字节", 150, true, () => navigator.clipboard.writeText(byteHex(macro.program.release))),
          skinButton("导出按下程序", 150, true, () => download(wordsToBytes(macro.program.press), `${macro.file || "macro"}-press.bin`)),
          skinButton("导出弹起程序", 150, true, () => download(wordsToBytes(macro.program.release), `${macro.file || "macro"}-release.bin`)),
        ),
        [
          ["按下", macro.program.press_lines],
          ["弹起", macro.program.release_lines],
        ].map(([title, lines]) => [
          h("div", { style: { paddingLeft: "14px" } }, title),
          h("div", { class: "mono", style: { paddingLeft: "14px" } }, lines.map((l) => `${String(l.offset).padStart(4, "0")}  ${l.hex.padEnd(15)}${t(l.text)}`).join("\n")),
        ]),
      )
    : null;
  return {
    body: [
      h(
        "div",
        { class: "row", style: { gap: "16px", alignItems: "flex-start" } },
        h(
          "div",
          { style: { width: px(left + 16 + middle) } },
          h(
            "div",
            { class: "row", style: { gap: "8px" } },
            step("①", "指令区"),
            h("span", { style: { width: px(left - 90) } }),
            step("②", "仿真区"),
            h("span", { style: { width: "16px" } }),
            h("span", { class: "gray", style: { fontSize: "16px" } }, "重复方式"),
            [
              [0, "单次", "不重复执行"],
              [1, "按住", "按下：执行\n弹起：终止执行"],
              [2, "开关", "按下：执行\n再按：终止执行"],
            ].map(([mode, title, hint]) =>
              tool(title, macro.repeat === mode, () => {
                macro.repeat = mode;
                compileMacro();
                render();
              }, hint),
            ),
          ),
          h(
            "div",
            { class: "row", style: { gap: "16px", alignItems: "flex-start", marginTop: "8px" } },
            h(
              "div",
              { style: { width: px(left), minHeight: "490px", background: "var(--column)", padding: "30px 22px" } },
              CATEGORIES.map(([caption, color], index) =>
                h("button", { class: `category${o.category === index ? " active" : ""}`, style: { "--mark": color }, onclick: () => ((o.category = index), render()) }, caption),
              ),
            ),
            h("div", { style: { width: px(middle), minHeight: "454px", background: "var(--column)", padding: "24px 16px" } }, emulation()),
          ),
        ),
        h(
          "div",
          { style: { width: px(right) } },
          step("③", "编程区"),
          h(
            "div",
            { class: "row", style: { margin: "8px 0" } },
            h("span", { class: "gray", style: { fontSize: "18px" } }, "文件"),
            h("input", { type: "text", value: macro.file, placeholder: "AMC 文件名", style: { width: px(right - 60), height: "32px" }, oninput: (e) => (macro.file = e.target.value) }),
            importInput,
          ),
          h(
            "div",
            { class: "program-bar" },
            tools.map(([glyph, hint, enabled, action]) => h("button", { title: hint, disabled: !enabled, onclick: action }, glyph)),
          ),
          memo(macro.press, 15, true, (text) => {
            editPress(text);
            showResult();
          }),
          h("div", { class: "gray", style: { margin: "8px 0 0" } }, macro.repeat === 0 ? "弹起程序" : "弹起程序由重复方式决定"),
          memo(macro.release, 3, macro.repeat === 0, (text) => {
            macro.release = text;
            compileMacro();
            showResult();
          }),
          h("div", { style: { marginTop: "8px" } }, result),
          h("div", { class: "light-gray small", style: { marginTop: "6px" } }, "⚠ 测试宏无法停止时，同时按下 Caps Lock、Scroll Lock、Num Lock 中断。"),
        ),
      ),
      details,
      o.notes ? notesDialog() : null,
    ],
    footer: [
      ["功能注解", true, () => ((o.notes = !o.notes), render())],
      ["中断宏", false, null, "浏览器不能向鼠标的键盘接口发送指示灯报告。请同时按下 Caps Lock、Scroll Lock、Num Lock。"],
      [
        "测试",
        connected() && !macro.error,
        () => {
          let block;
          try {
            block = state.editor.testMacroBlock(state.snapshot.profile[0]);
          } catch (error) {
            return report(message(error), true);
          }
          run((device) => device.saveButtons(block), "测试宏已写入当前配置的后侧键（第四键）。");
        },
      ],
    ],
  };
}

function notesDialog() {
  return h(
    "div",
    { class: "dialog" },
    h("h2", null, "功能注解"),
    h("button", { class: "close", onclick: () => ((state.oscar.notes = false), render()) }, "✕"),
    h("div", { class: "mono" }, "KeyDown 4\nDelay 100\nKeyUp 4\nMoveR 20 -5\nMoveTo 400 300\nvar VarA = 0\nVarA = VarA + 1\nif VarA != 3 goto 5\ngowhile 1 3\nifkey 4 1 9"),
    h("p", null, "跳转使用源代码行号。gowhile X N 把第 X 行至此行重复 N 次；", h("br"), "ifkey K S L 在按键 K 按住（S=1）或松开（S=0）时跳到第 L 行。"),
    h("p", null, "HighLanguage 格式的 AMC 文件导入时会转换为 M 文本。"),
    h("p", null, "测试：写入当前配置的后侧键（第四键）。", h("br"), "宏无法停止时，同时按下 Caps Lock、Scroll Lock、Num Lock 中断。"),
  );
}

// ---- 灯光设置 ----

/// Four-stop brightness selector with square knobs and bulbs below.
function levels(current, enabled, onpick) {
  const span = 806 - 84;
  const bulb = (level) => {
    const rays = [0, 2, 4, 6][level];
    const lines = Array.from({ length: rays }, (_, i) => {
      const angle = -Math.PI / 2 + (i - (rays - 1) / 2) * (Math.PI / 4) * 0.9;
      const [dx, dy] = [Math.cos(angle), Math.sin(angle)];
      return `<path d="M${20 + dx * 12} ${14 + dy * 12}L${20 + dx * 17} ${14 + dy * 17}"/>`;
    }).join("");
    return svg(
      40,
      40,
      `<g fill="none" stroke="#969698" stroke-width="2"><circle cx="20" cy="14" r="8"/><path d="M16 25h8M17 29h6"/>${lines}</g>${level === 3 ? '<text x="20" y="17.5" text-anchor="middle" font-size="10" fill="#969698">A</text>' : ""}`,
    );
  };
  return h(
    "div",
    { class: "levels" },
    h("div", { class: "rail" }),
    [0, 1, 2, 3].map((level) => {
      const x = 42 + (span * level) / 3;
      const el = bulb(level);
      el.style.left = px(x);
      return [h("button", { class: `knob${current === level ? " lit" : ""}`, style: { left: px(x) }, disabled: !enabled, onclick: () => onpick(level) }), el];
    }),
  );
}

function brightnessPage() {
  const current = state.snapshot?.brightness ?? null;
  const effects = wasm.lightEffects();
  const setLevel = (level) =>
    level !== current &&
    run(
      async (device) => {
        await device.setBrightness(level);
        state.snapshot.brightness = level;
      },
      `亮度已设为 ${level}`,
      { refreshAfter: false },
    );
  const effectOk = !!state.editor && state.source === "device";
  return {
    body: [
      h("div", { style: { height: "66px" } }),
      card(
        { icon: "light", title: "灯光亮度", help: "灯光亮度调整：\n提供鼠标四种背光亮度的设置，建议在使用无线鼠标选用低亮度或关灯，已提高无线续航能力。" },
        h("div", { style: { marginTop: "6px" } }, levels(current, connected(), setLevel)),
        h(
          "div",
          { class: "row", style: { width: "806px", margin: "30px auto 0" } },
          h("span", { style: { fontSize: "16px" } }, "RGB魔幻灯效："),
          h(
            "select",
            {
              style: { width: "180px" },
              disabled: !effectOk || state.busy,
              onchange: (e) => {
                const effect = Number(e.target.value);
                let block;
                try {
                  block = savedEditorWith((editor) => editor.setLightEffect(effect));
                } catch (error) {
                  return report(message(error), true);
                }
                run((device) => device.saveLighting(block), "灯效已保存并应用");
              },
            },
            state.lightEffect === null ? h("option", { selected: true, value: "" }, "") : null,
            effects.map((name, index) => h("option", { value: index, selected: index === state.lightEffect }, name)),
          ),
        ),
        h("div", { style: { height: "64px" } }),
        current === null ? h("div", { class: "gray" }, "请先连接鼠标。") : null,
      ),
    ],
    footer: [["恢复默认", connected(), () => setLevel(3)]],
  };
}

/// A key table built from the saved one with a single change, so unsaved
/// key drafts are not written with it.
function savedEditorWith(change) {
  const editor = new wasm.Editor(state.bytes);
  try {
    change(editor);
    return editor.buttonsBlock();
  } finally {
    editor.free();
  }
}

// ---- 其他设置 (src/app/other.rs) ----

function aboutPage() {
  const languages = wasm.languages();
  return {
    body: [
      h("div", { style: { height: "12px" } }),
      h(
        "div",
        { class: "card", style: { width: "689px", margin: "0 0 25px", padding: "16px 40px 16px 34px", minHeight: "100px" } },
        h(
          "div",
          { class: "card-title", style: { gap: "31px" } },
          icon("language"),
          h("span", { style: { flex: "1" } }, "显示语言"),
          languageSelect(languages),
        ),
      ),
      h(
        "div",
        { class: "card", style: { width: "auto", margin: "0", padding: "16px" } },
        h("div", { style: { fontSize: "22px", fontWeight: "700" } }, "Bloody A90 配置工具 · 网页版"),
        h("div", { class: "gray", style: { margin: "6px 0 14px" } }, "WebHID · Rust / wasm · 与桌面版共用配置编码"),
        [
          "已接入：设备识别、闪存读取、灯光亮度与灯效、DPI 和回报率保存、按键与宏保存、射击编程、抬起高度、表面校准、硬件响应时间、AMC 宏文件、HighLanguage、备份恢复。",
          "写入只使用与官方程序相同的保存流程，并限制在灵敏度页和 2600 至 3cff 的按键区。连接后先读取完整闪存并保存在浏览器中，再允许写入。",
          "即时 DPI 和回报率使用已验证的命令 0d。不发送传感器传输、引导程序覆盖或授权写入命令。",
          "主机功能（手势、主机动作、应用关联、桌面滚轮和指针设置、宏库、开机启动）需要桌面版。",
          "需要 Chrome、Edge 等支持 WebHID 的浏览器。Linux 上需安装 contrib/70-bloody-a90.rules 以获得 hidraw 权限。桌面版运行时请勿同时写入。",
        ].map((text) => h("p", { style: { margin: "0 0 10px" } }, text)),
        hid.supported() ? null : h("p", { class: "error" }, "此浏览器不支持 WebHID。"),
        h("a", { class: "link", href: REPOSITORY, target: "_blank", rel: "noopener" }, `GitHub · ${REPOSITORY.replace("https://", "")}`),
      ),
    ],
    footer: [],
  };
}

/// Language names stay in their own language, so they skip translation.
function languageSelect(languages) {
  const el = select(
    languages.map((language) => language.code),
    Math.max(0, languages.findIndex((language) => language.code === state.language)),
    (index) => {
      setLanguage(languages[index].code);
      render();
    },
    120,
  );
  [...el.options].forEach((option, index) => (option.textContent = languages[index].name));
  return el;
}

function field(caption, value) {
  return h("div", { class: "field" }, h("span", null, `${caption} :`), h("span", { class: "value", style: { width: "240px" } }, value));
}

function hardwarePage() {
  const snapshot = state.snapshot;
  let body;
  if (!snapshot) {
    body = h("div", { class: "gray" }, "请先连接鼠标。");
  } else {
    const { identity, info, profile } = snapshot;
    const rows = [
      ["机身码ID", hex(identity.model, 4)],
      ["固件", hex(identity.firmware, 2)],
      ["核心", String(identity.core)],
      ["接口", `WebHID · ${state.device?.hid.productName ?? ""}`],
      ["回报率", `${info.rateHz} Hz`],
      ["当前配置", `${profile[0] + 1} / ${profile[1] + 1}`],
      ["灵敏度区", `${hex(info.sensitivity, 4)}（出厂 ${hex(info.factorySensitivity, 4)}）`],
      ["按键区", `${hex(info.buttons, 4)}（出厂 ${hex(info.factoryButtons, 4)}）`],
      ["设置区", hex(info.settings, 4)],
      ["设备数据", hex(info.deviceData, 4)],
    ];
    body = h(
      "div",
      { class: "row", style: { marginTop: "-46px", justifyContent: "flex-end", alignItems: "flex-start", gap: "40px", paddingRight: "40px" } },
      [rows.slice(0, 5), rows.slice(5)].map((column) =>
        h("div", { style: { display: "flex", flexDirection: "column", gap: "12px" } }, column.map(([name, value]) => field(name, value))),
      ),
    );
  }
  return {
    body: [h("div", { style: { height: "36px" } }), card({ icon: "card", title: "硬件信息", help: "硬件信息：设备型号、固件和存储区地址。" }, body)],
    footer: [],
  };
}

function backupPage() {
  const openInput = h("input", {
    type: "file",
    accept: ".bin,application/octet-stream",
    hidden: true,
    onchange: async (event) => {
      const file = event.target.files[0];
      event.target.value = "";
      if (!file) return;
      try {
        const bytes = new Uint8Array(await file.arrayBuffer());
        if (state.device) {
          // Inspect only: the editors keep the mouse's image.
          const editor = new wasm.Editor(bytes);
          state.backupText = editor.info().summary;
          editor.free();
        } else {
          openImage(bytes, "file", file.name);
          state.backupText = state.editor.info().summary;
        }
        report("备份读取完成");
      } catch (error) {
        state.backupText = "";
        report(message(error), true);
      }
      render();
    },
  });
  const restoreInput = h("input", {
    type: "file",
    accept: ".bin,application/octet-stream",
    hidden: true,
    onchange: async (event) => {
      const file = event.target.files[0];
      event.target.value = "";
      if (file) restoreFrom(new Uint8Array(await file.arrayBuffer()), file.name);
    },
  });
  const stored = storedBackups();
  return {
    body: [
      h(
        "div",
        { class: "row", style: { gap: "10px" } },
        skinButton("读取文件", 140, !state.busy, () => openInput.click()),
        skinButton("解析鼠标闪存", 140, !!state.editor && state.source === "device", () => ((state.backupText = state.editor.info().summary), render()), "请先连接鼠标"),
        skinButton("下载本次备份", 140, !!state.sessionBackup, () => download(state.sessionBackup, `a90-backup-${timestamp()}.bin`)),
        skinButton("下载当前闪存", 140, !!state.bytes && state.source === "device", () => download(state.bytes, `a90-flash-${timestamp()}.bin`)),
        skinButton("从备份恢复", 140, connected(), () => restoreInput.click(), "只写入灵敏度页和按键区中与备份不同的页"),
        openInput,
        restoreInput,
      ),
      stored.length
        ? h(
            "div",
            { class: "row", style: { gap: "10px", marginTop: "12px", flexWrap: "wrap" } },
            h("span", { class: "gray" }, "浏览器中保存的备份："),
            stored.map((entry) => tool(new Date(entry.time).toLocaleString(), false, () => download(decodeStored(entry), `a90-backup-${entry.time.replace(/[:.]/g, "-")}.bin`), "下载")),
          )
        : null,
      state.backupText
        ? [
            h("div", { class: "row", style: { marginTop: "12px" } }, skinButton("复制解析结果", 140, true, () => navigator.clipboard.writeText(state.backupText))),
            h("div", { class: "mono summary" }, state.backupText),
          ]
        : h("div", { class: "gray", style: { marginTop: "12px" } }, "尚未读取文件。"),
    ],
    footer: [],
  };
}

async function restoreFrom(bytes, name) {
  let backup;
  try {
    backup = new wasm.Editor(bytes);
  } catch (error) {
    return report(message(error), true);
  }
  const mine = state.editor.info();
  const theirs = backup.info();
  let template = null;
  let debounce;
  try {
    template = backup.sensitivityTemplate();
  } catch {
    // Nothing to apply at runtime.
  }
  try {
    debounce = backup.debounce();
  } catch {
    // Pages are still restored as stored.
  }
  backup.free();
  if (theirs.model !== mine.model || theirs.firmware !== mine.firmware) {
    return report(`备份的型号 ${hex(theirs.model, 4)} / 固件 ${hex(theirs.firmware, 2)} 与鼠标不同，不能恢复。`, true);
  }
  if (!confirm(t(`将鼠标的灵敏度页和按键区恢复为 ${name} 中的内容。只写入不同的页。继续吗？`))) return;
  run(
    (device) => device.restoreBackup(bytes, runtimeFor(template), debounce),
    (pages) => (pages.length ? `恢复了 ${pages.length} 页：${pages.map((p) => hex(p, 4)).join(", ")}` : "闪存已与备份一致，没有写入。"),
    { keepKeys: false },
  );
}

// ---- Shell ----

const PAGES = {
  keys: keysPage,
  resolution: resolutionPage,
  response: responsePage,
  rate: ratePage,
  lift: liftPage,
  shooting: () => shootingPage(false),
  sniper: () => shootingPage(true),
  oscar: oscarPage,
  brightness: brightnessPage,
  about: aboutPage,
  hardware: hardwarePage,
  backup: backupPage,
};

/// Pages built from group panels carry the badge on each panel.
const PANEL_PAGES = ["response", "rate", "lift", "brightness", "about", "hardware", "shooting", "sniper"];

function renderShell() {
  $("main-tabs").replaceChildren(
    ...MAIN_TABS.map(([main, caption]) =>
      h("button", { class: `main-tab${state.main === main ? " active" : ""}`, onclick: () => (openMain(main), render()) }, caption),
    ),
  );
  const strip = $("sub-strip");
  const tabs = [];
  if (state.main === "keys") {
    tabs.push(h("button", { class: "template", onclick: () => ((state.templateMenu = !state.templateMenu), render()) }, "全键编程模板"));
  }
  for (const [page, caption, hint] of SUB_TABS[state.main]) {
    tabs.push(
      h(
        "button",
        {
          class: `sub-tab${state.page === page ? " active" : ""}`,
          disabled: !page,
          title: hint,
          onclick: () => {
            state.page = page;
            state.popup = null;
            render();
          },
        },
        caption,
      ),
    );
  }
  strip.replaceChildren(...tabs);
  document.querySelector(".template-menu")?.remove();
  if (state.main === "keys" && state.templateMenu) {
    $("window").querySelector(".central").append(
      h(
        "div",
        { class: "template-menu" },
        h("button", { class: "template-item current", onclick: () => ((state.templateMenu = false), render()) }, "全键编程模板"),
        h(
          "button",
          {
            class: "template-item",
            onclick: () => {
              state.templateMenu = false;
              state.main = "programming";
              state.page = "shooting";
              render();
            },
          },
          "左键编程模板",
        ),
      ),
    );
  }
  $("banner").textContent = t(BANNERS[state.main]);
  profileStrip();
  const areaInfo = $("info");
  areaInfo.hidden = PANEL_PAGES.includes(state.page);
  areaInfo.title = t(DESCRIPTIONS[state.page] ?? "");

  let device;
  if (state.device && state.snapshot) {
    const { identity } = state.snapshot;
    device = `A90 · 固件 ${hex(identity.firmware, 2)} · 核心 ${identity.core} · WebHID`;
  } else if (state.editor) {
    device = `未连接鼠标 · 正在编辑备份 ${state.fileName}`;
  } else {
    device = "未连接鼠标 · 可在 其他设置 → 已有备份 读取文件";
  }
  $("device-text").textContent = t(device);
  $("version").textContent = t("v0.1.0 · 网页版");
  const connectButton = $("connect");
  connectButton.disabled = state.busy || !hid.supported();
  connectButton.textContent = t(state.device ? "重新读取" : "连接鼠标");
  connectButton.title = hid.supported() ? "" : t("此浏览器不支持 WebHID");
  report(state.status, state.failed);
}

function render() {
  renderShell();
  const page = $("page");
  const scroll = page.scrollTop;
  let content;
  try {
    content = PAGES[state.page]();
  } catch (error) {
    content = { body: h("div", { class: "error" }, message(error)), footer: [] };
  }
  page.replaceChildren(...[content.body].flat(Infinity).filter(Boolean));
  page.scrollTop = scroll;
  $("footer").replaceChildren(
    ...content.footer.map(([label, enabled, action, title]) => skinButton(label, 137, enabled && !state.busy, action, title)),
  );
  fitCaptions();
}

/// Translated captions can be longer than the official controls. Shrink a
/// caption to fit its fixed-size control, as skin::fitted does, down to 11px.
function fitCaptions() {
  const selector = ".main-tab, .sub-tab, .template, .template-item, .profile-tab, .skin-button, .mode, .list-item, .category, .combo, .key-cap";
  for (const el of document.querySelectorAll(selector)) {
    el.style.fontSize = "";
    const available = el.clientWidth - 4;
    if (el.scrollWidth <= el.clientWidth || available <= 0) continue;
    const size = parseFloat(getComputedStyle(el).fontSize);
    el.style.fontSize = px(Math.max(11, Math.floor((size * available) / el.scrollWidth)));
  }
}

const LOCALES = { ChineseS: "zh-CN", ChineseT: "zh-TW", English: "en", Dutch: "nl", French: "fr", German: "de", Italian: "it", Japanese: "ja", Korean: "ko", Polish: "pl", Portuguese: "pt", Russian: "ru", Spanish: "es", Turkish: "tr" };

/// The saved choice, else the first browser language with a table.
function initialLanguage() {
  try {
    const saved = localStorage.getItem("a90-language");
    if (saved && LOCALES[saved]) return saved;
  } catch {
    // Fall back to the browser languages.
  }
  for (const tag of navigator.languages ?? [navigator.language]) {
    const lower = tag.toLowerCase();
    if (/^zh-(tw|hk|mo|hant)/.test(lower)) return "ChineseT";
    if (lower.startsWith("zh")) return "ChineseS";
    const found = Object.entries(LOCALES).find(([, locale]) => locale.length === 2 && lower.split("-")[0] === locale);
    if (found) return found[0];
  }
  return "English";
}

function setLanguage(code) {
  wasm.setLanguage(code);
  state.language = code;
  document.documentElement.lang = LOCALES[code];
  document.title = code === "ChineseS" ? "Bloody A90" : `Bloody A90 · ${t("网页版")}`;
  try {
    localStorage.setItem("a90-language", code);
  } catch {
    // The choice then lasts for this page only.
  }
}

/// Zoom the 1640 × 950 window to fit, as the desktop tool fits the screen.
function fit() {
  const zoom = Math.min(window.innerWidth / 1640, window.innerHeight / 950);
  $("window").style.zoom = String(Math.max(0.5, zoom));
}

async function main() {
  fit();
  window.addEventListener("resize", fit);
  await init();
  setLanguage(initialLanguage());
  hid.setChecksums((words) => wasm.checksums(words));
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && (state.popup || state.templateMenu || state.oscar.notes)) {
      state.popup = null;
      state.templateMenu = false;
      state.oscar.notes = false;
      render();
      return;
    }
    // 实体键盘输入 of the Oscar keyboard category.
    const target = event.target;
    const typing = target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement;
    if (state.page === "oscar" && state.oscar.category === 0 && state.oscar.physical && !typing && !event.repeat) {
      const usage = CODE_USAGE[event.code];
      if (usage) {
        event.preventDefault();
        insertKey(usage);
      }
    }
  });
  if (!hid.supported()) {
    state.status = "此浏览器不支持 WebHID。请使用桌面版 Chrome 或 Edge，并通过 HTTPS 打开本页。";
    state.failed = true;
  }
  render();
  $("connect").addEventListener("click", async () => {
    try {
      const device = state.device?.hid ?? (await hid.requestDevice());
      if (device) await connect(device);
    } catch (error) {
      report(message(error), true);
    }
  });
  if (hid.supported()) {
    navigator.hid.addEventListener("disconnect", (event) => {
      if (state.device?.hid === event.device) {
        state.device = null;
        state.snapshot = null;
        if (state.watching) toggleWatch();
        report("鼠标已断开。草稿保留，重新连接后可继续写入。", true);
        render();
      }
    });
    // Connect a mouse this site may already use, as the desktop tool
    // connects at start.
    const [paired] = await hid.pairedDevices();
    if (paired) connect(paired);
  }
}

main().catch((error) => report(message(error), true));
