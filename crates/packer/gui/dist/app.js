// LoomGUI Packer — frontend (plain JS, Tauri 2)
// Start screen: recent workspaces + new/open via native directory picker.

(function () {
  "use strict";

  // ── Tauri API helpers ──
  var invoke = window.__TAURI__ ? window.__TAURI__.core.invoke : null;

  if (!invoke) {
    document.body.innerHTML =
      '<p style="padding:48px;text-align:center;color:#fc5c7c;">' +
      "window.__TAURI__ not available — run inside Tauri shell</p>";
    return;
  }

  // ── DOM refs ──
  var $ = function (id) { return document.getElementById(id); };
  var startScreen = $("start-screen");
  var mainScreen  = $("main-screen");
  var recentList  = $("recent-list");
  var btnNew      = $("btn-new");
  var btnOpen     = $("btn-open");
  var btnBack     = $("btn-back");

  // ── Native directory picker (tauri-plugin-dialog) ──
  // 走 plugin command（不依赖 npm JS 包）。directory + 单选 → string | null。
  function pickDirectory(title) {
    return invoke("plugin:dialog|open", {
      options: { directory: true, multiple: false, title: title || "选择工作区目录" },
    }).then(function (result) {
      return result || null;
    });
  }

  // ── Start screen: load recent workspaces ──
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
      icon.textContent = "📁"; // folder emoji

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

  // ── Workspace actions ──
  function openWorkspace(path) {
    invoke("open_workspace", { path: path })
      .then(function (ws) {
        renderMain(ws, path);
      })
      .catch(function (err) {
        alert("打开工作区失败: " + err);
      });
  }

  // ── Screen transitions ──
  function showStart() {
    startScreen.classList.remove("hidden");
    mainScreen.classList.add("hidden");
    loadRecent();
  }

  function renderMain(ws, path) {
    startScreen.classList.add("hidden");
    mainScreen.classList.remove("hidden");

    $("main-path").textContent = path;

    if (window.LoomGUIEditor) {
      window.LoomGUIEditor.renderMain(ws, path);
    }
  }

  // ── Event bindings ──
  btnNew.addEventListener("click", async function () {
    var path = await pickDirectory("选择新建工作区的目录");
    if (!path) return;
    invoke("create_workspace", { path: path })
      .then(function (ws) {
        renderMain(ws, path);
      })
      .catch(function (err) {
        alert("创建失败: " + err);
      });
  });

  btnOpen.addEventListener("click", async function () {
    var path = await pickDirectory("选择要打开的工作区目录");
    if (!path) return;
    openWorkspace(path);
  });

  btnBack.addEventListener("click", function () {
    if (window.LoomGUIEditor && window.LoomGUIEditor.flushSave) window.LoomGUIEditor.flushSave();
    showStart();
  });

  // ── Init ──
  loadRecent();
})();
