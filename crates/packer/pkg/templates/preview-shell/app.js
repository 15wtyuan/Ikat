// loom preview shell 逻辑（ESM，无构建）。
// 保真语义（grilling 定案）：iframe 永远按工作区设计分辨率渲染，外壳只按
// match_mode 缩放——切设备框不触发 reflow，预览必须预测运行时。
"use strict";

const LS = {
  preset: "loomPreview.preset",
  safe: "loomPreview.safeArea",
  fit: "loomPreview.fitWindow",
  custom: "loomPreview.customResolutions",
};

// 内置设备清单：W/H + 安全区参考线（top/bottom，px；0 = 无）。
// 参考线是纯视觉提示，core 无安全区概念（真支持另有 issue 跟踪）。
const BUILTIN_PRESETS = [
  { id: "design",     name: "设计分辨率", w: 0,  h: 0,  safe: { top: 0,  bottom: 0 } },
  { id: "iphone-se",  name: "iPhone SE",  w: 375,  h: 664,  safe: { top: 0,  bottom: 0 } },
  { id: "iphone-14",  name: "iPhone 14",  w: 390,  h: 844,  safe: { top: 47, bottom: 34 } },
  { id: "iphone-16pm",name: "iPhone 16 PM", w: 440, h: 956,  safe: { top: 62, bottom: 34 } },
  { id: "pixel-8",    name: "Pixel 8",    w: 412,  h: 915,  safe: { top: 24, bottom: 16 } },
  { id: "galaxy-s23", name: "Galaxy S23", w: 360,  h: 780,  safe: { top: 24, bottom: 16 } },
  { id: "ipad-mini",  name: "iPad mini",  w: 744,  h: 1133, safe: { top: 24, bottom: 20 } },
  { id: "hd",         name: "1280×720",   w: 1280, h: 720,  safe: { top: 0,  bottom: 0 } },
  { id: "fhd",        name: "1920×1080",  w: 1920, h: 1080, safe: { top: 0,  bottom: 0 } },
];

const $ = (id) => document.getElementById(id);
const state = {
  api: null,          // /api/workspace.json
  design: { w: 1920, h: 1080 },
  matchMode: "letterbox",
  preset: null,       // 当前生效 {name,w,h,safe,custom?}
  selectedPage: null, // {pkg, name, url}
};

function loadCustom() {
  try { return JSON.parse(localStorage.getItem(LS.custom) || "[]"); }
  catch { return []; }
}
function saveCustom(list) { localStorage.setItem(LS.custom, JSON.stringify(list)); }

function allPresets() {
  const design = { id: "design", name: `设计 ${state.design.w}×${state.design.h}`,
                   w: state.design.w, h: state.design.h, safe: { top: 0, bottom: 0 } };
  return [design, ...BUILTIN_PRESETS.slice(1), ...loadCustom()];
}

// ---------------------------------------------------------------------------
// 初始化
// ---------------------------------------------------------------------------
async function init() {
  const api = await fetch("/api/workspace.json").then((r) => r.json());
  state.api = api;
  if (Array.isArray(api.design) && api.design.length === 2) {
    state.design = { w: api.design[0], h: api.design[1] };
  }
  state.matchMode = api.match_mode || "letterbox";

  $("ws-design").textContent = `${state.design.w}×${state.design.h}`;
  $("ws-match").textContent = state.matchMode;

  buildTree();
  buildPresetSelect();
  bindControls();

  // hash 深链（#pkg/page）优先，否则第一个包第一页。
  const fromHash = pageFromHash();
  selectPage(fromHash || firstPage(), { replaceHash: !fromHash });
  applyPreset(savedPresetId());
}

function firstPage() {
  const pkg = state.api.packages[0];
  return pkg && pkg.pages[0] ? { pkg: pkg.name, ...pkg.pages[0] } : null;
}
function pageFromHash() {
  const h = decodeURIComponent(location.hash.slice(1)).replace(/^\//, "");
  if (!h) return null;
  const [pkgName, pageName] = h.split("/");
  for (const pkg of state.api.packages) {
    if (pkg.name !== pkgName) continue;
    const p = pkg.pages.find((x) => x.name === pageName);
    if (p) return { pkg: pkg.name, ...p };
  }
  return null;
}
function pageToHash(pg) {
  if (pg) location.hash = `/${pg.pkg}/${pg.name}`; // 前导 / 防首段被吃
}

// ---------------------------------------------------------------------------
// 左树
// ---------------------------------------------------------------------------
function buildTree() {
  const tree = $("tree");
  tree.textContent = "";
  for (const pkg of state.api.packages) {
    const h = document.createElement("div");
    h.className = "pkg";
    h.textContent = pkg.name;
    tree.appendChild(h);
    for (const page of pkg.pages) {
      const row = document.createElement("div");
      row.className = "page";
      row.dataset.key = `${pkg.name}/${page.name}`;
      const dot = document.createElement("span");
      dot.className = "sim-dot" + (page.has_sim ? "" : " off");
      dot.title = page.has_sim ? "有预览模拟脚本" : "无预览模拟脚本";
      row.appendChild(dot);
      row.appendChild(document.createTextNode(page.name));
      row.addEventListener("click", () => selectPage({ pkg: pkg.name, ...page }));
      tree.appendChild(row);
    }
  }
}

function selectPage(pg, opts = {}) {
  if (!pg) return;
  state.selectedPage = pg;
  for (const row of $("tree").querySelectorAll(".page")) {
    row.classList.toggle("sel", row.dataset.key === `${pg.pkg}/${pg.name}`);
  }
  $("frame").src = pg.url;
  if (!opts.replaceHash) pageToHash(pg);
}

// ---------------------------------------------------------------------------
// 分辨率 / 缩放（match_mode 语义，镜像运行时；绝不 reflow）
// ---------------------------------------------------------------------------
function buildPresetSelect() {
  const sel = $("preset");
  sel.textContent = "";
  for (const p of allPresets()) {
    const o = document.createElement("option");
    o.value = p.id;
    o.textContent = p.custom ? `${p.name}（自定义）` : p.name;
    sel.appendChild(o);
  }
  const custom = document.createElement("option");
  custom.value = "_custom";
  custom.textContent = "自定义…";
  sel.appendChild(custom);
}

function savedPresetId() {
  const id = localStorage.getItem(LS.preset);
  return id && allPresets().some((p) => p.id === id) ? id : "design";
}

function applyPreset(id) {
  let p = allPresets().find((x) => x.id === id);
  if (!p) p = allPresets()[0];
  state.preset = p;
  localStorage.setItem(LS.preset, p.id);
  if (p.id !== "_custom") $("preset").value = p.id;
  layoutDevice();
}

function layoutDevice() {
  const p = state.preset || allPresets()[0];
  const { w: dw, h: dh } = p.id === "design" ? state.design : p;
  const frame = $("frame"), device = $("device"), scaler = $("device-scale");

  // match_mode 缩放：letterbox=contain（min）；fit-width/fit-height 单轴贴满。
  const sx = dw / state.design.w, sy = dh / state.design.h;
  const scale = state.matchMode === "fit-width" ? sx
    : state.matchMode === "fit-height" ? sy
    : Math.min(sx, sy);

  // 观察级缩放（适应窗口）：整个设备框等比缩进 stage 视口，上限 1（不放大——
  // 放大会糊像素）。transform 不触发 iframe reflow，页内仍按设计分辨率渲染，
  // 保真语义不变；关闭则回 1:1（像素级检查，stage 恢复滚动）。
  const stage = $("stage");
  const PAD = 24; // #stage padding（style.css）
  const fit = $("fit-window").checked
    ? Math.min((stage.clientWidth - PAD * 2) / dw, (stage.clientHeight - PAD * 2) / dh, 1)
    : 1;

  device.style.width = Math.round(dw * fit) + "px";
  device.style.height = Math.round(dh * fit) + "px";
  scaler.style.width = dw + "px";
  scaler.style.height = dh + "px";
  scaler.style.transform = `scale(${fit})`;
  frame.style.width = state.design.w + "px";
  frame.style.height = state.design.h + "px";
  frame.style.transform = `scale(${scale})`;

  // letterbox 单轴贴满时另一轴居中（黑边）。
  const rx = state.design.w * scale, ry = state.design.h * scale;
  frame.style.left = Math.max(0, (dw - rx) / 2) + "px";
  frame.style.top = Math.max(0, (dh - ry) / 2) + "px";

  $("zoom").textContent = Math.round(scale * 100) + "%";

  // 安全区参考线：开关 + 当前设备数据。
  const safeOn = $("safe-area").checked;
  const s = p.safe || { top: 0, bottom: 0 };
  $("safe-top").style.height = s.top + "px";
  $("safe-bottom").style.height = s.bottom + "px";
  $("safe-top").classList.toggle("has", safeOn && s.top > 0);
  $("safe-bottom").classList.toggle("has", safeOn && s.bottom > 0);
}

// ---------------------------------------------------------------------------
// 事件
// ---------------------------------------------------------------------------
function bindControls() {
  $("preset").addEventListener("change", (e) => {
    if (e.target.value === "_custom") return;
    applyPreset(e.target.value);
  });
  $("custom-go").addEventListener("click", () => {
    const w = parseInt($("custom-w").value, 10), h = parseInt($("custom-h").value, 10);
    if (!(w >= 100 && h >= 100)) return;
    state.preset = { id: "_custom", name: `${w}×${h}`, w, h, safe: { top: 0, bottom: 0 } };
    $("preset").value = "_custom";
    localStorage.setItem(LS.preset, "design"); // 自定义不持久为默认
    layoutDevice();
  });
  const safeBox = $("safe-area");
  safeBox.checked = localStorage.getItem(LS.safe) === "1";
  safeBox.addEventListener("change", () => {
    localStorage.setItem(LS.safe, safeBox.checked ? "1" : "0");
    layoutDevice();
  });
  const fitBox = $("fit-window");
  fitBox.checked = localStorage.getItem(LS.fit) !== "0"; // 默认开
  fitBox.addEventListener("change", () => {
    localStorage.setItem(LS.fit, fitBox.checked ? "1" : "0");
    layoutDevice();
  });
  $("reload").addEventListener("click", () => {
    if ($("frame").src) $("frame").contentWindow.location.reload();
  });
  $("settings").addEventListener("click", () => {
    renderResList();
    $("settings-box").showModal();
  });
  $("close-settings").addEventListener("click", () => $("settings-box").close());
  $("add-res").addEventListener("click", () => {
    const name = $("new-name").value.trim() || "自定义设备";
    const w = parseInt($("new-w").value, 10), h = parseInt($("new-h").value, 10);
    if (!(w >= 100 && h >= 100)) return;
    const list = loadCustom();
    const safe = $("new-safe").checked ? { top: 24, bottom: 16 } : { top: 0, bottom: 0 };
    list.push({ id: `c${Date.now()}`, name, w, h, safe, custom: true });
    saveCustom(list);
    buildPresetSelect();
    renderResList();
    $("new-name").value = $("new-w").value = $("new-h").value = "";
  });
  window.addEventListener("hashchange", () => {
    const pg = pageFromHash();
    if (pg && (!state.selectedPage || `${pg.pkg}/${pg.name}` !== `${state.selectedPage.pkg}/${state.selectedPage.name}`)) {
      selectPage(pg, { replaceHash: true });
    }
  });
  window.addEventListener("resize", layoutDevice);
}

function renderResList() {
  const box = $("res-list");
  box.textContent = "";
  const list = loadCustom();
  if (!list.length) {
    box.innerHTML = '<p class="hint">暂无自定义分辨率。</p>';
    return;
  }
  for (const r of list) {
    const row = document.createElement("div");
    row.className = "res-row";
    row.textContent = `${r.name}  ${r.w}×${r.h}`;
    const del = document.createElement("button");
    del.className = "del";
    del.textContent = "删除";
    del.addEventListener("click", () => {
      saveCustom(loadCustom().filter((x) => x.id !== r.id));
      buildPresetSelect();
      renderResList();
    });
    row.appendChild(del);
    box.appendChild(row);
  }
}

init();
