# M2 · @keyframes runtime + transition 设计

> 三束加宽阶段的动画里程碑。fence `@keyframes`/`animation` DSL 已就绪（commit `e2e2812`），
> 本 spec 把"只欠的 runtime 驱动"补齐：keyframes 表进 pkg → core 时间轴播放器驱动 →
> `NodeAnim` 写入 → Animation 句柄 + 事件层。交付一个 **public-api §9 功能完整**的动画系统
> （全 CSS、无命令式 tween、class 声明式 + `node.Play` 程序化双触发、句柄 L3 全套）。
> 引擎内部终态（池化 tween / 28+ 缓动 / layout 动画）拆为 **M2.5** 明确立项，由真实需求触发。

## 1. 背景与动机

### 1.1 roadmap 位置

摸黑（Spec-1~4b）已结束，核心范式（HTML → 打包 → core 建树 → layout → render）端到端打通。
现在处于 §4 三束加宽阶段。`docs/roadmap/milestones.md` 把可执行切片排成 M0~M6 backlog：

```
M0 验收 ──► M1 ListView ──► M4 文本模型 ──► M6 收口
        ├──► M2 keyframes ──► M5 视觉 ──► M6
        └──► M3 TabList ────────────────► M6
```

M1/M2/M3 互不依赖，M0 后可任选顺序。M0（P3 家里机验收 + IME 接线）需家里机，编码机做不了；
M2 的 runtime 部分（Rust core + FFI + C# 投影 + headless 断言）**几乎全是编码机活**，只有最后
"home 入场动画真机绿"要家里机。故 M0 卡家里机时，编码机上推进 M2 runtime，真机验收 defer——
与 M1/M3 既定模式（编码端 DONE、真机 defer 家里机）一致。milestones 虽明写"M0 阻塞 M2 进入"，
但该规则的意图是"不想在未验地基上叠大件"；M2 不碰控件地基，风险独立，破例合理。

### 1.2 直接驱动力

`showcase/showcase/home.html` 的入场动画 = `@keyframes fadeIn`（opacity + translateY）+ `:nth-child`
错峰 animation-delay，是 roadmap 点名的演示场景。当前 fence 已解析 `@keyframes` at-rule 语法，
但 **packer bridge 静默丢弃 keyframes 表、animation 声明不存进 ResolvedStyle**——合法的动画 CSS
运行时完全不跑。本 spec 接通这条链。

### 1.3 与 M2.5 的边界

grill 阶段发现一个 roadmap 漏洞：动画**引擎终态**（池化 tween、28+ 缓动、链式 builder、layout 动画
的 prop_type 分层）在 milestones M5 被列进"明确砍"，在 §4 又标"推 M5 或更后"——实际**没有明确归宿**。
本 spec 把动画能力分两层：
- **public-api §9 契约功能**（keyframes 渲染层动画、transition、Animation 句柄全套、@loom-hook、
  :nth-child）→ **M2 全做**，不留 defer。M2 交付的是功能完整的动画系统。
- **引擎内部终态**（池化 / 缓动全集 / layout 动画 prop_type 分层）→ **M2.5 明确立项**（§12），
  触发判据明确，不再悬置。

## 2. 现状核实（事实，非推断）

### 2.1 fence 解析器（已就绪，有缺口）

| 项 | 状态 | 证据 |
|---|---|---|
| `@keyframes` at-rule 解析 | **已做** | `crates/fence/src/css_rules.rs:331` `parse_keyframes_rule`，产 fence-local `KeyframesRule{name, stops}`，挂 `ParsedTemplate.keyframes`（pipeline.rs:23）。支持 `from`/`to`/`N%` + 逗号多 stop |
| `animation` 简写语法校验 | **只校验不存值** | `crates/fence/src/schema/css.rs:595` `validate_animation_value`；`css_resolve.rs:148` `continue` 跳过——合法值运行时静默不跑 |
| `transition` 属性 | **零校验** | `CssValueParser::Transition`（css.rs:37）全 crate 无对应 arm，落 `_ => {}` |
| animation 长划子属性 | **无** | 只有简写，无 `animation-name`/`duration`/... |
| 缓动 | **无结构化数据** | 仅 `is_animation_keyword`（css.rs:660）白名单识别 7 种关键字，无 EasingFunction enum |
| per-stop `animation-timing-function` | **不支持** | stop 声明走通用 parse，animation 系属性 apply_decl 不识别 |
| `@loom-hook` | **未解析** | 全 fence 无命中，仅 docs 提及 |

### 2.2 core tween 引擎（v1 算法在，能力受限）

| 项 | 状态 | 证据 |
|---|---|---|
| `TweenManager` | 单 `Vec<Tween>`（无池化） | `crates/core/src/tween.rs:181`。`update(dt, scene, out)` 推进 + emit `EVT_TWEEN_COMPLETE` |
| `Tween` 字段 | start/end `[f32;4]` + `Ease`(10 种解析式) + delay/duration | tween.rs:165 |
| 写入目标 | `NodeAnim{opacity, transform:Affine2, bg_color, text_color}` —— **全是渲染层 override** | scene/node.rs:298 |
| transition 引擎 | **core 侧已做实** | `emit_transition_requests`（dynamic.rs:807）rematch 检测 opacity/bg_color/text_color 变化 → 发 `TransitionRequest` → tick drain kill 旧 tween + 提新。有测试（dynamic.rs:1804+）。**不支持 transform transition**（dynamic.rs:864 注释：分解矩阵复杂） |
| prop_type 分层 | **完全没有** | `NodeAnim` 无 layout 通道；solve 从不读 anim |
| tick 时序 | tween.update 在最前；anim 只被 compute_world_transforms / build 消费 | stage.rs:890 `tick_and_render` |

**关键结论**：core tween 只能驱动渲染层（transform/opacity/color），动不了 layout 属性。
transition 引擎已就绪，缺的只是 fence 配置源。keyframes runtime 是**全新机制**（Tween 是
fire-and-forget，无句柄控制、无时间轴语义），不能直接复用 Tween。

### 2.3 设计契约

- **public-api §9**：全 CSS 定义、无命令式 tween。三种触发（class 切换 / `node.Play` / SetVar）。
  Animation 句柄全套（Play/Pause/Resume/Stop/Time/IsPlaying + OnStart/OnEnd/OnKey/OnHook）。
  `@loom-hook` 注释锚点。时序不变量：class/style 变更下帧 rematch 生效；transition 基线 = 上帧 computed。
- **main-design §13.1**：单一动画时钟 `TweenManager::update(dt)`。终态池化 `{active,pool}` +
  `TweenValue{x,y,z,w,d}` value_size(1..6) + 链式 builder + 28+ 缓动 + **prop_type 分层**
  （transform_dirty vs layout_dirty）。→ 这些终态归 M2.5。
- **main-design §16 tick 时序**：ScrollPane 自维护 tween 是单一时钟的既定例外（自维护但 dt 驱动）。
  KeyframePlayer 同模式（受 tick dt 驱动，不破坏单一时钟）。

## 3. 设计决策（grill 锁定）

### 3.1 路线甲：独立 KeyframePlayer（不翻译成 Tween 序列）

keyframes runtime 两种实现路线：
- **甲（选定）：独立时间轴播放器**。新增 `KeyframePlayer`，持有 keyframes 引用 + current_time +
  play_state + iteration + direction + 回调。每帧 `player.update(dt)` 推进时间轴、在 stops 间插值、
  直接写 `NodeAnim`。Animation 句柄即 player。
- 乙（否决）：keyframes 翻译成 Tween 序列。一个 animation = N×M 个 Tween（N 段 stop × M 属性），
  靠 TweenManager 管。

选甲的理由：(1) Animation 句柄的控制语义（Pause/Resume/Time seek/OnKey）+ keyframes 时间轴语义
（iteration/direction/fill-mode）天然是"播放器"模型，硬塞 fire-and-forget Tween（乙）会拧巴；
(2) 乙的 tween 数量爆炸（5-stop×4-property = 20 tween/animation，home 7 卡 = 140 tween，单 Vec 抖）；
(3) 甲与 main-design 单一时钟兼容（player 由 tick dt 驱动，同 ScrollPane 先例）；(4) M2.5 统一有
明确路径（player 插值原语与 Tween 抽出共享 `TweenValue`，不推翻甲）。

### 3.2 A1：只动渲染层（layout 动画 defer M2.5）

keyframes 可动属性 = `transform`(translate/scale/rotate) + `opacity` + `bg-color` + `text-color`。
**不动 layout 属性**（width/height/flex/position offset）——那需要 prop_type 分层 + layout_dirty +
solve 重入，是 main-design §13.1 明示"推 M5/M2.5"的终态。理由：home 演示场景（fadeIn）就是渲染层；
渲染层动画覆盖游戏 UI 80% 需求（淡入淡出/滑动/缩放/脉冲）；layout 动画（accordion 展开）当前无
showcase 刚需，等真出现再上（M2.5）。

### 3.3 opacity 父级累积传播（顺手做掉）

core `render/mod.rs:1640` 每节点只读自己的 opacity（`anim.and_then(|a| a.opacity).unwrap_or(style.opacity)`），
**子不继承父**——和标准 CSS 不同（CSS 父 opacity 隔离 layer，子整体乘父 alpha）。是 pre-existing 行为。
M2 顺手修正：render DFS 传 `parent_alpha`，`alpha = parent_alpha * own`，存累积值进 RenderNode.alpha，
后端零改。换符合 web 标准的 opacity 语义，"容器整体淡入"能自然写。代价小（几行）。

### 3.4 Animation 句柄 L3 全套（不 defer）

public-api §9.2 全套（Play/Pause/Resume/Stop/Time/IsPlaying + OnStart/OnEnd/OnKey/OnHook）全进 M2。
不 defer 的理由：这是 API 承诺，defer = 半成品；且 L3 在路线甲下成本不高——Pause/Resume/Time seek/
OnKey 都是 player 状态的基本操作，时间轴模型天然支持。`@loom-hook` 是 fence 多解析一个注释锚点。
（grill 初版曾低估 L3 可行性，误推 L2，已修正。）

### 3.5 keyframes 表组件级打包 + Scene 级查找

pkg 按 `ComponentTemplate` 存 keyframes（每组件自己的 `<style>` 提取）；runtime instantiate 时
合并进 Scene 级 `HashMap<String, KeyframesRule>`（名字全局查找，符合 CSS `@keyframes` 全局语义）。
同名后实例化覆盖（实际不冲突，作者按页面组织）。

### 3.6 transform 在 keyframes 里按 TRS 分解（非合并矩阵）

围栏 transform 子集只有 `translate/rotate/scale`（mapping.rs:490-512 `func_to_matrix` 核实，无 skew/matrix），
**1:1 分解无信息丢失**。静态 transform → Affine2 矩阵（现状不变）；keyframes stop 的 transform →
`TransformAnim{translate,scale,rotate}` 三分量存储，每帧分量级 lerp 合成矩阵。不做 CSS 矩阵插值
（matrix decomposition 复杂且有旋转/缩放混叠歧义）。

## 4. 架构：数据流 + 数据结构

### 4.1 端到端数据流

```
HTML @keyframes + animation 属性
  → fence 解析（KeyframesRule 挂 ParsedTemplate；animation 简写解析存值）
  → packer bridge 不再丢弃 → pkg.bin (v30)
  → core instantiate（组件 keyframes 合并进 Scene 全局表；base_style.animation 进节点）
  → cascade rematch（animation 暴露到 computed；class 变化检测）
  → KeyframePlayer 启动（class 触发 / node.Play FFI）
  → tick：player.update(dt) 推进时间轴 → 写 NodeAnim
  → compute_world_transforms + render 消费 NodeAnim（现有路径零改）
```

### 4.2 数据结构（core 定义，pkg 序列化 + runtime 共用）

```rust
// ── pkg 层：keyframes 表（ComponentTemplate.keyframes）──
pub struct KeyframesRule {
    pub name: String,
    pub stops: Vec<KeyframeStop>,
}
pub struct KeyframeStop {
    pub selector: KeyframeStopSelector,   // 复用 fence 现有 From|To|Percent(u8)
    pub props: AnimatableProps,           // 该 stop 声明的可动画属性值
    pub hook: Option<String>,             // /* @loom-hook name */ 锚点（§8.4）
}
pub struct AnimatableProps {
    pub opacity: Option<f32>,
    pub transform: Option<TransformAnim>,
    pub bg_color: Option<[f32; 4]>,
    pub text_color: Option<[f32; 4]>,
}
pub struct TransformAnim {                // TRS 分解（围栏子集 1:1）
    pub translate: Option<[f32; 2]>,
    pub scale: Option<[f32; 2]>,
    pub rotate: Option<f32>,              // radians
}

// ── ResolvedStyle 新增字段（base_style bake，和 transition 并列）──
pub struct ResolvedStyle {
    // ... 现有字段
    pub transition: Vec<TransitionSpec>,  // 已有
    pub animation: Vec<AnimationSpec>,    // 【新】逗号分隔多声明
}
pub struct AnimationSpec {
    pub name: String,
    pub duration: f32,
    pub delay: f32,
    pub iteration_count: Option<u32>,     // None = infinite
    pub direction: AnimationDirection,    // Normal|Reverse|Alternate|AlternateReverse
    pub fill_mode: AnimationFillMode,     // None|Forwards|Backwards|Both
    pub timing_function: Ease,            // 复用 tween::Ease（+ 新增 Step 变体）
    pub play_state: AnimationPlayState,   // Running|Paused
}

// ── core runtime：KeyframePlayer（Scene 级存活集合）──
pub(crate) struct KeyframePlayer {
    pub node: NodeId,
    pub spec: AnimationSpec,
    pub keyframes: KeyframesRule,         // 值拷贝（keyframes 表小，避免生命周期）
    pub current_time: f32,
    pub elapsed: f32,                     // 含 delay 计时
    pub iteration: u32,
    pub play_state: PlayerPlayState,      // Playing|Paused|Completed|Stopped
    pub on_key_percents: Vec<f32>,        // FFI 注册的 OnKey 阈值
    pub fired_keys: Vec<f32>,             // 已触发的（防同 iteration 重复）
    pub fired_start: bool,
}
```

**TemplateNode 不加字段**——节点靠 `base_style.animation[i].name` 引用 keyframes 表。

### 4.3 Scene 级存储

```rust
pub struct Scene {
    // ... 现有
    pub keyframes: HashMap<String, KeyframesRule>,   //【新】全局 @keyframes 查找表
    pub players: SlotMap<PlayerKey, KeyframePlayer>, //【新】活跃 player（slotmap 稳定 Key = Animation 句柄）
}
```
slotmap 与 `scene.nodes` 同模式（slotmap 1.1 已是项目依赖）。PlayerKey(u64) 稳定 → C# Animation 句柄。
M2.5 池化时再优化。

### 4.4 pkg 版本

v29 → **v30**。`ComponentTemplate` 加 `keyframes` 字段 + `ResolvedStyle` 加 `animation` 字段 +
`KeyframeStop.hook` → bincode 布局变。一刀切升 v30，`MIN=MAX=30`（沿用 P0 一刀切惯例）。
更新 bincode 稳定性测试。

## 5. tick 时序 + player 生命周期

### 5.1 tick 时序落点（现有 `tick_and_render` 加两处）

```
b.  TweenManager.update(dt) + KeyframePlayer.update(dt)   ←【新】player 推进时间轴写 NodeAnim
c.  process (hit)             d. scroll.advance   e. list virtualize
f.  rematch_pseudo_classes
g.  transition drain
g'. sync_animation_players                                 ←【新】检测 animation 声明变化，启停 player
h.  sync_control_visuals   i. solve   j. refresh_content_sizes
k.  compute_world_transforms                               ← 读 anim.transform（player 写的，零改）
l.  build (render)                                         ← 读 anim.opacity/bg_color/text_color（零改）
```

- `player.update` 在 **b**（和 tween 并列，单一时钟语义；新启动的 player 下帧才推进，接受 16ms 首帧延迟）。
- `sync_animation_players` 在 **g'**（rematch 后、solve 前；class 变化带来的 animation 声明增删在这里启停）。

### 5.2 player 启动触发

| 触发 | 何时 | 机制 |
|---|---|---|
| class 声明式（`.nav-card{animation:fadeIn}`）| g' | 比较 computed `animation` 声明 vs 节点活跃 player 的 spec：新增 name → 启 player；消失 → 停；参数变 → 重启 |
| `node.Play("name")`（程序化）| FFI 立即 | 不等 rematch，直接建 player，返 PlayerKey 给 C# |

**首帧 backwards fill 边角**：`fill-mode: backwards/both` 在 delay 期间应显示首帧值。启动时
（sync_animation_players 或 Play FFI）立即算一次首帧值写 NodeAnim，不等下帧 b update——避免延迟期闪 base。

### 5.3 时间轴推进逻辑（`player.update(dt)`）

```
1. 若 play_state==Paused → 跳过推进
2. elapsed += dt
3. 若 elapsed < delay → backwards fill 首帧值（fill backwards/both），return
4. anim_time = elapsed - delay          // 动画内时间（扣除 delay）
   iteration  = anim_time / duration      // 整数除，0-based
   current_time = anim_time % duration
   progress   = current_time / duration
5. 完成判定：iteration_count==Some(n) 且 iteration ≥ n → 标记本帧完成（仍应用末值）
6. direction 应用到 progress（用步骤 4 的当前 iteration，非旧值）：
     normal: progress; reverse: 1-progress;
     alternate: iteration 偶数取 progress、奇数取 1-progress;
     alternate-reverse: 反之
7. 在 keyframes stops 间定位 progress 落点，per-property 分量级 lerp + per-segment timing_function
8. 写 NodeAnim 四通道
9. OnKey 检测：last_progress→progress 跨越 on_key_percents 某 pct → emit EVT_ANIMATION_KEY
10. OnHook 检测：跨越某 stop.hook 的百分比 → emit EVT_ANIMATION_HOOK
11. 完成（步骤 5 标记）：emit EVT_ANIMATION_END（+ ITERATION 若 iteration 边界跨越）；
    fill forwards/both → Completed 态保留末值；none/backwards → 回收回退 base
```

时间模型唯一源头是 `elapsed`（wall clock），`iteration`/`current_time`/`progress` 全从它推导，
避免 direction 用到尚未更新的旧 iteration（grill 自审修正点）。

### 5.4 player 生命周期状态

- **Playing**（推进时间轴）/ **Paused**（暂停）/ **Completed**（完成，fill forwards/both 持续写末值，
  不回收，直到声明消失/Stop）/ **Stopped**（显式 Stop，回收）。
- class 触发的 Completed player：`sync_animation_players` 检测 animation 声明消失才回收。
- `node.Play` 的 player：不绑 class，靠句柄 `Stop()` 或完成（按 fill-mode）回收。

## 6. 优先级与冲突

核心原则：**NodeAnim 每帧由写入者按优先级顺序覆盖**。compute_world_transforms / build 读 NodeAnim 时
`anim.unwrap_or(style)` 天然优先于 base style（render/mod.rs:1640 已验证）。

### 6.1 写入顺序 = 优先级（每帧 step b 内）

```
1. TweenManager.apply  先写（transition 的 opacity/bg_color/text_color）
2. KeyframePlayer.update 后写（animation 的 transform/opacity/bg_color/text_color，同通道覆盖）
```
→ 天然实现 CSS **animation 优先于 transition**，无需占用标记。

### 6.2 通道独占

- `transform`：**player 独占**（transition 不支持 transform，dynamic.rs:864）。无冲突。
- `opacity/bg_color/text_color`：player 与 tween 都可能写，player 后写覆盖。

### 6.3 fill-mode 与完成态

| fill-mode | player 完成后 | NodeAnim 该通道 |
|---|---|---|
| `forwards` / `both` | Completed 态，不回收，每帧持续写末值 | 保留末值（直到声明消失/Stop）|
| `none` / `backwards`（默认）| 回收 player | 通道回 None → 下帧 tween/base 接管 |

home `animation:fadeIn .4s both` → Completed 后保留末值（opacity:1, translateY:0），元素停终态。

### 6.4 多 animation 并存

`animation: fadeIn .4s, spin 2s` → 一个节点 N 个 player。同通道冲突时 **后声明的赢**（CSS 标准，
列表后者优先）—— player.update 按 `animation: Vec` 顺序写，后者覆盖前者。

### 6.5 transition 与 animation 检测独立

transition 在 rematch(f) 检测 **computed style 变化**（base_style + cascade），不含 NodeAnim
（NodeAnim 是渲染 override，不进 cascade）。animation 播放期间 computed style 不变 → transition
不误触发。两者各跑各的，只在"写 NodeAnim"这步由写入顺序决定谁可见。

## 7. Animation 句柄 C# 投影 + 事件层

### 7.1 事件流（core emit → C# 双路由）

```
core player.update 检测 START/END/ITERATION/KEY/HOOK 阈值
  → emit EventRecord{type, node_id, player_key, payload}
  → borrow_events → C# LoomHost demux
       ├─ 全局路由：node.EventBus.On<AnimationXxxEvent>（class 触发也能订阅）
       └─ 句柄路由：按 player_key 找 Animation 实例 → 触发私有回调
```

### 7.2 Animation sealed class

```csharp
public sealed class Animation {
    internal ulong playerKey;  // core PlayerKey（slotmap 稳定句柄，u64）
    internal Node node;
    internal List<Action> onStart, onEnd;
    internal List<(float pct, Action)> onKeys;
    internal List<(string name, Action)> onHooks;

    public string Name { get; }
    public bool IsPlaying => FFI.get_animation_state(playerKey) == Playing;
    public float Time { get => FFI.get; set => FFI.set; }   // seek
    public void Pause(); public void Resume(); public void Stop();
    public Animation OnStart(Action cb);   // 链式
    public Animation OnEnd(Action cb);
    public Animation OnKey(float pct, Action cb);
    public Animation OnHook(string name, Action cb);
}
```

### 7.3 新增 FFI

| FFI | 作用 |
|---|---|
| `play_animation(stage, node, name) -> u64` | 创建 player，返 PlayerKey（class 触发由 cascade 自动建，不走此 FFI）|
| `pause/resume/stop_animation(stage, key:u64)` | 句柄控制 |
| `get/set_animation_time(stage, key:u64) -> f32` | Time 属性 + seek |
| `get_animation_state(stage, key:u64) -> u8` | IsPlaying（Playing/Paused/Completed）|
| `animation_on_key(stage, key:u64, pct:f32)` | **注册 OnKey 百分比到 core**（core 要知道检测哪些阈值）|

### 7.4 回调注册机制（哪些走 FFI、哪些纯 C#）

| 回调 | 机制 |
|---|---|
| `OnStart/OnEnd` | 纯 C#：core 本就 emit START/END 事件，C# 按 playerKey 找句柄触发，不需 FFI 注册 |
| `OnKey(pct, cb)` | 半 FFI：cb 留 C#，pct FFI 注册到 core（core 才知道检测哪些百分比跨越）|
| `OnHook(name, cb)` | 纯 C#：hook 锚点百分比是 keyframes 自带（@loom-hook 解析进 `KeyframeStop.hook`），core emit HOOK 带name，C# 按 name 匹配触发，不需 FFI 注册 |

### 7.5 新增事件 struct（补齐 public-api §1 动画事件）

| 事件 | 触发 | 字段 |
|---|---|---|
| `AnimationStartEvent` | player 首次 update | Target, Name |
| `AnimationEndEvent` | player 完成（class + Play 都发）| Target, Name |
| `AnimationIterationEvent` | 每个 iteration 结束 | Target, Name, Iteration |
| `AnimationKeyEvent` | OnKey 跨越（句柄私有，不广播 EventBus）| Target, Name, Percent |
| `AnimationHookEvent` | @loom-hook 跨越（句柄私有）| Target, Name, HookName |
| `TransitionEndEvent` | transition tween 完成 | Target, Prop |

> M2 接齐 AnimationStart/Iteration/TransitionEnd 的 core 事件源（之前 deferred 的 source-less stub，
> roadmap §4 tech-debt 列了）。

### 7.6 生命周期不变量

player 回收（Completed+fill none / Stop / 声明消失）→ playerKey 失效 → Animation 句柄标记 disposed →
后续调用 no-op。循环动画（infinite）句柄存活到 Stop。class 触发的动画无句柄，但 START/END/ITERATION
仍 emit → 走全局 EventBus → `node.On<AnimationEndEvent>()` 订阅（如退场动画结束 Dispose，public-api §1 示例）。

## 8. fence 解析 + pkg + bridge

| 项 | fence 现状 | M2 做 | 产出 |
|---|---|---|---|
| **A. @keyframes 进 pkg** | fence 已产 `KeyframesRule`，bridge 静默丢 | bridge 翻译 fence→core `KeyframesRule`，进 `ComponentTemplate.keyframes` | pkg 带动画表 |
| **B. animation 简写存值** | 只校验不存 | 解析简写 → `Vec<AnimationSpec>` bake 进 `base_style.animation` | ResolvedStyle.animation 有值 |
| **C. transition 简写存值** | 零校验 | 解析简写 → `Vec<TransitionSpec>` bake 进 `base_style.transition`（core 引擎已在，补配置源）| transition 真生效 |
| **D. @loom-hook 锚点** | 未解析 | fence 解析 stop 内 `/* @loom-hook name */` → `KeyframeStop.hook` | player 可触发 OnHook |
| **E. :nth-child(N)** | selector 子集不含 | fence parser 加 `:nth-child(An+B\|odd\|even\|N)` + specificity；core matcher 加分支（查节点在父 children 的 1-based index）| home 错峰可匹配 |

### 8.1 transform 解析分叉

静态 `transform` 走 `parse_transform`（产 Affine2 矩阵，现状）；keyframes stop 内的 `transform` 走新
`parse_transform_trs`（产 `TransformAnim{translate,scale,rotate}` 三分量，供 stop 间分量级 lerp）。
围栏 transform 子集只有 TRS（§3.6），1:1 分解。

### 8.2 animation-delay 区分（修 tech-debt"粗糙处理"）

简写解析时 **第一个 time = duration，第二个 time = delay**（CSS 语义），不再都当匿名 time。

### 8.3 ease 对齐（fence 7 keyword → core Ease）

| fence keyword | core Ease |
|---|---|
| `linear` | `Linear` |
| `ease`（CSS 默认）| `CubicOut`（近似 cubic-bezier(.25,.1,.25,1)）|
| `ease-in/out/in-out` | `Quad` In/Out/InOut |
| `step-start/step-end` | **M2 新增** `Step{start:bool}` 变体（阶跃函数，几行）|

cubic-bezier/Elastic/Bounce 推 M2.5（结构化 EasingFunction 一起）。home fadeIn 默认 ease → CubicOut。

### 8.4 @loom-hook 解析

fence 解析 keyframes stop 内 `/* @loom-hook name */` 注释 → 产 hook 标记挂在 `KeyframeStop.hook: Option<String>`。
bridge 翻译进 core。player 从 keyframes 读 hook（percent = stop 的 selector 命中值，name = hook）。

### 8.5 :nth-child selector

fence parser 加 `:nth-child(An+B | odd | even | N)` 变体（`odd`=`2n+1`、`even`=`2n`）+ specificity。
core matcher（`compound_matches_node`）加 :nth-child 分支：查节点在父 children 的 1-based index，
匹配 `(index - B) % A == 0 && (index-B)/A >= 0`。home 用 `:nth-child(1)..(7)` 纯整数；顺手支持 An+B。

### 8.6 长划子属性 defer

M2 只做 `animation`/`transition` **简写**。长划子属性（`animation-name`/`duration`/... 8 个）推后——
CSS 长划是简写语法糖，作者实际写简写，ROI 低。tech-debt 写进 roadmap。

## 9. 测试 + 验收门 + 两台机策略

### 9.1 测试矩阵（三层）

| 层 | 测什么 | 例子 |
|---|---|---|
| **core 单测**（机制级）| player 时间轴各路径 | delay 跳过 / iteration 累积 / direction(normal/reverse/alternate) / fill-mode(forwards 保留·none 回退) |
| | TRS 分量级 lerp | translate/scale/rotate 各分量独立插值 |
| | ease 求值 | linear/CubicOut/Quad×3/Step 各映射 |
| | :nth-child matcher | 整数/odd/even/An+B + specificity |
| | 解析 | animation 简写各子属性 + delay 区分 + 多声明逗号；transition 简写；@loom-hook 锚点 |
| | 优先级 | animation vs transition 写入顺序；多 animation 后者赢 |
| **core 集成测**（headless 端到端）| HTML @keyframes+animation → pkg → tick → 断言 NodeAnim | fadeIn(opacity 0→1,.4s) 在 t=0/0.2/0.4 取值 |
| | class 触发 | 加 class 带 animation → 下帧 player 启 → AnimationEndEvent emit |
| | transition 触发 | 改 class 改 opacity → tween 发 → 值变化 |
| | :nth-child 错峰 | N 个 nth-child 不同 delay → 各 player current_time 错峰 |
| | node.Play 句柄 | FFI 建 player → Pause/Resume/Time seek/Stop → 断言状态 |
| | OnKey/OnHook | 注册 → 推进越阈值 → emit 事件 |
| | opacity 父级累积 | 父 opacity=0.5 → 子 RenderNode.alpha = 0.5×own |
| **headless C# 测**（Spec-4a harness 复用）| Animation 句柄 API | Play/Pause/Resume/Stop/Time + On<AnimationEndEvent> + OnKey/OnHook |

### 9.2 确定性断言策略

用固定 `dt` 推进 tick，断言 NodeAnim 在确定性时刻的值，不依赖真时钟。例：`fadeIn .4s both` →
`tick(dt=0)` opacity≈0（backwards 首帧）/ `tick(dt=0.2)` opacity=CubicOut(0.5)≈0.82 /
`tick(dt=0.4)` opacity=1（forwards 保留）。无 flaky。

### 9.3 集成测重点（跨层缺口 per-task review 必漏）

M2 新增"player 写 NodeAnim"——强制 grep **所有读 NodeAnim 的点**（`compute_world_transforms`
读 anim.transform / `render` 读 anim.opacity·bg_color·text_color）确认消费正确；新增"animation 是
cascade 属性"——确认 rematch/inherit 路径不漏（animation 不继承，但要进 computed style）。

### 9.4 两台机策略（M1/M3 既定模式）

- **编码机**：core 单测 + 集成测 + headless C# 测 + fmt/clippy + PublicApi 编译门 + dll 重编 +
  binding sync + pkg v30 重打。**逻辑全锁**。
- **家里机（defer）**：showcase `home.html` 入场动画真机视觉验收（7 nav-card fadeIn 错峰）+
  M2 动画验收页（§10），和 M0 一起排队。

### 9.5 M2 验收门（编码端 DONE 判据）

- [ ] `cargo test` 全 workspace 绿（core 单测 + 集成测新增 ~30+）
- [ ] `dotnet test` HeadlessTests 绿（Animation 句柄 C# 测）
- [ ] PublicApi 编译门绿（新增 Animation 类 / 事件 struct 签名冻结）
- [ ] `cargo fmt --check` + `cargo clippy -D warnings` 清
- [ ] dll 重编 + `xtap sync-bindings` + pkg v30 重打入库
- [ ] bincode 稳定性测试更新（v30 形状锁）
- [ ] fence schema↔doc 交叉校验绿（`cargo test -p loomgui_fence`，含 animation/transition/@loom-hook 文档同步）
- [ ] M2 动画验收页 pkg 打包 exit 0

## 10. M2 动画验收 showcase 矩阵

专项验收页覆盖 M2 全部能力，每块配 headless 确定性断言 + 真机可视。落点：
- **headless fixture**：`tests/dotnet/LoomGUI.HeadlessTests/fixtures/animation.workspace/`（沿用既有 fixture 模式）
- **真机验收页**：`showcase/` 下新增动画 demo 页 + `home.html`（nth-child/keyframes 解注释后，主载体）

| # | 动画 | CSS 要点 | 覆盖维度 | headless 断言（固定 dt） |
|---|---|---|---|---|
| 1 | 淡入滑入 | `fadeIn: opacity0→1 + translateY(20→0), .5s both` | opacity+translate + fill both + class 触发 | t=0 opacity≈0/Y=20；t=.25 中值；t=.5 =1/Y=0 保留 |
| 2 | 脉冲缩放 | `pulse: scale(1↔1.1), infinite alternate` | scale 分量 + 3-stop + infinite + alternate | t=0 scale=1；t=半周期 scale=1.1；不结束 |
| 3 | 旋转 | `spin: rotate(0→360), infinite linear` | rotate 分量 + linear + infinite | t=0 rot=0；t=半周期 rot=180° |
| 4 | 颜色渐变 | `hue: bg-color A→B→A, infinite` | bg-color 通道 + 3-stop color lerp | t=0 color=A；t=1/4 color=B |
| 5 | 错峰入场 | `.item:nth-child(N){animation:slide .4s (N×.1)s both}` | :nth-child + delay 区分 + 多元素 | N player 各 delay 错峰，current_time 错峰 |
| 6 | fill-mode 对比 | 4 个一次性：none/forwards/backwards/both | fill 四态 | 完成后 none 回 base；forwards/both 保留；backwards delay 期显首帧 |
| 7 | direction 对比 | normal / reverse / alternate | direction 三态 | reverse 从末值起；alternate 偶正奇反 |
| 8 | ease 对比 | 同动画 × linear/ease/step-start | ease 子集 | 同 progress 不同值（linear 匀 / step 阶跃）|
| 9 | transition | `.btn{transition:background-color .3s}` + class 切 | transition + class 变化平滑 | 改 class→tween 发→.15s 中值 |
| 10 | 父级 opacity 累积 | 父 fadeIn opacity 0→1，子文字 | opacity 父级传播 | 子 RenderNode.alpha = 父opacity × own |
| 11 | node.Play + OnKey/OnHook | 程序化 Play("progress") + OnKey(.5) + `@loom-hook half` | node.Play + OnKey + OnHook | Play 建 player；OnKey .5 触发；hook 触发 |
| 12 | 句柄 L3 控制 | Play + Pause/Resume/Stop/Time seek 按钮 | 句柄全套 | Pause 不推进；seek 跳值；Stop 回收 |

**分组**：#1-10 纯 HTML/CSS → core 集成测 headless 断言 NodeAnim（不依赖 driver）；#11-12 需 driver
C# 脚本（Play/句柄操作）→ headless C# harness 测，真机视觉 defer 家里机。这页同时是 M2 真机验收门
（家里机逐块看动画）+ 将来动画能力的活文档。

## 11. 风险

- **首帧 backwards fill 边角**：启动到首帧 update 之间若不立即写首帧值，backwards/both 会闪 base。
  缓解：启动时立即算一次首帧值写 NodeAnim（§5.2）。
- **tween 与 player 写入顺序**：若未来重构 step b 内部顺序，要保证 player 在 tween 之后（animation
  优先）。加单测锁写入顺序。
- **infinite animation 不回收**：Completed/playing infinite player 持续占用 slotmap 槽 + 每帧 update。
  sync_animation_players 检测声明消失才回收；node.Play infinite 靠 Stop。slotmap 槽复用，无泄漏。
- **pkg v30 bincode 形状**：升 v30 改 bincode 布局，更新稳定性测试，避免运行时 BadKind（P0 教训）。
- **跨层 dispatch 漏 arm**（AGENTS.md 教训）：player 写 NodeAnim 后，强制 grep 所有读 NodeAnim 的点
  确认消费（§9.3）。

## 12. M2.5 立项（动画引擎终态，明确归宿）

M2 交付功能完整但引擎层用"够 keyframes 跑的实现"。以下归 M2.5，触发判据明确，不再悬置：

- **池化 Tween**（`TweenManager { active, pool }`，替换单 Vec）。
- **缓动全集**：cubic-bezier / Elastic / Bounce / Custom + per-stop timing-function（结构化 `EasingFunction`）。
- **链式 builder API**（`.tween().delay().ease().repeat(,yoyo).on_complete()`，替换位置参数 `tween()`）。
- **layout 动画 / prop_type 分层**（动 width/height/flex，tick 时序重构 + `layout_dirty` + solve 重入）。
- **player 与 Tween 插值原语统一**（共享 `TweenValue{x,y,z,w,d}` + value_size(1..6)）。

**进入判据**（任一满足即启动 M2.5）：
1. 第一个需要 layout 动画的 showcase 页面出现（如 character 技能面板 accordion 展开 / 用 width 而非
   scaleX 的进度条）。
2. M5 视觉束的 `NodeTransform` 替代 Affine2 升级时合并做（都动 render 数据结构，合做省一次 pkg bump）。
3. 动画实例并发量使单 Vec TweenManager 出现性能抖动（profiling 实证）。

**roadmap 同步**：本 spec 落地后，在 `docs/roadmap/milestones.md` 总览表加 M2.5 行（依赖 M2、阻塞 M6
视觉动效深度、估时 2-3 周），并在 §4 tech-debt 把"动画系统终态"从"悬置"改为"M2.5 明确立项"。

## 13. 落点清单（实现时按图索骥）

- `crates/core/src/`：新增 `KeyframePlayer`（建议 `scene/animation.rs` 或 `tween.rs` 同目录）；
  `Scene` 加 `keyframes` + `players` 字段；`ResolvedStyle` 加 `animation` 字段；`KeyframesRule`/
  `KeyframeStop`/`AnimatableProps`/`TransformAnim`/`AnimationSpec` 等类型；tick 加 b/g' 两处。
- `crates/core/src/render/mod.rs`：opacity 父级累积（DFS 传 parent_alpha）；读 NodeAnim 处确认消费 player 写入。
- `crates/core/src/style/dynamic.rs`：`sync_animation_players`（rematch 后）；transition 配置源已就绪。
- `crates/core/src/asset/mod.rs`：`PKG_FORMAT_VERSION = 30`；`ComponentTemplate` 加 keyframes；序列化形状锁测试。
- `crates/fence/src/`：css_resolve 存 animation/transition 值；@loom-hook 解析；:nth-child selector；schema 加 animation/transition。
- `crates/packer/`：bridge 翻译 keyframes 进 ComponentTemplate；transform TRS 分解解析。
- `crates/ffi_c/`：新增 play/pause/resume/stop/get-set-time/get-state/on-key FFI；csbindgen 生成 + sync-bindings。
- `unity/package/Runtime/Public/`：`Animation` sealed class；AnimationStart/End/Iteration/Key/Hook + TransitionEnd event struct；Node.Play。
- `unity/package/Plugins/LoomGUI/`：dll 重编入库。
- `tests/dotnet/LoomGUI.HeadlessTests/`：`fixtures/animation.workspace/` + Animation 句柄测。
- `showcase/`：home.html nth-child/keyframes 解注释；M2 动画验收 demo 页。
- `docs/`：main-design §13（动画）+ public-api §9（动画）对齐实现；fence.md（@keyframes/@loom-hook/:nth-child）；roadmap/milestones M2.5 立项 + tech-debt 更新。
