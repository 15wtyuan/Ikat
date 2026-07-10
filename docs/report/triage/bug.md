# 待办 · Bug（正确性缺陷 · 已核实）

> 2026-07-10 逐条对照当前源码/文档核实，剔除已修与误判。**每条都附核实结论**。
> 行号会漂移，按符号 grep。
> 核实方法：读当前源码取证（非靠报告转述）。文档漂移读当前 main-design.md/fence.md/pitfalls.md。

---

## 一、代码 bug（跑起来会错/会炸）

### FFI / 边界

| 级 | 问题 | 位置 | 核实 |
|---|---|---|---|
| 🔴 | `instantiate` 的 `id_map[pidx].expect("parent built before child")` 在 FFI 可达 pub fn（load_package→instantiate 路径）。corrupt/乱序 pkg 触发 panic → Unity 闪退（违反坑102 no-panic 契约） | stage.rs:613（instantiate 循环内） | ✅ 属实。expect 在 `if let Some(pidx) = tn.parent_idx` 内，pidx 越界或子先于父则 None→panic |
| 🟡 | `unwrap_or("")` 吞非 UTF-8 name：`from_utf8(...).unwrap_or("")` 不 panic（非 no-panic 问题），但非 UTF-8 字节静默变空串继续执行，应返错误码 | loomgui_ffi_c/src/lib.rs:198/225/227/1028/1030/1055/1057/1146/1167/1206/1225（11 处 `name/pkg/comp/kind/css/text/src` 入口） | ✅ 属实但**误标**：`unwrap_or` 不 panic，不是 no-panic 违规；是静默错误行为。降 🟡 |

### 文本 / 富文本

| 级 | 问题 | 位置 | 核实 |
|---|---|---|---|
| 🔴 | `parse_four` 对 `10%`/`1rem` strip `px` 失败 → `parse::<f32>` 失败 → `unwrap_or(0.0)` 静默返 0。padding/margin/gap/border-width 无声消失，`apply_decl` 仍返 true | style/mapping.rs:49-70（闭包 `p` 在 54 行 `strip_suffix("px").unwrap_or(x)`） | ✅ 属实。注意 `parse_four` 用的是内联闭包，非 `parse_px`；`%`/`em`/`rem` 全落 0 |
| 🟠 | `invert(x)` 非标准：`x>=0.5` 全量反相、否则单位阵。`invert(0.3)` 无效（CSS 规定按比例反相） | style/mapping.rs:109-116 | ✅ 属实 |
| 🟡 | 数字 HTML 实体 `&#60;`/`&#x3C;` 未解码：`parse_entity` 只 match 命名 + `&#160;`(nbsp)，其余 `_=>return None` → 字面 `&` 进文本。AI 生成 HTML 常用数字实体 | text/rich.rs:448-461 | ✅ 属实 |
| 🟡 | 伪类解析 `rest.find(':')` 会命中 attr 值内的 `:`：`[href="a:b"]` 的 `:` 被当伪类分隔，选择器可能误解析 | parse/selector.rs:153-159 | ✅ 属实（边缘：attr 值含 `:` 才触发） |
| 🟢 | `letter-spacing: 0.1em` 被 `parse_px` 拒 em 回退 0（`parse_px` 拒 `%`，未处理 `em`/`rem`） | style/mapping.rs:695（→ parse_px:1009） | ✅ 属实（低影响） |

### 输入 / 滚动

| 级 | 问题 | 位置 | 核实 |
|---|---|---|---|
| 🔴 | `touch_id == -1` 双重语义（鼠标主键 + 空闲槽），正确性靠 if-else 顺序，散落 9 处 `== -1` | input.rs:90/98/108/399/424/439/450 等 | ✅ 属实。语义重叠是设计债，重构非纯 bug，保留待重构档；但若顺序误改即错 |
| 🟠 | scroll `begin_bounce` 在 `for ax in 0..2` 循环**外**置 `tweening=2`：单轴 bounce 仍标记双轴 tweening，可能干扰同帧另一轴仍在跑的惯性 | scroll.rs:297 | ✅ 属实 |
| 🟡 | `refresh_content_sizes` 仅 `tweening != 0` 时 clamp scroll_pos：空闲/拖拽态视口缩小后 scroll_pos 越界，到下次 set_pos/wheel 才修，瞬时错渲 | scroll.rs:573-583 | ✅ 属实 |
| 🟡 | `MOVE_CANCEL_PX=50` 鼠标/触屏统一：鼠标移 11–49px 取消 click 但 longpress 仍可能触发 | input.rs:82, 522 | ✅ 属实（设计可辩护，边缘） |
| 🟡 | `DRAG_FOLLOW_ASSUMED_DT=0.016` 占位：`process` 无 dt 参数，30/120fps 速度平滑/惯性错（修需 FFI 加 dt，见 low.md deferred） | input.rs:87, 662 | ✅ 属实（同根推迟项） |
| 🟡 | 滚动物理常量（`DECELERATION_RATE=0.967`/`VELOCITY_SMOOTH=10`）无 dt 补偿，帧率偏离 60 漂移 | scroll.rs:36, 130 | ✅ 属实（同上） |
| 🟡 | loomgui_pkg CLI 缺值静默：`--html` 缺值→`unwrap_or_default()`=""→空列表；`--html --res-root foo` 会把 `--res-root` 当 --html 的值 | loomgui_pkg/src/main.rs:29, 39 | ✅ 属实 |

### 渲染 / 场景

| 级 | 问题 | 位置 | 核实 |
|---|---|---|---|
| 🟡 | `payload_hash`/`header_hash` 都不含 `parent_id`：节点同 node_id 换父 → ChangeLevel=Skip → C# MirrorPool 不 re-parent。未测 | render/dirty.rs:12（payload_hash）/ 65（header_hash） | ✅ 属实（latent：运行时换父罕见，未测） |
| 🟡 | `merge_batch` 合并节点硬编码 `color_matrix:[0.0;20]`（6 处），靠"可合并 program=0/1 节点绝不带 filter"隐式不变量兜着，`mesh_key` 不含 color_matrix | render/merge.rs:129/172/267/277/303/313 | ✅ 属实（latent：若破不变量则丢 filter） |
| 🟡 | `create_node` 后 slotmap 扩容，scene 级并行数组 `text_layouts`/`rich_fragments`（按 `nodes.capacity()+1` 预分配，见 node.rs:378/408）未同步扩容，新 index 越界；`.get` 返 None 重测兜底掩盖 | scene/dynamic.rs:97（create_node insert）；scene/node.rs:378-379/408-409 | ✅ 属实（已被 .get None 兜底，低影响） |

### Unity 后端

| 级 | 问题 | 位置 | 核实 |
|---|---|---|---|
| 🔴 | `PackPackage` 拼命令行时 `--html` 的值（`string.Join(",", htmlFiles)`）**未加引号转义**：HTML 文件名含空格/引号即破坏参数配对、exe 调崩。absSrc/resRoot/outPath 已加引号，唯独 htmlArg 漏 | LoomSettingsWindow.cs:358-361（htmlArg）/ 367（psi） | ✅ 属实 |
| 🟡 | `_parentCache` 仅 SetHandle 时清，运行时增删节点后返过期父 | LoomEventHandler.cs:139, 319 | ⚠️ 未直接核实（按报告保留，待 TDD 时取证） |

---

## 二、文档漂移（doc vs code · 已核实仍存在的）

> 核实结论：**bug.md 原列的文档漂移大半已修**（main-design §5.2/§5.3/§14/§3.2/§8.1/§10.2/§10.4、pitfalls:19、fence.md row-gap/overflow/display:grid 全已对齐——见下方剔除清单）。仅以下仍存：

| 级 | 问题 | 位置 | 核实 |
|---|---|---|---|
| 🟡 | fence.md 标【实证】却 fence_contract.rs 无对应测试：`align-self`/`flex-grow`/`flex-shrink`/`flex-basis`/`min-width`/`min-height`/`max-width`/`max-height`/`font-family`/`line-height`/`letter-spacing`/`overflow-x`/`overflow-y`。`supported_layout_props_return_true`/`supported_visual_props_return_true` 两测试的 case 列表不含这些 | fence.md §2.1/§2.2 ↔ tests/fence_contract.rs:40/64 | ✅ 属实。约 13 项标【实证】但仅在 case 表外。fix：补进 case 表（lock apply_decl 返 true）或降标【推断·待测】 |
| 🟡 | `check_no_flex_props` 只拒 justify/align/gap，`flex-direction`/`flex-wrap` 在 block div 静默通过；fence.md §2.5 却称"desugar 拒收 block div 上的 flex 属性" | loomgui_pkg/src/lib.rs:345-362 ↔ fence.md:144 | ✅ 属实。文档口径宽于代码 |
| 🟢 | CLAUDE.md tick 时序摘要漏 transition-drain 步骤（rematch 后、solve 前，承重）。stage.rs doc 与 main-design §14 都有，唯独 CLAUDE.md 漏 | CLAUDE.md 架构段 tick 序列 ↔ stage.rs:677/729-734 | ✅ 属实（原 🟢 误说 stage.rs doc 也漏，实则 stage.rs doc 有 ⑥.5） |
| 🟢 | CLAUDE.md FFI 段 Marshal.PtrToStructure 写"禁"过强：代码桌面 Mono 在用且有 IL2CPP 待换注释 | CLAUDE.md FFI 段 | ⚠️ 属实（措辞，低） |
| 🟢 | `base_style` 注释说"打包期产物（不变）"，但 `set_style` 直接改它 | scene/node.rs:105 附近 | ⚠️ 待核实 |
| 🟢 | `RelativeFromWorkspace` 注释硬编码"深度 2"假设，实际用 `Uri.MakeRelativeUri` 与深度无关 | LoomConfigExporter.cs:63 | ✅ 属实（cosmetic 注释漂移） |

---

## 三、已剔除（核实为 stale / 已修 / 误判）

> 这些原在 bug.md，核实后确认**不再是问题**。列此备查，防再列入。

| 原条目 | 剔除理由（核实取证） |
|---|---|
| 🔴 `render_json` `.unwrap()` 违反 no-panic | **误判**：render_json 仅被测试（stage/tests.rs:133/148、snapshot.rs:64/81/102）调用，**ffi_c 全无引用**（grep 0 match）。非 FFI 可达路径，no-panic 契约不适用。`.unwrap()` 在测试中可接受 |
| 🟠 PointerEvent/KeyEvent 缺 ABI 尺寸断言 | **误判**：abi_tests.rs:337/600/779 有 `size_of::<PointerEvent>()==16`、:777 `size_of::<KeyEvent>()==8`、:1148 WheelEvent 16B。三者都有断言 |
| 🔴 main-design §5.2 NodeKind 列 16 种 | **已修**：§5.2（main-design.md:163-179）现列 5 变体 + 历史说明"本栏曾列 List/ComboBox/...从未实现为 NodeKind 变体" |
| 🔴 main-design §5.3 Node 字段差七成 | **已修**：§5.3（:181-202）字段与 scene/node.rs::Node 一致 |
| 🔴 main-design §14 tick 时序与代码矛盾 | **已修**：§14（:623-648）顺序 = rematch→transition drain→solve→refresh_content→compute_world→build，与 stage.rs:728-760 一致；:645 明示"rematch 在 solve/compute 前" |
| 🟠 main-design §3.2 position:relative 过时 | **已修**：§3.2:86 现"v1.4-b 起已纳入显式映射" |
| 🟠 main-design §10.2 Transition 标未实现 | **已修**：§10.2:446 现"v1.5 已实现" |
| 🟠 main-design §10.4 Gear 无废弃标记 | **已修**：§10.4:453-455 有【砍】blockquote 废弃标记 |
| 🟠 main-design §8.1 后端生成 mesh 过时/无作废 | **已修**：§8.1:374 有"实测勘误（v1.6 后实现 ≠ 上述早期设计）"blockquote；:358 有 v1.6 前瞻注 |
| 🟠 main-design §11 自述草稿却处核心章节 / §11.4 TextureView | **已标注**：§11:466 有"章节状态：§11.2/§11.4/§11.5 仍是早期设计草稿...以代码为准"。非隐藏漂移，已知待重写 |
| 🟠 pitfalls.md:19 不映射 CSS position | **已修**：pitfalls.md:19 现"v1.4-b 起 position:relative 已纳入显式映射" |
| 🟠 fence.md row-gap/column-gap 标"待测" | **已修**：fence.md:56 现"❌ 不支持（仅 gap 简写）" |
| 🟠 fence.md overflow 文档/代码矛盾 | **已修**：fence.md:84 现明示"无效值如 bogus 不改变字段但仍返回 true" |
| 🟠 fence.md display:grid 陷阱未明确 | **已修**：fence.md:121-122 现有【实证】+ ⚠️ AI 陷阱说明（Chromium grid→打包不报错→Unity Flex 三地各异） |
| 🟢 stage.rs doc 漏 transition-drain | **误判**：stage.rs:677 doc 有"⑥.5 transition drain"。仅 CLAUDE.md 漏（见上表保留） |

---

## 建议修复次序（TDD）

1. **instantiate `parent_idx.expect`**（🔴 FFI no-panic）——FFI 可达 panic，直接违反坑102。修：None 时 skip 该子节点或返错误码，不 panic。
2. **PackPackage `--html` 值未转义**（🔴 editor）——文件名含空格即崩。修：每个 html 文件单独加引号或用 ArgumentList 风格。
3. **parse_four 静默吞 %/em/rem**（🔴 AI 可预测性）——AI 写 `padding:10%` 期望间距在。修：要么支持 %（如 parse_px 已拒 %，一致），要么 apply_decl 返 false 让围栏拒；现状返 true 是说谎。TDD：先写 `padding:10%` 应返 false（或应解析）的失败测试。
4. **fence.md 【实证】无测试**（🟡 文档漂移）——补 fence_contract.rs case 表，把 13 项加进 `supported_*_return_true`。
5. 其余 🟡 按子系统推（scroll 物理常量/dirty hash parent_id/merge color_matrix/create_node 并行数组）。
6. unwrap_or("") UTF-8（🟡）——改返错误码或至少 log，非 no-panic 但属静默错误。

> 注：touch_id==-1 双语义、input 上帝方法等结构债移至 refactor.md；dt 帧率问题进 low.md deferred。
