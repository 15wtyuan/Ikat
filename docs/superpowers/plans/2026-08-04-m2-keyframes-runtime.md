# M2 · @keyframes runtime + transition 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 fence 已就绪的 `@keyframes`/`animation` DSL 接通到 runtime——独立 `KeyframePlayer` 时间轴驱动 `NodeAnim`，交付 public-api §9 功能完整的动画系统（keyframes 渲染层动画 + transition + Animation 句柄 L3 全套 + @loom-hook + :nth-child + opacity 父级累积）。

**Architecture:** 路线甲（独立 KeyframePlayer，不翻译 Tween）。player 由 tick dt 驱动（与 TweenManager 并列，单一时钟），写 `NodeAnim` 渲染层四通道（transform TRS / opacity / bg-color / text-color）。Animation 句柄即 player（slotmap 稳定 Key）。class 声明式触发（rematch 检测）+ `node.Play` 程序化触发（FFI）。优先级靠写入顺序（player 后于 tween = animation 赢）。引擎终态（池化/28缓动/layout 动画）拆 M2.5。

**Tech Stack:** Rust（core + fence + packer + ffi_c），C#（Unity 投影），csbindgen FFI，pkg.bin v29→v30，slotmap 1.1。

## Global Constraints

- **两台机约束**：本机（编码机）跑 headless 测试锁逻辑；Unity PlayMode 真机验收（home 入场动画 + 动画验收页）留家里机，和 M0 一起排队。
- **pkg 三向同步**（坑 66/158/177）：改 pkg 格式后必须 dll + GUI exe + 所有 fixture pkg.bin 三者同步重编/重打 + commit，否则 rc=-1 或渲染错。本计划 pkg v29→v30。
- **加新机制必 grep 全消费点**（AGENTS.md 教训）：player 写 `NodeAnim` 后，强制 grep 所有读 `NodeAnim` 的点（`compute_world_transforms` 读 anim.transform / `render` 读 anim.opacity·bg_color·text_color）确认消费正确；新增 cascade 属性 `animation` 确认 rematch/inherit 路径。
- **单一动画时钟不变量**：player.update 受 tick dt 驱动（step b，和 TweenManager.update 并列），不引入第二个时钟源（ScrollPane 自维护 tween 是既定例外，player 同模式）。
- **Rust edition 2021**，依赖钉版本；CSS 选择器解析器手搓（零新依赖）；slotmap 1.1 已是项目依赖。
- **push 前**：`cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings` + `cargo test`（全 workspace）+ `dotnet test`（HeadlessTests）+ PublicApi 编译门。
- **变更同步**：任何 Rust 改动后重编 .dll（`cargo build -p loomgui_ffi_c --release` + 拷贝到 `unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll`）+ `cargo run -p xtask -- sync-bindings`；parse-time 逻辑改（fence/bridge）须重打 pkg。
- **围栏真相源**：`crates/fence/src/schema/` Rust const 表 + `docs/design/fence.md` 可读镜像，改 schema 必同步 fence.md（防漂移门 `cargo test -p loomgui_fence` 含文档↔schema 交叉校验）。
- **字段命名**：player 句柄 = `PlayerKey`（slotmap key，u64），C# `ulong`；时间变量 core 用 `elapsed`(wall)/`current_time`(iteration 内)；AnimationSpec 字段名对齐 CSS（`iteration_count`/`fill_mode`/`timing_function`/`play_state`）。
- **fence 动画长划子属性 defer**：M2 只做 `animation`/`transition` 简写，不做 8 个长划子属性（tech-debt 写 roadmap）。

**权威 spec**：`docs/superpowers/specs/2026-08-04-m2-keyframes-runtime-design.md`。数据结构定义见 spec §4.2，tick 时序见 §5，优先级见 §6，FFI 见 §7.3，测试矩阵见 §9，验收 showcase 见 §10。本计划实现层精确化任务编排，与 spec 决策一致。

---

## 文件结构（改动地图）

| 文件 | 责任 | 改动 |
|---|---|---|
| `crates/core/src/asset/mod.rs` | pkg 格式 + ComponentTemplate | v30 bump + `ComponentTemplate.keyframes` 字段 + 序列化 |
| `crates/core/src/asset/tests.rs` | pkg 测试 | v30 roundtrip/reject + bincode 形状锁 |
| `crates/core/src/style/resolved.rs` | ResolvedStyle | 加 `animation: Vec<AnimationSpec>` + AnimationSpec/Direction/FillMode/PlayState |
| `crates/core/src/tween.rs` | Ease 枚举 | 加 `Step{start:bool}` 变体 + ease 对齐映射 |
| `crates/core/src/scene/animation.rs` | **新建** KeyframePlayer + 时间轴 | player struct + update 推进 + TRS lerp + 事件 emit |
| `crates/core/src/scene/node.rs` | Scene | 加 `keyframes: HashMap` + `players: SlotMap` 字段 |
| `crates/core/src/scene/mod.rs` | Scene re-export | 导出 animation 模块 |
| `crates/core/src/stage.rs` | tick_and_render | 加 step b（player.update）+ step g'（sync_animation_players）|
| `crates/core/src/style/dynamic.rs` | rematch + transition + matcher | sync_animation_players + :nth-child 分支 |
| `crates/core/src/render/mod.rs` | render | opacity 父级累积（DFS parent_alpha）+ 确认消费 anim |
| `crates/fence/src/css_rules.rs` | @keyframes + selector | :nth-child selector + @loom-hook 锚点 |
| `crates/fence/src/schema/css.rs` | CssValueParser | animation/transition 简写解析产 AnimationSpec/TransitionSpec |
| `crates/fence/src/css_resolve.rs` | apply cascade | animation/transition 存值 bake base_style |
| `crates/core/src/style/mapping.rs` | transform 解析 | 新增 parse_transform_trs（TRS 分解，供 keyframes）|
| `crates/packer/pkg/src/bridge.rs` | IrTree→TemplateNode 桥 | 翻译 keyframes + transform TRS + @loom-hook |
| `crates/ffi_c/src/lib.rs` | C FFI | play/pause/resume/stop/get-set-time/get-state/on-key |
| `unity/package/Runtime/Projection/Animation.cs` | **新建** C# Animation 类 | sealed class 全套 |
| `unity/package/Runtime/Public/LoomGUI.Nodes.cs` | C# Node | `Play(name)` 方法 |
| `unity/package/Runtime/Public/LoomGUI.Events.cs` | C# 事件 | AnimationStart/End/Iteration/Key/Hook + TransitionEnd |
| `unity/package/Runtime/Host/EventDemuxer.cs` | 事件 demux | 动画事件路由 |
| `tests/dotnet/LoomGUI.HeadlessTests/` | headless 测试 | animation.workspace fixture + AnimationHandleTests |
| `showcase/` | showcase | home.html 解注释 + m2 动画验收 demo 页 |
| `docs/design/{main-design,public-api,fence}.md` | 设计文档 | 动画章节对齐实现 |
| `docs/roadmap/{roadmap,milestones}.md` | 路线文档 | M2 done + M2.5 立项 + tech-debt 更新 |

---

## Task 依赖总览

```
T1 (pkg v30 + core 类型地基)
 ├─► T2 (fence animation/transition 简写存值)
 │    └─► T3 (fence @loom-hook + bridge 翻译 keyframes + transform TRS 分解)
 ├─► T4 (fence :nth-child selector + core matcher)  [可与 T2/T3 并行]
 ├─► T5 (KeyframePlayer 时间轴推进 + 单测)
 │    └─► T6 (player.update 写 NodeAnim + tick step b + 优先级)
 │         └─► T7 (sync_animation_players step g' + class 触发 + fill 完成态 + 多 animation)
 │              ├─► T8 (opacity 父级累积)
 │              └─► T9 (事件 emit START/END/ITERATION/KEY/HOOK + OnKey 注册)
 │                   └─► T10 (FFI play/control/on-key + csbindgen + sync-bindings)
 │                        └─► T11 (C# Animation 类 + Node.Play + 事件 struct + demux)
 │                             ├─► T12 (core 集成测 端到端确定性断言)
 │                             ├─► T13 (headless C# 句柄测)
 │                             └─► T14 (M2 动画验收 showcase + fixture + home 解注释)
 │                                  └─► T15 (dll 重编 + pkg 重打 + 全门 + 文档同步 + M2.5 立项)
```

T1 是地基。fence 链（T2→T3）产数据。player 链（T5→T6→T7→T8/T9）建引擎，严格顺序。投影链（T10→T11）暴露 C#。验收链（T12→T13→T14→T15）测试收口。

---

## Phase 0：pkg + core 类型地基

### Task 1: pkg v30 + core 动画类型定义

定义所有动画类型 + pkg 升 v30，让 keyframes 表与 animation 声明能序列化活到 runtime。

**Files:** `crates/core/src/asset/mod.rs`（PKG_FORMAT_VERSION+MIN/MAX、ComponentTemplate.keyframes、write/read_package）、`asset/tests.rs`、`style/resolved.rs`（AnimationSpec+3 enum+ResolvedStyle.animation）、`tween.rs`（Ease::Step）、`scene/node.rs`（Scene.keyframes+players）、`scene/animation.rs`（**新建**类型+KeyframePlayer 占位）、`scene/mod.rs`（pub mod）

**Interfaces:**
- Consumes: 无（地基）
- Produces: spec §4.2 全部类型（`KeyframesRule`/`KeyframeStop`/`AnimatableProps`/`TransformAnim`/`AnimationSpec`/`AnimationDirection`/`AnimationFillMode`/`AnimationPlayState`/`KeyframeStopSelector`）+ `PlayerKey`(slotmap key u64) + pkg v30

- [ ] **Step 1: 写失败测试**（`asset/tests.rs`）— v30 roundtrip：构造带 keyframes（含 hook）+ animation 声明的 ComponentTemplate，write→read，断言 `ct.keyframes[0].stops[1].hook == Some("done")` + `ct.nodes[0].style.animation[0].fill_mode == Both` + `iteration_count == None`
- [ ] **Step 2: 验失败** — `cargo test -p loomgui_core asset::tests` 编译错（类型/字段不存在）
- [ ] **Step 3: 实现**
  - `asset/mod.rs`: `PKG_FORMAT_VERSION=30` + `MIN/MAX=30`
  - `scene/animation.rs` 定义全部类型（serde + Debug/Clone/PartialEq；enum `#[repr(u8)]`+Default；struct Default where applicable）。`KeyframePlayer` 先占位空 struct（T5 填）。`pub type PlayerKey = slotmap::DefaultKey;`
  - `style/resolved.rs` `AnimationSpec{name,duration,delay,iteration_count:Option<u32>,direction,fill_mode,timing_function:Ease,play_state}` + 3 个 `#[repr(u8)]` enum（Normal/Reverse/Alternate/AlternateReverse；None/Forwards/Backwards/Both；Running/Paused）+ `ResolvedStyle.animation: Vec<AnimationSpec>`
  - `tween.rs` `Ease` 追加 `Step{start:bool}`（保持 repr(u8) 判别值稳定）+ `evaluate`：阶跃 `if start||t>=dur {1.0} else {0.0}`
  - `scene/node.rs` `Scene` 加 `keyframes: HashMap<String,KeyframesRule>` + `players: SlotMap<PlayerKey, KeyframePlayer>` + Default 初始化
  - `asset/mod.rs` `ComponentTemplate.keyframes: Vec<KeyframesRule>` + write_package（u32 len + 逐 rule 序列化）+ read_package 反向 + `ct()` helper 补字段
- [ ] **Step 4: 验通过** — `cargo test -p loomgui_core asset::tests` 绿 + `cargo build -p loomgui_core`
- [ ] **Step 5: commit** — `feat(core): pkg v30 + animation types (KeyframesRule/AnimationSpec/player slotmap)`

---

## Phase 1：fence 解析（产数据进 pkg）

### Task 2: fence animation/transition 简写解析存值

`animation`/`transition` 简写从"只校验"变成"解析存值"bake 进 base_style。core transition 引擎已在，T1 接通配置源即生效。

**Files:** `crates/fence/src/schema/css.rs`（parse_animation_value/parse_transition_value）、`crates/fence/src/css_resolve.rs`（arm 调 parse 存值，删 continue）。Test: `crates/fence/tests/animation_parse.rs`

**Interfaces:**
- Consumes: T1 `AnimationSpec`/`TransitionSpec`/`Ease`(含 Step)
- Produces: 打包期 `base_style.animation`/`base_style.transition` 有值

- [ ] **Step 1: 写失败测试**（`crates/fence/tests/animation_parse.rs`）

```rust
use loomgui_core::style::resolved::{AnimationSpec,AnimationDirection,AnimationFillMode,AnimationPlayState};
use loomgui_core::tween::{Ease,TweenProp};
// 测 1: "fadeIn .4s .1s infinite alternate both ease" → name=fadeIn,duration=.4,delay=.1,
//   iteration=None,direction=Alternate,fill=Both,timing=CubicOut
// 测 2: "a .3s, b .5s infinite" → len 2
// 测 3: "opacity .3s ease .05s" → TransitionSpec{prop:Some(Opacity),duration:.3,ease:CubicOut,delay:.05}
// 测 4: "all .2s linear" → prop=None,duration=.2,ease=Linear
```

- [ ] **Step 2: 验失败** — `cargo test -p loomgui_fence` 编译错（函数不存在）
- [ ] **Step 3: 实现**
  - `schema/css.rs` `pub fn parse_animation_value(v:&str)->Vec<AnimationSpec>`：逗号拆多声明；每声明 tokenize → 首合法标识=name，**首个 time=duration，次 time=delay**（修粗糙处理），`infinite`→iteration_count=None / 整数→Some(n)，direction/fill/play-state/timing 关键字映射（ease→CubicOut，ease-in→QuadIn，ease-out→QuadOut，ease-in-out→QuadInOut，linear→Linear，step-start→Step{start:true}，step-end→Step{start:false}）
  - `pub fn parse_transition_value(v:&str)->Vec<TransitionSpec>`：`<prop?> <dur> <ease?> <delay?>`，prop=opacity/color/background-color/all(→None)
  - `css_resolve.rs` animation arm：`style.animation=parse_animation_value(value);`（替换 continue）；transition arm：`style.transition=parse_transition_value(value);`
- [ ] **Step 4: 验通过** — `cargo test -p loomgui_fence` 绿（含 doc_schema_sync）
- [ ] **Step 5: commit** — `feat(fence): parse animation/transition shorthand into specs`

### Task 3: @loom-hook 锚点 + bridge 翻译 keyframes + transform TRS 分解

接通 fence→pkg keyframes 通路 + @loom-hook + keyframes stop 的 transform 走 TRS 分解。

**Files:** `crates/fence/src/css_rules.rs`（parse @loom-hook 注释）、`crates/fence/src/pipeline.rs`（KeyframeStop 加 hook）、`crates/core/src/style/mapping.rs`（parse_transform_trs）、`crates/packer/pkg/src/bridge.rs`（translate_keyframes）、`crates/packer/pkg/src/build.rs`（传 keyframes）。Test: `crates/packer/pkg/tests/keyframes_bridge.rs`

**Interfaces:**
- Consumes: T1 core KeyframesRule/AnimatableProps/TransformAnim
- Produces: pkg.bin 带 keyframes 表；KeyframeStop.hook 有值

- [ ] **Step 1: 写失败测试** — HTML `@keyframes slideIn{from{opacity:0;transform:translateY(20px)}/* @loom-hook start */ to{opacity:1;transform:none}}` + `.card{animation:slideIn .4s both}` → invoke fence→bridge→write→read，断言 keyframes[0].stops[0].hook==Some("start") + props.transform==Some(TransformAnim{translate:Some([0.,20.]),..}) + nodes[0].style.animation[0].name=="slideIn"
- [ ] **Step 2: 验失败** — bridge 丢弃 / hook None / transform 未分解
- [ ] **Step 3: 实现**
  - `css_rules.rs` `parse_stop_declarations` 内正则 `/\* @loom-hook (\S+) \*/` 提 hook 挂 stop
  - fence `KeyframeStop` 加 `hook:Option<String>`（pipeline.rs struct + css_rules.rs 构造处）
  - `mapping.rs` `pub fn parse_transform_trs(v:&str)->Option<TransformAnim>`：复用 `iter_transform_funcs`，translate(x,y)→translate，scale→scale，rotate(deg)→rotate=radians；任一非 TRS 函数→None
  - `bridge.rs` `fn translate_keyframes(fence_kfs)->Vec<core::KeyframesRule>`：stop declarations 遍历，opacity→parse f32，transform→parse_transform_trs，background-color/color→parse_color，hook 直传；`pack_components` 调用填 ComponentTemplate.keyframes
- [ ] **Step 4: 验通过** — `cargo test -p loomgui_pkg` 绿
- [ ] **Step 5: commit** — `feat(packer): bridge keyframes to pkg + @loom-hook + transform TRS decomposition`

### Task 4: fence :nth-child selector + core matcher

selector 子集加 `:nth-child(An+B|odd|even|N)`，让 home 错峰规则可匹配。

**Files:** `crates/fence/src/css_rules.rs`（selector parser 加 :nth-child 变体 + specificity）、`crates/core/src/style/dynamic.rs`（compound_matches_node 加 nth-child 分支）。Test: `crates/fence/tests/nth_child.rs` + `crates/core/tests/nth_child_match.rs`

**Interfaces:**
- Consumes: fence `ParsedSelector`/core `compound_matches_node`
- Produces: `:nth-child(N)` 运行时可匹配

- [ ] **Step 1: 写失败测试**
  - fence: `.item:nth-child(2n+1)` 解析成 `:nth-child{a:2,b:1}` + specificity；`:nth-child(odd)`=`2n+1`，`:nth-child(3)`=`0n+3`，`:nth-child(even)`=`2n`
  - core: 3 子节点，`:nth-child(2)` 匹配第 2 个；`:nth-child(odd)` 匹配 1/3；`:nth-child(2n+1)` 匹配 1/3
- [ ] **Step 2: 验失败** — selector 解析拒 / matcher 返 false
- [ ] **Step 3: 实现**
  - fence selector parser：`:nth-child(...)` 内解析 `An+B`（正则 `^(\d*)n\s*([+-]\s*\d+)?$` 或纯整数或 odd/even）→ 产 `PseudoClass::NthChild{a:i32,b:i32}`；specificity = 伪类 1 档
  - core `compound_matches_node`/`pseudo_matches`：`:nth-child` 分支，查节点在父 `children` 的 1-based index `i`，匹配 `(i as i32 - b) % a == 0 && (i as i32 - b)/a >= 0`（a=0 时 i==b）
- [ ] **Step 4: 验通过** — 两处测试绿
- [ ] **Step 5: commit** — `feat(fence+core): :nth-child(An+B|odd|even|N) selector + matcher`

---

## Phase 2：core player 引擎

### Task 5: KeyframePlayer 时间轴推进（核心算法 + 单测）

实现 player 的纯时间轴逻辑（无副作用：不写 NodeAnim、不 emit 事件，只算"当前应取的属性值 + 状态"）。这是 player 的可单测核心。

**Files:** `crates/core/src/scene/animation.rs`（KeyframePlayer 完整字段 + 推进函数）、`crates/core/tests/player_timeline.rs`

**Interfaces:**
- Consumes: T1 KeyframesRule/AnimatableProps/TransformAnim/AnimationSpec/Ease；spec §5.3 时间轴算法
- Produces: `KeyframePlayer` + `fn advance(player, dt) -> PlayerFrame` 其中 `PlayerFrame{props:AnimatableProps, completed:bool, iteration_boundary:Option<u32>, play_state:PlayerPlayState}`

- [ ] **Step 1: 写失败测试**（确定性，固定 dt）

```rust
// fadeIn opacity 0→1 .4s both, ease CubicOut
// 测 1: advance(dt=0.0) → props.opacity≈0.0 (backwards fill 首帧)
// 测 2: advance 累计 0.2s → progress=0.5, opacity=CubicOut(0.5)≈0.82
// 测 3: advance 累计 0.4s → opacity=1.0, completed=true
// 测 4: infinite pulse scale 1↔1.1 .4s alternate: 0s→1.0, .2s(iter0 progress.5)→1.1,
//       .4s(iter1 progress0)→1.0, .6s(iter1 progress.5)→1.1（alternate 偶正奇反）
// 测 5: reverse: progress=1-elapsed/duration
// 测 6: delay 0.1s: advance 0.05s → 仍 backwards 首帧；advance 0.15s → progress=0.125
// 测 7: Step ease: progress .5 → Step{start} =1.0, Step{end}=0.0
```

- [ ] **Step 2: 验失败** — 函数不存在
- [ ] **Step 3: 实现**（spec §5.3 精确版）
  - `KeyframePlayer` 完整字段：node/AnimationSpec/keyframes(Value)/elapsed/current_time/iteration/play_state(PlayerPlayState:Playing/Paused/Completed/Stopped)/on_key_percents/fired_keys/fired_start/last_progress
  - `pub(crate) enum PlayerPlayState{Playing,Paused,Completed,Stopped}`
  - `fn advance(&mut self, dt:f32) -> PlayerFrame`：spec §5.3 伪代码——elapsed+=dt；<delay→backwards fill 首帧 return；anim_time=elapsed-delay；iteration=anim_time/duration；current_time=anim_time%duration；progress=current_time/duration；完成判定 iteration_count；direction 应用（用当前 iteration）；stop 间 lerp（per-property 分量级：opacity 数值 lerp，color [f32;4] lerp，transform TRS 各分量 lerp + None 用 identity）+ per-segment timing_function（段首 stop 的 ease，默认末段用整体 timing_function）；产 PlayerFrame
  - ease 求值：`Ease::evaluate(t,dur)` 复用现有（CubicOut 等）+ 新 Step
- [ ] **Step 4: 验通过** — `cargo test -p loomgui_core --test player_timeline` 全绿
- [ ] **Step 5: commit** — `feat(core): KeyframePlayer timeline advance (delay/iteration/direction/fill/ease/TRS lerp)`

### Task 6: player.update 写 NodeAnim + tick step b + 优先级

把 T5 的纯时间轴接进 tick：player.update 写 NodeAnim，接入 tick step b（在 tween.update 之后 = animation 优先）。

**Files:** `crates/core/src/scene/animation.rs`（update 写 NodeAnim）、`crates/core/src/stage.rs`（tick step b 加 players 遍历）、`crates/core/tests/player_write_anim.rs`

**Interfaces:**
- Consumes: T5 advance + PlayerFrame
- Produces: tick step b 推进所有 player + 写 NodeAnim（transform 合成 Affine2 / opacity / bg_color / text_color）

- [ ] **Step 1: 写失败测试**
  - 构造 Scene + 手动 insert player + tick(dt) → 断言 NodeAnim.opacity/transform 被写
  - 优先级：同节点 tween（transition opacity .3→.7）+ player（animation opacity 0→1）→ player 后写覆盖，NodeAnim.opacity = player 值
- [ ] **Step 2: 验失败** — NodeAnim 未被 player 写 / tick 无 player 遍历
- [ ] **Step 3: 实现**
  - `animation.rs` `pub fn update_all(scene:&mut Scene, dt:f32, out:&mut Vec<EventRecord>)`：遍历 `scene.players`，对 Playing 态调 advance，据 PlayerFrame 写对应节点的 NodeAnim（`scene.anim.ensure(node)`，transform 合成 Affine2 = translate∘scale∘rotate，opacity/bg_color/text_color 直写）；Completed+fill forwards/both 保留末值（不回收）；Completed+fill none/backwards 标记移除
  - `stage.rs` tick_and_render step b：现有 `tweens.update(dt,...)` **之后**加 `animation::update_all(scene, dt, &mut events)`（保证 player 后写 = animation 优先）；事件并入 events Vec
  - grep 确认 `compute_world_transforms`/`render` 消费 anim 正确（应零改，它们本就读 anim）
- [ ] **Step 4: 验通过** — 测试绿 + `cargo test -p loomgui_core`
- [ ] **Step 5: commit** — `feat(core): player.update writes NodeAnim in tick step b (animation > transition priority)`

### Task 7: sync_animation_players step g' + class 触发 + fill 完成态 + 多 animation

class 声明式触发：rematch 后检测 animation 声明变化启停 player。多 animation 并存。fill 完成态保留/回收。

**Files:** `crates/core/src/style/dynamic.rs`（sync_animation_players）、`crates/core/src/stage.rs`（tick step g'）、`crates/core/tests/sync_players.rs`

**Interfaces:**
- Consumes: T6 update_all；节点 computed style.animation（rematch 后可见）
- Produces: class 触发启停 player；多 player 并存；fill forwards/both 保留

- [ ] **Step 1: 写失败测试**
  - 节点带 `.fade{animation:fadeIn .4s both}`，初始无 class → 加 class → 下帧 sync 启 player → 推进 → 完成 forwards 保留末值（opacity=1 持续）；移除 class → player 回收 → NodeAnim 该通道回 None
  - `animation: a .3s, b .5s` → 2 player；同通道后者覆盖
  - node.Play 的 player 不受 sync 管（class 去掉不回收，靠 Stop）
- [ ] **Step 2: 验失败** — 加 class 无 player 启动
- [ ] **Step 3: 实现**
  - `dynamic.rs` `pub fn sync_animation_players(scene:&mut Scene)`：遍历节点，读 computed `style.animation`（rematch 后）vs 节点当前活跃 player（按 node 查 players + spec.name 比对）：新 name→insert player（含 backwards fill 首帧立即写 NodeAnim）；name 消失→remove player；参数变→kill 旧 insert 新。**node.Play 的 player 标记 `programmatic=true`**，sync 跳过
  - `stage.rs` tick step g'（rematch f 之后、solve i 之前）调 `sync_animation_players`
  - 多 animation：sync 为每声明建独立 player；update_all 按 Vec 顺序写（后者覆盖前者同通道）
- [ ] **Step 4: 验通过** — `cargo test -p loomgui_core`
- [ ] **Step 5: commit** — `feat(core): sync_animation_players (class trigger + fill lifecycle + multi-animation)`

### Task 8: opacity 父级累积传播

render DFS 传 parent_alpha，子 RenderNode.alpha = parent_alpha × own，符合 web 标准 opacity 语义。

**Files:** `crates/core/src/render/mod.rs`（render 主 DFS 传 parent_alpha；render_one_node 用累积 alpha）、`crates/core/tests/opacity_accumulation.rs`

**Interfaces:**
- Consumes: T6 NodeAnim.opacity
- Produces: 子节点 alpha 累积父级

- [ ] **Step 1: 写失败测试** — 父 opacity=0.5（player 写）+ 子 opacity=1.0 → 子 RenderNode.alpha==0.5；父 0.5 + 子 0.4 → 子 0.2
- [ ] **Step 2: 验失败** — 子 alpha=1.0（未累积，per-node）
- [ ] **Step 3: 实现** — render 主 DFS（调 render_one_node 的循环）维护 `parent_alpha: f32`，进子树前 `child_parent_alpha = parent_alpha * node_alpha`；render_one_node 的 `alpha` 参数改为累积值（已有签名读 own，改为接收累积）；存 RenderNode.alpha = 累积值。后端零改
- [ ] **Step 4: 验通过** — 测试绿 + 既有 render 测试不回归
- [ ] **Step 5: commit** — `feat(render): opacity parent accumulation (DFS parent_alpha * own)`

### Task 9: 事件 emit START/END/ITERATION/KEY/HOOK + OnKey 注册

player 检测阈值 emit 事件（补齐 source-less AnimationStart/Iteration + 新增 KEY/HOOK）。OnKey 百分比注册到 core。

**Files:** `crates/core/src/scene/animation.rs`（update_all emit 事件）、`crates/core/src/event.rs`（事件类型常量 + EventRecord 构造）、`crates/core/tests/player_events.rs`

**Interfaces:**
- Consumes: T6 update_all + T7 sync
- Produces: `EVT_ANIMATION_START/END/ITERATION/KEY/HOOK` EventRecord（字段：node_id, player_key, name, percent/hook_name/iteration payload）

- [ ] **Step 1: 写失败测试**
  - fadeIn .4s：首帧 emit START（一次）；完成 emit END；infinite pulse：每 iteration 边界 emit ITERATION
  - OnKey(.5) 注册 → progress 跨越 .5 emit KEY{percent:.5}；@loom-hook "half" 在 50% stop → 跨越 emit HOOK{name:"half"}
- [ ] **Step 2: 验失败** — 无事件 emit
- [ ] **Step 3: 实现**
  - `event.rs` 加 5 个事件 type 常量 + helper 构造（复用 EventRecord 现有字段编码 payload：name 字符串走既有 str 表 / percent 复用某 f32 槽 / iteration 复用 u32 槽；参考 EVT_TWEEN_COMPLETE 复用模式）
  - `animation.rs` update_all：advance 后据 PlayerFrame + player.fired_start/fired_keys 判定 emit START（首帧）/ITERATION（iteration 边界跨越）/END（完成）/KEY（on_key_percents 跨越）/HOOK（keyframes stop.hook 跨越）；防同 iteration 重复（fired_keys 记录）
  - OnKey 注册：`pub fn register_on_key(scene, key:PlayerKey, pct:f32)` push player.on_key_percents
- [ ] **Step 4: 验通过** — 测试绿
- [ ] **Step 5: commit** — `feat(core): animation events (START/END/ITERATION/KEY/HOOK) + OnKey registration`

---

## Phase 3：FFI + C# 投影

### Task 10: FFI play/control/on-key + csbindgen + sync-bindings

把 player 暴露成 C ABI，供 C# 句柄控制。

**Files:** `crates/ffi_c/src/lib.rs`（7 个 FFI 函数 + csbindgen 注解）。Test: `crates/ffi_c/tests/` 或 core 集成

**Interfaces:**
- Consumes: T9 player 事件 + Scene.players/PlayerKey
- Produces: C ABI `loomgui_stage_play_animation`/`pause`/`resume`/`stop`/`get_animation_time`/`set_animation_time`/`get_animation_state`/`animation_on_key`

- [ ] **Step 1: 写失败测试** — core 侧测 FFI 调用：play_animation 返 player_key(>0)；pause 后 get_animation_state==Paused；set_animation_time 后 get 值一致
- [ ] **Step 2: 验失败** — FFI 符号不存在
- [ ] **Step 3: 实现**（csbindgen 模式，参考既有 `loomgui_stage_*`）
  - `play_animation(stage,*const u8,len) -> u64`：查 Scene.keyframes[name] + 建 programmatic player，返 PlayerKey
  - `pause/resume/stop_animation(stage, key:u64)`：改 play_state / 标记移除
  - `get/set_animation_time(stage,key) -> f32` / `(stage,key,f32)`
  - `get_animation_state(stage,key) -> u8`：Playing=0/Paused=1/Completed=2/Invalid=255
  - `animation_on_key(stage,key,pct:f32)`：register_on_key
- [ ] **Step 4: 验通过** — `cargo build -p loomgui_ffi_c` + `cargo run -p xtask -- sync-bindings`（生成 LoomGUIBindings.cs 同步到 unity）
- [ ] **Step 5: commit** — `feat(ffi): animation player control FFI (play/pause/resume/stop/time/state/on-key)`

### Task 11: C# Animation 类 + Node.Play + 事件 struct + demux

C# 投影层：Animation sealed class + Node.Play + 6 个事件 struct + EventBus demux + 句柄回调路由。

**Files:** `unity/package/Runtime/Projection/Animation.cs`（**新建**）、`Public/LoomGUI.Nodes.cs`（Node.Play）、`Public/LoomGUI.Events.cs`（6 事件 struct）、`Host/EventDemuxer.cs`（动画事件路由）。Test: `tests/dotnet/LoomGUI.HeadlessTests/AnimationHandleTests.cs`（T13 细化，本 task 只编译门）

**Interfaces:**
- Consumes: T10 FFI；Spec-4a EventBus/EventDemuxer 模式
- Produces: `Animation` 句柄 + `Node.Play(name)` + 动画事件 struct（public-api §9.2/§7.5）

- [ ] **Step 1: 编译门先行** — PublicApi 项目编译过（签名冻结）
- [ ] **Step 2: 实现**
  - `Animation.cs` sealed class（spec §7.2）：internal playerKey(ulong)/node/onStart/onEnd/onKeys/onHooks；Name/IsPlaying/Time(get set)/Pause/Resume/Stop（FFI 转发）；OnStart/OnEnd/OnKey/OnHook（链式，cb 存 list，OnKey 额外 FFI 注册 pct）；disposed 守卫
  - `LoomGUI.Nodes.cs` `Node.Play(name)`：调 FFI play_animation → new Animation(playerKey,this)
  - `LoomGUI.Events.cs` 6 struct：AnimationStartEvent/EndEvent/IterationEvent（Target,Name[+Iteration]）+ AnimationKeyEvent/HookEvent（句柄私有，Target,Name,Percent/HookName）+ TransitionEndEvent（Target,Prop），均继承现有 RouteEventCore
  - `EventDemuxer.cs`：动画事件类型分派——START/END/ITERATION 走 node.EventBus 广播 + 按 playerKey 查 Animation 实例触发 onStart/onEnd；KEY/HOOK 只按 playerKey 查句柄触发 onKey(pct)/onHook(name)；Animation 实例注册表（playerKey→Animation weak/strong ref）
- [ ] **Step 3: 验编译** — `dotnet build` PublicApi + HeadlessTests 编译过
- [ ] **Step 4: commit** — `feat(c#): Animation handle + Node.Play + animation events + demux`

---

## Phase 4：测试 + 验收

### Task 12: core 集成测（端到端确定性断言）

端到端跨层：HTML @keyframes+animation → pkg → instantiate → tick → 断言 NodeAnim/事件。覆盖 spec §9.1 集成测矩阵 + §10 #1-10。

**Files:** `crates/core/tests/m2_animation_integration.rs`、`tests/dotnet/LoomGUI.HeadlessTests/fixtures/animation.workspace/`（HTML 源，T14 复用）

**Interfaces:**
- Consumes: T1-T11 全部
- Produces: M2 集成测门（编码机逻辑全锁）

- [ ] **Step 1: 写测试**（确定性 dt）覆盖：
  - fadeIn（opacity+translateY, fill both）t=0/.2/.4 取值
  - pulse（scale infinite alternate）不结束
  - spin（rotate infinite linear）
  - hue（bg-color 3-stop lerp）
  - nth-child 错峰（5 item 各 delay，current_time 错峰）
  - fill-mode 四态（none 回 base / forwards-both 保留 / backwards delay 期首帧）
  - direction（normal/reverse/alternate）
  - ease 对比（linear/step）
  - transition（class 改 opacity → tween 发 → 中值）
  - 父级 opacity 累积
  - AnimationEndEvent emit（class + 完成判定）
- [ ] **Step 2: 验通过** — `cargo test -p loomgui_core --test m2_animation_integration` 全绿
- [ ] **Step 3: commit** — `test(core): M2 animation integration (deterministic end-to-end assertions)`

### Task 13: headless C# 句柄测

Animation 句柄 L3 全套 C# 测（Spec-4a harness 复用，P/Invoke 真 dll）。

**Files:** `tests/dotnet/LoomGUI.HeadlessTests/AnimationHandleTests.cs`

- [ ] **Step 1: 写测试** — Play→IsPlaying；Pause→推进 dt→值不变；Resume→继续；Time set→seek 值跳；Stop→回收 IsPlaying=false；OnEnd（完成触发）；OnKey(.5)（跨越触发）；On<AnimationEndEvent>（EventBus 订阅）；@loom-hook OnHook
- [ ] **Step 2: 验通过** — `dotnet test`（HeadlessTests）绿
- [ ] **Step 3: commit** — `test(c#): Animation handle L3 full (Play/Pause/Resume/Stop/Time/OnKey/OnHook/OnEnd)`

### Task 14: M2 动画验收 showcase + fixture + home 解注释

真机验收载体：home.html 解注释 + 动画验收 demo 页（spec §10 12 块）+ headless fixture workspace。

**Files:** `showcase/showcase/home.html`（解注释 nth-child/animation）、`showcase/m2-animation.html`（**新建** 12 块验收页）、`tests/dotnet/LoomGUI.HeadlessTests/fixtures/animation.workspace/`（fixture，含 m2-animation 内容 + pkg 重打）

- [ ] **Step 1: home.html 解注释** — 移除 `.nav-card:nth-child(N){animation-delay}` 的 `TODO(roadmap §4)` 注释包裹，让 7 条错峰规则生效
- [ ] **Step 2: 新建 m2-animation.html** — spec §10 的 12 块（基础属性 1-4 / 时间轴 5-8 / 触发句柄 9-12），每块独立可断言
- [ ] **Step 3: fixture workspace** — `animation.workspace/`（loom.workspace.json + m2-animation.html + res），`cargo run -p loomgui_pkg -- build` 重打 pkg（pkg v30）
- [ ] **Step 4: 验打包** — `cargo run -p loomgui_pkg -- build showcase` exit 0（home + m2-animation 围栏内）+ `cargo run -p loomgui_pkg -- build tests/dotnet/LoomGUI.HeadlessTests/fixtures/animation.workspace` exit 0
- [ ] **Step 5: commit** — `feat(showcase): M2 animation acceptance page (12 blocks) + home.html stagger uncommented + fixture`

### Task 15: dll 重编 + pkg 重打 + 全门 + 文档同步 + M2.5 立项

收口：dll/绑定/pkg 三向同步、全部门绿、文档对齐、roadmap M2.5 立项。

**Files:** `unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll`、`unity/package/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs`、`docs/design/{main-design,public-api,fence}.md`、`docs/roadmap/{roadmap,milestones}.md`

- [ ] **Step 1: dll 重编入库** — `cargo build -p loomgui_ffi_c --release` + `cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll`（Unity 关着）
- [ ] **Step 2: 绑定同步** — `cargo run -p xtask -- sync-bindings`
- [ ] **Step 3: pkg 重打** — showcase + 所有 fixture workspace 重打 pkg v30 + commit
- [ ] **Step 4: GUI exe 重编**（坑 158 同源）— `cd crates/packer/gui/src-tauri && tauri build --no-bundle` + cp 到 `unity/package/Editor/Tools/loomgui_gui.exe`（fence 改了）
- [ ] **Step 5: 全门绿**
  - `cargo fmt --all -- --check`
  - `cargo clippy --all-targets -- -D warnings`
  - `cargo test`（全 workspace）
  - `dotnet test`（HeadlessTests）
  - PublicApi 编译门
  - `cargo test -p loomgui_fence`（doc_schema_sync 含 animation/transition/@loom-hook/nth-child 文档同步）
- [ ] **Step 6: 文档同步**
  - `main-design.md` §13 动画：对齐实现（player 模型 + 单一时钟 + tick 时序 b/g'）
  - `public-api.md` §9：确认 Animation 句柄/事件/@loom-hook 落地描述
  - `fence.md`：加 @keyframes/@loom-hook/:nth-child/animation/transition 可读镜像
  - `roadmap.md` §4：M2 done + tech-debt 更新（动画系统终态从悬置→M2.5 立项）+ §8 决策记录
  - `milestones.md`：M2 标编码端 DONE + **新增 M2.5 行**（依赖 M2、阻塞 M6、估时 2-3 周、进入判据见 spec §12）
- [ ] **Step 7: commit** — `chore(m2): dll/pkg rebin + docs sync + M2.5 charter (animation engine finalization)`

---

## Self-Review（plan 自审）

**Spec 覆盖**：spec §2 现状 → T1/T2/T3 接通缺口；§3 决策 → 路线甲(T5)/A1(T1 类型限定)/opacity 累积(T8)/L3 全套(T11)/TRS 分解(T3)/组件级表(T1);§4 数据结构 → T1;§5 tick/生命周期 → T5/T6/T7;§6 优先级 → T6/T7;§7 句柄/事件 → T10/T11;§8 fence → T2/T3/T4;§9 测试 → T12/T13;§10 showcase → T14;§12 M2.5 → T15。全覆盖。

**关键类型一致性**：PlayerKey=u64（T1 定 → T10 FFI → T11 C# ulong）；PlayerPlayState（T5 定 → T10 get_state u8 映射）；AnimationSpec 字段（T1 定 → T2 parse 产）；KeyframeStop.hook（T3 fence→bridge→core 贯通）。

**无占位**：每 task 有 Files/测试代码或断言/实现要点/commit。

**执行注意**：
- T1 是硬地基，所有 task 依赖——先做且做透（类型 + pkg + 序列化全绿才往下）。
- T5 player 时间轴是算法核心，单测要覆盖 spec §5.3 全部分支（delay/iteration/direction/fill/ease/step）才进 T6。
- T6 后强制 grep 读 NodeAnim 的点（compute_world_transforms/render）确认消费（AGENTS.md 教训）。
- T15 三向同步（dll/GUI exe/pkg）是坑 66/158/177 高发区，逐项验。
