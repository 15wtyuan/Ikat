// Ikat Packer — frontend (plain JS, Tauri 2)
// Start screen: recent workspaces + new/open via native directory picker.

(function () {
  "use strict";

  var invoke = window.__TAURI__ ? window.__TAURI__.core.invoke : null;

  if (!invoke) {
    document.body.innerHTML =
      '<p style="padding:48px;text-align:center;color:#fc5c7c;">' +
      "window.__TAURI__ not available — run inside Tauri shell</p>";
    return;
  }

  var $ = function (id) { return document.getElementById(id); };
  var startScreen = $("start-screen");
  var mainScreen  = $("main-screen");
  var recentList  = $("recent-list");
  var btnNew      = $("btn-new");
  var btnOpen     = $("btn-open");
  var btnBack     = $("btn-back");

  // 走 plugin command（不依赖 npm JS 包）。directory + 单选 → string | null。
  function pickDirectory(title) {
    return invoke("plugin:dialog|open", {
      options: { directory: true, multiple: false, title: title || "选择工作区目录" },
    }).then(function (result) {
      return result || null;
    });
  }

  function loadRecent() {
    invoke("recent_workspaces")
      .then(function (paths) {
        renderRecent(paths);
      })
      .catch(function (err) {
        console.error("recent_workspaces failed:", err);
        showEmpty("加载失败");
      });
  }

  function renderRecent(paths) {
    recentList.innerHTML = "";
    if (!paths || paths.length === 0) {
      showEmpty("暂无最近使用的工作区");
      return;
    }
    paths.forEach(function (p) {
      var card = document.createElement("div");
      card.className = "recent-card";
      card.title = p;
      card.addEventListener("click", function () { openWorkspace(p); });

      var icon = document.createElement("span");
      icon.className = "recent-card-icon";
      icon.textContent = "📁";

      var info = document.createElement("div");
      info.className = "recent-card-info";

      var pathEl = document.createElement("div");
      pathEl.className = "recent-card-path";
      pathEl.textContent = p;

      var name = p.replace(/[\\/]$/, "").split(/[\\/]/).pop() || p;
      var nameEl = document.createElement("div");
      nameEl.className = "recent-card-name";
      nameEl.textContent = name;

      // 移除按钮：只从列表删记录，不删工作区目录。无破坏性，不弹确认。
      var removeBtn = document.createElement("button");
      removeBtn.className = "recent-card-remove";
      removeBtn.type = "button";
      removeBtn.title = "从列表移除（不删除工作区）";
      removeBtn.textContent = "×";
      removeBtn.addEventListener("click", function (e) {
        e.stopPropagation();
        invoke("remove_recent", { path: p })
          .then(loadRecent)
          .catch(function (err) {
            alert("移除失败: " + err);
          });
      });

      info.appendChild(pathEl);
      info.appendChild(nameEl);
      card.appendChild(icon);
      card.appendChild(info);
      card.appendChild(removeBtn);
      recentList.appendChild(card);
    });
  }

  function showEmpty(msg) {
    recentList.innerHTML = '<p class="empty-msg">' + msg + "</p>";
  }

  var wsRootPath = null;

  function openWorkspace(path) {
    invoke("open_workspace", { path: path })
      .then(function (res) {
        wsRootPath = path;
        renderMain(res.ws, res.uiPath);
        probeWorkspaceUpdate(path);
      })
      .catch(function (err) {
        alert("打开工作区失败: " + err);
      });
  }

  // 生成物（skills + .ikat CLI）落后于 GUI 配套版本 → 亮「更新工作区」。
  // 一键 = ikat scaffold（刷新 skills / CLI / 版本戳；config 与源文件不动）。
  function probeWorkspaceUpdate(path) {
    var btn = $("btn-update-ws");
    btn.classList.add("hidden");
    invoke("workspace_update_state", { path: path })
      .then(function (st) {
        if (!st.stale) return;
        btn.classList.remove("hidden");
        btn.textContent = "更新工作区 (" + st.stamped + " → " + st.current + ")";
      })
      .catch(function () { /* 探测失败静默——更新入口非关键路径 */ });
  }

  function showStart() {
    startScreen.classList.remove("hidden");
    mainScreen.classList.add("hidden");
    loadRecent();
  }

  function renderMain(ws, path) {
    startScreen.classList.add("hidden");
    mainScreen.classList.remove("hidden");

    $("main-path").textContent = path;

    if (window.IkatEditor) {
      window.IkatEditor.renderMain(ws, path);
    }
  }

  $("btn-update-ws").addEventListener("click", function () {
    if (!wsRootPath) return;
    var btn = $("btn-update-ws");
    btn.disabled = true;
    invoke("update_workspace", { path: wsRootPath })
      .then(function (st) {
        btn.disabled = false;
        if (st.stale) {
          alert("刷新完成，但版本戳仍落后（" + st.stamped + " → " + st.current + "）——请重试或手动跑 ikat scaffold。");
        } else {
          btn.classList.add("hidden");
        }
      })
      .catch(function (err) {
        btn.disabled = false;
        alert("更新工作区失败: " + err);
      });
  });

  // 独立小弹窗（不与 editor.js 的 init-overlay 复用——那个绑定主屏的脚手架更新流程）。
  // 会话根 = agent 会话打开的目录（skills/.ikat 落这里）；ui 目录 = UI 工作区
  // （workspace.json 落这里）。默认 ui（根下子目录），可改成 "."（单目录形态）。
  // UI 目录三种输入：手输相对路径 / 「浏览」选目录 / 拖文件夹进输入区；后两者走
  // 后端 relativize（基准 = 会话根）转相对，根外路径行内报错（init 语义 ui 须在根内）。
  function pickLayoutAndAgents(sessionRoot, cb) {
    var sep = sessionRoot.indexOf("\\") >= 0 ? "\\" : "/";
    var overlay = document.createElement("div");
    overlay.className = "modal-overlay";
    overlay.innerHTML =
      '<div class="modal">' +
      "<h3>初始化工作区</h3>" +
      '<div class="nw-root-banner"><span class="nw-root-icon">📁</span>' +
      '<span class="nw-root-label">会话根</span>' +
      '<span class="nw-root-path"></span></div>' +
      '<p class="modal-desc">会话根将获得 skills 与 .ikat/（ikat CLI + config.json，建议入库）；' +
      "UI 工作区（ikat.workspace.json 与源文件）放在下面的目录。</p>" +
      '<div class="field-label"><span>UI 目录</span><span class="field-hint">手输相对路径 · 浏览 · 拖入文件夹</span></div>' +
      '<div class="nw-ui-row" id="nw-ui-row">' +
      '<div class="nw-ui-line">' +
      '<input type="text" id="nw-ui-dir" class="form-input" value="ui" spellcheck="false" autocomplete="off" />' +
      '<button type="button" id="nw-ui-browse" class="btn btn-secondary btn-sm">浏览…</button></div>' +
      '<div class="nw-ui-preview" id="nw-ui-preview"></div>' +
      '<div class="nw-ui-error nw-hidden" id="nw-ui-error"></div></div>' +
      '<div class="field-label"><span>Agent skills</span><span class="field-hint">可多选</span></div>' +
      '<label class="agent-option"><input type="checkbox" id="nw-check-claude" checked />' +
      '<div class="agent-option-body"><span class="agent-option-name">Claude</span>' +
      '<span class="agent-option-path">.claude/skills/</span></div></label>' +
      '<label class="agent-option"><input type="checkbox" id="nw-check-agents" checked />' +
      '<div class="agent-option-body"><span class="agent-option-name">其他</span>' +
      '<span class="agent-option-path">.agents/skills/</span>' +
      '<span class="agent-option-note">Codex / ZCode 等 AGENTS.md 约定</span></div></label>' +
      '<div class="modal-actions">' +
      '<button id="nw-cancel" class="btn btn-secondary">取消</button>' +
      '<button id="nw-ok" class="btn btn-primary">创建</button></div></div>';
    document.body.appendChild(overlay);

    overlay.querySelector(".nw-root-path").textContent = sessionRoot;
    overlay.querySelector(".nw-root-path").title = sessionRoot;

    var input = overlay.querySelector("#nw-ui-dir");
    var preview = overlay.querySelector("#nw-ui-preview");
    var errEl = overlay.querySelector("#nw-ui-error");
    var uiRow = overlay.querySelector("#nw-ui-row");

    function updatePreview() {
      var v = input.value.trim();
      var text = v === "." ? sessionRoot : sessionRoot + sep + (v || "ui").replace(/\//g, sep);
      preview.textContent = text;
      preview.title = text;
    }
    function showError(msg) {
      errEl.textContent = msg;
      errEl.classList.remove("nw-hidden");
    }
    input.addEventListener("input", function () {
      errEl.classList.add("nw-hidden");
      updatePreview();
    });

    // 绝对目录 → 相对会话根填入。空串 = 目录即会话根（"." 形态）；".." 开头 = 根外。
    function applyAbsPath(absPath) {
      invoke("relativize", { root: sessionRoot, abs: absPath })
        .then(function (rel) {
          if (rel !== "" && rel.indexOf("..") === 0) {
            showError("UI 目录须在会话根内：" + absPath);
            return;
          }
          input.value = rel === "" ? "." : rel;
          errEl.classList.add("nw-hidden");
          updatePreview();
        })
        .catch(function (err) { showError(String(err)); });
    }

    overlay.querySelector("#nw-ui-browse").addEventListener("click", function () {
      pickDirectory("选择 UI 目录（会话根内）").then(function (p) {
        if (p) applyAbsPath(p);
      });
    });

    // 拖拽：独立注册原生 drag-drop（与 editor.js 的主屏 dropzone 互不感知——
    // 这里不用 data-dropzone 属性，editor 的全局命中扫描扫不到弹窗）。弹窗关闭即注销。
    var webview = null;
    try {
      var tw = window.__TAURI__ && window.__TAURI__.webview;
      if (tw && typeof tw.getCurrentWebview === "function") webview = tw.getCurrentWebview();
    } catch (e) { /* dev shell 无 webview API 时静默降级为无拖拽 */ }
    function hitRow(pos) {
      if (!pos) return false;
      var dpr = window.devicePixelRatio || 1;
      var x = pos.x / dpr, y = pos.y / dpr;
      var r = uiRow.getBoundingClientRect();
      return x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
    }
    var unlisten = null;
    var closed = false;
    if (webview && typeof webview.onDragDropEvent === "function") {
      webview.onDragDropEvent(function (event) {
        var p = event.payload;
        if (p.type === "over" || p.type === "enter") {
          uiRow.classList.toggle("nw-drop-active", hitRow(p.position));
        } else if (p.type === "leave") {
          uiRow.classList.remove("nw-drop-active");
        } else if (p.type === "drop") {
          var on = hitRow(p.position);
          uiRow.classList.remove("nw-drop-active");
          if (on && p.paths && p.paths.length > 0) applyAbsPath(p.paths[0]);
        }
      }).then(function (un) {
        if (closed) { un(); } else { unlisten = un; }
      });
    }

    function close() {
      closed = true;
      if (unlisten) { unlisten(); unlisten = null; }
      document.body.removeChild(overlay);
    }
    function submit() {
      var agents = [];
      if (overlay.querySelector("#nw-check-claude").checked) agents.push("claude");
      if (overlay.querySelector("#nw-check-agents").checked) agents.push("agents");
      var uiDir = input.value.trim();
      close();
      cb(uiDir || "ui", agents);
    }
    overlay.querySelector("#nw-cancel").addEventListener("click", close);
    overlay.querySelector("#nw-ok").addEventListener("click", submit);
    overlay.addEventListener("keydown", function (e) {
      if (e.key === "Enter") submit();
      else if (e.key === "Escape") close();
    });
    updatePreview();
  }

  btnNew.addEventListener("click", async function () {
    var path = await pickDirectory("选择会话根目录（游戏仓库根或任意工作目录）");
    if (!path) return;
    pickLayoutAndAgents(path, function (uiDir, agents) {
      invoke("create_workspace", { path: path, uiDir: uiDir, agents: agents })
        .then(function (res) {
          renderMain(res.ws, res.uiPath);
        })
        .catch(function (err) {
          alert("创建失败: " + err);
        });
    });
  });

  btnOpen.addEventListener("click", async function () {
    var path = await pickDirectory("选择要打开的工作区目录");
    if (!path) return;
    openWorkspace(path);
  });

  btnBack.addEventListener("click", function () {
    if (window.IkatEditor && window.IkatEditor.flushSave) window.IkatEditor.flushSave();
    showStart();
  });

  loadRecent();
})();
