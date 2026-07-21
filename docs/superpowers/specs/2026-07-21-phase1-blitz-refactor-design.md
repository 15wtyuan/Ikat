# 阶段1:骨架链视觉/布局升级(参考 Blitz)

> 2026-07-21。摸黑结束(Spec-4b DONE)后,骨架链(div + 文字 + 图 + flex + cascade)端到端通了,但**视觉特效纯色块占位、`display:block` 用 flex 伪装**。本 spec = 参考 Dioxus Blitz(`tmp/blitz`,只读)把骨架链升级到「标准 CSS 视觉装饰 + 真 block 布局」,集中清掉 roadmap tech-debt 里视觉/布局相关项。
>
> 这是**阶段1**(重构已有)。阶段2 = 三束加宽(控件束/复合束/视觉剩余),另立 spec。完整 Blitz 参考评估见 `2026-07-21-blitz-refactor-evaluation.md`;Blitz 不能作底层(已否决),本阶段所有改动在 LoomGUI 既有架构内,**不动跨引擎 FFI / 渲染批合 / 滚动物理 / 文本自绘等成熟资产**。
>
> 用户决策:个人项目优先参考成熟项目少造轮子;先重构已有,再开发剩余。taffy 升级调研已 de-risk(见 §5.1,改动中等且全机械)。

## 1. 背景

摸黑打通的骨架链做的是**布局正确性**,渐变/阴影/圆角/文字特效用纯色块占位(护城河 = 布局可预测,不是滤镜像素,roadmap §3.4)。摸黑结束后,roadmap tech-debt 里积了一批视觉/布局近似项,正好可参考 Blitz 的成熟实现一次性清理。

| roadmap 已有近似/缺口 | Blitz 参考(2026-07-21 快照) | 本 spec 项 |
|---|---|---|
| `display:block` 强制映射 `taffy::Display::Flex`(flex 伪装) | `blitz-dom/layout/mod.rs:270-277`(真 block dispatch)+ taffy 0.12 `block_layout` | C1+C2 |
| border-radius 直角退化(`border.rs` `radii` 被 `let _ =` 丢弃) | `blitz-paint/kurbo_css/css_box.rs:616`(`start_angle` 闭式解)+ `:46-75`(radius 自动缩放) | A |
| 多层 background 无(单层) | `blitz-paint/render/background.rs`(899 行) | G |
| 渐变部分支持 | `blitz-paint/gradient.rs`(520 行) | I |
| CSS 字体单位 ex/ch/ic 无 | `blitz-dom/font_metrics.rs`(158 行) | J |

## 2. 目标 & 范围

**主线**:骨架链从「纯色块占位 + flex 伪装 block」→「标准 CSS 视觉装饰 + 真 block 布局」。

**范围(6 项)**:

| 项 | 内容 | 性质 | blitz 参考 |
|---|---|---|---|
| **C1** | taffy 0.5→0.12 升级(机械替换 + pkg bump) | 地基,版本升级 | — |
| **C2** | 启用真 block(`Display::Flex`→`Display::Block`) | 架构决策 | `blitz-dom/layout/mod.rs:270` |
| **A** | border-radius 圆角(含四角不等) | 视觉加法 | `css_box.rs:616` |
| **G** | 多层 background 管线 | 视觉加法 | `background.rs` |
| **I** | 渐变增强(linear/radial/conic) | 视觉加法 | `gradient.rs` |
| **J** | CSS 字体单位 ex/ch/ic(可选,顺手) | CSS 单位补全 | `font_metrics.rs` |

**非目标**(明确划出去):

- **B 文本 parley 化** → 合并到 roadmap 复合束「文本模型回归标准子树」(inline 扁平化 + parley inline 排版天然耦合,分开做会返工)。SDF/atlas 光栅化层不动(parley 只替换 measure 层)。
- **F box-shadow 真 blur** → 阶段2 视觉束延伸。Blitz 自己也没解决(只 4 角 average 近似,真 blur 在 anyrender 后端),需自研离屏 RT + Unity 后端配合。
- **H 文本选区 / E IME** → 控件束 TextField(新增交互,非重构已有;依赖 B)。
- **K damage 增量 / L scroll fling** → defer。K 是性能优化(无性能瓶颈,YAGNI);L 是移动端增强(桌面不需要,排到 v1.17 平台移植)。
- **stylo_taffy / 底层替换** → 不做。与围栏手搓 CSS 哲学冲突,且破坏跨引擎 FFI 立身之本(见评估文档)。
- **不动成熟资产**:渲染批合(`render/mesh`+`batch`)、滚动物理(`scroll.rs`)、文本自绘光栅(`text/atlas`+`sdf`)、FFI ABI。

## 3. 执行顺序(地基先行)

`C1 → C2 → A/G/I → J`。

**理由**:先把布局地基(taffy + 真 block)打到终态,视觉加法在真 block 基线上调,避免 flex→block 切换后视觉返工。C1 是 C2 的前提(taffy 0.12 才有 production-ready 的 `block_layout`);C1+C2 一起做(用户决策:一次到位,不拆两阶段)。A/G/I/J 是独立视觉加法,在 block 基线上并行或顺序均可。

## 4. Spec 分解

阶段1 拆 2 个子 spec,各自走 spec → plan → 实现:

- **Spec-P1「taffy 升级 + 真 block」**(C1+C2):地基,先做。
- **Spec-P2「视觉装饰加法」**(A+G+I+J):在 Spec-P1 的真 block 基线上做。

本 doc 是阶段1 总设计,P1/P2 的详细实现计划由 writing-plans 分别产出。

## 5. 每项设计

### 5.1 C1 — taffy 0.5→0.12 升级

**现状**:3 个 Cargo.toml 钉 `taffy = "0.5"`(core 带 `serde`,ffi/fence 裸)。用 high-level API(`TaffyTree<MeasureContext>` + `new_leaf`/`new_with_children` + `compute_layout_with_measure`),`taffy::style::Style` 嵌进 `ResolvedStyle.taffy_style` 贯穿 core/FFI/fence + serde 进 pkg.bin。

**调研结论**(taffy 0.5/0.8/0.9/0.10/0.11/0.12 六版本源码对照 LoomGUI 用法):
- high-level tree API **100% 兼容**(`TaffyTree`/`compute_layout_with_measure`/`NodeId`/`Style::DEFAULT`/测量闭包签名一字未改)。
- `Style` 字段名 **100% 兼容**(0.5→0.12 字段名零变化,只新增字段不改名)。
- `Display`/`FlexDirection`/`FlexWrap`/`Position`/`Overflow` enum 不变。
- **要改 = 两类机械替换**(编译器 + bincode round-trip 单测兜底,漏改即红):
  1. `LengthPercentage`/`LengthPercentageAuto`/`Dimension` 变体构造器 enum→关联函数(0.8 引入,内部改 `CompactLength` tagged pointer):`::Length(x)`→`::length(x)`、`::Percent(p)`→`::percent(p)`、`::Auto`→`::auto()`。约 40-50 处,集中在 `style/mapping.rs` + `style/resolved.rs`(含测试)。
  2. `AlignItems`/`AlignContent`/`JustifyContent`/`AlignSelf` enum 变体→struct 关联常量(0.11 引入,加 `safety` 字段):`::FlexStart`→`::FLEX_START` 等大写。约 20 处,集中在 `style/mapping.rs:1138-1155`(`parse_justify`/`parse_align`)+ `fence/css_resolve.rs`。
  3. 2 处 match 改写:`style/resolved.rs` 的 `as_corners` 闭包 + `layout/mod.rs:52` 的 `fn lp`,从 match 变体改 `into_raw()`+`tag()`。
- **Cargo.toml** 3 处版本号 0.5→0.12。

**pkg.bin 格式必然 bump**(三重叠加):Style 字段 31→41 + `LengthPercentage` wire(enum tag→8-byte u64)+ `AlignItems` wire(index→string)。bincode 不自描述,0.5 pkg.bin 在 0.12 不可读。**所有工作区 pkg 重打 + dll + GUI exe 重编入库**(CLAUDE.md「改 parse-time 逻辑必重打 pkg」工作流)。`style/resolved.rs` 的 `resolved_style_bincode_roundtrip_preserves_all_fields` 单测自验字段完整性。

**改法**(plan 级别,本 spec 只定决策):Cargo 版本号 → `cargo build -p loomgui_core` 按编译错机械替换 → `cargo test` 自验 → pkg bump → 重打 + 重编闭环。

**验收**:bincode round-trip 单测绿 + 全 workspace `cargo test` 绿 + showcase 8 页打包 exit 0。

### 5.2 C2 — 启用真 block

**现状**:`style/mapping.rs:665-678` 的 `display` 分支把 `block` 和 `flex` 都映射 `taffy::Display::Flex`(伪 block)。taffy 0.5 刻意不用 `Display::Block`(roadmap §5「taffy 0.5 flex + block,LoomGUI 刻意不用 block」)。roadmap tech-debt 终态 = 标准 CSS block。

**改法**:`mapping.rs:665` 一行 `Display::Flex`→`Display::Block`(block 分支)。taffy 0.12 的 `block_layout` 默认 features 已含,**不需额外 feature**。0.12 block 语义(margin collapse / BFC / 标准块流)符合 CSS 标准,production-ready。

**不变量更新**(CLAUDE.md 旧范式条目):
- 旧:「`<div>` 永远是 flex 容器,`display:block` 映射 `taffy::Display::Flex`」
- 新:「`<div>` 默认 block(标准 CSS,垂直堆叠子元素),`display:flex` 显式切 flex」。对齐 main-design §3.1 标准语义(div/header/nav/p/ul/ol/li 默认 block;span/strong/em/label/button/a/img 默认 inline)。

**待 plan 细化**:各围栏元素默认 display 表(block/flex/inline)对齐 fence.md;确认 `fence/css_resolve.rs` 给各元素的默认 display 现状。

**验收**(C2 最大风险,见 §7):rect-diff(headless Chrome DOM rect vs LoomGUI rect)验 block 垂直堆叠对齐浏览器 + HeadlessTests 断言子 div y 递增。

### 5.3 A — border-radius 圆角

**现状**:`render/border.rs` 的 `border_ring(rect, radii, widths, color)` 已支持四边不等宽(`BorderWidths`),但 `radii: &[(f32,f32);4]` 被 `let _ = radii; // 直角退化` 丢弃(圆角留 SDF task)。roadmap §5 v1.2 有旧 border-radius 算法(保留)。

**blitz 参考**:`blitz-paint/kurbo_css/css_box.rs:616` 的 `start_angle(bt_width, br_width, radii)` 闭式解(二次方程 `(k-2)s²-2ks+k=0`,`k=radii.y/(w·radii.x)`,避开 k=2 处 removable singularity)+ `:46-75` 的 radius 自动缩放(相邻角半径和≤边长)。配套 4 个回归测试。border-style 全支持(solid/double/dashed/dotted/groove/ridge/inset/outset)。

**改法**:
- `start_angle` 闭式解 + radius 自动缩放数学**原样照搬**(纯数学,无依赖)。
- 在 `border_ring` 真正用起 `radii`(去掉 `let _ =`),按圆角数学产 mesh 顶点。
- 适配:kurbo `BezPath` → LoomGUI 自绘 mesh 顶点(圆角用多边形/曲线离散)。
- **与 v1.2 旧算法关系**(待 plan 定):优先用 blitz 数学重写(更全,含四角不等 + 椭圆圆角);v1.2 旧算法对照参考,不直接迁移。

**A scope 边界**:只做圆角几何(border-radius 各形态)。border-style 扩展(dashed/dotted/groove/ridge/inset/outset)**不在 A**,属阶段2 视觉束延伸——blitz `css_box.rs` 虽全支持,本阶段只取其圆角数学。

**验收**:Rust render 层断言圆角几何(RenderNode 顶点)+ 浏览器打开验收 HTML 人工对比圆角视觉。

### 5.4 G — 多层 background

**现状**:LoomGUI background 单层。`style/computed.rs`+`mapping.rs` 有 background 解析,绘制端单层。

**blitz 参考**:`blitz-paint/render/background.rs`(899 行,1 panic,成熟)。多层 + background-clip/origin/size/ repeat(含 space/round),唯一缺 background-attachment。

**改法**:多层 background 管线照搬(数据结构 + 绘制),接入 render 层 background 绘制(现单层 → 多层循环)。background-clip/origin/size/repeat 语义对齐标准 CSS。

**验收**:computed style 断言多层 background 进 style + Rust render 层断言绘制数据 + 浏览器视觉。

### 5.5 I — 渐变增强

**现状**:`style/computed.rs`+`mapping.rs` 有渐变解析(程度待确认),绘制端程度待确认。

**blitz 参考**:`blitz-paint/gradient.rs`(520 行,4 unwrap)。linear/radial/conic 全套。

**改法**:先 plan 阶段确认 LoomGUI 渐变现状(解析 + 绘制各支持到哪),缺的(radial/conic)按 blitz 补。

**验收**:三种渐变 computed style + render 层 + 浏览器视觉。

### 5.6 J — CSS 字体单位 ex/ch/ic(可选)

**现状**:`style/mapping.rs` 的 `parse_lp` 不支持 ex(ch=0 字宽,ex=x-height,ic=表意字宽)。

**blitz 参考**:`blitz-dom/font_metrics.rs`(158 行)。CSS `ex/ch/ic` 单位的字体度量。

**改法**:`parse_lp` 加 ex/ch/ic 单位分支 + font_metrics 提供换算。独立小项,可塞 Spec-P2 顺手做。

**验收**:ex/ch/ic 换算单测(computed style 尺寸)。

## 6. 验收方案(仿 spec4b,headless 不碰 Unity)

**约束**:当前无 Unity 环境,showcase 8 页只打包通过(2026-07-21 unblock)、未在 Unity 跑过。故本阶段验收 headless 为主,不依赖 showcase 视觉回测。

**范式**:仿 spec4b —— `showcase/spec4b/spec4b-acceptance.html`(简洁验收页)+ `tests/dotnet/LoomGUI.HeadlessTests/AcceptanceGateTests.cs`(C# P/Invoke dll,加载 pkg.bin → Instantiate → Tick → 断言 layout rect / computed style / 树 / 事件,本机跑不碰 Unity)+ `Harness/StageHarness.cs`(建 Stage + UIContext,1280×720)。

**三层验收**:

1. **验收 HTML**:写阶段1 验收页(圆角 + 多层 bg + 渐变 + block 垂直堆叠的最小例,仿 spec4b-acceptance 简洁)。既喂 HeadlessTests,又能在浏览器打开人工对比。
2. **C# HeadlessTests**:加载验收页 pkg.bin,断言:
   - C2 真 block:子 div 垂直堆叠(y 递增,不再 flex row)。
   - J ex/ch/ic:尺寸换算对。
   - A/G/I:background/gradient 进了 computed style。
3. **Rust render 层 + rect-diff**:
   - 圆角几何 / 多层 bg / 渐变的**绘制数据**在 RenderNode,C# 投影层测不到 → Rust `dump_*.rs`/smoke 断言。
   - C2 的 layout 对齐用 `showcase/scripts/rect-diff`(headless Chrome rect vs LoomGUI rect,roadmap §3.4 护城河设施)。
4. **视觉**(圆角/渐变好不好看):没 Unity,浏览器打开验收 HTML 人工对比(双方都按标准 CSS 渲染,对得上 = 对)。

**分层诚实标注**:HeadlessTests 是 C# 投影层,验 layout / computed style / 树 / events;圆角/渐变的视觉几何在 render 层,得 Rust 侧断言 + 浏览器人工对比补。spec/plan 里讲清这个分层,避免「HeadlessTests 绿 = 视觉对」的假绿。

## 7. 风险

| 风险 | 处置 |
|---|---|
| **C2 flex→block 改变所有 div 布局结果**(最大风险) | rect-diff 全量验 block 对齐浏览器 + HeadlessTests 断言。C2 是阶段1 成败关键。 |
| **pkg.bin 格式 bump 闭环** | dll + GUI exe 重编入库(CLAUDE.md 工作流);bincode round-trip 单测自验。 |
| **taffy 0.12 行为微调**(flexbox bugfix + abspos/margin collapse 交互,layout 像素级可能变) | rect-diff 验;接受符合标准的微调,视为 bugfix。 |
| **C2 真 block 边缘 case**(margin collapse / BFC)headless 小例验不全 | 接受;复合束文本模型(p/h1-h6 block)时补全验收。 |
| **A 与 v1.2 旧算法关系未定** | plan 阶段定(blitz 数学重写 vs 旧算法迁移)。 |

## 8. 参考资产

- Blitz 评估(完整 A-L 清单 + 分档):`docs/superpowers/specs/2026-07-21-blitz-refactor-evaluation.md`
- Blitz 源码(只读):`tmp/blitz/packages/{blitz-paint,blitz-dom}/`
- spec4b 验收范式:`showcase/spec4b/spec4b-acceptance.html` + `tests/dotnet/LoomGUI.HeadlessTests/AcceptanceGateTests.cs` + `Harness/StageHarness.cs`
- rect-diff 设施:`showcase/scripts/rect-diff/`
- roadmap tech-debt:`docs/roadmap/roadmap.md` §4 tech-debt 段
- taffy 升级调研:本 spec §5.1(基于 taffy 0.5/0.8/0.9/0.10/0.11/0.12 源码对照)

## 9. 下一步

本 spec 批准后,分别给 Spec-P1(taffy+block)/ Spec-P2(视觉)过 **writing-plans** 出实现计划。P1 先行(地基),P2 在 P1 真 block 基线上做。
