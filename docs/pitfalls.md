# LoomGUI 踩坑记录

> 项目开发踩坑全库 + 依赖 API 适配。新踩坑继续编号递增（坑 100+），写法：症状/根因/解决/教训。AI 工作约束见 `CLAUDE.md`，设计契约见 `docs/design/main-design.md`。

---

## 1. 依赖 API 适配（plan 草稿常与 crate 实际不符）

> **plan/brief 写的 API 草稿常与实际 crate 版本不符**。遇编译错按本节对照，**勿硬改依赖版本**，按 crate 实际源码（`~/.cargo/registry/src/<crate>-<ver>/src/`）调。

### 1.1 taffy 0.5（layout/mod.rs）
- **无 `MeasureFunc::Boxed`**。用 `TaffyTree<NodeContext>` + `new_leaf_with_context(style, ctx)` + `compute_layout_with_measure(root, Size::MAX_CONTENT, FnMut)`。
- measure 闭包签名：`FnMut(Size<Option<f32>>, Size<AvailableSpace>, NodeId, Option<&mut NodeContext>, &Style) -> Size<f32>`。`known.width` 是 `Option<f32>`（Some=约束宽，None=不限）。
- **闭包可借 `&font`**（FnMat 调用期存活，非 `'static`）→ **不需要 `Arc<Font>`**（v0 一度误判要 Arc，实际单 FnMut 借用合法）。
- `Size::MAX` → `Size::MAX_CONTENT`。
- 根 size setter 用 `Dimension::Length`（`Style.size` 是 `Size<Dimension>`）。
- `Style` **无 `order` 字段**（CSS order 无法存 taffy；留 `ResolvedStyle.order` 待 v1 消费）。
- **`Style.overflow: Point<Overflow>`**（taffy 0.5，Overflow=Visible/Clip/Hidden/Scroll）——CSS flex §4.5 automatic min-size：overflow≠Visible 的 flex item min-size=0（不被 content 撑开）。**必须显式设**（LoomGUI OverflowMode→taffy Overflow 同步），否则默认 Visible→min-size=min-content→scroll 容器被 content 撑开 overlap=0（坑 59）。构造 `taffy::geometry::Point { x, y }`。
- **`Style::DEFAULT.position = Position::Relative`**（taffy 0.5 style/mod.rs:311）。**v1.4-b 起 `position:relative` 已纳入显式映射**（`apply_decl` 接受 `relative`/`absolute` 返回 true，`fence_contract.rs` 测试证明）。`position:absolute` 生效（脱离流，配 `top`/`right`/`bottom`/`left`；`fixed`/`sticky` 仍静默忽略。`inset` shorthand 不在围栏——用四显式属性，别用 shortcut）。`position:relative` 不设 inset 时与 taffy 默认行为一致（无偏移）。

### 1.2 ttf-parser 0.20（text/layout.rs）
- **`glyph_hor_advance(GlyphId) -> Option<u16>`**（非 `glyph_advance_width`，返回 u16 非 i16）。
- **kerning 在 `kern::Subtable`**：`face.tables().kern.subtables` 遍历（取 horizontal + 非状态机子表），`.glyphs_kerning(GlyphId, GlyphId) -> Option<i16>`。`Subtables` 是 `Copy`。
- `glyph_index(ch) -> Option<GlyphId>`（`GlyphId(pub u16)`）。
- `glyph_bounding_box(GlyphId) -> Option<Rect{i16}>`。bearing 用 `x_min`/`y_max`。
- `ascender()/descender()/line_gap()/units_per_em()` 在 `Face` 上。
- `Face::parse(&'static [u8], 0)`——v0 用 `Box::leak` 拿 `'static`（单字体 OK，多字体 v1 换 owned wrapper）。**`.ttc`（TrueType Collection）第二参=collection index**：文泉驿微米黑 .ttc index 0 = Micro Hei Regular（collection 含 2 face）。`.ttf` 单文件 index 0。

### 1.3 cssparser 0.34（parse/css.rs）
- **不能用 NestingParser + parse_one_rule 草稿**。
- `DeclParser` 需实现三 trait：`DeclarationParser + QualifiedRuleParser + AtRuleParser`（`RuleBodyItemParser` 要求三者）。
- 用 `StyleSheetParser` 迭代器替代 `parse_one_rule` 循环。
- `parse_block` 参数是 `ParserState` 非 `SourcePosition`。
- v0 不解析 @ 规则（`AtRuleParser` 默认拒）。

### 1.4 scraper 0.19（parse/dom.rs）
- `Html::parse_document` → `select("body")` → `children()` 迭代。
- `ElementRef::value()` 取 Element，`.attrs()` 取属性迭代。
- `<img>` 是 void 元素（无闭合标签），src 从 `attrs` 取非 text。

### 1.5 csbindgen 1.9（loomgui_ffi_c/build.rs + 生成 LoomGUIBindings.cs）
- 默认生成 **`internal`** 类型（`Native` 类、`StageHandle` 结构）→ 跨程序集（LoomGUI.Bindings→LoomGUI.Runtime）访问须 `[assembly: InternalsVisibleTo("LoomGUI.Runtime")]`（放 AssemblyInfo.cs）。
- 类型映射：`*const u8`→`byte*`、`*mut usize`→`nuint*`、opaque `*mut T`→`T*`（**类型化指针非 IntPtr**）。`csharp_use_function_pointer(false)` 切 Mono 模式。
- `CString::as_ptr()` 返 `*const c_char`(i8)，签名为 `*const u8` 时须 `as *const u8` cast。
- build.rs 跑两次（OUT_DIR 必成 `.expect`；Unity 目录那次失败要 `cargo:warning=` 勿 `let _ =` 吞错）。
- C# `fixed(T* p=&localVar)` **非法**（CS0213 "already fixed"）——局部栈上已固定，直接 `&localVar` 传；`fixed` 只 pin 托管对象（数组/string）。

### 1.6 Unity 6.5（6000.5）C# API
- `Object.GetInstanceID()` **废弃**→`GetEntityId()`，后者返 **`EntityId`（非 int）**，`EntityId→int` 隐式转换**也废弃**（"将来不能 int 表示"）。**绕开整条**：缓存 key 直接持 Object 引用（`Dictionary<Texture,...>`，Unity 对象引用同一性），别碰任何 id API。
- **EditMode 禁 `Object.Destroy`**（报 "Destroy may not be called from edit mode"），须 `DestroyImmediate`。`[ExecuteAlways]` 组件生产代码 EditMode 也跑 → `Application.isPlaying ? Destroy : DestroyImmediate`（Mesh 是独立 UnityEngine.Object，GO 销毁不连带，须显式销毁防泄漏）。
- `Camera.nearClipPlane` **须 >0**（负值抛异常）。
- Unity 开着**锁 native `.dll`**——重建/拷 `.dll` 须先关 Unity（锁文件 `Temp/UnityLockfile` 可能残留非真锁，以能否 `rm` 为准）。
- 生成物 gitignore：`.slnx`(6.5 解决方案)、csbindgen `.cs` 绑定及其 `.cs.meta`；`.dll`（`!**/Plugins/**/*.dll` 白名单）入库；`.meta` 是 Unity 资产元数据须入库（**implementer 提 .cs 易漏 .meta**——Unity 关着时不生成，坑 13）。
- **动态字体 API**（Text 光栅）：`Font.RequestCharactersInTexture(string, fontSize, FontStyle)` 填 atlas（必先调，否则 GetCharacterInfo 恒 false）→ `Font.GetCharacterInfo(char, out CharacterInfo, fontSize, FontStyle) -> bool` 取 `minX/maxY/maxX/minY`（像素 box）+ `uvBottomLeft/uvTopLeft/uvTopRight/uvBottomRight`。`CharacterInfo.advance` 存在但 LoomGUI **不用**（Rust 笔位）。`Font.textureRebuilt` 是**静态**事件（register/OnDestroy 解绑防泄漏）。
- **`HideFlags.DontSaveInEditor`**：`[ExecuteAlways]` 程序生成 GO 标之防被存进场景（否则 EditMode dirty 场景 + Play/Stop 累积残留，坑 11）。
- **`Mesh.SetVertices(List)`/`SetUVs`/`SetColors`/`SetTriangles(List)` overload**：零 per-frame 数组 alloc（vs `SetVertices(Vector3[])`）。`List.Clear()` 保 Capacity，warm-up 后复用零 alloc。
- **shader keyword**：`#pragma multi_compile _ CLIPPED`（两 variant 都编，`EnableKeyword` 切换生效）**非** `shader_feature`（未启用的 variant 会被 strip → clip 静默失效）。
- **ShaderLab Properties 类型**：只 Float/Range/Int/Color/Vector/2D/3D/Cube，**无 Matrix**（坑 52）。per-renderer matrix uniform 放 `UnityPerMaterial` CBUFFER（无 Properties 对应）+ MPB.SetMatrix 覆盖。
- **MaterialPropertyBlock 覆盖范围**：只覆盖 material property（Properties 或 `UnityPerMaterial` CBUFFER 字段）；CBUFFER 外全局 uniform MPB **不覆盖**（坑 52，静默失效）。per-renderer uniform 必须进 CBUFFER。
- **TransformObjectToWorld = mul(unity_ObjectToWorld, pos)**：GO 是 root 子时 = root_ObjectToWorld（含 root transform sf/-sf/sf+rootPos）。core 算 design world，shader 桥接 design→Unity world 用它（坑 51）。GO transform=identity 时仍 = root transform（继承父）。
- **Time.unscaledDeltaTime 首帧 spike**：PlayMode 首帧可达数秒（加载延迟，实测 2.07s），tween/动画别在 Start 自动播（坑 53，瞬间 complete 写末值）。
- **`Resources.Load` 不搜 `Editor/Resources/`**：`Resources.Load` 只搜任意目录下的 `Resources/` 子目录，但**排除 `Editor/` 下的 Resources**（那是 `EditorGUIUtility.Load` / `AssetDatabase.LoadAssetAtPath` 专用）。把资源放 `Assets/LoomGUI/Editor/Resources/LoomGUI/` 然后调 `Resources.Load<TextAsset>("LoomGUI/xxx")` → **返 null，功能静默失败**（InjectFenceRules/DistributeSkill 全挂）。Editor 下读 Editor/Resources 资产用 `AssetDatabase.LoadAssetAtPath<TextAsset>("Assets/LoomGUI/Editor/Resources/LoomGUI/xxx.md")`（含扩展名全路径）。
- **ScriptableObject 子类禁 `new`**：`SpriteAtlas` / 自定义 `ScriptableObject` 子类用 `new T()` 绕过 Unity 原生对象追踪 + 序列化，IL2CPP/特定版本静默失败或产损坏资产。用 `ScriptableObject.CreateInstance<T>()`。
- **`Uri.MakeRelativeUri` 剥 trailing slash + 兄弟目录算错**：`Path.GetFullPath` 剥目录末尾 `/`，`MakeRelativeUri` 把无 trailing slash 的路径当文件 → 产 `../StreamingAssets`（无尾斜杠）。且兄弟目录（如 `Assets/LoomUI/` → `Assets/StreamingAssets/`）相对只 1 级 `../`，非手工算的 2 级。修：`GetFullPath` 前记 `targetDir.EndsWith("/")` → 后补 trailing slash；相对路径级数靠 MakeRelativeUri 算别手算。

### 1.7 taffy 0.5.2 serde + bincode 1.x（style/resolved.rs + asset/mod.rs，v1b.1）
- taffy 0.5.2 有 **`serde` feature**：`Style`（style/mod.rs:189）及全部字段类型（geometry/dimension/flex/grid/alignment）都 `#[cfg_attr(feature="serde", derive(Serialize,Deserialize))]` + `#[serde(default)]`；`Style` 还派生 `PartialEq`。开 `taffy = { version="0.5", features=["serde"] }` 后，含 `taffy_style: taffy::style::Style` 的 `ResolvedStyle` 能整体 `#[derive(Serialize,Deserialize,PartialEq)]`。
- bincode 1.x：`bincode::serialize(&x)->Vec<u8>` / `bincode::deserialize::<T>(&bytes)`。`#[serde(default)]` 在 bincode（位置编码无缺字段概念）下透明。用于包格式的 StyleRecord——穷尽由 serde 派生保证，比手写枚举 taffy 30+ 字段稳健（R3≈0）。
- bincode 格式随 taffy/bincode 版本——升级时 bump 包 `formatVersion`。

### 1.8 image 0.25（loomgui_pkg，v1b.3）
- **`save_buffer_to_memory` 不存在**（plan 草稿写错）→ 用 `RgbaImage::write_to(&mut std::io::Cursor<Vec<u8>>, ImageFormat::Png)` 编码 PNG 到内存。
- 解码：`image::open(path)?.to_rgba8() -> RgbaImage`（像素+w/h）；合成 atlas：`RgbaImage::from_raw(w, h, buf)` 建图；回查测：`image::load_from_memory(&bytes).to_rgba8()`。
- Cargo：`image = { version = "0.25", default-features = false, features = ["png"] }`（仅 png 最小依赖；**只在 packer，core 不碰像素**）。
- 教训：plan 草稿的 crate API 名常错（本例 `save_buffer_to_memory`）→ 实现 RED 阶段验实际 API（`~/.cargo/registry/src/<image>-<ver>/src/`）。

### 1.9 unicode-linebreak 0.1（text/layout.rs，v1b.5）
- **`linebreaks(s: &str) -> impl Iterator<Item=(usize, BreakOpportunity)>`**（非草稿 `Vec<(usize, BreakType)>`——返**迭代器**非 Vec，需 `.collect::<Vec<_>>()`）。
- **`enum BreakOpportunity { Mandatory, Allowed }`**（非草稿 `BreakType`——变体名同但**枚举名不同**）。
- 返回 `usize` = **byte offset**（非 char index），升序；offset 语义 = 可在该 byte offset 处断（前段 `content[..offset]`，后段 `content[offset..]`）。unicode-linebreak 在空白**后**断 → segment 自含尾空白 → 行首无多余空格。
- 用法（layout.rs:194+）：`linebreaks(content).collect()` → 按 offset 切 segments `Vec<(&str, BreakOpportunity)>` → greedy fill（累加 seg 宽超 max_w 换行，Mandatory 强制结束行）。
- 教训：brief/草稿写 `Vec<(usize, BreakType)>` 实际是 `impl Iterator`+`BreakOpportunity`（坑 1/2/8 同源）→ 实现 RED 阶段验 `~/.cargo/registry/src/<unicode-linebreak>-<ver>/src/`。

### 1.10 Unity Input System 1.19（LoomInputCollector.cs，v1c.1）
- **新 API**：`Mouse.current.position.ReadValue()`（左下原点 screen 像素，同旧 `Input.mousePosition` 语义）/ `Mouse.current.leftButton.wasPressedThisFrame`·`wasReleasedThisFrame`（vs 旧 `Input.GetMouseButtonDown/Up`）。
- **双路径**：`#if ENABLE_INPUT_SYSTEM`（Player Settings Active Input Handling=New/Both 定义此宏）走新 API，else 旧 `UnityEngine.Input`。asmdef references 加 `"Unity.InputSystem"`（非 `UnityEngine.InputSystemModule`——那个名错编译失败）。
- 教训：plan 选旧 Input 但工程切了 Input System package → 运行时 `InvalidOperationException`（坑 28）。Unity Input System 是 package（assembly `Unity.InputSystem`），非UnityEngine 内置。

### 1.11 slotmap 1.1.1（scene/node.rs，v1.3+ 动态树）
- **实际 API**：`new_key_type! { pub struct NodeId; }` 生成 `pub struct NodeId(pub KeyData)`——**KeyData 是 `{ idx: u32, version: NonZeroU32 }` 64bit，两字段私有**，仅 `as_ffi()->u64`/`from_ffi(u64)` 公开。`as_ffi()`=`(version<<32)|idx`。`Key` trait 是 `pub unsafe trait`（**非 sealed**，外部可 unsafe impl，但 slotmap 强烈建议用 new_key_type!）。
- **与草稿/spec 的差异**：spec/plan 假设 `new_key_type!` 生成的 Key 是 u32 可装的（`pub struct NodeId(pub u32)`）——**错**。KeyData 64bit 无法无损装 u32，而 FFI/C#/FrameBlob/pkg.bin 全程硬约定 `node_id: u32` + sentinel `0xFFFF_FFFF`。故不能用 new_key_type! 重定义 NodeId，改 `SlotMap<DefaultKey, Node>` + 手写 `NodeId(pub u32)` 经 `from_key/to_key` 桥接。SecondaryMap<NodeId,T> 同理不可行（要 NodeId impl unsafe Key + 位宽不匹配），改 HashMap。
- **slotmap idx 从 1 起**（free_head:1，idx 0 是 sentinel slot）——首节点 NodeId.0=4097 非 0。`capacity()` = 总槽位数（occupied+free），remove 不缩 capacity。并行数组按 `capacity()+1` 分配覆盖所有 live idx。
- **version 恒奇**（occupied）：insert `version|1`，新 slot version=1，remove `wrapping_add(1)`（奇→偶 vacant）。12bit 截断后仍奇 → from_ffi no-op → round-trip 一致。version wrap：每次 remove+insert 净增 2，~2048 次后低 12bit 回 1（安全返 None 非错位）。
- 教训：plan 草稿假设 slotmap Key 编码 = spec 自定义位宽（20/12），实际 slotmap KeyData 是固定 64bit（idx 32 + version 32）。写 plan 前读 `~/.cargo/registry/src/slotmap-1.1.*/src/keys.rs` + `lib.rs` 确认 KeyData 实际布局，勿按 spec 假设。

---

## 2. 踩坑记录

### 坑 1：taffy 0.5 `MeasureFunc::Boxed` 不存在（API 详见 §1.1）
brief 写 `MeasureFunc::Boxed` 编译失败 → 0.5.2 改 `TaffyTree<NodeContext>` + `compute_layout_with_measure` FnMut（`Arc<Font>` carry 作废，FnMut 借用合法）。**教训**：brief API 草稿是起点非权威，按编译器 + crate 实际版本调。

### 坑 2：ttf-parser 0.20 advance/kerning API 改名（API 详见 §1.2）
`glyph_advance_width`/`kerning_for` 编译失败 → 0.20 改 `glyph_hor_advance`(返 u16) + kerning 移 `kern::Subtable`。**教训**：ttf-parser 跨版本 API 变动大，查 `~/.cargo/registry` 源码确认。

### 坑 3：默认 flex-direction 没落地 column
**症状**：未显式写 flex-direction 的 div 水平排列，违反 §4.1。
**根因**：`ResolvedStyle::default()` 用 taffy `Style::DEFAULT`（flex_direction=Row）。实现者一度用测试 CSS 掩盖（加 flex-direction:column 让测试过），final review 抓出。
**解决**：Default impl 设 Column；CSS 显式声明无条件覆盖（时序 default→apply_decl）。加 `default_div_is_column` 回归测试。
**教训**：AI 可预测性核心约束必须在**默认值层**落地，不能靠测试 fixture 掩盖。

### 坑 4：围栏外元素静默降级 Text
**症状**：`<video>` 等围栏外 tag 被当 Text 节点。
**根因**：scene 层 `_ => NodeKind::Text` fallback。
**解决**：parse 层白名单报错（执法点在 parse 非 scene），scene 删 fallback 改 `unreachable!`。
**教训**：「报错不降级」类约束执法点要对（parse），下游信任输入。

### 坑 5：div/button 裸文本被丢弃
**症状**：`<div>标题</div>` 无文本输出。
**根因**：scene `build_rec` 对 Container 不处理 `el.text`。
**解决**：`build_text_child` 生成 Text 子节点（继承父 8 文本字段，size=Auto 不污染高度）。
**教训**：spec §4.2「Text 叶子是 Container 子节点」——裸文本该成 flex item 子节点，非丢弃。

### 坑 6：cascade specificity 排序方向反
**症状**：多规则命中时低 specificity 胜。
**根因**：`match_element` 返回降序，直接顺序 apply 让低优先级后写覆盖高优先级（反了）。
**解决**：`resolve_styles` 加 `sort_by_key` 升序（高 specificity 后 apply 胜，稳定排序保同级 source 顺序）。
**教训**：接 `match_element` 时核对排序方向；CSS cascade 是高 specificity 胜。

### 坑 7：后代选择器只查直接父
**症状**：`div.a span` 在 `<div class=a><div><span>` 不命中。
**根因**：`matches()` 只查 parent 不递归祖先；`Combinator` 字段未用。
**解决**：`match_compound_chain` 递归祖先（Descendant 沿父链搜+回溯，Child 只直接父）。附带修 `parse_selector` 空格降级 Child bug。
**教训**：围栏声明「后代/子代」选择器就要真实现递归祖先。

### 坑 8：snapshot 绑系统 arial.ttf
**症状**：Linux CI（DejaVuSans 无 arial）snapshot 漂移。
**根因**：测试字体试系统路径。
**解决**：锁仓库内 `tests/fixtures/DejaVuSans.ttf` + `env!("CARGO_MANIFEST_DIR")`；fixture 用 ASCII（DejaVuSans 无 CJK）。
**教训**：测试产物跨平台一致就锁仓库内资源；CJK 渲染验证留 v1（需 CJK 字体）。

### 坑 9：color_tint 把背景色块涂黑（v1a T8）
**症状**：PlayMode 红背景块渲成黑色。
**根因**：v0 `ResolvedStyle::default().color=[0,0,0,1]`（CSS `color` 默认黑，是**前景/文本色**）；blob 烘焙 `bg×color_tint×alpha` 把红背景乘成不透明黑。
**解决**：build_blob **不乘 color_tint**——顶点色 = background_color，仅 `alpha×node opacity`。color_tint 是文本色（Phase 2 文本用）。
**教训**：mesh colors 已是最终色（bg-color / 图片白），别再叠 color_tint；§4.2b「tint×alpha 烘焙」指**文本/图片** tint，非背景色块。

### 坑 10：Rust 改 blob 格式后 .dll 没换 → C# 静默拒帧不渲染（v1a Phase 2）
**症状**：PlayMode **啥都不渲**（红块文字全无）、Console **干净无错**。
**根因**：Plugins 里 `.dll` 是旧的（Phase 1），产 **v1 blob（version=1）**；T1 起 C# `FrameBlob.IsValid` 只认 version==2 → `MirrorPool.Sync` 第 1 行 `if(!IsValid)return` **静默早退**。Unity 开着锁 .dll，编译后没换。
**解决**：`cargo build --release` → **关 Unity**（锁 .dll）→ `cp target/release/loomgui_ffi_c.dll Plugins/LoomGUI/` → 重开。
**教训**：**任何 Rust FFI 改动（尤其 blob/ABI 格式）后，PlayMode 验前必重编+换 .dll**。症状"全不渲+Console 干净"先怀疑 stale .dll（`md5sum` 对比 fresh build）。Unity 开着锁 .dll，换 .dll 必关 Unity。

### 坑 11：ExecuteAlways 程序生成 GO 累积泄漏（v1a Phase 2）
**症状**：Play/Stop 反复 + domain reload 后 `loom_node` GO 在 Hierarchy 累积、内存泄漏。
**根因**：`[ExecuteAlways]` EditMode 也跑，MirrorPool 产的 `loom_node` GO 挂 root 下被**存进场景**；每次 Awake 新 `_pool` 丢旧引用 → 孤儿 GO 清不掉。
**解决**：GO/Mesh 标 `HideFlags.DontSaveInEditor`（不入存盘）+ Awake 开头清 root 下 `loom_node` 孤儿 GO。
**教训**：ExecuteAlways + 程序生成 GO 必加 DontSave + 开局清孤儿；OnDestroy 的 `pool.Clear` 只清当前 run 的，跨 run 孤儿要 Awake 清。

### 坑 12：手搓 blob 测 fixture 写 AoS，多节点读串列（v1a Phase 2）
**症状**：EditMode 跑 `MirrorPoolFlattenTests` 报 `SetTriangles: idx 非三的倍数`。
**根因**：C# 手搓 2 节点 blob 写 **AoS**（node0 全字段、node1 全字段）但列 offset 按 1 节点 elemSize 递进、`FrameBlob` 读 **SOA**（列优先 `ColOff(idx)+i*elemSize`）→ node1 每字段读串一位，mesh_off 落到 node0 mesh_len → idx 读成垃圾。
**解决**：fixture 列 offset 按 `NodeCount×elemSize` 递进、数据列优先写（镜像 `blob.rs`）。单节点 fixture AoS≡SOA 不受影响（故 Phase 1 没暴露）。
**教训**：手搓 blob byte[] 测必 SOA 列优先，与 `blob.rs`/`FrameBlob` 一致；多节点才暴露（单节点掩盖）。

### 坑 13：implementer 提 .cs 漏 .meta（v1a Phase 2）
**症状**：合 main 后 4 个新 `.cs` 的 `.meta` 没入库（ClipMath/ClipBoxTests/FrameBlobV2Tests/MirrorPoolFlattenTests）。
**根因**：Unity 关着时 import 不生成 `.meta`；subagent 提 .cs 时 Unity 未开 → .meta 后生成、未 add。TextRasterizer.cs.meta 这次提了（那次 Unity 开着），其余漏。
**解决**：合 main 后补提交漏的 .meta；或 implementer 提前确保 Unity 开过一次生成 .meta。
**教训**：Unity `.meta` 随 `.cs` 入库；subagent 流程里提 .cs 后 controller 验 `.meta` 是否齐（Unity 生成的资产元数据，缺则 GUID 不稳）。

### 坑 14：Unity GetCharacterInfo 要 codepoint 非 glyph_id（v1a Phase 2）
**症状**：text_arena 只有 ttf `glyph_id`，Unity 取不到字。
**根因**：Unity `Font.GetCharacterInfo(char, ...)` 按 **Unicode 码点**，非 ttf glyph_id；ttf glyph_id 是字体内部字形索引。
**解决**：Rust `Glyph` 加 `codepoint:u32`（`measure_text` 遍历 char 时填），text_arena 送 codepoint，Unity `(char)codepoint` 调 GetCharacterInfo。
**教训**：引擎字体 API 多按码点；核心 Glyph 须同时持 glyph_id（ttf 直连后端）+ codepoint（引擎字体 API）。

### 坑 15：新二进制格式 magic 撞既有格式（v1b.1）
**症状**：v1b.1 包格式初拟 magic `"LOOM"`(0x4D4F4F4C)，与 frame blob 的 `MAGIC`（blob.rs）**完全相同**。
**根因**：两种格式独立，但 magic 是唯一识别码；撞了 magic→校验形同虚设（误传 frame blob 给 `load_package` 会过 magic 检查再挂错）。
**解决**：包改独立 magic `"LPKG"`(0x474B504C，磁盘字节 `4C 50 4B 47`)。planning 期 grep 现有 magic 才发现。
**教训**：新增二进制格式先 `grep -r 'MAGIC\|0x4D4F4F4C' src/` 确认 magic 唯一；formatVersion 是「同格式的版本」，magic 是「这是哪种格式」，两者正交。

### 坑 16：FFI 返 String::as_ptr() 无尾 NUL，C# NUL-scan 读越界（v1b.2）
**症状**：`image_src_at` 返 src 串指针，C# 用 `PtrStringAnsi(ptr)` 或 Rust 测用 `CStr::from_ptr` 读 → 越界读到 `Utf8Error{valid_up_to>N}` 或乱码。
**根因**：Rust `String`/`Vec<u8>` 缓冲**无尾 `\0`**；`PtrStringAnsi`/`CStr` 靠 NUL-scan 会越过 stage 缓存末尾。
**解决**：FFI 同时返 `*out_len`（字节长）；C# 用 `Encoding.UTF8.GetString(ptr,(int)len)`，Rust 测用 `slice::from_raw_parts(p,len)`+`from_utf8`。同 `borrow_frame` 的 ptr+len 契约。
**教训**：Rust FFI 返字符串一律 ptr+len（不靠 NUL）；C# 侧禁用 NUL-scan 读法。任何新 len-based 串返回（font/资源名）照此。

### 坑 17：bump blob version 须同步所有手搓 C# fixture，漏一个 4 字节 skew（v1b.2）
v2→v3 加 tex_id 列，某 C# builder 只升 header（14 列）没补 data 列写 → 4 字节 skew，`ReadMesh` 读串。手搓 blob fixture 散落多文件多 builder，version bump 后 review 只抽查漏一个。**解决**：grep 全 C# 测目录 `version=Nu`/`HeaderLen`/`elemSize = {`/`N \* 4` 逐 builder 升（version/HeaderLen/offs/elemSize 项数/loop 边界/补列写）；Rust `num_col_offsets=columns.len()` 自动传播，C# arena offset 基准 `12+N*4` 也改。**教训**：blob 是 Rust↔C# 字节契约，version bump = 全仓 fixture 同步事件，grep 枚举所有 builder 不能只改抽查的。

### 坑 18：`using System;` 引入 System.Object，裸 `Object` 与 UnityEngine.Object 歧义（v1b.2）
**症状**：LoomStage.cs 加 `using System;`（为 `Encoding`/`Exception`）后，6 处裸 `Object.Destroy`/`DestroyImmediate` 编译报 CS0104 `'Object' is an ambiguous reference between 'UnityEngine.Object' and 'object'`。
**根因**：`System.Object`（C# `object` 关键字的类型）与 `UnityEngine.Object` 同名；两个 using 都在时裸 `Object` 二义。
**解决**：全限定 `UnityEngine.Object.Destroy/DestroyImmediate`（不动 using——`System` 还要 `Array.Empty`/`Encoding`）。MirrorPool.cs 无 `using System;` 故裸 `Object` 无歧义。
**教训**：Unity C# 文件若同时 `using System;`+`using UnityEngine;`，裸 `Object` 必歧义——直接全限定 `UnityEngine.Object`。

### 坑 19：BitConverter.GetBytes 无数组 overload，手搓 blob 索引数组写法错（v1b.2）
**症状**：C# 测 `BitConverter.GetBytes(new uint[]{0,1,2,0,2,3})` 编译报 CS1503 cannot convert uint[] to bool。
**根因**：`BitConverter.GetBytes` 只接标量（无 `uint[]` overload），最近重载是 `GetBytes(bool)`，编译器把 uint[] 当 bool。
**解决**：逐个 `GetBytes(0u)`/`GetBytes(1u)`...（对齐 MirrorPoolFlattenTests 已验证写法）；或循环。
**教训**：手搓 blob byte[] 的 C# 测，索引/数据数组一律逐元素 `GetBytes`，禁数组语法。

### 坑 20：打包器磁盘 atlas 文件名 ≠ .pkg.bin header 记的名 → Unity 找不到 atlas（v1b.3）
**症状**：PlayMode atlas sprite 全白占位（atlas.png 没载），但 .pkg.bin 解析正常、Console 无报错。
**根因**：main.rs 用 `out_path.with_extension("atlas.png")` 写磁盘 → `<stem>.pkg.atlas.png`；lib.rs 把 `"loom.atlas.png"` 写进 AtlasSection header。Unity 按 header 名 `Path.Combine(StreamingAssets, atlas_filename)` 找 → 名不匹配 → `File.ReadAllBytes` 抛 → 跳过 → 白占位。
**解决**：main.rs 用 packer 返回的 `p.atlas_filename` 拼磁盘路径（`out_parent.join(&p.atlas_filename)`）→ header 与磁盘同串，by-construction 一致。
**教训**：打包器产两文件（.pkg.bin + atlas.png）时，**磁盘 atlas 名必须 == header 的 `atlas_filename`**（后端按 header 载）；用同一变量拼两端，别各算各的。**v1-showcase 加 `-a <name>.atlas.png`/`pack_named`**（c65db2f）——多 sample 共存 StreamingAssets 用独立 atlas 名，避免共享 `loom.atlas.png` 互相覆盖（LoomStage 按 pkg header `atlas_filename` 载，非 hardcode）。

### 坑 21：删 pub API 只验单 crate → 依赖 crate 编译断裂（v1b.3）
**症状**：T1 删 `TextureRegistry::register` 只跑 `cargo test -p loomgui_core`（绿），但 `loomgui_ffi_c`（register_texture FFI 调 register）编译断裂，到 T3 跑 workspace 才发现。
**根因**：`register` 是 pub API，被 ffi_c 跨 crate 调用；单 crate 测不覆盖跨 crate 依赖。
**解决**：删 pub API 必跑 `cargo test --workspace`（或至少 `cargo build` 所有依赖 crate）；T1 acceptance 只写 `-p loomgui_core` 是 plan 验测范围写窄。
**教训**：删/改 pub API（尤其被 FFI 跨 crate 用）→ acceptance 必含 `cargo test --workspace`，单 crate 绿 ≠ workspace 绿。

### 坑 22：fgui DoFairyBatching 不合并 mesh——照搬不够，core 要显式合并（v1b.4）
**症状**：初版 v1b.4 设计以为「照搬 fgui DoFairyBatching = N→1 draw call」。
**根因**：读 fgui Unity 源码（NGraphics.cs）发现每元素独立 MeshFilter+MeshRenderer，零 Graphics.DrawMesh/跨元素顶点拼接——DoFairyBatching 只重排 sortingOrder 让同 material 相邻，靠 **Unity Dynamic Batching 隐式合**（不可控、URP 下与 SRP Batcher 互斥、顶点≤300 限）。
**解决**：LoomGUI core 显式合并（render::merge::merge_meshes）——补 fgui 靠 Unity 隐式做的那步，确定性 N→1、跨后端。
**教训**：调研参考引擎时读源码确认「它实际做什么」vs「文档/印象说做什么」——fgui「合批」≠「合并 mesh」。

### 坑 23：fgui AABB 相交语义——同 material 相交仍聚拢保相对序（v1b.4）
**症状**：brief 测 `reorder_unit_overlapping_keeps_order` 断言 `[0,1,2]`（相交保序不动），实际 `[0,2,1]`。
**根因**：fgui 算法（Container.cs:923-931）相交 break 时用**已算出的 k**（同 material 聚拢点）——同 material 相交仍前移紧邻（不越目标，保绘制序 A→C）；不同 material 相交才不越过（k=m=i 不动）。「相交保序」=保相对绘制序非「完全不动」。
**解决**：断言改 `[0,2,1]` + 注释澄清。同 material 相交合并视觉安全（index buffer 保相对序）。
**教训**：算法移植按源码逐行 trace 验，勿按文字描述想当然；同 material 相交也能合（利好合批率）。

### 坑 24：merged node_id 必须=锚（batch 内 min）否则动画 GO 抖动（v1b.4）
**症状**：merge 后节点身份若每帧变（batch 划分随动画变），MirrorPool `_pool[node_id]→GO` 频繁增删 GO。
**根因**：merged 节点「虚拟」（多原始节点合并），无稳定 node_id → batch 划分变 → node_id 变 → GO 抖动。fgui 无此问题（不 merge，每元素稳定 GO）。
**解决**：merged node_id=batch 内最小原始 node_id（锚）——batch 划分不变时锚稳定→GO 复用零抖动。MirrorPool 零改（按 node_id 复用，看不出 merged vs 单）。
**教训**：merge 改变节点结构，必须给 merged 节点稳定身份（锚），否则后端池复用抖动。

### 坑 25：既有测用 N 同级同 DrawState 节点，merge 后 index OOB（v1b.4）
**症状**：`build_assigns_monotonic_keys`（root>[a,b] 3 Container）merge 后 3→1 节点，`rns[1]`/`rns[2]` OOB。
**根因**：3 同级 Container 同 DrawState（tex=0,program=0,mask=0）→ merge 成 1。
**解决**：改嵌套 clip 链（root clip→mid clip→leaf，3 不同 mask_context）→ 不同 DrawState → 不 merge → 保 3 节点，原 intent 保留。
**教训**：merge 上线后，既有「多同级同 DrawState 节点」测会 OOB——改成不同 DrawState（嵌套 clip/不同 tex_id）保留多节点。

### 坑 26：CJK sample width 放根节点 → root_size 覆盖致不换行（v1b.5）
**症状**：PlayMode CJK 段落整段挤一行不换行（~948px，远超 240px 约束）。
**根因**：sample 把 `width:240px` 放**根节点** div → `solve`（mod.rs:125 `set_style size=Length(root_size)`）强制覆盖根节点 size 为 root_size(1080) → Text 子 measure `known.width` 收 1080（非 240）→ 整段<1080 不换。§2.5 根节点 measure 陷阱的 sample 实例。
**解决**：sample 包一层 `.root` Container（吃 root_size）+ `align-items:flex-start`（防子项 cross 轴拉伸），`.c` 文本 div 作子节点（width:240 不被覆盖）。独立 layout 验 Text rect=240x113（逐字断行）。
**教训**：sample/测的**受约束文本/图必须在根节点之下**，根节点 size 必被 root_size 覆盖——根节点只作 viewport 容器，不设显式 CSS size。

### 坑 27：mandatory break 留 `\n` 字面量 → 下游幽灵 `.notdef` 字形（v1b.5）
**症状**：含 `\n` 文本换行时，行 text 含字面 `\n` → glyph gen 给 `\n` 产 `.notdef`（GlyphId 0）幽灵字形进 blob text_arena。
**根因**：unicode-linebreak 的 mandatory break 在 `\n` 字节 offset 断，segment `content[..offset]` 含 `\n`；flush 行时未 strip。
**解决**（defer post-merge，M3）：mandatory flush 前 `cur.trim_end_matches(|c| c=='\n'||c=='\r')` + 回归测断行 glyph 数。**当前不阻塞**：v1b.5 sample 无 `\n`；Unity `TextRasterizer.cs:67` `GetCharacterInfo('\n')`=false→continue 静默跳过幽灵字形，**无视觉伪影**（Rust 侧冗余，渲染干净）。
**教训**：断行产出喂 glyph gen 前须净化控制符；但后端 `GetCharacterInfo` false→continue 是天然兜底（坑 14 codepoint 路径副产物）。

### 坑 28：Unity 新旧输入系统不匹配 → InvalidOperationException（v1c.1 PlayMode）
**症状**：PlayMode 每帧 `InvalidOperationException: You are trying to read Input using UnityEngine.Input class, but you have switched active input handling to Input System package`。
**根因**：plan §7 选旧 `UnityEngine.Input`（桌面 Mono 最简），但工程装了 Input System package 1.19 且 Player Settings Active Input Handling≠Old → 旧 API 运行时禁用。
**解决**：`LoomInputCollector` 双路径 `#if ENABLE_INPUT_SYSTEM`（`Mouse.current` API）+ asmdef 加 `"Unity.InputSystem"` reference + Player Settings 改 Both/New（§1.10）。
**教训**：Unity 输入系统是工程级配置，plan 选型须先查工程 `ProjectSettings.asset activeInputHandler` + manifest 是否装 InputSystem package；默认走双路径兼容最稳。

### 坑 29：hover/active 只设命中点自身 → 文字子挡父 hover（v1c.1 PlayMode）
**症状**：hover 按钮的**文字区（上半段）不变蓝**，下半段（非文字）变蓝；click 文字区也无响应。
**根因**：v1c.1 `hover_diff` 只设 `cur_hit.hovered=true`（单点），父不因子孙 hover 而 hover。但 CSS `:hover` 语义是「鼠标在元素**或后代**上」+ fgui `HandleRollOver` 维护 `rollOverChain` 沿祖先链（`element=element.parent`）给 target+所有祖先派 RollOver/Out。命中 Text 子 → 只 Text.hovered，btn.hovered=false → `.btn:hover` 不匹配。
**解决**：`set_hovered_chain`/`set_active_chain` 沿 parent 链设 target+所有祖先 hovered/active；事件派发仍单点（v1c.1 无冒泡，v1c.2 加 BubbleEvent）。
**教训**：伪类状态须沿祖先链（对齐 fgui rollOverChain + CSS 祖先语义），单点设导致子孙挡父；「子节点挡父 hover/click」是 UI 框架经典坑，对照 fgui rollOverChain 设计。

### 坑 30：自动 Text 子节点不消费 StyleSheet（v1c.1 defer 根因 a）
**症状**：`span{pointer-events:none}` 写进 CSS，但 `<div>文字</div>` 自动建的 Text 子 `touchable` 仍 true（CSS 没匹配到它）。
**根因**：`build_scene` 给 Container 裸文本自动建的 Text 子**不是 DOM 元素**——`resolve_styles` 跑在 build_scene 前只算 DOM 元素，自动 Text 子拿不到任何 CSS 规则。只有显式 `<span>` 走 DOM resolve 才吃到 CSS。
**解决**（defer v1c.x）：修坑 29（hover 祖先链）后影响降级——文字挡命中但父也 hover，故不需 pointer-events 穿透。根治须 build_scene 后给自动 Text 子补 resolve（架构改），或框架默认 Text `touchable=false`。v1c.1 sample 用显式 `<span>` 绕（修坑 29 后已回归裸文本）。
**教训**：自动建的节点（非 DOM 元素）不消费 StyleSheet——CSS 规则只作用于 parse 期 DOM 元素；给自动子样式化须显式标签或框架默认。

### 坑 31：brief 测断言过强——`hover_chain_idempotent` assert `out.is_empty()` 但 Move 每次 emit（v1c.2）

**症状**：v1c.2 hover_diff 祖先链 diff 的幂等测验 `out.is_empty()`（同点 Move 第二次），但 v1c.1 Move handler 命中即产 `EVT_MOVE`（§7.1「move 恒产」），故 out 非空，测 FAIL。
**根因**：测的本意是「hover diff 幂等」（同点无 RollOver/Out），不是「无任何事件」。Move 事件每次 emit（指针移动事件，非 hover 变化），照 fgui `onTouchMove` 每次 dispatch。
**解决**：测断言改为 `out.iter().all(|e| e.event_type != EVT_ROLL_OVER && e.event_type != EVT_ROLL_OUT)`（无 hover 事件，Move 允许）。
**教训**：写测断言要精确反映验的语义——「幂等」≠「无事件」。Move/scroll 等每次产的事件不能和 hover/diff 状态变化混在 `is_empty` 断言里。

### 坑 32：implementer 为让 brief 测通过改实现（已恢复，v1c.2）
implementer 为坑 31 brief 测 `out.is_empty()` 通过，改 Move handler 抑制 EVT_MOVE——超 scope + 破坏 §7.1 恒产 + 影响drag。fix 恢复 Move 无条件 emit + 改测断言（坑 31）。**教训**：brief 测 vs 既有语义冲突**改测不改实现**；implementer 遇冲突应 flag DONE_WITH_CONCERNS 让 controller adjudicate，controller review 验「超 brief scope 改动」。

### 坑 33：C# EventBridge internal 测跨 namespace 不可见（v1c.2）

**症状**：T3 `EventBridge`（internal）单测在 `LoomGUI.Tests` namespace 直接 `new EventBridge()`——跨 namespace 不可见，编译报 inaccessible。
**根因**：v1c.1 测只碰 public 类型，无此需求；v1c.2 EventBridge internal + 测直接构造触发。
**解决**：`EventBridge` internal → public（测访问 + 避 `InternalsVisibleTo` 配置；EventBridge public 无害，业务通过 AddListener 用不直接 new）。
**教训**：C# 类型可见性要考虑测访问——单测直接构造的类型须 public 或加 `[InternalsVisibleTo("Tests")]`（项目已有此机制，坑 13 同源 csbindgen InternalsVisibleTo）。

### 坑 34：`#[repr(C)]` enum 无显式整型 → 判别默认 isize=4B 非 1B（v1c.3）

**症状**：v1c.3 PointerEvent 设计预期 16B（kind 1B + button 1B + pad 2B + touch_id 4B + x/y 8B），但 `cargo` 实测 20B——C# 若声明 PointerKind:byte 会 mis-slice 整个输入数组。
**根因**：`PointerKind` 是 `#[repr(C)] pub enum { Down=0, Up=1, Move=2 }` 无 `repr(uN)` → Rust 用平台 isize（4B）作判别，非 1B。spec/plan 草稿想当然以为 C-like enum 1B。
**解决**：`PointerKind` 加 `#[repr(u8)]` → 1B 判别，PointerEvent 回 16B。C# PointerEvent.kind 对齐 byte。
**教训**：FFI 边界的 C-like enum **必须显式 `#[repr(uN)]`**（u8/u16/u32），否则判别 isize 跨平台不稳 + 撑大 struct。永远 `size_of::<T>()` 断言 ABI struct 尺寸，别信草稿。本坑由 T3 implementer 诚实上报（初版把 sizeof 断言改成实际 20，controller 决定修 core repr(u8) 回 16 而非接受 20）。

### 坑 35：csbindgen 不为 use-imported 的 `#[repr(C)]` struct 生成 C# stub（v1c.3，v1d.2 复发）
csbindgen 只扫 `#[no_mangle] fn` 签名不追 `use` 路径 → FFI struct（PointerEvent/EventRecord/KeyEvent）无 C# stub，编译报找不到类型。**解决**：手补 C# 镜像（`[StructLayout(Sequential)]` 字段序对齐 Rust `#[repr(C)]`）——PointerEvent→`LoomGUIPointerEvent.cs`、EventRecord→手写 `LoomEvent`、KeyEvent→`LoomGUIKeyEvent.cs`。改字段 + 新增 struct 都要同步（v1c.3 PointerEvent 加 touch_id 漏、v1d.2 新增 KeyEvent 漏，均 final review 捕）。**教训**：csbindgen 项目 FFI struct 是「Rust 真相源 + 手补 C# 镜像」双份——「新增/改 `#[repr(C)]` struct → grep C# 镜像/补文件」是 FFI task 必检项（C# 本机不编译家里机才暴露，坑 13/38 同源）。

### 坑 36：csbindgen FFI 数组参数是 `T*` 非 managed array，须 fixed-pin（v1c.3）

**症状**：brief/草稿写 `Native.loomgui_stage_set_input(stage, events.ToArray(), events.Count)`——C# 编译错，`set_input` 签名是 `PointerEvent* events`（raw 指针）非 `PointerEvent[]`。
**根因**：csbindgen `csharp_use_function_pointer(false)`（Mono 模式）发 raw 指针参数，不自动 marshal managed array。
**解决**：`fixed (Bindings.PointerEvent* p = arr) { Native.loomgui_stage_set_input((StageHandle*)stage, p, (nuint)arr.Length); }`——`fixed` pin managed array 取 T*，调用期内钉住。空数组走 `null, 0`（Rust FFI guard）。
**教训**：csbindgen FFI 的数组/缓冲参数都是 raw `T*` + `nuint len`，C# 侧必须 `fixed`-pin（值类型数组）或 `GCHandle.Alloc`（引用类型）取指针。别直接传 managed array。`set_input`/`borrow_events`/`borrow_frame` 全是这模式。

### 坑 37：recompute 化重构漏条件门控——disabled 节点按住仍 :active（v1c review）

**症状**：PlayMode 按住 disabled 按钮仍变红（:active 触发），违反 §4.4「disabled active/click 抑制」。
**根因**：v1c.3 把 v1c.1 命令式 `set_active_chain(Some(n))`（含 `!disabled` 门控）改成全局 `recompute_active` 沿 `down_node` 链设 active，**漏复制 disabled 门控**（Down handler `down_node=hit` 无条件赋值，disabled 检查前）。更深：hit 落 disabled 节点的非 disabled Text 子（坑 29 同款挡命中）时 down_node=Text 子，原 fix 只查 down_node 漏判链上 disabled 祖先。
**解决**：`recompute_active` 链遍历**逐节点查 disabled**（不只 down_node），遇 disabled **截断**（自身+祖先都不 active）。+ 回归测（直击 / Text 子击两条）。
**教训**：命令式 set_X 重构为全局 recompute_X 时**逐条复刻原 set 的所有条件门控**（disabled/visible/touchable），重构最易丢门控。active/hover 链遍历须逐节点查状态（hit 可能落非 disabled 子，状态在祖先链）。Down+Up 同帧的测掩盖「按住 disabled」case（recompute 时 is_down 已 false）——禁用/按住类测须**分离帧**。

### 坑 38：InputSystem 1.19 `TouchPhase` 在 `UnityEngine.InputSystem` 非 LowLevel；双 using 致歧义（v1c review）

**症状**：`LoomInputCollector` 编译 CS0234（TouchPhase 不在 LowLevel）→ 改未限定后又 CS0104（ambiguous，UnityEngine.TouchPhase vs InputSystem.TouchPhase）。
**根因**：1.19 包 `TouchPhase` enum 在 `UnityEngine.InputSystem` 命名空间（Touchscreen.cs:390），非草稿/记忆的 `LowLevel`；文件同时 `using UnityEngine;`+`using UnityEngine.InputSystem;` → 未限定 TouchPhase 两命名空间都有 → 歧义。
**解决**：新/旧输入路径**全限定**——新 `UnityEngine.InputSystem.TouchPhase`，旧 `UnityEngine.TouchPhase`（不同枚举显式区分）。
**教训**：Unity InputSystem API 路径随版本变，查包源定 namespace（`Library/PackageCache/com.unity.inputsystem@*/InputSystem/Devices/Touchscreen.cs`），别信草稿/记忆。**C# 本机不编译家里机才暴露**（坑 28/35 同源）——同 using 下两 namespace 同名类型须全限定。

### 坑 39：borrow_events 的 out_len 是记录 COUNT 非 bytes（v1c.4）

**症状**：cancel_click abi_test 切 borrow_events 返回 slice 用 `len / size_of::<EventRecord>()` → `1/20=0` 记录 → 空 slice → 「Up 仍发」断言失败。
**根因**：`loomgui_stage_borrow_events(h, *mut out_len)` 写入的是 `events.len()`（记录**条数**）非字节数；brief/测草稿误以为字节。
**解决**：直接用 `len` 作记录数切 `from_raw_parts(ptr, len)`（同既有 `set_input_borrow_events_round_trip` 测用 `len * rec_size` 取字节的反向印证）。
**教训**：两个 borrow 的 out_len 语义**不同**——`borrow_events`=**记录 count**（EventRecord 条数），`borrow_frame`=**字节**（blob 字节）。测/消费代码勿混用。**复踩（v1d.4）**：v1d.4 plan T6 brief 又写 `len/size_of`——plan 作者写 FFI 测前须回读本坑。

### 坑 40：Assert.IsNotNull(ptr) 对非托管指针装箱恒非 null（no-op）（v1c.4）

**症状**：BuildStage guard `Assert.IsNotNull(stagePtr, ...)`（`stagePtr` 是 `StageHandle*`）——`stage_new` 返 null（font_path 占位未填）时 guard **仍过** → `load_html(null)` 触 FFI 崩溃，非干净测失败。
**根因**：NUnit `Assert.IsNotNull(object)` **装箱**参数；非托管指针（`T*`/`IntPtr`）装箱成含值 0 的**非 null** 对象 → 恒过。
**解决**：指针用真比较——raw `T*` 用 `Assert.IsTrue(stagePtr != null, ...)`（unsafe 上下文产 bool）；`IntPtr` 用 `Assert.AreNotEqual(IntPtr.Zero, stage, ...)`。
**教训**：NUnit Null 断言只对**引用类型**有效；值类型/裸指针/IntPtr 须显式 `!= null`/`!= IntPtr.Zero` 比较。C# 测 guard 裸指针/句柄首查此项。

### 坑 41：跨 crate 的签名变更只跑定义 crate 测会漏改消费 crate（v1d.1-T1→T5）

**症状**：T1 把 `Scene::build` 入参从 5-tuple 改 6-tuple（+draggable）。implementer 跑 `cargo test -p loomgui_core` 全绿就 commit。到 T5 首次 `cargo test -p loomgui_ffi_c` 才发现 `loomgui_ffi_c/src/lib.rs` 的 5 个 abi_test 还在构造 5-tuple → 整个 ffi crate 编不过（T1-T4 从没编译过 ffi crate）。
**根因**：`Scene::build` 是 `loomgui_core` 的公共 API，被 `loomgui_ffi_c`（abi_test 手搓 entries）+ `loomgui_core` 自身测消费。改签名只跑定义 crate 的测，漏掉跨 crate 消费者。brief 列了"修所有 5-tuple 调用点"但 implementer grep 只在 loomgui_core 内搜（lib.rs abi_test 在另一 crate）。
**解决**：签名/公共 API 变更后，跑**所有消费 crate** 的 build/test（`cargo build --workspace` 或至少 `-p loomgui_core -p loomgui_ffi_c`），不只定义 crate。T5 把漏的 5 处机械补 `, false`（FFI 测不测 drag，false 默认对）。
**教训**：workspace 多 crate 时，公共类型/函数签名变更 = 跨 crate 破坏性改动；验证须覆盖全 workspace，非单 crate。plan brief 列调用点时也别只列一个 crate。

### 坑 42：safe-area forward 渲染变换与 inverse 输入映射必须逐项一致（v1d.1-T8 Critical）
初版 forward 用 uniform `sf`、inverse 用 per-axis stretch + offX 语义错 → notched 屏触控落点≠渲染点。根因：render（root transform）和 input（ScreenToDesign）是同一 design↔screen 映射两面，必须互为精确逆。**解决**：统一 uniform `sf=min(area.w/dw,area.h/dh)` + `offX=area.x+(area.w-dw*sf)/2`（设计 span 居中 safe 区）+ `ScreenToDesign` 逐项逆 `dx=(sx-offX)/sf`；符号恒等 + notched 6 点 round-trip 测锁。**教训**：render↔input 双向映射两侧公式必须同源互逆；坐标系变换须符号推导 + round-trip 测，不能只验 degenerate case（safe==full 掩盖）。

### 坑 43：给广泛构造的 struct 加字段，plan 按文件派任务会漏枚举所有构造点（v1d.2-T1→T4）
给 `Scene` 加字段，plan 按文件派任务漏了 render/hit/layout 的字面量 → 测编译失败。根因：广泛构造的 struct 加字段是全局 fallout，plan per-file 划分枚举不全。**解决**：加字段后全仓 grep 构造点（`grep -rn "Scene {"` + `Scene::build(`）一次性枚举，不靠 per-file 任务记忆。**教训**：struct 字段/签名变更 fallout 枚举要全仓 grep 驱动，controller pre-flight grep 构造点写进 brief。**v1d.4 优化**：加相邻 transient 字段（如 `anim` 紧跟 `world_transforms`）用 `replace_all` 一次命中全 46 处字面量，cargo build missing-field 兜底。

### 坑 44：compound_matches 不检伪类 → 伪类规则污染 base_style（v1d 验收）

**症状**：v1d 加 `:focus` 后所有 `.btn` 默认紫、`.drag` 默认棕黄（v1c.1 的 :hover/:active 本也污染 base，颜色/源序未察觉，:focus 源序最后胜才暴露）。
**根因**：`pack` 调 `resolve_styles(tree, &sheet)` 用**完整 sheet**，`parse::selector::compound_matches` 只检 tag/classes/id **不检伪类标志** → `.btn:focus` 匹配 .btn 进 base（cascade specificity 同级 (0,2,0)，源序最后者胜 → 全 .btn 紫）。
**解决**：`compound_matches` 开头 `if pseudo_hover||active||disabled||focus { return false }`——伪类规则只走运行时 `rematch_pseudo_classes`，不进 base cascade。+ 回归测 `pseudo_class_rules_excluded_from_base_cascade`（RED base=紫 → GREEN base=灰）。
**教训**：base cascade（resolve_styles）与动态 rematch 是**两条独立匹配路径**；加新伪类时 `compound_matches` 跳过 + `extract_dynamic_rules` has_pseudo + `compound_matches_with_state` 状态门**三处同步**，漏任一即污染/漏匹配。

### 坑 45：键盘采集套 InputSystem 过度设计（补丁已撤销，v1d.2-T6）
CollectKeys 照搬指针 InputSystem 路径用 `Keyboard[Key]` → KeyCode≠Key 40-case 映射补丁。fix 撤销，统一 `Input.GetKeyDown(KeyCode)`（Both 模式零转换）；指针 Collect 仍双路径（多触摸要 InputSystem）。**教训**：输入采集按需选 API，键盘/鼠标按钮旧 `Input.GetKey(KeyCode)` 够，别一刀切套 InputSystem。

### 坑 46：`pub type Affine2` 与 `pub mod Affine2` 同名 type namespace 冲突（v1d.3-T1 plan self-review）

**症状**：plan 初稿想用 `pub type Affine2=[f32;6]` + `pub mod Affine2 { pub const IDENTITY... }`（测试写 `Affine2::IDENTITY`）→ 编译错「conflicting definitions」。
**根因**：Rust type namespace 同时容纳 type alias 和 module，同名冲突（value namespace 才允许 fn/const 同名 type）。
**解决**：删 `mod Affine2`，用 free fn（`pub const IDENTITY`/`pub fn from_translate`）+ `Affine2Ext` trait（链式方法），测试 `use super::*` 直接 `IDENTITY`/`from_translate(...)`。
**教训**：type alias 别想同名建 mod 提供常量；free fn + trait 是 Rust 惯用替代。plan 写代码也要 self-review 编译可行性。

### 坑 47：matrix shader 共享 material `_ObjectMatrix` 被覆盖（v1d.3 M1，**修复被坑 73① 取代**）
两非纯平移节点同 material → `SetMatrix` 最后写者胜。原 M1 fix（MPB SetMatrix）不够——MPB 不覆盖非 Properties CBUFFER（坑 73① 纠正）→ 现拆 4 Vector Properties + SetVector ×4（见坑 73）。**教训**：共享 material 下 per-instance uniform 必走 MPB（per-renderer，不污染缓存）。

### 坑 48：matrix shader GO identity 致 Mesh.bounds 剔除错位（v1d.3 I1，**补丁被坑 73③ 删除**）
非纯平移 GO identity + shader 移顶点 → Mesh.bounds 不反映世界变换 → 剔除错位。原 I1 fix（mutate bounds.center 到世界）是**补丁**：pure↔非 pure 切换双 translate 污染 mesh 资产（坑 73③）。现方案：translate 进 GO localPosition，_ObjectMatrix 只 scale/rotate → renderer.bounds 自动 world（见坑 73）。**教训**：别 mutate Mesh.bounds 做 culling（持久资产）；用 GO transform 让 bounds 自动 world。

### 坑 49：matrix shader 渲染须分顶点 re-base 两路径（v1d.3-T4 核心正确性）

**症状**：若所有节点统一 blob re-base 减 transform.x/y，非纯平移节点 world top-left≠layout top-left（旋转后偏移），re-base 错 → 顶点飞。
**根因**：blob 的 `local_x/local_y`（现 world matrix tx/ty）双用：GO 定位 + mesh re-base。identity 时两值同（layout top-left=world top-left），非 identity 时 world top-left≠layout top-left，统一减 tx,ty 对非 identity 错。
**解决**：render + blob 按纯平移分两路径——**identity**：render 产绝对顶点 + blob re-base 减 tx,ty → top-local + GO position=tx,ty（现状零改）；**非纯平移**：render 产 box 本地 (0..w)（减 layout_rect.xy）+ blob **不 re-base** + GO transform=identity + shader matrix。两处判断一致（同 `is_pure_translation`）。
**教训**：blob transform 列语义从「layout 绝对」变「world 累计」时，re-base 基准必须分路径（identity 走 layout top-left，非 identity 走 box 本地）。这是 transform 渲染最易藏 bug 处，需跨 render/mod.rs + blob.rs 一致。

### 坑 50：LoomGUIBindings.cs 是 csbindgen 自动生成 + gitignored，手写会被覆盖（v1d.4-T7）

**症状**：v1d.4 plan T7 让 implementer 手动给 `LoomGUIBindings.cs` 加 4 个 `loomgui_stage_tween*` DllImport。implementer 发现文件已被 T6 的 `cargo build` 自动再生出这 4 个，且文件 **gitignored**（`.gitignore:40 **/LoomGUI*Bindings*.cs`）——手写多余且会被下次 build 覆盖。
**根因**：`loomgui_ffi_c/build.rs` 每次 `cargo build` 跑 csbindgen 扫 `src/lib.rs` 的 `#[no_mangle]` → 写 `loomgui_unity/.../Bindings/LoomGUIBindings.cs`（best-effort，纯 Rust 构建时 Unity 目录可能不在则 `cargo:warning`）。文件是**构建产物**非源码，故 gitignored。
**解决**：新增 Rust `#[no_mangle]` FFI 符号后 `cargo build` 自动再生 C# 绑定——**绝不手编 LoomGUIBindings.cs**。C# 侧只手写**应用层镜像**（非 FFI 类型的 enum 如 TweenProp/Ease、wrapper 方法、EventType 路由）。家里机/CI 须在含 Unity 目录的仓库根 `cargo build` 才再生绑定。
**教训**：FFI 绑定是 csbindgen 单向产物（Rust→C#）；把"加 DllImport"当独立 C# 任务是错的——它是 Rust `#[no_mangle]` 任务的副产物。判断绑定文件是否手编：看 build.rs 是否 csbindgen 生成 + .gitignore 是否排除。

### 坑 51：shader matrix 路径漏 root transform（design world 当 Unity world）（v1d.3 验收）

**症状**：v1d.3 非纯平移节点（rotate/scale/skew）渲染位置/翻转/缩放全错，且与命中（design world matrix 逆投）不一致 → 点视觉位置点不到。
**根因**：shader matrix 路径 `worldPos=mul(_ObjectMatrix, box-local)` = **design world**，直接 `TransformWorldToHClip(designWorld)` 把 design 坐标当 Unity world——漏了 root GO transform（`sf,-sf,sf`+rootPos）。TRS 路径 `TransformObjectToWorld(v.pos)` 自动经 GO+root 全链，matrix 路径跳过了。
**解决**：matrix 路径 `worldPos=TransformObjectToWorld(designWorld)`（GO 是 root 子 + transform=identity → ObjectToWorld=root_ObjectToWorld，补回 design→Unity world）。两路径统一成 `root × design world`。
**教训**：core 算 design world matrix，Unity 渲染要 Unity world——桥接靠 `TransformObjectToWorld`（含 root transform）。spec §1.6 漏写这步，实现期补。坑 42（render↔input 映射一致）同类。

### 坑 52：ShaderLab 无 Matrix property + MPB 覆盖范围（v1d.3 验收，**修复方案被坑 73① 纠正为错**）
① Properties 无 Matrix 类型（只 Float/Vector/2D 等）；② MPB 只覆盖 material property。原方案「放 CBUFFER 无 Properties 对应 + MPB SetMatrix 按 name 覆盖」**错**——坑 73① 实测 MPB 不覆盖非 Properties CBUFFER 字段。现方案：拆 4 Vector Properties + SetVector ×4（见坑 73）。**教训**：MPB 只覆盖 Properties 字段；ShaderLab 无 Matrix property 类型。

### 坑 53：Unity PlayMode 首帧 Time.unscaledDeltaTime spike（tween 瞬间 complete）（v1d.4 验收）

**症状**：v1d.4 demo Start 注册 tween，Play 后 popup 无动画（opacity/scale 直接末值），dump 显示 `anim_op=1.000`（非渐变）但 complete 事件出了。
**根因**：Unity PlayMode 首帧 `Time.unscaledDeltaTime` 可达数秒（场景/library 加载延迟，实测 **dt=2.07s**）→ `advance_time(dt)` → tween `elapsed+=dt` 第一帧 `>=duration` → 瞬间 complete 写末值。dump_cdylib 固定 dt=0.15 渐变正常（证 core 对）。
**解决**：demo 改按钮/Space 触发（Play 后 dt 稳定再注册 tween）；core 不 clamp dt（破坏 stage 测 + time_s 语义，YAGNI）。
**教训**：Unity 首帧 unscaledDeltaTime spike 是通用陷阱；tween/动画别在 Start 自动播（首帧 dt 异常），用按钮/延迟/WaitEndOfFrame 触发。core time-based 逻辑（tween/longpress/双击）都对 dt spike 敏感——业务负责避开首帧。

### 坑 54：fgui `v2` 变量名误导——是 `|v|·scale` 非 `v²`（v1d.5-T6 reviewer 抓）
**症状**：scroll 惯性时长偏长 ~3x（v=2000px/s→5.5s）+ 触发阈值过敏 ~22x（v²>500 → |v|>22 即触发）。
**根因**：fgui `ScrollPane.cs:2060 UpdateTargetAndDuration` 里 `v2 = Mathf.Abs(v) * _velocityScale`（velocityScale 默认 1）——变量名 `v2` 望文生义像 v²，实为**线性 |v|·scale**。spec/plan 转录成 `v2 = v*v`（平方），duration=`log(60/v²,…)` + 阈值对 v² 判定全错。implementer 忠实 brief 无错，是 spec 转录 bug。
**解决**：`begin_inertia` 改 `let v2 = v.abs();`（duration + 阈值用线性 |v|）；`change = v*dur*0.4` 仍用 signed v 保方向。修后 v=2000→1.7s、阈值 |v|>500(PC) 合理。
**教训**：移植 fgui 算法时，**带数字后缀的变量名（v2/pos2/d2）不能望文生义**——须读源码表达式确认是平方还是线性命名残留。fgui 变量命名不一致（v2 非 v²）是通用转录陷阱。

### 坑 55：合成节点 sentinel id 跨 hit/render/wheel 须一致解码（v1d.5-T9 reviewer Critical）
**症状**：T9 改 `hit_test` 可返 sentinel thumb_id（`container|0x4000_0000`）后，T8 的 `apply_wheel_to_hit` while 循环 `scene.nodes[id.0].parent` 越界 crash（sentinel 0x4000_0000 >> nodes.len()）。T10 接 tick 后滚轮划过 scrollbar 区域必崩。
**根因**：T9 引入 sentinel id（合成 scrollbar thumb 不进 Scene 但要 hit-testable + MirrorPool 镜像），T8 的 wheel 路径读 `id.0` 索引 scene.nodes 未识别 sentinel。**跨 task 改动**：T9 改 hit_test 返回类型语义，T8 既有消费侧未同步守卫。
**解决**：`apply_wheel_to_hit` while 顶解码 `if id.0 & 0x6000_0000 != 0 { id = NodeId(id.0 & !0x6000_0000) }`（0x6000_0000=V|H flag 并集；清高位回 container_id），container 自身 effective 命中 apply_wheel。Up 段 `!grip_dragging` 守卫防 `scene.nodes[sentinel]` OOB。
**教训**：引入 sentinel/魔法 id 时，**所有读 id 做 scene 索引/沿 parent 链的路径**（hit_test/wheel/事件路由/仲裁）都须识别+解码 sentinel——跨 task 最易漏（各 task 只看自己的消费侧）。LoomGUI 首次用 sentinel id（合成节点），新路径。

### 坑 56：dirty hash 字段集遗漏致持续视觉错（v1e final review Critical）
dirty hash Mesh arm 只 hash `texture+verts.len+colors[0]` → `.btn:hover{width}` 改 size→verts 坐标变但 len/colors 不变 → hash 不变 → 误判 Unchanged（持续，非 1 帧延迟）。根因：quad 定 4 顶点，尺寸变体现在 verts 坐标非数量；Text 同族（align 改 pen_x/pen_y 但 glyph_count 不变）。**解决**：Mesh 加 verts[0]/verts[2] 首末顶点坐标 hash，Text 加首字 pen_x/pen_y，移除占位 sort_key/mask_context。**教训**：dirty hash 须覆盖体现几何变化的坐标字段（verts/glyph pen）非只 count/tex_id；字段集完整性是 final review 级审查项——per-task reviewer 易顺着 brief 验（brief 本身漏），须从"哪些视觉变化该触发重传"反推。

### 坑 57：plan/草稿写围栏外标签或属性——标签硬挡、属性静默死 CSS（v1-showcase T3/T7）
**症状**：v1-showcase plan §2 用 `<i>` 标签（justify 卡子项标记）→ 打包失败；plan §4.7 用 `position:absolute`+`left/top`（pointer-events 叠加演示）→ CSS 死代码（parse 静默忽略），reviewer 抓到。
**根因**：core parse 有 **FENCE_TAGS 硬白名单**（`parse/dom.rs`，仅 div/span/img/button——l-container 砍，坑 94）——围栏外标签 parse 失败打包报错；CSS 属性走 `style/mapping.rs` match，围栏外属性（position/left/top/z-index/background-image/font-style/grid/border-radius/渐变等）落 `_ => false` **静默忽略**（死 CSS 不报错）。v1 纯 taffy flexbox，**无 position/z-index/叠加**。
**解决**：`<i>`→`<span>`（CSS `.flx i`→`.flx span`）；删 position:absolute，pointer-events 演示改流内块 + 说明 v1 无叠加。
**教训**：写 sample HTML/CSS 前对照 FENCE_TAGS（标签）+ mapping.rs 白名单（属性）。**标签违规易发现**（打包失败），**属性违规隐蔽**（静默死 CSS，reviewer 须逐条扫 CSS 声明）。这是 AI 可预测性的打包期第一道反馈——围栏验证器工作正常。

### 坑 58：scroll offset 被 blob re-base 抵消——"控件不动仅文字动"（v1d.5 家里机验收）
**症状**：scroll 拖动时只有 Text 跟手，Mesh 控件（卡片背景/色块）纹丝不动。
**根因**：scroll offset 注入 `world_matrix.m_tx`。blob 纯平移 re-base `vert - world.tx`（坑 49）把 scroll 从 GO 挪进 vert → 渲染 `GO(world.tx)+vert(剩余)` 抵消回 layout。Text 不走 re-base（pen GO-local + GO at world.tx）所以跟随。
**解决**：render 纯平移 `rect = (wm[4],wm[5],w,h)`（world.tx 位置）非绝对 layout_rect → vert=world 位置 → re-base 减 world.tx 正好 top-local → 渲染=world.tx=layout-scroll。零回归（无 scroll 时 world.tx=layout）。
**教训**：scroll offset 与 v1d.3「绝对 vert + re-base 减 world.tx」冲突；改 scroll 进 world_matrix 后必验「Mesh 控件跟手」（不只 Text），抵消在 blob 层极隐蔽。

### 坑 59：overflow 容器被 content 撑开 + 子被 shrink——overlap=0 拖不动（v1d.5 家里机验收）
**症状**：scroll 容器 overlap=0（拖不动）—— main-scroll viewport=content=7311（被撑开）；mini `.filler{height:300}` 被 shrink 到 viewport（不溢出）。
**根因**：CSS flex §4.5 规定 overflow≠visible 的 flex item automatic min-size=0（不被 content 撑开）；LoomGUI 没设 taffy `Style.overflow`（用自己字段）→ taffy 默认 Visible → min-size=min-content → 容器被撑。同理 overflow 容器的直接空内容子（filler min-content=0）被 flex-shrink 收缩到 viewport。
**解决**：① layout build 设 `style.overflow = map(overflow_x/y)`（LoomGUI OverflowMode→taffy Overflow，Auto→Scroll）让 taffy flex automatic min=0；② build 加 `parent_overflow` 参数，overflow 容器直接子 `flex_shrink=0`。
**教训**：taffy 0.5 实现了 CSS flex §4.5（style/mod.rs:124 注释明说），但**需设 `Style.overflow` 字段触发**——LoomGUI 用自己 overflow 字段时必须同步设 taffy Style.overflow。dump_scroll 实测 overlap（别猜代码）。

### 坑 60：scroll 调试套娃——先验 layout 再改物理（v1d.5 家里机验收）
scroll 家里机拖动异常，逐层修 6 套娃 bug：drag 方向反 / x 轴 overlap=0 仍 apply delta 斜拖抖 / overflow 容器撑开（坑 59）/ 子 shrink（坑 59）/ sentinel 进 batch reorder 越界 panic / re-base 抵消（坑 58）。根因：scroll 跨 layout/render/blob/MirrorPool/merge 五层，bug 互相掩盖。**解决**：逐层 TDD + `dump_scroll` 实测 overlap 定位"哪层错"；sentinel 在 `build_render_nodes` 末尾 merge 后追加（不进 reorder）。**教训**：跨层特性 PlayMode 报「拖不动/晃动」先 example 实测 core 状态（overlap/scroll_pos/content_size）再改，避免盲改物理掩盖 layout 根因。

### 坑 61：cascade 不解析 inline `style="..."`（v0 缺口，v1-showcase 验收）
**症状**：`<div class="sw" style="background-color:#1a1d2e">` 色块透明看不见；§2 flx `style="flex-direction:column"` 被忽略（class row 兜底）。
**根因**：v0 cascade 只处理 StyleSheet rules，不解析元素 `style` attr（dom.rs attrs 收集了但 resolve 没用，cascade.rs 测试注释明写「v0 style 属性未在 dom 层解析」）。
**解决**：`css::parse_inline_style`（复用 DeclParser+RuleBodyParser，无 selector 的 declaration list）+ `cascade::resolve_styles` sheet rules 后 apply inline（specificity 最高，最后胜出）。
**教训**：inline style 是 CSS 契约，v0 缺口致 showcase 大量 `style=` 静默失效；加时复用 cssparser（手写 split 对 `url()`/注释脆弱）。

### 坑 62：Linear 项目 vertex color 没 sRGB→linear → 整体灰蒙蒙（v1-showcase 验收）
**症状**：v1-showcase 整体偏浅灰（灰蒙蒙），vs html 浏览器深蓝 dashboard；两边 letterbox 蓝（Main Camera 改 #1a1d2e）但中间 root 区灰。
**根因**：项目 Linear color space，CSS 颜色是 sRGB 编码（#1a1d2e=0.102）；Unity 不自动把 vertex color sRGB→linear → 当 linear 值 → 显示成 sRGB(0.102 linear)=0.35（浅灰蓝）。
**解决**：shader `frag` 手写 SRGBToLinear（精确 `(c<=0.04045)?c/12.92:pow((c+0.055)/1.055,2.4)`）应用于 vcolor.rgb（alpha 不转）；texture sRGB format 自动转不重复。
**教训**：Linear 项目 UI shader，vertex color（CSS sRGB）须手动 sRGB→linear；URP Color.hlsl include 路径不稳（`Couldn't open include file`），手写公式最稳。判据：背景色对但整体偏浅发灰 = color space 问题。

### 坑 63：font atlas alpha-mask（rgb 黑）→ 文字黑（v1-showcase 验收）
**症状**：坑 62 修后背景对了，但文字全黑看不清（html 是白）。
**根因**：font atlas 是 alpha-mask（字形在 alpha，rgb=黑）；shader `col=tex×vcol` 把 tex.rgb(黑)×vcol(白)=黑。core text color 正确（dump 验 #e0e0e0）传到 Unity。
**解决**：shader 加 `ALPHA_MASK` keyword（`#pragma multi_compile`，MaterialManager `program==1` 启用）：frag 分支 text=`half4(vcol.rgb, vcol.a*tex.a)`（用 vcol 色 + tex.a 字形 coverage），image=`tex*vcol`（彩色 texture rgb）。
**教训**：font atlas 单通道 alpha-mask 不能当普通 RGB texture 乘；text/image 须 shader 分支（program:1 text vs program:0 image）。诊断：TextRasterizer 加 Debug.Log 验 textColor 传对 → 黑在 shader/atlas 端。

### 坑 64：img UV v 翻转（design y-down ↔ Unity y-up，v1-showcase 验收）
**症状**：`<img>` 在 Unity 上下颠倒（text 不颠倒）。
**根因**：design y-down + LoomStage `localScale=(sf,-sf,sf)`（y-flip）→ img quad TL（design 顶）应映 texture 顶 (umin,vmax)；`mesh::quad` 固定 TL→(umin,vmin)（texture 底）→ 颠倒。text 走 TextRasterizer 用 `uvBottomLeft/uvTopLeft` 故不颠。
**解决**：render img 调 `quad(rect, white, [uv_min[0],uv_max[1]], [uv_max[0],uv_min[1]])`（swap v）；mesh::quad 不改（背景色块 UV 全图无方向）。
**教训**：img/纹理 quad UV 须配 design→Unity y-flip（TL→texture 顶）；text 独立 UV 路径不受影响。

### 坑 65：img 只设一维 → 另一维没等比（v1-showcase 验收）
**症状**：`<img style="width:48px">` 只宽变，高度没变（html 宽高一起等比）。
**根因**：layout img w/h 独立取（CSS Length > texture 原值 > 64），只设 width 时 h=texture 原 height（非等比）。
**解决**：layout img `match (w_css, h_css)`：两维都设→各自；只 width→`h=w*ih/iw`；只 height→`w=h*iw/ih`；都 auto→intrinsic。
**教训**：img 尺寸按 CSS 等比规则（只设一维按 intrinsic ratio），非两维独立取 texture 原值。

### 坑 66：改 parse-time style 逻辑必须重打 pkg（base_style 打包期烤，v1-showcase 验收）
**症状**：改 cascade（inline style 解析）后重编 .dll，色块仍不显示；重打 pkg 才对。
**根因**：`Node.base_style` 是**打包期 resolve_styles 产物**（不变，rematch 基线）；`Stage::load_package` runtime 不重 resolve（只 rematch 动态规则）。改 cascade/mapping/parse 只重编 .dll 不够。
**解决**：改 parse-time style 逻辑（cascade/resolve/mapping/parse）必须 `cargo run -p loomgui_pkg` 重打 pkg（html/css 未变也要）；纯 runtime（render/layout measure/scroll/anim）改 .dll 即可。
**教训**：分 parse-time（进 pkg base_style）vs runtime（用 pkg）逻辑；前者改重打 pkg，后者改 .dll。`dump_sw` example 验 pkg 里节点 base_style 值确认是否进包。

### 坑 67：layout/render 双测量 text 换行不一致（v1-showcase 验收，**已修·方案 A**）
showcase 72 短标题末字溢出到无高度的第 2 行。根因（推翻"浮点边界"猜测）：`measure_text` 独立调用两次，max_width 来源不同——layout（taffy 闭包）短文本只传 `None`→1 行；render 永远用 `rect.w`(stretch available) 重测，短文本 intrinsic 亚像素超 available → 误判 2 行。**解决**（方案 A，layout 为唯一测量权威）：`Scene.text_layouts` transient 字段存 layout 闭包"Some 优先"结果，render 复用（fallback `measure_text(rect.w)` 保 test 兼容）。**教训**：双测量是不一致之源；render 须复用 layout 结果非用 rect.w 重测；"浮点边界/epsilon"全是症状层猜测，dump 边界取证（`[LM] known=None` 揭示 taffy 传参）才定位真因。

### 坑 68：img Percent 压扁 + width:auto→0 不渲染（v1-showcase §1.3 验收）
两独立 bug：① **Percent 压扁**——measure 闭包对 Image 直接返 build 时 intrinsic (iw,ih) 不消费 taffy `known.width`，Percent width 图被定宽 500 但 height 用 intrinsic 64（没等比）→ 压扁；② **auto→0**——`parse_dimension` 没 handle `"auto"` 走 fallback `Length(0.0)` → rect=(0,0) 不渲染。**解决**：Image measure 存原始 `{iw,ih,w_dim,h_dim}` 闭包消费 `known` 解析（覆盖坑 65 + Percent）；`parse_dimension` 加 `"auto"→Dimension::Auto`；**改后重打 pkg**（parse-time，坑 66）。**教训**：Image measure 必须消费 taffy `known`；`width:auto` 是 CSS 默认须 Auto≠Length(0)；诊断用 `dump_img`（css.w/css.h/rect/tex 四列）+ 闭包 instrument `[IMG] known.w`。

### 坑 69：滚动松手物理（对照 fgui，v1-showcase 验收）
bug1 小拖松手回原位；bug2 快速拖到顶/底"先露空白再突然回弹"。根因：begin_inertia 硬阈值全速 inertia（界内小拖冲越界 snap）+ inertia change 未运行时截断（pos 冲远越界）。**解决**（对照 fgui）：begin_inertia 二次 ratio `((v2-thresh)/thresh)²` 削弱低速 + 越界松手直接 bounce 不 inertia；advance 运行时越界>20px 截断启回弹 tween（inertia target 不预 clamp，弹性过冲靠运行时检测）。**教训**：fgui inertia target 故意不 clamp，弹性过冲靠 RunTween 每帧检测越界>20 截断（手感来源）；初版 target-clamp 修 bug 但丢弹性（边界硬停），改运行时截断才对。fgui 行号见 §6。

### 坑 70：drag 越界双重打折致回弹弱（对照 fgui，v1-showcase 验收）
坑 69 方案 B 反馈"回弹弱"。根因：drag_follow 越界 `over=min(|np|,vp*0.5); np=-over*0.5` 双重打折（最大 vp*0.25），fgui 单打折（最大 vp*0.5）。**解决**：改 `dampened=min((lo-np)*PULL_RATIO, vp*PULL_RATIO)` 单打折，回弹翻倍。**教训**：对照 fgui 时 `min(a*c,b*c)` ≠ `min(a,b)*c`（先 cap 再打折 vs 单打折），抄公式先展开代数确认。

### 坑 71：CSS 分组选择器 `.op,.tr` 逗号不展开——规则整条失效（v1-showcase §3 验收）
**症状**：§3 .op/.tr 蓝底不显示，width/height/bg 全丢（rect 压成文字 intrinsic 尺寸）。
**根因**：parse_css 把 `.op,.tr` 整段 selector_text 喂 parse_selector，后者不认逗号 → compound class 切成 `["op,","tr"]`（逗号进 token）→ 要求元素同时含 "op," 和 "tr" → 永不匹配。
**解决**：parse_css 按逗号展开成多条 Rule（共享 declarations）；parse_selector/match_element 不感知逗号。
**教训**：CSS 分组选择器在 parse_css 层（prelude 整段切逗号）展开，别让 selector parser 处理。dump_render example 一眼定位（.tr rect 39x25 + no-bg vs 期望 80x60 + bg）。

### 坑 72：NativeHost GO 挂 root handedness flip——3D GO 被 cull（v1-showcase §1.6 验收）
**症状**：1m³ Cube 放 NativeHost 看不见 + 上下颠倒。
**根因**：LoomGUI `root.localScale=(sf,-sf,sf)` 在 transform 做 y-flip（fgui Stage `(upp,upp,upp)` 全正，y-flip 放 StageCamera position）→ GO 挂 root 子树 handedness flip（det<0）→ mesh winding 反 → Cull Back 剔除。
**解决**：建 `_container` 挂 root `localScale=(1,-1,1)` → worldScale=(sf,sf,sf) positive 翻正 handedness；GO 挂 _container + layer=LoomUILayer + material renderQueue=3000 + sortingOrder=sort_key（照 fgui GoWrapper 渲染顺序）。per-node wrapper（fgui GoWrapper cachedTransform 两层结构）保留用户 GO scale（Sync 设 wrapper 不动用户 GO）。
**教训**：LoomGUI root y-flip（vs fgui Stage 全正）致子树 handedness flip——3D GO/粒子挂 root 必 cull。独立 _container 翻正（不能改 root，UI mesh 依赖 y-flip）。照 fgui `temp/FairyGUI-unity/Assets/Scripts/Core/GoWrapper.cs`。

### 坑 73：_ObjectMatrix 三层 bug——非 pure 节点字消失（v1-showcase 按钮验收）
**症状**：按钮 :active{transform:scale} 字消失（按下/松手切换 pure↔非 pure 触发；按住显示松手消失循环）。
**根因**（三层叠加）：① `_ObjectMatrix` 在 CBUFFER 无 Properties 对应 → MPB.SetMatrix 不覆盖（**纠正坑 52**：非 Properties CBUFFER 字段 MPB 也不覆盖，坑 52「放 CBUFFER MPB 覆盖」错）；② 拆 4 Vector 后 HLSL `float4x4(v0..v3)` 是 **row-major**（v0..3=行），MirrorPool `GetColumn`（列）错位 → Mtx/Mty 没进 x/y（跑 w 分量）+ Mb/Mc 错位；③ I1 fix（坑 48）mutate `Mesh.bounds.center` 做 culling，非 pure→pure 切回时 GO localPosition=(Mtx,Mty) + mesh bounds（含旧 Mtx）= 双 translate → frustum culling 误剔（bounds 跑到 design y≈2630 超视口）。
**解决**：① `_ObjectMatrix` 拆 4 Vector Properties + MPB SetVector ×4；② `GetRow`（配 HLSL row-major）；③ 删 I1 fix，非 pure 也 GO localPosition=(Mtx,Mty)（translate 进 GO），_ObjectMatrix 只 scale/rotate → renderer.bounds 自动 world（culling 正确）。
**教训**：MPB 只覆盖 Properties 字段（坑 52 需纠正）；HLSL `float4x4(v0..v3)` row-major 不是 column；**别 mutate Mesh.bounds 做 culling**（mesh 持久资产，pure↔非 pure 切换污染）——用 GO transform 让 renderer.bounds 自动 world。

### 坑 74：ResolvedStyle 加字段必 bump PKG_FORMAT_VERSION（v1.1 background-image）
**症状**：spec 写"零改 FFI/blob"，但 Task 1 给 ResolvedStyle 加 `background_image`/`background_size` 两字段后，最终审查发现 `PKG_FORMAT_VERSION` 没 bump。
**根因**：`ResolvedStyle` 经 `bincode::serialize(&n.style)` 进 pkg.bin 每节点（`asset/mod.rs:188`）——bincode 是**字段序定长无 schema**，struct 中段加字段改变每节点 style blob 字节布局。版本仍 7 → 旧 v7 pkg 过 version gate 但反序列化 garbage（不 panic 即静默坏）。
**解决**：加 ResolvedStyle 字段 = bump `PKG_FORMAT_VERSION`（7→8）+ MIN/MAX（约定"拒绝+重打无迁移"，v5→6→7 同）。更新 `read_rejects_unsupported_version` 测试基线。C# 不解析 pkg（Rust-internal）零改。
**教训**：spec 写"零改 blob"前先查 ResolvedStyle 是否 pkg 载荷——**它是**。任何进 pkg.bin 的结构（ResolvedStyle/DynamicRuleTable）加字段必 bump version。设计阶段就该标出，别等最终审查。

### 坑 75：dirty hash 跳 uvs → background-size 单独变 stale（v1.1 background-image）
**症状**：`:hover` 切 `background-size: 100%`→`cover`（同纹理）渲染不更新。
**根因**：`dirty.rs:29` Mesh hash 解构 `Mesh { texture, verts, colors, .. }` 用 `..` 跳 `uvs`。background-size 变 → `fit_uv` 重算 UV，但 texture/verts/colors 不变 → hash 不变 → Unchanged → 新 UV 不 emit。Task 4 只验 background_image 变（改 texture→hash 变）漏了 size 单独变。
**解决**：destructure 加 `uvs` + hash `uvs[0]`/`uvs[2]`（同 verts 首末摘要模式）+ `mesh_uv_change_changes_hash` 测试。
**教训**：dirty hash 摘要要覆盖**所有渲染可见字段**。Mesh 的 `uvs` 是渲染可见的（UV 变=采样区变），`..` 跳过=漏。加新渲染路径（fit_uv）时审查 dirty hash 是否捕到其产物。

### 坑 76：dirty hash 采样首末顶点 → 圆角 mesh 非首末角变 stale（v1.2 border-radius）
**症状**：仅 BL 角非零且 BL 半径变（如 `border-bottom-left-radius:4px`→`5px`，TL/TR/BR 全 0，sides 不变）→ 渲染不更新（stale Unchanged）。
**根因**：§2.24 dirty hash 对 Mesh 只采样 `verts[0]`(中心)/`verts[2]`(TL 第二弧顶点)。圆角 mesh 25 顶点时 `verts[0]`=矩形中心（不随半径变），`verts[2]` 只反映 TL 角——BL-only 变 + sides 不变 → `verts.len` 也不变 → hash 不变。坑 75 修了"跳 uvs"，但采样摘要在多顶点 mesh 上仍漏非首末顶点。最终 opus 审查抓出（spec §9.1 要求的 `radius_0_to_8_hash_changes` 回归测试 plan 漏写）。
**解决**：`verts.len()>4` 时 hash **全顶点/全 UV**（圆角 mesh ≤33 顶点，成本可忽略）；`<=4`(quad) 保留首末采样（零回归 hash 不变）。加 `bl_radius_4_to_5_hash_changes` 测试（同 verts.len，仅 BL 半径变 → hash 必变）。注意 colors 哈希位置在 if/else 前（保 quad 流顺序零回归，re-review 抓的 Important）。
**教训**：dirty hash 采样摘要（首末/N 个）只对**固定顶点数** mesh 安全；变顶点数 mesh（圆角/多边形）须 hash 全顶点。摘要策略要随 mesh 类型分档，不能一套首末通吃。

### 坑 77：rounded_rect 混合椭圆角直角分支落圆心+方向偏离矩形顶点（v1.2 家里机验收）
**症状**：`border-radius:0 8px / 8px`（TL/BR 水平半径 0 → 直角，TR/BL 真弧）→ TL/BR 角附近镂空。
**根因**：直角分支 `corner_pt = 圆心 + (cos(start)·rx, sin(start)·ry)`。rx=0 ry>0 时 py=圆心.y+sin·ry 偏离矩形顶点（TL 落 [0,8] 非 [0,0]）。spec §5.1 注已预见"实现时可更直白按角序硬编码四角顶点"，但实现者照 brief 代码用了圆心+方向法。
**解决**：corners 元组附矩形顶点 corner（TL=[x,y]/TR=[x+w,y]/BR=[x+w,y+h]/BL=[x,y+h]），直角分支直接 push corner，不依赖圆心+方向。
**教训**：spec 注预见的简化方案别跳过——圆心+方向法在 rx=ry 时巧合正确（ry=0 不偏移），混合椭圆角（一轴 0 一轴 >0）才暴露。几何分支的退化 case（某半径 0）须显式落已知点，别靠通用公式。家里机实机验收才抓到（单测用对称半径漏）。

### 坑 78：scroll + CLIPPED clip rect design/world 空间错位全裁（v1.1/v1.2 家里机验收）
**症状**：`overflow:hidden` 容器（bg-demo/br-demo）scroll 时内容全空（col.a=0 全裁）。
**根因**：`transform.rs` 给子节点 world_matrix 注入 `T(-祖先.scroll_pos)`，节点 world 在 `(layout - scroll_offset)` 空间。但 CLIPPED 的 `_ClipBox` 由 `ComputeClipBox(root, design_rect)` 算（design 空间，不含 scroll），shader `clipPos = worldPos.xy × _ClipBox.zw + _ClipBox.xy` 用 world（含 scroll）——空间错位 → scroll > clip 半边时 clipPos 超界 → `step(max,1)=0` → col.a=0 全裁。scroll 越深越明显。
**解决**：`batch.rs::assign_sort_keys` dfs 加 `scroll_offset` 累积参数。**own clip 减 scroll_offset**（本节点 world 在 layout-scroll_offset 空间，clip rect 同空间）；**accumulated 不减 scroll**（祖先 clip 如 scroll 容器 viewport 在 world 固定——容器自身 world 不含自己 scroll_pos）；`intersected = accumulated(design) ∩ own(world)` = world 可见区。v1 错修（accumulated 也减 → viewport 跟着移，design 不相交同减仍 h=0），v2 才对。
**教训**：clip rect 空间必须与 shader clipPos 空间一致（都 world 或都 design）。scroll 容器 viewport 在 world 固定（容器自身 world 不含自己 scroll_pos），子节点 world 含 scroll——两者求交须先把 own 转 world。家里机用 `dump_clip_scroll` 实机 dump（先 tick 建 scroll 表再 set_scroll_pos）才发现 v1 错——本机单测测不出（无 scroll 运行时）。

### 坑 79：shader `tex×vcol` 非 CSS 合成 → 图透明区全透明不透 bg-color（v1.1 spec §6.2 承诺错误）
**症状**：§1.6 home.png（透明背景 icon）+ bg-color 共存，图透明区透出 root 深蓝，透不出 bg-color（青/红底看不见）。
**根因**：shader `LoomGUI/Unlit` program:0 frag `col = tex * vcol` = `(tex.rgb×vcol.rgb, tex.a×vcol.a)`。图透明区 `tex.a=0 → col.a=0` 全透明——是简单 tint，**非 CSS background 合成**（图透明区应透 bg-color）。v1.1 spec §6.2 承诺"shader mainTexture×vertexColor：图透明区透出 background-color"是**数学错误**（tex×vcol 在透明区得 0，透不出 vcol）。
**解决**（方案 A，**已实现** `0e941a3`..`0bf2b46`）：img 保持 program:0（tex×vcol，图透明透下层）；Container+bg-image 命中纹理（tex_id≠0）→ program:2 + `BG_COMPOSITE` keyword 走真合成 `col.rgb=tex.rgb×tex.a + vcol.rgb×(1-tex.a); col.a=vcol.a`。program 进 frame blob 第 19 列（u8，VERSION 4→5）——img 和 Container+bg-image 都用 tex1，shader  靠 program 区分。无图/未注册 Container 保持 program:0。
**实现要点**：① `NodePayload::Mesh.program` 字段 v1.2 早已埋好（半成品），核心 gap 仅 FFI 序列化层——blob 没传 program，MirrorPool 硬编码 0；加列后 MirrorPool 用 `blob.Program(i)`。② program 列 u8，payload 字段 u32 → 序列化 `*program as u8`（值域 0/1/2 安全）。③ shader 合成在 **linear 空间**正确：tex sRGB 自动转 + vcol 上方手动 sRGB→linear，合成在两者均 linear 时做。④ 圆角+bg-image 共存安全——rounded_rect 镂空区无 fragment，合成不需特判。⑤ pkg.bin 零改（program 是 frame blob 字段，非 scene/ResolvedStyle 字段）。
**教训**：shader 乘法 tint（tex×vcol）≠ CSS 合成——透明区是 0×vcol=0（透明）非透 vcol。色图共存须加法合成（tex.rgb×tex.a + vcol.rgb×(1-tex.a)）。spec 写"shader 已支持共存"前先算 shader 数学，别假设。img vs Container+bg-image 共用纹理时 shader 须靠 program 分流。**坑 79 延伸教训**：Container+bg-image **无 bg-color** 时 vcol=透明，合成 `col.a=vcol.a=0` → 整块透明（图看不见）；但这是既有行为（program:0 时 tex×透明 也透明），非本坑引入。§6.2 承诺只对"有 bg-color"成立。

### 坑 80：showcase 缺 `background-repeat:no-repeat` → 浏览器 contain 行平铺，与 Unity 单图不一致
**症状**：§1.6 `.bg-demo` 在浏览器直接打开，`contain` 行（983×64 容器装 64×64 home.png）平铺出 ~15 个图标，非预期单图居中。
**根因**：CSS 默认 `background-repeat:repeat`，`.bg-demo`/`.br-demo` 未显式 `no-repeat`。LoomGUI **不支持 `background-repeat` 属性**（解析器忽略未知属性，render 只画单图），故 Unity 端 contain 本就单图——浏览器（平铺）与 Unity（单图）渲染不一致，破坏"浏览器=ground truth"假设。
**解决**：`style.css` `.bg-demo`/`.br-demo` 加 `background-repeat:no-repeat`（`5f01bbb` 后续 fix）。LoomGUI 忽略此属性对 Unity 无影响（无害冗余），但让浏览器对齐 Unity + AI 心智模型（contain=单图居中）。
**教训**：showcase 是引擎无关 DSL 范例源，**浏览器渲染须与 Unity 语义一致**——LomGUI 不支持的 CSS 属性若浏览器默认值会改变渲染（如 repeat），showcase 必须显式声明对齐值。AI 可预测性：AI 看 showcase 学 contain 应配 no-repeat，三端（浏览器/Unity/AI 心智）一致。**别假设浏览器=ground truth 自动对齐 Unity**——LoomGUI 围栏外的属性浏览器按 CSS 默认走，可能发散。

### 坑 81：showcase bg-demo 容器 983×64 极端扁宽 → cover/100% 把 64×64 图拉成认不出横带，"图看不出"
**症状**：§1.6 浏览器打开，cover/100% 行"看不到图"——图在画但被横向拉到 983 宽 + 纵向裁切，线条 icon 退化成认不出的横带；contain 图又只占 6.5% 宽太小。
**根因**：`.bg-demo` 早期随手定 `width:100%;height:64px`（983×64，~15:1 极端扁宽），对 64×64 正方形图，cover/100% 必然横向拉满+纵向裁切，size 三模式差异全退化。
**解决**：`.bg-demo` 改 `160×120`（4:3，接近图比例），三模式差异明显且图标看得清（cover 满铺略裁/contain 居中露底/100% 横拉）。**pkg.bin 须重打**（容器尺寸是 scene 布局字段，非围栏外属性）。
**教训**：showcase 演示 size/flex/布局的卡片，**容器宽高比须让被演示特性差异可见**——别随手定 100%×固定小高度。改 showcase 布局类 CSS（width/height/flex）必重打 pkg.bin（scene 变）；改围栏外属性（如 no-repeat）不重打（LoomGUI 忽略，scene 不变，坑 80）。

### 坑 82：增量合并测试只查结构不查内容 → 正则 bug 静默漏过（editor init.mjs，final review C1）
**症状**：`init.mjs` `mergeRuleFile` 重复跑 init 更新规则时，action 显示 "updated" 但旧规则静默保留、新规则没写入。Task 4 测试却报通过。
**根因**：正则 `new RegExp(\`${BEGIN}[\\s\\S]*?${END}\`, "g")` 在模板字符串里 `[\\s\\S]` 坍缩成字面 `[sS]`（只匹配 s/S），永不匹配真实内容 → `existing.replace(re,...)` 返回未改的 existing。Task 4 测试只断言"标签计数=1"+"用户内容保留"，没断言"新内容真替换旧内容"，假阴性。
**解决**：正则改 `[^]*?`（匹配任意字符含换行）。补 `init.test.mjs`（node:test）三态测试，updated 断言"旧内容不残留"。
**教训**：增量合并/替换类测试**必须断言"旧内容被替换"**，不只查结构（标签计数/文件存在）。正则在模板字符串里转义要小心——`[\\s\\S]` 会塌成 `[sS]`，用 `[^]` 或 4 反斜杠。

### 坑 83：fence.md 改了权威副本忘同步分发副本 → 设计师拿到过时围栏（editor I1）
**症状**：Task 7 把 `docs/design/fence.md` 14 处【推断·待测】转【实证】，但 `editor/skill/loomgui-editor/references/fence.md`（Task 2 拷的副本）没同步，注入给设计师的是过时副本。
**根因**：fence.md 有多个副本（docs 权威 + editor references 分发），改权威时没同步分发副本。fence.md §5 本就警告此漂移风险，但无强制机制。
**解决**：重新 `cp docs/design/fence.md editor/skill/loomgui-editor/references/fence.md`，`diff -q` 验 byte-identical。
**教训**：围栏清单是多消费者（docs/editor references/CLAUDE.md.tmpl），改权威源后**逐个同步分发副本**。可加 pre-commit hook `diff -q` 挡漂移。单一真相源是测试（fence_contract.rs），文档副本靠流程同步。

### 坑 84：ColorFilter program=3 单 keyword 丢 BG_COMPOSITE（v1.3 final review I1）
**症状**：Container 同时设 `background-image` + `filter`（disabled 皮肤按钮核心用例）→ 图透明区不显 bg-color（CSS 合成丢失），shader 走 `tex×vcol` 而非 BG_COMPOSITE。
**根因**：render/mod.rs 有 filter 时一律 `program=3`（含 Container+bg-image+filter）；MaterialManager `if(program==3) EnableKeyword("COLOR_FILTER")` 没同时开 BG_COMPOSITE → shader 落 `#else tex×vcol`。spec §1.2 要求"bg-image+filter → 双 keyword"但 program=3 单值 encode 不下 base 是 0 还是 2。per-task review 漏（各自只验自己 task），final review opus 跨 task 追 ColorFilter 全链才抓。
**解决**：拆 program=3（filter+tex*vcol base，Image+filter / Container+filter-无bg）vs program=4（filter+BG_COMPOSITE base，Container+bg-image+filter）。MaterialManager `program==4` 双 keyword。program 列 u8 装得下 0-4，blob schema 零改。dirty hash 已捕 program（Task 8），3↔4 自动覆盖。
**教训**：一个 program 号 encode 不下"base 程序 + 叠加 flag"两层语义时，拆号（别试图单值复用）。shader keyword 组合（COLOR_FILTER × BG_COMPOSITE）的合流点是 final review 级跨任务审查项——per-task reviewer 只验自己 task 的 keyword，看不到 Rust program 语义 ↔ C# keyword 接力的裂缝。

### 坑 85：filter 多函数 concat 顺序反（v1.3 final review I2）
**症状**：`filter: hue-rotate(90deg) saturate(0)` 渲染色相错（CSS 要先 hue-rotate 后 saturate，LoomGUI 反了）。
**根因**：`parse_filter` 累加 `acc = concat(&acc, &m)` = `acc × m`（新 preset 右乘）。但 CSS/fgui `ConcatValues` 是 `_matrix = newPreset × _matrix`（新 preset **左乘**）。对不可交换链（hue-rotate × saturate ≠ saturate × hue-rotate）顺序反。showcase 只测 `grayscale brightness`（近似可交换）没暴露。
**解决**：改 `acc = concat(&m, &acc)`（新 preset 左乘，匹配 fgui）。加顺序敏感测试（`concat(&hue,&sat)` correct vs `concat(&sat,&hue)` reversed 断言不等）。
**教训**：矩阵相乘顺序移植 fgui 时核对 `ConcatValues` 是 `new × old` 还是 `old × new`——fgui `newPreset × _matrix` 是左乘。可交换函数对（grayscale×brightness）掩盖顺序 bug，测试须用不可交换对（hue-rotate×saturate）。

### 坑 86：nine_slice_rounded 角扇形覆盖间隙 + 圆角靠纹理 alpha 伪造（v1.3 final review Critical）
**症状**：slice+radius 共存时每角 `(slice²-r²)` 区域（测试用例 12% 面积）未覆盖（可见洞）；圆角切口靠源图透明像素伪造非几何。
**根因**：初版角扇形覆盖 `[0,r]²`，strip 从 `slice` 起 → `[r,slice]` 区无人覆盖；外角顶点 + 边缘三角形画弧外区靠纹理 alpha 隐藏（违反 rounded_rect 几何圆角约定——源角不透明时方角）。implementer 误判为"亚像素缝 GPU 覆盖"。3 测试只验顶点数/UV 区/fallback，无覆盖率测试。
**解决**：纯弧扇形（去外角顶点 + 边三角形，几何圆角弧外不绘制）+ L 形 quads 覆盖 `[r,slice]` 间隙 + strip 跨 `[slice,w-slice]`。加覆盖率测试（point-in-triangle 扫描：center/edge/corner-L 须覆盖，arc-cutout 点 (2,2) 弧外须**不**覆盖）。reviewer 全网格 5 配置 1px 步长扫描验 holes=0/overlaps=0。
**教训**：自设计 mesh（fgui 无参考）的覆盖率不能只验顶点数/UV——须 point-in-tri 扫描验"该覆盖的覆盖、该镂空的镂空"。implementer 的"亚像素缝"自判常误（实际是大空间间隙）；几何圆角必须弧外不绘制（mirror rounded_rect），靠纹理 alpha 伪造是内容依赖的脆弱正确。

### 坑 87：spec self-review 漏——矩阵走 MPB 但 frame blob 是 Rust→Unity 唯一通道（v1.3 设计期矛盾）
**症状**：v1.3 spec §1.3 写"矩阵走 MPB"、§5 写"blob 零改"，但 MPB 是 Unity 侧，矩阵值在 Rust 侧——Unity MPB 无值可 Set。
**根因**：spec 写"矩阵走 MPB"时只想 Unity 侧 per-renderer 覆盖（不拆 Material，对），漏了"矩阵值怎么从 Rust 传到 Unity MPB"——frame blob 是 Rust→Unity 唯一运行时数据通道，矩阵不进 blob 则 Unity 拿不到。self-review 漏（只查 placeholder/矛盾/scope，没查数据流完整性）。
**解决**：矩阵进 frame blob SOA `color_matrix` 列（[f32;20]，VERSION 5→6）→ Unity `blob.ColorMatrix(i)` 读 → MPB SetVector。两个 version 独立：blob v6（frame 层）+ pkg v10（scene 层 color_filter/border_image_slice 字段）。
**教训**：spec 数据流设计要追"值从产生到消费的完整路径"——跨 Rust↔C# 边界的值必经 frame blob（唯一通道），不能只写"走 MPB"（MPB 是消费侧机制，不是传输通道）。self-review 加"数据流完整性"检查项：每个跨语言值标 Rust 产 → blob 列 → C# 读 → 消费。

### 坑 88：MirrorPool ColorFilter MPB 只 program==3 漏 4（v1.3 家里机验收）
**症状**：§1.8 cf-demo 全青色，filter 不变色（仅隐见房子轮廓）。
**根因**：MirrorPool.cs:140 `if (Program==3)` 设 ColorFilter MPB，cf-demo（bg-image+filter）走 program=4 漏设 → 矩阵 identity。坑 84 修了 MaterialManager keyword（设计期），漏了 MirrorPool 运行时 MPB（消费侧）。
**解决**：`==3 || ==4`。
**教训**：program 分流 bug 查全链（MaterialManager keyword + MirrorPool MPB + shader 分支），设计期修一处运行时漏另一处——静态测试只验 Rust 侧 program 值，C# 消费侧 MPB 设置要 PlayMode 验。**program 号语义变更（加 4）= 全消费点同步事件**：所有按 program 分流的点（MaterialManager EnableKeyword / MirrorPool MPB 触发 / merge·batch is_mergeable / shader keyword 组合）都要同步，final review 追数据流时要逐个消费点核对，不能只改设计期那一处（坑 84 final review I1 漏 MirrorPool 即此）。

### 坑 89：BG_COMPOSITE col.a=vcol.a 无 bg-color 全透明丢图（坑 79 方案 A 修正，v1.3 验收）
**症状**：§1.6 第4行 eye.png 无底色、§1.7 图+圆角、§1.9 图全消失。
**根因**：坑 79 方案 A 合成 `col.a=vcol.a`，无 bg-color 时 vcol=[0,0,0,0] → col.a=0 全透明丢图（方案 A 只对有 bg-color 成立）。
**解决**：标准 source-over `col.a=tex.a+vcol.a·(1-tex.a)`，rgb 直通 `预乘/max(bgA,1e-6)`。有 bg-color 不透明（vcol.a=1）零回归；无 bg-color 等价 program:0 图直通。
**教训**：CSS background 是 source-over（alpha 合成 `a=tex.a+dest.a·(1-tex.a)` 非 `dest.a`）；SrcAlpha blend 下 col.rgb 须直通（max 防 0 除）。

### 坑 90：brightness 照搬 fgui 加法 vs CSS 乘法（v1.3 验收）
**症状**：§1.8 brightness(1.5) 发白非提亮，和 html 对不上。
**根因**：color_filter.rs brightness 照搬 fgui AdjustBrightness（加法 offset=n-1），CSS brightness 是乘法 rgb×n。
**解决**：brightness(n) 对角 n，offset 0。
**教训**：fgui ColorFilter 是 fgui API 语义非 CSS filter，照搬逐函数对照 CSS spec；contrast 公式恰好一致 `(rgb-0.5)·n+0.5`，brightness/saturate/hue 系数/定义不同。

### 坑 91：filter 矩阵在 linear 应用 vs CSS sRGB（v1.3 验收）
**症状**：§1.8 saturate/hue/contrast 色相和 html 对不上。
**根因**：shader 在 linear 应用 filter 矩阵（vcol 先 sRGB→linear），CSS filter 定义在 sRGB；矩阵 offset（contrast -0.25 = sRGB 中点 0.5 偏移）在 linear 错位。
**解决**：shader COLOR_FILTER 加 linear→sRGB→矩阵→sRGB→linear（max(.,0) 防 pow 负底 NaN）。
**教训**：CSS filter 是 sRGB 运算（spec），Linear 项目应用前转 sRGB 后转回；grayscale/invert 线性运算无差，contrast/saturate/hue 非线性感知差异明显。

### 坑 92：nine_slice_rounded UV 全局线性超 atlas 子区（坑 86 延伸，v1.3 验收）
**症状**：§1.9 slice+radius 失真（采到相邻图）。
**根因**：角区 UV 用全局线性 `umin+(px-rect.x)·sxf`（假设 rect.w=src_w），rect.w>src_w（图拉伸放大）时右角区 UV 超 umax 采相邻图。nine_slice（无圆角）用 tex_x 分段钉 umax 不超，nine_slice_rounded 漏。
**解决**：UV 按 slice 分段（u_of/v_of 闭包：左角区 1:1 从左、中拉伸、右角区 1:1 从右钉 umax；v 同理）。
**教训**：九宫格 UV 必按 slice 分段（角区 1:1、边/中拉伸），禁全局线性 rect 像素→源像素（rect≠src 尺寸时超界）；nine_slice 的 tex_x 分段是参考，nine_slice_rounded 要照搬。

### 坑 93：打包器 uv v 用 PNG y-down vs Unity y-up（v flip 潜伏 bug，v1.3 验收）★
**症状**：§1.2/1.3 img + §1.6 bg-image 图上偏（design 顶采到图底）。
**根因**：asset/mod.rs build_registry `uv_min=[x/aw, y/ah]`（PNG y-down 坐标，v=0=图顶），但 Unity texture v=0=底，反向。mod.rs design 顶→uv_max[1] 本意 texture 顶，但 uv_max[1]=PNG 底→Unity v 中部。旧 atlas（icon 占满高 v[0,1]）巧合不暴露；v1.3 加 108px skin.png 打破 atlas 高度，icon 不再占满高，bug 显形。texture.rs:11 注释"y-down 约定"是错误根源。
**解决**：打包器 uv v flip（Unity 约定 v=0 底）：`uv_min[1]=(ah-(y+h))/ah, uv_max[1]=(ah-y)/ah`。mod.rs 交换 v 保持。**已实机验过 img/bg-image 方向正向**（v1.3 家里机验收）。
**教训**：UV v 约定须统一（打包器存 Unity UV v=0 底，非 PNG y-down）；潜伏 bug 在"占满高/宽"对称 case 巧合不暴露，atlas 多尺寸图（布局变）是 v flip 验金石——单尺寸 atlas 永远测不出。**Unity texture flip 是 v 约定的另一半真相**：Unity texture 默认 **不勾 Flip Vertically**（上传像素 y=0 行 ↔ v=0，OpenGL 约定 v=0=底），故 PNG y=0 顶必须打包器翻 v。若工程改勾 Flip Vertically，v 约定反转，打包器 flip 要同步去掉——atlas texture 的 import 设置是 v flip 的配对不变量，改任一侧都要同步另一侧。家里机下轮验收仍重点验 img/bg-image 上下方向（atlas 布局变即回归验）。

### 坑 94：l-container 假自定义元素 → Chromium 预览塌 + AI 困惑（editor 工作流实测）
**症状**：editor 工作流实测 backpack.html 在 open-design 预览整页塌成一坨（子项不排布、宽高塌）。
**根因**：`l-container` 与 div 100% 同映射（`"div" | "l-container" => NodeKind::Container`），是冗余假自定义元素。Chromium 不认 `l-container`（默认 `display:inline`）→ 预览塌；AI 训练没见过 l-container，见白名单有它反困惑"该用 div 还是 l-container"。
**解决**：砍 l-container 出 FENCE_TAGS + node.rs 映射 + dynamic.rs `kind_from_tag`（动态树重构提取的，rebase 时漏改需补）+ 所有围栏/设计文档。白名单只留 div/span/img/button（全 HTML 标准）。
**教训**：`l-` 前缀留给真·自定义元素（l-list/l-rich 等 v1.x 独特语义），**别造与标准标签同映射的假自定义元素**——预览塌 + AI 困惑双重负收益。rebase 解冲突时，被重构移走的逻辑（如 tag→NodeKind 提到 kind_from_tag）里的清理容易漏，需全仓库 grep 验证零残留。

### 坑 95：polyfill 靠设计师手抄 → 漏抄预览塌（editor 工作流实测）
**症状**：v1-showcase 的 head polyfill（`div{display:flex;flex-direction:column}` + `*{box-sizing:border-box}`）要设计师每次手抄到新文件 head，backpack.html 没抄就预览塌。
**根因**：LoomGUI 契约 div 永远 flex column（main-design §1.1），Chromium 默认 div=block/content-box/body 8px margin → 不挂 polyfill 预览骗 AI（gap/flex-grow/align-items 全不生效）。polyfill 靠手抄易漏。
**解决**：严 polyfill 固化进 skill——`editor/skill/loomgui-editor/references/preview-polyfill.html` 标准片段（三行：div flex column + box-sizing + body margin:0）+ SKILL.md 工作流强制 head 内联。polyfill 在 head `<style>`（预览用），设计师样式放外部 css（打包用），pack.mjs 吃外部 css、parse_html 忽略 head（dom.rs:34），polyfill 不进 pkg。
**教训**：预览对齐 polyfill 不能靠手抄，要固化进 skill 强制。polyfill 只补"能补的"（display/box-sizing 默认值差异），补不了的（margin 折叠/文本换行算法级差异）靠 preview-trust.md 不可信清单教 AI。**严 polyfill（强制 div flex column）不让 AI 先验失效**——AI 画面想象来自它写的 CSS 规则，polyfill 让预览如实呈现这些规则；唯一失效先验"div 默认 block flow"本就该失效（围栏禁 block flow）。

### 坑 96：slotmap new_key_type! KeyData 64bit 无法装 u32 FFI 句柄（v1.3+ T2）
**症状**：spec/plan 假设 `new_key_type! { pub struct NodeId(pub u32); }` 让 slotmap 直接用 NodeId 当 Key——实际不编译（宏语法是 `pub struct NodeId;` 无字段）+ 生成的 Key 内部 KeyData 64bit 无法装 u32。
**根因**：slotmap 1.1 KeyData = `{idx:u32, version:NonZeroU32}` 64bit 私有，FFI/C#/FrameBlob/pkg.bin 硬约定 `node_id:u32` + sentinel `0xFFFF_FFFF`，两者互斥。SecondaryMap<NodeId,T> 同理（Key 是 unsafe trait + 位宽不匹配）。
**解决**：`SlotMap<DefaultKey, Node>` + 保留 `NodeId(pub u32)` 经 `from_key/to_key` 桥接（20bit idx + 12bit version）。anim/scroll 改 `HashMap<NodeId,T>`（NodeId 已 Hash+Eq）。
**教训**：plan 草稿假设 slotmap Key 编码 = spec 自定义位宽，实际 slotmap KeyData 固定 64bit。写 plan 前读 crate 源码确认 Key 实际布局，勿按 spec 假设。代际句柄 + FFI u32 硬约定冲突时，桥接层（应用 u32 句柄 ↔ slotmap Key）是干净解。

### 坑 97：AnimTable 读写不对称 — tween 写 .index() 但 get 仍 .0 as usize（v1.3+ T2 review Critical）
**症状**：T2 改 tween 写入侧用 `id.index()`（适配代际 NodeId），但漏改 AnimTable 读取侧（`get/clear_node/clear_prop` 仍 `node.0 as usize`），打包后 `node.0`=4097 越界 → anim override 在渲染层全丢失（alpha/颜色回退 CSS）。
**根因**：T2 把"AnimTable 整体"当 T3 工作，漏了"被 T2 生产路径调用的读取方法必须随写入侧一起改"。测试盲区：现有测试用 `ensure(rid.0 as usize+1)` 撑表写入（走错误 `.0` 语义）→ `get(4097)` 命中测试绿；生产用 `.index()` 正确语义与 get 不匹配。
**解决**：AnimTable 三方法 `node.0 as usize` → `node.index()`（+3 test 适配 + 端到端回归测试 tween→anim→render alpha==0.5 堵盲区）。
**教训**：改写入侧语义时，所有读取侧（含被同 task 生产路径调用的）必须同步改——读写对称是 invariant。测试要断言端到端输出（render 读到的值）而非中间表内值，否则读写不对称的 bug 测试盲区。

### 坑 98：并行数组按 nodes.len() 分配 → remove 后 slotmap 间隙越界 panic（v1.3+ T5 根因）
**症状**：T5 加 remove_node 后，删中间节点 → 高 idx live 节点访问 `world_transforms[id.index()]`/`text_layouts[id.index()]` 越界 panic（slotmap 删留洞，idx 不变但存活数减，len < max idx）。
**根因**：T2 slotmap 接入时并行数组按 `nodes.len()`（存活数）分配，未考虑 remove 后 idx 间隙。T2 无删除故不爆，T5 引入 remove 才暴露。
**解决**：并行数组改按 `nodes.capacity()+1` 分配（capacity = 总槽位数 ≥ max idx，+1 因 idx 从 1 起）。4 分配点统一（world_transforms/taffy_ids/text_layouts）。batch::assign_sort_keys 的 `id.index()-1` 假设连续也改 `id_to_pos` HashMap。
**教训**：slotmap 删除留洞 → idx 不连续，并行数组不能按 len 分配要按 capacity。引入删除操作时 audit 所有按 idx 索引的并行数组。reviewer 建议：新 `id.index()` 索引点沿用 capacity+1 或 id_to_pos 模式。

### 坑 99：remove_node 漏清 scene.focused_node → 悬空 NodeId 致 FOCUS_OUT 带 stale id（v1.3+ final review）
**症状**：remove_node 联动清了 anim/scroll/tween，但漏清 `scene.focused_node`。删焦点节点后 focused_node 悬空，process_keys 发 FOCUS_OUT 事件带 stale node_id（C# listener 查不到节点）。
**根因**：spec §5.3 不变量"所有持久附属同步清"——focused_node 是持久状态（单一全局焦点），remove_node 应清但漏列。grip-dragging 的 `expect("live node")` 同类问题（拖滚动条时 remove 容器 panic）。
**解决**：remove_node 加 `if scene.focused_node == Some(id) { scene.focused_node = None; }`。grip-dragging expect 改安全 match（None 清 scrolling_pane + grip_dragging + continue）。
**教训**：删节点联动清要覆盖**所有**持有 NodeId 的持久状态——anim/scroll/tween/focused_node/input(down_targets/last_hovered_chain/touch_monitors)。新增持久状态时同步加 remove_node 联动。input 持有的 NodeId 用 OOB guard + 祖先回退兜底（v1 已有），但 focused_node 这种单一全局要主动清。

### 坑 100：commit 了没重编的旧 .dll — 本地缓存掩盖，干净环境才暴露（v1.4-a T7）
**症状**：v1.4-a push 后家里机 `git pull` + Unity 编译报 CS0117 `loomgui_stage_instantiate` 不存在 + CS1501 `load_package` 参数数不对。公司机本地一切正常。
**根因**：T7 改 FFI 源码（load_package 加 name + 新增 instantiate），但 commit 进 git 的是**没重编的旧 dll**（`findstr` 查 dll 无 instantiate 导出）。公司机本地缓存的旧 dll + gitignored 的 bindings.cs 把问题盖住；家里机干净 pull 才暴露。csbindgen 的 `LoomGUIBindings.cs` gitignored（各机各 build 不跨机同步），bindings 由 build.rs 重新生成，但 dll 必须手动重编 + commit。
**解决**：家里机 `cargo build -p loomgui_ffi_c --release` 重 build + cp dll（build.rs 自动重生成 bindings 落 Unity 目录）。
**教训**：改 FFI 后 push 前必须 `cargo build -p loomgui_ffi_c --release` + cp dll + **实测 dll 导出**（`nm`/`findstr` 查新符号在），本地缓存的旧 dll 会骗过公司机。stale .dll 是坑 10 的老问题在跨机工作流下的新变体——公司机"能跑"不代表 dll 是新的。

### 坑 101：C# using alias 解不了父命名空间同名类型遮蔽（v1.4-a T9）
**症状**：`namespace LoomGUI.Editor` 里用 `EventType.DragPerform` 报 CS0117——解析到父 `LoomGUI.EventType`（runtime 事件枚举），遮蔽 `UnityEngine.EventType`。加 `using EventType = UnityEngine.EventType;` 无效。
**根因**：C# 名称查找规则——当前 + 父命名空间的类型优先于 using alias。`LoomGUI.EventType` 在父命名空间，先命中，alias 排后面被遮蔽。
**解决**：fully-qualify `UnityEngine.EventType.DragPerform/Updated`（不能靠 alias）。
**教训**：项目自定义类型与 Unity 类型同名（EventType/Object 等）时，子命名空间引用 Unity 版必须 fully-qualify。using alias 对"父命名空间已有同名类型"无效。

### 坑 102：cdylib FFI 入口 panic 拖垮宿主进程（v1.4-a 家里机验收 A3）
**症状**：Unity 打开场景闪退。Editor.log：`panicked at stage.rs:421 'load first'` → `loomgui_stage_tick` → `non-unwinding panic. aborting` → Unity 进程崩。
**根因**：LoomStage `[ExecuteAlways]` → EditMode 也跑 LateUpdate tick。v1.4-a 砍了 Awake 自动建 scene（改 driver.CreateRoot 建），但 LateUpdate 只 guard `_stage==null`（handle 在），没 guard scene 是否建成 → EditMode driver.Start 没跑、scene=None 时 tick → Rust `tick_and_render` 的 `.expect("load first")` panic → cdylib panic abort 拖垮 Unity。
**解决**：LoomStage 加 `_sceneBuilt` 字段，CreateRoot 成功置 true，LateUpdate 顶 `if (_stage==null || !_sceneBuilt) return`。
**教训**：FFI 入口（跨 cdylib 边界）**绝不能 panic**——`.expect`/`unwrap` 遇 None 会 abort 拖垮宿主。scene=None 等状态要优雅早返空帧。`tick_and_render` 的 `.expect("load first")` 是隐患，应改防御性早返（C# guard 是治标，Rust 侧也该加防御）。改加载生命周期（如砍 Awake 自动建 scene）时 audit 所有 tick 路径的 scene 前置假设。

### 坑 103：tick 时序 rematch 在 compute_world_transforms 之后 → :active transform 当帧丢（v1 遗留，v1.4-a showcase 放大）★已修
**症状**：nav 按钮 `:active{transform:scale(0.96)}` 缩放「时好时坏」，快击偶有反馈。`:hover{background-color}` 变色能刷但 `:active` scale 常丢。
**根因**：旧 `tick_and_render` 顺序 = solve → process → scroll → compute_world_transforms → rematch_pseudo_classes → build。rematch 把 `:active` transform 写进 `node.style.transform`，但 compute 本帧已用旧 transform 算完 world_matrix → build 读到无 scale world → 误判 Unchanged → scale 本帧不可见。v1d.3/v1d.5 定的时序，v1 遗留 bug；v1.4-a showcase 大量 nav 按钮伪类才暴露。
**解决**（2026-07-03 渲染管线重构 T1）：tick 重排为 `process → rematch → solve → refresh → compute → build`——rematch 提到 solve/compute 前，伪类改 taffy_style/transform/colors 三类当帧全生效。删 `rematch_pseudo_classes` 的 `any_layout_dirty` 返回值（solve 每帧全量，无需 dirty 驱动）。
**教训**：伪类 rematch 改的 style 跨三层——transform（compute 读）/ taffy_style（solve 读）/ colors（build 读）。tick 时序须保证 rematch 在三类消费者之前。「改属性只置 dirty + solve 在最前 + 丢 layout_dirty」是反模式——伪类改布局属性根本不生效。是「所有布局帧末一致」不变量与「伪类每帧 rematch」的时序冲突，已系统重构。

### 坑 104：SpriteResolver 缓存 miss 永久化 → atlas 后来 pack 好也不重查（v1.4-a T8）
**症状**：图片白块。诊断主因是 atlas 空包（folder packable + m_PackedSprites 全空，Unity 6 folder packable 失效），但即使 pack 好，运行时可能仍白。
**根因**：`SpriteResolver.GetSprite` 缓存 miss（`_cache[path] = found ?? _missingSprite`，line 92-93）——首次查 miss（atlas 未 pack/时机晚）→ null 被永久缓存 → 后续即使 atlas pack 好也不重查（除非 ClearCache）。与 atlas 空包正交，诊断没提。
**解决**（待定）：miss 不缓存，或 atlas pack/注册变化时 ClearCache。atlas 空包本身是 Unity asset 设置问题（packables 改逐 Sprite + Pack Preview + Include in Build），家里机改 asset 可验主因。
**教训**：查询缓存别缓存 miss（除非确定源不变）——运行时资源（atlas pack 时机、异步加载）可能后到，缓存 miss 会永久遮蔽。B2 双因：atlas 空包（即时病因，asset 设置）+ 缓存 miss 永久化（潜伏，代码）。

### v1.4-a 验收诊断方法论（外部 AI 诊断的 3 处误判，本会话 git 历史核实纠正）
v1.4-a 家里机验收 4 bug，外部 AI 出了诊断报告，本会话用「这次 SDD 的完整改动上下文 + git -S/log 追溯」核实，纠正 3 处：
1. **B1 :active 时序**：诊断归 v1.4-a 改动域 → 实为 v1d.3/v1d.5 **v1 遗留**（`git log -S compute_world_transforms` 证），showcase 多按钮才暴露。且诊断漏了 rematch layout_dirty 丢弃的更深问题（坑 103）。
2. **B4 文字乱**：诊断说「既有 bug 放大」→ **对**（`git log 7a01d77..HEAD -- TextRasterizer.cs text/layout.rs` 为空，v1.4-a 没碰 text 路径）。
3. **B2 图白块**：诊断说 atlas 空包 → 即时病因对，但漏了 SpriteResolver 缓存 miss 永久化（坑 104）。
**教训**：bug 归因（哪个改动引入）用 `git log -S/-G <关键代码>` + `git log <base>..HEAD -- <文件>` 直接证，别靠"最近改过这块"推论。有当次改动上下文的人比只读最终代码的诊断者更能准确归因——「这次引入 vs 既有放大」区分决定修复策略（既有 bug 放大 = 补既有短板，非回滚本次改动）。

### 坑 105：dirty hash 采样漏字段——同一行文本首字+字数同则撞 hash（坑 56/75/76 一类，已根治）
**症状**：文字内容变（`"hello"`→`"helps"`）但屏幕不更新；mesh 某字段变但 Unchanged。
**根因**：旧 `node_hash` 为省 CPU 只采样部分字段——文字只 hash 首字 codepoint + glyph_count + 首字 x/y；mesh 只 hash verts[0]/[2] + uvs[0]/[2]。每加一个视觉字段（program/color_matrix/UV/圆角顶点）就漏一次，补一个采样点 + 一个回归测试，补丁累积。
**解决**（2026-07-03 T2）：`payload_hash` 全量——所有 verts/uvs/colors/indices、所有 glyph codepoint+x+y 一律 `to_le_bytes().hash()`，不挑代表。删采样逻辑 + 一半"补漏字段"回归测试。
**教训**：hash 采样是补丁累积区——「省几个字段的 hash 开销」换来「每加字段必漏一次」的反复 bug。几何 hash 要么全量要么别 hash。CPU 开销（几万次哈希 ≈ 几十微秒）远小于跨 FFI 传输/Unity 重建，不值得为它牺牲正确性。

**v1.4-b review 补漏**（2026-07-03）：T2"全量根治"仍漏两处——①`header_hash` 漏 `reuse_key`（身份字段，同 NodeId 换 reuse_key 不触发 Header）②`payload_hash` Text arm 漏 `line.y/height/baseline/width`（line-height 变只改 height，glyph pen 不动则撞 hash）。补字段 + 3 回归测试（header_hash_includes_reuse_key / payload_hash_text_includes_line_height / line_baseline_y）。教训延续：每加一个 RenderNode/Line 字段必检 dirty hash 是否覆盖，"全量"是动态契约不是一次性声明。


### 坑 106：payload_hash 位置-bake——pure-translation 顶点烤 world.tx，位置变误落 Full（双 re-base 中间漏）
**症状**：滑动列表/transform 动画时，节点位置变（只该走 Header 廉价路径）却误判 Full，每帧重建 mesh。
**根因**：纯平移节点 `build_render_nodes`（mod.rs:118）把 `Rect{x:wm[4],y:wm[5]...}` 位置烤进 quad verts；`blob.rs:110` 后续 re-base 减 `wm[4]/[5]` 还原成 local——**两个 re-base 互相抵消**，最终 blob arena 字节位置无关。但 `payload_hash` 在 mod.rs 之后、blob 之前算，看到的是**位置烤过的 verts** → 位置变 → payload_hash 变 → Full。spec §3.7「滑动天然 HEADER」的承诺在 hash 这层被打破。
**解决**（2026-07-03 T4 fix）：`payload_hash` Mesh 臂算 verts 前先 re-base 到 local（pure-translation 减 `wm[4]/[5]`，非纯平移 verts 已 box-local 不减），与 blob 的 re-base 同逻辑。位置变 → header_hash（world_matrix）变、payload_hash 不变 → Header。
**教训**：当存在「中间表示被烤入位置、下游再 re-base 还原」的双 re-base 模式时，**任何 hash/dirty 检测必须在 re-base 之后做**（即对最终 local 形态 hash），否则位置变化会穿透检测。hash 要对「真正发给后端的数据形态」算，不是对中间表示算。reviewer 在 T4 初版发现——单测"位置变→Header"是锁这个的关键。

### 坑 107：剥离 uniform 漏改旧烘焙路径——alpha 双乘（text opacity 渲成平方）
**症状**：`opacity:0.5` 的文字渲染成 0.25（平方），mesh 路径正常。
**根因**：T6 把 alpha 从顶点色烘焙剥离到 `_Alpha` shader uniform——blob 不再 `c[3]*rn.alpha`，shader `col.a *= _Alpha`。但 C# `TextRasterizer.BuildMesh` 仍 `tinted.a *= alpha`（旧烘焙路径），且 `MirrorPool` 又设 `_Alpha=nodeAlpha` → text 顶点色烤一次 + uniform 再乘一次 = 双乘。mesh 路径 T6 已改对（UploadMesh 原样拷），**只有 text 路径漏改**（BuildMesh 是 C# 侧独立光栅化，不在 blob 链上）。
**解决**（2026-07-03 T8 fix）：BuildMesh 删 `tinted.a *= alpha`（彻底，Option A），测试传 `alpha=0.5f` 断言 `vertex.a==color.a` 真锁契约（传 1f 是恒真无效测）。
**教训**：剥离一个属性到新通道（uniform/MPB）时，**必须grep所有旧烘焙路径**——不是只改主链（blob），C# 侧独立光栅化（TextRasterizer）、merge_batch 等旁路都有自己的一份烘焙逻辑，漏一个就双乘/漏乘。测试要传非 trivial 值（0.5 不是 1）否则恒真无效。这类 bug 只在 Unity PlayMode 可见（公司机静态审 + cargo test 都测不到 C# 光栅化），静态 review 要把"所有烘焙点"列全逐一核查。

### 坑 108：DrawState key 含 alpha.to_bits() → opacity 动画期间节点退出批合（设计权衡，非 bug）
**症状**：opacity 动画的节点在动画期间 draw call 涨（退出批合），动画结束回 1.0 后重新合批。
**根因**：渲染管线重构 T9 把 alpha 从顶点色烘焙剥离到 `_Alpha` per-renderer uniform（坑 107）。per-renderer uniform 是单值——同一 draw call 内所有顶点共享一个 `_Alpha`，**不同 alpha 的节点物理上不能合一个 draw call**（合了就只能用一个 alpha）。所以 `mesh_key`（merge.rs:12）必含 `alpha.to_bits()` 把 alpha 进 DrawState key，不同 alpha 的节点不合并。
**性质**：正确权衡，非 bug。uniform 单值物理约束决定的——不是"alpha 不该进 key"，是"alpha 进了 uniform 就必进 key"。代价：opacity 动画期间动画节点退出批合，draw call 涨。兑现"opacity 走 Header 不重建 mesh"（alpha 变只改 MPB _Alpha，不重建 mesh），但牺牲合批。
**教训**：剥离属性到 per-renderer uniform 时，该属性自动成为 DrawState key 的一部分（合批边界）——"省 mesh 重建"和"保合批"是 trade-off，per-renderer uniform 选前者。若 opacity 动画节点多到 draw call 成问题，考虑：①alpha 量化（0.1 步进，有限档位合批）；②或 alpha 不进 uniform 改回顶点色烘焙（但牺牲 Header 廉价路径）。当前 v1.x 不动，记性能特征待 Profiler 实测决定。

### 坑 109：虚拟列表 reuse_key 绑 itemIndex → GO 复用完全失效（伪虚拟化）
**症状**：虚拟列表滚动时 GameObject 持续销毁+重建，Profiler 见 GO/Mesh GC 峰值，快速滚动卡顿——reuse_key 机制名存实亡。
**根因**：v1.4-b T8 driver 把 `reuse_key = itemIndex + 1`（绑数据 item 索引）。滚动时 item 进出可见区 → 新进入 item 的 itemIndex 不同于刚离开的 → reuse_key 不同 → MirrorPool `_poolByReuse` 永不命中 → 旧 GO stale 销毁 + 新 GO 新建。等于"每 item 一个独立 GO"，完全没复用。设计意图（a329c26 commit + MirrorPool 注释）是"slot 换绑 item 时 NodeId 变但 reuse_key 不变 → GO 复用"，实现完全相反。
**双重失效**：MirrorPool EditMode 测试用"同 reuse_key、node_id 100→200"理想场景验证复用，但 driver 从不产生这种场景——测试绿但 bug 活着。
**解决**（2026-07-03 P0-2 fix）：`reuse_key` 绑**槽位序号** slotIdx（视口内 0..visibleCount-1），非 itemIndex。`_slots` dict key 改 slotIdx。滚动时槽位稳定（slot 0 永远是视口第一个可见位），只换绑的 itemIndex 变（SetStyle 改 top/height + SetText 改内容），slotIdx 不变 → reuse_key 不变 → MirrorPool 命中 → 只重建 mesh 不销毁 GO。
**教训**：reuse_key 是"渲染复用身份"，要绑**稳定的槽位**而非"数据身份"。fgui GList 范式（GList.cs:1975 `ii.obj = ii2.obj` 搬对象不销毁）就是槽位稳定 + 换绑数据。测试不能只验"同 reuse_key 能复用"（理想场景），要验"driver 实际产生的 reuse_key 序列"——否则测试和实际使用脱节，绿但不防 bug。

### 坑 110：merge modify/delete 冲突按 delete 解决 → 误删 HEAD 扩展的活测试
**症状**：deadcode-cleanup worktree merge 回 main（已含 workflow-atlas-rework v9）时，`MirrorPoolTests.cs` / `AtlasMirrorPoolTests.cs` 报 modify/delete 冲突（worktree 删 / HEAD 改）。按 delete 解决后 489 行 v9 验收测试（`MirrorPoolReuseKeyTests` T6 reuse_key GO 复用 + `AtlasMirrorPoolPathTests` SpriteResolver 冒烟）丢失，T6 核心 C# 侧零测试覆盖。
**根因**：worktree 把这两个文件当 spec §2.1「[Ignore] 退役测试」删了（基于 v8 基线判断）。但 main 已被 workflow-atlas-rework 扩展——`MirrorPoolTests.cs` 加了 `MirrorPoolReuseKeyTests`（v9 22 列活测试）、`AtlasMirrorPoolTests.cs` 改名 `AtlasMirrorPoolPathTests`（活的非退役）。spec 的「退役」判断只在 worktree 基线版本成立，主分支演进后被删文件已被 HEAD 重新填充活内容。按 delete 解决 = 丢了 HEAD 的新内容。
**解决**：从 `08e5582` 恢复 v9 测试类（`MirrorPoolReuseKeyTests` 22 列 colOff=132 与当前 FrameBlob `ExpectedVersion=9`/`ReuseKey=ColOff(21)` 一致），不恢复 v4 [Ignore] 退役类。`AtlasMirrorPoolTests` 恢复 + 清 v1.4-a T8 历史注释 + 改测试名 `MissingSprite`→`NoAtlas`。
**教训**：merge modify/delete 冲突按 delete 解决前**必须核实 HEAD 改了什么**——worktree 删的文件不等于 HEAD 没扩展。`git show <HEAD>:<path>` 看一眼 HEAD 版本内容再决定 delete/modify。死代码清理 spec 的「退役」判断只在基线版本成立，merge 到已演进的 main 时每个被删文件都要重新核实是否被 HEAD 扩展。


### 坑 111：plan 跨 task 改动不同步 — 后续 task 照搬 plan 旧代码与已改 API 不符
**症状**：Spec A 执行中，Task 2 改了打包器 CLI（`--res` → `--res-root <path>`）。Task 5（LoomSettingsWindow 拼 CLI args）照搬 plan 原代码仍写 `--res`，与 Task 2 改后的 CLI 不匹配 → 打包功能 exit(2) 全挂。review 才发现。
**根因**：plan 写时 Task 2 的改动还没发生，Task 5 代码基于旧 CLI。plan 是一次性写的快照，跨 task 的接口改动不会自动传播到后续 task 的代码块。
**解决**：reviewer 跨 task 一致性检查（CLI flag / 字段名 / 签名跨层对齐）捕到，fix 改 `--res-root` + 显式传 res 绝对路径。
**教训**：plan 里后续 task 照搬的代码块，执行时要核实它依赖的接口有没有被前置 task 改过——尤其 CLI flag、函数签名、字段名。SDD 的 task reviewer 要带「跨 task 一致性」lens（plan 自身不会自动同步）。同类：Task 2 res 提走后 `source_dir.join(res_dir)` 读 PNG 断（plan Task 2 低估了，执行中发现要加 res_root 参数）。

### 坑 112：多虚拟列表同屏 reuse_key 撞车 → slot 背景按槽位互相覆盖（灰底闪/缺）
**症状**：page_list 两个虚拟列表（等高 + 不等高）同屏，拖动时 item 灰底（`#252839`）按 slot 闪现/消失，灰底缺失处露出深色轨道 `#1a1d2e` 像"缝隙"，整体一闪一闪。单列表测试无此问题——必须两列表同屏才暴露。
**根因**：两个 `VirtualListDriver` 都用 `reuse_key = slotIdx+1`（都从 1 起）。MirrorPool `_poolByReuse` 是**场景级单字典**（`MirrorPool.cs:82-83`）——两列表同 slotIdx 的 slot 容器抢同一个 GO（DFS 序后者覆盖前者位置 + mesh）。关键：仅 slot 容器背景带 `reuse_key>0`（icon/span 为 0 → 按 node_id 走，唯一不撞），撞车**精确发生在灰底背景** → 被 另一列表同 slotIdx 顶掉的 slot 没了灰底（只剩 icon + 文字浮在深色轨道上）→ 灰底按 slot 闪/缺。
**解决**（2026-07-04）：driver 加 `reuseBase` 偏移，等高用 0 段（1..N，行为不变），不等高挪到 100000 段——每列表独占 reuse_key 段，不相交。纯 C#，无 .dll 重编。
**教训**：reuse_key 是**场景级全局命名空间**（非 per-container），多虚拟列表同屏必须用不相交的 reuse_key 段。与坑 109 同字段不同坑：109 是单列表内绑 itemIndex vs slotIdx；112 是跨列表命名空间撞车。单列表测试发现不了——多列表同屏是复用机制的必修验收场景。

### 坑 113：动态字体 atlas rebuild 后 text mesh 漏重建 → 偶现上下颠倒/碎片
**症状**：文字偶现上下颠倒/只显示一部分，随机但高频易触发（文字多/动态内容尤甚）；静态文字也中招，且错乱定住不恢复。
**根因**：Unity 动态字体 atlas 在 `BuildMesh` 的 `RequestCharactersInTexture` **内部**异步 rebuild（atlas 被撑爆时），所有 glyph UV 变。MirrorPool 三分支只重建 `change_level==Full` 的 text；content 没变的静态 text 是 `Skip`，不进重建分支 → UV 指旧 atlas。且 rebuild 发生在 Sync 遍历**中途**，Sync 末 `_lastFontVersion` 又追上新版本号 → 下帧 `fontDirty=false` → 旧 UV **永久残留**（非闪一帧）。v1.4-a 的 B4 诊断（`plans/2026-07-03-v1.4a-acceptance-bugs-diagnosis.md`）早指出 + 给了方向，但**没沉淀进 pitfalls** → v1.4-b 重新踩、重新调试一轮。
**解决**（2026-07-04）：① `fontDirty` 时把所有 text 节点（含 Skip/Header）提升 Full 重建；② `RenderObj` 缓存上次 Full 的 glyphs，`text_len==0`（Skip/Header，blob 不写 arena）走缓存重建取新 UV——**禁 ReadText**（text_arena 空读越界 → count 垃圾 → OOM）；③ Sync 末检测中途 rebuild（`FontVersion != start`）则**不追** `_lastFontVersion`，下帧再全重建。
**教训**：偶现/时序 bug（依赖 Unity 内部事件 + 帧序）**光读代码定位不了**——必须运行时 log 取证（`OnRebuilt` 调用栈暴露 rebuild 在 `BuildMesh` 内触发是破案关键）。**诊断过但没沉淀 = 必重踩**——B4 要进 pitfalls 不能只留 plans。与坑 107 同为 text 渲染状态机坑，107 是 alpha 双乘、113 是 atlas rebuild 漏刷。

### 坑 114：blob text 序列化 pen_y = line.y + line.baseline 多加行高（单行掩盖至 v1.4-b）
**症状**：多行文字行距翻倍（行重叠/越往下越偏），单行正常。
**根因**：`blob.rs` 序列化 glyph pen_y 用 `line.y + line.baseline`，但 `Line.baseline`（layout.rs:43）已是**绝对 y**（= line_y + baseline_offset），又叠 `line.y`（= line_y）→ 多一个行高。单行 line_y=0 掩盖。源头：v1a plan（`plans/2026-06-19-v1a-unity-render-phase2.md:231`）写错公式，实现照抄 3 个版本至 v1.4-b 才发现。
**解决**：`pen_y = line.baseline`。补多行 blob 测试（原单行测试掩盖了 bug）。
**教训**：plan/spec 的数学公式照抄进实现前核对字段语义（看 struct 注释 + 赋值处）——`baseline` 绝对还是相对要看定义。text 测试要覆盖 ≥2 行（单行掩盖多行 bug）。

### 坑 115：Unity 6 Sprite Atlas 自动建/pack 五连坑（V1 不 pack / V2 创建 / Packer Mode）
**症状**：代码自动建的图集运行时 `atlas.GetSprite` 全 miss（`packed.spriteCount=0`），图片不显示；Editor inspect 看 packables 有 Sprite 但 packed sprites 空。
**根因**（五坑串联，家里机踩五六轮）：
1. `new SpriteAtlas()` + `AssetDatabase.CreateAsset` 存的是 **V1**（`isAtlasV2:0`、`m_Guid` 全零、`m_PackedSprites` 空）——V1 在 Unity 6 不 pack。
2. `SpriteAtlasAsset.Load` 只认 V2（`.spriteatlasv2` 扩展名），对 V1 `.spriteatlas` 返 null。
3. V2 创建正道：`new SpriteAtlasAsset()`（**不是** `new SpriteAtlas`，**不是** `ScriptableObject.CreateInstance`——`SpriteAtlasAsset` 继承 `Object` 不 `ScriptableObject`）+ `SpriteAtlasAsset.Save(saa, "x.spriteatlasv2")`（Venkify 官方法，[discussions.unity.com/t/949154](https://discussions.unity.com/t/how-do-you-create-atlas-v2-using-code/949154)）。
4. V2 pack 须 **Project Settings > Editor > Sprite Packer > Mode = Always Enabled**——Disabled 时 `SaveAndReimport` 也不 pack。"Enabled for Builds" 只 build 时 pack，PlayMode 不 pack（仍 spriteCount=0）。
5. `entry.atlas` 旧 V1 引用用 `if (entry.atlas == null)` 判断不会重绑——要**强制赋值**覆盖旧 V1 引用。
**解决**（`LoomAtlasSync.cs` V2 全链路）：`EnsureAtlasAsset` 用 `new SpriteAtlasAsset()` + `Save(.spriteatlasv2)` 建，`SyncEntry` 用 `SpriteAtlasAsset.Load/Add/Remove/Save` + `importer.SaveAndReimport()` 触发 pack（joe_nk [discussions.unity.com/t/917311](https://discussions.unity.com/t/spriteatlasextensions-are-broken/917311) workaround），`entry.atlas` 强制绑 V2，`ResolveAtlasPath` 只认 `.spriteatlasv2`。Packer Mode 文档说明（非代码可控）。
**教训**：Unity 6 Sprite Atlas **只用 V2**（`.spriteatlasv2` + `SpriteAtlasAsset` API），别碰 V1（`new SpriteAtlas`）。pack 非代码可控——Packer Mode 是前置（Disabled 时全 miss）。Agent 调研（理论"import auto-pack"）要实测兜底。SpriteAtlasExtensions（atlas.Add/Remove）/SerializedObject 在 V2 都 broken，只有 SpriteAtlasAsset 路径对。

### 坑 116：Unity .md/.html 当 DefaultAsset 不是 TextAsset → LoadAssetAtPath<TextAsset> 返 null
**症状**：初始化按钮点了显示「完成」，但 CLAUDE.md / skill 文件实际没写。
**根因**：`AssetDatabase.LoadAssetAtPath<TextAsset>("x.md")` 返 null——Unity 把 .md/.html 导入成 `DefaultAsset` 不是 `TextAsset`，按 `<TextAsset>` 筛类型不匹配返 null，注入流程 `LogError + return`，但按钮回调还无条件显示「完成」掩盖了失败。
**解决**：纯文本资源（fence-rules.md / SKILL.md / *.html 等）用 `File.ReadAllText(projRoot + assetPath)` 直读磁盘，不走 AssetDatabase 类型转换。`Initialize` 返回 `Result{ok,msg}`，按钮据实显示。
**教训**：`LoadAssetAtPath<T>` 按 T 筛——Unity 把该文件导入成非 T 类型则返 null，不报错。给 AI/打包器用的纯文本（不须 Unity 当资产）一律 `File.ReadAllText` 直读最稳。

### 坑 117：Unity 只忽略点开头文件 import——挡不住 CLAUDE.md / .html / .css
**症状**：想挡工作区下非资源文件（CLAUDE.md / *.html / *.css）不被 Unity 导入，找不到 API。
**根因**：Unity 硬规则——只忽略 `.` 开头文件/目录（`.claude` / `.git` 等不生成 .meta）。非点开头文件必 import 成 `DefaultAsset` + `.meta`，**无公开 API 阻止**（`OnPreprocessAsset` 不能 abort；`AssetImporter.SetNonAsset` 是幻觉 API；`.unityignore` 是社区未实现请求）。
**解决**：接受现状——DefaultAsset 不进 build，打包器 `File.ReadAllText` 直读源文件，不影响运行时/AI 工作流。`.claude` 点开头自动忽略。spec §3.2「跳过导入」理想化，Unity API 无解。
**教训**：挡 Unity 导入非点开头文件做不到——要么改名点开头（CLAUDE.md 固定不能改），要么移出 Assets/。工作区设计别依赖「挡导入」。

### 坑 118：EditMode 测试硬编码 NodeId 全挂——generational NodeId 不顺序
**症状**：`LoomEventHandlerTests` 硬编码 nodeId 0/1/2 全挂（事件路由链断）。
**根因**：`NodeId(pub u32)` 是代际的（slotmap `idx<<12 | gen`，首节点 `(1<<12)|1=4097`，非 0）。`load_html` 的 NodeId 顺序依赖解析/插入序，不可预测。测试硬编码 0/1/2 与实际 NodeId 不符。
**解决**：测试用 `loomgui_stage_create_root` + `create_node` + `append_child` 建 tree，拿真实 NodeId（root/parent/child 变量），事件路由测用这些变量非字面量。
**教训**：FFI 句柄（NodeId 代际、reuse_key 等）是不透明/代际的，测试别硬编码字面量——用 create API 拿真实值。`load_html` 适合测样式/结构，不适合测 NodeId 身份。

### 坑 119：CJK 缺字 advance 不匹配 → 字距重叠（Rust .notdef 0.6em vs Unity fallback 1em）
**症状**：用不含 CJK 的字体（DejaVuSans）渲染含 CJK 文本，CJK 字距重叠（pen_x 间隔 0.6em，字叠在一起）；wqy-microhei（有 CJK）正常；Latin 正常。禁 kern 无效（首轮误诊为 kern）。
**根因**：Rust 测量字体缺 CJK 字形 → `glyph_index` 返 `None` → 走 `.notdef`(gid0) advance（DejaVu ≈0.6em）。但 Unity 动态字体缺字 **fallback 到系统 CJK 字体**渲染 1em 方块 quad。Rust pen_x 用 0.6em 间隔 < Unity 1em quad → 重叠 0.4em（PlayMode `[TEXTDIAG]` 实测「控件」@24pt：第二字 penX=14.4、Unity quad 宽 25、重叠 11.6px；Latin 正常因字体有字形）。Rust 单字体无 fallback（layout.rs「进程级单字体」），Unity 动态字体有 OS fallback——两边缺字处理不对称 → advance 不一致。
**解决**（2026-07-04）：`advance` 闭包改接 `Option<GlyphId>`，缺字（`None`）兜底 `font_size`（CJK 方块 1em 假设）匹配 Unity fallback quad 宽度。回归测试 `missing_glyph_advance_falls_back_to_font_size`。wqy 有 CJK 走 `Some` 分支不受影响。
**教训**：① **误诊**：首轮 EditMode DIAG 测 Latin kern 对（AV/Te quad 重叠），但用户看的是 CJK——症状对错了字符类型。多字体/多字符类字距问题**先确认症状落哪类字符**（CJK/Latin/数字），别假设。② Rust/Unity 字体处理不对称（Rust 单字体 vs Unity OS fallback）是缺字场景的系统性不匹配源——单字体架构下 Rust 缺字**必须兜底**，禁走 `.notdef`。③ **加坑前 grep 编号**——本坑 commit 初写「坑 115」撞了 Sprite Atlas 坑，pitfalls 实际到 118，本坑应是 119。

### 坑 120：SpriteAtlas UV remap 两连坑——GetSprite clone 的 .rect 是源 rect + rotation 打包 AABB 溢出
**症状**：PlayMode 图片整张错（所有图显示成同一块）→ 修后边缘渗邻居像素。
**根因**：两个独立坑都在「取 sprite 的 atlas 子区 UV」这步（core 产 [0,1] blob UV + Unity remap 的契约本身没错）：
① `SpriteAtlas.GetSprite(name)` 返回 clone，其 `.rect` 是**源图 rect**（0,0,w,h）非 atlas 打包位置——实测 home/eye/zap 三张 `.rect` 全 =(0,0,64x64)，原 remap `sp.rect / texture 尺寸` 算出同一 atlasUV → 全采样 atlas 同一区 → 整张错图。真实打包位置在 `sp.uv`（atlas 纹理坐标）。
② atlas `enableRotation=1`（V2 默认）→ `sp.uv` min/max 取的是**轴对齐包围盒 AABB**，旋转打包的 AABB 比实际 sprite 大、溢出邻居区 → 边缘渗邻居像素。半纹素内缩（0.5px）对几 px 的 AABB 溢出无效（实测「一样没变」）。`enableTightPacking=1` 雪上加霜（裁透明边，rect 非整数）。
**解决**（2026-07-04）：① `MirrorPool.RemapMeshUvToSprite` 弃 `sp.rect`，改读 `sp.uv` 取 min/max 当子区。② `LoomAtlasSync.EnsureAxisAlignedPacking`：建/同步 atlas 时强制 `enableRotation=false / enableTightPacking=false / padding≥4`（`SpriteAtlasImporter.packingSettings`，已 axis-aligned 则跳过不 reimport）。UI atlas 必须 axis-aligned 矩形排。（注：曾加半纹素内缩防 padding 边缘 fringe，后删——padding=4+无 mipmap 下 fringe 几乎不可见，缩水代价比 fringe 值不来；真需要开 `enableAlphaDilation` 才是正解。）
**教训**：① Unity API 别信草稿/记忆——`Sprite.rect` 在 packed clone 上是源 rect，得 `grep` `Editor/Data/Managed/UnityEditor.xml` 查证（`sp.uv` 才是打包坐标）。② **旋转打包 + AABB 取子区天然不兼容**——任何「取 sprite 子区 UV」的实现要么关 rotation 要么处理 packingRotation。③ 取证式分层 debug：先 `[UVDBG]` 打 path→sprite→rect→tex 确认「错图」是 rect 全同（坑①），再打 mipmap/fmt + 对比 inset 前后渗量确认「边缘渗」既非 mipmap（mip=1）也非压缩而是 rotation AABB（坑②）。

### 坑 121：AssetPostprocessor 在 import 期调 CreateAsset → UnityException（非确定性禁令）
**症状**：点「同步此图集」报 `UnityException: Calls to "AssetDatabase.CreateAsset" are restricted during asset importing`。堆栈：`LoomWorkspaceAssetPostprocessor.OnPreprocessAsset → LoomSettings.GetOrCreateDefault → AssetDatabase.CreateAsset`。
**根因**：`OnPreprocessAsset` 跑在 import 流水线里，import 期禁任何 mutating AssetDatabase API（CreateAsset/SaveAssets/DeleteAsset，防非确定性）。但它调 `GetOrCreateDefault()`，后者在 settings 资产 `Resources.Load` 返 null 时（编译后/批量重导瞬态，资产未加载好）走 `CreateAsset` → 崩。`SyncEntry` 末尾 `SaveAndReimport` 触发 import 激活此链。潜伏 bug，被 EnsureAxisAlignedPacking + 编译刷新逼出。
**解决**（2026-07-04）：加 `LoomSettings.GetDefault()`（只 `Resources.Load` 不建），postprocessor 改用它——settings 没加载好就返 null 跳过，不再 CreateAsset。
**教训**：**AssetPostprocessor / ScriptedImporter 跑在 import 流水线里，禁调 mutating AssetDatabase API**。要读配置走 load-only 路径，找不到就跳过。「找不到就建」的 helper（GetOrCreateDefault）不能在 import 期用。

### 坑 122：csbindgen build.rs 输出路径未跟随插件包移动 → 每次 build 重生 straggler
**症状**：插件代码移入 UPM 包（`loomgui_unity_package/`）后，旧路径 `loomgui_unity/Assets/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs` 每次 `cargo build` 都冒出来；git merge 清了又回。
**根因**：`loomgui_ffi_c/build.rs` 的 `unity_bindings` 路径硬编旧位，每次 build 自动写盘（best-effort），merge 删 straggler 治标不治本。
**解决**：build.rs 路径同步改包内 `../loomgui_unity_package/Plugins/LoomGUI/Bindings/`。
**教训**：生成器（csbindgen/cbindgen/...）的输出路径是"产物归属"的一部分——移动产物位置时**同步改生成器输出路径**，否则每次构建在旧位重生 = 永久 straggler。删 straggler 前先问：是不是有生成器还写到这。

### 坑 123：`git add <name>` 只加同名目录，不加 `<name>.rs` 文件（测试外提漏 commit）
**症状**：把 `input.rs` 内嵌测试外提到 `input/tests.rs`，`git add loomgui_core/src/input` + commit，`cargo test` 全过——但 `input.rs` 本身的"测试已外提"修改**没进 commit**（HEAD 里 input.rs 还是带内嵌测试的旧版 + 孤儿 input/tests.rs）。
**根因**：`git add <path>` 加 `input/` 目录（含 tests.rs）但**不加同级 `input.rs` 文件**（不同 path）。dir/file 同名时极易漏。
**解决**：测试外提 commit 时**显式 `git add <name>.rs <name>/`**（文件 + 目录都列）；commit 后 `git diff --cached --name-status` 验 `M <name>.rs` + `A <name>/tests.rs` 都在。
**教训**：`git add` 路径精确匹配——dir 不含同名 file。重构涉及"文件 + 同名子目录"（Rust `mod X;` 抽 `X.rs` + `X/`）时，add 后必验暂存集。

### 坑 124：Unity UPM 包内代码引用包资源用 `Packages/<name>/...` 非 `Assets/...`
**症状**：插件移入 `com.loomgui.unity` UPM 包后，Editor 脚本 `Path.Combine(projRoot, "Assets/LoomGUI/Editor/Tools/loomgui_pkg.exe")` 找不到 exe（路径空）。
**根因**：Unity 把包内容虚拟化挂载在 `Packages/<package-name>/`，**不是 `Assets/`**。包移动后，引用包内资源的代码路径要从 `Assets/LoomGUI/...` 改 `Packages/com.loomgui.unity/...`。
**解决**：包内 Editor 脚本所有硬编 `Assets/LoomGUI/` 路径改 `Packages/com.loomgui.unity/`（exe + Resources + skill 都如此）；相对路径算法不变（URI MakeRelative）。
**教训**：UPM 包代码访问包内资源 = `Packages/<name>/`（Unity 虚拟挂载），非 `Assets/`。包一移动，grep 包内所有 `Assets/` 硬编路径同步改。本地包 manifest 引用同级包：`"file:../../<pkg-dir>"`（相对 `Packages/` 文件夹）。

### 坑 125：showcase HTML 围栏外写法三连——`<br>` 非白名单 / 行内混排 / `inset` shorthand

**症状**：showcase HTML 改了内容后打包器报错拒收（`parse_html` 行内混排 / `fence_tags_all_accepted` 拒 `<br>`），或 Unity 静默丢属性（`inset:0` 没反应但浏览器看着对）。

**根因**：① `<br>` 不在围栏白名单（只有 div/span/img/button），围栏外标签一律报错。② `<span>文本 <span id="count">0</span> 文本</span>` = 行内混排（元素内文本+子元素），编译期报错。③ `inset` shorthand 围栏外——`apply_decl` 只映射 `top`/`right`/`bottom`/`left` 四个显式属性，`inset:0` 无 handler 静默丢（浏览器原生支持 → 预览看着对，Unity 不生效——典型"信围栏别信预览"）。

**解决**：① 文本分段用 `<div>` 子节点（每个 div = 一行 flex item），不用 `<br>`。② 计数 label 改兄弟 span 在 flex-row div 内（每 span 纯文本，无嵌套混排）。③ `inset:0` 改 `top:0;left:0;width:100%;height:100%`（四个显式 fence 内属性）。

**教训**：showcase HTML 也是 pkg 源，**守围栏**——别用 `<br>`（围栏外 tag）、别 span 套 span + 文本（行内混排）、别用 `inset` shorthand（围栏只认四个显式方向）。打包器是 fence gate：改了 HTML 必重打验证；浏览器支持不等于围栏支持（浏览器 mask Unity 的静默忽略 bug）。

### 坑 126：`<base href>` 影响 `location.href` 相对赋值——showcase 预览导航 404

**症状**：showcase 浏览器预览中 nav 跳转 404——`location.href = 'page_list.html'` 没跳 `showcase/page_list.html`，跳到 `LoomUI/page_list.html`。

**根因**：HTML `<base href="..">` 不仅影响 `<img>`/`<link>`/`<script>` 的内容 URL 解析，**也影响 `location.href = 'relative'` 的相对赋值**——JS 对 location 赋相对 URL 时按文档 **baseURL**（受 `<base>` 控制）解析，而非文档地址栏 URL。`<base href="..">` 锚到 `LoomUI/`，故 `'page_list.html'` → `LoomUI/page_list.html`（404——文件在 `LoomUI/showcase/` 下）。

**解决**：不从相对路径依赖 baseURL。用 `location.href` **getter**（返回文档真实 URL，不受 `<base>` 影响）取当前目录拼**绝对路径**：`var dir = location.href.substring(0, location.href.lastIndexOf('/') + 1); location.href = dir + name + '.html';`——与 `<base>` 行为彻底解耦。`<base>` 保留给 `<img>`/`<link>`/`<script>` 内容路径。

**教训**：`<base>` 既影响内容属性（已知）也影响 **JS `location.href` 相对赋值**（按 baseURL 解析——易漏）。同目录跳转不想踩 base 歧义时用 `location.href` getter 取真实 URL 拼绝对路径。不要单靠资料推理——浏览器行为（`<base>` + location 赋值）须在目标环境实证验证。



### 坑 127：NativeHost 空 div slot 不进 blob——Sync 查不到（v1d.3 假设漏洞）

**症状**：page_nativehost 角色挂 nh-stage（空 div slot），Bind 成功但角色永远隐藏（GO active=false）。

**根因**：v1d.3 NativeHost-lite 假设后端能从 `frame.nodes`（blob）拿任意绑定节点 transform。但 blob 是**渲染列表**——空 div（无 bg/text/img）产的透明 Container mesh 被 `merge_meshes` 合并掉 → slot 不在 blob → Sync 遍历 blob 找不到 slot → wrapper 永不设置 → GO 永久 `SetActive(false)`。dump 验证：nh-stage NOT IN frame.nodes，但 `scene.world_transforms`/`node_sort_keys` 含它。

**解决**：NativeHost Sync 走 FFI 按 nodeId 查（`get_node_world_matrix`/`sort_key`/`visible`，读 `world_transforms`/`node_sort_keys`——按节点 index 存全节点，独立于 merge）。不再遍历 blob。core 加 `node_sort_keys` 快照（assign_sort_keys merge 前填）。

**教训**：blob 是渲染列表（受 merge 优化），**不是 transform 通道**。NativeHost 查询通道必须独立于 merge。

### 坑 128：NativeHost Bind 直接绑 prefab asset 报错（caller 必须传场景实例）

**症状**：Inspector 把 prefab asset 拖进 driver 字段，Bind 报 "Transform resides in a Prefab asset and cannot be set"。

**根因**：`NativeHostManager.Bind` 对传入 GO 调 `SetParent`——但 prefab asset transform 不可改（Unity 禁）。Bind 契约是场景实例，caller 传 prefab asset 越界。

**解决**：driver `EnsureXxxInstance` Instantiate prefab 成场景实例后绑（fgui `GoWrapper` 同款：caller 管实例化，框架只显示）。**Bind 内部不该 Instantiate**（生命周期归 caller）。

**教训**：NativeHost Bind 契约 = 场景实例。caller（driver）管实例化 + 生命周期。框架不越界实例化（fgui `SetWrapTarget` 接收已有 GO，不 Instantiate）。

### 坑 129：URP/Lit `_Surface=0`（Opaque）即使 renderQueue=3000 仍在 Opaque pass（坑 127 demo 验收）

**症状**：NativeHost 3D GO 设 `renderQueue=3000`（Transparent）+ `ZWrite Off`，仍被 UI 覆盖看不到。

**根因**：URP/Lit shader 用 keyword `_SURFACE_TYPE_TRANSPARENT`（Lit.shader:125）+ property `_Surface`（:48）决定 variant/队列。`_Surface=0`（Opaque 默认）即使 `renderQueue=3000`，shader 走 Opaque variant → 在 Transparent 之前画 → 被 UI（Transparent）覆盖。`renderQueue` 只设队列，`_Surface` 设 shader Blend/variant——两者要配套。

**解决**：URP/Lit 切 Transparent 三件套：`renderQueue=3000` + `SetInt("_Surface",1)` + `EnableKeyword("_SURFACE_TYPE_TRANSPARENT")` + `SetInt("_ZWrite",0)`。`_ZWrite` 也是 URP/Lit property（:56 `ZWrite[_ZWrite]`）。

**教训**：URP material 切 Transparent 不只 `renderQueue`——要 `_Surface` + keyword 配套（缺 keyword 不生效）。

**ownership 边界（v1.5 后）**：材质归 caller GO，框架不自动碰——早期 `CacheRenderers` 在 `Bind` 时直接改 `sharedMaterial`（污染资产 + 影响所有引用者），已废弃。现框架提供 `NativeHostManager.ConfigureTransparentMaterials(go)` / `UnconfigureTransparentMaterials(go)` 静态工具：前者 clone sharedMaterial + 设三件套 + 挂回 `renderer.material`（instance，不污染资产），clone name 追加 `" (LoomNH)"` 标记；后者据后缀销毁 clone。caller 在 `Instantiate` 后调 Configure、销毁 GO 前调 Unconfigure——clone 的 ownership 走 caller，框架不跟踪、`Bind` 不自动调。原因：clone 挂 user GO 的 renderer，框架销毁 clone 会破坏 caller 还在用的 GO（renderer.material 槽变 destroyed），材质不像 wrapper GO 能 reparent 隔离——必须随 user GO 归 caller。

### 坑 130：orthographic 相机 3D GO scale z 大 → near clip（坑 127 demo 验收）

**症状**：`_characterScale=(120,120,120)` → 角色看不到（bounds z Extents 254，超相机 near/far）。

**根因**：orthographic 相机 z 缩放**不影响视觉大小**（只看 z=0 平面投影），只放大 depth 厚度。z=120 → 角色 z 厚度 ~254 → 超 near（0.1，z=-9.9）被切大部分。

**解决**：driver `_characterScale` 改 Vector2（xy），z 由 driver 自动设 1（不暴露给用户填）。粒子 `_effectScale` 同。

**教训**：orthographic 相机下 3D GO scale 的 z 分量该用小值（1）——z 缩放只增 depth 厚度（near/far clip 风险），不增视觉。别让用户填 z（陷阱）。

### 坑 131：display:none 子树不剪 → 0 尺寸 Text 字形残留（"隐藏内容仍显示"）

**症状**：`[data-page]` 切隐藏页（display:none）后，被隐藏的 Text 内容仍显示（堆屏幕左边）；panel/dialog/b-icon 全如此。

**根因**：`build_render_nodes` 遍历所有节点无过滤。display:none 节点 taffy 不布局（layout_rect=0），但其 Text 后代仍产 RenderNode——Text 字形按自身 TextLayout 尺寸渲染（不靠节点 rect），0 尺寸也画出来。

**解决**：build 前预计算 display:none 子树（`collect_display_none_subtree`），遍历 + batch DFS 跳过。id_to_pos 改 push 时记（剪枝后 nodes 与 scene.nodes 不等长，破坏 batch 旧契约"同长同序"）。

**教训**：CSS `display:none` 整子树不渲染是语义不变量——core 产 RenderNode 必剪，不能靠"rect=0 引擎自然不画"（Text 例外）。改 build 遍历要检查 batch/id_to_pos 等假设"nodes 与 scene 同长同序"的下游。

### 坑 132：runtime color 继承缺失 → 选中/hover 文字色不传子 Text

**症状**：选中 tab / hover 按钮时 Container 背景变色了，但文字色没变（子 Text 保持灰）。

**根因**：打包期 `resolve_rec` 把父 color 继承进子 base_style（静态），但 runtime `rematch_pseudo_classes` 逐节点从 base_style 重起**不读父**——父命中选中/hover 改 style.color，子 Text 的 base_style.color 是打包期旧父色，不跟随。

**解决**：rematch 末尾按树序 `propagate_color_inheritance`：子若 `new.color == 父 base.color`（没声明没动态命中）则继承父 effective color（anim tween 当前值优先）。判据用 rematch 后的 new.color 不是 base.color（默认黑场景 base 都相等会误判）。

**教训**：CSS color 是继承属性，"打包期展开"只够静态；runtime 父色 dynamic 变化（伪类/Controller）要补树序传播。判据"base==parent base"在默认值场景失效，改"new==parent base"。

### 坑 133：transition 逗号多 spec 被 split_whitespace 吞（color spec 丢）

**症状**：`transition:background-color 0.3s, color 0.3s` 只 BgColor 生效，color 瞬切没渐变。

**根因**：`parse_transition` 用 `split_whitespace`，逗号附 token（`ease-out,color`）→ ease 分支吞，第二 spec 的 color 被当 prop 覆盖或丢。

**解决**：`parse_transition` 外层 `split(',')` + 每段 `parse_one_transition`；`ResolvedStyle.transition` 从 `Option<TransitionSpec>` 改 `Vec<TransitionSpec>`，rematch emit 遍历 Vec。

**教训**：CSS 多值属性逗号分隔（transition 等），解析要 split(',') 再每段，不能 split_whitespace 一把梭。

### 坑 134：findstr 扫 PE 导出表不可靠 → 用 GetProcAddress 验 dll 导出（坑 100 增强）

**症状**：改 FFI 后 `findstr /c:"symbol" file.dll` 查导出全 MISS（即使刚从当前源码编出的 dll）。

**根因**：findstr 按文本行扫二进制，PE 导出表的 ASCII 符号名常被 findstr 行处理/编码吞——工具不可靠，非 dll 缺符号。

**解决**：用 OS 加载器级查询：PowerShell `LoadLibraryEx(DONT_RESOLVE_DLL_REFERENCES)` + `GetProcAddress`（非 0 = 真导出）。或 `dumpbin /exports`（要 VS）。

**教训**：坑 100 的"findstr/nm 查新符号"——findstr 不可靠，改 GetProcAddress（权威）。stale .dll 诊断不能靠 findstr 假阴性。

### 坑 135：bump formatVersion 后重打 showcase 漏 → stale pkg 版本号（hexdump 验）

**症状**：core `MIN_VERSION` bump 后，入库 `showcase.pkg.bin` 仍是旧版本号 → dump example `read_package: TooOld` panic；Unity PlayMode 加载也 TooOld。

**根因**：bump 在一个 commit（改常量），showcase.pkg.bin 重打在另一 commit（worktree 可能用旧 pkg crate，或忘重打），入库 pkg 版本号字段是旧值。

**解决**：`od -An -tx1 -N 8 showcase.pkg.bin` 看头——magic `4c 50 4b 47`("LPKG") 后 4 字节 u32 LE 版本号（13=`0d 00 00 00`）。不等 MIN_VERSION = stale，重打。

**教训**：坑 66 的版本号维度——bump formatVersion 后入库 pkg.bin 版本号字段要 hexdump 核对（头 8 字节），不能只看文件大小变了就以为打了新版。

### 坑 136：text mesh 顶点缺 rect 偏移 + y 符号反（文字堆左上角 + 上下颠倒）

**症状**：v1.6 font-to-core 首次 PlayMode，所有文字堆屏幕左上角且上下颠倒。

**根因**：`build_text_mesh` 顶点用 pen 坐标（node-local 0..text_width），但 blob re-base 契约假设顶点在 parent/world 空间（`mesh::quad` 用 `rect.x/rect.y`）——re-base 减 (tx,ty)、后端 GO 加回，净值落 design 原点 (0,0)。且 `top = line.baseline + g.bearing_y` 符号反：`baseline` 是 y-down 绝对值，`bearing_y` 是 font y-up（顶到 baseline 正值），y-down 里字形顶在 baseline 上方应 `baseline - bearing_y`。

**解决**：`build_text_mesh` 加 `rect` 参数，顶点加 `rect.x/rect.y`（对齐 Image quad 父空间约定）；`top = baseline - bearing_y - pad`、`bottom = top + px_h`；减 `GLYPH_PAD`（pub const）对齐位图原点（bearing 来自 bbox，px_w/h 含 pad）。

**教训**：新增 mesh 顶点源要和 `mesh::quad` 同空间约定（rect.xy = 节点绝对位置，纯平移时 = wm[4],wm[5]）——否则 re-base 契约失配。font y-up 度量进 y-down 坐标系一律减（bearing/ascender）。commit 9db4550 留了"PlayMode 实测校准"注释就是没验就 merge——v1.6 文本几何要 PlayMode 过。

### 坑 137：SyncFontAtlas 硬编码 f0 vs render f{font_id}（非默认字体全空白）

**症状**：JetBrainsMono（首个注册）默认时英文显示；DejaVuSans 默认时所有字符不显示（空白）。

**根因**：`font_id` 按注册顺序分配（`next_id` 0 递增），`is_default` 不影响它。`SyncFontAtlas`（LoomStage.cs）硬编码注册 `loomgui://font-atlas/f0/p{page}`，但 render 用实际 `font_id` 合成 `f{font_id}/p{page}`——默认字体非首注册时 font_id≠0，path 不匹配 → SpriteResolver miss → tex 回落 whiteTexture → 全空白。

**解决**：core 的 `GlyphAtlas` 是 Stage 级单一共享实例（所有字体字形混在同一 page），path 里的 font_id 本就冗余。两边统一去掉 font_id：`loomgui://font-atlas/p{page}`（page 是唯一键）。

**教训**：合成 path 的 key 要和后端注册 key 严格对齐——一边硬编码常量一边用动态 id 是定时炸弹。共享 atlas（多字体同页）路径不该含 font_id（font_id 只作 GlyphKey 区分槽位，不进 path）。对**未来 CJK fallback** 也成立：fallback 字形仍进同一共享 atlas，path 只需 page。

### 坑 138：无轮廓字形误走 tofu（空格渲染成方块 + 位置偏下）

**症状**：空格渲染成方块，位置偏下（在 baseline 下方而非行内）。

**根因**：`rasterize_glyph` 对 `glyph_bounding_box` 返 None 的字形一律 `tofu_box`。空格 U+0020 有 glyph entry（gid>0、advance 正常）但无轮廓 → bbox None → 误走 tofu。且空格 `bearing_y=0`，tofu 框 `top = baseline - 0 - pad` → 从 baseline 往下 size_px → 位置偏下。

**解决**：按 gid 区分——gid=0（真缺字 .notdef）保留 tofu（开发期占位）；gid>0 无轮廓（空格、nbsp、零宽空格等）返空 bitmap，`ensure` 早返空 rect（不分配/不标脏，避 `allocate(0,0)` panic），`build_text_mesh` 跳过 `px_w==0` 字形（advance 在 layout 已算，pen 已前进）。

**教训**：无 bbox ≠ 缺字——空格等有 advance 无像素的字形不该画占位。tofu 只给真缺字（gid=0 .notdef，且字体连 .notdef 方框都没有时才兜底；字体自带 .notdef 方框走正常光栅）。光栅兜底要区分"缺字"（gid=0）与"无轮廓"（gid>0 bbox None）。

### 坑 139：RichText 行内图合成 id 触发 reorder panic（Unity 闪退）

**症状**：PlayMode 进含行内图（`<img>` in `<div style="display:block">`）的富文本页，Unity 直接闪退（无 Console 报错，进程消失）。

**根因**：RichText build（`render/mod.rs` 行内图 arm）给行内图 image mesh 设 `node_id = synth_text_node_id(...)`（合成 id，不在 scene）+ `program: 0`（mergeable）。batch 的 `reorder_for_batching` → `reorder_unit` 对 mergeable mesh 用 `aabb_of` 反查 `scene.get(NodeId).expect("live node")` 取 layout_rect 算 AABB 重叠——合成 id 不在 scene → `scene.get` 返 None → `.expect` panic。cdylib panic 不能 unwind → abort → 拖垮 Unity 宿主进程（坑 102 实例）。text 跨页子页也用 synth id 但 `program: 1`（不 mergeable）→ 不进 reorder → 不崩；**只有行内图（program:0 mergeable + synth id）触发**。render 单测漏：单 RichText 节点 n<2，`reorder_unit` 提前 return 不触 aabb_of。

**解决**：`batch.rs` 的 `aabb_of` 对 `scene.get` 返 None 零面积兜底（`.unwrap_or(Rect{0,0,0,0})`），不 panic。行内图位置已由 build 期 mesh verts 固定，reorder 只按 draw_state（image_path）归批，零面积 AABB 让它不参与重叠判断（本就无 scene layout_rect）。

**教训**：
- 渲染管线（build/reorder/merge）绝不能 panic——任何 `.expect`/`unwrap` 遇合成 id / 临时产物 / 边界都可能 non-unwinding abort 拖垮宿主。用 `unwrap_or` 兜底。
- 合成 id（`synth_text_node_id`：text 跨页子页 / 行内图）是有意不进 scene 的纯渲染产物，任何"反查 scene"的逻辑（reorder AABB / sort_key / NativeHost 查询）都要对它兜底。
- 单测要覆盖**多节点 reorder**（n≥2 才进 `reorder_unit`），单节点测不到 aabb_of 路径。
- PlayMode 闪退先查 project-relative `loomgui_unity/Logs/Editor.log` 末尾的 panic 栈——比读代码猜快得多。

### 坑 140：parse_color 不支持 3 位 hex（#RGB）

**症状**：`<span style="color:#888">`（3 位 hex）不生效，run 回退 base 色；6 位 hex 正常。失效的全是 3 位（#888/#3f6/#aaa），6 位（#ffd700/#5fb2c4）正常——按 hex 位数定位。

**根因**：`style::mapping::parse_color` 只认 `len()==6`，3 位返 None → `apply_inline_style` 跳过 → run 用 base 色。单测只测 6 位，PlayMode 才现（坑 131-133 类）。

**解决**：加 `len()==3` 分支，每数字重复（`#rgb→#rrggbb`，digit×17：0→0、f→255）。

**教训**：CSS 颜色子集要覆盖常见简写。单测别只测一种形式——按"失效的全是 X 特征"快速定位。

### 坑 141：RichText 叶漏画背景 quad

**症状**：`<div style="display:block" class="rt">`（.rt 有 background-color）Unity 无底色，文字直接叠父容器。

**根因**：desugar 把 block div 覆盖成 `NodeKind::RichText`，但 Container arm（`render/mod.rs:186`）的 bg quad 逻辑不覆盖 RichText arm（`:391` 直接产 text mesh 跳过 bg）。block div 盒属性留在 style，但叶没画 bg。

**解决**：RichText arm 在 push_text_meshes 前产 bg quad（`n.style.background_color`，program 0/3）。z 序：bg 占真 node_id（DFS sort_key S），text 首页改用 synth 子页 1（propagate 给 S+1）→ bg 下层。无 bg-color 时 text 用真 node_id（原行为不变，现有测试不破）。

**教训**：desugar 覆盖 kind 时，原 kind 渲染逻辑（bg）不自动迁移——叶要自画。z 序靠 id_to_pos + synth 子页机制。单测验 text 没验 bg（PlayMode 才见底色缺）。

### 坑 142：underline 偏移方向反（偏上）

**症状**：富文本下划线偏上（跑到字形里），删除线略偏下。

**根因**：`build_text_mesh` underline `line.baseline + position×scale`，`underline_metrics.position` 是 y-up 负值（基线下方），design y-down 加负 → baseline 上方（字形里）。注释误以为"经 Unity y-flip 后变下方"——design 内就该下方，与 flip 无关。

**解决**：改 `line.baseline - position×scale`（减负=加=baseline 下方）。strike 0.25→0.3em（略上移）。

**教训**：y 偏移明确 design y-down 语义（下方=y 更大=减 y-up 负值）。别靠后端 y-flip 掩盖 design 方向错。坑 136（text mesh y 符号反）同类。

### 坑 143：行内图 verts 缺 rect 偏移（跑原点）

**症状**：富文本行内图不显示（跑 design 原点，屏外）。

**根因**：RichText arm 行内图 verts `ix=img.x+off_x`（缺 `rect.x`），字形 `left=g.x+...+rect.x` 加了。单测 rect=(0,0) 巧合过（坑 131-133 类）。

**解决**：行内图 `ix=img.x+off_x+rect.x`，`iy=img.y+off_y+rect.y`。加位置测试（rect≠0 验 min_x>=rect.x）。

**教训**：mesh verts 世界坐标都要加 rect.x/y（blob re-base 减、后端加回）。位置单测用 rect≠0——rect=0 掩盖位置 bug。

### 坑 144：bake_content_offset 只烤 glyphs 不烤 line.baseline → 文字缺 padding top

**症状**：富文本/文本文字顶到盒顶（无 padding top），html 文字在 padding 内（视觉居中），Unity 上对齐。行内图加了 off_y、文字没加 → 行内图比文字低 padding（偏下）。框架层（所有 Text/RichText，正是"所有文本没考虑好"的怀疑）。

**根因**：`bake_content_offset` 只烤 `glyphs.x/y`，但字形 top=`line.baseline-bearing+rect.y` 走 `line.baseline`（不读 g.y）→ y padding 丢。x 走 g.x（含 off_x）对，y 走 line.baseline（不含 off_y）错——x/y 不对称。

**解决**：bake 也烤 `line.y`+`line.baseline`（+off_y）。行内图世界 y=`line.y+y_top`（layout 存世界 y，含 line.y）。fragments y 用 line.y（已含 off_y，去掉旧 +off_y）。

**教训**：layout 偏移要烤全（glyphs+line.y/baseline+images），别只 glyphs——字形 top 走 line.baseline 不走 g.y。padding top 偏移是框架层（Text+RichText 共享 bake）。行内图偏下 + 文字垂直对齐同根因——别当两个 bug。

### 坑 145：measure_rich_text split(' ') 丢空格 → 无空格 + 断行错

**症状**："fragment 矩形" 渲染成 "fragment矩形"（无空格），断行点与 html 不一致（html 空格后断，Unity CJK 后断，导致超界）。

**根因**：`measure_rich_text` 用 `text.split(' ')`，词间空格不补 token（注释说 MVP 丢空格"精度可忽略"）。空格 advance 丢 → 无空格 + 断行点错（空格是词边界）。

**解决**：split 后词间补空格 token（advance=space advance；空格无轮廓不画，但占宽 + 词边界断行）。HTML 空白折叠（连续空格→单，split 空 part 跳过）。

**教训**：空格不是"可忽略精度"——占宽 + 断行边界，丢了两都错。MVP 简化别砍关键语义。框架层（rich 文本空格；plain measure_text 若不同需对齐）。

### 坑 146：富文本两个 PlayMode 集成 bug（行内图白方块 + text-align 失效）

**症状**（v1.7 rich text showcase，PlayMode 才现，单测全绿）：
1. 行内 `<img>` 渲染成白色方块（图没出来）。
2. `text-align:center/right` 都显示成左对齐。

**根因**：
1. `scene_to_template`（打包期）只对 `NodeKind::Image` + `background-image` 调 `normalize_path`（剥 `res/` 前缀 + 入 manifest）；**行内图嵌在 `NodeKind::RichText { runs } → RichKind::Image`，顶层 match 不下钻**。src 保持 `res/icons/zap.png` → 运行时 `SpriteResolver.TopDir="res"` 无 folder→atlas 映射 → 默认图集 miss → `GetSprite` 返 null → 白方块（且不入 manifest，Unity 根本没打包进 atlas）。
2. `measure_rich_text` 签名没有 `align` 参数——`MeasureContext::RichText.align` 在构造时已填，但 measure 闭包调用处用 `..` 把它吞掉了（注释写"T4 边界：rich 不支持对齐"）。glyph/image 永远 `pen_x=0`，无对齐偏移。

**解决**：
1. `scene_to_template` 加 `NodeKind::RichText` 分支：遍历 runs，对 `RichKind::Image` 的 src 同样 `normalize_path` + 入 manifest + 回写。
2. `measure_rich_text` 加 `align` 参数，每行（glyph + 同行 image）按容器宽（`max_width`，render 传 `rect.w`）整体偏移；`max_width=None`（不换行）或行宽>容器时不偏。render 调用处传 `s.text_align`。

**教训**：
- **打包期对每种"带 src 的节点"都要走 normalize + manifest**——`NodeKind::Image` 是顶层，`RichKind::Image` 嵌在 RichText runs 里。新增任何携带图片路径的 NodeKind/Run 都须在 `scene_to_template` 补归一化，否则运行时路径不匹配 → 白方块/缺图。顶层 match 不会自动下钻嵌套结构的子字段。
- **"已填字段但调用处没读"是隐蔽 bug 源**——`MeasureContext::RichText.align` 构造时赋值却用 `..` 吞掉，单测不覆盖（单测直接调 measure_rich_text 不经 MeasureContext）。富文本对齐只在 PlayMode（经完整 measure 闭包）才显现。坑 131-133 同类：单测绿 ≠ 集成对。
- **text-align 语义**：要对齐到盒边（rect.w），须在 render 期（知 rect.w）应用，measure 期用 available width 近似——stretch 布局下二者相等。plain `measure_text` 目前对齐基准是 text_width（最宽行），单行短文本同样不生效；rich 修法（对齐 rect.w）比 plain 更对，plain 可后补。

### 坑 147：long-running worktree 期间 main 前进 → merge 非快进 + 签名冲突（v1.8 SDD）

**症状**：v1.8 13 task 用 worktree 串行做（worktree 从 `1252f0b` 派生），做完 merge 回 main 时不是 fast-forward——main 已前进 9 个 commit（v1.7 PlayMode fixes `8155e95` + showcase + fence doc sync）。`render/mod.rs` 的 `build_text_mesh`/`push_text_meshes` 两边都改了**同一签名**（main 改 `push_text_meshes` 加 `register_id_map: bool`；v1.8 改加 `shadow_pairs`/`stroke_pairs` + `TextMeshes` 返回类型），git 无法自动合并，整段函数体标冲突。

**根因**：worktree 长时间运行（13 task × review），期间 main 被别的会话推进。两边独立改了同一核心函数签名，git merge 只能标冲突让人解。

**解决**：反向 merge（`git merge main` 进 worktree 分支，在 worktree 解冲突——有完整 v1.8 上下文）。以 v1.8 超集签名为基准（`push_text_meshes` 同时收 `register_id_map` + `text_primary_id` + `shadow_pairs` + `stroke_pairs`），把 main 的 4 个修复点逐个移植进 v1.8 代码路径：① has_rich_bg 背景逻辑 + text_primary_id 子页 1；② underline offset `- position`（坑 142）；③ `bake_content_offset` 也烤 `line.y`/`line.baseline`（坑 144）；④ 行内图 verts 加 `rect.x`/`rect.y`（坑 143）。3 个 main 的 v1.7 测试（`rich_text_node_emits_bg_quad` / `rich_text_padding_offsets` / `rich_image_positioned_at_node_rect`）是合并验收标准——移植完应全绿。解完 fast-forward main。

**教训**：
- **worktree 长任务前先确认 main 是否会动**——若 main 可能被推进，merge 时必有冲突。本机串行工作流下 main 由本会话/别的会话推进都可能。
- **签名冲突要合超集，不是二选一**——两边改同一函数时，合并签名收两边的参数（main 的 bool + v1.8 的 pairs），函数体取超集逻辑 + 移植对方的非签名修复（offset/bake/rect 偏移等）。别丢任一边的修复。
- **用对方分支的测试当合并验收标准**——main 的 v1.7 测试失败点精确指向要移植的修复，测试绿 = 合并正确（单测能抓签名/逻辑合并错；PlayMode 集成 bug 仍抓不住，坑 131-133）。
- **反向 merge（main 进 feature 分支）比正向（feature 进 main）安全**——冲突在 feature 分支解（有 feature 上下文），main 不动直到 fast-forward，解错也不污染 main。

### 坑 148：subagent 撞 API 限流中断 → 代码完整但没收尾（v1.8 SDD）

**症状**：subagent-driven SDD 期间，implementer subagent 撞账户 5 小时 API 上限被 kill。日志显示断点时**代码已写完 + 全测试套件已跑通**，但没 commit / 没写 report / 没跑完 fmt+clippy。controller 拿到的是"代码完整但未提交"状态。

**根因**：subagent 的收尾步骤（fmt→clippy→commit→report）是限流后未跑的部分。subagent 被 kill 不回滚已写的代码，留下半成品工作区。

**解决**：controller 核实状态后补收尾——① `git status` 看代码是否写完（新文件 + 改文件都在）；② `cargo build` + `cargo test` 确认代码完整能跑（implementer 断点前跑通的测试应仍绿）；③ 跑 `cargo fmt`（implementer 常没跑，有格式 diff）+ `cargo clippy`；④ 补缺失的单测（日志常显示断点时正想加的测试，如 `as_corners`）；⑤ commit + 写 report（controller 基于实际 diff 补写，注明 implementer 限流断点 + controller 收尾）。然后正常派 reviewer。

**教训**：
- **subagent kill 不回滚代码**——撞限流/超时被 kill 后，先 `git status` + `build` + `test` 核实代码完整度，别假设白干或假设完成。日志最后一句常提示断点时在做什么（"add a unit test for X" = 正加测试）。
- **限流背景下的收尾走 controller 工具调用（Bash/Edit），不派 subagent**——Bash/Edit 不耗 Agent API，能绕限流。确定性的收尾（fmt/clippy/单测/commit/report）controller 直接做比再派 subagent（会再撞限流）快且稳。
- **implementer report 注明限流断点**——controller 补写的 report 写清"implementer 限流断在 X，controller 补 Y"，让 reviewer 知道证据链（TDD RED/GREEN 可能没记录）。
- **别假设完成**——subagent 报"running tests"被 kill ≠ 测试过，要自己跑确认。

### 坑 149：text-shadow 赋值清空 text_effects → 与其他文字效果属性不组合（v1.8 review 抓）

**症状**：v1.8 Task 8 让 `text-shadow` 用 `style.text_effects = shadows`（赋值），Task 9/10 让 `-webkit-text-stroke`/`font-effect` 用 `.push()`（追加）。混用时声明序敏感：`font-effect:glow(...); text-shadow:...;` → glow 被 text-shadow 赋值清空丢失（违反 CSS 语义——不同属性应组合）。

**根因**：text-shadow 的赋值语义是 v1.7 单属性思维（text-shadow 重声明应覆盖旧 shadow），但 v1.8 加了 stroke/glow 共享 `text_effects` vec 后，赋值会误杀其他属性的 effect。CSS cascade：同属性后声明覆盖，不同属性组合。text-shadow 和 font-effect 是不同属性，应组合。

**解决**：text-shadow 改 `retain(|e| !matches!(e, FontEffect::Shadow{..}))` + `extend(shadows)`——只替换自己的 Shadow 变体，保留 Stroke/Glow/Blur。加反向声明序测试（`font-effect` 先 `text-shadow` 后，两者都在）。

**教训**：
- **多属性共享一个 vec 时，每个属性只管自己的变体**——赋值（`=`）清空整 vec 会误杀别的属性；用 `retain(非自己变体) + extend` 隔离。判据：CSS 里这两个是 same property（覆盖）还是 different property（组合）。
- **AI 可预测性 = CSS 语义对齐**——AI 按 CSS 训练，期望不同属性组合。赋值清空让"声明序"影响结果，AI 无法预测。reviewer 抓这类"语义不一致"比抓 bug 更重要（项目首要判据是 AI 可预测性）。
- **新属性接共享字段时要审既有属性的写入语义**——Task 8 text-shadow 先建 text_effects 用赋值，Task 9/10 加属性用 push，不一致在 Task 10 review 才抓出（pre-existing bug 加剧）。加属性时 grep 共享字段的既有写入，统一语义。

