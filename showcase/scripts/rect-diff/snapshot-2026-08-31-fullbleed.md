# rect-diff 报告 — showcase 全量全屏/刘海适配（2026-08-31，#110 续批）

- 改造面：11 页 `.root` 1920×1080 固定 → `100vw/100vh` + 四向
  `env(safe-area-inset-*)` padding（背景满铺贴边、内容避让 notch）；character
  `.native-slot` 680px → 62.96vh、effects `.fx-slot` 300px → 27.78vh（@1080 基线
  等值）；shop `.dialog-overlay` 同步 100vw/100vh；workspace `match_mode` →
  `fit-width`。home 零改动（root 已 100vw/100vh；其 48px 设计留白 ≥ 常见横屏
  inset ≈47px，留白即避让——无 calc 无法叠加 env，有意保持基线不动）。adapt 页
  上批已流式。
- fit-width 减负面：root 宽恒 1920 design px，全部横向布局零改动；竖向 body 区
  overflow-y:auto/hidden 既有，root 高变化自动滚动/裁切。

## 判据结果

| 判据 | 结果 |
|---|---|
| A/B 时间轴（16:9 同栈 browser 前后对拍，静态布局零变化） | 9 页 0 diff；lab/m2-animation/layout-anim 的 24 处全落动画元素（eg-dot/charge 条/keyframes 盒），同栈两次采样噪声底 12/11/0 同元素实锤 = 相位噪声 |
| 16:9 browser↔core（对照历史/今日基线） | settings 12=12 精确同数；home 101→69（变少）；lab 311 = 改造前今日基线 311（stash 对拍实锤，历史 245 为 8-14 旧数字页面已演进） |
| fit-width@4:3（root=1920×1440）形状双侧对拍 | home 69 / inventory 5 / lab 311 —— 与各自 16:9 同量级（root 宽恒定、文本度量差不随形状变）；inventory 全 1.5px 级 |
| 围栏 check | 0 errors（32 warning = 既有 lab/settings MixedPaintOrder） |

## 刘海避让验证路径

- 桌面/编辑器 inset=0 → A/B 零变化即证 padding 声明无害。
- 预览侧：`yio preview` 选 iPhone 14/16 PM（横持 left/right inset 生效）+ fit-width
  模式，页面内容应让开左右参考线。
- 运行时：真机/模拟 inset 下 env() 经 `yio_stage_set_safe_area` 注入（上批链路）。
