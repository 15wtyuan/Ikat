// showcase 预览模拟入口（AI 手写，loom preview server 自动注入本文件 + 按页的
// pages/<页名>.js；HTML 源零引用、不进打包）。
//
// 分层约定（loomgui-preview skill）：本文件管「能从源码推断」的全页模拟——
// 组件展开、控件语义、tabs/dialogs、导航、动画重播；每页专属的演示数据归
// pages/<页名>.js。font/letterbox 等基础设施在 preview-base.css；分辨率缩放
// 由 loom preview 外壳按 match_mode 负责（页面自身不再缩放）。

import { expandComponents, fetchRegistry } from './lib/expand.js';
import { pageDir } from './lib/fill.js';
import {
  wireComboboxes,
  wireDialogs,
  wireProgressbars,
  wireSliders,
  wireSpinbuttons,
  wireSwitchesAndRadios,
  wireTabs,
  wireTextboxes,
} from './lib/controls.js';

// pages/<页名>.js 等 main 就绪后再填数据/做页面级演示。
export const ready = boot();

async function boot() {
  // preview-base.css（浏览器侧 polyfill：@font-face、box-sizing、button reset）
  // 必须走脚本通道注入——围栏校验每个 <link rel=stylesheet>，polyfill 故意满含
  // 围栏外声明。插到 head 顶：polyfill 先、页面 <style> 后（旧内联脚本的级联序
  // 语义——经典 script 在解析中 appendChild 天然落位在前；ESM 延后执行需显式
  // insertBefore 复原，否则同名规则胜负翻转）。
  const link = document.createElement('link');
  link.rel = 'stylesheet';
  link.href = 'preview/preview-base.css';
  document.head.insertBefore(link, document.head.firstChild);

  try {
    const reg = await fetchRegistry();
    expandComponents(reg);
  } catch (_) {
    // 组件清单拿不到（如直接 file:// 打开）→ 退化预览：不展开组件。
  }
  wireNav();
  wireTabs();
  wireDialogs();
  wireProgressbars();
  wireSliders();
  wireSwitchesAndRadios();
  wireComboboxes();
  wireSpinbuttons();
  wireTextboxes();
  installAnimReplay();
}

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
  btn.id = 'loom-anim-replay';
  btn.textContent = '↻ 重播动画';
  btn.addEventListener('click', replayAnimations);
  document.body.appendChild(btn);
}
