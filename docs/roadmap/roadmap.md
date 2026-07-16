# LoomGUI 路线图

> v1 架构验证完成（v1a–v1e + showcase + v1.1–v1.8），桌面 Mono 可演示。
> 当前进入 **API 范式重构**：从"stage 全局 + NodeId 句柄 + 命令式 SetX"重做为"类型化对象树 + 标准 HTML 围栏 + CSS 优先"。
>
> **本轮路线的组织方式（2026-07-15 重定）**：不再横切 R2–R8 大层，而是**先摸黑打通一条最小骨架链、再沿骨架加宽**。原因见 §1。
>
> **进度（2026-07-16）**：Spec-1（阶段 S spike）✅ 完成——`div/span` HTML → `<style>` cascade → rect 端到端打通，4 条验收断言绿，三个致命假设证成。**下一棒 = Spec-2（① core 类型化重构）**。详见 §2「阶段 S」状态、§2「①」前置、§8。
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

1. **dll/绑定/源码同步**：入库 `.dll` + csbindgen 绑定比 Rust 源码**过期**（`loomgui_stage_load_html` / `loomgui_stage_set_rich_text` 已删源码、dll 里还在，旧 showcase 仍调）。按当前源码重编前先解决三者同步，否则任何重编都踩雷。
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

**① core 类型化重构（档位2，见 §3.1）** — 最机械、低风险，放 S 之后（**= Spec-2，下一棒**）
- **Spec-2 必做前置（spike 挖出）**：拆 `ResolvedStyle` + 升 v17 时，把"属性是否显式声明"的 **set-ness 一起 bake 进 base_style**。spike 的 set-ness 只追踪 dynamic cascade，打包期声明拿不到 bit → 生产环境继承会被父运行时值覆盖（详见阶段 S ⚠️ + §8）。这是 ③ cascade 收尾能正确处理打包期声明的前提，趁拆 struct + 升版本一次做掉。
- `NodeKind` enum 从 5 变体扩容到承载围栏语义（div/text/image/button + 各控件语义），`match` 分发。**81 处 `NodeKind::` match（14 文件，重灾 scene/dynamic.rs 32、layout/mod.rs 17）编译器会牵着逐处过。**
- 控件私有状态（slider value/min/max、dropdown selectedIndex、textfield value/光标）走 **side table**（按 NodeId 索引的稀疏表），续用旧代码 anim/scroll 已在用的 `HashMap<NodeId,_>`/并行数组模式——**不塞进统一 struct**。
- 共性运行时 flag（hovered/active/disabled/focused）收进 bitflags；27 字段巨 struct 按关注点（结构/几何/运行时状态）拆分。
- 类型化的**用户可见表面留给 C# 投影层**，Rust 侧只需语义 enum + match，**不上 trait object**。
- FFI 的 `node_id:u32` ABI 约定**不砸**（enum 扩变体不影响 ABI）。

**③ cascade 收尾（把 S 的 spike 产品化，见 §3.2）**
- 把 S1 解析器 + S2 继承接到扩容后的 enum 上，覆盖全量围栏标签/控件。规则表写进包（配合 P0 的 v17）。
- core 当**唯一 cascade 引擎**：节点 class 集 + 伪类态 对规则表匹配 → specificity 排序合并 → 继承。伪类复用已有 `rematch_pseudo_classes`（这部分**确实**是复用）。
- `base_style` **不是可选缓存，是 rematch 的每帧基线**（`rematch` 每帧从 base_style 重启，见 §3.2 修订）——"摸黑期不做 base_style"的说法作废，必须填 base_style 或重构 rematch 契约。
- 选择器摸黑 MVP 只支持子集：class / tag / id / 后代组合 / 伪类。`:nth-child`、`@keyframes`/`animation` 是否进 scope 见 §3.5（关系到终点线2 能否跑 home 动画）。打包期验证"只用了支持的选择器"。
- **cascade 单独上集成级测试**（整段 HTML+CSS 断言 computed style）——历史翻车点，per-property 单测兜不住。

**② Ir→TemplateNode 桥 + 打包编排（比"一个函数"大）**
- 桥：fence 产物翻译成新 core 形状 `Vec<TemplateNode>`（DFS、全局 IrNodeId→局部 parent_idx、抽 class/id/tabindex、img src 归一化）。
- **打包编排（被删的部分，不止桥）**：packer 的 `invoke fence → 建 ComponentTemplate → 从 cascade 填 dynamic_rules → write_package → 回填 packages` 整条编排层在 `d8fe705` 被删，要重建，不只写个翻译函数。加 `loomgui_fence` 依赖。
- **SemanticKind（25 种）→ 新 NodeKind 映射必须 total 且非静默**（§3.3）：无专用 NodeKind 的语义映射到 Container/Button **+ 保留 `semantic_hint` 字段**，加测试断言"无 showcase 元素静默丢失语义标签"。绝不 lossy-and-silent。

### 🏁 终点线 1（纯 Rust，headless，本机验）

**真 smoke 测试在 ② 之后**（① 阶段 HTML 还进不来，那时只能拿手搓 Rust 树测"表示层机制"，**证明不了范式**、也覆盖不了新控件——别把它当范式验收，是"假绿"最危险的一种）。

**② 后的端到端**：HTML 字符串 → 打包（内存 bytes）→ `load_package` → `instantiate` → `tick_and_render` → 断言。判据**不只 rect**（§3.3）：同时覆盖 cascade 继承传播、display:none 子树剪枝、class 命中 computed style、SemanticKind→NodeKind 无静默丢失。rect 对 ≠ 语义对。绿 = 核心范式活了。

### ④ 后端对象层（依赖终点线1）

- 填 `Public/LoomGUI.*.cs` 718 行签名壳（52 个 `NotImplementedException`）→ 每个方法**转发到成熟的旧 LoomStage 命令式 API**（`Style.Width = Px(100)` → `SetStyle(nodeId, "width:100px")`）。翻译层，非从零造功能。
- 加 **NodeId→Node 强引用缓存**（对象身份稳定，投影层 §2.4）+ **节点类型识别**（Instantiate 返回 NodeId 后要能造出正确子类；当前 FFI **无 node-kind 查询导出**，需新增 FFI 命令或从 dump_scene 绕）。
- typed 值转换：`Length`/`Color` ↔ CSS 字符串。
- **事件 typed 层（命名子任务，在关键路径上）**：旧 `EventHandler` 是 `EventType(byte)+nodeId` 面，没有 typed 分发。`node.On<ClickEvent>()` / `button.Clicked +=` 是**新 C# glue**（byte-EventType→typed-event demux + per-node 路由），不是"零改复用"。终点线2 要求 `Clicked` 触发 → 这块在关键路径，不是可选。
- 渲染/输入/**底层**事件路由管线（MirrorPool/InputCollector/borrow_events）**零改复用**（要不要跟改，④ 干活时再调研）。
- **headless C# harness（破两台机串行瓶颈）**：④ 的 ~52 个方法多数可在**编码机**上用 console test 直接驱动真 dll 的 `LoomStage` 验证，不必每次 commit-dll-push 去家里机跑 PlayMode。只把真正依赖渲染/输入的检查留给 Unity 机。
- **不做攒批回写 / set_transform**——即时过桥兜底（低频够用），标 `ponytail:` 欠债，留到"第一个高频改值控件"再上（§3.5 + §4 控件束）。⚠️ 见 §3.5：transform 债可能在终点线2 就爆。

### 🏁 终点线 2（Unity，家里机验收）

`UIContext → LoadPackage → Instantiate → Get<Button> → Clicked → 真机渲染`。新范式端到端能用 = 摸黑结束。

> **两台机约束**：终点线1 纯 Rust、本机跑 headless 测试即可，核心范式对不对在本机就锁死；终点线2 才需搬去家里机跑 Unity PlayMode。核心风险在本机验完再进 Unity，不会"搬过去才发现核心错了"。

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
2. **@keyframes 根本没进解析器 scope**：home.html 的入场动画是 CSS `@keyframes`+`animation`。而 §2 的选择器 scope 只列了 class/tag/id/后代/伪类，**没有 @keyframes/animation 解析**。所以 home 的动画**在终点线2 可能压根不跑**，与 transform 债无关。

**决策**：终点线2 的验收页要么**选一个不含 @keyframes/transform 动画的 showcase 页**当门（推荐，把动画留到 §4 视觉束/控件束），要么把 @keyframes/animation 解析 + set_transform 提前拉进 S/③/④ scope。不要让"transform 债留以后"和"home 当 demo"硬撞。

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

**本轮之后、与范式重写解耦的**（旧 §3 功能线，保留对照）：

| 功能 | 旧编号 | 归属 |
|---|---|---|
| 动画增强（@keyframes/ease/iteration） | v1.10 | 三束后 |
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
- **资源**：pkg.bin 格式 v16（写包/读包/实例化全链）；独立工作区 + Tauri GUI 打包器；Rust 自绘图集。
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
- **红队修订：spike 先行 + 三个假设证伪（2026-07-15）**：代码核实推翻了"cascade 只需复用 rematch 扩一下"——① CSS 选择器**解析器缺席**（只有匹配器）；② 继承**只支持 color 且是脆弱 hack**；③ 打包器 HTML 编排**整段被删**。故顺序改为"先 P0（弃旧 pkg + 一刀切升 v17）→ 阶段 S（cascade+bridge 在旧 enum 上探路）→ ①enum 扩容 → ③cascade 收尾 → ②桥+打包编排"。真 smoke 在 ② 后（① 阶段 HTML 进不来，只能测表示层，是"假绿"）。SemanticKind→NodeKind 映射必须 total+非静默（保留 semantic_hint）。④ 加 headless C# harness 破两台机串行。
- **旧 showcase.pkg.bin ≠ 新 showcase（2026-07-15 用户更正）**：入库旧包是旧范式旧内容，新 showcase 8 页是靶子期重写、从未打包。个人项目不考虑兼容，旧包直接弃、不做基线快照、v17 不留后向兼容。原红队 F3"冻结 pkg 不可再生死锁"基于误判（把两者当同一个），已作废。
- **core 类型化走档位2（2026-07-15）**：扩 enum + side table + bitflags，类型化用户表面留 C# 投影层，Rust 不上 trait object（封闭类型集 + 热循环数据局部性 + projection-layer 推论）。
- **cascade 归 core、fence 只解析（2026-07-15）**：规则表必须进包（逻辑层运行时大量用 CSS，bake 丢规则会破"CSS 优先"赌注）；单一真相源，复用已有 rematch 扩全量。
- **护城河判据 = 布局可预测不是滤镜像素（2026-07-15）**：放弃像素级 diff 和 PNG 软件渲染器（YAGNI），用 headless rect 比对 + Unity 视觉验收。
- **攒批/set_transform 推迟到第一个高频控件（2026-07-15）**：摸黑期即时过桥兜底，标 ponytail 欠债。
- **设计自上而下、实现按合理路径**：公共 API 优先（已冻结 Public/*.cs + public-api.md），实现从内向外（先 Rust 骨架，后端对象层随后）。
- **平台移植排最后**；**编辑器工具链并行**（独立于 runtime）。
- **Spec-1 阶段 S spike 完成（2026-07-16）**：三个致命假设全部证成，`div/span` HTML → `<style>` cascade → rect 端到端打通（4 验收断言绿，commit `1c21b4d..9acd01e` 已 merge main）。关键定论：① 选择器解析器走**路径 c 手搓、零新依赖**（cssparser/scraper 全仓未引；cssparser 是分词器不给 selector AST——初版"接 cssparser"前提被推翻）；selector/rule **类型留 core**（`style/dynamic.rs`），fence 直产 core 类型（无适配层/共享 crate）。② cascade 引擎 `rematch_pseudo_classes` **本就完整**（非"复用 rematch 扩一下"——它遍历全规则 + specificity 合并 + base_style 每帧重起），spike 只产规则表喂它。③ 通用继承 pass 替代 color-only hack，set-ness 用 transient bitmask **不进 `ResolvedStyle`**（避免升 pkg 版本）。真实断点经核为**三处**（IrTree↔TemplateNode 桥 / packer HTML 编排被删 / `<style>` 选择器解析器缺席），非旧述"一处"。**⚠️ Spec-2 前置**：set-ness 须打包期 bake 进 base_style（spike 只追 dynamic cascade，打包期声明会被父运行时值覆盖；详见 §2 ① + 阶段 S ⚠️）。anim-text-color 跨节点 override 随通用 pass 丢弃，标 ponytail 推 Spec-3。

