# 待办 · 低优先级（杂项 / 性能 / 测试缺口 / 已知推迟）

> 来源：`docs/report/review-*.md`，剔除已修项。这批**不急**：cosmetic、性能非热点、测试覆盖缺口、或报告自荐"维持现状/推迟"。
> 含三类：(1) **deferred**——已知未来工作/roadmap 项，非当前缺陷；(2) **性能/GC**——非热点，profile 命中再动；(3) **测试缺口/脆弱** + cosmetic。
> 行号会漂移，按符号 grep。

---

## 已知推迟（deferred · 非当前 bug，触发条件到了再做）

| 项 | 说明 | 位置 |
|---|---|---|
| Marshal.PtrToStructure → Span+BinaryPrimitives | 桌面 Mono 当前在用且 OK，**IL2CPP 移动端上线前**须换（struct 对齐坑） | LoomEventHandler.cs:205,262 |
| 缺 package unload API | 无 `unload_package`/`clear_packages`，切场景时 packages + image_sizes 累积 | stage.rs:26 附近 |
| Atlas cache 无失效路径 | `_atlasCache` 永持 SpriteAtlas，未来 atlas 热重载需 `InvalidateAtlas(name)` | SpriteResolver.cs:130 附近 |
| NativeHost 每 bound 节点每帧 3 次 FFI | get_visible/world_matrix/sort_key 分开调，bound 节点增多时合并或保留列 | NativeHostManager.cs:189 附近 |
| VirtualListDriver O(n) 每帧 | FindFirst/Last/Sum 线性，demo 200 项无碍，万级需前缀和+二分 | LoomShowcaseDriver.cs:1176 附近 |
| 帧率依赖常量（input/scroll） | 见 bug.md——修需 FFI 加 dt 参数，属同根推迟项 | input.rs / scroll.rs |
| rematch_pseudo_classes O(N×M×depth) | 规则 10–100 可接受，数百时按 tag/class/id 预建索引 | dynamic.rs:304 附近 |
| sepia 退化为 grayscale | fence.md 已标已知 deferred | style/mapping.rs:117 附近 |
| 扩展 CJK（CJK-B/C/D/E/F）逐字换行 | 可能映 AL class 不逐字，有报告再修 | text/layout.rs:420 附近 |
| AssetPostprocessor 只处理 .png | 规范定 PNG 故当前非问题，jpg/tga 等未来格式不覆盖 | LoomWorkspaceAssetPostprocessor.cs:23 |
| DistributeSkill 无条件覆盖 skill 文件 | 当前设计意图（框架分发），未来允许定制需加版本机制 | LoomWorkspaceInitializer.cs:66 附近 |
| bench inline CSS 重复 500 div | 真实场景用 class selector，代表性差 | frame_emit.rs:36 附近 |
| reorder_for_batching 每帧新 Vec | <4KB/帧，profile 命中再说 | render/batch.rs:246 附近 |
| propagate_text_sub_page_sort_keys O(k*n) | k≪n 实践可忽略 | render/mod.rs:675 附近 |

## 性能 / GC（非热点）

| 项 | 说明 | 位置 |
|---|---|---|
| `ReadPath` 每节点每次线性扫 path table | 共享 path_idx 的节点可 Dictionary 缓存（仿 CLIP 表） | FrameBlob.cs:118 附近 |
| `AncestorChain` 每事件 alloc 新 `List<uint>` | 高事件量下复用 cleared list 省 GC | LoomEventHandler.cs:309 附近 |
| `SyncFontAtlas` 每 dirty page 新 Texture2D | `tex.Reinitialize(w,h)` 复用实例省 GC | LoomStage.cs:230 附近 |
| `RemapMeshUvToSprite` 每次 alloc `List<Vector2>` | 类级复用 list（仿 VList/UvList） | MirrorPool.cs:281 附近 |
| `used_touch_ids` Vec::contains O(n)/槽 ~O(n²) | 5 槽几十事件非热点 | input.rs:464 附近 |
| `process_keys` 每帧重建 Tab chain | DFS+sort 每帧，节点/频率涨时成瓶颈 | input.rs:348 附近 |
| `hit_scrollbar_grip` 每次 hit_test O(N) 全节点扫 | 维护 active_scroll_containers 列表 | hit.rs:25 附近 |
| `compute_world_transforms` rec 每层 clone children | 数千节点内可忽略 | scene/transform.rs:61 附近 |
| Atlas dirty-page 跟踪 O(n) Vec::contains | 改 HashSet 当页数涨 | text/atlas.rs:137 附近 |
| Atlas page 增长无 MAX_PAGES 上限 | 当前不触发 | text/atlas.rs:209 附近 |

## 测试缺口（行为正确，缺覆盖）

| 项 | 说明 | 位置 |
|---|---|---|
| 无 EVT_KEY_UP 测试 | process_keys 的 is_down=false 路径零覆盖 | input/tests.rs |
| cancel_click 仅测 mouse（touch_id=-1） | 找槽逻辑的触摸分支 + 无效 touch_id no-op 未覆盖 | input/tests.rs:1663 附近 |
| scroll 触摸阈值（SCROLL_THRESHOLD_TOUCH=20）测试全缺 | 7 个 scroll 仲裁测试全用 mouse=8，移动端核心场景未测 | input/tests.rs:2950 附近 |
| 阈值常量硬编码于测试 | 源码 const 私有，改常量则测试不报警 | input/tests.rs 多处 |
| v1e_dirty 缺 ChangeLevel::Header 测试 | 只测 Skip/Full，Header（transform/opacity 动画优化路径）无覆盖 | tests/v1e_dirty.rs |
| mapping 字段级断言不足 | ~17/30 属性只验 apply_decl 返 true，未验写进 ResolvedStyle 的值 | style/mapping/tests.rs |
| build_render_nodes 缺直接单测 | sort_keys buffer / parent_id 填充 / 全 display:none / 混 kind 边界无断言 | render/tests.rs |
| PointerEvent/KeyEvent ABI 尺寸断言 | 见 bug.md 🟠（防 field-add 静默错位） | input.rs |
| scroll_up_starts_inertia 测试名与断言不符 | 只验字段清未断 tweening>0，不可靠 | input/tests.rs:3203 附近 |
| LoomStageDriverTests 只测"Awake 不抛" | 字体注册/Tick/OnDestroy/pkg.bin 加载未覆盖 | LoomStageDriverTests.cs |
| 缺 display:none 布局行为 + overflow:scroll flex_shrink 测试 | 7 个列布局/Image 三档测试无这两项 | layout 测试 |
| parse_tests 缺 display:none build_scene 验证 | scene/node/parse_tests.rs |
| CoordMath round-trip 缺屏幕四角/safe-area 边缘 | 只验中心↔中心 | CoordMathTests.cs |
| node_sort_keys 仅测两兄弟 | 缺深层嵌套/多子树/reuse_key 场景 | tests/node_sort_keys.rs |
| color_matrix 缺非零 round-trip | FrameBlobTests.cs:110 附近 |
| PkgManifestReader 缺 0 宽高/空路径/特殊字符边界 | PkgManifestReaderTests.cs |
| 双击窗口缺 350ms 边界测试（0.34/0.35） | 现测 0.2/0.4 内部值 | input/tests.rs:1475 附近 |

## 测试脆弱性 / 冗余

| 项 | 说明 | 位置 |
|---|---|---|
| down_leaf_destroyed 依赖 from_nodes NodeId 逐增 | slotmap 改分配策略则失效 | input/tests.rs:1350 附近 |
| grip_scroll_scene 硬编码 thumb 坐标 (96,25) | 改 scroll 公式则不再命中 | input/tests.rs:3431 附近 |
| snap 文件含 slotmap NodeId 绝对值 | 频繁漂移则考虑 render_json 输出相对 id | tests/snapshots/*.snap |
| LoomConfigExporterTests 硬编码期望相对路径 | 目录结构变则被测代码对但测试挂 | LoomConfigExporterTests.cs |
| _parentCache 既有 stale 风险（见 bug）也脆 | FFI 查询便宜，缓存可能整体不值 | LoomEventHandler.cs |
| 测试直接 poke `TweenManager.tweens` 内部 | Vec→HashMap 重构会断 | stage/tests.rs:685 附近 |
| LoomEventTypes.cs 桩跳号（LongPress=9 后 KeyDown=12） | 须核实与生产定义一致 | tests/dotnet/Stubs/LoomEventTypes.cs |
| V10Header helper 缺 columnData 长度防御 | 长度不符会写错偏移 | FrameBlobTests.cs:36 附近 |
| ControllerChangedEvent ABI 尺寸测试重复两处 | scene/node/tests.rs:558 + 615 |
| drag_threshold 内联 4 场景，失败不显子用例 | mouse 2/3 + touch 10/11 挤一函数 | input/tests.rs:1978 附近 |
| 滚动断言仅验方向不验精确增量 | 公式变速度偏差无报警 | input/tests.rs:3155 附近 |

## cosmetic / 命名 / 文档杂项

| 项 | 说明 | 位置 |
|---|---|---|
| loomgui_pkg example `required-features` 多数未声明 | 8/11 example auto-discovered 未声明 parse；**仅影响孤立 `cargo build -p loomgui_core --no-default-features`**（workspace 模式靠 ffi_c/pkg 依赖统一拉回 parse，CI 不触发）| loomgui_core/Cargo.toml |
| dump_*.rs pkg.bin 路径硬编码（concat! CARGO_MANIFEST_DIR） | 诊断 example 依赖 repo 目录结构 | dump_text/scroll/img/bg.rs |
| dump_img.rs 功能过时（遍历 packages 字典非 scene） | 看不到实例化后布局，T5 TODO 未完 | dump_img.rs |
| dump_*.rs 注释含坑号（坑 127 / 坑 131-133） | 违反 CLAUDE.md「坑号只在 pitfalls.md」 | dump_nativehost_slot.rs:127 / dump_controller.rs:127 |
| unwrap_or_else(|e| panic!) 可简化为 expect | 风格不统一 | dump_nativehost_slot.rs:35 等 |
| bench font_path() 返回冗余 usize + format! | 调用方只用 .0 | frame_emit.rs:15 附近 |
| dump_rich.rs Box::leak 无原因注释 | 故意泄漏取 'static | dump_rich.rs:18 |
| 64×64 图像兜底魔数未常量化 | 散落代码与测试 | layout/mod.rs:170 附近 |
| `src_size` fallback `64.0` 未命名常量 | DEFAULT_SRC_SIZE 利 grep | render/mod.rs:30 附近 |
| Leaf 节点不验证 children 为空 | 正常流程不触发 unwrap | layout/mod.rs:188 附近 |
| Empty img src 静默产空串 | 渲染空，应 skip 或 warn | text/rich.rs:218 附近 |
| read_package 忽略 trailing bytes | 无 pos==len 检查 | asset/mod.rs:476 附近 |
| root_w/root_h 不写入 package | design-size 契约靠 Stage | asset/mod.rs:397 附近 |
| is_inline_display_block 子串匹配可误触 | parse/dom.rs:171 附近 |
| 相邻文本节点用空格连接（newline 语义塌缩） | parse/dom.rs:109 附近 |
| 围栏外 attr 运算符（~=,^=,$=,*=,|=）降级为 Exists | parse/selector.rs:190 附近 |
| filter `split_whitespace` 依赖函数间空格 | `grayscale(1)brightness(1.2)` 解析错但不报 | style/mapping.rs:82 附近 |
| 无 CRC/integrity 校验 pkg.bin | 可信构建期输入，corrupt pkg = packager bug 非攻击 | asset/mod.rs |
| component_count/string_count 无上限校验 | corrupt pkg 致 OOM（可信输入下低风险） | asset/mod.rs:491 附近 |
| `loomgui_shutdown` 空体 + Font Box::leak | 有意 trade-off，Stage 反复建/销（Domain Reload）才累积 | loomgui_ffi_c/src/lib.rs:925 附近 |
| `_pkg_name` 参数未用 | pkg.bin header 无包名字段，保持 `_` 即可 | loomgui_pkg/src/lib.rs:318 附近 |
| `pointer_event_event_record_sizeof` 测试名拼写（point→pointer） | grep 漏 | abi_tests.rs:243 附近 |
| `dump_scene` 每次重新分配 CString | 调试路径，循环频繁 dump 才抖动 | loomgui_ffi_c/src/lib.rs:285 附近 |
| `strip_style_and_link` 手写 HTML 序列化 | 报告自荐维持现状（已测、省正则开销） | loomgui_pkg/src/lib.rs:202 附近 |
| SetLayerRecursive 把 GO 自身 layer 设两次 | GetComponentsInChildren(true) 含自身，无害冗余 | NativeHostManager.cs:60 附近 |
| CollectKeys 40-key 白名单 | 业务要非修饰键（F1-F12）须扩 | LoomInputCollector.cs:198 附近 |
| Showcase 根视口 1080×1920 硬编码 | demo 合理，作参考实现易被照抄 | LoomShowcaseDriver.cs:162 附近 |
| RichText link_id 硬编码为 1 | 依赖 pkg 内部 markup 约定 | LoomShowcaseDriver.cs:505 附近 |
| FinishMeasure 隐式依赖 Update→LateUpdate 时序 | 读 layout_rect 注释未说明 | LoomShowcaseDriver.cs:1086 附近 |
| 日志缓冲从头部截 4000 字符 | 可能切断 UTF-16 代理对、丢关键报错 | LoomSettingsWindow.cs:567 附近 |
| 工作区初始化 WriteAllText 非原子 | 快速连续初始化有竞争 | LoomWorkspaceInitializer.cs:48 附近 |
| NodeBlock 截断包诊断信息不足 | PkgManifestException 只含字节偏移 | PkgManifestReader.cs:88 附近 |
| totalNodes ulong 无总量上限 | 恶意 pkg 致循环超时（非崩溃） | PkgManifestReader.cs:84 附近 |
| pkgOutputDir+"ui/" 拼接依赖 trailing slash | 默认恰好有 slash 但无显式保证 | LoomConfigExporter.cs:22 |
| GUIUtility.ExitGUI() 在 BeginHorizontal/Vertical 内反模式 | EndH/V 永不执行 | LoomSettingsWindow.cs:162 等 |

---

## 备注

- **deferred 段**多数已在 roadmap / CLAUDE.md / fence.md 标注，这里集中索引防遗忘。
- **测试缺口段**与 bug.md 的 🟡 互补：bug.md 是"行为可能错"，这里是"行为可能对但没测护着"。
- **cosmetic 段**整段可长期搁置；挑着改即可，不必批量。
