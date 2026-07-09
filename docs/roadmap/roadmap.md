# LoomGUI 路线图

> v1 架构验证完成（v1a-v1e + showcase + v1.1-v1.3+ 动态树，家里机验收坑 58-95 修复），桌面 Mono 可演示。
> 本文是 **v1 全貌（已交付）+ v1.x 连续功能线（一路做到上线就绪）+ v other 编辑器并行 + v1.x 机制草稿**。
> **规划原则（2026-07-07 重订）**：不再设独立 v2 特性桶——原 v2 的平台/特效/几何工作全部拆进 v1.x 连续编号，**v2.0 = 全部做完的"真正上线就绪"毕业里程碑**，不是特性堆。RmlUi/fgui 对标依据见 `docs/roadmap/rmlui-research.md`；设计契约见 `docs/design/main-design.md`；围栏权威见 `docs/design/fence.md`。

---

## 0. 当前状态（TL;DR）

- **v1 = 架构走通 + 桌面可演示**（demo-grade，非上线）。
- **距上线三缺口**：① 关键控件（文本线：富文本/输入/字体效果 + 列表强化/进度条/滑块）② 移动平台 ③ 编辑器/AI 工作流闭环。
- **差异化已立**（别丢）：AI 可预测性（HTML-as-DSL，AI 能编辑+预测渲染）+ flexbox（超 fgui Relations）+ Rust 跨引擎共享核心 + 围栏验证器（打包期挡违规）。
- **预览方案定型**：LoomGUI 是 HTML/CSS 子集，外部编辑器（open-design/Chromium）+ 围栏验证器 + 预览可信清单是**长期方案**，非临时兜底——不再自建 WASM 零偏差预览（那是重造已有外部管线）。

---

## 1. v1 已交付

### 1.1 能力清单

**渲染**：贴图 quad + 纯文本 + 硬矩形裁剪（rect mask）；FairyBatching 重排 + 显式 mesh 合并（真 N→1 draw call）；Unity 后端 GameObject 镜像 + DrawState 缓存（MaterialManager）+ 提交。

**文本**：ttf-parser 测量 + unicode-linebreak 断行（砍 rustybuzz 复杂 shaping + unicode-bidi，亚洲/国内首发）；后端据 TextLayout（SOA 三表）生成 mesh；字体用包声明的同一 ttf（一致性根基）。

**事件**：命中（按等效绘制顺序逆序）+ click/hover/leave + 拖拽；多触摸（5 槽）+ CaptureTouch + 拖拽/滚动仲裁（阈值 + 退让）+ 键盘/焦点/Tab；`is_pointer_on_ui` 消费。

**布局**：taffy flexbox（围栏子集）；参考分辨率缩放（MatchWidthOrHeight）；safe-area（异形屏 uniform shrink-to-fit + letterbox）。

**滚动**：ScrollPane：惯性 + 回弹 + 滚动条 + 鼠标滚轮（自维护可变 target tween，不走 GTween）。

**资源**：打包器 `loomgui_pkg`（HTML+CSS+资源→.pkg.bin+图集）；二进制包加载（formatVersion + 迁移器）；图集（散图 shelf 打包）+ refcount。

**FFI**：csbindgen 通路 + SOA+多 arena 渲染树同步（blob v4，18 列）。

**状态/样式**：`:hover/:active/:disabled/:focus`（运行时伪类 + 样式 dirty 重匹配）；cascade 继承（打包期展开）+ 合并 + 出现顺序。

**动态树（v1.3+）**：代际 NodeId + slotmap + 9 个命令式 API（create/remove/move/set_text/set_src/set_style），详见 design §13.1。

### 1.2 v1 围栏冻结子集

> **权威清单 = `docs/design/fence.md`**（真相源 `loomgui_core/tests/fence_contract.rs`）。本节只标 v1 冻结口径，不重复属性表。

- **元素**：`div`(Container) / `span`+裸文本(Text) / `img`(Image) / `button`(Button)。围栏外标签报错（不降级）。**v1.x 设计层也不用 `<l-list>`/`<l-rich>`**——虚拟列表/富文本由代码层做，围栏不暴露，AI 不知道就不会写（design §4.1）。
- **CSS**：布局（flex 全家）+ 视觉（含 v1.x 已实现 `background-image`/`background-size`/`border-radius`/`filter`/`border-image-slice`）+ `transform`/`overflow(+x/y)`/`pointer-events`。值约束 + 围栏外静默忽略项见 fence.md §2。
- **选择器**：标签/类/id/后代/子代/分组 + `:hover/:active/:disabled/:focus`。
- **目标市场**：亚洲/国内首发（决定文本可砍 BiDi）。**平台**：v1 仅 Win/Mac 桌面 + Mono backend（IL2CPP/移动端排 v1.17）。

### 1.3 预览方案（外部编辑器，长期方案）

LoomGUI 是 HTML/CSS 的**子集**，浏览器/open-design 这类外部编辑器天然能渲染它，偏差靠围栏验证器 + 预览可信清单管住即可——**这是长期方案，不自建 WASM 跑核心做零偏差预览**（那是重造已有外部管线）。当前用 **open-design Chromium**：围栏验证器（打包期挡违规）+ Chromium 预览 + head 内联 polyfill（`div{display:flex;flex-direction:column}` + `*{box-sizing:border-box}` + `body{margin:0}`，对齐 LoomGUI 契约：div 总是 flex column）。
- **预览可信清单**（fence.md §6）：flex/gap/color/px/background-image 可信；margin 折叠/文本换行/`position:absolute`/`display:grid`/`@media` 不可信。口径："信围栏规则，别信预览不可信项"。

### 1.4 Unity 胶水任务（G1-G14，已交付）

| # | 任务 |
|---|---|
| G1 | 打包器 `loomgui_pkg` |
| G2 | Stage MonoBehaviour 驱动（唯一 tick 入口） |
| G3 | 根 Stage 挂 Unity（Camera/GameObject + 根 y-flip） |
| G4 | 输入采集→扁平事件→FFI 注入（新旧输入系统 + IME character） |
| G5 | IME/字符输入（v1 最小 PC 键盘字符级） |
| G6 | 字体资源进 Unity + 注册给核心（同一 ttf） |
| G7 | 纹理加载（磁盘→Unity→GPU→TexId） |
| G8 | 坐标翻转（根 Stage 一次性） |
| G9 | GameObject 镜像池 diff（NodeId→GO，slot 复用、Mask 独立、Unchanged） |
| G10 | DrawState 缓存（MaterialManager）+ Image/Text shader |
| G11 | csbindgen 生成 + native lib 构建脚本 |
| G12 | 参考分辨率 Unity 侧落地 |
| G13 | Domain reload / Play mode 重置保护 |
| G14 | 滚动条 Unity 侧渲染 |

### 1.5 v1 验收标准

能演示：① 按钮+文本+图片面板 ② 可滚动容器（惯性+回弹+滚动条）③ 按钮 hover/active 视觉反馈 ④ 自适应分辨率（1080×1920 等比缩放） ⑤ UI 挡住时游戏不响应点击 ⑥ HTML 经打包器产出二进制包加载。

**性能基线**：500 节点静态 UI 每帧无卡顿；冷帧/换页帧（500 节点全 dirty）FFI 拷贝 + arena 解析 ≤ 2ms（v1e dirty hash → Unchanged，静态帧≈0 upload）。

### 1.6 v1 明确不做（推 v1.x 后续编号）

富文本、软裁剪/形状遮罩(paintingMode)、Transition 编排（拆 v1.5-b）、滚动分页/吸附/下拉刷新、IME 完整链路+软键盘、字体 fallback 链、完整 NativeHost、rustybuzz 复杂 shaping+BiDi、IL2CPP+移动端、grid。**均在下方 §2 v1.x 连续编号里各有归属，不再有独立 v2 板块。**

> 注：border-image-slice 九宫格（v1.3 已做）。v1d（滚动/键盘/transform/动画/safe-area）子轮已全交付，明细见 git history + docs/pitfalls.md。

---

## 2. v1.x — 一路做到上线就绪（原 v2 全部并入）

**排期定稿（2026-06-30，2026-07-07 重订）**：v1.x 单一编号，一功能一号，按完成序递增；补丁不占号。首要判据 = AI 可预测性 → 先填"静默忽略"视觉 gap + 绘制质量（低风险快赢），再上线控件。**2026-07-07 重订**：RmlUi/fgui 对标后（见 `rmlui-research.md`）确立**文本线先行**（字体地基→富文本→效果→输入→动画，字体地基有架构依赖必须连着做），控件强化+平台+特效全部并入 v1.x 连续编号，**取消独立 v2 板块**。本机唯一编码机，串行推进。

### 2.1 已交付 / 进行中

| v1.x | 项 | why | 状态 |
|---|---|---|---|
| v1.1 | **background-image**（+坑79 共存视觉补丁）| AI 必写 `background-image:url`；围栏内却零解析，静默忽略=契约违背 | ✅ |
| v1.2 | **border-radius（圆角 mesh）** | AI 必写 CSS；围栏外静默丢弃违背可预测性 | ✅ |
| v1.3 | **ColorFilter + 九宫格 slice + profiling** | 色调统一 + disabled 灰化升级；UI 皮肤缩放不变形；draw call/GC/内存实机达标 | ✅ |
| v1.3+ | **动态树重构（地基）** | v1 static-tree 撞墙。代际 NodeId + slotmap + 动态 API，非功能号 | ✅ 待家里机验 |
| v1.4 | **虚拟化列表 + position:absolute** | 背包/排行榜/邮件必备。建在动态树之上。absolute 解 tips overlay/列表 slot 定位 | 已交付（仅单列，多列/翻页/吸附拆 v1.11） |
| v1.5 | **Controller（纯 CSS 路径）+ transition 动画；Gear 砍；Transition 拆 v1.5-b** | 标签页/弹窗/过场/状态切换必备 | 进行中 |

### 2.2 文本线（v1.6-v1.10，先行）

文本渲染当前"核心测量 / 后端光栅化"是"引擎无关纯核心"立项根基里唯一破例，且是富文本/字体效果的地基。对标后确立**整条文本线一次做透**——地基（字体自绘）→ 富文本 → 效果 → 输入 → 动画。依据见 `rmlui-research.md` §1/§4/§5。

| v1.x | 项 | why | 状态 |
|---|---|---|---|
| v1.6 | **核心自绘字体（地基）** | 推翻"文本 mesh 后端光栅化"例外：ttf-parser outline + ab_glyph 光栅 + etagere 图集（CJK 增量货架/多页）；FFI blob v10 带 atlas 纹理通道；后端降为贴图上传、删 Unity 动态字体。**根治坑 113/119，重开 kerning，text 可合批，补齐跨引擎一致地基**。见 rmlui-research §1。**fontdb fallback 链未做（推后）** | ✅ 代码合并（T1-T9，待家里机 PlayMode 验收） |
| v1.7 | **富文本（简化 inline flow）** | 聊天/物品描述必备。**简化 inline flow（非完整 CSS IFC）+ `display:block` desugar**——AI 写 CSS 标准 HTML，打包器展开成 flex+rich 叶（保 div=flex 铁律）。per-run 多色多字号（建在 v1.6 atlas 顶点色）+ 行内图（含 emoji-图片）+ 超链接（fragment 矩形命中）+ 下划线删除线。measure/build 分离守纯 measure 不变量。字体效果推 v1.8、彩色字形 emoji 推 v1.8+。spec 见 `docs/superpowers/specs/2026-07-07-v1.7-rich-text-design.md` | 进行中（spec 定稿） |
| v1.8 | **文字效果 + 装饰视觉** | 文字：下划线/删除线/阴影/渐变字体（纯几何）+ 描边/发光/blur（核心字形位图 CPU 后处理，v1.6 阶段 c）。装饰：**彩色边框（修 border_color 死字段）**/2 色顶点渐变/sepia 补全/圆角 SDF 裁剪/box-shadow 几何近似/九宫格退化按比例分+接缝取整。AI 必写 gradient/border。DSL 混合：标准 CSS 优先（text-shadow/-webkit-text-stroke/text-decoration/linear-gradient/border/box-shadow/sepia）+ 私有 font-effect（glow/blur，标准 CSS 表达不了）。spec 见 `docs/superpowers/specs/2026-07-08-v1.8-text-effects-decoration-design.md`。**border 四边不等宽 + text-decoration CSS 规则形式推后**（见 §2.7） | ✅ 代码合并（13 task，待家里机 PlayMode 验收） |
| v1.9 | **TextInput / IME（光标/选区/composing）** | 登录/搜索/聊天输入必备。IME 最重。建在 v1.6 精确 metric（kerning 已开）之上 | 待开 |
| v1.10 | **动画 + 滚动手感增强** | **修单矩阵覆盖 bug**（2D 三通道 lerp）+ @keyframes 时间轴 + 补 ease（bounce/elastic/circular/sine…+统一 In/Out 推导）+ iteration/alternate + 颜色 linear 空间插值 + transition transform 通道补全 + 滚轮 smoothscroll + 触摸速度采样（含 scroll padding/content_size 补偿/drag 真实 dt）。见 rmlui-research §4 | 待开 |

### 2.3 控件强化（v1.11-v1.13）

| v1.x | 项 | why | 状态 |
|---|---|---|---|
| v1.11 | **列表强化（driver 层）** | v1.4 只做单列等高，gap 分析指出深度被低估。补 flow 多列（背包网格）/pagination 翻页（商店）/snap 吸附（banner）/loop 循环。核心仍不认识"列表"。见 rmlui-research §5.2 | 待开 |
| v1.12 | **轻量控件 + 变换锚点** | 进度条（血条/加载，可 CSS+driver 拼）+ 滑块（音量/设置，拖拽+值映射）+ **pivot/transform-origin**（当前 transform 绕左上角，旋转/缩放锚点受限，真实短板，升入围栏） | 待开 |
| v1.13 | **DragDrop + Window/Popup（driver 层）** | 背包物品拖到装备栏（跨对象拖放 + drop-target 目标匹配）优先于 window；modal/popup/tooltip driver 层管理。都不进 Rust 核心 | 待开 |

### 2.4 离屏渲染 + 特效 + 几何（v1.14-v1.16，原 v2 特效档）

需 Unity 后端补离屏 RT/stencil 基础设施；SOA blob 单向数据流要新增"离屏组"概念。与"AI 可预测渲染结果"有张力（模糊结果 AI 难精确预测），故排文本线+控件之后。

| v1.x | 项 | why |
|---|---|---|
| v1.14 | **离屏 RT 基础设施** | shape mask / alpha mask / reversedMask / soft clip（羽化）/ paintingMode（cacheAsBitmap）。CommandBuffer + 临时 RT。见 §5.2 草稿 |
| v1.15 | **高级滤镜 + BlendMode** | filter blur / backdrop-filter / 真实 box-shadow / drop-shadow / glow（依赖 v1.14 RT + 分离高斯）+ BlendMode 扩展（Add/Screen/Multiply…，v1 仅 Normal） |
| v1.16 | **几何扩展** | 椭圆 / 多边形 / RadialFill mesh（进度条径向填充）。v1 仅矩形 quad+圆角 |

### 2.5 平台 + 生态（v1.17-v1.19，排最后）

平台移植工作量重、风险未探（当前仅桌面 Mono 验证过），排在功能+特效全做完之后。

| v1.x | 项 | why |
|---|---|---|
| v1.17 | **移动 + IL2CPP + WebGL** | 上线游戏必备平台。IL2CPP struct 对齐/回调坑（blob 已按此设计） |
| v1.18 | **多引擎（Godot）** | 验证跨引擎一致性，Rust 核心共享价值兑现。**字体已在 v1.6 搬进核心 → Godot 直接受益（不必重写文本光栅化）** |
| v1.19 | **多语言 / 异步加载 / 热更新** | 上线运营必备（音效/branch/高分辨率变体） |

### 2.6 毕业里程碑

| 版本 | 定义 |
|---|---|
| **v2.0** | = 上述 v1.6-v1.19 全部完成 = **真正上线就绪**。不是特性堆，是"功能+特效+平台+生态全齐、跨引擎一致性兑现"的毕业标志 |

### 2.7 补丁项（不占功能号，尽快修）

对标挖出的围栏内合法 CSS 却坏/无效的真 bug，违背 AI 可预测性契约，作补丁尽快修（不必等功能号）：

- **border-image-slice `%` 渲染期不 resolve**（`mapping.rs:145` 存 0.25 比例，`mesh.rs:190` 当像素用）→ render 期乘 `src_w/src_h` + 补渲染单测。见 rmlui-research §3.5.1。**v1.8 已修**（resolve_slice_percent + 按比例分 + UV 同步 + 接缝取整）。
- **border_color 死字段**（`resolved.rs:108` 存了 render 零引用）→ 并入 v1.8 彩色边框（补描边渲染即修）。**v1.8 已修**（border_ring 激活死字段）。
- **border 四边不等宽**：v1.8 只做单值四边同宽（`border_width: f32`）。四边不等宽（`border-width: 1px 2px 3px 4px`）推后——扩 `[f32;4]` + border_ring 四边独立几何，排控件/补丁档。
- **text-decoration CSS 规则形式**：v1.8 只 honor inline style（`<span style="text-decoration:underline">`），CSS 规则形式（`.cls { text-decoration:underline }`）静默忽略（解析在 rich.rs apply_inline_style，非 mapping.rs apply_decl）。proper fix = resolved 加 text_decoration 字段 + apply_decl 解析 + build 注入 RichRun.deco + cascade 继承。fence.md 已标 inline-only 限制。
- 完整 defer/债清单见 rmlui-research §6。

---

## 3. v other — 编辑器工作流（独立并行，不阻塞主线）

Unity 内 C# 实现。`LoomSettingsWindow`（`LoomGUI > Settings` 面板）提供工作区 tab：
1. **工作区初始化**：`LoomWorkspaceInitializer` 在目标工作区生成 `config.json` + 注入围栏规则 + skill（从 `Editor Resources` 拷贝模板）。
2. **config.json**：记录工作区根路径 + res 根 + 输出路径；AI harness（Claude Code 等）读取它定位 `loomgui_pkg.exe` + 打包参数。
3. **open-design 导入**：初始化后的工作区直接用 `od project import <workspace>` 导入，AI 在 cwd 读 `CLAUDE.md` + `.claude/skills/` 自动工作。
4. **打包 CLI**：`loomgui_pkg.exe`（落位 `Editor/Tools/`）做验证+打包，`--res-root` 指 res 绝对路径。AI 调它做围栏验证→自纠→产出 `.pkg.bin`。

**预览**：Unity PlayMode 加载 `.pkg.bin` 做真实渲染验收。日常设计预览用外部编辑器（open-design/Chromium，见 §1.3）——LoomGUI 是 HTML/CSS 子集，外部管线天然能渲染，不自建 WASM 预览。

**围栏验证**（单一真相源）：`loomgui_core/tests/fence_contract.rs` 可执行围栏契约。`cargo test -p loomgui_core fence_contract` 是防漂移门。

---

## 5. 机制草稿（v1.x）

> 收留从主设计搬出的 **v1.x 机制草稿**——实现期才该定的细节。主文档只写设计意图 + v1 契约；这些机制等实现验证后"毕业"回主文档。**草稿不是契约**：字段/算法实现时按真实约束调。（原 §4 独立 v2 平台/特效板块已并入 §2 连续编号。）

### 5.1 虚拟化列表：slot 复用模型（v1.4）

核心维护固定数量可视槽（item index → slot_id）：同 slot 这一帧 item5、下一帧 item6，**slot_id 稳定，NodeId 变**。后端 diff 按复用键复用渲染对象——`reuse_key = slot_id`（若非 None）否则 `node_id`。两身份正交：NodeId=逻辑身份（事件/命中），slot_id=渲染复用身份。**核心不变量（防花屏）**：slot 换内容时必发真实 payload 非 Unchanged。

**v1.4-b 实现注（层 B'）**：核心不认识"列表"（无 NodeKind::List），列表=普通 div+slot 子节点。reuse_key 是核心传给后端的通用字段（每节点一个 u32，0=无复用）；"可视槽"由 driver 建（instantiate item + set_reuse_key）。核心加 3 个 FFI 口子（set_content_size / get_scroll_pos / get_node_layout_rect）让 driver 注入 content_size + 读 scroll_pos/layout_rect。不等高全 driver 侧（尺寸补偿），核心零额外改。

### 5.2 Shape mask + 两遍 DFS（v1.14）

RenderNode payload 加 `Mask{shape_ref, mode: MaskMode}`，MaskMode{Write,Content,Erase}。遮罩是跨节点时序意图：核心 DFS 算嵌套深度填 `MaskContext`。两遍 DFS sort_key 规则（防批合越界）：Pass1 按 Write 最小/Content 居中分配；Pass2 `Erase.sort_key = max(子树 Content)+1`。批合重排约束在 `[Write+1, Erase-1]` 内。后端自选：Unity stencil / Godot canvas_group / 软件 alpha mask。soft clip（羽化）、paintingMode（离屏 RT）同期 v1.14。

### 5.3 NativeHost（v1.13）

v1d.3 已做 **NativeHost-lite**（div 占位 + 后端 `BindNativeHost` 跟随 world transform + 显隐 + 排序）。**v1.4-c 修隐藏漏洞**：空 div slot（被 `merge_meshes` 吞）不进 blob → Sync 查不到 → 改 FFI 按 nodeId 查 `world_matrix`/`sort_key`/`visible`（独立于 merge；core 加 `node_sort_keys` 快照，坑 127）。URP Transparent 三件套（`_Surface`+keyword+`ZWrite`）让 3D GO 进 UI 同队列（坑 129）。完整版仍加：尺寸 push（后端 push 给核心 `set_native_host_size`，核心缓存值在 MeasureFunc 返回——避免每帧回调风暴）、hit/clip/所有权/Godot 镜像。管线加 drain 步（set_input 后、tick 前，后端须完成本帧 size push）。

### 5.4 Controller / Transition（v1.5）【Gear 砍】

**Controller**（状态机，纯状态）：`set_selected_index` 改 index + 更新元素 `data-page` 属性 + 派发 onChanged + 置子树 style dirty。页面切换效果通过 CSS 属性选择器（`[data-page]`）+ `transition` 动画实现，无需 Gear 中间层。
**Gear**（已砍）：CSS 属性选择器 + 动态 `set_style` API 取代状态→属性映射。更简洁、AI 更可预测。
**Transition**（时间线=编排器，不自驱）：拆 v1.5-b。纯数据 `items: Vec<TransitionItem>`。Play 翻译成 Tweener 提交 TweenManager。倒放=逆序+start/end 互换+delay 镜像。

### 5.5 文本：v1.x 字段与跨引擎归一化

- **cluster**：v1 不带（无 shaping 时与 glyph 1:1）。v1.x 加 IME/光标/选区时再加，**届时 cluster 语义随 shaping 变**（rustybuzz 后 many-to-one），勿基于 1:1 设计光标。
- **font_id** per-glyph：v1 per-run（单字体）。v1.x emoji fallback 升 per-glyph。
- **跨引擎归一化契约**（Godot 接入时定）：advance/vertical metric Rust 权威（后端禁用引擎 `CharacterInfo.advance`）；引擎字体 API 降为光栅化器；关 hinting。
- **v1 文本简化代价**：emoji→tofu（无 fallback）、组合符号→错位（无 shaping）、RTL 不支持。

### 5.6 包格式：v1.x 演进项

集中式迁移器链（多版本累积后）；`nextPos` 长度前缀 forward-compat（v2 加字段）；branches（多语言）/highResolution（1x/2x/3x）；scaleLevel（MatchWidth/MatchHeight）。v1 当前 formatVersion 8（详见 docs/pitfalls.md §1 包格式）。

### 5.7 契约版本化（待第二个契约版本时定）

主文档不加版本字段——无 v2 契约。将来真有第二版本时：公共头 `contract_version:u32` + `feature_flags:u64` + 可选扩展列（arena 内相对偏移，绝不跨 FFI 传裸指针）。SemVer：加可选=minor，改必选=major。不变量：feature_flags 变化视为 payload 变化（必发真实 payload 非 Unchanged）。

### 5.8 其它 v1.x

- **世界空间 UI**：NodeTransform 加 `Option<VertexMatrix>`（透视/斜切）。
- **DrawState 扩展**：DrawFlags 加 SoftClipped/Masked/AlphaMask/ColorFilter；BlendMode 全 12 种；ProgramId 加 BMFont/自定义。
- **SRP 混合渲染**（Unity）：自绘节点用自定义 SRP RendererFeature 批合。
- **节点类型**：RichText/TextInput/Graph/Loader/MovieClip/Slider/ProgressBar/ComboBox/Tree/NativeHost（内部 NodeKind，**不暴露为 HTML 标签**）。
- **CSS 扩展**：border-radius/filter/border-image-slice/:focus/overflow-x/y/row-gap 等已在 v1 实现，余 v1.x。

### 5.9 借鉴 RmlUi / fgui 对标（跨版本）

> RmlUi（`temp/RmlUi/`，只读，libRocket fork v6.3）+ FairyGUI（`temp/FairyGUI-unity/`，只读）对标调研，完整结论 + file:line 证据见 **`docs/roadmap/rmlui-research.md`**。已核实**不能换 RmlUi 底层**——核心三件套（快照增量 + 核心批合 + 文本只测量）与 RmlUi retained 全量重画/独立 geometry/核心光栅化字形正面冲突，换 = 重新立项，否决。但其纯算法部分可 port，且**字体核心自绘反而是补齐"引擎无关纯核心"破例的正确方向**（v1.6）。

- **已排期进 v1.x**：v1.6 核心自绘字体（推翻文本后端光栅化例外）、v1.7 富文本 IFC、v1.8 文字效果+装饰视觉（彩色边框/2 色渐变/sepia/圆角 SDF 裁剪）、v1.10 动画增强（修单矩阵覆盖 bug + @keyframes + 补 ease + iteration/alternate + 颜色 linear 插值）+ 滚动手感。
- **第二梯队**（实现对应功能时附带）：transition 中断平滑（reverse_adjustment_factor）、单图 background-repeat、radial/conic 渐变 shape 数学、box-shadow 真实版。
- **补丁项**（不占号尽快修）：border-image-slice `%` 不 resolve、border_color 死字段（并入 v1.8）。
- **别抄**：回弹/九宫格圆角共存/批合/ChangeLevel/filter 色矩阵/opacity（LoomGUI 已领先，照抄倒退）；RmlUi 全 atlas 重建（O(N²)，用 etagere 增量替代）；filter blur/drop-shadow/文字描边发光走后端离屏 RT（v1.14+）；RmlUi Euler 滚动模型/分布式动画时钟（破架构不变量）；fgui Relations/Gears/可视化编辑/GTree/BMFont（差异化已替代或游戏罕用）。

---

## 6. 关键决策（why）+ 对标基线

### 6.1 关键决策

- **平台移植排 v1.x 最后（v1.17+）不设独立 v2**：v1.x 聚焦功能必备先行，平台移植工作量重+风险未探（仅桌面 Mono 验证过），排在功能+特效全做完之后。v2.0 = 全部完成的毕业里程碑。
- **编辑器用 open-design 不自建**：复用其插件/对话/导出机制，省自建壳基建。LoomGUI 是 HTML/CSS 子集，外部编辑器天然能预览，不自建 WASM 预览。
- **文本线先行 + 字体搬进核心**：文本"核心测量/后端光栅化"是"引擎无关纯核心"唯一破例，也是富文本/字体效果地基。v1.6 搬进核心补齐破例（接 Godot 直接受益），整条文本线一次做透。
- **shape mask/特效拆分**：border-radius/background-image/soft clip/ColorFilter 进 v1.x 前段（AI 必写不可推 + 配合功能）；离屏 RT 特效（blur/glow/异形 mask/blend）排 v1.14+（PNG 皮肤/九宫格能补大部分，AI 也难精确预测模糊结果）。
- **v other 并行**：编辑器工作流独立于 runtime，不阻塞 v1.x。

### 6.2 对标基线 + 成熟度

- **对标 FairyGUI**：10 年沉淀，跨引擎（Unity/Cocos/UE/Laya），可视化编辑器，30 示例，MIT。LoomGUI 精神继承 + 布局替换（flexbox 代 Relations）。
- **v1 成熟度**：架构完整（FFI/打包/Unity 后端/事件/滚动/动效/状态 全）+ 桌面可演示 + 性能 500 节点静态无卡顿。距上线 = v1.x（功能→特效→平台，一路到 v2.0 毕业）+ v other（编辑器并行）。
- **LoomGUI 差异化**（对标 fgui 的竞争力）：AI 可预测性（HTML-DSL，fgui .fui 二进制 AI 不能编辑）+ flexbox（流式/响应式/内在尺寸，超 fgui 锚点）+ Rust 跨引擎共享核心（fgui 各引擎独立 SDK）+ 围栏验证器（AI 第一道反馈）。
