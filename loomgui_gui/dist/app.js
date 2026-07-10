// LoomGUI Packer — frontend (plain JS, Tauri 2)
// Task 19: start screen (recent workspaces + new/open)
// Dialog: text-input fallback (tauri-plugin-dialog not wired — T20 can upgrade)

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

  // Modal
  var modalOverlay  = $("modal-overlay");
  var modalTitle    = $("modal-title");
  var modalInput    = $("modal-input");
  var modalError    = $("modal-error");
  var modalCancel   = $("modal-cancel");
  var modalConfirm  = $("modal-confirm");
  var modalAction   = null; // "new" or "open"

  // ── State ──
  var currentWorkspace = null;
  var currentPath = null;

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

      // Extract last component as display name
      var name = p.replace(/[\\/]$/, "").split(/[\\/]/).pop() || p;
      var nameEl = document.createElement("div");
      nameEl.className = "recent-card-name";
      nameEl.textContent = name;

      info.appendChild(pathEl);
      info.appendChild(nameEl);
      card.appendChild(icon);
      card.appendChild(info);
      recentList.appendChild(card);
    });
  }

  function showEmpty(msg) {
    recentList.innerHTML = '<p class="empty-msg">' + msg + "</p>";
  }

  // ── Modal ──
  function showModal(action) {
    modalAction = action;
    if (action === "new") {
      modalTitle.textContent = "新建工作区";
      modalConfirm.textContent = "创建";
    } else {
      modalTitle.textContent = "打开工作区";
      modalConfirm.textContent = "打开";
    }
    modalInput.value = "";
    modalError.classList.add("hidden");
    modalOverlay.classList.remove("hidden");
    modalInput.focus();
  }

  function hideModal() {
    modalOverlay.classList.add("hidden");
    modalAction = null;
  }

  function confirmModal() {
    var path = modalInput.value.trim();
    if (!path) {
      modalError.textContent = "请输入工作区目录路径";
      modalError.classList.remove("hidden");
      return;
    }
    modalError.classList.add("hidden");

    if (modalAction === "new") {
      invoke("create_workspace", { path: path })
        .then(function (ws) {
          hideModal();
          renderMain(ws, path);
        })
        .catch(function (err) {
          modalError.textContent = "创建失败: " + err;
          modalError.classList.remove("hidden");
        });
    } else {
      openWorkspace(path);
      hideModal();
    }
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
    currentWorkspace = null;
    currentPath = null;
    loadRecent();
  }

  function renderMain(ws, path) {
    currentWorkspace = ws;
    currentPath = path;
    startScreen.classList.add("hidden");
    mainScreen.classList.remove("hidden");

    $("main-path").textContent = path;

    if (window.LoomGUIEditor) {
      window.LoomGUIEditor.renderMain(ws, path);
    }
  }

  // ── Event bindings ──
  btnNew.addEventListener("click", function () { showModal("new"); });
  btnOpen.addEventListener("click", function () { showModal("open"); });
  btnBack.addEventListener("click", showStart);
  modalCancel.addEventListener("click", hideModal);
  modalConfirm.addEventListener("click", confirmModal);

  // Keyboard: Enter to confirm, Escape to cancel
  modalInput.addEventListener("keydown", function (e) {
    if (e.key === "Enter") confirmModal();
    if (e.key === "Escape") hideModal();
  });
  modalOverlay.addEventListener("click", function (e) {
    if (e.target === modalOverlay) hideModal();
  });

  // ── Init ──
  loadRecent();
})();
