// api-infra 页演示数据（AI 手写，ikat preview server 按页注入；不进打包）。
// GetTemplate 具名模板行：运行时由 driver 填充；预览侧克隆 template 补条目。
import { ready } from '../main.js';
import { fillList } from '../lib/fill.js';

ready.then(() => {
  document.querySelectorAll('[role="list"][data-fill]').forEach((list) => {
    const count = parseInt(list.getAttribute('data-fill'), 10) || 8;
    fillList(list, count);
  });
});
