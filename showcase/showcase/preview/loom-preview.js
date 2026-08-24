// LoomGUI showcase browser preview driver (preview-only — packer consumes body only).
// Nav + role=tablist switch + dialog display toggle + ListView visual fill + NativeHost placeholder + body letterbox
// + control interaction driver (mirrors runtime control semantics so the browser preview behaves like Unity).
// Classic script (non-ES module) to avoid file:// CORS.
(function () {
  'use strict';

  // Inject preview-base.css (the browser-side polyfill: @font-face fonts, body
  // letterbox, box-sizing, button reset). It must ride the script channel: the
  // fence validates every <link rel="stylesheet"> against the CSS subset, and a
  // polyfill is intentionally full of out-of-fence declarations. Script tags are
  // shell-consumed at build time, so the packer never sees this. Appending here
  // (script sits in <head> before the page <style>) keeps the original cascade
  // order: polyfill first, page styles after.
  var link = document.createElement('link');
  link.rel = 'stylesheet';
  link.href = 'preview/preview-base.css';
  document.head.appendChild(link);

  var NAV = {
    'nav-settings': 'settings',
    'nav-inventory': 'inventory',
    'nav-mail': 'mail',
    'nav-shop': 'shop',
    'nav-character': 'character',
    'nav-form': 'form',
    'nav-lab': 'lab',
    'nav-anim': 'm2-animation',
    'nav-infra': 'api-infra'
  };

  function $(id) { return document.getElementById(id); }
  function bind(id, type, fn) { var el = $(id); if (el) el.addEventListener(type, fn); }


  // Custom Element expansion (mirrors the packer's pack-time expansion so the
  // browser preview shows the same tree core lays out). Registry comes from
  // window.__LOOM_COMPONENTS__ (injected by rect-diff's browser-rect.mjs from
  // the components/ dir; manual file:// preview bootstraps it from the
  // generated preview/components-registry.js — see ensureRegistry below).
  //
  // Semantics (component-system spec): host keeps its place/attrs; the component
  // template root is appended under it; <slot name=x> is REPLACED at its splice
  // position by the host light children with slot="x" (fallback children kept
  // when nothing is assigned); whitespace-only light children are dropped.
  // Nested components (inside templates or projected content) expand in further
  // passes until fixpoint.
  function expandComponents() {
    var reg = window.__LOOM_COMPONENTS__;
    if (!reg) return;
    var passes = 0;
    while (passes++ < 16) {
      var hosts = Array.prototype.filter.call(
        document.querySelectorAll('*'),
        function (el) {
          var name = el.tagName.toLowerCase();
          return name.indexOf('-') >= 0 && reg[name] && !el.hasAttribute('data-loom-expanded');
        }
      );
      if (!hosts.length) return;
      hosts.forEach(function (host) {
        host.setAttribute('data-loom-expanded', '');
        var name = host.tagName.toLowerCase();
        var doc = new DOMParser().parseFromString(reg[name], 'text/html');
        Array.prototype.forEach.call(doc.querySelectorAll('style'), function (st) {
          injectComponentStyle(name, st.textContent);
          st.remove();
        });
        // 模板内相对 URL（src/href）按组件文件位置（components/<name>.html）解析成绝对
        // URL——镜像打包器的 html_rel 归一。不重写则按页面位置解析，fallback 图 404。
        var compBase = new URL('components/' + name + '.html', document.baseURI);
        Array.prototype.forEach.call(doc.querySelectorAll('[src],[href]'), function (el) {
          var attr = el.hasAttribute('src') ? 'src' : 'href';
          el.setAttribute(attr, new URL(el.getAttribute(attr), compBase).href);
        });
        var root = doc.body.firstElementChild;
        if (!root) return;
        root.setAttribute('data-loom-comp', name);
        // Slot assignment from host light children (slot attr; whitespace text
        // dropped). Nodes are detached by projection (insertBefore moves them).
        var assign = {};
        var defaults = [];
        Array.prototype.slice.call(host.childNodes).forEach(function (ch) {
          if (ch.nodeType === 3) {
            if (ch.textContent.trim()) defaults.push(ch);
            return;
          }
          if (ch.nodeType !== 1) return;
          var sn = ch.getAttribute && ch.getAttribute('slot');
          if (sn) (assign[sn] = assign[sn] || []).push(ch);
          else defaults.push(ch);
        });
        var imported = document.importNode(root, true);
        projectSlots(imported, assign, defaults);
        host.appendChild(imported);
        // Unassigned leftovers (invalid slot — packer errors at build time):
        // re-attach to host so they stay measurable in the degraded preview.
        Object.keys(assign).forEach(function (k) {
          assign[k].forEach(function (n) {
            if (!n.parentNode) host.appendChild(n);
          });
        });
      });
    }
  }

  // Replace <slot> elements under root with assigned light children (LIVE nodes
  // move — listeners/state preserved), or with the slot's fallback children when
  // unassigned. assign: {name: [nodes]}, defaults: [nodes].
  function projectSlots(root, assign, defaults) {
    Array.prototype.forEach.call(root.querySelectorAll('slot'), function (slot) {
      var name = slot.getAttribute('name');
      var kids = name != null ? assign[name] || [] : defaults;
      var parent = slot.parentNode;
      if (!parent) return;
      if (kids.length) {
        kids.forEach(function (k) { parent.insertBefore(k, slot); });
      } else {
        Array.prototype.slice.call(slot.childNodes).forEach(function (c) {
          parent.insertBefore(c, slot);
        });
      }
      slot.remove();
    });
  }

  // Component <style> with per-component selector prefix ([data-loom-comp="name"]) —
  // preview-side scope emulation (core scopes rules per expansion instance).
  // @-rules pass through untouched (no element selectors to prefix).
  function injectComponentStyle(name, css) {
    if (!css || !css.trim()) return;
    var out = css.replace(/([^{}]+)\{/g, function (m, sel) {
      var trimmed = sel.trim();
      if (trimmed.charAt(0) === '@') return m;
      var prefixed = trimmed
        .split(',')
        .map(function (part) { return '[data-loom-comp="' + name + '"] ' + part.trim(); })
        .join(', ');
      return prefixed + ' {';
    });
    var st = document.createElement('style');
    st.setAttribute('data-loom-comp-style', name);
    st.textContent = out;
    document.head.appendChild(st);
  }

  function goPage(name) {
    var dir = location.href.substring(0, location.href.lastIndexOf('/') + 1);
    location.href = dir + name + '.html';
  }

  function wireNav() {
    Object.keys(NAV).forEach(function (id) {
      bind(id, 'click', function () { goPage(NAV[id]); });
    });
    bind('back-home', 'click', function () { goPage('home'); });
    // m2-animation 页内「↻ 重播」：浏览器侧等价语义 = 原地重启全部 CSS 动画
    // （Unity 侧走 dispose + 重实例化；预览不重导航，避免打断对照观察）。
    bind('btn-replay', 'click', function () {
      var els = document.querySelectorAll('.root, .root *');
      Array.prototype.forEach.call(els, function (el) {
        if (getComputedStyle(el).animationName !== 'none') {
          el.style.animation = 'none';
          void el.offsetWidth;
          el.style.animation = '';
        }
      });
    });
  }

    function wireTabs() {
      var tabs = document.querySelectorAll('[role="tab"]');
      // 激活态应用（与 core sync_control_visuals 同语义）：目标 panel 清 inline display
      // 回落作者 CSS（''——不写死 block，作者 flex/grid 布局不被覆写），其余 'none' 剪枝。
      // 初始化 + click 共用：HTML 里非激活 panel 不写 style="display:none"（显隐所有权
      // 归控件，首帧剪枝接管），浏览器侧由本初始化补齐首帧显隐。
      function applyTabState(activeTab) {
        var target = activeTab.getAttribute('aria-controls');
        document.querySelectorAll('[role="tabpanel"]').forEach(function (p) {
          p.style.display = (p.id === target) ? '' : 'none';
        });
      }
      tabs.forEach(function (tab) {
        tab.addEventListener('click', function () {
          var list = tab.closest('[role="tablist"]');
          if (!list) return;
          list.querySelectorAll('[role="tab"]').forEach(function (t) {
            t.setAttribute('aria-selected', 'false');
          });
          tab.setAttribute('aria-selected', 'true');
          applyTabState(tab);
        });
      });
      // 初始化：按 aria-selected=true 的首个 tab 设一遍 panel 显隐（无则第一个 tab）。
      var initial = document.querySelector('[role="tab"][aria-selected="true"]')
        || document.querySelector('[role="tab"]');
      if (initial) applyTabState(initial);
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
    wireSpinbuttons();
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

  // role=spinbutton (NumberField) → render aria-valuenow as text content, support up/down
  // adjustment (wheel, ArrowUp/ArrowDown, click+type). Mirrors core NumberField semantics.
  // The author writes aria-valuenow/min/max + data-step; the preview keeps the div's text in sync.
  function wireSpinbuttons() {
    document.querySelectorAll('[role="spinbutton"]').forEach(function (sb) {
      function read() {
        var min = parseFloat(sb.getAttribute('aria-valuemin')) || 0;
        var max = parseFloat(sb.getAttribute('aria-valuemax')) || 0;
        var step = parseFloat(sb.getAttribute('data-step')) || 1;
        var val = parseFloat(sb.getAttribute('aria-valuenow')) || min;
        return { min: min, max: max, step: step, val: val };
      }
      function render() {
        var s = read();
        sb.textContent = String(s.val);
      }
      function setVal(v) {
        var s = read();
        var nv = s.min + Math.round((v - s.min) / s.step) * s.step;
        nv = Math.max(s.min, Math.min(s.max, nv));
        sb.setAttribute('aria-valuenow', String(nv));
        render();
        sb.dispatchEvent(new Event('input', { bubbles: true }));
      }
      render();
      // wheel adjusts by step
      sb.addEventListener('wheel', function (e) {
        e.preventDefault();
        var s = read();
        setVal(s.val + (e.deltaY < 0 ? s.step : -s.step));
      }, { passive: false });
      // keyboard: ArrowUp/Down, also type digits
      sb.setAttribute('tabindex', '0');
      sb.setAttribute('contenteditable', 'true');
      sb.addEventListener('keydown', function (e) {
        var s = read();
        if (e.key === 'ArrowUp') { e.preventDefault(); setVal(s.val + s.step); }
        else if (e.key === 'ArrowDown') { e.preventDefault(); setVal(s.val - s.step); }
      });
      // commit typed text on blur/Enter: parse as number, clamp+quantize
      function commit() {
        var raw = parseFloat(sb.textContent);
        if (!isNaN(raw)) setVal(raw); else render();
      }
      sb.addEventListener('blur', commit);
      sb.addEventListener('keydown', function (e) { if (e.key === 'Enter') { e.preventDefault(); commit(); sb.blur(); } });
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
    ensureRegistry(function () {
      expandComponents();
      wireNav();
      wireTabs();
      wireDialogs();
      fillListViews();
      fillNativeHost();
      wireControls();
      fitScale();
      installAnimReplay();
      window.addEventListener('resize', fitScale);
    });
  }

  // 「重播动画」悬浮按钮（preview-only）：把页面上所有 CSS animation 重置重播——
  // 入场/fill-mode/delay 标本验收时反复对比用。restart 手法：内联 animation:none →
  // 强制 reflow → 移除内联，浏览器按原 class 声明重新起播。
  function installAnimReplay() {
    var btn = document.createElement('button');
    btn.id = 'loom-anim-replay';
    btn.textContent = '↻ 重播动画';
    btn.addEventListener('click', function () {
      var els = document.querySelectorAll('.root, .root *');
      Array.prototype.forEach.call(els, function (el) {
        if (getComputedStyle(el).animationName !== 'none') {
          el.style.animation = 'none';
          void el.offsetWidth;
          el.style.animation = '';
        }
      });
    });
    document.body.appendChild(btn);
  }

  // Manual file:// preview has no rect-diff injection; load the generated
  // components-registry.js (next to this script) before expanding. A missing
  // registry file degrades to the old unexpanded preview.
  function ensureRegistry(done) {
    if (window.__LOOM_COMPONENTS__) return done();
    var base = (document.currentScript && document.currentScript.src) ||
      document.querySelector('script[src*="loom-preview.js"]').src;
    var s = document.createElement('script');
    s.src = base.replace(/[^/]*$/, 'components-registry.js');
    s.onload = done;
    s.onerror = done;
    document.head.appendChild(s);
  }
})();
