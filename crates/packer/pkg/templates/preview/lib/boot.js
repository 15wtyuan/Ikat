// 预览 A 层引导（框架真相副本，嵌在 ikat 二进制里；server 对每个 HTML 页自动注入
// 本入口，先于工作区 preview/main.js 执行）。
//
// 分层契约（#92）：本 boot 覆盖「单真相语义」——组件展开、控件视觉/交互语义、
// 结构性 polyfill（base.css）。这些与打包/运行时一一对应，答案全网只有一个，
// 不许工作区复刻（复刻物 = 第二真相源，随版本演进腐烂）。工作区脚本只写 B 层：
// 演示数据（pages/<页>.js）、页面导航、页面专属交互——站在本 boot 之上。
//
// 工作区脚本用 `import { ready } from '/ikat-preview/lib/boot.js'` 等待 A 层
// 就绪后再操作 DOM（填充的列表克隆会被展开 pass 看到——fill 在 ready 之后跑
// 的组件已在树，无需再手动触发展开）。

import { expandComponents, fetchRegistry } from './expand.js';
import {
  wireComboboxes,
  wireDialogs,
  wireProgressbars,
  wireSliders,
  wireSpinbuttons,
  wireSwitchesAndRadios,
  wireTabs,
  wireTextboxes,
} from './controls.js';

export const ready = boot();

async function boot() {
  injectBaseCss();
  // 组件清单拿不到（如 file:// 直开、registry 为空）→ 退化预览：不展开组件。
  try {
    const reg = await fetchRegistry();
    if (Object.keys(reg).length) expandComponents(reg);
  } catch (_) {}
  wireTabs();
  wireDialogs();
  wireProgressbars();
  wireSliders();
  wireSwitchesAndRadios();
  wireComboboxes();
  wireSpinbuttons();
  wireTextboxes();
}

function injectBaseCss() {
  // 插到 head 顶：polyfill 先、页面 <style> 后（同名规则胜负不翻转——经典级联序
  // 语义。ESM 延后执行需显式 insertBefore 复原，appendChild 会落在最后）。
  const link = document.createElement('link');
  link.rel = 'stylesheet';
  link.href = '/ikat-preview/lib/base.css';
  document.head.insertBefore(link, document.head.firstChild);
}
