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
- `serde` feature 可整体序列化 `Style`（pkg 格式依赖它）；bincode 编码随 taffy/bincode 版本走——升级依赖必 bump `PKG_FORMAT_VERSION`（见 §2）。
- **行为怪癖**：① span 显式 flex + padding + 文字子 → flex 容器不做文本测量，宽度退化为 padding 值；② 测量不能只看 `known_dimensions`，须结合 `available_space`（否则定宽容器内文本不换行）；③ 某些 sizing 轮次传 `Definite(0)`——首个 0 宽测量若被当最终结果钉死，文字会竖排。

### ttf-parser 0.20（core/src/text）
- kerning 在 `face.tables().kern.subtables` 遍历（取 horizontal 非状态机子表），`.glyphs_kerning(GlyphId, GlyphId) -> Option<i16>`。
- `glyph_hor_advance(GlyphId) -> Option<u16>`（无旧名 `glyph_advance_width`）。
- `.ttc`（TrueType Collection）`Face::parse` 第二参 = collection index，index 0 未必是目标 face。

### slotmap 1.1（core/src/scene）
- `new_key_type!` 生成的 Key 是 `KeyData { idx: u32, version: NonZeroU32 }` **64bit**——装不进 u32 FFI 句柄。NodeId 保持手写 `pub struct NodeId(pub u32)` + `from_key/to_key` 桥接，勿改用 new_key_type!。
- idx 从 1 起（0 是 sentinel slot）；version 恒奇 = occupied；`capacity()` 是总槽位且 remove 不缩——并行数组按 `capacity()+1` 分配才不越界。

### csbindgen 1（ffi/build.rs）
- 默认生成 `internal` 类型——跨程序集访问须 `[assembly: InternalsVisibleTo]`。
- 类型映射：opaque `*mut T` → `T*`（类型化指针非 IntPtr）；`csharp_use_function_pointer(false)` 切 Mono 模式。
- C# `fixed (T* p = &localVar)` 非法（CS0213 already fixed）——`fixed` 只 pin 托管对象（数组/string），局部变量直接取址。

### Unity Input System 1.19（LoomInputCollector.cs）
- 双路径 `#if ENABLE_INPUT_SYSTEM`（`Mouse.current...` 新 API）/ else 旧 `UnityEngine.Input`；asmdef 引用名是 `Unity.InputSystem`（非 `UnityEngine.InputSystemModule`）。

### image 0.25（packer）
- PNG 编码到内存用 `RgbaImage::write_to(&mut Cursor<Vec<u8>>, ImageFormat::Png)`（无 `save_buffer_to_memory` 这个 API）。

### unicode-linebreak 0.1（core/src/text）
- `linebreaks() -> impl Iterator`（非 Vec）；枚举名 `BreakOpportunity`；返回 **byte offset**（非 char index）；在空白**后**断 → 行首无多余空格。

## 2. 跨层闭环规则

### pkg 格式 bump 代价链
改任何进 `.pkg.bin` 的序列化布局（ResolvedStyle、ControlInit、bincode 结构）→ **必 bump `PKG_FORMAT_VERSION`**。bump 的代价链：重打所有 pkg + 重编 .dll + 重出双 exe（loom + GUI）+ **重打全部 headless fixtures**。漏一环就版本错配（stale pkg / loader rc=-1 / 「tag for enum is not valid」），且常在离改动最远的 consumer 测试才炸——文本 merge 干净 + cargo 全绿 ≠ C# 测试绿。

## 3. Unity 平台特性

- **EditMode 禁 `Object.Destroy`**（须 `DestroyImmediate`）；Mesh 是独立 Object，GO 销毁不连带——`[ExecuteAlways]` 路径须显式销毁防泄漏。
- **`.meta` 须入库**，且 Unity 关着时不生成（新增 .cs 要启动 Unity 才产 .meta）——提代码漏 .meta，别人打开工程全断链。
- **`Resources.Load` 不搜 `Editor/Resources/`**（那是 `AssetDatabase.LoadAssetAtPath` 专用，后者要含扩展名全路径）；`.md`/`.html` 在 Unity 里是 DefaultAsset 非 TextAsset。
- **ScriptableObject 禁 `new`** → `CreateInstance<T>()`（`new` 绕过原生对象追踪，IL2CPP 静默失败或产损坏资产）。
- **shader keyword 须 `multi_compile` 非 `shader_feature`**——未启用的 variant 会被 strip，clip 类功能静默失效且构建期不可见。
- **ShaderLab Properties 无 Matrix 类型**；MPB 只覆盖 `UnityPerMaterial` CBUFFER 内字段——per-renderer uniform 必须进 CBUFFER 才能被 MPB 覆盖。
- **PlayMode 首帧 `Time.unscaledDeltaTime` 可达秒级**（加载延迟）——tween/动画别在 Start 自动播（瞬间 complete 写末值）。
- **UPM 包内代码引用包资源**用 `Packages/<name>/...` 路径，非 `Assets/...`。

## 4. 动态契约

- **dirty hash 的「全量」是动态契约**：每给 RenderNode/Line 加视觉字段，必同步检 payload/header hash 是否覆盖新字段——漏一个 = 静默 stale（不崩、只是不更新）。历史上反复漏过（uvs / 圆角顶点 / line-height / reuse_key / baseline）。
- **查询缓存别缓存 miss**（除非确定源不变）——运行时资源可能后到，缓存 miss 会永久遮蔽后到的正确值。
- **坐标空间劈叉**：`pos` 是世界坐标、`layout_rect` 是页面内容坐标，祖先滚动下两者劈叉——调试命中/滚动偏移先分清在哪个空间。
