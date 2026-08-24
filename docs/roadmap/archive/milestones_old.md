# LoomGUI 里程碑 Backlog（归档：摸黑后 → 可演示 纪元执行计划）

> **⚠️ 本文件已归档**（2026-08-11）。这是**旧 roadmap（现 `roadmap_old.md`）§4「三束加宽」的可执行切片**——M0–M6 + M∞ 的进入/退出判据 / 依赖图 / 周节奏表。该纪元已完工（M1–M3 编码端 DONE、视觉束大半交付），向前里程碑规划见新 `docs/roadmap/roadmap.md`，defer 项见 `docs/roadmap/deferred.md`。
>
> 下方内容 stale（如 M1 标「❌ 未开」实际已完工），保留作历史决策记录。**M2.5 / M5 作为 spec 术语**仍被 `deferred.md` 和各 spec 引用，定义见本文件对应小节。
>
> 排序原则见 `roadmap.md` §3 + 上一轮 grill 结论：先关已开的锅 → 趁新鲜啃最大风险 → 中间插爽点保持士气 → 低 ROI 狠砍。

---

## 依赖图总览

```
        ┌──► M1 ListView ──┬──► M4 文本模型 ──┐
M0 验收 ─┤                  │                  ├──► M6 showcase 收口 ──► M∞ 解耦项
        ├──► M2 keyframes ──┼──► M2.5 动画引擎终态 ──┤
        │                  └──► M5 视觉精简 ──┤
        └──► M3 TabList ──────────────────────┘
                                               └─（M1–M5 + M2.5 齐备后收口）
```

- **M1/M2/M3 互不依赖**，M0 后可并行或任选顺序（下方按推荐节奏排）。
- **M2.5**（动画引擎终态）依赖 M2，阻塞 M6（视觉动效深度）。（NodeTransform 升级是 M2.5 的进入判据之一 #2，M5 仍自有 NodeTransform 退出判据，非被 M2.5 阻塞。）
- **关键路径**（到可演示的最长链）= `M0 → M1 → M4 → M6`，约 6–8 周；M2/M3/M5/M2.5 是侧支，喂进 M6。
- M∞ 与 runtime 解耦，另条命，不计入。

---

## 总览表

| ID | 名称 | 依赖 | 阻塞 | 估时 | 状态 |
|----|------|------|------|------|------|
| M0 | P3 家里机验收 + IME 接线 | — | M1/M2/M3 进入 | 几天 | ⏳ 编码端 DONE，验收 defer |
| M1 | ListView 虚拟化 | M0 | M4, M6 | 2–3 周 | ❌ 未开 |
| M2 | @keyframes runtime + transition | M0 | M2.5, M5, M6 | 1–2 周 | ⏳ 编码端 DONE，Unity 验收 defer |
| M3 | TabList（P4）| M0 | M6 | ~1 周 | ⏳ 编码端 DONE，Unity 验收 defer |
| M4 | 文本模型回归标准子树 | M1 | M6 | 2–3 周 | ❌ 未开 |
| M5 | 视觉束（精简版）| M2（部分）| M6 | ~2 周 | ❌ 未开（scope 已砍）|
| M2.5 | 动画引擎终态（池化/缓动全集/layout 动画）| M2 | M6 | 2–3 周 | ❌ 未开（触发判据见下）|
| M6 | showcase 收口 + tech-debt 扫除 | M1–M5 + M2.5 | 可对外演示 | 1–2 周 | ❌ |
| M∞ | Custom Element / 平台移植 / 编辑器闭环 | — | — | 不计入 | 按需 |

---

## M0 · P3 家里机验收 + IME 接线

> 关掉已开的锅：编码端写完没验的代码堆积是单人项目最大隐性风险。先把控件束 officially 关掉。

- **进入判据**：家里机可用。pkg v26 已就绪（main 上）。
- **退出判据**（全绿才算 done）：
  - [ ] `showcase/spec4b/dropdown-acceptance.html` 在 Unity PlayMode 四门绿：渲染 + 点击/键盘交互 + `SelectionChanged` typed 事件 + class 命中。
  - [ ] NumberField 在 PlayMode 绿：数值约束 + ValueChanged（clamp/quantize）+ 控制键路由。
  - [ ] IME 后端接线：`Input.compositionString` 采集 + KeyList 补 Backspace/Delete/Home/End + composition commit/close 在真机绿（core 侧已就绪）。
  - [ ] 控件束零散 getter 收口：`NumberField.Min/Max/Step` setter、`Dropdown.SelectedValue`、`OptionItem.Value/Selected`、`ProgressBar.IsIndeterminate`（当前全 `NE`）。
- **依赖**：无（P3 编码已合 main）。
- **阻塞**：M1/M2/M3 的进入（不想在未验证的地基上叠新大件）。
- **估时**：几天（纯验收 + 小接线，无新架构）。

---

## M1 · ListView 虚拟化（复合束第一硬骨头）

> 最高杠杆（mail/inventory/shop/character 4 页全卡）+ 最高剩余架构风险。趁架构在脑子里新鲜时啃。

- **进入判据**：M0 绿。
- **退出判据**：
  - [ ] **core**：`ul → ListView`、`li → ListItem`；虚拟化内核——slot 池化 + 可见区裁剪 + 不等高补偿 + content_size 回填 + reuse_key 场景级命名。
  - [ ] **C# 投影**全实装（当前全 `NE`，`LoomGUI.Nodes.cs:2336-2349`）：`ItemCount` / `ItemTemplate` / `TemplateSelector` / `BindItem` / `ScrollToItem` / `RefreshItem(s)` / `NotifyInserted/Removed/Moved` / `ItemExitClass`。`SelectedIndex`/`SelectionChanged` 已删（ul 无 HTML 选中语义，留待 P4 listbox/aria-selected）。
  - [ ] **撤旧**：driver 手写虚拟列表（旧 v1.4 + v1.11）整层吸收进框架，driver 不再做 slot 映射。
  - [ ] **headless 断言**：1000 项数据集只渲染可见区——render node 数 ≈ 可见项数，**不随总项数增长**（这是虚拟化的本质判据，不是"列表能显示"）。
  - [ ] **showcase**：mail 或 inventory 单页真机滚动绿（不等高 + 复用）。
- **依赖**：M0（控件层稳定）。
- **阻塞**：M4（列表项塞富文本的落点）、M6（4 页卡它）。
- **估时**：2–3 周。
- **备注**：文本模型排在它后面——列表项先用现有文本能力凑，富文本等 ListView 在了再塞。

---

## M2 · @keyframes runtime + transition（爽点 + 闭环 fence）

> fence DSL 已就绪，只欠 runtime。home 入场动画 = 演示的那一秒，体感回报高，刻意从 roadmap 表"三束后"拉前。
>
> **状态：编码端 DONE（2026-08，15 tasks SDD）；Unity PlayMode 验收 defer 家里机**。spec `docs/archive/superpowers-specs-2026-08/2026-08-04-m2-keyframes-runtime-design.md`、plan `docs/superpowers/plans/2026-08-04-m2-keyframes-runtime.md`。§4 tech-debt「keyframes runtime」+「动画系统终态」条同步更新。动画引擎终态从「悬置」→ **M2.5 立项**（见下）。

- **进入判据**：M0 绿。fence `@keyframes` at-rule + `animation` 简写 DSL 已完成（commit `e2e2812`）。
- **退出判据**：
  - [x] **pkg**：`KeyframesTable` 进 pkg（v30 → v31 bump）+ bridge 序列化（v30 起含 keyframes 类型定义）。
  - [x] **core**：`ResolvedStyle.animation` 字段 + `KeyframePlayer`（路线甲：独立 player 不翻译成 Tween 序列；ease / iteration / fill-mode / delay / direction / per-segment timing-function）。
  - [x] **transition 真生效**（fence 解析存值 + core transition 引擎消费，共用 core `parse_transition`）。
  - [x] **动画句柄 L3 全套**：`Node.Play(name)` + `Animation` 类（`IsPlaying` / `Time` / `Pause` / `Resume` / `Stop` / `OnStart` / `OnEnd` / `OnKey` / `OnHook`）+ 事件 START/END/ITERATION/KEY/HOOK + TransitionEnd 双路由。
  - [x] **`:nth-child(An+B|odd|even|N)` selector** + `animation-delay` 错峰（home 导航卡入场依赖）。
  - [x] **`@loom-hook` 锦点** + **opacity 父级累积传播**（顺手做掉）。
  - [x] **headless**：keyframes 定义的属性在 t=0 / 0.5 / 1s 取值正确（确定性断言，core 集成测）。
  - [ ] **showcase home**：入场动画真机绿（家里机验收窗口）。
- **依赖**：M0。与 M1/M3 可并行。
- **阻塞**：M2.5（引擎终态）、M5（视觉动效载体）、M6（home 活起来）。
- **估时**：1–2 周。
- **备注**：动画系统终态（池化 Tween + 缓动全集 + 链式 builder + layout 动画 prop_type 分层）做"够 keyframes 跑"的子集，终态推 **M2.5**（不再悬置）。

---

## M2.5 · 动画引擎终态（明确归宿，触发判据启动）

> M2 交付功能完整但引擎内部用"够 keyframes 跑"的实现。以下归 M2.5，触发判据明确，不再悬置。

- **进入判据**（任一满足即启动）：
  1. 第一个需要 layout 动画的 showcase 页出现（如 character 技能面板 accordion 展开 / 用 width 而非 scaleX 的进度条）。
  2. M5 视觉束的 `NodeTransform` 替代 Affine2 升级时合并做（都动 render 数据结构，合做省一次 pkg bump）。
  3. 动画实例并发量使单 Vec TweenManager 出现性能抖动（profiling 实证）。
- **退出判据**：
  - [ ] **池化 Tween**：`TweenManager { active, pool }` 替换单 Vec。
  - [ ] **缓动全集**：cubic-bezier / Elastic / Bounce / Custom + per-stop timing-function（结构化 `EasingFunction`）。
  - [ ] **链式 builder API**：`.tween().delay().ease().repeat(,yoyo).on_complete()`，替换位置参数 `tween()`。
  - [ ] **layout 动画 / prop_type 分层**：动 width/height/flex，tick 时序重构 + `layout_dirty` + solve 重入。
  - [ ] **player 与 Tween 插值原语统一**：共享 `TweenValue{x,y,z,w,d}` + value_size(1..6)。
  - [ ] **`animation` 长划子属性**：8 个标准 CSS 长划属性（`animation-name`/`-duration`/`-delay`/`-iteration-count`/`-direction`/`-fill-mode`/`-play-state`/`-timing-function`）。
- **依赖**：M2。
- **阻塞**：M6（视觉动效深度）。（NodeTransform 升级是进入判据 #2 的 trigger——M5 触发 M2.5 合做，非 M2.5 阻塞 M5；M5 自有 NodeTransform 退出判据。）
- **估时**：2–3 周。
- **备注**：不是"动画能用"（M2 已交付），是"引擎内部终态"——无硬阻塞 showcase 页面，按需启动。详细范围见 main-design §13.2 M2.5 标记 + §13.6。

---

## M3 · TabList（P4，WAI-ARIA 复合控件先行）

> settings 页硬卡 TabList。它动 cascade 主路径，独立 spec 干净，别和渲染管线改动混（roadmap 自己也这么定）。
>
> **状态：编码端 DONE（2026-08，10 tasks SDD）；Unity PlayMode 验收 defer 家里机**。spec `docs/archive/superpowers-specs-2026-08/2026-08-04-m3-tablist-design.md`、plan `docs/superpowers/plans/2026-08-04-m3-tablist.md`。§4 tech-debt `[aria-selected]` 条同步标 RESOLVED。
>
> **实现期发现**：原 exit criteria 的「Node attrs 存储」「attr_matches_node 扩展」「role dispatch」三条前置里，后两条早在控件束 P1-P3 做掉（commit `0a7373d` / `a797840`）；第一条未走通用 attrs 仓库（β），改 α 路线——`RoleInfo.aria_controls` 特定存储。故 M3 真实工作远比原列窄。

- **进入判据**：M0 绿；建议挑 cascade 主路径无其他改动的窗口。
- **退出判据**：
  - [x] **pkg v29 bump**（`TemplateNode.aria_controls` 字段，TabList tab→panel 跨树关联）。
  - [x] **TabList/Tab 投影类**（`NodeKind::TabList/Tab` + `SemanticKind` + `ControlState::TabList{selected_index}` + packer bridge `map_semantic` + C# `TabList`/`Tab`）。
  - [x] **`[aria-selected="true"]` selector** 运行时可匹配（`synth_aria_value` 跨节点合成 aria-selected，从父 TabList.selected_index 派生）。
  - [x] **panel 显隐 P1**（`sync_control_visuals` 据 `RoleInfo.aria_controls` 每帧 `find_by_id_attr` 解析 + `set_inline_override` display:block/none）。
  - [x] **键盘导航 K1**（方向键移动 selected_index，水平/垂直分派）。
  - [x] **SelectionChanged** 复用（typed 事件层 demux）。
  - [x] **showcase settings**：`.tab[aria-selected="true"]` CSS 规则解注释，pkg 重打 v29。
  - [ ] **Unity PlayMode settings 页**：点 tab → 高亮 + panel 切换；方向键切 tab；`SelectionChanged` 真机接（家里机验收窗口）。
- **依赖**：M0。与 M1/M2 独立可并行。
- **阻塞**：M6（settings 页）。
- **估时**：~1 周。
- **备注**：**Tree 推迟**（按需）——如果 character 技能树 / 背包分类树真需要再提上来跟 TabList 合并做，否则不进本里程碑。

---

## M4 · 文本模型回归标准子树（复合束第二硬骨头）

> 现有文本够用 → 不是硬阻塞，所以排后。等 ListView 在了，列表项塞富文本才有落点。

- **进入判据**：M1 绿。
- **退出判据**：
  - [ ] **删** `display:block` RichText 暗号残留路径（算法 `text/rich.rs` 保留复用）。
  - [ ] **block 文本**：`p/h1–h6` 建文本 block；`span/a/strong/em` 等 inline 元素是语义容器。
  - [ ] **内部扁平化**：编译成 TextRun / ImageRun / LinkRun；**公共树保留** TextNode / TextElement / Image / Link 的 ID 和事件（公共语义树 ≠ 内部渲染树）。
  - [ ] **复用** v1.6 字体自绘 + v1.8 文字效果算法（换表达方式，算法主体不动）。
  - [ ] **showcase**：form 或 mail 富文本块绿（标题 + 段落 + 行内链接/强调）。
- **依赖**：M1（列表项富文本落点）。
- **阻塞**：M6（form/mail 质量）。
- **估时**：2–3 周。

---

## M5 · 视觉束（精简版，scope 已砍）

> 护城河是布局可预测，不是滤镜像素。撑到"好看够用"就收手。

- **进入判据**：M2 绿（视觉动效要有动画载体）；纯装饰部分（grayed / shadow / gradient）可与 M2/M3/M4 并行。
- **退出判据**（只做这些）：
  - [ ] **grayed 灰化**：`RenderNode` 加 `grayed: bool` + 渲染（color tint 路径，小）。
  - [ ] **linear 多 stop + 任意角度**：沿渐变线切 N 段 sub-quad（pkg bump）。
  - [ ] **box-shadow inset** + 按需多层（v1.8 算法在，换表达）。
  - [ ] **card-img Image bg 合成 node_id 机制**：照 box-shadow `BOX_SHADOW_FLAG` 模式（tech-debt）。
  - [ ] **NodeTransform 替代 Affine2**：`RenderNode.world_matrix` 从 `[f32;6]` → 分解 Position/Scale/Rotation，与 set_transform FFI 同频（第一个高频控件触发）。
- **明确砍**（需用时再提）：
  - radial / conic（要新 shader，编码机验不了）
  - 多层 background（99% 单层够，ROI 低）
  - 高级 filter / BlendMode 12 种
  - 动画引擎终态（池化 + 缓动全集 + 链式 builder + layout 动画）→ **已独立立项 M2.5**（见上），不再压在 M5
- **依赖**：M2（部分）。
- **阻塞**：M6（home/shop 好看）。
- **估时**：~2 周。

---

## M6 · showcase 收口 + tech-debt 扫除

> 到这步所有能力齐备，把页面逐个捅绿 + 把零散债清掉。

- **进入判据**：M1–M5 + M2.5 绿（M2.5 按需启动，不硬阻塞 M6）。
- **退出判据**：
  - [ ] **代表页真机全绿**（建议 4–5 页够证明能力，不必死磕 8 页）：home + settings + mail + inventory（+ shop 视精力）。每页布局与浏览器 rect 比对一致（护城河判据）。
  - [ ] **零散 `NE` 清零或显式 defer**：`StyleSheet.Add/Clear`、`Container.ScrollTo`、`Image.Src`、`Node.Play`、`SetVar/RemoveVar`、`ZIndex/Visibility/Touchable/Focusable`、`OnUpdate`、source-less 事件（`ScrollChanged`/`AnimationStart|Iteration`）。
  - [ ] **tech-debt 归零或显式 defer**：`Scene::build` dead `data_controller` 参数、add_class null check、GUI exe 拷贝流程、loom.runtime.json stomping。
  - [ ] **门全绿**：`cargo fmt --check` + `cargo clippy -D warnings` + `cargo test`（全 workspace）+ PublicApi 编译门。
- **依赖**：M1–M5（+ M2.5 若已启动）。
- **阻塞**：可对外演示终态。
- **估时**：1–2 周。

---

## M∞ · 解耦项（另条命，不计入）

> 与 runtime 范式解耦，按需 / 排最后。

- **Custom Element + slot 组件系统**：当前无人用它做游戏，roadmap 明示排最后。
- **平台移植**：移动 + IL2CPP + WebGL（排最后）。
- **编辑器 / 工具链闭环**：独立于 runtime，可并行。
- **视觉完整版**：radial/conic/多层 background/高级 filter（M5 砍掉的，真有需求再提）。

---

## 推荐执行节奏（单人）

| 周 | 干什么 | 产出 |
|----|--------|------|
| W1 | M0 | 控件束 officially done |
| W2–4 | M1 ListView | 4 页解锁（最大杠杆）|
| W5–6 | M2 keyframes | home 活起来（士气爽点）|
| W7 | M3 TabList | settings 解锁 |
| W8–10 | M4 文本模型 | form/mail 富文本 |
| W11–12 | M5 视觉精简 | 好看够用 |
| 按需 | M2.5 动画引擎终态 | 触发判据满足时启动（不阻塞可演示）|
| W13 | M6 收口 | 可演示 |

- 上面是"精简路径"约 3 个月到可演示。**再省**：M4 砍到只做 block 文本不做 inline 富链接、M5 砍到只 grayed + linear，可压到 ~2 个月。
- **家里机串行约束**：凡是退出判据里标"真机绿"的，都得排队去家里机验；编码机用 headless 测试先锁逻辑（M0 的教训）。
