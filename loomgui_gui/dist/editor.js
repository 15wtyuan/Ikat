// LoomGUI Packer — Main Editor (Task 20)
// 4-section workspace editor: workspace / packages / atlases / fonts
// Drag-drop via Tauri 2 native onDragDropEvent, save-on-change, build button.
(function () {
  "use strict";

  // ── Tauri API helpers ──
  var invoke = window.__TAURI__ ? window.__TAURI__.core.invoke : null;
  var $ = function (id) { return document.getElementById(id); };

  // ── State ──
  var ws = null;
  var wsPath = null;
  var autoHtmlCache = {}; // pkgName -> string[] (auto-scanned html files)

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

  // ── Save ──
  function collectWorkspace() {
    // Read named fields from DOM (text inputs created with IDs)
    var pkgCount = ws.packages ? ws.packages.length : 0;
    var atlasCount = ws.atlases ? ws.atlases.length : 0;
    var fontCount = ws.fonts ? ws.fonts.length : 0;

    var packages = [];
    for (var i = 0; i < pkgCount; i++) {
      var nameEl = $("pkg-name-" + i);
      var dirs = readTagList($("pkg-dirs-" + i));
      var html = readTagList($("pkg-html-" + i));
      packages.push({
        name: nameEl ? nameEl.value.trim() : (ws.packages[i] ? ws.packages[i].name : ""),
        dirs: dirs,
        html: html,
      });
    }

    var atlases = [];
    for (var i2 = 0; i2 < atlasCount; i2++) {
      var aname = $("atlas-name-" + i2);
      var adefault = $("atlas-default-" + i2);
      var astandalone = $("atlas-standalone-" + i2);
      var adirs = readTagList($("atlas-dirs-" + i2));
      var amax = $("atlas-max-size-" + i2);
      var apad = $("atlas-padding-" + i2);
      atlases.push({
        name: aname ? aname.value.trim() : (ws.atlases[i2] ? ws.atlases[i2].name : ""),
        default: adefault ? adefault.checked : false,
        standalone: astandalone ? astandalone.checked : false,
        dirs: adirs,
        max_size: amax ? (parseInt(amax.value, 10) || 2048) : 2048,
        padding: apad ? (parseInt(apad.value, 10) || 4) : 4,
      });
    }

    var fonts = [];
    for (var i3 = 0; i3 < fontCount; i3++) {
      var ffam = $("font-family-" + i3);
      var ffile = $("font-file-" + i3);
      var fdef = $("font-default-" + i3);
      var ffall = $("font-fallback-" + i3);
      fonts.push({
        family: ffam ? ffam.value.trim() : (ws.fonts[i3] ? ws.fonts[i3].family : ""),
        file: ffile ? ffile.value.trim() : (ws.fonts[i3] ? ws.fonts[i3].file : ""),
        default: fdef ? fdef.checked : false,
        fallback: ffall ? ffall.checked : false,
      });
    }

    return {
      version: ws.version || 1,
      output_dir: ($("ws-output-dir") ? $("ws-output-dir").value.trim() : "") || "../dist",
      packages: packages,
      atlases: atlases,
      fonts: fonts,
    };
  }

  function saveIfLoaded() {
    if (!wsPath) return;
    var newWs = collectWorkspace();
    // Update the in-memory ws so renders pick up the latest state
    ws = newWs;
    invoke("save_workspace", { path: wsPath, ws: newWs }).catch(function (err) {
      console.error("save_workspace failed:", err);
    });
  }

  function readTagList(container) {
    if (!container) return [];
    var result = [];
    var children = container.querySelectorAll(".tag[data-value]");
    for (var i = 0; i < children.length; i++) {
      var v = children[i].getAttribute("data-value");
      if (v !== null) result.push(v);
    }
    return result;
  }

  // ── Re-render helpers ──
  function renderAll() {
    renderWorkspace();
    renderPackages();
    renderAtlases();
    renderFonts();
    refreshAutoScans();
  }

  // ── 1. Workspace section ──
  function renderWorkspace() {
    var body = $("section-ws-body");
    body.innerHTML = "";

    var row = el("div", "form-row");
    var label = el("span", "form-label", "输出目录");
    var inp = inputEl("text", ws.output_dir || "../dist", "form-input", "例如 ../dist");
    inp.id = "ws-output-dir";
    inp.addEventListener("blur", function () {
      saveIfLoaded();
    });
    inp.addEventListener("keydown", function (e) {
      if (e.key === "Enter") { inp.blur(); }
    });

    row.appendChild(label);
    row.appendChild(inp);
    body.appendChild(row);
  }

  // ── 2. Packages section ──
  function renderPackages() {
    var body = $("section-pkg-body");
    body.innerHTML = "";

    if (!ws.packages) ws.packages = [];

    ws.packages.forEach(function (pkg, i) {
      var card = el("div", "config-card");
      card.innerHTML = "";

      // --- name ---
      var nameRow = el("div", "form-row");
      var nameLabel = el("span", "form-label", "名称");
      var nameInp = inputEl("text", pkg.name, "form-input");
      nameInp.id = "pkg-name-" + i;
      nameInp.addEventListener("blur", function () { saveIfLoaded(); });
      nameInp.addEventListener("keydown", function (e) {
        if (e.key === "Enter") { nameInp.blur(); }
      });
      nameRow.appendChild(nameLabel);
      nameRow.appendChild(nameInp);

      var delBtn = el("button", "btn-danger-sm", "删除");
      delBtn.addEventListener("click", (function (idx) {
        return function () {
          var pkgName = ws.packages[idx].name;
          ws.packages.splice(idx, 1);
          delete autoHtmlCache[pkgName];
          renderPackages();
          saveIfLoaded();
        };
      })(i));
      nameRow.appendChild(delBtn);
      card.appendChild(nameRow);

      // --- dirs ---
      var dirsLabel = el("div", "form-label", "源目录");
      card.appendChild(dirsLabel);
      var dirsList = el("div", "tag-list");
      dirsList.id = "pkg-dirs-" + i;

      (pkg.dirs || []).forEach(function (d, di) {
        var tag = el("span", "tag");
        tag.setAttribute("data-value", d);
        tag.textContent = d;
        var rm = el("button", "tag-remove", "×");
        rm.addEventListener("click", (function (idx, dirIdx) {
          return function () {
            ws.packages[idx].dirs.splice(dirIdx, 1);
            refreshAutoScans();
            renderPackages();
            saveIfLoaded();
          };
        })(i, di));
        tag.appendChild(rm);
        dirsList.appendChild(tag);
      });

      // dropzone for adding dirs to this package
      var dropDirs = el("span", "dropzone-inline", "+ 拖入目录");
      dropDirs.setAttribute("data-dropzone", "pkg-dirs");
      dropDirs.setAttribute("data-dropzone-idx", String(i));
      dirsList.appendChild(dropDirs);

      card.appendChild(dirsList);

      // --- html ---
      var htmlLabel = el("div", "form-label", "HTML 文件");
      card.appendChild(htmlLabel);
      var htmlList = el("div", "tag-list");
      htmlList.id = "pkg-html-" + i;

      if (!pkg.html || pkg.html.length === 0) {
        // Auto-state: show auto-scan results (or loading placeholder)
        var autoFiles = autoHtmlCache[pkg.name];
        if (autoFiles === undefined) {
          var scanning = el("span", "auto-scan-status", "扫描中...");
          htmlList.appendChild(scanning);
        } else if (autoFiles.length === 0) {
          var empty = el("span", "auto-scan-status", "未找到 .html 文件");
          htmlList.appendChild(empty);
        } else {
          autoFiles.forEach(function (f) {
            var tag = el("span", "tag auto");
            tag.setAttribute("data-value", f);
            tag.textContent = f;
            htmlList.appendChild(tag);
          });
        }

        var lockBtn = el("button", "btn-link-sm", "手动指定");
        lockBtn.title = "锁定为显式 HTML 列表";
        lockBtn.addEventListener("click", (function (idx) {
          return function () {
            var auto = autoHtmlCache[ws.packages[idx].name] || [];
            ws.packages[idx].html = auto.slice();
            saveIfLoaded();
            renderPackages();
          };
        })(i));
        htmlList.appendChild(lockBtn);
      } else {
        // Explicit-state: show specified files as tags
        pkg.html.forEach(function (h, hi) {
          var tag = el("span", "tag");
          tag.setAttribute("data-value", h);
          tag.textContent = h;
          var rm = el("button", "tag-remove", "×");
          rm.addEventListener("click", (function (idx, htmlIdx) {
            return function () {
              ws.packages[idx].html.splice(htmlIdx, 1);
              renderPackages();
              saveIfLoaded();
            };
          })(i, hi));
          tag.appendChild(rm);
          htmlList.appendChild(tag);
        });

        // add-html manually
        var addHtml = el("span", "dropzone-inline", "+ 添加文件");
        addHtml.style.cursor = "pointer";
        addHtml.addEventListener("click", (function (idx) {
          return function () {
            var fname = prompt("输入 HTML 文件名（含 .html 扩展名）:");
            if (fname) {
              ws.packages[idx].html = (ws.packages[idx].html || []).concat([fname.trim()]);
              saveIfLoaded();
              renderPackages();
            }
          };
        })(i));
        htmlList.appendChild(addHtml);

        var autoBtn = el("button", "btn-link-sm", "恢复自动扫");
        autoBtn.title = "清空显式列表，恢复自动扫描";
        autoBtn.addEventListener("click", (function (idx) {
          return function () {
            ws.packages[idx].html = [];
            saveIfLoaded();
            refreshAutoScans();
            renderPackages();
          };
        })(i));
        htmlList.appendChild(autoBtn);
      }

      card.appendChild(htmlList);
      body.appendChild(card);
    });

    // --- "建包" dropzone ---
    var newPkgDz = el("div", "dropzone", "拖入目录建包");
    newPkgDz.setAttribute("data-dropzone", "new-package");
    // Also clickable to add manually
    newPkgDz.style.cursor = "pointer";
    newPkgDz.addEventListener("click", function () {
      var fname = prompt("输入包名称:");
      if (!fname) return;
      var fdir = prompt("输入源目录（相对工作区根，例如 showcase）:");
      if (!fdir) return;
      ws.packages = (ws.packages || []).concat([{
        name: fname.trim(),
        dirs: [fdir.trim()],
        html: [],
      }]);
      saveIfLoaded();
      refreshAutoScans();
      renderPackages();
    });
    body.appendChild(newPkgDz);
  }

  // ── 3. Atlases section ──
  function renderAtlases() {
    var body = $("section-atlas-body");
    body.innerHTML = "";

    if (!ws.atlases) ws.atlases = [];

    ws.atlases.forEach(function (atlas, i) {
      var card = el("div", "config-card");
      card.innerHTML = "";

      // --- name ---
      var nameRow = el("div", "form-row");
      var nameLabel = el("span", "form-label", "名称");
      var nameInp = inputEl("text", atlas.name, "form-input");
      nameInp.id = "atlas-name-" + i;
      nameInp.addEventListener("blur", function () { saveIfLoaded(); });
      nameInp.addEventListener("keydown", function (e) {
        if (e.key === "Enter") { nameInp.blur(); }
      });
      nameRow.appendChild(nameLabel);
      nameRow.appendChild(nameInp);

      var delBtn = el("button", "btn-danger-sm", "删除");
      delBtn.addEventListener("click", (function (idx) {
        return function () {
          ws.atlases.splice(idx, 1);
          renderAtlases();
          saveIfLoaded();
        };
      })(i));
      nameRow.appendChild(delBtn);
      card.appendChild(nameRow);

      // --- checkboxes ---
      var checksRow = el("div", "card-checks");

      var defCb = el("label", "form-check");
      var defInp = inputEl("checkbox", "", "");
      defInp.id = "atlas-default-" + i;
      defInp.checked = !!atlas.default;
      defInp.addEventListener("change", function () { saveIfLoaded(); });
      defCb.appendChild(defInp);
      defCb.appendChild(document.createTextNode(" 默认图集"));
      checksRow.appendChild(defCb);

      var saCb = el("label", "form-check");
      var saInp = inputEl("checkbox", "", "");
      saInp.id = "atlas-standalone-" + i;
      saInp.checked = !!atlas.standalone;
      saInp.addEventListener("change", function () { saveIfLoaded(); });
      saCb.appendChild(saInp);
      saCb.appendChild(document.createTextNode(" 独立单页"));
      checksRow.appendChild(saCb);

      card.appendChild(checksRow);

      // --- dirs ---
      var dirsLabel = el("div", "form-label", "资源目录");
      card.appendChild(dirsLabel);
      var dirsList = el("div", "tag-list");
      dirsList.id = "atlas-dirs-" + i;

      (atlas.dirs || []).forEach(function (d, di) {
        var tag = el("span", "tag");
        tag.setAttribute("data-value", d);
        tag.textContent = d;
        var rm = el("button", "tag-remove", "×");
        rm.addEventListener("click", (function (idx, dirIdx) {
          return function () {
            ws.atlases[idx].dirs.splice(dirIdx, 1);
            renderAtlases();
            saveIfLoaded();
          };
        })(i, di));
        tag.appendChild(rm);
        dirsList.appendChild(tag);
      });

      var dropDirs = el("span", "dropzone-inline", "+ 拖入目录");
      dropDirs.setAttribute("data-dropzone", "atlas-dirs");
      dropDirs.setAttribute("data-dropzone-idx", String(i));
      dirsList.appendChild(dropDirs);

      card.appendChild(dirsList);

      // --- max_size + padding ---
      var sizeRow = el("div", "form-row");

      var msLabel = el("span", "form-label", "最大尺寸");
      var msInp = inputEl("number", String(atlas.max_size || 2048), "form-input form-input-number");
      msInp.id = "atlas-max-size-" + i;
      msInp.min = "64";
      msInp.step = "64";
      msInp.addEventListener("change", function () { saveIfLoaded(); });
      sizeRow.appendChild(msLabel);
      sizeRow.appendChild(msInp);

      var padLabel = el("span", "form-label", "间距");
      var padInp = inputEl("number", String(atlas.padding || 4), "form-input form-input-number");
      padInp.id = "atlas-padding-" + i;
      padInp.min = "0";
      padInp.max = "32";
      padInp.addEventListener("change", function () { saveIfLoaded(); });
      sizeRow.appendChild(padLabel);
      sizeRow.appendChild(padInp);

      card.appendChild(sizeRow);
      body.appendChild(card);
    });

    // --- add atlas button ---
    var addBtn = el("button", "btn-add", "+ 添加图集");
    addBtn.addEventListener("click", function () {
      var fname = prompt("输入图集名称:");
      if (!fname) return;
      ws.atlases = (ws.atlases || []).concat([{
        name: fname.trim(),
        default: false,
        standalone: false,
        dirs: [],
        max_size: 2048,
        padding: 4,
      }]);
      saveIfLoaded();
      renderAtlases();
    });
    body.appendChild(addBtn);
  }

  // ── 4. Fonts section ──
  function renderFonts() {
    var body = $("section-font-body");
    body.innerHTML = "";

    if (!ws.fonts) ws.fonts = [];

    ws.fonts.forEach(function (font, i) {
      var card = el("div", "config-card compact");
      card.innerHTML = "";

      // --- family ---
      var famRow = el("div", "form-row");
      var famLabel = el("span", "form-label", "字体族");
      var famInp = inputEl("text", font.family, "form-input");
      famInp.id = "font-family-" + i;
      famInp.addEventListener("blur", function () { saveIfLoaded(); });
      famInp.addEventListener("keydown", function (e) {
        if (e.key === "Enter") { famInp.blur(); }
      });
      famRow.appendChild(famLabel);
      famRow.appendChild(famInp);

      var delBtn = el("button", "btn-danger-sm", "删除");
      delBtn.addEventListener("click", (function (idx) {
        return function () {
          ws.fonts.splice(idx, 1);
          renderFonts();
          saveIfLoaded();
        };
      })(i));
      famRow.appendChild(delBtn);
      card.appendChild(famRow);

      // --- file ---
      var fileRow = el("div", "form-row");
      var fileLabel = el("span", "form-label", "文件");
      var fileInp = inputEl("text", font.file, "form-input");
      fileInp.id = "font-file-" + i;
      fileInp.addEventListener("blur", function () { saveIfLoaded(); });
      fileInp.addEventListener("keydown", function (e) {
        if (e.key === "Enter") { fileInp.blur(); }
      });
      fileRow.appendChild(fileLabel);
      fileRow.appendChild(fileInp);

      // inline dropzone for font file
      var dropFile = el("span", "dropzone-inline", "拖入字体");
      dropFile.setAttribute("data-dropzone", "font-file");
      dropFile.setAttribute("data-dropzone-idx", String(i));
      fileRow.appendChild(dropFile);
      card.appendChild(fileRow);

      // --- default (radio) + fallback (checkbox) ---
      var checksRow = el("div", "card-checks");

      var defCb = el("label", "form-check");
      var defInp = inputEl("radio", "", "");
      defInp.id = "font-default-" + i;
      defInp.name = "font-default-group";
      defInp.checked = !!font.default;
      defInp.addEventListener("change", function () {
        // Uncheck all others (radio group behavior, but we manage manually
        // in case DOM is regenerated)
        for (var j = 0; j < ws.fonts.length; j++) {
          ws.fonts[j].default = false;
        }
        ws.fonts[i].default = true;
        saveIfLoaded();
        renderFonts();
      });
      defCb.appendChild(defInp);
      defCb.appendChild(document.createTextNode(" 默认"));
      checksRow.appendChild(defCb);

      var fallCb = el("label", "form-check");
      var fallInp = inputEl("checkbox", "", "");
      fallInp.id = "font-fallback-" + i;
      fallInp.checked = !!font.fallback;
      fallInp.addEventListener("change", function () { saveIfLoaded(); });
      fallCb.appendChild(fallInp);
      fallCb.appendChild(document.createTextNode(" 后备"));
      checksRow.appendChild(fallCb);

      card.appendChild(checksRow);
      body.appendChild(card);
    });

    // --- drag-drop zone for new font ---
    var dropFont = el("div", "dropzone", "拖入字体文件添加");
    dropFont.setAttribute("data-dropzone", "new-font");
    // Allow manual add too
    dropFont.style.cursor = "pointer";
    dropFont.addEventListener("click", function () {
      var ffam = prompt("输入字体族名称（例如 NotoSansSC）:");
      if (!ffam) return;
      var ffile = prompt("输入字体文件路径（相对工作区根，例如 fonts/myfont.ttf）:");
      if (!ffile) return;
      ws.fonts = (ws.fonts || []).concat([{
        family: ffam.trim(),
        file: ffile.trim(),
        default: ws.fonts.length === 0,
        fallback: false,
      }]);
      saveIfLoaded();
      renderFonts();
    });
    body.appendChild(dropFont);
  }

  // ── Auto-scan HTML files ──
  function refreshAutoScans() {
    if (!ws.packages) return;

    ws.packages.forEach(function (pkg) {
      if (pkg.html && pkg.html.length > 0) {
        // Explicit mode — no auto-scan needed
        return;
      }
      // Auto mode — scan each dir
      var dirs = pkg.dirs || [];
      if (dirs.length === 0) {
        autoHtmlCache[pkg.name] = [];
        return;
      }

      var scansRemaining = dirs.length;
      var allFiles = [];

      dirs.forEach(function (dir) {
        var fullPath = wsPath + "/" + dir;
        invoke("scan_html", { pkgDir: fullPath })
          .then(function (files) {
            allFiles = allFiles.concat(files);
            scansRemaining--;
            if (scansRemaining === 0) {
              // Deduplicate and sort
              var seen = {};
              var unique = [];
              allFiles.forEach(function (f) {
                if (!seen[f]) { seen[f] = true; unique.push(f); }
              });
              unique.sort();
              autoHtmlCache[pkg.name] = unique;
              renderPackages(); // update the html tags display
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

  function setupDragDrop() {
    var webview = null;

    // Try Tauri 2 webview API
    try {
      var tauriWebview = window.__TAURI__ && window.__TAURI__.webview;
      if (tauriWebview && typeof tauriWebview.getCurrentWebview === "function") {
        webview = tauriWebview.getCurrentWebview();
      }
    } catch (e) {
      console.warn("window.__TAURI__.webview.getCurrentWebview() failed:", e);
    }

    // Fallback: try window module
    if (!webview) {
      try {
        var tauriWindow = window.__TAURI__ && window.__TAURI__.window;
        if (tauriWindow && typeof tauriWindow.getCurrentWebviewWindow === "function") {
          webview = tauriWindow.getCurrentWebviewWindow();
        }
      } catch (e2) {
        console.warn("window.__TAURI__.window.getCurrentWebviewWindow() failed:", e2);
      }
    }

    if (!webview || typeof webview.onDragDropEvent !== "function") {
      console.warn(
        "Drag-drop not available: onDragDropEvent not found on webview. " +
        "Tauri 2 native drag-drop requires @tauri-apps/api v2 webview module."
      );
      return;
    }

    webview.onDragDropEvent(function (event) {
      var payload = event.payload;
      var type = payload.type;
      var paths = payload.paths || [];
      var pos = payload.position;

      if (type === "over" || type === "enter") {
        var targetEl = null;
        if (pos) {
          targetEl = document.elementFromPoint(
            pos.x / (window.devicePixelRatio || 1),
            pos.y / (window.devicePixelRatio || 1)
          );
        }
        var dz = targetEl ? targetEl.closest("[data-dropzone]") : null;
        if (dz !== currentDropzone) {
          if (currentDropzone) currentDropzone.classList.remove("drop-active");
          currentDropzone = dz;
          if (currentDropzone) currentDropzone.classList.add("drop-active");
        }
      } else if (type === "drop") {
        if (currentDropzone) currentDropzone.classList.remove("drop-active");
        var dropTarget = null;
        if (pos) {
          dropTarget = document.elementFromPoint(
            pos.x / (window.devicePixelRatio || 1),
            pos.y / (window.devicePixelRatio || 1)
          );
        }
        var dz = dropTarget ? dropTarget.closest("[data-dropzone]") : null;
        if (dz && paths.length > 0) {
          handleDrop(dz, paths);
        }
        currentDropzone = null;
      } else if (type === "leave") {
        if (currentDropzone) {
          currentDropzone.classList.remove("drop-active");
          currentDropzone = null;
        }
      }
    });
  }

  function handleDrop(dropzone, droppedPaths) {
    var type = dropzone.getAttribute("data-dropzone");
    var idxStr = dropzone.getAttribute("data-dropzone-idx");
    var idx = idxStr !== null ? parseInt(idxStr, 10) : -1;

    droppedPaths.forEach(function (absPath) {
      invoke("relativize", { root: wsPath, abs: absPath })
        .then(function (rel) {
          if (type === "new-package") {
            var name = rel.replace(/^.*[\\/]/, "").replace(/[\\/]$/, "");
            ws.packages = (ws.packages || []).concat([{
              name: name,
              dirs: [rel],
              html: [],
            }]);
            saveIfLoaded();
            refreshAutoScans();
            renderPackages();
          } else if (type === "pkg-dirs") {
            if (idx >= 0 && idx < (ws.packages || []).length) {
              ws.packages[idx].dirs = (ws.packages[idx].dirs || []).concat([rel]);
              saveIfLoaded();
              refreshAutoScans();
              renderPackages();
            }
          } else if (type === "atlas-dirs") {
            if (idx >= 0 && idx < (ws.atlases || []).length) {
              ws.atlases[idx].dirs = (ws.atlases[idx].dirs || []).concat([rel]);
              saveIfLoaded();
              renderAtlases();
            }
          } else if (type === "new-font") {
            var base = rel.replace(/^.*[\\/]/, "");
            var family = base.replace(/\.[^.]+$/, "");
            ws.fonts = (ws.fonts || []).concat([{
              family: family,
              file: rel,
              default: ws.fonts.length === 0,
              fallback: false,
            }]);
            saveIfLoaded();
            renderFonts();
          } else if (type === "font-file") {
            if (idx >= 0 && idx < (ws.fonts || []).length) {
              var base2 = rel.replace(/^.*[\\/]/, "");
              var family2 = base2.replace(/\.[^.]+$/, "");
              ws.fonts[idx].file = rel;
              if (!ws.fonts[idx].family) ws.fonts[idx].family = family2;
              saveIfLoaded();
              renderFonts();
            }
          }
        })
        .catch(function (err) {
          console.error("relativize failed for " + absPath + ":", err);
          alert("无法将拖入路径转换为相对路径:\n" + absPath + "\n\n错误: " + err +
            "\n\n请确认拖入的文件/目录位于工作区目录下。");
        });
    });
  }

  // ── Build ──
  function setupBuild() {
    var btnBuild = $("btn-build");
    if (!btnBuild) return;

    btnBuild.addEventListener("click", function () {
      var logDiv = $("build-log");
      logDiv.innerHTML = '<p style="color:#808090">构建中...</p>';
      $("section-build-log").scrollIntoView({ behavior: "smooth" });

      // Save before build
      var currentWs = collectWorkspace();
      ws = currentWs;
      invoke("save_workspace", { path: wsPath, ws: currentWs })
        .then(function () {
          return invoke("run_build", { path: wsPath });
        })
        .then(function (report) {
          showBuildReport(report);
        })
        .catch(function (err) {
          logDiv.innerHTML = '<p class="log-err">构建失败: ' + err + "</p>";
        });
    });
  }

  function showBuildReport(report) {
    var logDiv = $("build-log");
    var html = "";

    // Summary
    html += '<div class="build-summary">';
    if (report.packages && report.packages.length > 0) {
      html += "<span>包: " + report.packages.join(", ") + "</span>";
    }
    if (report.atlases && report.atlases.length > 0) {
      html += "<span>图集: " + report.atlases.join(", ") + "</span>";
    }
    if (report.fonts && report.fonts.length > 0) {
      html += "<span>字体: " + report.fonts.join(", ") + "</span>";
    }
    if (!report.packages || report.packages.length === 0) {
      html += '<span class="build-err">未发现包</span>';
    }
    html += "</div>";

    // Log lines
    if (report.log && report.log.length > 0) {
      report.log.forEach(function (line) {
        var cls = "";
        if (line.indexOf("[OK]") !== -1 || line.indexOf("[SUCCESS]") !== -1) {
          cls = "log-ok";
        } else if (
          line.indexOf("[ERROR]") !== -1 ||
          line.indexOf("[FAIL]") !== -1 ||
          line.indexOf("error:") !== -1
        ) {
          cls = "log-err";
        } else if (line.indexOf("[WARN]") !== -1) {
          cls = "log-warn";
        }
        html += '<p class="log-line' + (cls ? " " + cls : "") + '">' +
          escapeHtml(line) + "</p>";
      });
    }

    logDiv.innerHTML = html;
  }

  function escapeHtml(text) {
    return text
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;");
  }

  // ── Entry point (called from app.js) ──
  function renderMain(_ws, _path) {
    ws = _ws || { version: 1, output_dir: "../dist", packages: [], atlases: [], fonts: [] };
    wsPath = _path;

    // Normalize: ensure arrays exist
    if (!ws.packages) ws.packages = [];
    if (!ws.atlases) ws.atlases = [];
    if (!ws.fonts) ws.fonts = [];

    renderAll();
    setupDragDrop();
    setupBuild();
  }

  // ── Expose to app.js ──
  window.LoomGUIEditor = { renderMain: renderMain };
})();
