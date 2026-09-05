// 组件展开模拟（框架真相副本，嵌在 yio 二进制里由 preview server 路由供给；
// 消费侧经 `/yio-preview/lib/expand.js` 导入，不拷贝、跟 CLI 版本走）。
// 镜像打包期 Custom Element 展开：宿主保留位置/属性；模板根 append 到宿主下；
// <slot name=x> 在拼接位被宿主 light children（slot="x"）替换（无投射内容时保留
// slot 的 fallback 子女）；纯空白文本节点丢弃；嵌套组件迭代展开至不动点（≤16 pass）。
// 组件清单来自 server 的 /api/workspace.json（与打包同一套扫描口径）。
//
// 组件 <style> 的作用域改写不在本文件（#95 教训：手写正则前缀只会拼后代选择器，
// 根类规则整条死；@media 放行、keyframes 同名优先级反转同批审计实锤）——CSS
// 语义单真相在 Rust：server 经 /yio-preview/comp-style/<name>.css 供给双分支
// 改写版，这里只注入 <link>。本文件残留职责是纯 DOM 机械层。

// 组件清单缓存（fetchRegistry 灌入）：expandComponentsNow 的补展开数据源。
// 条目 = { src: 组件 HTML 文本, rel: 工作区相对路径 }——rel 是模板内相对 URL
// 的解析基准（server 已按打包口径给出，不猜目录布局）。
let cachedReg = null;

export async function fetchRegistry() {
  const api = await fetch('/api/workspace.json').then((r) => r.json());
  const entries = Object.entries(api.components || {});
  const reg = {};
  await Promise.all(
    entries.map(async ([name, rel]) => {
      // rel 是工作区相对路径（如 showcase/components/item-card.html）；
      // server 把工作区挂载在 /ws/ 下。
      const src = await fetch('/ws/' + rel).then((r) => r.text());
      reg[name] = { src, rel };
    }),
  );
  cachedReg = reg;
  return reg;
}

// 晚到 DOM 的补展开入口（fill.js 克隆后调）：用缓存的组件清单就地再跑一轮。
// 初始 pass 看不见 <template> 内部（惰性 DocumentFragment），克隆落树后须补跑
// 才会展开 template 里含的自定义组件；boot 未跑过（cachedReg 空）时安全 no-op。
export function expandComponentsNow() {
  if (cachedReg && Object.keys(cachedReg).length) expandComponents(cachedReg);
}

export function expandComponents(reg) {
  let passes = 0;
  while (passes++ < 16) {
    const hosts = Array.from(document.querySelectorAll('*')).filter((el) => {
      const name = el.tagName.toLowerCase();
      return name.indexOf('-') >= 0 && reg[name] && !el.hasAttribute('data-yio-expanded');
    });
    if (!hosts.length) return;
    for (const host of hosts) {
      host.setAttribute('data-yio-expanded', '');
      const name = host.tagName.toLowerCase();
      const def = reg[name];
      const doc = new DOMParser().parseFromString(def.src, 'text/html');
      // <style> 只负责登记作用域样式表（server 改写版），节点本身不进页面——
      // 原样 import 会全局裸生效。
      for (const st of Array.from(doc.querySelectorAll('style'))) st.remove();
      ensureComponentStylesheet(name);
      // 模板内相对 URL（src/href）按组件文件位置（server 给的 rel）解析成绝对
      // URL——镜像打包器的 html_rel 归一。不重写则按页面位置解析，fallback 图 404。
      const compBase = new URL('/ws/' + def.rel, document.baseURI);
      for (const el of Array.from(doc.querySelectorAll('[src],[href]'))) {
        const attr = el.hasAttribute('src') ? 'src' : 'href';
        el.setAttribute(attr, new URL(el.getAttribute(attr), compBase).href);
      }
      const root = doc.body.firstElementChild;
      if (!root) continue;
      root.setAttribute('data-yio-comp', name);
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
      // 初始快照先镜像一次：宿主**静态** class（HTML 里写死的 `class="sub"` 一类）
      // 不随任何 mutation 到来——core 侧 `stat-text.sub …` 链静态即命中，漏初始
      // 镜像会让这类规则在预览里静默死（#96 取证发现的残留缺口）。之后由
      // MutationObserver 跟踪运行时变更。
      mirrorHostState(host, imported);
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
// （data-yio-comp 载体），使改写后的规则与 core 判定一致（Tripawd 狗粮实证：
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
  // 不设 attributeFilter：固定快照看不到 B 层事后**新增**的 data-属性（动态挂状态
  // 会漏镜像）；镜像内容在 mirrorHostState 里筛（class/data-*/aria-*），观察面宽
  // 只多几次幂等回调。
  new MutationObserver(() => mirrorHostState(host, root)).observe(host, {
    attributes: true,
  });
}

// ---- 组件作用域样式表（server 单真相）----
//
// 每组件一条 <link>，指向 server 的 Rust 改写版 CSS（双分支选择器、@media 拒绝、
// url() 绝对化都在 server 做）。注入位置 = head 里第一个「页面自有」style/link 之前
// （base.css 与本类链接都算框架自有）——三层 keyframes/级联序由此对齐 core：
// base < 组件（后展开者后插 = 后胜，镜像 core「后实例化组件覆盖同名规则」）
// < 页面（同名 @keyframes 宿主胜，镜像打包期 host 优先）。同名组件去重，多实例/
// 克隆补展开只登记一次。

function ensureComponentStylesheet(name) {
  if (document.querySelector(`link[data-yio-comp-style="${name}"]`)) return;
  const link = document.createElement('link');
  link.rel = 'stylesheet';
  link.setAttribute('data-yio-comp-style', name);
  link.href = '/yio-preview/comp-style/' + name + '.css';
  const isOwned = (el) =>
    el.tagName === 'STYLE' ||
    (el.tagName === 'LINK' && (el.rel || '').toLowerCase() === 'stylesheet');
  const isFramework = (el) =>
    el.hasAttribute('data-yio-comp-style') || el.hasAttribute('data-yio-preview');
  const anchor = Array.from(document.head.children).find((el) => isOwned(el) && !isFramework(el));
  if (anchor) document.head.insertBefore(link, anchor);
  else document.head.appendChild(link);
}
