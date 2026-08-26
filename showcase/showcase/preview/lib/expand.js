// 组件展开模拟（AI 手写预览脚本，loom preview server 注入；不进打包）。
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
      return name.indexOf('-') >= 0 && reg[name] && !el.hasAttribute('data-loom-expanded');
    });
    if (!hosts.length) return;
    for (const host of hosts) {
      host.setAttribute('data-loom-expanded', '');
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
      root.setAttribute('data-loom-comp', name);
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

// 组件 <style> 加 per-component 选择器前缀（[data-loom-comp="name"]）——预览侧
// 作用域模拟（core 按展开实例作用域规则）。@-rule 原样放行（无元素选择器可前缀）。
function injectComponentStyle(name, css) {
  if (!css || !css.trim()) return;
  const out = css.replace(/([^{}]+)\{/g, (m, sel) => {
    const trimmed = sel.trim();
    if (trimmed.charAt(0) === '@') return m;
    const prefixed = trimmed
      .split(',')
      .map((part) => `[data-loom-comp="${name}"] ${part.trim()}`)
      .join(', ');
    return prefixed + ' {';
  });
  const st = document.createElement('style');
  st.setAttribute('data-loom-comp-style', name);
  st.textContent = out;
  document.head.appendChild(st);
}
