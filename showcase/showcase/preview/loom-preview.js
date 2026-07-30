// LoomGUI showcase browser preview driver (preview-only — packer consumes body only).
// Nav + role=tablist switch + dialog display toggle + ListView visual fill + NativeHost placeholder + body letterbox
// + control interaction driver (mirrors runtime control semantics so the browser preview behaves like Unity).
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

  // Preview-only: clone the <template> child of a role=list[data-fill] so data-driven
  // ListViews read as filled. Runtime ListView is data-driven; this is a visual aid.
  // (pivot: was ul[data-fill], now [role="list"][data-fill] per spec §2.2)
  var ITEM_ROTATION = ['item-potion', 'item-chest', 'item-gem', 'item-scroll', 'item-staff', 'item-wand'];
  function fillListViews() {
    var dir = location.href.substring(0, location.href.lastIndexOf('/') + 1) + '../res/icons/';
    document.querySelectorAll('[role="list"][data-fill]').forEach(function (list) {
      var tpl = list.querySelector('template');
      if (!tpl) return;
      var count = parseInt(list.getAttribute('data-fill'), 10) || 8;
      for (var i = 1; i < count; i++) {
        var node = tpl.content.cloneNode(true);
        var img = node.querySelector('img');
        if (img && /\/item-/.test(img.getAttribute('src') || '')) {
          img.setAttribute('src', dir + ITEM_ROTATION[i % ITEM_ROTATION.length] + '.png');
        }
        list.appendChild(node);
      }
    });
  }

  // NativeHost slot is now a plain <div> (canvas removed from the fence).
  // A div has no 2D context, so the preview only shows the CSS background;
  // the runtime still projects the real 3D model via FFI onto this element.
  //
  // Post role-pivot: controls are <div role="..."> + author-written children (spec §2.2).
  // No .loom-* injection anymore — the author already wrote fill/thumb/listbox/option/value
  // in the HTML. The preview only DRIVES them (fill width, thumb position, aria-checked toggle,
  // listbox expand, textbox editability) so the browser preview matches runtime behavior.
  function wireControls() {
    wireProgressbars();
    wireSliders();
    wireSwitchesAndRadios();
    wireComboboxes();
    wireTextboxes();
  }

  // role=progressbar → drive [data-slot=fill] width from aria-valuenow/min/max (mirrors core sync).
  function wireProgressbars() {
    document.querySelectorAll('[role="progressbar"]').forEach(function (pb) {
      var fill = pb.querySelector('[data-slot="fill"]');
      if (!fill) return;
      var min = parseFloat(pb.getAttribute('aria-valuemin')) || 0;
      var max = parseFloat(pb.getAttribute('aria-valuemax')) || 100;
      var val = parseFloat(pb.getAttribute('aria-valuenow')) || 0;
      var pct = max > min ? (val - min) / (max - min) * 100 : 0;
      fill.style.width = pct + '%';
    });
  }

  // role=slider → position [data-slot=thumb] + size [data-slot=fill] from aria-valuenow/min/max.
  // Draggable: pointer drag updates value, clamps to [min,max], quantizes to data-step.
  // Mirrors core sync_control_visuals geometry (no track layer post-pivot; slider IS the track).
  function wireSliders() {
    document.querySelectorAll('[role="slider"]').forEach(function (slider) {
      var fill = slider.querySelector('[data-slot="fill"]');
      var thumb = slider.querySelector('[data-slot="thumb"]');
      if (!fill && !thumb) return;

      function read() {
        var min = parseFloat(slider.getAttribute('aria-valuemin')) || 0;
        var max = parseFloat(slider.getAttribute('aria-valuemax')) || 100;
        var step = parseFloat(slider.getAttribute('data-step')) || 0;
        var val = parseFloat(slider.getAttribute('aria-valuenow')) || min;
        return { min: min, max: max, step: step, val: val };
      }

      function render() {
        var s = read();
        var pct = s.max > s.min ? (s.val - s.min) / (s.max - s.min) : 0;
        if (fill) fill.style.width = (pct * 100) + '%';
        if (thumb) {
          var sw = slider.clientWidth || 0;
          var tw = thumb.clientWidth || thumb.offsetWidth || 0;
          // center thumb vertically on the slider bar
          thumb.style.position = 'absolute';
          thumb.style.top = '50%';
          thumb.style.transform = 'translateY(-50%)';
          thumb.style.left = (Math.max(sw - tw, 0) * pct) + 'px';
        }
      }

      function setValueFromX(clientX) {
        var rect = slider.getBoundingClientRect();
        var ratio = rect.width > 0 ? (clientX - rect.left) / rect.width : 0;
        ratio = Math.max(0, Math.min(1, ratio));
        var s = read();
        var val = s.min + ratio * (s.max - s.min);
        if (s.step > 0) val = s.min + Math.round((val - s.min) / s.step) * s.step;
        val = Math.max(s.min, Math.min(s.max, val));
        slider.setAttribute('aria-valuenow', String(val));
        render();
        slider.dispatchEvent(new Event('input', { bubbles: true }));
      }

      var dragging = false;
      slider.addEventListener('pointerdown', function (e) {
        dragging = true;
        slider.setPointerCapture(e.pointerId);
        setValueFromX(e.clientX);
        e.preventDefault();
      });
      slider.addEventListener('pointermove', function (e) {
        if (!dragging) return;
        setValueFromX(e.clientX);
      });
      slider.addEventListener('pointerup', function (e) {
        dragging = false;
        try { slider.releasePointerCapture(e.pointerId); } catch (_) {}
      });
      render();
    });
  }

  // role=switch / role=radio → click toggles aria-checked. Radio also clears same-name siblings
  // (data-name group). Mirrors core Toggle/RadioButton state semantics.
  function wireSwitchesAndRadios() {
    document.querySelectorAll('[role="switch"]').forEach(function (sw) {
      sw.addEventListener('click', function () {
        var on = sw.getAttribute('aria-checked') === 'true';
        sw.setAttribute('aria-checked', on ? 'false' : 'true');
        sw.dispatchEvent(new Event('change', { bubbles: true }));
      });
    });
    document.querySelectorAll('[role="radio"]').forEach(function (radio) {
      radio.addEventListener('click', function () {
        var name = radio.getAttribute('data-name');
        if (name) {
          // clear siblings in the same group, then check this one
          document.querySelectorAll('[role="radio"][data-name="' + cssEsc(name) + '"]').forEach(function (s) {
            s.setAttribute('aria-checked', 'false');
          });
        }
        radio.setAttribute('aria-checked', 'true');
        radio.dispatchEvent(new Event('change', { bubbles: true }));
      });
    });
  }

  // role=combobox → click/Enter toggles aria-expanded; selecting an option writes its text into
  // [data-slot=value] and collapses. Mirrors core Dropdown open/select/close.
  function wireComboboxes() {
    document.querySelectorAll('[role="combobox"]').forEach(function (cb) {
      var valueEl = cb.querySelector('[data-slot="value"]');
      var listbox = cb.querySelector('[role="listbox"]');
      if (!listbox) return;

      function open() {
        cb.setAttribute('aria-expanded', 'true');
        listbox.style.display = 'block';
      }
      function close() {
        cb.setAttribute('aria-expanded', 'false');
        listbox.style.display = 'none';
      }
      function select(opt) {
        if (valueEl) valueEl.textContent = opt.textContent;
        cb.querySelectorAll('[role="option"]').forEach(function (o) {
          o.setAttribute('aria-selected', o === opt ? 'true' : 'false');
        });
        close();
        cb.dispatchEvent(new Event('change', { bubbles: true }));
      }

      // the whole combobox is the click target (value slot is a transparent overlay);
      // clicking anywhere on the combobox toggles, clicking an option selects.
      cb.addEventListener('click', function (e) {
        // let option clicks bubble separately (they stopPropagation)
        if (e.target.closest('[role="option"]')) return;
        e.stopPropagation();
        if (cb.getAttribute('aria-expanded') === 'true') close(); else open();
      });
      listbox.querySelectorAll('[role="option"]').forEach(function (opt) {
        opt.addEventListener('click', function (e) {
          e.stopPropagation();
          select(opt);
        });
      });
      // outside click closes
      document.addEventListener('click', function () {
        if (cb.getAttribute('aria-expanded') === 'true') close();
      });
      close(); // start collapsed
    });
  }

  // role=textbox → make it editable in the browser. <div role=textbox> is not editable by default,
  // so we set contentEditable. placeholder (aria-placeholder) shows when empty (CSS :empty).
  // Number (role=spinbutton) stays read-only display (author writes value as text content).
  function wireTextboxes() {
    document.querySelectorAll('[role="textbox"]').forEach(function (tb) {
      tb.setAttribute('contenteditable', 'true');
      // placeholder via data attribute + CSS [data-empty]: toggle on input
      function syncEmpty() {
        if (tb.textContent.trim() === '') tb.setAttribute('data-empty', 'true');
        else tb.removeAttribute('data-empty');
      }
      tb.addEventListener('input', syncEmpty);
      syncEmpty();
    });
  }

  // minimal CSS.escape polyfill for radio data-name selectors (names are simple identifiers here)
  function cssEsc(s) {
    return String(s).replace(/["\\]/g, '\\$&');
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
    wireControls();
    fitScale();
    window.addEventListener('resize', fitScale);
  }
})();
