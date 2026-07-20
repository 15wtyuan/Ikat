# 预览可信清单

open-design Chromium iframe 预览 ≠ LoomGUI 渲染。AI 须分清：

## 可信（Chrome ≈ LoomGUI）

flex 轴/方向、`gap` 间距、颜色、opacity、border、图片、px 尺寸、`background-image`/`background-size`（标准 CSS，Chrome 原生）、`position:absolute`（脱离流定位）、`overflow:scroll/auto`（滚动行为一致）。

## 不可信（Chrome ≠ LoomGUI，别按预览调）

- **`display:block` 默认**：`div` `p` `ul` `li` 等块级元素，Chrome 和 LoomGUI 都是标准 block 语义（垂直堆叠）。但 LoomGUI 当前版本的 block 布局仍在完善中，实际行为可能偏向 flex。建议显式写 `display:flex` 确保布局可控。
- **margin 控间距**：Chrome（block flow）折叠 margin、LoomGUI（flex 容器）求和不折叠。**子项间距用 `gap`**，别用 margin。
- **文本换行/像素级**：Chrome 文本引擎 vs LoomGUI（unicode-linebreak），换行点/塞文本宽度会偏。
- **`display:grid`**：不在围栏内，打包期报错。别用。
- **`@media` 响应式**：Chrome 响应、LoomGUI 用参考分辨率缩放不响应 @media。别用。
- **LoomGUI 私有扩展**：`font-effect`（文字特效）仅 LoomGUI 有效，Chrome 预览看不到。

## 口径

不可信项"信围栏规则，别信预览"。
