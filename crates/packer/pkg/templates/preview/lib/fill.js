// 演示数据填充（框架真相副本，嵌在 yio 二进制里由 preview server 路由供给；
// 消费侧经 `/yio-preview/lib/fill.js` 导入，不拷贝、跟 CLI 版本走）。
// 运行时 ListView 是数据驱动的（C# 设 ItemCount 填充），HTML 里只留 <template>。
// 预览侧克隆 template 补足条目，人类预览才不空列表。
// rect-diff 对拍时克隆会被测量脚本撤掉（core 静态 dump 无 C# 驱动）——两侧口径一致。

import { expandComponentsNow } from './expand.js';

// 克隆 list 的 <template> 子女补到 count 条（template 自身算第 0 条）。
// decorate(i, node) 可按索引改克隆内容（轮换图标/文案等）。
export function fillList(list, count, decorate) {
  const tpl = list.querySelector('template');
  if (!tpl) return;
  for (let i = 1; i < count; i++) {
    const node = tpl.content.cloneNode(true);
    if (decorate) decorate(i, node);
    list.appendChild(node);
  }
  // 克隆发生在初始展开 pass 之后（调用方通常在 ready.then 里填），补一轮展开——
  // template 内含自定义组件时克隆才有展开形（清单由 boot 的 fetchRegistry 缓存）。
  expandComponentsNow();
}

// 页面 URL 所在目录（含尾斜杠）——相对资源定位用。
export function pageDir() {
  return location.href.substring(0, location.href.lastIndexOf('/') + 1);
}
