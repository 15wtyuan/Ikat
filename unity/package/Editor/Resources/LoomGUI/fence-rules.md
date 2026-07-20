<!-- loomgui-editor-begin -->
# LoomGUI 围栏规则（硬约束）

生成 HTML+CSS 时严守以下围栏。围栏外写法打包期报错，无法产出 .pkg.bin。

## 元素白名单（30 标签）

**文档壳**（不进运行时）：`html` `head` `body` `title` `meta` `style` `link` `script`

**结构容器**：`div` `header` `nav`
**文本**：`p` `span` `strong` `em` `br`
**关联文本**：`label`
**操作**：`button` `a`
**图片/绘制**：`img` `canvas`
**输入**：`input`（`type` 决定具体控件） `textarea` `select` `option`
**进度**：`progress`
**列表**：`ul` `ol` `li`
**模板/投影**：`template` `slot`

自定业务组件：标签名含 `-`（如 `<game-item-card>`），打包期识别为 CustomElement。

## display 默认值（标准 CSS）

- `div` `header` `nav` `p` `ul` `ol` `li` `option` → 默认 `display:block`
- `span` `strong` `em` `label` `button` `a` `img` `canvas` `input` `textarea` `select` `progress` `template` `slot` → 默认 inline
- `display:flex` 默认 `flex-direction:row`（标准 CSS）
- 纵向堆叠写 `display:flex; flex-direction:column`

## CSS 布局

- `display`: `block` `flex` `none`（禁 `grid` `inline-block`，围栏外报错）
- `flex-direction` `flex-wrap` `gap` `justify-content` `align-items` `align-self`
- `flex-grow` `flex-shrink` `flex-basis` `order` `aspect-ratio`
- `width` `height` `min-width` `max-width` `min-height` `max-height`（px / % / auto）
- `padding` `margin` `border-width`（px）
- `position`: `relative` `absolute`（`absolute` 脱离流，配 `top` `right` `bottom` `left` 定位；禁 `fixed` `sticky`）
- **子项间距用 `gap`，别用 margin**（flex 容器内 margin 不折叠，与浏览器预览不同）
- 禁：`float` `align-content` `inset`（用 `top`/`right`/`bottom`/`left`）

## CSS 视觉

- `background-color` `background-image`(`url()`) `background-size`(`cover`/`contain`/`100%`)
- `border-color` `border-radius` `opacity`
- `overflow` `overflow-x` `overflow-y`（`visible` `hidden` `scroll` `auto`）
- `color` `font-size`(px) `font-family` `font-weight` `text-align` `line-height` `letter-spacing` `white-space`(`nowrap`)
- `transform`（`translate()` `rotate()` `scale()`；禁 `skew()` `matrix()`）
- `pointer-events` `filter`（grayscale/brightness/contrast 等颜色矩阵）
- `box-shadow` `text-shadow` `font-effect`
- `transition` `animation`（简写语法接受，runtime 驱动待后续版本）
- 禁：`clip-path` `background-position` `background-repeat` `transform-origin` `font-style` `cursor` `z-index` `visibility`

## 交互 / 选择器

- 伪类：`:hover` `:active` `:disabled` `:focus` `:checked`
- 选择器：标签 `.class` `#id` 后代 子代(`>`) 分组(`,`)
- 禁：`+` `~` `*` 属性选择器 `:nth-child` `:not()`
- `@keyframes` at-rule 接受语法，runtime 驱动待后续版本

## 预览可信清单

**可信**（Chrome ≈ LoomGUI）：flex 轴/方向、`gap`、颜色、opacity、border、px 尺寸、`background-image`/`background-size`、`position:absolute`

**不可信**（Chrome ≠ LoomGUI）：margin 折叠、文本换行像素级、`display:grid`、`@media`

## 生成完跑验证+打包

生成 HTML+CSS 后调 `loom-pkg build <workspace-dir>` 验证+打包。非零退出=围栏违规，读 stderr 自纠后重跑。零退出=.pkg.bin+图集产出。
<!-- loomgui-editor-end -->
