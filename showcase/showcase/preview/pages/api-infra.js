// api-infra 页演示数据（AI 手写，yio preview server 按页注入；不进打包）。
// GetTemplate 具名模板行：运行时由 driver 填充；预览侧克隆 template 补条目。
import { ready } from '/yio-preview/lib/boot.js';
import { fillList } from '/yio-preview/lib/fill.js';

ready.then(() => {
  document.querySelectorAll('[role="list"][data-fill]').forEach((list) => {
    const count = parseInt(list.getAttribute('data-fill'), 10) || 8;
    fillList(list, count);
  });
  // #6 多模板列表：模拟 TemplateSelector 分派（第 3/6/9… 行走强调蓝图）——与
  // ShowcaseRunner 的 lambda 同规则，浏览器侧克隆对应 template 补条目。
  const mt = document.querySelector('[role="list"][data-fill-mt]');
  if (mt) {
    const count = parseInt(mt.getAttribute('data-fill-mt'), 10) || 12;
    const tplNormal = mt.querySelector('template#row-tpl');
    const tplAccent = mt.querySelector('template#row-tpl-accent');
    for (let i = 0; i < count; i++) {
      const tpl = (i % 3 === 2) ? tplAccent : tplNormal;
      const node = tpl.content.cloneNode(true);
      const spans = node.querySelectorAll('span');
      if (spans.length >= 2) {
        spans[0].textContent = '#' + String(i).padStart(2, '0');
        spans[1].textContent = (i % 3 === 2) ? '强调行' : '普通行';
      }
      mt.appendChild(node);
    }
  }
});
