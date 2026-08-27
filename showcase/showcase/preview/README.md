# Showcase browser preview

`ikat preview <workspace>`（在仓库 showcase/ 目录跑）起本地预览工作台：左侧包/页
树、右侧按 match_mode 缩放的设计分辨率预览、设备框 + 安全区参考线。file:// 双击
已退役——预览脚本由 server 注入，HTML 源零 `<script>` 引用。

## 架构：谁负责什么

| 层 | 归属 |
|---|---|
| 工作台外壳（树/缩放/设备框/设置） | ikat.exe 内嵌，版本随 CLI |
| 注入（main.js / pages/<页>.js，存在才注入） | `ikat preview` server |
| 组件清单数据 | server `/api/workspace.json`（与打包同一套扫描口径） |
| 模拟脚本本体 | **本目录，AI 手写**（约定见 ikat-preview skill） |

```
preview/
  main.js             ← 全页共享入口：base.css 注入、组件展开、控件/tabs/dialogs/导航、动画重播
  pages/<页>.js       ← 按页演示数据（mail/inventory/api-infra 的 data-fill 列表）
  lib/                ← expand（组件展开）/ controls（控件语义）/ fill（演示填充），ESM 自由组织
  preview-base.css    ← 浏览器 polyfill（@font-face、box-sizing、button reset、重播按钮样式）
```

**不进打包**：`preview/` 不在打包扫描面（包目录只扫顶层 `*.html`）；HTML 零引用。
`ikat check` 对 `data-fill` 页缺 `pages/<页>.js` 会报 `PreviewDataFillWithoutSim`。

## 页面与经典场景
| 页 | 经典场景 |
|---|---|
| home | 主菜单导航、卡片网格、CTA |
| settings | tablist/tabpanel、textbox/textarea/spinnumber、combobox/option、switch/radio-group |
| inventory | ListView 虚拟化（data-fill）、物品网格、详情面板、progressbar 耐久 |
| mail | ListView、富文本（span 混排）、纵向滚动 |
| shop | progressbar、dialog 弹窗、spinnumber 输入 |
| character | NativeHost 3D、装备槽网格、技能列表（role=list）、stat-bar |
| form | 表单全控件编排、textarea、slider、div 分组（替代 fieldset） |
| lab | CSS 全属性 specimen（flex/盒模型/边框/背景/文本特效/变换/溢出/自定义元素） |
| m2-animation / layout-anim / api-infra | 动画端点 / 布局动画 / 运行时 API 演示 |

组件：nav-bar（顶栏）、stat-bar（状态条）、item-card（自定义元素 + slot 投影）。

## 设计：精致暗色（Deep Ocean）

- **单一 accent**：cyan `#5fb4d4`（主交互 / CTA / focus）。
- **gold `#d4a44e`**：语义色，仅稀有 / 货币 / 等级，不抢主 CTA。
- **圆角锁**：统一 10px。
- **面板**：半透明 surface + `box-shadow: inset 0 0 0 1px ...` 内发光边（围栏无 `border-style`，边框用 box-shadow 模拟）。
- token 见 `../design-systems/tokens.css`。

## Trustworthy（与运行时一致）
flex layout/direction/gap/justify/align；px/% sizing；color/opacity/border/radius；
background-image/size；filter；transform；overflow:scroll；border-image-slice 9-grid；list skeleton。

## Approximate（布局漂移，非像素级）
- 文本换行/间距：Chrome vs unicode-linebreak，断点可能不同。
- tween 动画：CSS transition 近似，非逐曲线 ease。
- 拖拽/长按/按键事件：浏览器事件近似。

## Runtime-only（不在 HTML 镜像）
TweenManager 逐曲线 ease、虚拟列表 slot 复用/不等高补偿、NativeHost 3D/粒子、事件系统、overlay 堆叠时序。

## Maintenance
- 改 showcase HTML / 模拟脚本：刷新浏览器（server 每请求现读源文件，无需重启）。
- 改 `components/*.html`：无需任何再生步骤（组件清单由 server /api 实时吐）。
- 新页导航入口：NAV 表在 `main.js` 顶部。
- 改后跑 `coverage-check.py` + `ikat check` 确认 diagnostics 干净。
