# LoomGUI 围栏权威清单

围栏 = 面向游戏 UI 的标准 HTML/CSS 子集。围栏外输入打包期明确报错，不静默降级。

## 1. 元素标签围栏

### 1.1 文档壳标签（8 个，不进运行时树）

| 标签 | 用途 |
|---|---|
| `html` | 文档根 |
| `head` | 元数据容器 |
| `body` | 内容根，子元素提升为模板根 |
| `title` | 文档标题 |
| `meta` | 元数据 |
| `style` | 内联 CSS（打包期消费） |
| `link` | 外部 CSS 引用（`rel=stylesheet`） |
| `script` | 脚本（围栏外，打包期报错） |

### 1.2 运行时标签（23 个）

| 标签 | 语义类型 | 默认 display | 分类 | 子内容 | 自闭合 |
|------|---------|-------------|------|--------|--------|
| `div` | Container | block | Block | Flow | |
| `header` | Container | block | Block | Flow | |
| `nav` | Container | block | Block | Flow | |
| `p` | TextBlock | block | Block | Phrasing | |
| `span` | TextElement | inline | Phrasing | Phrasing | |
| `strong` | TextElement | inline | Phrasing | Phrasing | |
| `em` | TextElement | inline | Phrasing | Phrasing | |
| `br` | LineBreak | inline | Void | None | ✓ |
| `label` | Label | inline | Phrasing | Phrasing | |
| `button` | Button | inline | Phrasing | Phrasing | |
| `a` | Link | inline | Transparent | Transparent | |
| `img` | Image | inline | Void | None | ✓ |
| `canvas` | Canvas | inline | Phrasing | Flow | |
| `input` | InputDispatch | inline | Void | None | ✓ |
| `textarea` | TextArea | inline | Phrasing | Text | |
| `select` | Dropdown | inline | Phrasing | Only(`option`) | |
| `option` | OptionItem | block | Block | Text | |
| `progress` | ProgressBar | inline | Phrasing | Phrasing | |
| `ul` | ListView | block | Block | Only(`li`,`template`) | |
| `ol` | ListView | block | Block | Only(`li`,`template`) | |
| `li` | ListItem | block | Block | Flow | |
| `template` | Template | none | Phrasing | Flow | |
| `slot` | Slot | inline | Transparent | Transparent | |

### 1.3 input[type] 分派

| type | 语义类型 | 说明 |
|------|---------|------|
| `text` `password` `search` | TextField | 文本输入 |
| `number` | NumberField | 数字输入 |
| `range` | Slider | 滑块 |
| `checkbox` | Toggle | 勾选框 |
| `radio` | RadioButton | 单选按钮 |

不支持的值域：`submit` `reset` `button` `file` `hidden` `image` `color` `date` `time` `email` `url` `tel`。写了打包期报错。

### 1.4 自定义元素

标签名含 `-`（如 `<game-item-card>`）识别为 CustomElement。未注册的自定义元素打包期报错。

## 2. 属性围栏

### 2.1 全局属性（所有元素可用）

`id` `class` `style` `slot` `hidden` `tabindex` `role` `aria-*` `data-*` `--*`

### 2.2 结构属性（特定元素）

- `input[type]`：值域见 §1.3
- `label[for]`：IdRef
- `a[href]`：FreeText

### 2.3 内容属性（元素特有的 HTML 属性）

- `img[src]` `img[alt]` `canvas[width]` `canvas[height]`
- `input[value]` `input[placeholder]` `input[min]` `input[max]` `input[step]` `input[checked]`
- `textarea[value]` `textarea[placeholder]`
- `select[value]` `option[value]` `option[selected]`
- `progress[value]` `progress[max]`
- `button[disabled]` `input[disabled]` `select[disabled]` `textarea[disabled]` `option[disabled]`
- `slot[name]`

## 3. CSS 属性围栏

### 3.1 布局

| 属性 | 值 | 默认 |
|------|----|------|
| `display` | `block` `flex` `none` | `block`（块级元素）/ inline（行内元素） |
| `flex-direction` | `row` `row-reverse` `column` `column-reverse` | `row` |
| `flex-wrap` | `nowrap` `wrap` | `nowrap` |
| `justify-content` | `flex-start` `center` `flex-end` `space-between` `space-around` `space-evenly` | `flex-start` |
| `align-items` | `flex-start` `center` `flex-end` `stretch` `baseline` | `stretch` |
| `align-self` | `flex-start` `center` `flex-end` `stretch` `baseline` | `auto` |
| `gap` `row-gap` `column-gap` | px | `0` |
| `flex-grow` | number | `0` |
| `flex-shrink` | number | `1` |
| `flex-basis` | px / % / auto | `auto` |
| `width` `height` | px / % / auto | `auto` |
| `min-width` `min-height` `max-width` `max-height` | px / % | — |
| `padding` | 1-4 值 px | `0` |
| `margin` | 1-4 值 px / % / auto | `0` |
| `border-width` | px（简写只取宽度） | `0` |
| `position` | `relative` `absolute` | `relative` |
| `top` `right` `bottom` `left` | px（配合 `absolute`） | — |
| `order` | integer | `0` |
| `aspect-ratio` | number | — |

### 3.2 视觉

| 属性 | 值 | 默认 |
|------|----|------|
| `background-color` | #rrggbb / #rrggbbaa | transparent |
| `background-image` | `url("...")` | none |
| `background-size` | `cover` `contain` `100%` | — |
| `border-color` | #rrggbb | transparent |
| `border-radius` | 1-4 值 px / % | `0` |
| `opacity` | 0-1 | `1` |
| `overflow` `overflow-x` `overflow-y` | `visible` `hidden` `scroll` `auto` | `visible` |
| `color` | #rrggbb / #rrggbbaa | 继承 |
| `font-size` | px | 继承 |
| `font-family` | 原样字符串 | 继承 |
| `font-weight` | 数字 | 继承 |
| `text-align` | `left` `center` `right` | 继承 |
| `line-height` | px 或裸数字 | 继承 |
| `letter-spacing` | px | 继承 |
| `white-space` | `nowrap` | 继承 |
| `text-shadow` | text-shadow 语法 | 继承 |
| `font-effect` | LoomGUI 私有文字特效 | 继承 |
| `transform` | `translate(x,y)` `rotate(deg)` `scale(x[,y])` | none |
| `pointer-events` | `auto` `none` | `auto` |
| `filter` | grayscale/brightness/contrast/saturate/hue-rotate/invert/sepia | none |
| `box-shadow` | box-shadow 语法 | none |
| `border-image-slice` | 九宫格 | none |

### 3.3 动画（fence 接受语法，runtime 驱动待后续版本）

| 属性 | 说明 |
|------|------|
| `animation` | `<name> <duration> [easing] [iteration-count] [fill-mode] [direction]` |
| `transition` | 待实现（当前接受任意值但不生效） |

`@keyframes <name> { from/to/<N>% { ... } }` at-rule 接受并解析。

## 4. 选择器围栏

### 4.1 支持

- 标签选择器：`div` `button` 等
- 类选择器：`.btn` `.active`
- ID 选择器：`#main`
- 后代组合：`div span`
- 子代组合：`div > span`
- 分组：`.a, .b`

### 4.2 伪类

`:hover` `:active` `:disabled` `:focus` `:checked`

### 4.3 不支持（打包期报错或忽略）

复杂选择器：`:nth-child` `:nth-of-type` `:first-child` `:last-child` `:not()` 属性选择器 `[attr=val]`
关系选择器：相邻兄弟 `+` 后续兄弟 `~` 通配符 `*`

## 5. 子内容规则（围栏违规报错）

- Flow 容器（`div` `header` `nav` `li` `canvas`）：可含块级 + 行内 + 文本
- Phrasing 容器（`p` `span` `strong` `em` `label` `button` `progress`）：可含行内 + 文本，不可含块级（`<span><div/></span>` 报错）
- None（`img` `input` `br`）：无子内容
- Text（`textarea` `option`）：仅文本
- Only([...])（`ul`/`ol` 仅 `li`/`template`；`select` 仅 `option`）
- Transparent（`a` `slot`）：继承父级限制

## 6. 验证+打包

`loom-pkg build <workspace-dir>` 执行围栏验证，非零退出 = 违规，读 stderr 自纠重跑。零退出 = .pkg.bin + 图集产出。
