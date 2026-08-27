// 组件展开模拟（框架真相副本，嵌在 ikat 二进制里由 preview server 路由供给；
// 消费侧经 `/ikat-preview/lib/expand.js` 导入，不拷贝、跟 CLI 版本走）。
// 镜像打包期 Custom Element 展开：宿主保留位置/属性；模板根 append 到宿主下；
// <slot name=x> 在拼接位被宿主 light children（slot="x"）替换（无投射内容时保留
// slot 的 fallback 子女）；纯空白文本节点丢弃；嵌套组件迭代展开至不动点（≤16 pass）。
// 组件清单来自 server 的 /api/workspace.json（与打包同一套扫描口径）。

export async function fetchRegistry() {
  const api = await fetch('/api/workspace.json').then((r) => r.json());
  const entries = Object.entries(api.components || {});
  const reg = {};
  await Promise.all(
    entries.map(async ([name, rel]) => {
      // rel 是工作区相对路径（如 showcase/components/item-card.html）；
      // server 把工作区挂载在 /ws/ 下。
      reg[name] = await fetch('/ws/' + rel).then((r) => r.text());
    }),
  );
  return reg;
}

export function expandComponents(reg) {
  let passes = 0;
  while (passes++ < 16) {
    const hosts = Array.from(document.querySelectorAll('*')).filter((el) => {
      const name = el.tagName.toLowerCase();
      return name.indexOf('-') >= 0 && reg[name] && !el.hasAttribute('data-ikat-expanded');
    });
    if (!hosts.length) return;
    for (const host of hosts) {
      host.setAttribute('data-ikat-expanded', '');
      const name = host.tagName.toLowerCase();
      const doc = new DOMParser().parseFromString(reg[name], 'text/html');
      for (const st of Array.from(doc.querySelectorAll('style'))) {
        injectComponentStyle(name, st.textContent);
        st.remove();
      }
      // 模板内相对 URL（src/href）按组件文件位置（components/<name>.html）解析成绝对
      // URL——镜像打包器的 html_rel 归一。不重写则按页面位置解析，fallback 图 404。
      const compBase = new URL('components/' + name + '.html', document.baseURI);
      for (const el of Array.from(doc.querySelectorAll('[src],[href]'))) {
        const attr = el.hasAttribute('src') ? 'src' : 'href';
        el.setAttribute(attr, new URL(el.getAttribute(attr), compBase).href);
      }
      const root = doc.body.firstElementChild;
      if (!root) continue;
      root.setAttribute('data-ikat-comp', name);
      // Slot 分配自宿主 light children（slot 属性；纯空白文本丢弃）。投射是节点
      // 移动（insertBefore）——监听器/状态保留。
      const assign = {};
      const defaults = [];
      for (const ch of Array.from(host.childNodes)) {
        if (ch.nodeType === 3) {
          if (ch.textContent.trim()) defaults.push(ch);
          continue;
        }
        if (ch.nodeType !== 1) continue;
        const sn = ch.getAttribute && ch.getAttribute('slot');
        if (sn) (assign[sn] = assign[sn] || []).push(ch);
        else defaults.push(ch);
      }
      const imported = document.importNode(root, true);
      projectSlots(imported, assign, defaults);
      host.appendChild(imported);
      observeHostState(host, imported);
      // 悬空投影（打包期会报错）：重新挂回宿主，退化预览里仍可测量。
      for (const nodes of Object.values(assign)) {
        for (const n of nodes) {
          if (!n.parentNode) host.appendChild(n);
        }
      }
    }
  }
}

// 把 root 下的 <slot> 替换为分到的 light children（无分配时回落 slot 的 fallback
// 子女）。assign: {name: [nodes]}，defaults: [nodes]。
function projectSlots(root, assign, defaults) {
  for (const slot of Array.from(root.querySelectorAll('slot'))) {
    const name = slot.getAttribute('name');
    const kids = name != null ? assign[name] || [] : defaults;
    const parent = slot.parentNode;
    if (!parent) continue;
    if (kids.length) {
      for (const k of kids) parent.insertBefore(k, slot);
    } else {
      for (const c of Array.from(slot.childNodes)) parent.insertBefore(c, slot);
    }
    slot.remove();
  }
}

// ---- 宿主状态镜像 ----
//
// 组件惯用「宿主标签开头」的规则寻址自身态（`skill-slot.is-press .slot`）——core
// 里这条匹配 host 节点（标签保留、状态类挂 host）再下探。预览没有 :host 伪类，
// 改写选择器后由模板根承接：把 host 的 class 与 data-*/aria-* 属性镜像到根
// （data-ikat-comp 载体），使改写后的规则与 core 判定一致（Tripawd 狗粮实证：
// 无镜像时整套状态样式在预览里静默全坏）。
//
// 合并而非覆盖：根自身的类保留在前（组件样式链可能锚根自身类），host 类追加在后；
// 同名冲突时 CSS 以样式表语义裁决，属可接受近似。host 永远是真源——镜像只读，
// 不回写。

function mirrorHostState(host, root) {
  const hostCls = host.className;
  // 追加去重：host 类串原样整体追加（不逐 token 拆分，避免与根自身 token 撞名时
  // 误删）；host 清空类名的罕见路径不做缩减——镜像只增不减是可接受的近似。
  if (hostCls && !root.className.includes(hostCls)) {
    root.className = (root.className ? root.className + ' ' : '') + hostCls;
  }
  for (const attr of Array.from(host.attributes)) {
    if (/^(data|aria)-/.test(attr.name)) root.setAttribute(attr.name, attr.value);
  }
}

function observeHostState(host, root) {
  if (typeof MutationObserver === 'undefined') return;
  const attrs = ['class'].concat(
    Array.from(host.attributes)
      .filter((a) => /^(data|aria)-/.test(a.name))
      .map((a) => a.name),
  );
  new MutationObserver(() => mirrorHostState(host, root)).observe(host, {
    attributes: true,
    attributeFilter: attrs,
  });
}

// ---- 组件 <style> 作用域前缀 ----
//
// 每实例一条前缀 `[data-ikat-comp="name"]` 预览作用域模拟。@keyframes 内部的帧
// 选择器（0%/from/to）不是元素选择器，必须原样放行（Tripawd 狗粮实证：朴素前缀
// 把 `0% {` 改写成非法选择器 → 浏览器整帧丢弃 → 组件动画全灭）；@media 等外层
// @-rule 维持旧约定原样放行（内部规则不加前缀）。宿主标签开头的复合链（见上节）
// 吃掉链首 TAG 段后同样加前缀。

function injectComponentStyle(name, css) {
  if (!css || !css.trim()) return;
  const out = splitKeyframes(css)
    .map(({ raw, text }) =>
      raw ? text : text.replace(/([^{}]+)\{/g, (m, sel) => prefixSelector(name, sel)),
    )
    .join('');
  const st = document.createElement('style');
  st.setAttribute('data-ikat-comp-style', name);
  st.textContent = out;
  document.head.appendChild(st);
}

function escapeRe(s) {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function prefixSelector(name, sel) {
  const trimmed = sel.trim();
  if (trimmed.charAt(0) === '@') return sel; // @media 等外层 @-rule 原样放行
  const hostLed = new RegExp('^' + escapeRe(name) + '(?=[.:[]|\\s|$)');
  const prefixed = trimmed
    .split(',')
    .map((part) => part.replace(hostLed, '').trim())
    .map((part) =>
      part ? '[data-ikat-comp="' + name + '"] ' + part : '[data-ikat-comp="' + name + '"]',
    )
    .join(', ');
  return prefixed + ' {';
}

// 按 `@keyframes 名 {...}` 坐标切段：raw=true 的段原样保留（平衡大括号跟踪覆盖
// 内嵌百分比块），其余段交给朴素前缀器。
function splitKeyframes(css) {
  const segs = [];
  let i = 0;
  const re = /@(?:-webkit-)?keyframes\b/g;
  let m;
  while ((m = re.exec(css))) {
    if (m.index > i) segs.push({ raw: false, text: css.slice(i, m.index) });
    let depth = 0;
    let j = m.index;
    let started = false;
    for (; j < css.length; j++) {
      if (css[j] === '{') { depth++; started = true; }
      else if (css[j] === '}') { depth--; if (started && depth === 0) { j++; break; } }
    }
    segs.push({ raw: true, text: css.slice(m.index, j) });
    i = j;
    re.lastIndex = i;
  }
  if (i < css.length) segs.push({ raw: false, text: css.slice(i) });
  return segs;
}
