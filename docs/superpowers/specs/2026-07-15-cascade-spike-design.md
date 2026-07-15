# 阶段 S spike：cascade + bridge 探路设计

> **日期**：2026-07-15
> **轮次**：摸黑打通骨架链 · Spec-1（共 4 spec）
> **状态**：设计已定（经源码核实 + 外部 review 修订），待实现
> **前置依赖**：无（P0.1/P0.3 显式推迟，见 §6）
> **后续**：Spec-2（core 类型化重构①）、Spec-3（cascade 收尾③ + 桥/打包编排② + 终点线1）、Spec-4（后端对象层④ + 终点线2）
> **权威上下文**：[roadmap §2](../../roadmap/roadmap.md)（施工图）、[main-design §5](../../design/main-design.md)（样式）、[fence.md](../../design/fence.md)（围栏）

## 1. 背景与目的

路线图红队修订后的核心结论：整条 HTML→屏幕链的真实断点有**三处**（roadmap 三方代码核实 + 本轮独立复验）：

1. **`IrTree → TemplateNode` 翻译桥缺失**——fence 产 `IrTree`（`crates/fence/src/ir.rs`），core 消费 `TemplateNode`（`crates/core/src/asset/mod.rs:69`），二者间无翻译代码；`IrTree` 在 core/packer 零引用。
2. **packer 的 HTML→pkg.bin 编排层被删**——commit `d8fe705` 删了 `resolve.rs`/`lib.rs`/`pack.rs` 等（-1502 行）；现 `build.rs` 顶部注释写明"只做 atlas/font/runtime，packages 恒空"。
3. **`<style>` 选择器文本解析器不存在**——cssparser / selectors / scraper **全仓一个都没引**（fence 现仅依赖 `loomgui_core`/`taffy`/`html5gum`，跟着旧 packer 一起删了）；core 只有匹配器（`compound_matches_node`/`rematch`，`crates/core/src/style/dynamic.rs`），没有从字符串构造 `ParsedSelector` 的生产路径；继承只有 `propagate_color_inheritance`（`dynamic.rs:370`）的 color-only hack。

本 spike 的目的：**在不动 core 表示层（旧 5 变体 enum）的前提下，用 `div/span` 最小例先打通 `HTML → cascade → rect`，把这三个致命假设探掉**，再决定投入大重构（Spec-2 的 enum 扩容）。绿 = 路线可继续；红 = 某假设证伪，回路线图重审。

S1/S2 的产物（选择器解析器 + 继承机制）**不是废弃代码**——它们是 Spec-3 ③（cascade 收尾）的主体，只是先在旧 enum 上验证可行性，避免在移动靶子上写。仅 S3 的 IrTree→Node 映射是 throwaway。

> **源码核实带来的好消息（重塑工作量）**：core 的 cascade **引擎**（匹配 + specificity 合并 + base_style 重起）**已经存在且在跑**（§5.0）。所以真正的新代码只剩三块——S1 解析器、S2 通用继承、S3 mini-bridge。spike 比初版 spec 想的更轻。

## 2. 范围

**做**：
- S1 选择器解析器（解析 `<style>` → rule table，喂给现成 cascade 引擎）。
- S2 通用继承机制（推翻 color-only hack）。
- S3 端到端集成测试（div/span HTML → rect + 语义断言）。

全程在**旧 5 变体 `NodeKind` enum**（`crates/core/src/scene/node.rs:61`：Container/Text/RichText/Image/Button）上。不扩 enum、不拆 struct。

**不做**（显式排除，见 §7）：enum 扩容、生产桥/打包编排重建、C# 投影层、FFI/dll/Unity、`:nth-child`/`@keyframes`/属性选择器/`!important`、视觉特效、`<link>` 外部 sheet 加载（showcase 用的 `preview/preview-base.css` 是浏览器预览 polyfill，非运行时内容，见 §7）。

## 3. 验收门

一个 `cargo test` 集成测试。输入**合成的** `div/span` HTML（含 inline `<style>` 块 + class 选择器 + 后代组合 + 继承属性 + `display:none` 子树）。经 fence 解析 → rule table → core cascade → layout 后，**同时断言四件事**：

1. **rect 坐标正确**——flex/block 摆位与预期一致（现成 taffy layout，验接通）。
2. **继承传播**——仅在祖先规则里设 `font-size` 的文本节点，字号正确继承（**S2 新代码**，证明通用继承非 color-only）。
3. **class 命中**——带 class 的节点 computed style 反映规则（现成 cascade 引擎，验 S1 产的规则表喂得进去）。
4. **`display:none` 剪枝**——`display:none` 子树不参与布局/渲染（S3 实测确认，taffy 支持）。

绿 = 三个假设（解析器/继承/编排接通）被证实或证伪，路线可继续。

> **判据不只 rect**（roadmap §3.3）：项目历史证明"单测全绿但 CSS 集成错"。rect 对 ≠ 语义对。四条断言一起过才算 spike 成功。断言 1/3 主要验"接通现成引擎"；断言 2 是 S2 新逻辑；断言 4 是端到端确认。

## 4. 数据流与分层

```text
<div>/<span> HTML + <style>（合成测试 HTML，inline <style>）
   │  fence::parse_template（复用现有流水线，但须先改 fence 留存 <style> 文本，见 §5.1）
   ▼
IrTree + <style> 文本
   │  S1：解析 <style>（路径 S1.0 三选一）→ rule table（选择器 + specificity + 声明）
   ▼
rule table（Vec<DynamicRule>）+ IrTree
   │  测试内部 mini-bridge（throwaway）：IrTree → 旧 Node 树；规则表 → scene.dynamic_rules
   ▼
core Node 树 + dynamic_rules
   │  core cascade（现成引擎 rematch_pseudo_classes，见 §5.0）+ 继承（S2 独立 pass）
   ▼
computed style
   │  layout（taffy，复用现有）
   ▼
rect  ← 断言 §3 四条
```

**关键边界**：
- **fence = 纯解析器**（构建期）：文本 → rule table，不做 cascade 决策。
- **core = 唯一 cascade 引擎**（运行期）：消费 rule table，做匹配 + 继承 + 合并。`base_style` = 每帧 cascade 基线（§5.3 源码已确认）。
- **rule table 进 pkg.bin 是终态**——但本轮不走完整 pkg 生产路径（packer 编排已删，Spec-3 ② 重建）。S3 用测试内部通路接通。
- **类型单一真相源**：selector/rule 类型（`ParsedSelector`/`Compound`/`Specificity`/`Declaration`/`DynamicRule`，现均在 core `style/dynamic.rs`）只定义一份，不重复。具体产/消费归属由 S1.0 定（§5.1）。

## 5. 组件设计

### 5.0 现有 cascade 引擎（好消息，重塑工作量）

核实 `crates/core/src/style/dynamic.rs:301` 的 `rematch_pseudo_classes`：**它已是完整 cascade 引擎，名字是历史遗留**。每帧对每个节点：

- 从 `base_style.clone()` 重起（`:330`）。
- 遍历 `dynamic_rules.rules` **全部**规则收集命中（`:332-337`，**不只筛伪类**）。
- 按 specificity 升序排（`:338-339`，高 specificity 后 apply 胜出）。
- `apply_decl` 叠加声明（`:340-344`）→ 写 `Node.style`。
- 末尾 `propagate_color_inheritance`（`:359`，color-only，S2 要泛化/替换）。

后代链匹配（`match_element_with_state` `:236-290`）、tag 选择器（`compound_matches_node` `:100-109`：Container→div / Button→button / Image→img / Text→span / RichText→span）、伪类状态门（hover/active/disabled/focus）都已在跑，dynamic.rs 单测全覆盖。

**推论**：把 class/tag/后代/伪类规则塞进 `scene.dynamic_rules`，§3 的断言 1（rect）和 3（class 命中）**近乎白拿**——cascade 引擎现成。S1 唯一的 cascade 相关工作 = **产出规则表**，不是写匹配/合并。真正的新代码只剩 S1 解析器、S2 通用继承、S3 mini-bridge。

> **旧设计的 bake 分流已失效**：旧 packer 把无伪类静态规则 bake 进 base_style、只把伪类/属性规则放 dynamic_rules（`dynamic.rs:756` 注释）。packer 已删，故 spike 里 **全部规则进 dynamic_rules**、base_style = UA 默认（§5.3）。

### 5.1 S1 选择器解析器（本轮唯一有不确定性的新代码）

**关键更正（推翻初版前提）**：cssparser 是**分词器**，不产选择器 AST、也算不了 specificity——选择器 prelude 在 cssparser 里只是一串不透明 token。要得到 `ParsedSelector`/`Compound`/`Specificity`，标准做法是在 cssparser 之上叠 `selectors` crate，或用 scraper（内部包了 selectors）。CLAUDE.md 把 `cssparser 0.34` / `scraper 0.19` 钉为"用则钉此版本"，正是旧 packer 当年的选择。

**S1.0 阻抗 spike（最先做，~半天，三选一）**：用最小例（`.foo { color: red }`）实证挑解析路径：

- **(a) scraper 0.19**——CLAUDE.md 已钉、旧 packer 用过、自带 cssparser+selectors+specificity。代价：拖进一个 fence 已不需要的 HTML 解析器（fence 用 html5gum）；scraper `Selector` → core `ParsedSelector` 要适配层。
- **(b) cssparser 0.34 + selectors crate**——selectors 给 `Selector<Impl>` + specificity，需定义 `Impl` + 适配到 core 类型。中等。
- **(c) 硬化现有测试解析器**——`dynamic.rs:505` 的 `hand_selector` **已经是覆盖本子集（class/id/tag/伪类/属性/后代）的可用解析器**（仅测试用），只差 specificity 计算（现写死 `Specificity(0,0,0)`）。硬化它 + 补 specificity = 零新依赖、直产 core 类型。代价：手搓解析器的边界 case 触手（但子集小、有参考）。

产出：(1) 三选一决定 + 理由；(2) 若选 (a)/(b)，selector 类型归属（fence 产 core 类型 / 共享 crate / packer 适配）一并定。回填本 spec §4。

**前置：保留 `<style>` 文本**：fence 现把 `<style>` 当 shell 标签消费、文本丢弃（`SHELL_TAGS` + `tree_builder::is_shell_tag`，`tag.rs:147` / `tree_builder.rs:227,256`）。S1 须改 fence 让 `<style>` 文本留得下来（进 `ParsedTemplate.styles` 一类产物）供解析。

**支持子集**：`class`（`.foo`）/ `tag`（`div`）/ `id`（`#bar`）/ 后代组合（`.a .b`）/ 伪类（`:hover/:active/:disabled/:focus/:checked`）。specificity 标准 a-b-c（`inline > id > class > tag`）。打包期验证"只用了支持的选择器"，其余明确报错不静默降级。

### 5.2 S2 继承机制（推翻 color-only hack）

现状：`propagate_color_inheritance`（`dynamic.rs:370`）只处理 `color`，且用脆弱的值相等猜测"子是否声明"（`my_style == parent_base`，`:392`）。`ResolvedStyle` 不存 set-ness。

**做**：

- 给 `apply_decl`（`style/mapping`）加 set-ness 追踪：被规则显式写过的属性标记为 set；`ResolvedStyle` 加 per-property set 标记。
- 通用 inherited 传播 pass（按 tree order DFS，替代 `propagate_color_inheritance`）：`font-size`/`font-family`/`line-height`/`color`/`visibility`/`letter-spacing`/…（标准可继承集）；子**未显式声明**（看 set-ness，非值相等）→ 继承父 effective 值。
- 删 `propagate_color_inheritance` 的值相等猜测，**不复制 hack**。

**验收**：§3 断言 2。注意继承**不走 base_style 链**（每节点从自己 base_style 重起、不读父，§5.3），故必须是这个独立 tree-order pass——和现状架构一致，只是从 color-only 泛化。

### 5.3 S3 端到端 + base_style 契约（源码已确认，无悬念）

**base_style = 源码已定调，不是二选一**：`dynamic.rs:330` 白纸黑字 `let mut new_style = ...base_style.clone()`，每帧从 base_style 重起。故 spike **必须填 base_style**；无打包期 bake 时 **base_style = UA 默认 `ResolvedStyle::default()`**（所有节点）。运行时样式 = `base_style(default) + dynamic_rules cascade + S2 继承`。

**HTML→node 通路 = 测试内部 throwaway mini-bridge**：

- fence `parse_template`（真实）→ `IrTree`。
- 测试内部最小 `IrTree → 旧 Node` 映射：div→Container、span/文本→Text、button→Button；抽 class/id。约 30 行，标 `// ponytail: throwaway mini-bridge for spike; replaced by production bridge (Spec-3 ②) on new enum`。
- 规则表 → `scene.dynamic_rules`（喂给现成 cascade 引擎，§5.0）。
- 不做 SemanticKind 24 种 total 映射（生产桥，Spec-3）。

S3 集成测试 = §3 验收门。

## 6. P0（前置修复）处理

- **P0.2 删旧 `showcase.pkg.bin`**（`unity/showcase-unity/Assets/Bundles/ui/showcase.pkg.bin`，约 660KB，旧范式旧内容，commit a1a294b）——**本轮顺手删**。
- **P0.1（dll/绑定/源码同步）推迟**——触发条件是"重编 dll"；本轮不动 FFI、不重编 dll，`cargo test` 验收不踩雷。**真正需要是在碰 Unity（Spec-4）前。**
- **P0.3（pkg 升 v17 + bincode 稳定性测试）推迟**——本轮不扩 enum/bincode。**Spec-2 扩 enum 时连同 bincode 稳定性测试一起上。**

## 7. 非目标（显式排除）

- enum 扩容（5→~15 变体）——Spec-2。
- 生产 IrTree→TemplateNode 桥 + packer HTML 编排重建——Spec-3 ②。
- SemanticKind→新 NodeKind total 映射——Spec-3。
- C# 投影层（718 行壳）+ FFI/dll/Unity——Spec-4。
- `:nth-child`、`@keyframes`/`animation`、属性选择器 `[type=…]`（注：旧范式 `[data-controller]`/`[data-page]` 的 attr 匹配已在 core，但本轮新解析器不扩 attr 子集）、`!important`——选择器子集之外。
- **`<link>` 外部 sheet 加载**——showcase 8 页全部 `<link rel="stylesheet" href="preview/preview-base.css">`；`preview/preview-base.css` 是浏览器预览专用 polyfill（`preview/` 前缀），非运行时内容。外部 sheet 解析/加载 defer 到 Spec-3。spike 测试用合成 inline `<style>` HTML，不碰它。
- 视觉特效（渐变/box-shadow/filter/文字发光）——摸黑期纯色块占位（护城河是布局可预测，不是滤镜像素，roadmap §3.4）。
- PNG 软件渲染器——YAGNI，已放弃。

## 8. 风险与 verify-first

| 风险 | 应对 |
|---|---|
| 解析路径三选一不定，S1 工作量超预期 | S1.0 阻抗 spike 先行（~半天）：scraper 0.19 / cssparser+selectors / 硬化 `hand_selector` 三选一；选 (c) 有现成参考解析器兜底，选 (a)/(b) 有旧 packer 经验。超预期则回路线图重审 |
| S2 set-ness 追踪波及 `ResolvedStyle`/`apply_decl` | 先核对 `ResolvedStyle`/`apply_decl` 现状，局部加 set 标记，不重构整体 |
| spike 在旧 enum 上，后续迁移到新 enum 有返工 | S1/S2 产物设计成 enum-agnostic（解析器不关心 NodeKind；继承走属性层）；仅 mini-bridge 是 throwaway |
| `dynamic.rs:8-9` 注释 stale（声称"解析器在 fence"，其实没写） | 实现时顺手修正该注释为现状 |

## 9. 验收清单

- [ ] S1.0 阻抗 spike：三选一（scraper 0.19 / cssparser 0.34+selectors / 硬化 `hand_selector`）定解析路径 + 类型归属，回填本 spec §4。
- [ ] 改 fence 留存 `<style>` 文本（不当 shell 丢弃）。
- [ ] `<style>` → rule table，支持 class/tag/id/后代/伪类子集 + specificity。
- [ ] S2 set-ness 追踪 + 通用继承 pass，删 color-only hack。
- [ ] base_style 填 UA 默认；cascade 每帧基线正确（源码已确认须填）。
- [ ] S3 集成测试绿（§3 四条断言全过）。
- [ ] 删旧 `showcase.pkg.bin`。
- [ ] 修正 `dynamic.rs:8-9` stale 注释。
- [ ] `cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings` 过。
- [ ] `cargo test -p loomgui_core` + `cargo test -p loomgui_fence` 全绿。
