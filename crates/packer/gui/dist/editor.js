// LoomGUI Packer — Main Editor
// Tab layout: general / packages / atlases / fonts. Build log in modal.
// Drag-to-create via Tauri 2 onDragDropEvent; directory re-pick via dialog plugin.
(function () {
  "use strict";

  var invoke = window.__TAURI__ ? window.__TAURI__.core.invoke : null;
  var $ = function (id) { return document.getElementById(id); };

  var ws = null;
  var wsPath = null;
  var autoHtmlCache = {};
  var editorInited = false;

  // ── DOM helpers ──
  function el(tag, className, textContent) {
    var e = document.createElement(tag);
    if (className) e.className = className;
    if (textContent !== undefined) e.textContent = textContent;
    return e;
  }
  function inputEl(type, value, className, placeholder) {
    var inp = document.createElement("input");
    inp.type = type;
    inp.className = className || "form-input";
    if (value !== undefined && value !== null) inp.value = value;
    if (placeholder) inp.placeholder = placeholder;
    return inp;
  }

  // ── Native pickers (tauri-plugin-dialog) ──
  function pickDirectory(title) {
    return invoke("plugin:dialog|open", {
      options: { directory: true, multiple: false, title: title || "选择目录" },
    }).then(function (r) { return r || null; });
  }
  function pickFile(title) {
    return invoke("plugin:dialog|open", {
      options: { directory: false, multiple: false, title: title || "选择文件" },
    }).then(function (r) { return r || null; });
  }
  function relativizePath(absPath) {
    return invoke("relativize", { root: wsPath, abs: absPath });
  }

  // ── Save ──
  function collectWorkspace() {
    var packages = [];
    (ws.packages || []).forEach(function (pkg, i) {
      var nameEl = $("pkg-name-" + i);
      packages.push({
        name: nameEl ? nameEl.value.trim() : pkg.name,
        dirs: readTagList($("pkg-dirs-" + i)),
        html: readTagList($("pkg-html-" + i)),
      });
    });
    var atlases = [];
    (ws.atlases || []).forEach(function (atlas, i) {
      var aname = $("atlas-name-" + i);
      atlases.push({
        name: aname ? aname.value.trim() : atlas.name,
        standalone: $("atlas-standalone-" + i) ? $("atlas-standalone-" + i).checked : false,
        dirs: readTagList($("atlas-dirs-" + i)),
        max_size: parseInt(($("atlas-max-size-" + i) || {}).value, 10) || 2048,
        padding: parseInt(($("atlas-padding-" + i) || {}).value, 10) || 4,
      });
    });
    var fonts = [];
    (ws.fonts || []).forEach(function (font, i) {
      var ffam = $("font-family-" + i);
      var ffile = $("font-file-" + i);
      fonts.push({
        family: ffam ? ffam.value.trim() : font.family,
        file: ffile ? ffile.value.trim() : font.file,
        default: $("font-default-" + i) ? $("font-default-" + i).checked : false,
        fallback: $("font-fallback-" + i) ? $("font-fallback-" + i).checked : false,
      });
    });
    return {
      version: ws.version || 1,
      output_dir: $("ws-output-dir") ? $("ws-output-dir").value.trim() : "",
      packages: packages,
      atlases: atlases,
      fonts: fonts,
    };
  }
  var saveTimer = null;
  function saveIfLoaded() {
    if (!wsPath) return;
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(function () {
      ws = collectWorkspace();
      invoke("save_workspace", { path: wsPath, ws: ws }).catch(function (err) {
        console.error("save_workspace failed:", err);
      });
    }, 400);
  }
  function readTagList(container) {
    if (!container) return [];
    var result = [];
    var children = container.querySelectorAll(".tag:not(.auto)[data-value]");
    for (var i = 0; i < children.length; i++) {
      var v = children[i].getAttribute("data-value");
      if (v !== null) result.push(v);
    }
    return result;
  }

  // ── Tab switching ──
  function setupTabs() {
    document.querySelectorAll(".tab").forEach(function (tab) {
      tab.addEventListener("click", function () {
        var name = tab.getAttribute("data-tab");
        document.querySelectorAll(".tab").forEach(function (t) { t.classList.remove("active"); });
        document.querySelectorAll(".tab-panel").forEach(function (p) { p.classList.remove("active"); });
        tab.classList.add("active");
        $("panel-" + name).classList.add("active");
      });
    });
  }

  // ── 1. General section ──
  function renderGeneral() {
    var body = $("section-general-body");
    body.innerHTML = "";

    // 输出目录：[label][input][+拖入][打开]
    var row = el("div", "form-row");
    row.appendChild(el("span", "form-label", "输出目录"));
    var inp = inputEl("text", ws.output_dir || "", "form-input", "例如 ../loomgui_unity/Assets/Bundles");
    inp.id = "ws-output-dir";
    inp.addEventListener("input", function () { saveIfLoaded(); });
    inp.addEventListener("keydown", function (e) { if (e.key === "Enter") inp.blur(); });
    row.appendChild(inp);
    var dropDir = el("span", "dropzone-inline", "+ 拖入");
    dropDir.setAttribute("data-dropzone", "output-dir");
    row.appendChild(dropDir);
    var openBtn = el("button", "btn btn-ghost btn-sm", "打开");
    openBtn.addEventListener("click", async function () {
      var path = await pickDirectory("选择输出目录");
      if (!path) return;
      inp.value = await relativizePath(path);
      saveIfLoaded();
    });
    row.appendChild(openBtn);
    body.appendChild(row);

    // 初始化工作区
    var initRow = el("div", "form-row");
    var initBtn = el("button", "btn btn-secondary", "初始化工作区");
    initBtn.title = "按勾选的 agent 覆盖拷贝指令文档 + loomgui-editor skill 到工作区（不碰 workspace.json 和源文件）";
    initBtn.addEventListener("click", function () {
      openInitModal(function (agents) {
        initBtn.disabled = true;
        initBtn.textContent = "初始化中...";
        invoke("init_workspace", { path: wsPath, agents: agents })
          .then(function () { alert("脚手架已更新"); })
          .catch(function (err) { alert("初始化失败: " + err); })
          .finally(function () { initBtn.disabled = false; initBtn.textContent = "初始化工作区"; });
      });
    });
    initRow.appendChild(initBtn);
    body.appendChild(initRow);
  }

  // ── Init agent scaffold modal (multi-select) ──
  var initConfirmCb = null;
  function setupInitModal() {
    var overlay = $("init-overlay");
    var okBtn = $("btn-init-ok");
    var cancelBtn = $("btn-init-cancel");
    var claudeCb = $("init-check-claude");
    var agentsCb = $("init-check-agents");

    function selectedAgents() {
      var list = [];
      if (claudeCb.checked) list.push("claude");
      if (agentsCb.checked) list.push("agents");
      return list;
    }
    function refreshOk() { okBtn.disabled = selectedAgents().length === 0; }
    function close() {
      overlay.classList.add("hidden");
      initConfirmCb = null;
    }

    claudeCb.addEventListener("change", refreshOk);
    agentsCb.addEventListener("change", refreshOk);
    okBtn.addEventListener("click", function () {
      var agents = selectedAgents();
      if (!agents.length) return;
      var cb = initConfirmCb;
      close();
      if (cb) cb(agents);
    });
    cancelBtn.addEventListener("click", close);
    overlay.addEventListener("click", function (e) {
      if (e.target === overlay) close();
    });
  }
  function openInitModal(onConfirm) {
    initConfirmCb = onConfirm;
    $("init-check-claude").checked = true;
    $("init-check-agents").checked = true;
    $("btn-init-ok").disabled = false;
    $("init-overlay").classList.remove("hidden");
  }

  // ── 2. Packages section ──
  function renderPackages() {
    var body = $("section-pkg-body");
    body.innerHTML = "";
    if (!ws.packages) ws.packages = [];

    ws.packages.forEach(function (pkg, i) {
      var card = el("div", "config-card");

      // name row: [name][+目录][删除]
      var nameRow = el("div", "form-row");
      nameRow.appendChild(el("span", "form-label", "名称"));
      var nameInp = inputEl("text", pkg.name, "form-input");
      nameInp.id = "pkg-name-" + i;
      nameInp.addEventListener("input", function () { saveIfLoaded(); });
      nameInp.addEventListener("keydown", function (e) { if (e.key === "Enter") nameInp.blur(); });
      nameRow.appendChild(nameInp);
      nameRow.appendChild(makeAddDirButton("选择源目录", i, "packages", renderPackages));
      nameRow.appendChild(makeDeleteButton(i, "packages", renderPackages));
      card.appendChild(nameRow);

      card.appendChild(el("div", "form-label", "源目录"));
      card.appendChild(makeDirsList(pkg.dirs, "pkg-dirs-" + i, "pkg-dirs", i, "packages", renderPackages));

      card.appendChild(el("div", "form-label", "HTML 文件"));
      card.appendChild(makeHtmlList(pkg, i));
      body.appendChild(card);
    });

    var dz = el("div", "dropzone", "拖入目录建包");
    dz.setAttribute("data-dropzone", "new-package");
    body.appendChild(dz);
  }

  // ── 3. Atlases section ──
  function renderAtlases() {
    var body = $("section-atlas-body");
    body.innerHTML = "";
    if (!ws.atlases) ws.atlases = [];

    ws.atlases.forEach(function (atlas, i) {
      var card = el("div", "config-card");

      var nameRow = el("div", "form-row");
      nameRow.appendChild(el("span", "form-label", "名称"));
      var nameInp = inputEl("text", atlas.name, "form-input");
      nameInp.id = "atlas-name-" + i;
      nameInp.addEventListener("input", function () { saveIfLoaded(); });
      nameInp.addEventListener("keydown", function (e) { if (e.key === "Enter") nameInp.blur(); });
      nameRow.appendChild(nameInp);
      nameRow.appendChild(makeAddDirButton("选择资源目录", i, "atlases", renderAtlases));
      nameRow.appendChild(makeDeleteButton(i, "atlases", renderAtlases));
      card.appendChild(nameRow);

      // checkboxes
      var checksRow = el("div", "card-checks");
      checksRow.appendChild(makeCheckbox("atlas-standalone-" + i, " 独立单页", !!atlas.standalone, function () { saveIfLoaded(); }));
      card.appendChild(checksRow);

      card.appendChild(el("div", "form-label", "资源目录"));
      card.appendChild(makeDirsList(atlas.dirs, "atlas-dirs-" + i, "atlas-dirs", i, "atlases", renderAtlases));

      var sizeRow = el("div", "form-row");
      sizeRow.appendChild(el("span", "form-label", "最大尺寸"));
      var msInp = inputEl("number", String(atlas.max_size || 2048), "form-input form-input-number");
      msInp.id = "atlas-max-size-" + i; msInp.min = "64"; msInp.step = "64";
      msInp.addEventListener("change", function () { saveIfLoaded(); });
      sizeRow.appendChild(msInp);
      sizeRow.appendChild(el("span", "form-label", "间距"));
      var padInp = inputEl("number", String(atlas.padding || 4), "form-input form-input-number");
      padInp.id = "atlas-padding-" + i; padInp.min = "0"; padInp.max = "32";
      padInp.addEventListener("change", function () { saveIfLoaded(); });
      sizeRow.appendChild(padInp);
      card.appendChild(sizeRow);
      body.appendChild(card);
    });

    var dz = el("div", "dropzone", "拖入目录建图集");
    dz.setAttribute("data-dropzone", "new-atlas");
    body.appendChild(dz);
  }

  // ── 4. Fonts section ──
  function renderFonts() {
    var body = $("section-font-body");
    body.innerHTML = "";
    if (!ws.fonts) ws.fonts = [];

    ws.fonts.forEach(function (font, i) {
      var card = el("div", "config-card compact");

      var famRow = el("div", "form-row");
      var upBtn = el("button", "btn btn-secondary btn-sm", "↑");
      upBtn.disabled = (i === 0);
      upBtn.title = "上移";
      upBtn.addEventListener("click", (function (idx) { return function () { if (idx === 0) return; var m = ws.fonts.splice(idx, 1)[0]; ws.fonts.splice(idx - 1, 0, m); saveIfLoaded(); animateRender(renderFonts); }; })(i));
      famRow.appendChild(upBtn);
      var downBtn = el("button", "btn btn-secondary btn-sm", "↓");
      downBtn.disabled = (i === ws.fonts.length - 1);
      downBtn.title = "下移";
      downBtn.addEventListener("click", (function (idx) { return function () { if (idx >= ws.fonts.length - 1) return; var m = ws.fonts.splice(idx, 1)[0]; ws.fonts.splice(idx + 1, 0, m); saveIfLoaded(); animateRender(renderFonts); }; })(i));
      famRow.appendChild(downBtn);
      famRow.appendChild(el("span", "form-label", "字体族"));
      var famInp = inputEl("text", font.family, "form-input");
      famInp.id = "font-family-" + i;
      famInp.addEventListener("input", function () { saveIfLoaded(); });
      famInp.addEventListener("keydown", function (e) { if (e.key === "Enter") famInp.blur(); });
      famRow.appendChild(famInp);
      famRow.appendChild(makeDeleteButton(i, "fonts", renderFonts));
      card.appendChild(famRow);

      // file row: [file][+拖入][打开]
      var fileRow = el("div", "form-row");
      fileRow.appendChild(el("span", "form-label", "文件"));
      var fileInp = inputEl("text", font.file, "form-input");
      fileInp.id = "font-file-" + i;
      fileInp.addEventListener("input", function () { saveIfLoaded(); });
      fileInp.addEventListener("keydown", function (e) { if (e.key === "Enter") fileInp.blur(); });
      fileRow.appendChild(fileInp);
      var dropFile = el("span", "dropzone-inline", "+ 拖入");
      dropFile.setAttribute("data-dropzone", "font-file");
      dropFile.setAttribute("data-dropzone-idx", String(i));
      fileRow.appendChild(dropFile);
      var openBtn = el("button", "btn btn-ghost btn-sm", "打开");
      openBtn.addEventListener("click", (function (idx) {
        return async function () {
          var path = await pickFile("选择字体文件");
          if (!path) return;
          var rel = await relativizePath(path);
          ws.fonts[idx].file = rel;
          if (!ws.fonts[idx].family) {
            var base = rel.replace(/^.*[\\/]/, "");
            ws.fonts[idx].family = base.replace(/\.[^.]+$/, "");
          }
          saveIfLoaded();
          renderFonts();
        };
      })(i));
      fileRow.appendChild(openBtn);
      card.appendChild(fileRow);

      // default (radio) + fallback
      var checksRow = el("div", "card-checks");
      var defCb = el("label", "form-check");
      var defInp = inputEl("radio", "", "");
      defInp.id = "font-default-" + i;
      defInp.name = "font-default-group";
      defInp.checked = !!font.default;
      defInp.addEventListener("change", function () {
        for (var j = 0; j < ws.fonts.length; j++) ws.fonts[j].default = false;
        ws.fonts[i].default = true;
        saveIfLoaded();
        renderFonts();
      });
      defCb.appendChild(defInp);
      defCb.appendChild(document.createTextNode(" 默认"));
      checksRow.appendChild(defCb);
      checksRow.appendChild(makeCheckbox("font-fallback-" + i, " 后备", !!font.fallback, function () { saveIfLoaded(); }));
      card.appendChild(checksRow);
      body.appendChild(card);
    });

    var dz = el("div", "dropzone", "拖入字体文件添加");
    dz.setAttribute("data-dropzone", "new-font");
    body.appendChild(dz);
  }

  // ── Shared card builders ──
  function makeCheckbox(id, label, checked, onChange) {
    var cb = el("label", "form-check");
    var inp = inputEl("checkbox", "", "");
    inp.id = id; inp.checked = checked;
    inp.addEventListener("change", onChange);
    cb.appendChild(inp);
    cb.appendChild(document.createTextNode(label));
    return cb;
  }
  function makeDeleteButton(idx, collection, rerender) {
    var btn = el("button", "btn-danger-sm", "删除");
    btn.addEventListener("click", function () {
      ws[collection].splice(idx, 1);
      if (collection === "packages") delete autoHtmlCache[ws[collection] && ws[collection][idx] && ws[collection][idx].name];
      rerender();
      saveIfLoaded();
    });
    return btn;
  }
  function makeAddDirButton(title, idx, collection, rerender) {
    var btn = el("button", "btn btn-ghost btn-sm", "+ 目录");
    btn.addEventListener("click", async function () {
      var path = await pickDirectory(title);
      if (!path) return;
      var rel = await relativizePath(path);
      ws[collection][idx].dirs = (ws[collection][idx].dirs || []).concat([rel]);
      saveIfLoaded();
      if (collection === "packages") refreshAutoScans();
      rerender();
    });
    return btn;
  }
  function makeDirsList(dirs, listId, dropType, idx, collection, rerender) {
    var list = el("div", "tag-list");
    list.id = listId;
    (dirs || []).forEach(function (d, di) {
      var tag = el("span", "tag");
      tag.setAttribute("data-value", d);
      tag.textContent = d;
      if (collection === "atlases") {
        var thumbBtn = el("button", "tag-action", "🖼");
        thumbBtn.title = "查看图片";
        thumbBtn.addEventListener("click", function (e) {
          e.stopPropagation();
          toggleThumbs(tag, d);
        });
        tag.appendChild(thumbBtn);
      }
      var rm = el("button", "tag-remove", "×");
      rm.addEventListener("click", function () {
        ws[collection][idx].dirs.splice(di, 1);
        if (collection === "packages") refreshAutoScans();
        rerender();
        saveIfLoaded();
      });
      tag.appendChild(rm);
      list.appendChild(tag);
    });
    var drop = el("span", "dropzone-inline", "+ 拖入目录");
    drop.setAttribute("data-dropzone", dropType);
    drop.setAttribute("data-dropzone-idx", String(idx));
    list.appendChild(drop);
    return list;
  }

  function toggleThumbs(tag, dir) {
    if (tag.__thumbGrid) { tag.__thumbGrid.remove(); tag.__thumbGrid = null; return; }
    var grid = el("div", "thumb-grid");
    grid.textContent = "加载中...";
    tag.parentNode.appendChild(grid);
    tag.__thumbGrid = grid;
    var scanPath = wsPath + "/" + dir;
    invoke("scan_pngs", { pkgDir: scanPath })
      .then(function (pngs) {
        grid.textContent = "";
        if (!pngs.length) { grid.textContent = "无 png"; return; }
        pngs.forEach(function (png) {
          var box = el("div", "thumb-box");
          var img = document.createElement("img");
          img.src = window.__TAURI__.core.convertFileSrc(scanPath + "/" + png);
          img.className = "thumb-img";
          img.title = png;
          box.appendChild(img);
          box.appendChild(el("div", "thumb-name", png));
          grid.appendChild(box);
        });
      })
      .catch(function (err) { grid.textContent = "扫描失败: " + err; });
  }
  function makeHtmlList(pkg, i) {
    var list = el("div", "tag-list");
    list.id = "pkg-html-" + i;
    if (!pkg.html || pkg.html.length === 0) {
      var autoFiles = autoHtmlCache[pkg.name];
      if (autoFiles === undefined) {
        list.appendChild(el("span", "auto-scan-status", "扫描中..."));
      } else if (autoFiles.length === 0) {
        list.appendChild(el("span", "auto-scan-status", "未找到 .html 文件"));
      } else {
        autoFiles.forEach(function (f) {
          var tag = el("span", "tag auto");
          tag.setAttribute("data-value", f);
          tag.textContent = f;
          list.appendChild(tag);
        });
      }
      var lockBtn = el("button", "btn-link-sm", "手动指定");
      lockBtn.addEventListener("click", function () {
        ws.packages[i].html = (autoHtmlCache[ws.packages[i].name] || []).slice();
        saveIfLoaded();
        renderPackages();
      });
      list.appendChild(lockBtn);
    } else {
      pkg.html.forEach(function (h, hi) {
        var tag = el("span", "tag");
        tag.setAttribute("data-value", h);
        tag.textContent = h;
        var rm = el("button", "tag-remove", "×");
        rm.addEventListener("click", function () {
          ws.packages[i].html.splice(hi, 1);
          renderPackages();
          saveIfLoaded();
        });
        tag.appendChild(rm);
        list.appendChild(tag);
      });
      var autoBtn = el("button", "btn-link-sm", "恢复自动扫");
      autoBtn.addEventListener("click", function () {
        ws.packages[i].html = [];
        saveIfLoaded();
        refreshAutoScans();
        renderPackages();
      });
      list.appendChild(autoBtn);
    }
    return list;
  }

  // ── Auto-scan HTML files ──
  function refreshAutoScans() {
    if (!ws.packages) return;
    ws.packages.forEach(function (pkg) {
      if (pkg.html && pkg.html.length > 0) return;
      var dirs = pkg.dirs || [];
      if (dirs.length === 0) { autoHtmlCache[pkg.name] = []; return; }
      var scansRemaining = dirs.length;
      var allFiles = [];
      dirs.forEach(function (dir) {
        var scanPath = wsPath + "/" + dir;
        invoke("scan_html", { pkgDir: scanPath })
          .then(function (files) {
            allFiles = allFiles.concat(files);
            scansRemaining--;
            if (scansRemaining === 0) {
              var seen = {}, unique = [];
              allFiles.forEach(function (f) { if (!seen[f]) { seen[f] = true; unique.push(f); } });
              unique.sort();
              autoHtmlCache[pkg.name] = unique;
              renderPackages();
            }
          })
          .catch(function (err) {
            console.error("scan_html failed for " + dir + ":", err);
            scansRemaining--;
            if (scansRemaining === 0 && !autoHtmlCache[pkg.name]) {
              autoHtmlCache[pkg.name] = [];
              renderPackages();
            }
          });
      });
    });
  }

  // ── Drag-drop: Tauri 2 native onDragDropEvent ──
  var currentDropzone = null;

  // 用 getBoundingClientRect 命中 dropzone，比 elementFromPoint 可靠（不依赖 z-index/pointer-events）。
  function findDropzone(pos) {
    var dpr = window.devicePixelRatio || 1;
    var x = pos.x / dpr, y = pos.y / dpr;
    var zones = document.querySelectorAll("[data-dropzone]");
    for (var i = zones.length - 1; i >= 0; i--) {
      var r = zones[i].getBoundingClientRect();
      if (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) return zones[i];
    }
    return null;
  }

  function setupDragDrop() {
    var webview = null;
    try {
      var tw = window.__TAURI__ && window.__TAURI__.webview;
      if (tw && typeof tw.getCurrentWebview === "function") webview = tw.getCurrentWebview();
    } catch (e) { console.warn("getCurrentWebview failed:", e); }
    if (!webview) {
      try {
        var twin = window.__TAURI__ && window.__TAURI__.window;
        if (twin && typeof twin.getCurrentWebviewWindow === "function") webview = twin.getCurrentWebviewWindow();
      } catch (e2) { console.warn("getCurrentWebviewWindow failed:", e2); }
    }
    if (!webview || typeof webview.onDragDropEvent !== "function") {
      console.warn("Drag-drop unavailable: onDragDropEvent not found on webview.");
      return;
    }
    webview.onDragDropEvent(function (event) {
      var payload = event.payload;
      var type = payload.type;
      var pos = payload.position;
      if (type === "over" || type === "enter") {
        var dz = pos ? findDropzone(pos) : null;
        if (dz !== currentDropzone) {
          if (currentDropzone) currentDropzone.classList.remove("drop-active");
          currentDropzone = dz;
          if (currentDropzone) currentDropzone.classList.add("drop-active");
        }
      } else if (type === "drop") {
        if (currentDropzone) currentDropzone.classList.remove("drop-active");
        var paths = payload.paths || [];
        var dropTarget = pos ? findDropzone(pos) : null;
        if (dropTarget && paths.length > 0) handleDrop(dropTarget, paths);
        currentDropzone = null;
      } else if (type === "leave") {
        if (currentDropzone) { currentDropzone.classList.remove("drop-active"); currentDropzone = null; }
      }
    });
  }

  function handleDrop(dropzone, droppedPaths) {
    var type = dropzone.getAttribute("data-dropzone");
    var idxStr = dropzone.getAttribute("data-dropzone-idx");
    var idx = idxStr !== null ? parseInt(idxStr, 10) : -1;

    droppedPaths.forEach(function (absPath) {
      relativizePath(absPath).then(function (rel) {
        if (type === "output-dir") {
          ws.output_dir = rel;
          saveIfLoaded(); renderGeneral();
        } else if (type === "new-package") {
          ws.packages = (ws.packages || []).concat([{ name: basename(rel), dirs: [rel], html: [] }]);
          saveIfLoaded(); refreshAutoScans(); renderPackages();
        } else if (type === "pkg-dirs" && validIdx("packages", idx)) {
          ws.packages[idx].dirs = (ws.packages[idx].dirs || []).concat([rel]);
          saveIfLoaded(); refreshAutoScans(); renderPackages();
        } else if (type === "new-atlas") {
          ws.atlases = (ws.atlases || []).concat([{ name: basename(rel), standalone: false, dirs: [rel], max_size: 2048, padding: 4 }]);
          saveIfLoaded(); renderAtlases();
        } else if (type === "atlas-dirs" && validIdx("atlases", idx)) {
          ws.atlases[idx].dirs = (ws.atlases[idx].dirs || []).concat([rel]);
          saveIfLoaded(); renderAtlases();
        } else if (type === "new-font") {
          var fam = basename(rel).replace(/\.[^.]+$/, "");
          ws.fonts = (ws.fonts || []).concat([{ family: fam, file: rel, default: ws.fonts.length === 0, fallback: false }]);
          saveIfLoaded(); renderFonts();
        } else if (type === "font-file" && validIdx("fonts", idx)) {
          ws.fonts[idx].file = rel;
          if (!ws.fonts[idx].family) ws.fonts[idx].family = basename(rel).replace(/\.[^.]+$/, "");
          saveIfLoaded(); renderFonts();
        }
      }).catch(function (err) {
        alert("无法将拖入路径转为相对路径:\n" + absPath + "\n\n错误: " + err);
      });
    });
  }
  function basename(rel) { return rel.replace(/^.*[\\/]/, "").replace(/[\\/]$/, ""); }
  function validIdx(collection, idx) { return idx >= 0 && idx < (ws[collection] || []).length; }

  // ── Log modal ──
  function setupLog() {
    $("btn-log").addEventListener("click", function () { $("log-overlay").classList.remove("hidden"); });
    $("btn-log-close").addEventListener("click", function () { $("log-overlay").classList.add("hidden"); });
    $("log-overlay").addEventListener("click", function (e) {
      if (e.target === $("log-overlay")) $("log-overlay").classList.add("hidden");
    });
  }

  // ── Build ──
  function setupBuild() {
    $("btn-build").addEventListener("click", function () {
      var logDiv = $("build-log");
      var currentWs = collectWorkspace();
      if (!currentWs.output_dir || !currentWs.output_dir.trim()) {
        logDiv.innerHTML = '<p class="log-err">未配置输出目录</p>';
        alert("请先在【常规】页配置「输出目录」(output_dir)，再打包。");
        return;
      }
      ws = currentWs;
      logDiv.innerHTML = '<p style="color:#808090">构建中...</p>';
      invoke("save_workspace", { path: wsPath, ws: currentWs })
        .then(function () { return invoke("run_build", { path: wsPath }); })
        .then(function (report) {
          showBuildReport(report);
          var parts = [];
          if (report.packages && report.packages.length) parts.push("包: " + report.packages.join(", "));
          if (report.atlases && report.atlases.length) parts.push("图集: " + report.atlases.join(", "));
          if (report.fonts && report.fonts.length) parts.push("字体: " + report.fonts.join(", "));
          alert("✓ 打包成功\n\n" + (parts.length ? parts.join("\n") : "(无产物)"));
        })
        .catch(function (err) {
          logDiv.innerHTML = '<p class="log-err">构建失败: ' + escapeHtml(err) + "</p>";
          alert("✗ 构建失败: " + err + "\n\n点【日志】查看详情");
        });
    });
  }
  function showBuildReport(report) {
    var logDiv = $("build-log");
    var html = '<div class="build-summary">';
    if (report.packages && report.packages.length) html += "<span>包: " + report.packages.join(", ") + "</span>";
    if (report.atlases && report.atlases.length) html += "<span>图集: " + report.atlases.join(", ") + "</span>";
    if (report.fonts && report.fonts.length) html += "<span>字体: " + report.fonts.join(", ") + "</span>";
    if (!report.packages || !report.packages.length) html += '<span class="build-err">未发现包</span>';
    html += "</div>";
    if (report.log && report.log.length) {
      report.log.forEach(function (line) {
        var cls = "";
        if (line.indexOf("[OK]") !== -1 || line.indexOf("[SUCCESS]") !== -1) cls = "log-ok";
        else if (line.indexOf("[ERROR]") !== -1 || line.indexOf("[FAIL]") !== -1 || line.indexOf("error:") !== -1) cls = "log-err";
        else if (line.indexOf("[WARN]") !== -1) cls = "log-warn";
        html += '<p class="log-line' + (cls ? " " + cls : "") + '">' + escapeHtml(line) + "</p>";
      });
    }
    logDiv.innerHTML = html;
  }
  function animateRender(fn) {
    if (document.startViewTransition) {
      document.startViewTransition(function () { fn(); });
    } else {
      fn();
    }
  }
  function escapeHtml(text) {
    return String(text).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  }

  // ── Entry point (called from app.js) ──
  function renderMain(_ws, _path) {
    ws = _ws || { version: 1, output_dir: "", packages: [], atlases: [], fonts: [] };
    wsPath = _path;
    if (!ws.packages) ws.packages = [];
    if (!ws.atlases) ws.atlases = [];
    if (!ws.fonts) ws.fonts = [];

    renderGeneral();
    renderPackages();
    renderAtlases();
    renderFonts();
    refreshAutoScans();
    if (!editorInited) {
      editorInited = true;
      setupTabs();
      setupDragDrop();
      setupLog();
      setupBuild();
      setupInitModal();
    }
  }

  function flushSave() {
    if (!wsPath) return;
    if (saveTimer) { clearTimeout(saveTimer); saveTimer = null; }
    ws = collectWorkspace();
    invoke("save_workspace", { path: wsPath, ws: ws }).catch(function (err) {
      console.error("flushSave failed:", err);
    });
  }

  window.LoomGUIEditor = { renderMain: renderMain, flushSave: flushSave };
})();
