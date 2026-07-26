// LoomGUI showcase browser preview driver (preview-only — packer consumes body only).
// Nav + role=tablist switch + dialog display toggle + ListView visual fill + NativeHost placeholder + body letterbox
// + control visual injection (mirrors core's .loom-* children so the browser preview matches the runtime).
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
  // Mirrors the runtime: core injects .loom-* visual children into control nodes so CSS
  // (not the browser UA) decides their look. Without this the browser renders <progress>/<input
  // type=range> with the green OS UA, which diverges from the Unity output and misleads designers.
  // We inject the same .loom-* structure (class only — showcase CSS paints it) and neutralize the
  // native widget via appearance:none. Interactive state (slider drag, toggle click) is wired too,
  // so the preview stays in sync as the user interacts.
  function injectControlVisuals() {
    // progress → append .loom-fill sized by value/max (like core sync_control_visuals).
    document.querySelectorAll('progress').forEach(function (p) {
      if (p.querySelector('.loom-fill')) return; // idempotent
      var max = parseFloat(p.getAttribute('max')) || 100;
      var val = parseFloat(p.getAttribute('value')) || 0;
      var fill = document.createElement('div');
      fill.className = 'loom-fill';
      fill.style.width = (val / max * 100) + '%';
      // progress UA fills its own bar; reset so only .loom-fill shows the fill color.
      p.style.appearance = 'none';
      p.style.MozAppearance = 'none';
      p.style.webkitAppearance = 'none';
      p.appendChild(fill);
    });

    // input[type=range] → .loom-track > .loom-fill + sibling .loom-thumb (core inject layout).
    document.querySelectorAll('input[type="range"]').forEach(function (r) {
      if (r.querySelector('.loom-track')) return;
      r.style.appearance = 'none';
      r.style.MozAppearance = 'none';
      r.style.webkitAppearance = 'none';
      var track = document.createElement('div');
      track.className = 'loom-track';
      var fill = document.createElement('div');
      fill.className = 'loom-fill';
      var thumb = document.createElement('div');
      thumb.className = 'loom-thumb';
      track.appendChild(fill);
      // core structure: slider → [track, thumb]; track → [fill]. thumb is a sibling of track.
      r.appendChild(track);
      r.appendChild(thumb);
      positionRangeThumb(r, track, fill, thumb);
      r.addEventListener('input', function () { positionRangeThumb(r, track, fill, thumb); });
    });

    // checkbox/radio → .loom-check (visibility mirrors checked state, like core).
    document.querySelectorAll('input[type="checkbox"], input[type="radio"]').forEach(function (c) {
      if (c.querySelector('.loom-check')) return;
      c.style.appearance = 'none';
      c.style.MozAppearance = 'none';
      c.style.webkitAppearance = 'none';
      var check = document.createElement('div');
      check.className = 'loom-check';
      check.style.display = c.checked ? '' : 'none';
      c.appendChild(check);
      c.addEventListener('change', function () { check.style.display = c.checked ? '' : 'none'; });
    });
  }

  // Position the range thumb like core sync_control_visuals: traversable = track_w - thumb_w,
  // left = traversable * pct. Reads computed px (after CSS sizing) to stay layout-accurate.
  function positionRangeThumb(range, track, fill, thumb) {
    var min = parseFloat(range.getAttribute('min')) || 0;
    var max = parseFloat(range.getAttribute('max')) || 100;
    var val = parseFloat(range.getAttribute('value')) || 0;
    var pct = max > min ? (val - min) / (max - min) : 0;
    var tw = track.clientWidth || 0;
    var th = thumb.clientWidth || 0;
    var traversable = Math.max(tw - th, 0);
    thumb.style.position = 'absolute';
    thumb.style.left = (traversable * pct) + 'px';
    fill.style.width = (pct * 100) + '%';
  }

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
    injectControlVisuals();
    fitScale();
    window.addEventListener('resize', fitScale);
  }
})();
