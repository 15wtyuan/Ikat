// inventory 页演示数据（AI 手写，ikat preview server 按页注入；不进打包）。
// 格子列表运行时由 C# 填充（ItemCount）；预览侧克隆 template 轮换图标。
import { ready } from '/ikat-preview/lib/boot.js';
import { fillList, pageDir } from '/ikat-preview/lib/fill.js';

const ITEM_ROTATION = ['item-potion', 'item-chest', 'item-gem', 'item-scroll', 'item-staff', 'item-wand'];

ready.then(() => {
  const iconsDir = pageDir() + '../res/icons/';
  document.querySelectorAll('[role="list"][data-fill]').forEach((list) => {
    const count = parseInt(list.getAttribute('data-fill'), 10) || 8;
    fillList(list, count, (i, node) => {
      const img = node.querySelector('img');
      if (img && /\/item-/.test(img.getAttribute('src') || '')) {
        img.setAttribute('src', iconsDir + ITEM_ROTATION[i % ITEM_ROTATION.length] + '.png');
      }
    });
  });
});
