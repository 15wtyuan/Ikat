# LoomGUI 路线图

> v1 架构验证完成（v1a–v1e + showcase + v1.1–v1.8），桌面 Mono 可演示。
> 当前进入 **API 范式重构**：从"stage 全局 + NodeId 句柄 + 命令式 SetX"重做为"类型化对象树 + 标准 HTML 围栏 + CSS 优先"。
>
> **本轮路线的组织方式（2026-07-15 重定）**：不再横切 R2–R8 大层，而是**先摸黑打通一条最小骨架链、再沿骨架加宽**。原因见 §1。
>
> **进度（2026-07-17）**：Spec-1（阶段 S spike）✅；Spec-2（① core 类型化重构）✅；Spec-3 ②（IrTree 桥 + 打包编排）✅；**Spec-3 ③（cascade 收尾 + 查询出口）✅ 完成**——`ComputedNodeStyle` typed 快照（curated 子集，排 internal set-ness/几何/复杂视觉）+ Stage 查询出口（`get_node_kind` / `get_node_computed_style`）+ FFI 导出（return-code + out-param，避 NodeKind Container=0 哨兵撞）+ probe 集成断言（继承/specificity/class/kind 保真全绿，cascade 引擎健全坐实）。终点线1 加固：核心范式 headless 完全可断言，「rect 对 ≠ 语义对」盲区消除。全 workspace ~805 测试绿，fmt/clippy 清。**Spec-4a spec/plan 完成（2026-07-17），进入实现**：④ 拆 2 棒（4a 本机 headless / 4b Unity 验收）；4a = C# 投影层 + core 便签层 + headless harness。关键发现：core 无 inline override 层（`set_style` 写 `base_style`），4a 装便签层折进 rematch 单 set_map（propagate 零改）。详见 §2「④」、§8。
>
> 权威设计契约：`docs/design/main-design.md`（总体架构）、`docs/design/public-api.md`（公共 API 终态，21 条锁定决策）、`docs/design/projection-layer.md`（C# 投影层机制）、`docs/design/fence.md`（围栏）。

---

## 1. 为什么推翻旧路线（横切 → 摸黑打通 + 加宽）

**旧路线的病**：旧 roadmap 把重构横着切成 R2（整层核心树）→ R3（整层打包）→ R4（整层 C#）→ R5/R6/R7（控件/列表/文本）。每一层要"全做完"才算数，层与层互相咬依赖，于是每个 R 都是一座啃不动的大山，站在山脚无从下手。更糟的是"Chromium 差分门"——要跑浏览器对比就得先接一条 IR→render 线（本质是半个 R2+R3），于是差分门和大重写的边界糊成一团。

**真实现状（三方代码核实，是事实不是推断）**：整条"HTML→屏幕"的链，**只断在一处**——fence crate 的 `IrTree`（标注语义的 HTML 树）↔ core 的 `TemplateNode`（打包目标结构）之间没有桥。其余全是活的：

| 层 | 真实状态 | 结论 |
|---|---|---|
| 算法层（layout/text/scroll/tween/render 批合） | 成熟，609 测试绿，**且与"节点怎么表示"解耦**（走 NodeId + 并行表） | 复用，类型化时算法不动 |
| pkg.bin 格式 + 写包/读包/实例化 | 全链可用有测试（当前 v16） | 格式复用，本轮升 v17 弃旧兼容 |
| 单向渲染管线（Rust blob 21 列 SOA → Unity MirrorPool 镜像） | 成熟，三级 change_level | 不推翻，其上加回写层 |
| FFI 命令式面（55 个）+ Unity 后端（LoomStage/MirrorPool/EventHandler/InputCollector） | 成熟在跑 | 当投影层底座 |
| fence（HTML→IrTree，6 阶段流水线） | 成熟，79 测试 | 复用，但停在 IrTree |
| **断点** | **IrTree ↔ TemplateNode 之间没有桥** | 需新写 |
| core 表示层（NodeKind 5 变体 enum + 27 字段巨 struct + NodeId u32 句柄） | 纯旧范式，Button 行为≈Container | 类型化重构（档位2，§3.1） |
| C# 终态签名（Public/LoomGUI.*.cs 718 行） | 纯 NotImplementedException 壳，与旧实现零重叠 | 实现为转发层 |

**三个文档没写清、但实测暴露的真相**：
1. **CSS cascade 完全不存在**。fence 只吃 inline `style=""`，`<style>` 块被当 shell 标签整个丢弃，class/继承/伪类全没做——而 showcase 样式 99% 写在 `<style>`+class 里。cascade 是本轮最大的**新建**代码。
2. **"类型化"没想象中值得上重型方案**。core 里 Button 和 Container 行为几乎一样，真正的类型化用户表面应活在 C# 投影层，Rust 侧只需语义 enum。见 §3.1 档位2 决策。
3. **后端对象层要新写，但"薄"**。718 行签名壳要填，但它下面垫着成熟的旧实现，做的是"翻译/转发"而非从零造功能。

**新路线的形状**：先把断点接上、把 core 表示层换成新范式、把 cascade 补上，一路推到"一段 HTML → 打包 → core 建树 → layout → 吐 render nodes → 断言矩形坐标"这个 headless 测试变绿（**终点线1**，纯 Rust、本机可验）；再填后端对象层，推到 Unity 真机端到端能用（**终点线2**）。这一整程叫**摸黑**——核心范式没有端到端通过一次，靠自动化测试兜底摸黑慢跑，通了再加油。摸黑之后按三束能力沿骨架加宽（§4）。

---

## 2. 摸黑一程：施工图

> 目标 = 把新范式的骨架链（div + 文字 + 图 + flex + cascade）从 HTML 一路打通到渲染，全程自动化测试兜底。骨架链**只做布局正确性**，渐变/阴影/滤镜/文字特效先用纯色块占位（护城河是"布局可预测"，不是"滤镜像素"，见 §3.4）。

> **顺序原则（红队修订 2026-07-15）**：真风险不在 core 表示层重构（那是最机械的活，编译器牵着走 81 处 match），而在 **cascade 和桥**——三份代码核实推翻了本轮最初"复用已有 rematch 扩一下"的假设：CSS **选择器解析器根本不存在**（只有匹配器）、CSS **继承只支持 color 且是脆弱 hack**、打包器 **HTML→pkg 编排整段被删**。所以**先探真风险（spike），再动大重构**。

### P0 · 前置修复（必须最先）

1. ~~**dll/绑定/源码同步**：入库 `.dll` + csbindgen 绑定比 Rust 源码**过期**（`loomgui_stage_load_html` / `loomgui_stage_set_rich_text` 已删源码、dll 里还在，旧 showcase 仍调）~~ **✅ 已解决（Spec-4a 重编 .dll + b03929a 修 stale caller）**。三者当前同步，重编不再踩雷。
2. **旧 `showcase.pkg.bin` 直接弃用**：入库的 `showcase.pkg.bin`（commit a1a294b）是**旧范式时期打的、装的是旧 showcase 内容**，与靶子期重写的新 `showcase/showcase/*.html`（8 页，从未打包）**不是同一个东西**。个人项目不考虑兼容 → **旧包彻底抛弃，不为它做任何基线快照、不保留读旧格式能力**。整程的真基线是"新 showcase HTML 在浏览器里的 rect"，等打包链（②）通了才谈得上。
3. **pkg 格式版本 = 一刀切升 v17，不留后向兼容**：扩 `NodeKind`（5→~15 变体）改 tag 空间 + 拆 `Node` struct 会改 bincode 形状。直接升 v17、`MIN=MAX=17`（弃 v16，无迁移器）。加 bincode 稳定性测试（序列化形状变了就红），别撞运行时 `BadKind` 才发现。

### 阶段 S · cascade + bridge 探路（spike，在当前 5 变体 enum 上，先行）— ✅ 完成（2026-07-16）

> **状态：DONE**。spec `docs/superpowers/specs/2026-07-15-cascade-spike-design.md`、plan `docs/superpowers/plans/2026-07-16-cascade-spike.md`、commit `1c21b4d..9acd01e`（已 merge main）。`div/span` HTML → `<style>` cascade → rect 端到端打通；4 条验收断言（rect / font-size 继承 / class 命中 / display:none 剪枝）全绿（`crates/fence/tests/cascade_spike.rs`，opus 逐条 trace 生产源码确认非假绿）。三个致命假设全部证成，路线可继续 Spec-2。
>
> **产出（非废弃，③ 主体）**：(1) fence `css_rules.rs` 选择器解析器——**路径 c 手搓，零新依赖**（cssparser/scraper 全仓未引；cssparser 是分词器不给 selector AST，初版"接 cssparser"前提被推翻），直产 core `ParsedSelector`/`DynamicRule`；子集 class/tag/id/后代/伪类 + specificity。(2) fence 留存 `<style>` 文本 + 解析规则表接进 pipeline（`ParsedTemplate.dynamic_rules`）。(3) core `propagate_color_inheritance`（color-only hack）→ 通用继承 pass（set-ness bitmask，**transient 不进 `ResolvedStyle` / 不升 pkg 版本**）。cascade 引擎 `rematch_pseudo_classes` 本就完整（遍历全规则 + specificity 合并 + base_style 每帧重起），spike 只产规则表喂它。
>
> **throwaway（Spec-3 ② 替代）**：S3 的 IrTree→Scene mini-bridge 仅测试内部（div→Container / span→Text），不做 SemanticKind 24 种 total 映射。
>
> ⚠️ **Spec-2 前置（spike 挖出，必做）**：set-ness 现只追踪 **dynamic cascade** 写的属性 → 打包期声明（baked 进 base_style）的继承属性拿不到 set-bit，生产环境会被父运行时值覆盖（spike 不触发：base=default、全 dynamic）。**Spec-2 升 v17 + 拆 `ResolvedStyle` 时，set-ness 须打包期 bake 进 base_style**（见 §2 ①、§8）。另：anim-text-color 跨节点 override 随通用 pass 一起丢了，标 ponytail 推 Spec-3。

原 spike 计划（保留对照）——在**不动** core 表示层的前提下，用 `div`/`span` 最小例先打通 `HTML → cascade → rect`，把三个致命假设探掉再决定投入大重构：

- **S1 选择器解析器（净新代码，最大一坨）**：接 `cssparser`（CLAUDE.md 已钉 0.34，但**尚未接进任何 crate**）→ 把 `<style>` 文本 tokenize、拆规则、解析 compound 选择器、从源码算 specificity → 产 `Vec<DynamicRule>`。**先做 cssparser ↔ core 已有 `ParsedSelector`/`Compound` 的阻抗 spike**（0.34 的 selector AST 不是 drop-in）。
- **S2 继承机制（推翻 color-only hack）**：给 cascade 加"属性是否**显式声明**"追踪（`ResolvedStyle` 当前只存 resolved 值、不存 set-ness）→ 通用 inherited 属性传播（font-size/font-family/line-height/... 全部），**删掉** `propagate_color_inheritance` 的值相等猜测，别复制它。验收测试必须断言：只在祖先规则里设了 `font-size` 的文本节点，字号正确继承。
- **S3 最小应用 + rect 验证**：`div/span` HTML → S1 规则表 → S2 继承 → cascade 合并 → layout → 断言 rect。这一步绿 = 三个假设都被证伪或证实，路线可继续。

> S 是**探路**不是废弃代码：S1/S2 的产物（解析器 + 继承机制）就是 ③ 的主体，只是先在旧 enum 上验证可行性，避免在移动靶子上写。

### 摸黑主线（纯 Rust，依赖驱动顺序）

**① core 类型化重构（档位2，见 §3.1）** ✅ DONE — Spec-2 完成（commit `c77967c..221c558`，branch `codex-spec-2-core-refactor`）
- **Spec-2 必做前置（spike 挖出）**：拆 `ResolvedStyle` + 升 v17 时，把"属性是否显式声明"的 **set-ness 一起 bake 进 base_style**。spike 的 set-ness 只追踪 dynamic cascade，打包期声明拿不到 bit → 生产环境继承会被父运行时值覆盖（详见阶段 S ⚠️ + §8）。这是 ③ cascade 收尾能正确处理打包期声明的前提，趁拆 struct + 升版本一次做掉。
- `NodeKind` enum 从 5 变体扩容到承载围栏语义（div/text/image/button + 各控件语义），`match` 分发。**81 处 `NodeKind::` match（14 文件，重灾 scene/dynamic.rs 32、layout/mod.rs 17）编译器会牵着逐处过。**
- 控件私有状态（slider value/min/max、dropdown selectedIndex、textfield value/光标）走 **side table**（按 NodeId 索引的稀疏表），续用旧代码 anim/scroll 已在用的 `HashMap<NodeId,_>`/并行数组模式——**不塞进统一 struct**。
- 共性运行时 flag（hovered/active/disabled/focused）收进 bitflags；27 字段巨 struct 按关注点（结构/几何/运行时状态）拆分。
- 类型化的**用户可见表面留给 C# 投影层**，Rust 侧只需语义 enum + match，**不上 trait object**。
- FFI 的 `node_id:u32` ABI 约定**不砸**（enum 扩变体不影响 ABI）。

**③ cascade 收尾（把 S 的 spike 产品化，见 §3.2）** ✅ DONE — Spec-3 ③ 完成（commit `22e922d..a010508`，branch `spec3-cascade-finalization`，merge `e05fe96`）

> **状态：DONE**。spec `docs/superpowers/specs/2026-07-17-cascade-finalization-and-projection-design.md`、plan `docs/superpowers/plans/2026-07-17-cascade-finalization.md`。③ 验收（终点线1 加固）：probe HTML 经新查询出口断言 继承/specificity/class/kind 保真 全绿——cascade 引擎健全坐实，「rect 对 ≠ 语义对」盲区消除。
>
> **关键定论**：① ③ 的真缺口**不是「重写 cascade 引擎」**（引擎已在子集内跑通全量标签，spike + ② 已验），而是**可观测性出口**——Stage 之前只有 rect/visible，无 computed-style / node-kind 查询，无法断言 cascade 正确性。补 `Stage::get_node_kind` + `get_node_computed_style` + `ComputedNodeStyle` typed 快照（从 ResolvedStyle 投影 curated 子集，**排除** internal set-ness 位图/taffy 几何/复杂视觉——三方调研锁定，不用全量 struct 导出也不用按属性字符串查）。② **base_style 每帧重起已确认**（`dynamic.rs:359-360` 每节点每帧 `base_style.clone()` 重起 + `:376` set-ness 双源），无需重构 rematch——§3.2「不符则重构」警告不成立。③ FFI 用 **return-code + out-param**（非 `-> u8` + 0 哨兵）：NodeKind 首变体 Container 判别值=0，0 哨兵会和每个 div 撞——spec review 抓到的真 bug。enum 稳定化：`#[repr(u8)]` 用 `as u8`，无 repr（TextAlign）/外部类型（taffy FlexDirection）走 `match`。④ kind 保真（§3.3 防「假绿」）由 `get_node_kind` 直接断言兑现——控件不塌 Container。⑤ computed-style 出口 C# 落点 = 并列 `NodeComputedStyle` 只读 struct（不扩 NodeGeometry，对齐 public-api.md 三分模型「computed 走只读快照层」）——④ 实现。.dll 重编 + binding sync（csbindgen v1 实际生成了 `ComputedNodeStyleRepr` struct stub，④ 直接用）。
>
> 原 spec 五 bullets（保留对照，全证成）：
- 把 S1 解析器 + S2 继承接到扩容后的 enum 上，覆盖全量围栏标签/控件。规则表写进包（配合 P0 的 v17）。
- core 当**唯一 cascade 引擎**：节点 class 集 + 伪类态 对规则表匹配 → specificity 排序合并 → 继承。伪类复用已有 `rematch_pseudo_classes`（这部分**确实**是复用）。
- `base_style` **不是可选缓存，是 rematch 的每帧基线**（`rematch` 每帧从 base_style 重启，见 §3.2 修订）——"摸黑期不做 base_style"的说法作废，必须填 base_style 或重构 rematch 契约。
- 选择器摸黑 MVP 只支持子集：class / tag / id / 后代组合 / 伪类。`:nth-child`、`@keyframes`/`animation` 是否进 scope 见 §3.5（关系到终点线2 能否跑 home 动画）。打包期验证"只用了支持的选择器"。
- **cascade 单独上集成级测试**（整段 HTML+CSS 断言 computed style）——历史翻车点，per-property 单测兜不住。

**② Ir→TemplateNode 桥 + 打包编排（比"一个函数"大）** ✅ DONE — Spec-3 完成（commit `aee3716..349ccb2`，branch `spec3-ir-bridge`）

> **状态：DONE**。spec `docs/superpowers/specs/2026-07-16-ir-bridge-packer-design.md`、plan `docs/superpowers/plans/2026-07-16-ir-bridge-packer.md`。原三 bullets 全落地（见下"原 spec"）。终点线1 smoke 绿：HTML 字符串 → 内存 pkg.bin → `load_package` → `instantiate` → `tick_and_render` → 断言 rect（width=200）+ display:none 子树剪枝 + class 命中 computed style（`tests/smoke_ir_bridge.rs`）。
>
> **关键定论**：① pkg 格式 v17→**v18**——`kind_tag` 从独立 tag 表改为 **`NodeKind` 判别值 `u8`**（`#[repr(u8)]` + `from_u8`，全 23 变体），删 RichText 死字段（layout/render 旁路清，`text/rich.rs` 算法保留待复合束）；加 BadKind 测试（unknown kind_tag 走 `read_package` 错误路径）。② IrTree→TemplateNode **桥放 packer**（非 fence 非 core）：DFS 翻译，全局 IrNodeId→局部 parent_idx，SemanticKind→NodeKind **1:1 total 映射**（22 可映射语义各有专属 NodeKind，无 collapse；InputDispatch/Template/None → Err，semantic_hint 无需），抽 class/id/tabindex/src/data-controller。③ ResolvedStyle.inherited_set **inline bake**（修坑 161——inline style 的继承属性打包期拿不到 set-bit，运行时被父值覆盖）；坑 161 双源统一（inline + dynamic 都走打包期 bake）。④ packer HTML→pkg.bin 编排重建（`pack_components`：invoke fence → bridge → write_package v18 → 回填 packages），`referenced_sprites` 回接 atlas 校验闭环。⑤ fence 顶层空白文本根跳过（smoke 撞到的真实 bug，非假绿）。
>
> 原 spec（保留对照）：
- 桥：fence 产物翻译成新 core 形状 `Vec<TemplateNode>`（DFS、全局 IrNodeId→局部 parent_idx、抽 class/id/tabindex、img src 归一化）。
- **打包编排（被删的部分，不止桥）**：packer 的 `invoke fence → 建 ComponentTemplate → 从 cascade 填 dynamic_rules → write_package → 回填 packages` 整条编排层在 `d8fe705` 被删，要重建，不只写个翻译函数。加 `loomgui_fence` 依赖。
- **SemanticKind（25 种）→ 新 NodeKind 映射必须 total 且非静默**（§3.3）：每个可映射语义各有专属 NodeKind（1:1，无 collapse；InputDispatch/Template/None → Err），加测试断言"无 showcase 元素静默丢失语义标签"。绝不 lossy-and-silent。

### 🏁 终点线 1（纯 Rust，headless，本机验）

**真 smoke 测试在 ② 之后**（① 阶段 HTML 还进不来，那时只能拿手搓 Rust 树测"表示层机制"，**证明不了范式**、也覆盖不了新控件——别把它当范式验收，是"假绿"最危险的一种）。

**② 后的端到端**：HTML 字符串 → 打包（内存 bytes）→ `load_package` → `instantiate` → `tick_and_render` → 断言。判据**不只 rect**（§3.3）：同时覆盖 cascade 继承传播、display:none 子树剪枝、class 命中 computed style、SemanticKind→NodeKind 无静默丢失。rect 对 ≠ 语义对。绿 = 核心范式活了。

### ④ 后端对象层（依赖终点线1）

> **状态：Spec-4a DONE（2026-07-18）**。spec/plan 同上，commit `354ab7d..9b04dd6`（36 commits），branch `spec4a-projection-layer`。core 便签层（`inline_override`/`inline_set` 折进 rematch 单 set_map，propagate 零改）+ Rust FFI 缺口（set/unset_inline_override、get_children/get_child_count、add/remove/has_class）+ parse_color 8-hex + C# 投影壳（NodeRegistry/NodeFactory 全 23 变体 dispatch/lazy children/StyleMirror 稀疏镜像+FlushInline seam/Geometry 直读 FFI/Transform 标脏 defer/ClassList/Container 读写/Get-Query 作用域/生命周期）+ typed 事件层（RouteEventCore sealed class + 18 event struct + EventBus On&lt;T&gt; capture/bubble/once + demux 接线 + 语义糖 Clicked/Activated + 业务字段从 raw 填）+ headless harness（P/Invoke 真 dll 不碰 Unity）+ fixture pkg.bin + E3 验收门（spec §4 全 9 条真断言绿，含 Criterion 5 假绿修正）。**300 HeadlessTests + PublicApi 编译门 + 626 core 测试 + dll md5 synced**。final review Critical none（sole blocker cargo fmt 已修）。**4a 比 4b 重坐实**（还 inline override 债 + 投影壳 + 事件层，非填壳）。
>
> **D2 修订**：RouteEventCore 由 spec 的 struct 改 sealed class——`Action<T>` 按值传 struct 致 handler `StopPropagation()` 突变作用副本丢失，class 共享堆引用恢复 DOM 突变语义（D1 全测不回归）。
>
> **deferred（track for 4b / core-fix / roadmap）**：① core bug `dynamic.rs:231` remove_child 非直系子误清 parent（C# GetChildIndex 守卫，非 C# FFI caller 风险，独立 core-fix）；② `create_node`/`set_text` null css → `from_raw_parts(null,0)` UB（pre-existing FFI，core null-check 待修）；③ D3 3 source-less event structs（ScrollChanged/AnimationStart/Iteration 无 core 事件源）；④ `kind_from_tag` 4-tag vs `resolve_semantic` 23-tag（dynamic span→TextNode 不匹配 Query("span")，C7 doc caveat）；⑤ LoomEventHandler 并行 backward-compat（AddListener→On&lt;T&gt; 迁移后移除）。
>
> **④ 拆 2 棒**：**4a（本机 headless）** = C# 投影层 + core 便签层（inline override）+ Rust FFI 缺口（child/class/inline）+ typed 事件层 + headless harness，全部编码机验；**4b（家里机 Unity）** = 终点线2 真机验收 + 字体/文本 + 复用 MirrorPool/InputCollector/EventHandler。
>
> **关键定论**：① **core 无 inline override 层**——`set_style` 写 `base_style`（`scene/dynamic.rs:260`），projection §2.2「复用现有 FFI 零改动」与 public-api §3.1「Style 是 inline override 层」假设落空。4a 装便签层（Node 加 `inline_override`+`inline_set`），**折进 rematch 单 set_map**，`propagate_inherited` 零改（`style/dynamic.rs:329-456` 核实：inline 继承属性经 set_map 自动传播）。② **即时过桥版 = 最终版子集**：StyleMirror 稀疏镜像 + FlushInline seam（getter 契约强制镜像存在），升级攒批只改 setter 调用时机，不推翻。③ set_transform / 攒批 flush / 控件业务事件 / @keyframes 推后（spec §5 defer）。④ **4a 比 4b 重**（还 inline override 层的债，非填壳）。⑤ FrameBlob 21 列无 rect → Geometry 4a 直读 FFI（blob 缓存推后）。
>
> 下方原 ④ 目标 bullets（保留对照，部分被上方定论修正：set_style 不胜任 Style，需便签层 FFI）：

- 填 `Public/LoomGUI.*.cs` 718 行签名壳（52 个 `NotImplementedException`）→ 每个方法**转发到成熟的旧 LoomStage 命令式 API**（`Style.Width = Px(100)` → `SetStyle(nodeId, "width:100px")`）。翻译层，非从零造功能。
- 加 **NodeId→Node 强引用缓存**（对象身份稳定，投影层 §2.4）+ **节点类型识别**（Instantiate 返回 NodeId 后要能造出正确子类；当前 FFI **无 node-kind 查询导出**，需新增 FFI 命令或从 dump_scene 绕）。
- typed 值转换：`Length`/`Color` ↔ CSS 字符串。
- **事件 typed 层（命名子任务，在关键路径上）**：旧 `EventHandler` 是 `EventType(byte)+nodeId` 面，没有 typed 分发。`node.On<ClickEvent>()` / `button.Clicked +=` 是**新 C# glue**（byte-EventType→typed-event demux + per-node 路由），不是"零改复用"。终点线2 要求 `Clicked` 触发 → 这块在关键路径，不是可选。
- 渲染/输入/**底层**事件路由管线（MirrorPool/InputCollector/borrow_events）**零改复用**（要不要跟改，④ 干活时再调研）。
- **headless C# harness（破两台机串行瓶颈）**：④ 的 ~52 个方法多数可在**编码机**上用 console test 直接驱动真 dll 的 `LoomStage` 验证，不必每次 commit-dll-push 去家里机跑 PlayMode。只把真正依赖渲染/输入的检查留给 Unity 机。
- **不做攒批回写 / set_transform**——即时过桥兜底（低频够用），标 `ponytail:` 欠债，留到"第一个高频改值控件"再上（§3.5 + §4 控件束）。⚠️ 见 §3.5：transform 债可能在终点线2 就爆。

### 🏁 终点线 2（Unity，家里机验收）— ✅ DONE（Spec-4b，2026-07-20）

`UIContext → LoadPackage → Instantiate → Get<Button> → Clicked → 真机渲染`。新范式端到端能用 = 摸黑结束。

> **状态：DONE**。spec `docs/superpowers/specs/2026-07-18-spec4b-unity-acceptance-and-backend-retirement-design.md`、plan `docs/superpowers/plans/2026-07-18-spec4b-p3-unity-acceptance.md`。三 phase 落地：
>
> - **P1 摸黑清理**（branch `spec4b`，commit `cb7e595..d4c0f28`）：Controller 全链删（core state 机 + 4 FFI + 6 测 + C# wrapper + ControllerEntry/ControllerSection schema + LoomEventHandler 连锁删 + Demo 目录）+ pkg v18→**v19**（Controller schema drop）+ set_style 死路径 FFI 删（`apply_css` 留）+ rich_link_at 死链删（`text/rich.rs` 算法留）+ dump_controller example 删。
> - **P2 多引擎分层**（commit `8e2df1c..d4c0f28`）：新 `Runtime/Host/` 引擎无关层——`LoomHost`（持 stage handle + UIContext，零 `using UnityEngine`，驱动 main-design §16 五步序 CollectInput→tick→borrow_frame→SyncFrame→borrow_events typed 路由）+ `LoomBackend` 抽象契约（main-design §17 三件事）+ `UnityLoomBackend : LoomBackend`（持 MirrorPool/MaterialManager/NativeHostManager/SpriteResolver/InputCollector，从 LoomStage 搬过来零改）。**LoomStage 退役**（commit `9495c88`，589 行业务 API 透传层整层删，driver 改走 LoomHost + ctx.LoadPackage）。deferred ①②顺手清（core `remove_child` 直系子校验 commit `56e7669` + FFI null-safe css/text/src/prop commit `80552a0`）。
> - **P3 Unity 真机验收**（commit `b8cab3c..8abe8a2` + 后续 P3.4 a/b/c 修补）：手写 spec4b-acceptance 最小页（div/button/img/text + flex + cascade + class，剥 @keyframes/transition/progress/input/hover transform）→ pkg v19 → Unity PlayMode 真机跑 §5 四门。**逻辑 4 门全绿**：渲染（div flex column/row + span text + img + button）+ Get&lt;Button&gt;("btn-back") 作用域查找 + Clicked typed 事件全链通（InputCollector→set_input→process→borrow_events→EventDemuxer→EventBus→On&lt;ClickEvent&gt;）+ class 命中（card-2 highlight → card-text computed color = #e94560）+ card-1 LayoutRect w≈300（CSS width:300px）跨层一致。**视觉 4/5 通过**（Buy 居中 / 间距收紧 / tofu 解 / Back 透明无底图）；card-img Image bg 紫底机制 tech-debt 留（§4 tech-debt 段）。fence `@keyframes`/`animation` DSL 已对齐 public-api §9 终态（commit `e2e2812`），runtime 驱动留 §4 视觉束 v1.10 后续。
>
> **关键定论**：① **LoomStage 退役是 clean break，不留双壳**——589 行业务 API 透传层（29 个零活体 caller）整层删，driver 唯一活体 caller `LoadPackage` 迁 `ctx.LoadPackage`，生命周期/后端编排（Tick/RegisterFont/SetImageSizes 等 ~10 个）按多引擎接缝迁 LoomHost/UnityLoomBackend。② **多引擎接缝落点 = "驱动核心 + 后端契约"两层**——LoomHost 持 stage 驱动序（main-design §16）零 UnityEngine，LoomBackend 抽象契约（CollectInput / SyncFrame / 资源对象上传）；Godot-C# 未来复用 LoomHost + 整个 Projection + Public，只写 GodotLoomBackend。③ **borrow_frame FFI 调用放 LoomHost**（产生引擎特定镜像对象的 FFI 归引擎无关驱动核心），**set_input FFI 在 backend**（采集引擎特定但 FFI 引擎中立，省一次交互）。④ **Card-img Image bg tech-debt**：Unity 后端按 node_id 去重，Image bg+texture 同 node_id 只画 texture；要合成 node_id 机制（IMG_BG_FLAG + Unity 建 2 GameObject + sort_key propagate bg 在 texture 下），本轮补丁（73e560e）reverted（8ea81aa），机制后续商议。⑤ **空白 TextNode 撑开布局**：HTML 元素间换行+缩进被建成 TextNode，layout 没折叠成 flex item 撑开间距/挤压兄弟（坑 163）——layout build filter + render skip + write_back 早返（commit `3916a1c`）。⑥ **tofu 字体根因**：font-family 显式声明才进 ResolvedStyle.font_family，未声明 fam=None → tofu；spec4b 显式 `LXGWWenKai` 解。
>
> **两台机约束**：终点线1 纯 Rust、本机跑 headless 测试即可，核心范式对不对在本机就锁死；终点线2 才需搬去家里机跑 Unity PlayMode。核心风险在本机验完再进 Unity，不会"搬过去才发现核心错了"。
>
> **摸黑结束**。下一阶段 §4 三束加宽（控件束/复合束/视觉特效束）。

---

## 3. 摸黑期的关键决策（本轮 grill 锁定）

### 3.1 core 类型化 = 档位2（扩 enum + side table，不上 trait object）

Rust 侧**不做**"每标签一 struct / trait object"。理由是合理性，非工作量：
- **类型集封闭**：围栏钉死 23 标签 / ~15 控件，运行时不能新增节点类型。trait object 为开放扩展服务，而开放扩展已被设计禁止。
- **热循环要数据局部性**：core 每帧遍历全树三遍（layout/build/world），trait object 散堆 + vtable 跳转，直接砸紧批合（N→1 draw call）这个对 RmlUi 的差异化卖点。游戏引擎走数据导向。
- **类型化用户表面活在 C# 投影层**：business 程序员碰的 OOP 类型化在 C# 兑现，Rust 只需 enum 语义——这是 projection-layer"真身 Rust、C# 投影"的直接推论。
- 控件私有状态走 side table（不是塞统一 struct）：UI 树 90%+ 是普通 div，控件稀疏。

> **文档待改**：main-design 若有"Rust 类型化节点层级"暗示 trait object 的措辞，显式降级为"Rust 用扩容 enum 承载语义，类型化对象树只在 C# 投影层兑现"。

### 3.2 cascade = fence 解析成规则表 + core 唯一引擎（规则表必须进包）

决定因素：**逻辑层大量用运行时 CSS**（`Classes.Add/Replace`、`StyleSheet.Add`、class 切换驱动动画）。若打包期把规则 bake 进设计期节点然后丢掉规则表，运行时对没带该 class 的节点 `Classes.Add("compact")` 就完全失效——"CSS 优先"赌注破产。所以**规则表必须活到运行时**，cascade 引擎跟着放 core。

- fence = 纯解析器（文本→规则表），不做 cascade 决策。**注意：解析器是净新代码**——core 现有的是匹配器（`compound_matches_node`/`rematch`），把 `<style>` 文本解析成选择器 + 算 specificity 的部分**不存在**（唯一构造点是测试 helper），cssparser 依赖也尚未接入。这是本轮最大一坨新代码，见 §2 阶段 S。
- core = 唯一 cascade 引擎，实例化算一次、class/伪类/Style override 变化触发 rematch。
- **继承是新机制，不是"扩一下"**：现有 `propagate_color_inheritance` 只处理 `color`、且用脆弱的值相等猜测"是否声明"，**不能推广**。要给 cascade 加"属性是否显式声明"追踪（`ResolvedStyle` 当前不存 set-ness）+ 通用 inherited 传播，删掉 color 猜测。
- 单一真相源：cascade 语义只 core 一份，避免"fence 一套 + core 一套"漂移（CLAUDE.md 禁的双份白名单）。
- 和"真身在 Rust"咬合：cascade 是核心逻辑，Godot/UE 全共享。
- **base_style 不可"摸黑期不做"**：`rematch_pseudo_classes` **每帧从 base_style 重启**（它是每帧 cascade 基线，不是首帧缓存）。"实例化时现算、不填 base_style"会让每帧伪类重 cascade 丢基线。实现前先核对 rematch 契约，要么填 base_style，要么重构 rematch。

> **文档待改**：main-design §5.3"打包期展开继承到 base_style；运行时 rematch 只处理伪类"这句与"运行时任意 class 切换要重 cascade"矛盾，改成"规则表进包；运行时 rematch 处理伪类 + class + Style override 变化，每帧从 base_style 重算基线"。

### 3.3 终点线1 判据 = rect + 语义，不只 rect

项目历史证明"单测全绿但 CSS 集成错"（text-decoration CSS 规则形式静默忽略、display 子树剪枝等）。所以终点线1 不能只断言矩形坐标：
- rect 对 ≠ 语义对。要同时验：cascade 继承传播、display:none 剪枝、class 命中 computed style、SemanticKind→NodeKind 映射无静默丢失。
- **"假绿"风险**：控件塌成 Container/Button 时，页面渲染出来了（rect 对），但控件语义（这是 Slider 不是 div）丢了，要到很后面才暴露。映射表加断言"每个 SemanticKind 显式有对应或显式 defer"，别静默塌陷。

### 3.4 护城河判据 = 布局可预测，不是滤镜像素

放弃"和浏览器像素级 diff"。理由：home/lab 页满是渐变、多层阴影、filter、文字发光、渐变字，自绘渲染器不可能和 Chromium 像素一致，追求它等于复刻半个浏览器。真正的护城河 = **AI 能预测布局**（盒子摆哪、文字怎么换行、谁在谁上面），这个能用 rect 数字自动比对（headless Chrome 导出 DOM 布局矩形 vs LoomGUI render node rect）。渐变/阴影/滤镜是"好不好看"（FairyGUI 也能好看），护城河之外，摸黑期纯色块占位。

> **放弃了 PNG 软件渲染器**（YAGNI）：headless rect 断言 + Unity 视觉验收两头夹掉了它——摸黑期用不上（rect 断言不产像素），最终验收去 Unity。

### 3.5 攒批回写 / set_transform 推迟到第一个高频控件

摸黑期低频改值（progress.Value 偶尔设）用即时过桥（一行 FFI，标 ponytail 注释），不上 projection-layer 那套攒批 flush 重机器——那套为高频（拖拽/动画每帧改）设计，无高频负载时造了没处使。

⚠️ **两个可能在终点线2 就爆、而非"以后"的雷**（红队 I4）：
1. **Transform 债**：Transform（Position/Scale/Rotation）是公共 API。从 C# `SetStyle("transform:...")` 字符串走 cascade 每帧重应用，表达不了公共 `Transform` API 隐含的逐帧动画值。
2. ~~**@keyframes 根本没进解析器 scope**~~（**已解**，commit 见下方）：home.html 的入场动画是 CSS `@keyframes`+`animation`，原 §2 的选择器 scope 不含。**fence 已加 `@keyframes` at-rule + `animation` 属性 DSL**（对齐 public-api.md「动画定义全在 CSS」终态）；runtime 驱动（keyframes 表序列化 + tween 发射）仍留 §4 视觉束 v1.10——本轮 fence 接受语法、bridge 静默丢弃、runtime 收到 animation 声明不报错不跑动画。pkg.bin 格式不变（避免版本 bump + dll 重编），keyframes 序列化随 §4 一同落地。

**决策**：终点线2 的验收页要么**选一个不含 @keyframes/transform 动画的 showcase 页**当门（推荐，把动画留到 §4 视觉束/控件束），要么把 @keyframes/animation 解析 + set_transform 提前拉进 S/③/④ scope。不要让"transform 债留以后"和"home 当 demo"硬撞。

**P3.4c 落地**（@keyframes DSL）：`@keyframes` at-rule + `animation` CssPropSpec（parser Animation）入 fence crate。keyframes 规则在 ParsedTemplate.keyframes 暴露；packer bridge 当前静默丢弃（不序列化进 pkg.bin）；css_resolve 给 `SemanticKind::Button` 加默认 `justify-content/align-items: center`（修 Buy 字不居中 bug——core dump 实证 text.x=269 居中 btn-buy.x+w/2=280）。runtime 驱动（@keyframes 表 + tween 发射）留 §4 视觉束。

---

## 4. 摸黑之后：三束加宽（笼统，验证完再详细展开）

摸黑打通的是"div + 文字 + 图 + flex + cascade"骨架链。之后沿骨架加宽，大致三束，不排死顺序（每束的细化留到摸黑验证完）。加宽的加料方式统一是"core 加语义 + cascade 加伪类 + 后端加对象类"，由 showcase 页面逼出需求，不凭空补理论清单。

**控件束**：progress → input 全家（text/password/number/range/checkbox/radio）→ select/textarea → 滑块/开关。
- 第一个真正高频改值的控件出现时，**还摸黑期欠的债**：上攒批回写 flush + set_transform 数值 FFI（projection-layer §2）。
- 吸收旧 v1.9（TextInput/IME）、v1.12（滑块/进度条）、v1.13（DragDrop/Window/Popup）大部分功能。
- WAI-ARIA 复合控件（TabList/Tree 等，role dispatch）——签名级结构缺口，单独立项。

**复合束**（三块各是硬骨头，独立推进，不挤一片）：
- **ListView 虚拟化**：`ul/ol/li/template → ListView/ListItem`，把 driver 层虚拟化（池化/可见区/不等高补偿/content size/reuse key）全吸收进框架。吸收旧 v1.4 + v1.11。
- **文本模型回归标准子树**：删 `display:block` RichText 暗号，`p/h1-h6` 建文本 block、inline 元素是语义容器；内部编译成 TextRun/ImageRun/LinkRun，公共树保留 TextNode/TextElement/Image/Link 的 ID 和事件。复用 v1.6 字体自绘 + v1.8 文字效果算法，换表达方式。吸收旧 v1.7。
- **Custom Element + slot 组件系统**：用户业务组件（含 hyphen 标签名）+ 标准 `<slot>` 内容投影 + Package 注册表（`customElements.define` 角色）。**当前无人用它做游戏，排最后**。

**视觉/特效束**（摸黑期纯色块占位的东西，护城河之外，按需补）：渐变（linear/radial）、多层 box-shadow（含 inset）、filter（grayscale/brightness/hue-rotate）、文字特效（发光/描边/投影/渐变字）、transform 视觉变换（rotate/scale/translate）、border-radius 各形态、九宫格。这些多数 v1.8 已有算法（保留），换到新范式的表达。

**最终验收**：终态 showcase 8 页（home/settings/mail/inventory/shop/character/form/lab）全部在 Unity 真机跑通 + 布局与浏览器 rect 比对一致。

### tech-debt（摸黑期 deferred，按机制 / 修法归档）

> 摸黑打通骨架链的过程里有意 defer 的非阻塞项，按「后续在哪个束 / 机制草稿待商议 / 直接修」分类。新加 tech-debt 统一进这里，写法：症状 / 根因 / 处置路标。

- **card-img Image bg 合成 node_id 机制**（Spec-4b P3.4 视觉 1/5 未过；机制后续商议）：Unity 后端按 node_id 去重，Image bg + texture 同 node_id 只画 texture。要照 box-shadow `BOX_SHADOW_FLAG` bit 28 合成 id 模式：core render Image bg 用 `IMG_BG_FLAG` 合成 id + Unity 后端建 2 GameObject + sort_key propagate（bg 在 texture 下）。本轮补丁（commit `73e560e`）reverted（`8ea81aa`）——机制草稿待商议后再实现。
- **keyframes runtime 驱动 + fence 动画子集补全**（§4 视觉束 v1.10 后续）：fence `@keyframes` at-rule + `animation` 简写 DSL 已完成基础语法校验（commit `e2e2812`），但存在以下缺口，全部归入视觉束与 runtime 驱动一同落地：
  - **runtime 驱动缺失**：keyframes 规则在 `ParsedTemplate.keyframes` 暴露但 packer bridge 静默丢弃；缺 `KeyframesTable` 进 pkg + `ResolvedStyle.animation` 字段 + bridge 序列化 + tween 发射 + pkg bump 20→21 + dll 重编。
  - **`transition` 空壳**：`CssValueParser::Transition` 枚举变体已定义但零校验逻辑。fence 接受任意 `transition` 值不报错，但实际不生效。
  - **无 `animation` 长划子属性**：仅 `animation` 简写存在，缺少标准 CSS 的 8 个长划属性（`animation-name`/`animation-duration`/`animation-delay`/`animation-iteration-count`/`animation-direction`/`animation-fill-mode`/`animation-play-state`/`animation-timing-function`）。长划是简写的语法糖基础，应先补长划再补简写展开。
  - **`animation-delay` 处理粗糙**：简写解析器将最后一个数值 token 当 delay，非标准 CSS delay 语法。
  - **缓动仅 7 种**：`linear`/`ease`/`ease-in`/`ease-out`/`ease-in-out`/`step-start`/`step-end`。无 `cubic-bezier()`、无弹簧/弹性物理缓动。
  - **keyframes 内不支持 per-stop 缓动**：标准 CSS 每个 stop 可带 `animation-timing-function`，当前不支持。
  - **无 `@loom-hook`**：public-api.md §9.3 描述的 `/* @loom-hook name */` 注释锚点，fence 未解析。
- ✅ **showcase 围栏违规 — RESOLVED**（showcase-package-unblock 2026-07-21）：原 blocker（home `:nth-child` + form/settings 逗号/属性 selector + `resize` 围栏外）已由 fence 扩围（逗号 list / 属性 selector / resize noop）+ showcase nth-child/aria-selected defer 注释解决，`cargo run -p loomgui_pkg -- build showcase` exit 0、8 组件 showcase.pkg.bin 产出。剩余 nth-child / aria-selected / keyframes runtime 见下专门条目。
- **`:nth-child(N)` selector + `animation-delay` 错峰**（§4 视觉束）：fence 选择器子集本轮不收 `:nth-child(N)`，相关错峰规则 defer 到视觉束与 keyframes runtime 一同落地（pkg v20→v21 reserved）。showcase `home.html` 7 条 `.nav-card:nth-child(N){animation-delay:...}` 已注释（见 home.html TODO）。
- **`[aria-selected]` state-attr selector**（§4 控件束 TabList）：fence 属性选择器本轮只匹配 `[type=x]`，state-attr 匹配（`[aria-selected="true"]` 等）随 Tab 控件（role dispatch + WAI-ARIA 复合控件）落地。showcase `settings.html` `.tab[aria-selected="true"]` CSS 规则已注释（见 settings.html TODO；HTML `aria-selected` 属性本身 fence 解析正常，仅 CSS 选择器 deferred）。
- **`Scene::build` data_controller dead 参数**（R2 待办段，签名重构后清）：P1 妥协保留 `Scene::build` 入参里的 data_controller 位，Controller 全链已删但签名未重构。
- **projection-layer §3 items 2/4 set_style 残留**（R2 待办段）：projection-layer 文档 §3 items 2/4 描述的 set_style 残留，R2 投影层升级攒批时清。
- **add_class null check gap**（Minor，P2.5）：FFI `add_class` 对 null class 指针的 null-check 守卫缺失（与 P2.5 deferred ② `create_node`/`set_text` null css 同源模式），低风险（业务 caller 不传 null），核心 null-check 修法套用。
- **GUI exe 拷贝滞后**（编码机工作流）：GUI exe 重出后编码机忙没拷 `unity/package/Editor/Tools/loomgui_gui.exe`，编码机关 GUI 后拷。不影响 runtime，影响打包器版本（pkg bump 时触发，坑 158 同源 stale exe 链）。
- **loom.runtime.json stomping**（P3.2 concern 1，多 workspace 共享 output_dir）：多 workspace 共享同一 output_dir 时 packer 重写 `loom.runtime.json` 互相覆盖。处置：每 workspace 独立 output_dir（`loom.workspace.json` 配），或 packer 加 namespace 隔离。
- ✅ **showcase src/key packer bug — RESOLVED**（showcase-package-unblock Task 8）：`normalize_sprite_key`（crates/packer/pkg/src/build.rs）把 HTML 相对 img src 归一为 workspace-root 相对 sprite_key；showcase img src + spec4b img-src 深度修对后 referenced_sprites ↔ atlas 校验过。

### main-design.md 校验发现的 deferred 项（2026-07-20 review）

> 以下各项 = 文档描述终态、代码尚未实现。全部归入 tech-debt，按束分配处置路标。

- **Block 布局策略**（§4 视觉束 / 复合束文本模型）：`display:block` 当前强制映射为 `taffy::Display::Flex`（mapping.rs:672-675），仅旁路 `DisplayMode::Block` 标记未消费。taffy 0.5 虽有 block layout 但 LoomGUI 刻意不用。终态需实现标准 block 布局（垂直堆叠、margin collapse），触发时机与复合束文本模型（p/h1-h6 建文本 block）同频。
- **grayed 灰化渲染**（§4 视觉束）：`RenderNode` 缺少 `grayed: bool` 字段。文档描述禁用节点灰化渲染，全仓搜索零匹配。待视觉束补字段 + 渲染管线（shader / color tint 路径）。
- **NodeTransform 替代 Affine2**（§4 控件束，第一个高频控件触发）：`RenderNode.world_matrix` 当前为 `Affine2`（[f32;6] 裸仿射矩阵），文档终态为 `NodeTransform`（分解 Position/Scale/Rotation，对齐 public-api.md 三分模型）。升级与 set_transform 数值 FFI 同频。
- **动画系统终态**（§4 视觉束 v1.10）：当前 `TweenManager` 为单个 `Vec<Tween>`、flat `tween()` API、10 种缓动（Quad/Cubic/Back）、value_size max=4。文档描述终态为池化 `{active,pool}` + 链式 builder API + 28+ 缓动（含 Sine/Elastic/Bounce/Custom）+ value_size(1..6) + prop_type 分层（transform_dirty vs layout_dirty）。与 keyframes runtime 驱动一同落地。
- **控件 C# 投影类缺失**（控件束/复合束）：`OptionItem`、`LineBreak`、`Slot`、`CustomElement` 在 Rust `NodeKind` 中已有变体，但 C# 投影层无对应 public class，`NodeFactory` fallback 到 `Container`。
- **IsScopeRoot 作用域查找**（复合束）：`Get<T>("id")` 当前仅 `IsInSubtree` 简单祖先检查（Nodes.cs:190-194 显式标注 gap），完整 IsScopeRoot 边界（不穿透嵌套组件/List item）未实现。
- **Per-scope ID 去重**（复合束）：打包期 ID 唯一性校验当前为全 tree 去重（structural.rs），文档描述 per-template-scope 语义未实现。
- **Shadow DOM 样式隔离**（复合束）：Rust cascade 引擎零 scope 隔离代码。模板内部选择器作用域边界、父组件选择器不穿透——全部未实现。
- **CSS 自定义属性 `--*`**（控件束/复合束）：Rust core 零 custom property / `var()` 代码。C# `SetVar`/`RemoveVar` 为 `NotImplementedException` 壳。
- **控件 API 实现**（控件束）：`Slider`/`Toggle`/`TextField`/`NumberField`/`TextArea`/`Dropdown`/`ProgressBar` 全部属性/事件（除 `Button.Clicked`/`Link.Activated`）为 `NotImplementedException` 壳。公共签名已冻结，实现待控件束推进。
- **`UIStyleException` 缺失**（Minor，控件束）：public-api.md 声明 4 种异常类型（§1.4 / §12），`Types.cs` 仅定义 `UIContractException` + `UIPackageException`；`UIStyleException` 未定义。在控件束实现 Style 相关功能时补上此类。

**本轮之后、与范式重写解耦的**（旧 §3 功能线，保留对照）：

| 功能 | 旧编号 | 归属 |
|---|---|---|
| 动画 runtime 驱动（@keyframes 表 + tween 发射/ease/iteration） | v1.10 | 三束后（fence DSL 已提前进 P3.4c，跟 public-api 终态；只欠 runtime） |
| 离屏 RT 基础设施 | v1.14 | 渲染层工作，与 API 范式无关 |
| 高级滤镜 + BlendMode（12 种） | v1.15 | 视觉束延伸 |
| 几何扩展 | v1.16 | 三束后 |
| 移动 + IL2CPP + WebGL | v1.17 | 平台移植，排最后 |
| 编辑器/工具链闭环 | v other | 独立于 runtime，并行 |

---

## 5. v1 已交付（旧范式，可复用算法资产）

> 以下能力的**算法实现**是可复用资产（渲染批合、文本测量/光栅、滚动物理、字体图集、FFI SOA）。它们的**公共接口形状**在本轮被重写；算法主体与"节点怎么表示"解耦，迁移时不动。

- **渲染**：贴图 quad + 纯文本 + 硬矩形裁剪；FairyBatching 重排 + 显式 mesh 合并（真 N→1 draw call）；Unity GameObject 镜像 + DrawState 缓存。
- **文本**：核心自绘字体（ttf-parser outline + ab_glyph 光栅 + etagere 图集）；kerning；可合批；跨引擎一致；CJK 断行 + fallback 栈。
- **事件**：命中（等效绘制顺序逆序）+ click/hover/leave + 拖拽；多触摸（5 槽）+ CaptureTouch + 拖拽/滚动仲裁 + 键盘/焦点/Tab。
- **布局**：taffy 0.5 flex + block；参考分辨率缩放；safe-area。
- **滚动**：ScrollPane 惯性 + 回弹 + 滚动条 + 鼠标滚轮（自维护可变 target tween）。
- **资源**：pkg.bin 格式 v19（写包/读包/实例化全链）；独立工作区 + Tauri GUI 打包器；Rust 自绘图集。
- **FFI**：csbindgen + SOA 多 arena 渲染树同步（blob v11，21 列，含 SDF 文字特效）。
- **状态/样式**：`:hover/:active/:disabled/:focus` 动态规则；apply_decl 属性级覆盖相当完整（flex/盒模型/背景/边框/字体/transform/filter/box-shadow/transition/font-effect）。

### v1.x 版本历史与本轮去向

| 版本 | 内容 | 本轮去向 |
|---|---|---|
| v1.1 background-image / v1.2 border-radius / v1.3 ColorFilter+九宫格+动态树 | ✅ | 算法保留，视觉束换表达 |
| v1.4 虚拟化列表 + position:absolute | ✅ 旧范式 | 复合束 ListView 吸收 |
| v1.5 Controller（data-controller/data-page） | 🛑 停止 | WAI-ARIA 替代（控件束） |
| v1.6 核心自绘字体 | ✅ | 算法保留 |
| v1.7 富文本（display:block desugar） | 🛑 停止 | 复合束文本模型替代 |
| v1.8 文字效果 + 装饰视觉 | ✅ | 算法保留，视觉束换表达 |

### 旧范式退役清单（本轮清除）

- `<div>` 永远 flex column → 标准 block/flex（taffy 已支持，走 display）
- `display:block` RichText desugar 暗号 → 正常 HTML 子树
- 四标签围栏（div/span/img/button）→ 标准 HTML 子集（fence 23 标签已完成）
- `NodeKind` 5 变体 enum + `uint NodeId` 命令式句柄 → 档位2 语义 enum + C# 类型化投影
- `data-controller/data-page` 私有状态协议 → 标准 WAI-ARIA Pattern
- `FindNodeById` 全局首匹配 → 组件作用域 `Get<T>("id")`
- driver 手写虚拟列表 → ListView 内置虚拟化

---

## 6. 对标基线与借鉴

- **对标 FairyGUI**：10 年沉淀、跨引擎、可视化编辑器。LoomGUI 精神继承 + 布局替换（flexbox 代 Relations）+ 类型化对象树（标准 HTML 元素决定类型）。
- **LoomGUI 差异化**：标准 HTML/CSS（AI 强先验，fgui `.fui` 二进制 AI 不能编辑）+ flexbox + Rust 跨引擎共享核心 + 围栏验证器 + 类型化对象树 API（HTML 语义 → 稳定类型，CSS 赋予行为不改变类型）。
- **实现任何机制前先对照** `temp/FairyGUI-unity/` 和 `temp/RmlUi/`（只读）：渲染/对象模型/批合/事件/动画/资源借鉴 fgui，文本/布局借鉴 RmlUi/UITK。
- **可 port 的纯算法**：字体核心自绘（已做 v1.6）、滚动手感数学、transition 中断平滑。
- **别抄**：回弹/批合/filter/opacity（LoomGUI 已领先）；RmlUi 全 atlas 重建；Euler 滚动模型/分布式动画时钟（破不变量）；fgui Relations/Gears/GTree/BMFont。
- 不能换 RmlUi 底层——核心三件套与 RmlUi retained 全量重画正面冲突。

---

## 7. 机制草稿（实现期钉，非契约）

> 收留从主设计搬出的机制草稿——实现期才该定的细节。字段/算法实现时按真实约束调。

- **Shape mask + 两遍 DFS**：RenderNode payload 加 `Mask{shape_ref, mode}`，核心 DFS 算嵌套深度填 MaskContext，后端自选实现（Unity stencil / Godot canvas_group / 软件 alpha mask）。
- **NativeHost**：框架只提供 FFI 查询（world_matrix/sort_key/visible）+ 材质配置；anchor/位置/scale 在 driver（Unity 侧已有 NativeHostManager）。
- **Transition**：纯数据 `items: Vec<TransitionItem>`，`Play()` 翻译成 Tweener 提交 TweenManager，由控件状态变化触发。
- **包格式演进**：集中式迁移器链；`nextPos` 长度前缀 forward-compat；branches（多语言）/highResolution（1x/2x/3x）；scaleLevel。
- **契约版本化**：公共头 `contract_version:u32` + `feature_flags:u64` + 可选扩展列。SemVer：加可选=minor，改必选=major。
- **世界空间 UI**：NodeTransform 加 `Option<VertexMatrix>`。
- **SRP 混合渲染**（Unity 增强）：自绘节点用 SRP RendererFeature 批合。

---

## 8. 关键决策记录

- **推翻横切 R2–R8，改摸黑打通 + 三束加宽（2026-07-15）**：真实断点只在 IrTree↔TemplateNode 一处，其余全活。横切让每层成"啃不动的大山"，摸黑一次打通骨架链（终点线1 headless + 终点线2 Unity）比反复切片省，且 TDD 兜得住核心重构。
- **红队修订：spike 先行 + 三个假设证伪（2026-07-15）**：代码核实推翻了"cascade 只需复用 rematch 扩一下"——① CSS 选择器**解析器缺席**（只有匹配器）；② 继承**只支持 color 且是脆弱 hack**；③ 打包器 HTML 编排**整段被删**。故顺序改为"先 P0（弃旧 pkg + 一刀切升 v17）→ 阶段 S（cascade+bridge 在旧 enum 上探路）→ ①enum 扩容 → ③cascade 收尾 → ②桥+打包编排"。真 smoke 在 ② 后（① 阶段 HTML 进不来，只能测表示层，是"假绿"）。SemanticKind→NodeKind 映射必须 total+非静默（实现证成 1:1，semantic_hint 无需）。④ 加 headless C# harness 破两台机串行。
- **旧 showcase.pkg.bin ≠ 新 showcase（2026-07-15 用户更正）**：入库旧包是旧范式旧内容，新 showcase 8 页是靶子期重写、从未打包。个人项目不考虑兼容，旧包直接弃、不做基线快照、v17 不留后向兼容。原红队 F3"冻结 pkg 不可再生死锁"基于误判（把两者当同一个），已作废。
- **core 类型化走档位2（2026-07-15）**：扩 enum + side table + bitflags，类型化用户表面留 C# 投影层，Rust 不上 trait object（封闭类型集 + 热循环数据局部性 + projection-layer 推论）。
- **cascade 归 core、fence 只解析（2026-07-15）**：规则表必须进包（逻辑层运行时大量用 CSS，bake 丢规则会破"CSS 优先"赌注）；单一真相源，复用已有 rematch 扩全量。
- **护城河判据 = 布局可预测不是滤镜像素（2026-07-15）**：放弃像素级 diff 和 PNG 软件渲染器（YAGNI），用 headless rect 比对 + Unity 视觉验收。
- **攒批/set_transform 推迟到第一个高频控件（2026-07-15）**：摸黑期即时过桥兜底，标 ponytail 欠债。
- **设计自上而下、实现按合理路径**：公共 API 优先（已冻结 Public/*.cs + public-api.md），实现从内向外（先 Rust 骨架，后端对象层随后）。
- **平台移植排最后**；**编辑器工具链并行**（独立于 runtime）。
- **Spec-1 阶段 S spike 完成（2026-07-16）**：三个致命假设全部证成，`div/span` HTML → `<style>` cascade → rect 端到端打通（4 验收断言绿，commit `1c21b4d..9acd01e` 已 merge main）。关键定论：① 选择器解析器走**路径 c 手搓、零新依赖**（cssparser/scraper 全仓未引；cssparser 是分词器不给 selector AST——初版"接 cssparser"前提被推翻）；selector/rule **类型留 core**（`style/dynamic.rs`），fence 直产 core 类型（无适配层/共享 crate）。② cascade 引擎 `rematch_pseudo_classes` **本就完整**（非"复用 rematch 扩一下"——它遍历全规则 + specificity 合并 + base_style 每帧重起），spike 只产规则表喂它。③ 通用继承 pass 替代 color-only hack，set-ness 用 transient bitmask **不进 `ResolvedStyle`**（避免升 pkg 版本）。真实断点经核为**三处**（IrTree↔TemplateNode 桥 / packer HTML 编排被删 / `<style>` 选择器解析器缺席），非旧述"一处"。**⚠️ Spec-2 前置**：set-ness 须打包期 bake 进 base_style（spike 只追 dynamic cascade，打包期声明会被父运行时值覆盖；详见 §2 ① + 阶段 S ⚠️）。anim-text-color 跨节点 override 随通用 pass 丢弃，标 ponytail 推 Spec-3。
- **Spec-2 ① core 类型化重构完成（2026-07-16）**：NodeKind 5 payload 变体 → 22 unit 变体（derives Copy + 谓词方法 is_container()/is_leaf()/has_children()）；Node 27 字段巨 struct 拆分（hovered/active/focused/disabled/cascaded_once → NodeFlags bitflags，	ouchable/draggable/tabindex → NodeInteraction 子 struct）；leaf 数据（text content / image src）从 enum payload 迁移到 Scene.text_contents / Scene.image_srcs HashMap side table，跨 pkg.bin 持久化靠 TemplateNode.content / TemplateNode.src 新字段（spike 挖出的数据丢失硬伤修法）；ResolvedStyle.inherited_set 打包期 bake（spike 前置完成）；pkg 格式 v16→v17。全 workspace 675+ 测试绿。关键定论：① RichText 变体退役（layout/render 旁路删除，	ext/rich.rs 算法保留待复合束）；② NodeKind 全 unit 后 bincode serialize = 4B（FixintEncoding u32 判别值）；③ Scene::build entry tuple 8→10（末两位 content/src）。commit `c77967c..221c558`，branch `codex-spec-2-core-refactor`。
- **Spec-3 ② IrTree 桥 + 打包编排完成（2026-07-16）**：骨架链断点接上——HTML→fence→桥→pkg.bin→core 建树→layout→render 端到端打通（终点线1 smoke 绿，rect width=200 + display:none 剪枝 + class 命中）。关键定论：① pkg v17→**v18**，`kind_tag` 改为 **`NodeKind` 判别值**（`#[repr(u8)]` + `from_u8`，全 23 变体，**kind_tag≠独立 tag 表**是 v18 与 v17 的本质差），删 RichText 死字段；② IrTree→TemplateNode **桥放 packer**（fence 产 ParsedTemplate，packer 调 `bridge()` 翻译，core 不认 fence 类型——保持 core 纯运行时）；SemanticKind→NodeKind **1:1 total + 非**静默（22 可映射语义各有专属 NodeKind，无 collapse；InputDispatch/Template/None → Err，§3.3 判据兑现，semantic_hint 无需）；③ **坑 161 修法 = inline inherited_set bake**（Spec-2 只 bake 了 dynamic cascade 的 set-bit，inline style 声明的继承属性仍漏——本棒补 inline 源，双源统一走打包期 bake）；④ packer HTML→pkg.bin 编排重建（`pack_components` 闭环）+ `referenced_sprites` 回接 atlas 校验；⑤ fence 顶层空白文本根跳过（真实 bug，smoke 暴露非假绿）。**执行顺序修订**：原红队定 ①→③→②，实际走 ①→②（③ cascade 收尾推后到 ② 链通后，让 smoke 先验范式）。全 workspace ~790 测试绿，fmt/clippy/feature-gate 清。commit `aee3716..349ccb2`（含 .dll 重编），branch `spec3-ir-bridge`。
- **Spec-3 ③ cascade 收尾 + 查询出口完成（2026-07-17）**：③ 重心从「重写 cascade 引擎」翻成「补可观测性 + 锁集成断言」——引擎已在子集内跑通全量标签（spike + ② 已验），真缺口是 Stage 无 computed-style / node-kind 查询出口，无法断言 cascade 正确性。补 `Stage::get_node_kind` + `get_node_computed_style`（core public）+ `ComputedNodeStyle` typed 快照（从 ResolvedStyle 投影 curated 子集）+ FFI 导出（return-code + out-param）+ probe 集成断言（继承/specificity/class/kind 保真全绿）。关键定论：① **computed-style 出口选型 = 快照子集 struct（候选 C）**——三方调研（RmlUi ComputedValues / Unity UITK resolvedStyle / FairyGUI TextFormat）一致支持；全量 struct 导出（A）泄漏 internal，按属性字符串查（B）丢类型安全且静态语言反模式；LoomGUI（Rust 静态 + 围栏闭合 CSS 子集 + FFI 值类型）落在 C 场景。② **base_style 每帧重起已确认**（`dynamic.rs:359-360`），set-ness 双源（坑 161 修法）已落地，无需重构 rematch。③ FFI **return-code + out-param**（避 NodeKind Container=0 哨兵撞——spec review 抓的真 bug）；无 repr enum（TextAlign）/外部类型（FlexDirection）走 match 稳定化。④ kind 保真由 get_node_kind 直接断言（§3.3 防「假绿」兑现）。⑤ C# 落点 = 并列 NodeComputedStyle 只读 struct（public-api.md 三分模型「computed 走只读快照层」，④ 实现）。deferred Minor 给 ④：ffi 直接依赖 taffy（④ 改 core re-export FlexDirection）、sentinel `*out` 不写断言、POD size lock（④ `Marshal.SizeOf==sizeof`）、probe float tolerance。全 workspace ~805 测试绿，fmt/clippy 清。commit `22e922d..a010508`，merge `e05fe96`，branch `spec3-cascade-finalization`。
- **Spec-4a 实现 complete（2026-07-18）**（spec/plan 2026-07-17）：④ 后端对象层拆 2 棒（4a 本机 headless / 4b Unity 验收）。4a = C# 投影层 + core 便签层 + typed 事件层 + headless harness。关键查证修正文档假设：① **core 无 inline override 层**（`set_style` 写 `base_style`，`scene/dynamic.rs:260`）——projection §2.2「复用现有 FFI 零改动」+ public-api §3.1「Style 是 inline override 层」假设落空，4a 装便签层还债（Node 加 `inline_override`+`inline_set`）。② 便签层**折进 rematch 单 set_map**，`propagate_inherited` **零改**（inline 继承属性经 set_map 自动传播，`style/dynamic.rs:329-456` 核实——review 挖出、已读源码验证的简化，消掉原估「扩 propagate」真复杂点）。③ **即时过桥版=最终版子集**：StyleMirror 稀疏镜像 + FlushInline seam（getter 契约强制镜像存在），升级攒批只改 setter 调用时机，不推翻。④ FrameBlob 21 列无 rect → Geometry 4a 直读 FFI（blob 缓存推后，YAGNI）。⑤ **4a 比 4b 重**（还 inline override 债，非填壳）。defer：set_transform / 攒批 flush / 控件业务事件 / @keyframes（spec §5）。**实现 outcome（2026-07-18，commit `354ab7d..9b04dd6`，36 commits）**：core 便签层（inline_override 折进 rematch 单 set_map, propagate 零改）+ Rust FFI 缺口 + parse_color 8-hex + C# 投影壳（NodeRegistry/NodeFactory 全 23 变体 dispatch/lazy children/StyleMirror 稀疏镜像+FlushInline seam/Geometry 直读 FFI/Transform 标脏 defer/ClassList/Container 读写/Get-Query 作用域/生命周期）+ typed 事件（RouteEventCore **sealed class**——D2 修订：struct 按 `Action<T>` 按值传致 StopPropagation 突变丢失，class 共享堆引用修复 + 18 event struct + EventBus On&lt;T&gt; capture/bubble/once + demux 接线 + Clicked/Activated 语义糖 + 业务字段从 raw 填）+ headless harness（P/Invoke 真 dll 不碰 Unity）+ fixture pkg.bin + E3 验收门（spec §4 全 9 条真断言绿，Criterion 5 假绿修正）。**300 HeadlessTests + PublicApi 编译门 + 626 core + dll md5 synced**。final review Critical none（sole blocker cargo fmt 已修）。**deferred（track for 4b/core-fix）**：core bug `dynamic.rs:231` remove_child 非直系子误清 parent；`create_node`/`set_text` null css → `from_raw_parts(null,0)` UB（core FFI null-check 待修）；D3 3 source-less event structs（ScrollChanged/AnimationStart/Iteration）；`kind_from_tag` 4-tag vs `resolve_semantic` 23-tag（dynamic span→TextNode 不匹配 Query）；LoomEventHandler 并行 backward-compat（AddListener→On&lt;T&gt; 迁移后移除）。spec/plan 见 `docs/superpowers/{specs,plans}/2026-07-17-spec4a-projection-layer*`，branch `spec4a-projection-layer`。
- **Spec-4b 实现 complete（2026-07-20，摸黑结束）**（spec/plan 2026-07-18）：④ 第 2 棒 = LoomStage 退役 + 多引擎后端分层 + 终点线2 Unity 真机验收。三 phase 落地：**P1 摸黑清理**（Controller 全链删 + pkg v18→v19 + set_style 死路径删 + rich_link_at 死链删 + dump_controller example 删 + LoomEventHandler 连锁删）+ **P2 多引擎分层**（新 `Runtime/Host/` 引擎无关层：LoomHost + LoomBackend 抽象契约 + UnityLoomBackend；LoomStage 589 行业务 API 透传层整层删；driver 改走 LoomHost + ctx.LoadPackage；deferred ①②顺手清）+ **P3 Unity 真机验收**（spec4b-acceptance 最小页 pkg v19 + PlayMode 真机 §5 四门）。**逻辑 4 门全绿** + 视觉 4/5。关键定论：① **LoomStage 退役是 clean break**——589 行业务 API 透传层（29 个零活体 caller）整层删，不留双壳（呼应终态契约 public-api §11.3 / main-design §2.2/§17 里没有 LoomStage）。② **多引擎接缝落点 = "驱动核心 + 后端契约"两层**——LoomHost 持 stage 驱动序（main-design §16 五步 CollectInput→tick→borrow_frame→SyncFrame→borrow_events typed 路由）零 `using UnityEngine`，LoomBackend 抽象契约（CollectInput / SyncFrame / 资源对象上传），UnityLoomBackend : LoomBackend（MirrorPool/MaterialManager/NativeHostManager/SpriteResolver/InputCollector 全部零改复用，只是从 LoomStage 搬过来）。③ **borrow_frame FFI 放 LoomHost**（产生引擎特定镜像对象的 FFI 归引擎无关驱动核心），**set_input FFI 在 backend**（采集引擎特定但 FFI 引擎中立，省一次交互）。④ **pkg v18→v19**：ControllerEntry/ControllerSection schema + TemplateNode.data_controller 字段 drop（Controller 全链删的 schema 一致性需要，bincode 布局变）；包格式真相源仍是 `crates/core/src/asset/mod.rs` `PKG_FORMAT_VERSION`。⑤ **fence `@keyframes`/`animation` DSL 已对齐 public-api §9 终态**（commit `e2e2812`：parser Animation + css_resolve Button 默认 justify/align center 修 Buy 字不居中），runtime 驱动（KeyframesTable + ResolvedStyle.animation + bridge 序列化 + tween 发射）留 §4 视觉束 v1.10——fence 已就绪无需再改，pkg.bin 格式不变（避免版本 bump），keyframes 规则当前在 ParsedTemplate.keyframes 暴露但 packer bridge 静默丢弃。⑥ **deferred ①②顺手修**：core `remove_child` 直系子校验（commit `56e7669`，加 C# GetChildIndex 守卫）+ FFI null-safe css/text/src/prop（commit `80552a0`）。deferred ③（kind_from_tag 4-tag vs resolve_semantic 23-tag）推后（spec4b 走 pkg.bin 实例化 resolve_semantic 23-tag 全，不触发 dynamic 创建）。**tech-debt**（card-img Image bg 机制 + keyframes runtime 驱动 + showcase 围栏违规 + Scene::build data_controller dead 参数 + projection-layer §3 items 2/4 set_style 残留 + add_class null check gap + GUI exe 拷 + loom.runtime.json stomping + showcase src/key packer bug，详见 §4 tech-debt 段）。**两台机约束兑现**：核心范式对错在 4a headless 已锁，4b 验的是集成层没歪——逻辑 4 门全绿坐实。spec/plan 见 `docs/superpowers/{specs,plans}/2026-07-18-spec4b-*`，branch `spec4b`。**摸黑结束**，下一阶段 §4 三束加宽。
