# Blitz 参考重构评估（LoomGUI 落后项对照 Dioxus Blitz）

> 2026-07-21。评估 Dioxus Blitz（`tmp/blitz`，pre-alpha HTML/CSS 渲染引擎）能否作 LoomGUI 底层、以及 LoomGUI 落后项可否直接参考 Blitz 重构。
>
> 本文是**评估候选清单**，非已定计划。目的是为"个人项目优先参考成熟项目、少造轮子"提供决策依据。所有代码行数、文件位置均为 2026-07-21 快照（LoomGUI 与 Blitz 双方都会演进），引用前请按当时源码核实——防文档漂移。

## 1. 背景

LoomGUI 是个人兴趣项目，单人开发。与其自己造轮子，落后项应直接参考成熟项目重构。本文以 Blitz 为主对照（已在 `tmp/blitz`），并按 CLAUDE.md 既定方法论交叉参考 FairyGUI（渲染/对象模型/批合/事件/动画/资源管线）、RmlUi（文本/布局/装饰）。

Blitz 定位：Dioxus 的 Rust 原生 HTML/CSS 渲染引擎，pre-alpha，~35k 行（blitz-dom）。用 Servo **Stylo**（完整浏览器级 CSS）+ **Taffy 0.12**（flex/grid/block/float）+ **Parley 0.10**（文本）+ **anyrender**→vello/skia/svg（自绘，自己开窗 winit/wgpu 渲染）。两个上层 wrapper：`blitz`（HTML/markdown）、`dioxus-native`（Dioxus app）。

## 2. 总体判断：能否直接用 Blitz 作底层？不能

决定性理由（任一已足够，叠加是必然否决）：

1. **跨引擎 FFI 是 LoomGUI 的 G1 立身之本**（同一份包在 Unity/Godot 布局/几何一致）。Blitz 是纯 Rust 单进程，winit 自己开窗 + wgpu/vello 渲染，**没有"把渲染交给别的引擎后端"的概念**。用 Blitz = 抛弃整个跨引擎后端模型。
2. **Stylo 与"围栏"哲学冲突**。LoomGUI 刻意手搓 CSS 子集（钉版本精简依赖、围栏外打包期报错不静默降级、AI 强先验）。换 Stylo = 引入整个 Servo CSS 引擎体量，依赖塞不进 Unity IL2CPP。
3. **无打包期边界**（`.pkg.bin` + 图集 + `runtime.json` 自举）。Blitz 运行时直接吃 HTML 字符串。
4. **节点模型不兼容**。Blitz 的 Node 是浏览器 DOM（`Slab<Node>` + `usize` 非代际 id、无类型分派、unsafe 裸指针回树、多套父子链）。LoomGUI 要的是类型化对象树（`NodeKind` enum + 代际 `NodeId` + C# 投影层）。
5. **成熟度**。Blitz 自标 pre-alpha。

**相似度**：表层技术栈（Rust + taffy + HTML/CSS + 自绘）~45%，核心架构范式 ~15%。Blitz = 精简浏览器引擎；LoomGUI = 游戏 UI 框架。结论：**Blitz 当算法库/参考实现读，有价值；当底层框架，不行**。

## 3. LoomGUI 各子系统现状（2026-07-21 实测）

| 子系统 | 规模（行） | 成熟度 | 落后点 |
|---|---|---|---|
| text | 2944（layout.rs 1751） | 中 | 换行只是简单宽度切分（非 Unicode UAX#14）；无复杂脚本 shaping/bidi/rtl；自研 SDF atlas |
| render | 8514（mesh 1508 / mod 1494 / batch 1024 / merge 484 / border 374 / dirty 238） | 高（核心，不动） | box-shadow 几何近似无 blur；border-radius 直角退化 |
| style | 3692（dynamic 1592 / mapping 1160 / resolved 619） | 高（手搓 cascade 成熟） | — |
| layout | **816（单文件 mod.rs）** | **薄** | flex-only 旧范式，block→flex 伪装 |
| scene | 2191（dynamic 1120 / node 618 / transform 444） | 成熟 | — |
| border（render/border.rs） | 374 | 中 | ✅ 四边不等宽已支持（`BorderWidths`）；❌ 无圆角（`radii` 被 `let _ =` 丢弃） |
| input / hit / scroll / tween | 1038 / 412 / 700 / 619 | 成熟 | — |

## 4. 候选参考重构清单（A–L）

### 直接抄档（独立、Blitz 成熟干净）

| # | 项 | LoomGUI 现状 | Blitz 参考源（2026-07-21 快照） | 怎么搬 | 成本 | 前置 | ROI |
|---|---|---|---|---|---|---|---|
| **A** | border-radius 圆角 | 直角退化（`crates/core/src/render/border.rs:42` `let _ = radii`） | `packages/blitz-paint/src/kurbo_css/css_box.rs:616`（`start_angle` 闭式解二次方程）+ `:46-75`（radius 自动缩放）+ `non_uniform_radii.rs` | 数学**原样照搬**，把 kurbo `BezPath` 换成自绘 mesh 顶点；在 `border_ring` 真正用起 `radii` | 中 | 无 | ⭐⭐⭐⭐⭐ |
| **G** | 多层 background 管线 | 无（单层） | `packages/blitz-paint/src/render/background.rs`（899 行，1 panic） | 多层 + background-clip/origin/size/repeat 照搬 | 中 | 无 | ⭐⭐⭐⭐ |
| **H** | 文本选区 selection | 无 | `packages/blitz-dom/src/selection.rs`（120 行，0 panic） | anchor/focus 模型 + 选区高亮几何，小而独立 | 中 | 无 | ⭐⭐⭐⭐ |
| **J** | CSS 字体单位 ex/ch/ic | 无 | `packages/blitz-dom/src/font_metrics.rs`（158 行） | 字体度量补齐 | 低 | 无 | ⭐⭐⭐ |

### 战略性档（前置或高成本，收益大）

| # | 项 | LoomGUI 现状 | Blitz 参考源 | 怎么搬 | 成本 | 前置 | ROI |
|---|---|---|---|---|---|---|---|
| **C** | taffy 0.5→0.12 升级 + 真 block 布局 | 816 行 flex-only，block→flex 伪装 | `packages/blitz-dom/src/layout/mod.rs:270-277`（真 block dispatch，无 `todo!`）；taffy 0.12 `block_layout` feature | 升 taffy（API 破坏性，先评估 migration）→ 启 `block_layout` feature → dispatch 切 block。**最大单点收益**：几乎零成本消除 flex 伪装 | 高 | 无，但是 D 的前置 | ⭐⭐⭐⭐⭐ |
| **B** | 文本 parley 化（复杂脚本 + UAX#14 换行） | 无 shaping/bidi/rtl；换行简单切分 | `packages/blitz-dom/src/stylo_to_parley.rs:291`（style→TextStyle 适配，~400 行范本）；parley 0.10 + skrifa + fontique | 依赖 parley 三件套；写适配函数；节点持 `Layout<UiBrush>`；绘制遍历 `lines()` 喂现有 SDF/atlas。**不需要 Stylo** | 高 | 无（动核心） | ⭐⭐⭐⭐ |

### 参考设计非照搬档

| # | 项 | LoomGUI 现状 | Blitz 参考源 | 备注 | ROI |
|---|---|---|---|---|---|
| **I** | 渐变增强（radial/conic） | 有解析（computed/mapping），绘制端程度待确认 | `packages/blitz-paint/src/gradient.rs`（520 行，4 unwrap） | 先确认 LoomGUI 绘制端支持到哪再补 | ⭐⭐⭐ |
| **K** | 增量 damage 设计模式 | `render/dirty.rs` 238 行（薄） | `packages/blitz-dom/src/layout/damage.rs`（698 行，`RestyleDamage` 位域 + style diff，⚠️ 9 panic/unwrap） | 参考设计模式，**非直接照搬**，panic 要改 | ⭐⭐⭐ |
| **L** | scroll fling 动量 + overlay 拖拽 | 有 scrollbar 基础（`scroll.rs` sentinel + thumb） | `packages/blitz-dom/src/events/pointer.rs`（PanState/FlingState/DragMode::ScrollbarDrag） | 触摸 fling 物理可借鉴 | ⭐⭐ |
| **E** | IME | `input.rs` 1038 行，IME 程度未知 | `packages/blitz-dom/src/node/text.rs:755`（`apply_ime_event`）+ parley `PlainEditor` | ⚠️ 修正：Blitz 的 IME 走 winit→parley；LoomGUI 运行时是 Unity 后端，IME 走 Unity 采集→FFI→core。可参考的是 parley `PlainEditor` 的 composition/commit 处理层，非 winit 管道 | 依赖 B | ⭐⭐⭐ |

### 慎抄/不抄档（Blitz 自身有瑕疵或与哲学冲突）

| # | 项 | 原因 |
|---|---|---|
| **D** | stylo_taffy 桥（fork `packages/stylo_taffy/` ~1250 行） | 与 LoomGUI 核心赌注冲突（手搓 CSS 子集 + 围栏 + AI 先验）。换 Stylo = 引入完整 Servo CSS 引擎，破坏围栏哲学，依赖塞不进 IL2CPP。**价值是"未来想要完整 CSS 时的现成桥"，非当前落后项补丁**。若执意要，fork 后优先修 13 个 `unreachable!()`（anchor positioning / grid fit-content 分支）。 |
| **F** | box-shadow 真 blur | **Blitz 自己也没解决**——blitz-paint 也是 4 角 `average` 近似，真模糊委托 anyrender 后端（vello/skia）。Blitz 非参考源，需另寻 vello_cpu 高斯模糊算法或自研离屏 RT（Unity 后端要配合）。 |
| — | clip-path / mask | Blitz `clip_path.rs:237-238` 有 2 个 `todo!()` 运行时崩溃（FarthestCorner/ClosestCorner）；且自绘 mesh 管线做任意 clip 成本高。 |
| — | AccessKit 无障碍 | 游戏 UI 现阶段非目标。 |
| — | form.rs（463 行） | LoomGUI `input.rs` 已 1038 行，非空白，不必抄。 |

## 5. 分档总览

| 档 | 项 | 启动建议 |
|---|---|---|
| 🟢 直接抄 | A 圆角、G 多层 background、H 文本选区、J ex/ch/ic | 个人项目优先，风险低、独立 |
| 🟡 战略性 | C taffy 升级、B 文本 parley 化 | 有整块时间再做；C 是 D 的前置 |
| 🟠 参考设计 | I 渐变、K damage、L scroll、E IME | 学思路按自管线改 |
| 🔴 慎抄/不抄 | D stylo_taffy、F box-shadow、clip-path、accessibility、form | 别踩 |

## 6. 跨项目参考建议（不唯 Blitz）

CLAUDE.md 既定方法论：渲染/对象模型/批合/事件/动画/资源管线借鉴 **FairyGUI**，文本/布局借鉴 **RmlUi**。按子系统挑最对的参考源：

| 子系统 | 最佳参考源 | 理由 |
|---|---|---|
| CSS 装饰数学（border/gradient/background） | **Blitz** | `kurbo_css/css_box.rs` 闭式解数学，开源最佳 |
| 文本（复杂脚本/换行/富文本/IME） | **Blitz（Parley 生态）** | Parley 免费 shaping/bidi/换行，Blitz 适配层薄 |
| 布局（标准 block/grid） | **Blitz（证明 taffy 0.12 可用）/ RmlUi** | RmlUi 模型更接近"标准 CSS 子集" |
| 多层 background / 渐变 | **RmlUi / Blitz** | RmlUi 更对口（标准子集而非完整浏览器） |
| scroll 物理 / fling | **FairyGUI** | ScrollPane 物理业界标杆（memory 已记录自维护 tween） |
| 动画 / tween | **FairyGUI** | GTween 更全，对照 `tween.rs` 619 行 |
| 渲染批合 / 对象模型 | **FairyGUI / UGUI** | memory 已定 MirrorPool 参考 UGUI 哲学 |

## 7. 待决策与风险提示

- **优先级待定**：🟢 档四项（A/G/H/J）都独立小干净，可并行或任选先做。推荐 **A 圆角**（最高 ROI，直接搬现成数学）。
- **C（taffy 升级）是分水岭**：做了它，D（stylo_taffy）才有意义；但 D 与围栏哲学冲突，C 本身（真 block + grid）已值回票价，D 可缓。
- **B（文本 parley 化）要评估多语言需求**：若只中文/英文，当前文本够用，B 可缓；若要做阿拉伯/复杂脚本，B 是必经路。
- **F（box-shadow）无现成参考**：blitz 无解，需自研离屏 RT，Unity 后端要配合，工程量大，建议 defer。
- **漂移风险**：本文行数/行号均为 2026-07-21 快照，动手前按当时源码核实 Blitz 参考位置是否仍存在；LoomGUI 侧行数会随重构变。

---

附：评估过程的对话结论已存 memory（`blitz-evaluation-not-suitable-as-base`），本文为该结论的完整候选清单展开。
