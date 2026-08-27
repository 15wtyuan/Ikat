// showcase 预览模拟入口（B 层，ikat preview server 自动注入；HTML 源零引用、
// 不进打包）。
//
// 分层契约（#92，A 层 = /ikat-preview/lib/boot.js 由 server 恒注入先行）：
// 组件展开、控件语义、结构性 polyfill 全在 A 层——本文件只写「showcase 专属」的
// B 层：主题样式注入、导航接线、动画重播。演示数据归 pages/<页名>.js。

// showcase 主题面（@font-face、配色、装饰）——消费侧资产，与 A 层 polyfill 无重叠。
const link = document.createElement('link');
link.rel = 'stylesheet';
link.href = 'preview/preview-base.css';
document.head.insertBefore(link, document.head.firstChild);

const NAV = {
  'nav-settings': 'settings',
  'nav-inventory': 'inventory',
  'nav-mail': 'mail',
  'nav-shop': 'shop',
  'nav-character': 'character',
  'nav-form': 'form',
  'nav-lab': 'lab',
  'nav-anim': 'm2-animation',
  'nav-infra': 'api-infra',
};

wireNav();
installAnimReplay();

function pageDir() {
  return location.href.substring(0, location.href.lastIndexOf('/') + 1);
}

function goPage(name) {
  location.href = pageDir() + name + '.html';
}

function wireNav() {
  const bind = (id, fn) => {
    const el = document.getElementById(id);
    if (el) el.addEventListener('click', fn);
  };
  for (const [id, page] of Object.entries(NAV)) bind(id, () => goPage(page));
  bind('back-home', () => goPage('home'));
  // m2-animation 页内「↻ 重播」：浏览器侧等价语义 = 原地重启全部 CSS 动画
  // （Unity 侧走 dispose + 重实例化；预览不重导航，避免打断对照观察）。
  bind('btn-replay', replayAnimations);
}

function replayAnimations() {
  document.querySelectorAll('.root, .root *').forEach((el) => {
    if (getComputedStyle(el).animationName !== 'none') {
      el.style.animation = 'none';
      void el.offsetWidth;
      el.style.animation = '';
    }
  });
}

// 「重播动画」悬浮按钮（preview-only）：入场/fill-mode/delay 标本验收时反复对比。
// 手法：内联 animation:none → 强制 reflow → 移除内联，浏览器按原声明重新起播。
function installAnimReplay() {
  const btn = document.createElement('button');
  btn.id = 'ikat-anim-replay';
  btn.textContent = '↻ 重播动画';
  btn.addEventListener('click', replayAnimations);
  document.body.appendChild(btn);
}
