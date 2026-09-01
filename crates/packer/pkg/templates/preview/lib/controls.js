// 控件/交互模拟（框架真相副本，嵌在 ikat 二进制里由 preview server 路由供给；
// 消费侧经 `/ikat-preview/lib/controls.js` 导入，不拷贝、跟 CLI 版本走）。
// 镜像 core 的控件语义（sync_control_visuals 家族）：预览里控件要点得动、状态
// 要长得对，人类预览才不骗人。作者在 HTML 里写好结构（fill/thumb/listbox/...），
// 这里只负责驱动。

// role=progressbar → 按 aria-valuenow 驱动 [data-slot=fill] 宽度。口径 = core
// sync_control_visuals 的 (value-min)/(max-min).clamp(0,1)（ARIA 语义，#97；
// min 缺省 0 时与 value/max 等价——预览镜像 core）。
export function wireProgressbars() {
  document.querySelectorAll('[role="progressbar"]').forEach((pb) => {
    const fill = pb.querySelector('[data-slot="fill"]');
    if (!fill) return;
    const min = parseFloat(pb.getAttribute('aria-valuemin')) || 0;
    const max = parseFloat(pb.getAttribute('aria-valuemax')) || 100;
    const val = parseFloat(pb.getAttribute('aria-valuenow')) || 0;
    const pct = max > min ? Math.min(1, Math.max(0, (val - min) / (max - min))) * 100 : 0;
    fill.style.width = pct + '%';
  });
}

// role=slider → 定位 [data-slot=thumb] + [data-slot=fill] 宽度（aria-valuenow 驱动）。
// 可拖：pointer 拖动改值、夹 [min,max]、按 data-step 量化。slider 本体就是轨道。
export function wireSliders() {
  document.querySelectorAll('[role="slider"]').forEach((slider) => {
    const fill = slider.querySelector('[data-slot="fill"]');
    const thumb = slider.querySelector('[data-slot="thumb"]');
    if (!fill && !thumb) return;

    const read = () => ({
      min: parseFloat(slider.getAttribute('aria-valuemin')) || 0,
      max: parseFloat(slider.getAttribute('aria-valuemax')) || 100,
      step: parseFloat(slider.getAttribute('data-step')) || 0,
      val: parseFloat(slider.getAttribute('aria-valuenow')) || 0,
    });
    const render = () => {
      const s = read();
      const pct = s.max > s.min ? (s.val - s.min) / (s.max - s.min) : 0;
      if (fill) fill.style.width = pct * 100 + '%';
      if (thumb) {
        const sw = slider.clientWidth || 0;
        const tw = thumb.clientWidth || thumb.offsetWidth || 0;
        // thumb 垂直居中在轨道上
        thumb.style.position = 'absolute';
        thumb.style.top = '50%';
        thumb.style.transform = 'translateY(-50%)';
        thumb.style.left = Math.max(sw - tw, 0) * pct + 'px';
      }
    };
    const setValueFromX = (clientX) => {
      const rect = slider.getBoundingClientRect();
      let ratio = rect.width > 0 ? (clientX - rect.left) / rect.width : 0;
      ratio = Math.max(0, Math.min(1, ratio));
      const s = read();
      let val = s.min + ratio * (s.max - s.min);
      if (s.step > 0) val = s.min + Math.round((val - s.min) / s.step) * s.step;
      val = Math.max(s.min, Math.min(s.max, val));
      slider.setAttribute('aria-valuenow', String(val));
      render();
      slider.dispatchEvent(new Event('input', { bubbles: true }));
    };

    let dragging = false;
    slider.addEventListener('pointerdown', (e) => {
      dragging = true;
      slider.setPointerCapture(e.pointerId);
      setValueFromX(e.clientX);
      e.preventDefault();
    });
    slider.addEventListener('pointermove', (e) => {
      if (dragging) setValueFromX(e.clientX);
    });
    slider.addEventListener('pointerup', (e) => {
      dragging = false;
      try {
        slider.releasePointerCapture(e.pointerId);
      } catch (_) { /* 已释放 */ }
    });
    render();
  });
}

// role=switch / role=radio → 点击翻转 aria-checked；radio 另清同组（data-name）。
export function wireSwitchesAndRadios() {
  document.querySelectorAll('[role="switch"]').forEach((sw) => {
    sw.addEventListener('click', () => {
      const on = sw.getAttribute('aria-checked') === 'true';
      sw.setAttribute('aria-checked', on ? 'false' : 'true');
      sw.dispatchEvent(new Event('change', { bubbles: true }));
    });
  });
  document.querySelectorAll('[role="radio"]').forEach((radio) => {
    radio.addEventListener('click', () => {
      const name = radio.getAttribute('data-name');
      if (name) {
        document
          .querySelectorAll('[role="radio"][data-name="' + cssEsc(name) + '"]')
          .forEach((s) => s.setAttribute('aria-checked', 'false'));
      }
      radio.setAttribute('aria-checked', 'true');
      radio.dispatchEvent(new Event('change', { bubbles: true }));
    });
  });
}

// role=combobox → 点击/回车开合 aria-expanded；选中项文本写入 [data-slot=value]
// 并收起。整个 combobox 是点击目标（value 槽是透明覆盖）；点 option 是选择。
export function wireComboboxes() {
  document.querySelectorAll('[role="combobox"]').forEach((cb) => {
    const valueEl = cb.querySelector('[data-slot="value"]');
    const listbox = cb.querySelector('[role="listbox"]');
    if (!listbox) return;

    const open = () => {
      cb.setAttribute('aria-expanded', 'true');
      listbox.style.display = 'block';
    };
    const close = () => {
      cb.setAttribute('aria-expanded', 'false');
      listbox.style.display = 'none';
    };
    const select = (opt) => {
      if (valueEl) valueEl.textContent = opt.textContent;
      cb.querySelectorAll('[role="option"]').forEach((o) => {
        o.setAttribute('aria-selected', o === opt ? 'true' : 'false');
      });
      close();
      cb.dispatchEvent(new Event('change', { bubbles: true }));
    };

    cb.addEventListener('click', (e) => {
      if (e.target.closest('[role="option"]')) return; // option 自行处理
      e.stopPropagation();
      if (cb.getAttribute('aria-expanded') === 'true') close();
      else open();
    });
    listbox.querySelectorAll('[role="option"]').forEach((opt) => {
      opt.addEventListener('click', (e) => {
        e.stopPropagation();
        select(opt);
      });
    });
    document.addEventListener('click', () => {
      if (cb.getAttribute('aria-expanded') === 'true') close();
    });
    close(); // 起始收起
  });
}

// role=spinbutton（NumberField）→ aria-valuenow 渲染为文本；滚轮/方向键/点按键入
// 调值，blur/Enter 提交（解析、量化、夹取）。缺省 min/max 无界（镜像 core 的
// f32::MIN/MAX——旧版缺 aria-valuemax 时 max=0，任何交互都被钳死到 0）；缺省
// step=0 不量化（core 语义），方向键步进用 1 兜底。
export function wireSpinbuttons() {
  document.querySelectorAll('[role="spinbutton"]').forEach((sb) => {
    const bound = (attr, fallback) => {
      const v = parseFloat(sb.getAttribute(attr));
      return Number.isNaN(v) ? fallback : v;
    };
    const read = () => ({
      min: bound('aria-valuemin', -Infinity),
      max: bound('aria-valuemax', Infinity),
      step: bound('data-step', 0),
      val: parseFloat(sb.getAttribute('aria-valuenow')) || 0,
    });
    const render = () => {
      sb.textContent = String(read().val);
    };
    const setVal = (v) => {
      const s = read();
      let nv = v;
      if (s.step > 0) {
        // 量化锚点：有界 min 锚 min（保持旧网格语义），无界退 0——锚 -Infinity
        // 会把 (v - min) 做成 Infinity、整个量化 NaN。
        const anchor = Number.isFinite(s.min) ? s.min : 0;
        nv = anchor + Math.round((v - anchor) / s.step) * s.step;
      }
      nv = Math.max(s.min, Math.min(s.max, nv));
      sb.setAttribute('aria-valuenow', String(nv));
      render();
      sb.dispatchEvent(new Event('input', { bubbles: true }));
    };
    render();
    sb.addEventListener(
      'wheel',
      (e) => {
        e.preventDefault();
        const s = read();
        setVal(s.val + (e.deltaY < 0 ? s.step || 1 : -(s.step || 1)));
      },
      { passive: false },
    );
    sb.setAttribute('tabindex', '0');
    sb.setAttribute('contenteditable', 'true');
    sb.addEventListener('keydown', (e) => {
      const s = read();
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        setVal(s.val + (s.step || 1));
      } else if (e.key === 'ArrowDown') {
        e.preventDefault();
        setVal(s.val - (s.step || 1));
      }
    });
    const commit = () => {
      const raw = parseFloat(sb.textContent);
      if (!Number.isNaN(raw)) setVal(raw);
      else render();
    };
    sb.addEventListener('blur', commit);
    sb.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') {
        e.preventDefault();
        commit();
        sb.blur();
      }
    });
  });
}

// role=textbox → contenteditable 使其可编辑；空文案挂 data-empty（placeholder 由
// preview-base.css 的 [data-empty]::before 呈现——空 textbox 的占位行高必须与 core
// 一致，否则 rect-diff 假 diff）。
export function wireTextboxes() {
  document.querySelectorAll('[role="textbox"]').forEach((tb) => {
    tb.setAttribute('contenteditable', 'true');
    const syncEmpty = () => {
      if (tb.textContent.trim() === '') tb.setAttribute('data-empty', 'true');
      else tb.removeAttribute('data-empty');
    };
    tb.addEventListener('input', syncEmpty);
    syncEmpty();
  });
}

// role=tab → 激活态应用（与 core sync_control_visuals 同语义）：目标 panel 清
// inline display 回落作者 CSS（''——不写死 block，作者布局不被覆写），其余
// 'none' 剪枝。HTML 里非激活 panel 不写 style="display:none"（显隐所有权归控件，
// 首帧剪枝由本初始化接管）。
export function wireTabs() {
  const applyTabState = (activeTab) => {
    const target = activeTab.getAttribute('aria-controls');
    document.querySelectorAll('[role="tabpanel"]').forEach((p) => {
      p.style.display = p.id === target ? '' : 'none';
    });
  };
  document.querySelectorAll('[role="tab"]').forEach((tab) => {
    tab.addEventListener('click', () => {
      const list = tab.closest('[role="tablist"]');
      if (!list) return;
      list.querySelectorAll('[role="tab"]').forEach((t) => {
        t.setAttribute('aria-selected', 'false');
      });
      tab.setAttribute('aria-selected', 'true');
      applyTabState(tab);
    });
  });
  const initial =
    document.querySelector('[role="tab"][aria-selected="true"]') ||
    document.querySelector('[role="tab"]');
  if (initial) applyTabState(initial);
}

// role=tree/treeitem → 树语义（镜像 core scene/control/tree，#8）：单击条目 = 选中 +
// branch 折叠/展开互切；选中/展开/层级是 synth aria（作者 HTML 不写 aria-selected/
// aria-expanded=false/aria-level——运行时合成），浏览器里由本函数补戳同名属性使
// [aria-selected]/[aria-expanded]/[aria-level] 选择器与 core 命中一致。
// 初始态：aria-expanded="true" 的 branch 展开、其余折叠；初始选中 = 首个
// aria-selected="true" 条目（作者写）或首个条目（缺省，core ControlInit 同语义）。
export function wireTrees() {
  const isItem = (el) => el && el.getAttribute('role') === 'treeitem';
  const directItems = (el) => Array.from(el.children).filter(isItem);
  const itemsInOrder = (root) => {
    const out = [];
    const walk = (el) => {
      for (const c of el.children) {
        if (isItem(c)) { out.push(c); walk(c); }
        else walk(c);
      }
    };
    walk(root);
    return out;
  };
  const branchOf = (item) => directItems(item).length > 0;
  const setExpanded = (item, expanded) => {
    item.setAttribute('aria-expanded', expanded ? 'true' : 'false');
    directItems(item).forEach((c) => { c.style.display = expanded ? '' : 'none'; });
  };
  const select = (item) => {
    const tree = item.closest('[role="tree"]');
    if (!tree) return;
    itemsInOrder(tree).forEach((i) => i.setAttribute('aria-selected', 'false'));
    item.setAttribute('aria-selected', 'true');
  };
  document.querySelectorAll('[role="tree"]').forEach((tree) => {
    const items = itemsInOrder(tree);
    // aria-level 补戳（层级 = 嵌套深度，顶层 = 1——core tree_item_level 同口径）。
    const stampLevel = (el, level) => {
      for (const c of el.children) {
        if (isItem(c)) { c.setAttribute('aria-level', String(level)); stampLevel(c, level + 1); }
        else stampLevel(c, level);
      }
    };
    stampLevel(tree, 1);
    // 初始选中（作者 aria-selected 或首项）。
    const initial = tree.querySelector('[role="treeitem"][aria-selected="true"]') || items[0];
    if (initial) select(initial);
  });
  document.querySelectorAll('[role="treeitem"]').forEach((item) => {
    // 初始折叠剪枝（aria-expanded 缺省 = 折叠，core ControlInit 同语义）。
    if (branchOf(item)) setExpanded(item, item.getAttribute('aria-expanded') === 'true');
    item.addEventListener('click', (e) => {
      // 命中在 label/图标子上也归条目（core find_control_at 上溯同语义）。
      e.stopPropagation();
      select(item);
      if (branchOf(item)) setExpanded(item, item.getAttribute('aria-expanded') !== 'true');
    });
  });
}

export function wireDialogs() {
  document.querySelectorAll('[data-open-dialog]').forEach((btn) => {
    btn.addEventListener('click', () => {
      const dlg = document.getElementById(btn.getAttribute('data-open-dialog'));
      if (dlg) dlg.style.display = '';
    });
  });
  document.querySelectorAll('[data-close-dialog]').forEach((btn) => {
    btn.addEventListener('click', () => {
      const dlg = btn.closest('[role="dialog"]');
      if (dlg) dlg.style.display = 'none';
    });
  });
}

// radio data-name 选择器的最小转义（组名是简单标识符场景）。
function cssEsc(s) {
  return String(s).replace(/["\\]/g, '\\$&');
}
