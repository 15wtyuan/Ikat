# LoomGUI 路线图

> 本文件**只看向前**。已完成的历史（摸黑打通 + 三束加宽纪元）归档在 [`roadmap_old.md`](./roadmap_old.md)（含旧里程碑执行计划 [`milestones_old.md`](./milestones_old.md)）；活的架构契约在 `docs/design/`（`main-design.md` / `public-api.md` / `projection-layer.md` / `fence.md`）。
>
> 踩坑全库见 `docs/pitfalls.md`，有意 defer 的可执行项登记在文末「延期项登记表」，重构决策史见 `roadmap_old.md` §8。

---

## 北极星：完全体

把 LoomGUI 做成一个**真能拿来发布游戏**的跨引擎游戏 UI 框架。判据（全部满足 = 完全体）：

1. **契约对齐**：三份终态契约（`public-api.md` / `main-design.md` / `fence.md`）100% 落地——公共 API 无 `NotImplementedException` 壳、围栏 CSS 子集完整、架构边界（公共语义 / 内部行为 / 引擎后端三层）干净。
2. **跨引擎**：Unity 后端 ✓；Godot-C# 后端跑通同一 showcase——兑现「Rust 核心 + 多引擎投影」赌注。
3. **布局护城河**：showcase 8 页在 Unity 真机全跑通 + 布局 rect 与浏览器对齐（护城河 = AI 能预测布局，不是像素级渲染）。
4. **可用证明**：一个真实小游戏用它做出来，证明「AI 拼 HTML/CSS 即得界面」核心论点。
5. **发版**：v1.0——公共 API 冻结 + 契约版本化 + 文档 + git URL 分发（release CI 已就绪）。

---

## 工作方式（老 roadmap 的教训）

- **track 并行 + 里程碑门**：四条 track 独立推进；每个里程碑有可测 done 标准。**不横切大层**（老 roadmap 横切 R2–R8 成了啃不动的山）。
- **showcase / dogfood 逼需求**：不凭空补理论清单，能力由「做页面 / 做游戏」逼出来。
- **SDD**：每件稍大的事 `spec → plan → implement → review`；坑进 `pitfalls.md`，不进代码。
- **两台机约束**：核心范式在编码机 headless 锁死；真机视觉 / 输入验收在 Unity 机。
- **叙事进 commit，不进 roadmap**：「为什么改方向」写进决策记录 commit / pitfalls；roadmap 保持稳定向前，只记「奔哪去 + 到没到」。

---

## Tracks

四条并行工作流（ABC 各一 + 一条横切）。每条列「到契约对齐还差什么」，按里程碑里被逼出的优先级做。

### T1 · 能力补全（→ 契约对齐）

把围栏 CSS 子集 + 公共 API 补到与设计契约对齐。缺口：

- **视觉**：~~渐变（现仅 linear 2 色 4 方向）~~ **radial + 多 stop + 任意角度已交付（2026-08-14，program=6/7 per-fragment shader + blob v13 grad_params）**；剩余 defer：conic、repeating-\*、渐变×圆角/边框共存（见延期项表）。`filter: blur()`（需离屏 RT 基建，非小缺口）、`grayed` 渲染（现借 `filter: grayscale(1)`）照旧。
- **复合 · scope 三件套**：`Get<T>` 的 `IsScopeRoot` 边界（现裸 DFS，穿透嵌套组件 / list item）、per-scope ID 去重（现全模板级）、Shadow DOM 样式隔离（现仅 dynamic rule 按 scope 过滤，非完整 cascade scoping）。
- **复合 · 组件系统**：Custom Element + `<slot>` 内容投影（现空壳——fence 认 hyphen 标签 + `<slot>`，C# 有空类；无 `customElements.define` 注册表 / 投影 / 生命周期）。
- **控件**：Tree（`role=tree`，无）。
- **动画引擎终态**：池化 Tween + 缓动全集（cubic-bezier / Elastic / Bounce / per-stop timing）+ 链式 builder + layout 动画 prop_type 分层。进入判据：第一个需 layout 动画的页面，或动画并发使单 `Vec<Tween>` 抖动。
- **文本模型收尾**：inline run 编译（TextRun / ImageRun / LinkRun），公共树保留 TextNode / TextElement / Image / Link 的 ID 和事件；`display:block` RichText 暗号已退役，收尾表达层。
- **公共 API 扫尾**：扫 `unity/package/Runtime/Public/LoomGUI.*.cs` 残留 `throw NE()`，逐个接通或显式标注 by-design。（2026-08-11 audit 已 triage 全部 NE stub——`ScrollTo`/`Button.Disabled` 已接、`Visibility` 砍出契约，其余详见「延期项登记表」公共 API NE stub 条目。）
- **运行时 CSS**：`StyleSheet.Add` 解析路径接通 + `UIStyleException` 抛出（异常类已定义，路径未接）。

### T2 · 验收 + 发布（→ 可用）

证明框架完整可用并推向发版。

- **showcase 收官**：8 页（home / settings / mail / inventory / shop / character / form / lab）Unity 真机全跑通 + headless Chrome rect-diff 对齐（工具链 `showcase/scripts/rect-diff/` 已就绪）。
- **清家里机验收债**：ListView / 动画 / TabList / Dropdown 的 Unity PlayMode 四门（验收页 + pkg + checklist 已备）集中跑过。
- **dogfood**：做一个小游戏（验收载体，非 roadmap 主线）逼出真实需求、反哺 T1。
- **文档**：公共文档 + quickstart + tutorial。
- **发版**：v1.0——公共 API 冻结 + 契约版本化 + tag。

### T3 · 横向扩展（→ 跨引擎 / 工具 / 平台）

核心稳了之后铺开。

- **Godot-C# 后端**：复用 `LoomHost` + 整个 Projection + Public，只写 `GodotLoomBackend`（多引擎分层 `LoomHost` / `LoomBackend` 已为此预留）。跑通同一 showcase = 跨引擎赌注兑现。
- **编辑器 / 工具链闭环**：packer GUI（`loomgui_gui`）→ 可视化编辑 / Inspector / 热重载（scope 待用户反馈定）。
- **平台**：IL2CPP（移动 / 主机）+ WebGL + mobile（多触摸 v1 已有）。

### T× · 质量 / 性能 / 债（横切）

跨 track 随时插。

- **性能**：`solve` 每帧重建 taffy 树（坑 186，文本重测已 memoize 缓解，树重建本身待定）。（攒批回写 flush 已落地——StyleMirror/NodeTransform 帧末 `FlushDirtyStyles`/`FlushTransform` 排空 dirty 集合，`LoomHost` flush seam 驱动。）
- **机制债**：card-img Image bg 合成 node_id 机制（悬置，照 box-shadow 合成 id 模式）；`RenderNode.world_matrix` `Affine2` → `NodeTransform` 升级（TRS 分解对齐公共 API）。
- **清理 / defer 登记**：有意 defer 的可执行项登记在文末「延期项登记表」（每项带进入判据 + 来源 spec），做完即移除；旧纪元 tech-debt 见 `roadmap_old.md` §4。

---

## 里程碑（gated 序列）

> 序列哲学：**先验证再扩展**（老 roadmap 核心教训）。T1 喂 T2（能力够才验收），T2 验过才 T3（核心稳了才铺第二引擎），T3 跑通才发版。

- **里程碑 1 · Unity 收官** — 证明框架在 Unity 上完整可用。
  - 视觉束补齐到 showcase 够用：渐变（radial + 多 stop）✅ 代码侧 done（2026-08-14，Unity 视觉验收随任务 4 逐页过）。（`filter: blur()` 需离屏 RT 基建、非 showcase 阻塞，留后续。）
  - 8 页 showcase Unity 真机全跑通 + rect-diff 对齐浏览器。
  - 清家里机 PlayMode 验收债。
  - **门**：8 页真机全绿 + home radial 渐变可见 + rect-diff 通过。

- **里程碑 2 · Dogfood** — 证明能拿来做事，逼出真实需求。
  - 小游戏 demo 可玩；按 demo 逼出的需求排 T1 剩余（scope 三件套 / Custom Element / Tree / 动画引擎终态）。
  - **门**：demo 可玩 + scope 三件套完成（组件封装真正成立）+ demo 逼出的 feature 全绿。

- **里程碑 3 · 跨引擎** — 兑现跨引擎赌注 + 工具链 / 平台铺开。
  - Godot-C# 后端跑通同一 showcase；编辑器闭环 v1；一个新平台（IL2CPP 或 WebGL）。
  - **门**：Godot 跑通 showcase + 可视化编辑 v1 + 一个新平台跑通。

- **里程碑 4 · v1.0 发版** — 公共 API 冻结。
  - 文档 + quickstart + tutorial；公共 API 冻结 + 契约版本化；v1.0 tag。
  - **门**：v1.0 release。

---

## 近期任务（里程碑 1 展开）

> 里程碑 1「Unity 收官」拆成 5 个有依赖序的可执行任务。**任务 1 文本模型已 done（代码侧）**，Unity 视觉 QA 留家里机；**任务 2 rect-diff 工具链 settings 页已就绪（🟡，Unity rect 半留任务 4）；任务 3 渐变补齐代码侧 done（Unity 视觉验收留家里机）；下一件事 = 任务 4（逐页修，依赖 1+2+3 已齐）+ 任务 5（独立可插）**。

**任务 1 · 文本模型回归标准子树（inline flow）**【✅ done · 2026-08-12】
- **落地**：fence 6.4 分类（rich-text-block + mixed 报错 + img 豁免）→ pkg v33 + `Node.rich_text_block` → run 编译器（`compile_rich_runs`）→ solve 折叠（RichText leaf + `rich_text_fingerprint` memo）→ render Container+flag arm（多 run mesh + box-shadow）→ `hit_test_rich` + FFI。详见 spec `docs/superpowers/specs/2026-08-12-text-model-design.md` + plan `docs/superpowers/plans/2026-08-12-text-model.md`（9 task SDD，全部 per-task + final review APPROVED）。
- **实证**：packer showcase 0 mixed；`dump_rich_text` 实测 mail 正文 7 inline 子→1 行（非竖排），公共树 ID 保留。workspace 1506 测全绿。
- **门**：✅ 代码侧全绿。⏳ Unity PlayMode 视觉 QA（form/mail inline flow 浏览器对齐 + span click via hit_test_rich + rect-diff）留家里机。

**任务 2 · rect-diff 工具链打通一页**【🟢 编码机侧全通 · 2026-08-14】
- browser rect 已有（`showcase/scripts/rect-diff/browser-rect.mjs`，headless Chrome 导出 DOM rect）；core dump 路径已打通（`dump_page --json` 接 `diff.mjs` 比对）。**Unity PlayMode 运行时 rect 路径已接**（`run-page.sh --scene=`：Unity 机 `LoomBridge.DumpScene()` 导出 → `normalize-dump-scene.mjs` 归一 → diff；编码机 round-trip 自测通过，待 Unity 机首跑）。
- **门**：rect-diff 在一页产比对报告。（工具链可先搭，但结果要等任务 1 文本对了才有意义。）
- **进度（2026-08-12）**：settings 页端到端跑通——browser-rect → **core dump（`dump_page --json`，非 Unity rect 路径）** → diff.mjs 三步 runner（`run-page.sh`）产报告 `snapshot-2026-08-12-settings.md`，门「报告产出」✅ 达成。4 处工具链修复（合成根 DFS / 0-size 原点 / preview letterbox）剔 206 假 diff，剩 12 残余（slider thumb transform 发射缺口 / CJK 字宽 / sub-2px 级联）全归类为 Task 4 燃料，**settings 零 core 布局 bug**。
- **进度（2026-08-14，8 页全量）**：全部 8 页 core-dump 路径跑通（报告 `snapshot-2026-08-14-8pages.md`），工具链再净化 5 处（tag 词汇表归一 / preview JS 保留但撤销 data-fill 克隆 / 0×0 不进配对桶 / FOLDED 类别 / preview-base.css 补 workspace 字体）——unpaired 噪声 106→2。**8 页 0 unmatched、无结构性分歧、零疑似 core 布局 bug**；475 残余全归四类（文本测量精度差 / TextElement inline 盒宽语义 / slider thumb transform 发射缺口 / template 枚举差），A 类容差定标与 B 类潜在视觉风险留任务 4 / dogfood 逼出再定。

**任务 3 · 渐变补齐（home radial 光晕 + 多 stop）**【✅ 代码侧 done · 2026-08-14】
- **落地**：`Gradient` 数据模型（linear 任意角度 + 多 stop ≤8 / radial 全形）替换 `Gradient2`（pkg v34）；渲染统一 program=6/7 per-fragment 渐变 shader（blob v13 grad_params 列 208B；premultiplied 插值 + bg-color 垫底 source-over 合成）；文本渐变 CPU 采样与 shader 同一套 t 数学；fence `<style>` 渐变值探针（坏渐变打包期报）。spec 见 `docs/superpowers/specs/2026-08-14-gradient-radial-multistop-design.md`。
- **实证**：cargo workspace 1533 测全绿（clippy/fmt 严门过）；dotnet 三套 410 测全绿（fixture 重打 v34 + 顺手修了 test.workspace 撞 fence 6.4 的存量问题）；`dump_page` 渐变参数 dump——lab 页 20 节点（角度归一/多 stop 等分/显式位置/radial 各形）+ home 光晕（c=1574.4,-129.6 / 1100×560 / 0.1→transparent@0.6）全部正确；Chrome 基准截图 3 张入库 `showcase/scripts/gradient-baseline/`。
- **门**：✅ 代码侧全绿（多 stop rect 对齐浏览器留 rect-diff 逐页跑，归任务 4）。⏳ Unity PlayMode 视觉验收（lab section 12 标本矩阵 + home 光晕 + 渐变字）留家里机——shader 纸面设计，GRADIENT 变体首次编译在 Unity 机。

**任务 4 · 逐页 Unity PlayMode 真机 + rect-diff（8 页）**
- 依赖任务 1（文本）+ 2（工具链）+ 3（渐变，home 要）——三者代码侧均已 done。按依赖排：先静态页（settings/character/shop/form/lab）→ 再虚拟列表页（mail/inventory）→ home（动画 + 渐变）。
- **编码机半场（2026-08-14）已完工**：8 页 core-dump 路径 rect-diff 全绿收敛（见任务 2 进度）——**core 侧零布局 bug**，残余全归工具链无关的度量/语义类别；PlayMode 运行时比对路径（`--scene=`）也已接好待 Unity 机首跑。Unity 机剩：每页 PlayMode 跑通 + `--scene=` 导出比对 + driver 列表虚拟化验收 + lab/home 容差定标。
- 每页：PlayMode 跑通 → rect-diff 比对 → 修 bug → 下一页。
- **门**：8 页真机全绿 + rect-diff 通过。

**任务 5 · 清家里机验收债**
- ListView / 动画 / TabList / Dropdown 的 Unity PlayMode 四门（验收页 + pkg + checklist 已备，编码端全 DONE）。
- **门**：四门全绿。独立可随时插。

> 任务 1 是阻塞项（最先）；任务 2-3 可并行；任务 4 依赖 1+2+3；任务 5 独立。做完 5 个 = 里程碑 1 完工，进里程碑 2（dogfood）。

---

## 当前快照（2026-08-14，时点状态）

- 摸黑打通 + 三束加宽纪元已完工（详见 `roadmap_old.md`）：Unity 端到端可用，21 控件全栈、cascade / 动画 / 虚拟列表 / box-shadow / 文字特效 / transform / filter 矩阵已交付，release CI 就绪。
- **近期优先**：**里程碑 1 任务 1（文本模型）+ 任务 3（渐变补齐：radial + 多 stop + 任意角度，program=6/7 shader）代码侧均 done**（任务 3：commit 见 spec `2026-08-14-gradient-radial-multistop-design.md`，pkg v34 + blob v13 + dll/GUI exe 已同步入库）；**任务 2 rect-diff 工具链 settings 页端到端跑通**（core-dump 路径，2026-08-12）—— 12 残余 + Unity rect 半留 Task 4。**下一件事 = 任务 4（逐页修，1+2+3 依赖已齐）与任务 5（清验收债，独立）**。
- **横切收尾（2026-08-14）**：公共 API FFI 批四件接通（NumberField bounds setter / ProgressBar.IsIndeterminate / RadioButton.Name / UIContext.Pick，见延期表）；CI 补 dotnet 门（HeadlessTests 现场 Linux .so 跑 P/Invoke 全套 + PublicApi 编译门——此前 CI 只跑纯 managed 31 测，HeadlessTests 曾随 pkg 版本漂移静默腐烂无人知）。
- **rect-diff 8 页全量（2026-08-14）**：任务 2/4 编码机半场完工——8 页 core-dump 路径 0 unmatched / 零疑似 core 布局 bug（报告 `snapshot-2026-08-14-8pages.md`），工具链净化 5 处（tag 归一 / data-fill 撤销 / 0×0 桶 / FOLDED / workspace 字体）。**Unity 机剩任务 4 的真机半 + 任务 5 验收债 + 任务 3 渐变视觉验收。**
- **悬置判据项**：动画引擎终态（等 layout 动画需求）、Godot / 编辑器（等里程碑 1、2）。

---

## 延期项登记表

> 各 spec 里**有意 defer 的可执行项**（不是踩坑——踩坑进 `docs/pitfalls.md`）。每项：做什么 / 进入判据（trigger）/ 来源 spec。做完或确认不做即移除。核于 2026-08-11，来源是 2026-07-28 起的 8 个 spec。

### T1 · 能力补全

**Tree 复合控件** — WAI-ARIA `role=tree/treeitem` 复合控件（角色技能树 / 背包分类树），镜像 TabList 全套机制（ControlState + synth_aria + role 分派）。
- 判据：character 技能树 / 背包分类树真需要时。
- 来源：`2026-08-04-m3-tablist` §11；`2026-07-28-controls-debt-and-dropdown` §11；`2026-07-30-control-role-refactor` §3.1。

**通用 Node attrs 仓库（β 路线）** — per-Node 通用 HTML 属性存储 + 业务驱动选择（vs α 路线的特定字段如 `aria_controls`）。
- 判据：第二个非控件状态属性用例出现（Tree / Accordion）。
- 来源：`2026-08-04-m3-tablist` §3.1、§11。
- ⚠ `2026-07-28-controls-debt-and-dropdown` §11 把 "Node attrs 存储 + role dispatch + `[aria-selected]` 匹配" 整体 defer 给 P4，但 M3 spec §2.1 确认 role dispatch + `attr_matches_node` 的 role/aria 支持**已做**。只剩通用 β 仓库真 defer。

**`aria-labelledby` 运行时关联** — tab↔panel 反向关联；fence 已认（`structural.rs:140`），运行时未用。
- 判据：将来需要时，与 `aria-controls` 同机制一并补。
- 来源：`2026-08-04-m3-tablist` §11。

**组件封装三件套（L2 + L3）** — `Get<T>` 的 `IsScopeRoot` 完整边界（不穿透嵌套组件 / list item）+ per-scope ID 去重（真嵌套组件语义）+ Shadow DOM 样式隔离（模板内选择器作用域）。三者同一套系统。
- 判据：第一个嵌套组件 / 需样式隔离的组件系统。
- 来源：`2026-08-05-pooled-slot-lifecycle` §5.1、§5.4、§8。

**Slot / CustomElement 投影 + 注册机制** — 复合束 `<slot>` 内容投影 + `customElements.define` 注册表 + 生命周期（C# 投影类已是空壳占位）。
- 判据：复合束组件系统推进时。
- 来源：`2026-07-28-controls-debt-and-dropdown` §3.3、§11。

**layout 动画 / prop_type 分层** — 动画 layout 属性（width/height/flex，非只渲染层 transform/opacity/color）——需 `prop_type` 分层（`transform_dirty` vs `layout_dirty`）+ tick 时序重构 + solve 重入。
- 判据：第一个需 layout 动画的页面（如 accordion 展开 / 用 width 而非 scaleX 的进度条）。
- 来源：`2026-08-04-m2-keyframes-runtime` §1.3、§3.2、§12。

**视觉：渐变剩余形态** — radial + 多 stop + 任意角度已交付（2026-08-14，见里程碑 1 任务 3）。剩余 defer：`conic-gradient` / `repeating-linear/radial-gradient`（围栏打包期拒收，判据：第一个真需要的 UI）；渐变 × 圆角 / 九宫格 / 边框共存（`use_gradient` 门互斥，共存需混合 mesh 或边框独立 draw call，判据：第一个圆角渐变按钮 UI）；`to top right` 角点方向关键字；>8 stops（FFI grad_params 列定长 8 槽）。
- 判据：上述形态被 showcase / dogfood 逼出时。
- 来源：`2026-08-14-gradient-radial-multistop-design.md` §2/§10。

**视觉：`filter:blur()` 任意内容模糊（需离屏 RT 基建）** — 任意内容模糊需离屏 RenderTexture 基建；box-shadow 的 SDF 模糊只覆盖圆角矩形形状，覆盖不了任意内容。
- 判据：真有内容模糊 / 视觉后处理需求（基建级，非小缺口）。
- 来源：`2026-08-09-box-shadow-multilayer-inset-blur` §2.2。

**动画长划子属性（8 个）** — `animation-name/duration/timing-function/delay/iteration-count/direction/fill-mode/play-state`（及 transition 等价）——现只解析简写。
- 判据：低 ROI（简写语法糖），按需。
- 来源：`2026-08-04-m2-keyframes-runtime` §8.6。

**控件细化（按需）** — TabList 手动激活模型（方向键只移焦点、Enter 才选中）；Dropdown popup 视口感知定位（上/下/收缩，现只向下）。
- 判据：真机使用反馈需要时。
- 来源：`2026-08-04-m3-tablist` §3.2、§11；`2026-07-28-controls-debt-and-dropdown` §11。

**运行时 CSS 解析（`StyleSheet.Add` + `UIStyleException` 接通）** — 运行时 CSS 解析失败抛 `UIStyleException`（异常类已定义，解析路径未接）。
- 判据：运行时 CSS 解析这块活推进时。
- 来源：`2026-07-28-controls-debt-and-dropdown` §3.4、§11。

**a11y 焦点链（roving tabindex / Home/End / Tab 遍历）** — 完整 WAI-ARIA 焦点管理。
- 判据：重 a11y，游戏 UI 不刚需，长期低优。
- 来源：`2026-08-04-m3-tablist` §3.2、§11。

**公共 API NE stub 接线（public-api audit triage，2026-08-11）** — `Public/LoomGUI.*.cs` 残留 `throw NE()` 的公共 API，按 core/FFI 支持分档处置（详情见 public-api audit）。
- ✅ **已接（2026-08-11）**：`Container.ScrollTo`、`Button.Disabled`（FFI 已有，C# 漏接，坑 191 模式）。
- ✅ **已接（2026-08-14，FFI 批）**：`NumberField.Min/Max/Step` setter（FFI arm 扩 NumberField，改界后 value 文本重约束）、`ProgressBar.IsIndeterminate`（新 get/set FFI，纯状态位）、`RadioButton.Name`（新 get_radio_name 双调法 FFI）、`UIContext.Pick`（新 loomgui_stage_hit_test，thumb sentinel decode 回容器）。dll/bindings 已同步入库。
- 🟡 **按域 defer**（core 也没，真 feature）：`Touchable`/`ZIndex`（扩 inline_bit 表，归 T1）、`Focusable`（runtime tabindex setter，T1）、`Dropdown.SelectedValue`（option `value` 存储，T1）、`SetVar`/`RemoveVar` + `StyleSheet.Add`/`Clear`（custom props 系统 + runtime CSS parser，归 T1 运行时 CSS 大件）、`OnUpdate` + `CallLater`/`CallNextFrame`（per-frame hook + 延迟回调队列，归 T× 框架基础设施）、`UnloadPackage`（包生命周期，归 T2）、`GetTemplate`（归 T1 Custom Element 组件系统）、`ListView.ItemExitClass`（归 T1 list 进出场动画）。
- 🔴 **砍出契约**：`NodeStyle.Visibility`（fence CSS 子集无 `visibility` prop，`opacity:0` 覆盖占位隐藏；public-api.md 已删，C# enum/property 代码删除 follow-up）。
- 判据：按域 defer 随各自 track 推进。
- 来源：public-api.md audit（2026-08-11，triage agent）。

### T2 · 验收 + 发布

**OpenUPM registry 接入** — 发布到 OpenUPM（包名 `com.loomgui.unity` 已兼容）供版本浏览 + 社区曝光。
- 判据：框架稳定（约 v0.5+）后评估。
- 来源：`2026-08-09-loomgui-release` 范围外。

**CI 方案 B（自动编 dll + 自动打 tag）** — 从手动编 dll + CI 校验，升级到 CI 自动编 dll + 自动 tag。
- 判据：手动编 dll 成为负担时（git-URL 硬约束：dll 必须在 tag commit 里）。
- 来源：`2026-08-09-loomgui-release` 范围外。

**Git LFS（入库 dll）** — 入库的 native dll 迁到 Git LFS。
- 判据：仓库膨胀后再评估（⚠ OpenUPM 不支持 LFS，二选一）。
- 来源：`2026-08-09-loomgui-release` 范围外。

### T3 · 横向扩展

**多平台 native 库（macOS / Linux / 移动端）** — FFI native 库现仅 Windows `loomgui_ffi_c.dll`；跨平台需补 macOS/Linux/移动 native 编译。
- 判据：跨引擎 / 跨平台推进时（Godot 非 Windows / 移动平台的硬前置）。
- 来源：`2026-08-09-loomgui-release` 范围外。

**独立 player build（IL2CPP）调试支持** — debug-bridge 对独立 player（IL2CPP）的支持——Roslyn 不能 JIT，C 方案锁定下 out of scope。
- 判据：需 player build 调试时（方案需重做，非小补）。
- 来源：`2026-08-08-unity-debug-bridge` §1、§6。

### T× · 质量 / 性能 / 债 / 工具

**M2.5 · 动画引擎终态（4 子项）** — spec 为动画引擎终态明确立项 M2.5（之前「悬置」无家可归）。含：
1. **池化 Tween**：`TweenManager { active, pool }` 替单 `Vec<Tween>`（判据：并发量使单 Vec 抖动，profiling 实证）。
2. **缓动全集**：cubic-bezier / Elastic / Bounce / Custom + per-stop `animation-timing-function`（现 Ease enum 已 10 keyword 变体，但缺 cubic-bezier/Elastic/Bounce/Custom）。
3. **链式 builder API**：`.tween().delay().ease().repeat(,yoyo).on_complete()` 替位置参 `tween()`。
4. **player 与 Tween 插值原语统一**：共享 `TweenValue{x,y,z,w,d}` + `value_size(1..6)`（可与 M5 `NodeTransform` 替 `Affine2` 合做——都动 render 数据结构，省一次 pkg bump）。
- 进入判据（任一）：① 第一个需 layout 动画的页面；② M5 视觉束 `NodeTransform` 升级；③ 单 Vec TweenManager profiling 抖动。
- 来源：`2026-08-04-m2-keyframes-runtime` §3.1、§8.3、§12。

**card-img Image bg 合成 node_id 机制** — Unity 后端按 node_id 去重，Image bg + texture 同 node_id 只画 texture。要照 box-shadow 合成 id 模式（IMG_BG_FLAG + 建 2 GameObject + sort_key propagate）。补丁曾 revert，机制草稿待商议。
- 判据：Image 同时要 bg 色 + texture 时（视觉正确性）。
- 来源：`roadmap_old.md` tech-debt 段（Spec-4b P3.4 视觉 1/5 未过）。

**`RenderNode.world_matrix` Affine2 → NodeTransform 升级** — 现 `Affine2`（[f32;6] 裸仿射），终态 `NodeTransform`（TRS 分解，对齐公共 API）。与 set_transform 数值 FFI、M2.5 插值原语统一同频。
- 判据：第一个高频 transform 控件 / M2.5 合做。
- 来源：`roadmap_old.md` tech-debt 段（M5）。

**slot pool 内存驱逐（eviction）** — dormant slot 池现 high-water 不收缩、无驱逐（设计约束 e）。
- 判据：长会话 + 海量 slot 场景（mail high-water ~50 slot 内存可忽略）。
- 来源：`2026-08-05-pooled-slot-lifecycle` §8。

**调试 dump 工具（debug-bridge v1 移除项）** — DumpNode 单节点聚焦 dump（含 `ComputedNodeStyleRepr` marshal + scroll_pos）/ Render-tree 内部 dump / Text-glyph metrics dump。均需新 FFI。临时替代：`execute-dynamic-code` + 公共 Node API + 反射。
- 判据：现有 dump 不够定位某类 bug 时。
- 来源：`2026-08-08-unity-debug-bridge` §5.2、§6。

**pi 专用 Skills 打包（debug-bridge P5）** — 高频 LoomBridge helper 包装成 unity-cli-loop 自定义工具（`[McpTool]` + `SKILL.md` 自动发现）/ pi skill 打包。
- 判据：某 helper「用顺了」再升级成工具。
- 来源：`2026-08-08-unity-debug-bridge` §5.4、§6、§8。

**solve 每帧重建 taffy 树（坑 186）** — 文本重测已 memoize 缓解（97.5% 命中），taffy 树每帧重建本身待定。
- 判据：profiling 实证树重建成热点时。
- 来源：`docs/pitfalls.md` 坑 186。

### 已被后续 spec 取代的 stale defer（勿重新加回）

- ~~Node attrs 存储 + role dispatch + `[aria-selected]` 匹配~~：`2026-07-28-controls-debt-and-dropdown` §11 整体 defer 给 P4，但 `2026-08-04-m3-tablist` §2.1 确认 role dispatch + `attr_matches_node` 的 role/aria 支持**控件束 P1–P3 已做**。只剩「通用 β attrs 仓库」（见上 T1）真 defer。
