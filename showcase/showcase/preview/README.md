# Showcase browser preview

双击 `../home.html` 浏览器预览（视觉参考，非运行时行为镜像）。

## 架构：预览 vs 打包（重要）

showcase 当前是**预览靶子**，还不能直接被 loomgui 运行时渲染。两层原因：

1. **打包器（R1.1）不打包 HTML**。`crates/packer/pkg/src/build.rs` 在 R1.1 移除了 HTML→pkg.bin 路径（`packages` 恒空），只产 atlas/fonts/runtime manifest。HTML→pkg 将在 **R3 经 fence crate 重建**。
2. **fence 只消费 inline style**。fence 的 `css_resolve` 只解析每个 element 的 `style="..."` 属性 + 标签 `DisplayDefault`（div/nav/header→Block；a/button/span→Flex Row）。`<style>` 块和 `<link rel=stylesheet>`（即 preview-base.css）**不进 IR/pkg**。

结论：
- 各页 `<style>` 块的 class CSS（`.root`/`.nav-card` 等）当前只服务**浏览器预览**。
- `preview-base.css` 是浏览器预览 polyfill（@font-face 字体、body 居中 letterbox、box-sizing、button/a reset），不进 pkg。
- R3 重建打包路径后，若 fence 仍只 inline，class CSS 需转 inline style 才能进 pkg；若 fence 届时支持 `<style>`，则现状即可。这是 R3 决定项。

验证围栏符合度（diagnostics 应为 0）：
```bash
cargo run -p loomgui_fence --example dump_showcase -- showcase/showcase/<page>.html
```

## 围栏覆盖矩阵

showcase 覆盖全部围栏表面，供 R2-R7 运行时重写验收。

```bash
python showcase/scripts/coverage-check.py    # → COVERAGE OK
```

- **23 runtime tags**：div header nav p span strong em br label button a img canvas input textarea select option progress ul ol li template slot
- **7 input types**：text password search number range checkbox radio
- **9 CSS groups**：sizing layout position box-model border background visual text overflow
- **custom-element**：`<item-card>`（hyphenated tag = CustomElement）+ `<slot>` 投影
- **无 forbidden tag**（h1-h6 / meter / dialog / details / form / fieldset 等）

### 页面与经典场景
| 页 | 经典场景 |
|---|---|
| home | 主菜单导航、卡片网格、CTA |
| settings | tablist/tabpanel、input 全 7 型、select/option、checkbox/radio-group |
| inventory | ListView 虚拟化（data-fill）、物品网格、详情面板、progress 耐久 |
| mail | ListView、富文本（strong/em/a/br）、纵向滚动 |
| shop | progress、dialog 弹窗、number 输入 |
| character | canvas NativeHost 3D、装备槽网格、有序技能列表、stat-bar |
| form | 表单全控件编排、textarea、slider、div+p 分组（替代 fieldset） |
| lab | CSS 全属性 specimen（flex/盒模型/边框/背景/文本特效/变换/溢出/自定义元素） |

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
- 改 showcase HTML：刷新浏览器。
- 改后跑 `coverage-check.py` + `dump_showcase` 确认围栏 diagnostics=0。
- NAV 表在 loom-preview.js 顶部，新页加那里。
