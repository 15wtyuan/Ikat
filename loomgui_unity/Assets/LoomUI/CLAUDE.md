<!-- loomgui-editor-begin -->
# LoomGUI 围栏规则（硬约束）

生成 HTML+CSS 时严守以下围栏。围栏外写法写了不报错但**不生效**（静默忽略），会导致预览与 Unity 渲染不一致 = 不可预测。完整规则见 skill references/fence.md。

## 元素白名单
只用 `div` / `span`（+裸文本）/ `img` / `button`。其他标签（video/input/p/ul/...）会报错。

## CSS 布局
- `display:flex/none`（**禁 grid**，写了落 Flex 预览会骗你）
- `flex-direction` / `flex-wrap` / `gap` / `row-gap` / `column-gap` / `justify-content` / `align-items` / `align-self` / `flex`(grow/shrink/basis) / `order` / `aspect-ratio`
- `width/height/min/max`(px/%/auto) / `padding` / `margin` / `border-width`
- **子项间距用 `gap`，别用 margin**（Chrome 折叠 margin、LoomGUI 求和不折叠）
- `position:absolute`（v1.4-b 起支持，脱离流，配 `top`/`right`/`bottom`/`left` 定位）；禁 `position:fixed/sticky`、`float`、`align-content`
- 禁 `inset` shorthand（围栏外静默丢，用 `top`/`right`/`bottom`/`left` 四个显式属性）

## CSS 视觉
- `background-color` / `background-image`(url) / `background-size`(cover/contain/100%，拒两值)
- `border-radius` / `border`(简写只取宽度) / `border-color` / `opacity`
- `overflow` / `overflow-x` / `overflow-y`
- `color` / `font-size`(px) / `font-family` / `font-weight` / `text-align` / `line-height` / `letter-spacing` / `white-space:nowrap`
- `transform`(translate/rotate/scale，禁 skew/matrix) / `pointer-events`
- `filter`(grayscale/brightness/contrast/saturate/hue-rotate/invert/sepia) / `border-image-slice`(九宫格)
- 禁：`clip-path` / `background-position` / `background-repeat` / `transform-origin` / `font-style` / `cursor`

## 交互/选择器
- 伪类：`:hover` / `:active` / `:disabled` / `:focus`
- 选择器：标签/类/id/后代/子代/分组。禁 `+`/`~`/`*`/属性选择器/`:nth-child`/`:not()`

## position:relative / absolute
`relative` 靠 taffy 默认生效，写不写行为一致（无 inset 偏移）。`absolute`（v1.4-b）脱离流，配 `top`/`right`/`bottom`/`left` 定位。`fixed`/`sticky` 仍静默忽略。

## 预览可信清单
信 flex/gap/color/px/background-image/position:absolute；**不信** margin 折叠/文本换行像素/display:grid/@media。口径"信围栏规则别信预览"。详见 references/preview-trust.md。

## 生成完跑验证+打包
生成 HTML+CSS 后，读 `.claude/skills/loomgui-editor/config.json` 拿 exe_path + 配置，调
`loomgui_pkg.exe <sourceDir> <pkgName> --html <list> --res-root <工作区根/res> -o <out>` 验证+打包。
非零退出 = 围栏违规，读 stderr 自纠后重跑。零退出 = pkg.bin 产出。
<!-- loomgui-editor-end -->
