// LoomGUI showcase browser preview driver (preview-only — packer consumes body only).
// Nav + role=tablist switch + dialog display toggle + ListView visual fill + NativeHost placeholder + body letterbox.
// Classic script (non-ES module) to avoid file:// CORS.
(function () {
  'use strict';

  var NAV = {
    'nav-settings': 'settings',
    'nav-inventory': 'inventory',
    'nav-mail': 'mail',
    'nav-shop': 'shop',
    'nav-character': 'character',
    'nav-form': 'form',
    'nav-lab': 'lab'
  };

  function $(id) { return document.getElementById(id); }
  function bind(id, type, fn) { var el = $(id); if (el) el.addEventListener(type, fn); }

  function goPage(name) {
    var dir = location.href.substring(0, location.href.lastIndexOf('/') + 1);
    location.href = dir + name + '.html';
  }

  function wireNav() {
    Object.keys(NAV).forEach(function (id) {
      bind(id, 'click', function () { goPage(NAV[id]); });
    });
    bind('back-home', 'click', function () { goPage('home'); });
  }

  function wireTabs() {
    var tabs = document.querySelectorAll('[role="tab"]');
    tabs.forEach(function (tab) {
      tab.addEventListener('click', function () {
        var list = tab.closest('[role="tablist"]');
        if (!list) return;
        list.querySelectorAll('[role="tab"]').forEach(function (t) {
          t.setAttribute('aria-selected', 'false');
        });
        tab.setAttribute('aria-selected', 'true');
        var target = tab.getAttribute('aria-controls');
        document.querySelectorAll('[role="tabpanel"]').forEach(function (p) {
          p.style.display = (p.id === target) ? '' : 'none';
        });
      });
    });
  }

  function wireDialogs() {
    document.querySelectorAll('[data-open-dialog]').forEach(function (btn) {
      btn.addEventListener('click', function () {
        var id = btn.getAttribute('data-open-dialog');
        var dlg = $(id);
        if (dlg) dlg.style.display = '';
      });
    });
    document.querySelectorAll('[data-close-dialog]').forEach(function (btn) {
      btn.addEventListener('click', function () {
        var dlg = btn.closest('[role="dialog"]');
        if (dlg) dlg.style.display = 'none';
      });
    });
  }

  // Preview-only: rotate item icons across filled rows so the inventory grid
  // reads as varied items. Runtime ListView is data-driven; this is a visual aid.
  var ITEM_ROTATION = ['item-potion', 'item-chest', 'item-gem', 'item-scroll', 'item-staff', 'item-wand'];
  function fillListViews() {
    var dir = location.href.substring(0, location.href.lastIndexOf('/') + 1) + '../res/icons/';
    document.querySelectorAll('ul[data-fill]').forEach(function (ul) {
      var tpl = ul.querySelector('template');
      if (!tpl) return;
      var count = parseInt(ul.getAttribute('data-fill'), 10) || 8;
      for (var i = 1; i < count; i++) {
        var node = tpl.content.cloneNode(true);
        var img = node.querySelector('img');
        if (img && /\/item-/.test(img.getAttribute('src') || '')) {
          img.setAttribute('src', dir + ITEM_ROTATION[i % ITEM_ROTATION.length] + '.png');
        }
        ul.appendChild(node);
      }
    });
  }

  // NativeHost slot is now a plain <div> (canvas removed from the fence).
  // A div has no 2D context, so the preview only shows the CSS background;
  // the runtime still projects the real 3D model via FFI onto this element.
  function fillNativeHost() {
  }

  function fitScale() {
    var root = document.querySelector('.root');
    if (!root) return;
    var rw = 1920, rh = 1080;
    var sw = (window.innerWidth - 48) / rw;
    var sh = (window.innerHeight - 48) / rh;
    var s = Math.min(sw, sh, 1);
    document.body.style.zoom = s;
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }
  function init() {
    wireNav();
    wireTabs();
    wireDialogs();
    fillListViews();
    fillNativeHost();
    fitScale();
    window.addEventListener('resize', fitScale);
  }
})();
