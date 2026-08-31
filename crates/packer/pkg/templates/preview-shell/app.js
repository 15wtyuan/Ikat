// ikat preview shell 逻辑（ESM，无构建）。
// 保真语义（grilling 定案）：iframe 永远按工作区设计分辨率渲染，外壳只按
// match_mode 缩放——切设备框不触发 reflow，预览必须预测运行时。
"use strict";

const LS = {
  preset: "ikatPreview.preset",
  match: "ikatPreview.matchMode",
  safe: "ikatPreview.safeArea",
  fit: "ikatPreview.fitWindow",
  custom: "ikatPreview.customResolutions",
};

// 内置设备清单：W/H + 安全区四边（px；0 = 无）。safe 值与 env(safe-area-inset-*)
// 模拟同源（layoutDevice 换算成 design px 注进 iframe CSS 变量），参考线可视化
// 同一份数据——预览与运行时同公式（core ikat_stage_set_safe_area）。
const BUILTIN_PRESETS = [
  { id: "design",     name: "设计分辨率", w: 0,  h: 0,  safe: { top: 0, bottom: 0, left: 0, right: 0 } },
  { id: "iphone-se",  name: "iPhone SE",  w: 375,  h: 664,  safe: { top: 0, bottom: 0, left: 0, right: 0 } },
  { id: "iphone-14",  name: "iPhone 14",  w: 390,  h: 844,  safe: { top: 47, bottom: 34, left: 0, right: 0 } },
  { id: "iphone-16pm",name: "iPhone 16 PM", w: 440, h: 956,  safe: { top: 62, bottom: 34, left: 0, right: 0 } },
  { id: "pixel-8",    name: "Pixel 8",    w: 412,  h: 915,  safe: { top: 24, bottom: 16, left: 0, right: 0 } },
  { id: "galaxy-s23", name: "Galaxy S23", w: 360,  h: 780,  safe: { top: 24, bottom: 16, left: 0, right: 0 } },
  { id: "ipad-mini",  name: "iPad mini",  w: 744,  h: 1133, safe: { top: 24, bottom: 20, left: 0, right: 0 } },
  { id: "r43",        name: "4:3 (1024×768)", w: 1024, h: 768, safe: { top: 0, bottom: 0, left: 0, right: 0 } },
  { id: "u21x9",      name: "21:9 (2560×1080)", w: 2560, h: 1080, safe: { top: 0, bottom: 0, left: 0, right: 0 } },
  { id: "hd",         name: "1280×720",   w: 1280, h: 720,  safe: { top: 0, bottom: 0, left: 0, right: 0 } },
  { id: "fhd",        name: "1920×1080",  w: 1920, h: 1080, safe: { top: 0, bottom: 0, left: 0, right: 0 } },
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
                   w: state.design.w, h: state.design.h, safe: { top: 0, bottom: 0, left: 0, right: 0 } };
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
  state.matchMode = localStorage.getItem(LS.match) || api.match_mode || "letterbox";

  $("ws-design").textContent = `${state.design.w}×${state.design.h}`;
  $("ws-match").textContent = state.matchMode;
  $("match").value = state.matchMode;

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
  const safe = Object.assign({ top: 0, bottom: 0, left: 0, right: 0 }, p.safe);
  // safe 内容框（letterbox 的 contain 框，镜像 core：letterbox 以 safe 矩形为框）。
  const availW = dw - safe.left - safe.right, availH = dh - safe.top - safe.bottom;

  // match_mode 缩放 + root 尺寸（镜像 core adapt::compute）：letterbox=contain
  // （min，以 safe 框为界），root=设计分辨率；fit-width/fit-height 单轴贴满
  // （fit 贴物理边，safe 不缩基座——避让走 env()），另一维 root 取设备换算值
  // ——iframe 按 root 尺寸渲染，切设备框触发真实 reflow（运行时 fit 模式如此，
  // vw/vh 分母跟随），而非 letterbox 的「锁设计分辨率不 reflow」。
  const sx = dw / state.design.w, sy = dh / state.design.h;
  const scale = state.matchMode === "fit-width" ? sx
    : state.matchMode === "fit-height" ? sy
    : Math.min(availW / state.design.w, availH / state.design.h);
  const rootW = state.matchMode === "fit-height" ? dw / scale : state.design.w;
  const rootH = state.matchMode === "fit-width" ? dh / scale : state.design.h;

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
  frame.style.width = rootW + "px";
  frame.style.height = rootH + "px";
  frame.style.transform = `scale(${scale})`;

  // letterbox 在 safe 框内居中（黑边；镜像 core 的 safe contain）；fit 贴设备框原点。
  const rx = rootW * scale, ry = rootH * scale;
  const rootL = state.matchMode === "letterbox" ? safe.left + Math.max(0, (availW - rx) / 2) : 0;
  const rootT = state.matchMode === "letterbox" ? safe.top + Math.max(0, (availH - ry) / 2) : 0;
  frame.style.left = rootL + "px";
  frame.style.top = rootT + "px";

  $("zoom").textContent = Math.round(scale * 100) + "%";

  // 安全区参考线：开关 + 当前设备数据（四向）。
  const safeOn = $("safe-area").checked;
  $("safe-top").style.height = safe.top + "px";
  $("safe-bottom").style.height = safe.bottom + "px";
  $("safe-left").style.width = safe.left + "px";
  $("safe-right").style.width = safe.right + "px";
  $("safe-top").classList.toggle("has", safeOn && safe.top > 0);
  $("safe-bottom").classList.toggle("has", safeOn && safe.bottom > 0);
  $("safe-left").classList.toggle("has", safeOn && safe.left > 0);
  $("safe-right").classList.toggle("has", safeOn && safe.right > 0);

  // env(safe-area-inset-*) 模拟：与 core ikat_stage_set_safe_area 同公式——
  // root（iframe 映射进设备框的矩形）伸进 unsafe 区的深度 / scale = design px。
  // letterbox root 在 safe 框内 → 恒 0（黑边已让位）；fit 贴物理边 → 真实 inset。
  // server 已把 env() 改写成 var(--ikat-safe-*)，这里注值即生效（与参考线同源）。
  const ins = {
    top: Math.max(0, safe.top - rootT) / scale,
    right: Math.max(0, (rootL + rx) - (dw - safe.right)) / scale,
    bottom: Math.max(0, (rootT + ry) - (dh - safe.bottom)) / scale,
    left: Math.max(0, safe.left - rootL) / scale,
  };
  const doc = frame.contentDocument;
  if (doc && doc.documentElement) {
    const rs = doc.documentElement.style;
    rs.setProperty("--ikat-safe-top", ins.top + "px");
    rs.setProperty("--ikat-safe-right", ins.right + "px");
    rs.setProperty("--ikat-safe-bottom", ins.bottom + "px");
    rs.setProperty("--ikat-safe-left", ins.left + "px");
  }
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
    state.preset = { id: "_custom", name: `${w}×${h}`, w, h, safe: { top: 0, bottom: 0, left: 0, right: 0 } };
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
    const safe = $("new-safe").checked ? { top: 24, bottom: 16, left: 0, right: 0 } : { top: 0, bottom: 0, left: 0, right: 0 };
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
  $("match").addEventListener("change", (e) => {
    state.matchMode = e.target.value;
    localStorage.setItem(LS.match, state.matchMode);
    $("ws-match").textContent = state.matchMode;
    layoutDevice();
  });
  window.addEventListener("resize", layoutDevice);
  $("frame").addEventListener("load", () => layoutDevice());
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
