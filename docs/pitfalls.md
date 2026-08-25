# LoomGUI 踩坑记录

> 只收**可复用规则**：依赖/平台不讲理的事实、跨层动态契约——看代码看不出来的才配进这里。
> 新坑按主题归位、不编号；bug 编年史不记（代码 + git history 是载体）。
> 历史 231 条全文见 [archive/pitfalls-2026-08.md](archive/pitfalls-2026-08.md)（v0.x–v1.x 纪元，只读）。

## 1. 依赖 API 适配

> plan/草稿写的 API 常与 crate 实际不符——遇编译错按 crate 实际源码调（`~/.cargo/registry/src/<crate>-<ver>/src/`），勿硬改依赖版本。

### taffy 0.12（core/src/layout）
- **trait 对象模式，无 `MeasureFunc` 枚举**：`TaffyTree<NodeContext>` + `new_leaf_with_context(style, ctx)` 存 owned 测量上下文，单个 `compute_layout_with_measure(root, avail, FnMut)` 闭包按 `Option<&mut NodeContext>` 分派 Text/Image 测量。
- 测量闭包是 `FnMut` 非 `'static`——闭包内借 `&FontTable` 合法，不需要 Arc。
- **`LengthPercentage` 是 tagged pointer**（`pub struct(CompactLength)`，内字段私有无法 match 变体）——解构用 `into_raw()` + tag 位判 Length 分支。
- `Style` 无 `order` 字段——flex order 排序做不了，渲染按 DOM 顺序。
- **`overflow ≠ Visible` 必须显式设置**：flex 自动 min-size=0 只对非 Visible 生效——不设则容器被 content min-content 撑开，scroll overlap=0 失效。
- **auto-min 是裸 min-content，不实现 CSS 的 specified-size suggestion**：空/无内容容器 min-content=0 → 溢出的 flex 行会把**显式定尺寸的兄弟**也按比例挤扁（76px 顶栏被压成 50px；浏览器以声明尺寸为地板）。近似修法（core layout build 已做）：`size` 为 Length 且 `min_size` 为 Auto 时把 Length 复制进 min——作者显式 min 声明永远赢。
- `serde` feature 可整体序列化 `Style`（pkg 格式依赖它）；bincode 编码随 taffy/bincode 版本走——升级依赖必 bump `PKG_FORMAT_VERSION`（见 §2）。
- **行为怪癖**：① span 显式 flex + padding + 文字子 → flex 容器不做文本测量，宽度退化为 padding 值；② 测量不能只看 `known_dimensions`，须结合 `available_space`（否则定宽容器内文本不换行）；③ 某些 sizing 轮次传 `Definite(0)`——首个 0 宽测量若被当最终结果钉死，文字会竖排。
- **Block 流不实现 gap**（flex 才读 `row_gap`）——block ul 的 spacer 间无 gap，可见区计算盲扣会让 spacer 偏矮、滚动条失真；flex 容器的 gap 必须计入可见区累积位，漏计 = 视口顶部空白。
- **增量 API 事实**（持久树复用必备）：`set_node_context` 存在（换 ctx + 标脏）；`set_children` 自带从旧父摘挂 + `mark_dirty`；`remove` 只摘自身、**子节点留孤儿滞留树内**（删子树须逐节点 remove）；`mark_dirty` 递归上溯祖先（已脏早退）；`Style`/节点上下文可 `PartialEq` 值比较短路 set；`children()` 返回 `Vec` clone（比较别怕贵）；compute 对干净子树跳过（布局缓存按节点粒度）。

### ttf-parser 0.20（core/src/text）
- kerning 在 `face.tables().kern.subtables` 遍历（取 horizontal 非状态机子表），`.glyphs_kerning(GlyphId, GlyphId) -> Option<i16>`。
- `glyph_hor_advance(GlyphId) -> Option<u16>`（无旧名 `glyph_advance_width`）。
- `.ttc`（TrueType Collection）`Face::parse` 第二参 = collection index，index 0 未必是目标 face。

### slotmap 1.1（core/src/scene）
- `new_key_type!` 生成的 Key 是 `KeyData { idx: u32, version: NonZeroU32 }` **64bit**，且字段私有不能自定位型——NodeId 保持手写 `pub struct NodeId(pub u64)`（位型 idx:32+gen:24+tag:8，tag 字节归渲染合成 id 命名空间）+ `from_key/to_key` 桥接，勿改用 new_key_type!。
- idx 从 1 起（0 是 sentinel slot）；version 恒奇 = occupied；`capacity()` 是总槽位且 remove 不缩——并行数组按 `capacity()+1` 分配才不越界。

### csbindgen 1（双扫描点：ffi/build.rs + xtask/src/bindings.rs）
- **两处独立 csbindgen 扫描清单互为镜像**——`input_extern_file` 只显式列文件，FFI 函数挪进新模块必须两处同步补，漏一处绑定**静默缺函数**（编译全绿，运行时 EntryNotFound 才炸）。
- 默认生成 `internal` 类型——跨程序集访问须 `[assembly: InternalsVisibleTo]`。
- 类型映射：opaque `*mut T` → `T*`（类型化指针非 IntPtr）；`csharp_use_function_pointer(false)` 切 Mono 模式。
- C# `fixed (T* p = &localVar)` 非法（CS0213 already fixed）——`fixed` 只 pin 托管对象（数组/string），局部变量直接取址。
- **FFI enum 出口用 return-code + out-param，勿用 0 当「不存在」哨兵**——首变体判别值 = 0（如 NodeKind::Container），会与合法值相撞、无法区分。

### Unity Input System 1.19（LoomInputCollector.cs）
- 双路径 `#if ENABLE_INPUT_SYSTEM`（`Mouse.current...` 新 API）/ else 旧 `UnityEngine.Input`；asmdef 引用名是 `Unity.InputSystem`（非 `UnityEngine.InputSystemModule`）。

### image 0.25（packer）
- PNG 编码到内存用 `RgbaImage::write_to(&mut Cursor<Vec<u8>>, ImageFormat::Png)`（无 `save_buffer_to_memory` 这个 API）。

### unicode-linebreak 0.1（core/src/text）
- `linebreaks() -> impl Iterator`（非 Vec）；枚举名 `BreakOpportunity`；返回 **byte offset**（非 char index）；在空白**后**断 → 行首无多余空格。

### Tauri 2（packer/gui 前端）
- `onDragDropEvent` 的 `position` 是物理像素，须除 `devicePixelRatio` 才是 CSS 逻辑坐标；payload 字段名 `type`/`paths`/`position`。

## 2. 跨层闭环规则

### pkg 格式 bump 代价链
改任何进 `.pkg.bin` 的序列化布局（ResolvedStyle、ControlInit、bincode 结构）→ **必 bump `PKG_FORMAT_VERSION`**（含 MIN/MAX + mod.rs 顶部 changelog 注释）。bump 的代价链（v42 全程实录）：① `core/asset/tests.rs` 有 4 处钉死版本号的断言要同步升；② 重打 14 个 fixtures（13 个 `tests/dotnet/.../fixtures/*.workspace` + showcase 直出 `Assets/Bundles`）并拷回 `*.pkg.bin`；③ `packer/pkg/tests/schema_lock.rs` 用失败信息里的新哈希更新 `LOCKED_HASH`；④ golden 事件流 `LOOMGUI_UPDATE_GOLDEN=1 cargo test -p loomgui_ffi_c --lib golden` 再生成；⑤ C# `GoldenEventsAndAbiLayoutTests` 的 `REC` 常量 + 尺寸断言同步；⑥ 重编 .dll + 重出双 exe。漏一环就版本错配（stale pkg / loader rc=-1 / 「tag for enum is not valid」），且常在离改动最远的 consumer 测试才炸——文本 merge 干净 + cargo 全绿 ≠ C# 测试绿。

### 机制设计/删除前置检查
- **设计渲染合成机制前先读对端 shader 能力**：曾设计整套合成 RenderNode 机制，被一次 shader 阅读推翻——对端早已做 source-over 合成。core program 编号 ↔ Unity shader 能力是跨层闭环，先核对两端现状再设计。
- **删「看似单用途」机制前先 grep 共用者**：曾视作单用途的 flag 实为两条路径共用（div box-shadow 与文字效果层），grep 取证救过一次静默错渲染。
- **fence 委托 + core 解析永真 = 围栏静默放行破损**：fence 零自有校验、委托 core `apply_decl` 的 CSS 属性，core 解析失败必须返 false，否则 `FenceBadCssValue` 链路整体失效——「fence 放行 + core 渲染坏」可同时成立、打包不报错。
- **core 反向调宿主服务须启动期注册函数指针对**：core 是 cdylib，不能 extern 调宿主符号（链接期不可解析 + C# 给不出 linkable C 符号）——剪贴板/原生弹窗/系统字体查询都走注册回调模式；内存契约：get 缓冲区宿主持有（活到下次 get），core 立即拷贝、不跨分配器 free。
- **快照测试锁 glyph 度量必须钉仓库内字体**：不同 OS 默认字体（Win arial vs Linux DejaVu）度量漂移、Linux CI 无 arial——fixtures 字体入库是前提，不是优化。

## 3. Unity 平台特性

- **非纯平移节点的 renderer.bounds ≠ 真实视觉 AABB**：rotate/scale 走 `_ObjM` shader 矩阵、GO 只带平移分量 → Unity 剔除/拾取看到的 bounds = GO 平移 × 未旋转 mesh。任何新消费 renderer.bounds 的功能（剔除/遮挡/视口判定）都须过 `MirrorPool.CompensateMeshBoundsForLinear`（bounds 置线性矩阵 × 顶点 AABB）；否则旋转节点滚动/移动中被错误剔除（#66 实锤）。
- **`git tag` 输出按字典序**：`v0.0.10` 排在 `v0.0.5` 之前——`git tag | tail` 会漏最新版本号、误判「未发过版」。查 tag 存在性用 `git tag | grep <精确版本>` 或 `git ls-remote --tags origin | grep`。

- **EditMode 禁 `Object.Destroy`**（须 `DestroyImmediate`）；Mesh 是独立 Object，GO 销毁不连带——`[ExecuteAlways]` 路径须显式销毁防泄漏。
- **`.meta` 须入库**，且 Unity 关着时不生成（新增 .cs 要启动 Unity 才产 .meta）——提代码漏 .meta，别人打开工程全断链。
- **`Resources.Load` 不搜 `Editor/Resources/`**（那是 `AssetDatabase.LoadAssetAtPath` 专用，后者要含扩展名全路径）；`.md`/`.html` 在 Unity 里是 DefaultAsset 非 TextAsset。
- **ScriptableObject 禁 `new`** → `CreateInstance<T>()`（`new` 绕过原生对象追踪，IL2CPP 静默失败或产损坏资产）。
- **shader keyword 须 `multi_compile` 非 `shader_feature`**——未启用的 variant 会被 strip，clip 类功能静默失效且构建期不可见。
- **ShaderLab Properties 无 Matrix 类型**；MPB 只覆盖 `UnityPerMaterial` CBUFFER 内字段——per-renderer uniform 必须进 CBUFFER 才能被 MPB 覆盖。
- **PlayMode 首帧 `Time.unscaledDeltaTime` 可达秒级**（加载延迟）——tween/动画别在 Start 自动播（瞬间 complete 写末值）。
- **UPM 包内代码引用包资源**用 `Packages/<name>/...` 路径，非 `Assets/...`。
- **单通道纹理（R8）存非颜色数据**：采样只采 `.r`（D3D 下单通道 GBA 缺省 (0,0,1)，错采 `.a` 恒 1——SDF 文本全画成实心方块）；上传必须 `linear: true`（否则按 sRGB 采样，SDF 距离场全空、字体消失）。
- **根 y-flip 使 winding 反转**——UI shader 必须 `Cull Off`，漏了 = 背面剔除把整个 UI 吃掉。
- **Domain Reload 保护**：关闭 Domain Reload 时 C# static 活过 Play、native 句柄已释放 → 野指针 crash。`SubsystemRegistration` hook 必须调 shutdown；将来引入全局 native 态（global texture/font registry）在此自动清。
- **读渲染 blob 用定长列 + `BitConverter` 直读**——不用 `Marshal.PtrToStructure` 走 marshal 对齐假设；Unity Mono 缺新 .NET API（如 `BitConverter.SingleToUInt32Bits`），用版本无关等价写法。
- **Material 缓存键不含 shader keyword**——新 keyword 组合必须有独立 key 来源（新 program 号或新 key flag 维度），蹭已有 program/键会命中同一 Material 实例 → keyword 冲突静默错渲染。
- **fgui 的 mesh 合并实靠 Unity Dynamic Batching**（隐式、与 SRP Batcher 互斥——URP 下不可控）；SRP Batcher 只降 CPU 不降 draw call。要真 N→1 必须自己合并 mesh。
- **csproj `<Link>` 引用带 UnityEngine/native 依赖的生产源进纯 net10.0 headless 项目编译失败**——`<Link>` 只拷文件不带依赖链；headless 测试用物理拷贝源文件。
- **C# `using` alias 解不了父命名空间同名类型遮蔽**：子命名空间内的类型名必先命中父级同名类型（如 `LoomGUI.Editor.EventType` 撞 `LoomGUI.EventType`），只能全限定名，alias 无用。

## 4. 动态契约

- **dirty hash 的「全量」是动态契约**：每给 RenderNode/Line 加视觉字段，必同步检 payload/header hash 是否覆盖新字段——漏一个 = 静默 stale（不崩、只是不更新）。历史上反复漏过（uvs / 圆角顶点 / line-height / reuse_key / baseline）。
- **查询缓存别缓存 miss**（除非确定源不变）——运行时资源可能后到，缓存 miss 会永久遮蔽后到的正确值。
- **坐标空间劈叉**：`pos` 是世界坐标、`layout_rect` 是页面内容坐标，祖先滚动下两者劈叉——调试命中/滚动偏移先分清在哪个空间。
- **keepalive 保留粒度必须对齐后端 GO 持有粒度**：MirrorPool 是扁平池（slot 根按 reuse_key、叶子按 node_id 独立持有）——core 只发 slot 根 keepalive 保不住叶子 GO，stale 销毁→reactivate 重建→churn 复发；keepalive 须发整子树超集。改 blob 契约或池模型任一侧都要重新对齐粒度。
- **跨树 id 解析必须作用域化**：每新增一种作用域形态（组件实例/List item），全局 `find_by_id_attr` 首匹配就会串实例（组件多实例全部命中第一个）——解析须向上找最近 LOOKUP_SCOPE 根在其子树内做（`find_node_by_id_in_own_scope`）。
- **`remove_node` 联动清理是动态契约**：删节点须同步清全部持久附属表（anim/scroll/controls/roles/lists/text_contents/image_srcs…）——新增持久附属表必须同步加清理，漏一个 = 悬空引用/残留状态。
- **ABI 位型/字段宽度变更的静默错解码**：位掩码/移位常量（`& 0x6000_0000`、`>> 24`、`0xFFFFFFFF` 哨兵）在位型拓宽后**编译全过但语义死掉**——必须 grep 全部位常量逐个对新位型表重审，不能只跟编译器走；C# 侧同理，csbindgen 不生成 struct stub，手写镜像（repr struct、事件 SOA 偏移、`NativeEventBuffer` 手写偏移）的宽度/布局无编译期保护，字段变宽必须人工重排。另防装箱断言陷阱：`Assert.Equal(42u, ulong值)` 经 object 装箱恒 false 但编译过。
