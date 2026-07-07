# RmlUi / FairyGUI 对标调研（v1.6+ 规划依据）

> 本文替代旧 `borrow-from-rmlui.md`（结论有误已删：曾把"RmlUi 九宫格支持平铺"当真、把"字体核心自绘"列为不抄）。
> 六方向调研，全部读真实源码给 file:line 证据，批判验证、不复述假设。参考源：RmlUi `temp/RmlUi/`（libRocket fork v6.3，只读）、FairyGUI `temp/FairyGUI-unity/`（只读）。
> 结论直接驱动 roadmap §2 的 v1.6-v1.x 排期。

## 目录

1. [字体渲染搬进核心（架构决策）](#1-字体渲染搬进核心架构决策)
2. [HTML/CSS 视觉效果](#2-htmlcss-视觉效果)
3. [九宫格 / 平铺装饰](#3-九宫格--平铺装饰)
4. [动画 HTML/CSS](#4-动画-htmlcss)
5. [FairyGUI 控件 / 能力 gap](#5-fairygui-控件--能力-gap)
6. [v1.1 以来未记录的 defer / 技术债](#6-v11-以来未记录的-defer--技术债)
7. [别抄清单（架构冲突 / 已领先）](#7-别抄清单架构冲突--已领先)

---

## 0. 一个纠错前提

旧文档的两处硬错，本轮调研推翻：

- **"RmlUi 九宫格支持平铺"是错的**。RmlUi 的 `DecoratorNinePatch` 和 `DecoratorTiledBox` 边块/中心块**也全是 stretch**，真正的 repeat 只在单图 `DecoratorTiledImage`（证据 §3）。LoomGUI 九宫格与 RmlUi 在"边不平铺"这点上其实平手。
- **"字体核心自绘抄不了"是过时结论**。旧文档基于"核心不持字形位图"直接否决描边/发光。但那是**因为当前架构选择后端光栅化**，不是不可行——Rust 生态（ab_glyph_rasterizer + etagere）完全能让核心自绘 atlas（§1）。这条是 v1.6 的核心决策。

## 1. 字体渲染搬进核心（架构决策）

> **结论：搬。作为 v1.6 文本线地基先做。** 理由是架构纯净度而非工作量——补齐"引擎无关纯核心"立项根基里唯一的破例。

### 1.1 当前文本管线真相（file:line）

核心侧**只做测量+布局，不产任何像素/UV/几何**：
- `measure_text`（`loomgui_core/src/text/layout.rs:171-377`）：`ttf-parser 0.20` 取度量 + `unicode-linebreak` 断行（CJK 逐字），产 `TextLayout`（`layout.rs:52-58`）= lines[→runs[→glyphs{glyph_id,codepoint,x,y,bearing}]]。字形坐标是绝对 pen 坐标（已累加 advance + align），后端零累加。
- 进程级单字体、无 fallback（`layout.rs:60-65,100-109`）。依赖仅 `ttf-parser=0.20` + `unicode-linebreak=0.1`（`Cargo.toml:10-11`）。打包器完全不碰字体。
- FFI blob 每字形只序列化 3 字段 `{codepoint,x,pen_y}`（`loomgui_ffi_c/src/blob.rs:209-214`）——**没有 UV、没有几何**。Text 合批时永远独立不合并（`render/merge.rs:5,41`）。

**Unity 后端才是光栅化器**（`loomgui_unity_package/Runtime/TextRasterizer.cs`）：
- `font.RequestCharactersInTexture`（`:43`）填 Unity 动态字体 atlas，`GetCharacterInfo`（`:55`）取像素 box + UV。**UV 100% 来自 Unity，Rust 不产 UV**。
- atlas rebuild 应对：`Font.textureRebuilt` 事件 → `_fontVersion++`（`LoomStage.cs:118-123`）→ `MirrorPool.Sync` 强制所有 text 重光栅（`MirrorPool.cs:91-94`），且处理"Sync 中途 rebuild"（`:158-162`）——这是坑 113 的修复补丁。

> **文档纠错**：`main-design.md §8.2`（359-368）描述的 3 表 SOA + bearing + inline_objects 富文本内联表**与实际不符**——实际 blob 每字形只有 `{codepoint,x,pen_y}`，设计文档超前描述了未实现形态。

### 1.2 用户四诉求逐一验证——"一举多得"是误判

| 诉求 | 真相 | 判定 |
|---|---|---|
| ① 字符串嵌套/行内混排 | 编译期报错在 **DOM 解析层**（`parse/dom.rs:104`），根因是核心不变量"div 永远 flex、无 inline flow"。解药是 inline formatting context（IFC）= 布局子系统，与光栅化正交 | **搬字体一行都不解决**，是 v1.7 富文本的范畴 |
| ② 大小渲染不一致 | 已基本解决——Rust 权威度量 + Unity 只填像素，同 ttf 下大小/断行已跨平台一致。残余不一致来自 Unity padded quad vs Rust bbox（禁 kerning `layout.rs:203-213`、坑 119） | 搬后可重开 kerning、消 padding 歧义。**锦上添花非救火** |
| ③ 富文本 | 主体是 IFC 布局（同①），不是光栅化。不搬字体 Unity 动态字体一样能多字号多色 | 搬字体只帮部分（per-glyph color 进顶点更易）。**主体工作量在 IFC** |
| ④ 描边/下划线/阴影/渐变字体 | 下划线(纯quad)、阴影(偏移复制TextLayout)、渐变字体(顶点色)**现在就能做，不依赖搬字体**；只有**描边/发光/blur**（邻域采样类）真正独占依赖核心持字形位图 | **只有描边/发光/blur 是搬字体的独占价值** |

**搬字体的真实独占收益** = 跨引擎一致的描边/发光/blur + 根治坑 113/119 + text 可参与合批 + 补齐"一份核心多后端"的最后破例（接 Godot 时不必重写文本光栅化）。**不是**嵌套/富文本的解药。

### 1.3 RmlUi 怎么做（核心自绘 atlas 的成熟范式，file:line）

路径相对 `temp/RmlUi/`：
- **光栅化**：FreeType 光栅成 CPU 灰度/彩色位图，核心自持 `bitmap_owned_data`（`Source/Core/FontEngineDefault/FreeTypeInterface.cpp:290-448`；`FontGlyph.h:11-38`），支持 MONO/GRAY(A8)/BGRA(彩色 emoji)。
- **atlas**：shelf packing 按高度降序行装箱、字形间 1px padding 防渗色（`FontFaceLayer.cpp:73-104`，`TextureLayout*.cpp`）。单张上限 1024²，放不下多张（`TextureLayout.cpp:50-68`）。UV = 像素/贴图尺寸（`FontFaceLayer.cpp:109-125`）。
- **贴图交后端**：Callback 延迟生成 `RenderInterface.h:59 GenerateTexture(Span<byte>, Vector2i)`——与 LoomGUI"核心产数据、后端上传"边界天然契合。
- **字形几何**：per-glyph quad 4 顶点 6 索引，颜色烤顶点（`MeshUtilities.cpp:15-49`）。
- **FontEffect**：每 effect 一个额外 atlas layer + CPU 后处理。outline=膨胀卷积（`FontEffectOutline.cpp:26-70`）、blur=可分离高斯（`FontEffectBlur.cpp:33-87`）、glow=膨胀+高斯（`FontEffectGlow.cpp:97-117`）、shadow=clone base 只偏移（`FontEffectShadow.cpp:19-31`）。
- **text-decoration**：纯 quad 线条不进 atlas（`ElementText.cpp:545-567`）。
- **inline 混排（对应诉求①）**：完整 IFC——`InlineContainer` 维护 line box 栈（`InlineContainer.h:20-103`）、`LineBox::AddBox` 产 fragment + `SplitLine` 拆行复制（`LineBox.cpp:21-180`）。**重量级子系统**，佐证 LoomGUI"混排编译期报错、只走 flex"是主动省掉这块的合理决策。

> **RmlUi 最大短板（别照抄）**：新字形触发**全 atlas 重建**——`GetOrAppendGlyph` 缺字置 `is_layers_dirty`（`FontFaceHandleDefault.cpp:363`），下次全量 `GenerateLayer` + `texture_layout={}` 清空重来（`FontFaceLayer.cpp:20-28`，源码自己 `@performance` 注释）。对 CJK 逐字出现是 O(N²)。

### 1.4 Rust 选型：方案 A（最小侵入，零版本冲突）

**保留 `ttf-parser 0.20`（测量）+ 用它 `outline_glyph(gid, &mut OutlineBuilder)` 取轮廓 + 新增 `ab_glyph_rasterizer`（纯覆盖率光栅，不依赖字体解析）填灰度 + `etagere`（货架分配器）打图集。**

- `ab_glyph_rasterizer 0.1`（月下载 ~800万）纯算法：喂贝塞尔段 → `for_each_pixel(|idx,alpha|)` 出灰度，**不依赖 ttf-parser**——一份 ttf-parser 0.20 同时服务测量和轮廓，零版本冲突、不必 bump。
- `etagere 0.3`（glyphon 同款）：增量 `allocate` 不 repack，满了走多页图集（新纹理页，UV 不失效）——**正好避开 RmlUi 全 atlas 重建短板**。
- 灰度 R8 纹理，Unity 后端只多上传一张 R8 图，UV = rect/atlas_size。
- **不推荐 cosmic-text/swash**：会吞掉 LoomGUI 自己的 text/layout 分层 + 离开 ttf-parser 生态。仅当明确要复杂 shaping+RTL 才值得。
- **CJK 容量**：3000-4000 常驻字形 @48px ≈ 7.5-10M px²，一张 4096² R8（16MB）从容装下，超出上多页。**可行**。
- **fallback**：要 CJK 缺字回退用 `fontdb` 枚举系统字体 + 自写"对每 face 调 glyph_index 看是否 Some"的回退逻辑，不吞 cosmic-text。
- 依赖钉版本：`ab_glyph_rasterizer` 钉 0.1、`etagere` 钉 0.3、`ttf-parser` 维持 0.20。

### 1.5 代价与风险

| 项 | 代价/收益 |
|---|---|
| CJK 大字符集 | 可行（4096² R8），但必须增量货架 + 多页图集，绝不学 RmlUi 全量重建 |
| 动态加字形 | 核心要维护"哪些码点已在 atlas"跨帧可变状态——打破当前 measure 无副作用纯函数性，须放 Stage 而非 measure |
| 坑 119（缺字 advance） | **搬后根治**：核心自己光栅，缺字画 replacement 方框、advance 自定，不再猜 Unity fallback 1em。`layout.rs:220-229` hack 可删 |
| 坑 113（atlas rebuild 时序） | **搬后消失**：无 Unity textureRebuilt 异步 rebuild，`MirrorPool.cs:91-94,158-162` 补偿逻辑全删 |
| FFI 契约 | blob VERSION 从 9 升级；text_arena 从 `{codepoint,x,pen_y}` 改带 per-glyph UV+quad（或直接走 mesh_arena 像普通 mesh）；新增 atlas 纹理传输通道（R8+尺寸+脏标记）；`payload_hash` 改 hash UV/几何；csbindgen 镜像同步（坑 35） |
| 两后端 | Unity：删 TextRasterizer 全部 Unity Font/RequestChars/GetCharacterInfo，改"接收核心 atlas R8 + 现成 UV quad"，text 退化成普通贴图 mesh（**能合批**）。Godot 同理。**净简化**——后端从光栅化器降为贴图上传器 |
| 彩色 emoji | ab_glyph 只出灰度，不处理 COLR/CBDT。v1 目标（亚洲首发）emoji→tofu 本就砍了，可接受 |
| shaping/kerning | ab_glyph 不做 shaping，但搬后 quad=真实 bbox，**可安全开 ttf kern 表**（现因 Unity padded quad 禁用）。复杂 shaping(rustybuzz) 仍砍 |

### 1.6 分阶段（v1.6 内部）

- **阶段 a**：core 加灰度光栅 + etagere 单页 atlas，先支持 ASCII+已知字集，产 UV，FFI 加 atlas 通道，Unity 改贴图上传。跑通基础自绘（此时应见 text 能合批、坑 113 消失）。
- **阶段 b**：增量货架 + 多页图集 + CJK 按需加字形 + fallback 链（fontdb）。CJK 生产可用关键，最易踩性能坑。
- **阶段 c**：FontEffect 描边/发光/blur（CPU 卷积，照 RmlUi GetGlyphMetrics/GenerateGlyphTexture 范式，但 atlas 增量而非全重建）。可延到 v1.8 文字效果。

**依赖它先落地的效果**：仅描边/发光/blur。阴影/下划线/渐变字体不依赖它（v1.8 纯几何先交付）。

## 2. HTML/CSS 视觉效果

### 2.1 判断"能不能抄"的根本

RmlUi 效果分两条路：
- **Decorator**（gradient/shader/nine-patch/tiled）：多数 compile 一个 GPU shader（`DecoratorGradient.cpp:252 CompileShader("linear-gradient")`）。**唯一例外** 2 色直线渐变 `DecoratorStraightGradient` 逐顶点插值 `RoundedLerp`（`DecoratorGradient.cpp:158/166`），不 compile shader。
- **Filter/Effect**（filter/backdrop/box-shadow/mask）：走 `RenderInterface` 高级能力 `PushLayer/CompositeLayers/SaveLayerAsTexture/RenderToClipMask`（`RenderInterface.h:98-116`）= render-to-texture + 图层合成 + stencil。

**LoomGUI 范式完全不同**：核心产 SOA blob，Unity 后端只有一个 Unlit shader + keyword 变体（`LoomGUI-Unlit.shader`），**无 RT / 图层合成 / stencil**，裁剪是单矩形 `_ClipBox` uniform。所以：**RmlUi 假设后端有完整 GPU RT/stencil/合成能力，而这正是 LoomGUI 刻意没有的。**

### 2.2 效果清单大表

| 效果 | RmlUi（file:line） | LoomGUI 现状（file:line） | 归类 | 难度 | 优先级 |
|---|---|---|---|---|---|
| border-radius 圆角 | 固定圆心椭圆弧（`GeometryBackgroundBorder.cpp:37-69`） | **已领先**：自适应分段弧扇+椭圆半径+九宫格圆角共存（`mesh.rs:51-163,267`） | 已有 | — | 勿动 |
| **彩色边框 border** | 逐边描边几何 per-edge 色（`GeometryBackgroundBorder.cpp`） | **缺/死字段**：`border_width` 进 taffy 占布局，`border_color` render 层零引用（`resolved.rs:108`） | 纯几何可 port | 中 | **高** |
| outline | 盒外不占布局的描边 | 无 | 纯几何可 port | 中 | 中 |
| **2 色线性渐变** | 逐顶点 `RoundedLerp`（`DecoratorGradient.cpp:153-168`） | 无（仅纯色 quad `mesh.rs:22`） | 纯几何(顶点色) | 低-中 | **高** |
| 多 stop/角度 linear | GPU shader（`:252`） | 无 | 需后端 shader | 中-高 | 中 |
| radial/conic 渐变 | GPU shader（`:422/458/619`） | 无 | 需后端 shader | 高 | 低 |
| filter 色彩系(brightness等) | compile shader+CompositeLayers（`FilterBasic.cpp:17`） | **已领先**：4×5 色矩阵核心算好+单 shader mul（`color_filter.rs`，`shader:115-125`），program 3/4 透传 | 已有 | — | 勿动 |
| filter: sepia | 同上 shader | **占位残缺**：退化成 grayscale（`mapping.rs:116-119` TODO） | 纯算法补一矩阵 | 低 | 中 |
| filter: blur 高斯 | 离屏 RT 分离高斯（`FilterBlur.cpp:16`，`GL3:1331-1351`） | 无（无位图/邻域/RT） | 需后端 RT+shader | 很高 | 低 |
| backdrop-filter | PushLayer 读背景层（`ElementEffects.cpp:256-281`） | 无 | 需后端 RT | 很高 | 低 |
| box-shadow | 离屏 RT：PushLayer+spread+ClipMask+blur→SaveLayerAsTexture（`GeometryBoxShadow.cpp:157-243`） | 无 | 真实版需后端 RT；近似可纯几何 | 高/中 | 中 |
| 矩形裁剪 clip | scissor+可选 stencil | **已有**：AABB 交集（`batch.rs:23`）+`_ClipBox` step（`shader:128-131`） | 已有 | — | — |
| 圆角裁剪(overflow+radius) | RenderToClipMask 圆角 stencil | 无（仅矩形 AABB） | 需后端 shader(SDF 圆角，可扩 `_ClipBox`) | 中 | 中 |
| 任意形状 mask/clip-path | SaveLayerAsMaskImage（`ElementEffects.cpp:296-307`） | 无（clip-path 静默忽略 `fence_contract.rs:126`） | 需后端 RT/stencil | 很高 | 低 |
| blend-mode | RenderInterface 仅 Blend/Replace（`RenderInterface.h:15`） | 仅 Normal（`node.rs:24`），shader 有 `_SrcFactor/_DstFactor` 骨架（`shader:6-7`） | 简单混合可透传；multiply 需读 dst→RT | 中 | 低 |
| opacity | 烤进色/层 | **已领先**：`_Alpha` per-renderer MPB（`shader:127`），Header 级变更即生效 | 已有 | — | 勿动 |

### 2.3 值得借鉴的重点

- **彩色边框**（投产比最高）：`border_width` 已进 taffy border box（布局已留空间 `mapping.rs:384`），但 render 从不画描边，`border_color` 死字段。移植 = `render/mesh.rs` 新增 `border_ring(rect,radii,edge_widths,edge_colors)` 生成"外轮廓减内轮廓"环形三角带，独立 RenderNode（program=0 顶点色）叠背景上。**纯核心几何，零后端改动，无需 shader。**
- **2 色渐变**：`quad` 已 per-vertex color（当前 4 角同色 `mesh.rs:37`），横/竖 2 色 = 四角给两种颜色。核心纯几何，AI 可预测性好。多 stop/radial/conic 暂缓（需 shader + AI 预测力下降）。
- **box-shadow**：真实版需 RT+stencil+高斯（三样都没有），**务实替代**=生成比 rect 大 spread 的圆角 quad + 四周顶点 alpha 渐隐，纯核心几何出柔和投影观感。
- **sepia**：补一个棕褐 tint 矩阵即可（`mapping.rs:116`），纯算法极低成本。
- **圆角裁剪**：可不引入 stencil，shader 里把 `_ClipBox` 扩成圆角矩形 SDF + corner-radius uniform，`col.a *= sdf_rounded_box(...)`。单 shader 变体、复用现有 clip 通道，**值得做**。任意形状 mask 须 RT/stencil，成本极高，不建议。

### 2.4 后端要补什么

当前 Unity：单 Unlit + keyword 变体（ALPHA_MASK/BG_COMPOSITE/COLOR_FILTER/CLIPPED/OBJECT_MATRIX）、URP Transparent、`_ClipBox` 矩形裁、`_Alpha`、`color_matrix` 透传通道（blob 第 20 列）。无 RT/合成/stencil。
- **只需新 shader 变体 + 复用透传通道**（低-中成本）：多 stop/角度渐变（GRADIENT 变体 + blob 新列传 stop）、圆角裁剪（扩 `_ClipBox` SDF）、简单 blend-mode（已有 `_SrcFactor/_DstFactor`，核心扩 BlendMode 枚举 + blob blend 列，几乎零 shader 改）。
- **需整套离屏 RT + 合成**（高成本，落后期 v1.14+）：blur/backdrop/真 box-shadow/drop-shadow/mask。要引入 CommandBuffer + 临时 RT + 分离高斯 pass（可参考 RmlUi `RenderBlur`/`SetBlurWeights` 权重 `GL3:1331-1348`），且 SOA blob 单向数据流要新增"离屏组"概念。与"AI 可预测渲染结果"目标有张力（模糊结果 AI 难精确预测）。
- **复用点**：`program` 字段 + `color_matrix` 80 字节列是天然 per-node 效果参数扩展点，新效果优先挂这套而非新开 FFI 通道。

## 3. 九宫格 / 平铺装饰

### 3.1 纠错：RmlUi 九宫格也不平铺

RmlUi 的九宫格（`DecoratorNinePatch` + `DecoratorTiledBox`）**边块和中心块也是纯 stretch，不平铺**。真正的 repeat 只在单图 `DecoratorTiledImage`（唯一传 `register_fit_modes=true`，`DecoratorTiledImage.cpp:53`）；`DecoratorTiledBox` 不传（`DecoratorTiledBox.cpp:230-247`）→ 默认 `fit_mode=FILL`（stretch）。这与 LoomGUI 一样。

### 3.2 LoomGUI 现状（file:line）

- 参数唯一入口 `border-image-slice`（`mapping.rs:478-487`→`parse_slice:145-169`），存 `SliceInsets{top/right/bottom/left:f32}`（`resolved.rs:70-75`）。**无** repeat/width/outset/source（切片图=background-image 或 img 本身）。
- `nine_slice`（`mesh.rs:174-238`）：4×4/16 顶点/9 quad（索引照抄 fgui），四角不缩放、四边单轴拉伸、中心双轴拉伸。**全 stretch。**
- `nine_slice_rounded`（`mesh.rs:267-731`）：**LoomGUI 自研，fgui/RmlUi 都无**。四角发 1/4 圆弧三角扇几何镂空 + L 形补齐 + 边带拉伸；UV 分段映射 `u_of/v_of`（`mesh.rs:321-340`）避免右角区 UV 越界。有 point-in-triangle 扫描单测（`mesh.rs:1157-1227`）。

### 3.3 三处硬伤

- **A. `border-image-slice: %` 坏掉（真 bug）**：`parse_slice` 把 `25%` 存成 `0.25`（`mapping.rs:149-151`），渲染期无任何代码乘回源图边、直接当像素用（`mesh.rs:190,207`）→ 得到 0.25px 切片，九宫格坍缩。对照 border-radius 有 resolve 闭包（`render/mod.rs:187-192`），slice 没有。单测只锁 parse 存 0.25、没锁 render（`mapping/tests.rs:450-456`）——正是漏网原因。
- **B. 完全不支持平铺**：边/中只能 stretch，像素风/描边类素材（边需重复）无解。
- **C. 退化时四角固定 50/50 分且 UV 不同步**：`min(w*0.5)`（`mesh.rs:190`），左 10px 右 30px 时应 25/75 却强行 50/50；且 UV 侧 `tex_x` 仍用原始 slice（`:207`）——几何钳了 UV 没钳，四角纹理非等比压缩。

### 3.4 RmlUi 可借鉴（NinePatch.cpp）

- **退化按角比例分**（`:84-95`）：`surface_pos = tl/(tl+br)*dim`，比 LoomGUI 50/50 正确。
- **内缝取整防裂缝**（`:102-103` `.Round()`）。
- **edge 覆盖**（`:69-82`）：显示边厚度与纹理切片解耦。
- **HiDPI**（`:59` dp-ratio×display_scale）。
- 单图 8 档 fit 模式（FILL/CONTAIN/COVER/SCALE_NONE/SCALE_DOWN/REPEAT/REPEAT_X/REPEAT_Y，`DecoratorTiled.h:33-42`）——但 REPEAT 靠 GPU wrap，与 atlas sprite 冲突（RmlUi 自己 `DecoratorTiled.cpp:321` 报错拒绝）。

### 3.5 可借鉴清单

1. **修 `%` bug**（最高，非借鉴是修 bug）：render 期把比例乘 `src_w/src_h` resolve 成像素（对照 radii resolve 闭包）。纯核心，附一条 % 渲染单测。→ **补丁项，尽快修**
2. **退化按比例分 + UV 同步**（借 NinePatch.cpp:84-95）：`min(w*0.5)` 换 `left/(left+right)*w`，几何 UV 用同一钳制。纯核心几何。→ v1.8 附带
3. **接缝取整**（借 NinePatch.cpp:102-103）：内切片线 round。注意 y-flip，可能放后端更合适。→ v1.8 附带
4. **单图 background-repeat**（借 DecoratorTiledImage）：LoomGUI 用 atlas 不能靠 GPU wrap，只能几何层发 N 个 tile quad。需评估真实素材需求 + quad 数爆炸 vs dirty hash 交互。→ 有需求再排

**别借鉴**：RmlUi REPEAT-via-GPU-wrap（与 atlas 架构冲突，它自己都对 sprite 禁用）。

### 3.6 LoomGUI 已领先

- **九宫格+圆角几何共存**（`nine_slice_rounded`）：RmlUi 装饰器完全不做圆角。
- **atlas 原生兼容**：天然走 atlas UV 子区；RmlUi 平铺 REPEAT 反而与 atlas sprite 冲突需报错。
- **圆角自适应分段**（每 ~4px 弧长一段，消 fgui /8 分段毛刺）。

## 4. 动画 HTML/CSS

### 4.1 LoomGUI 现状（file:line）

- **10 个 ease**（`tween.rs:46-57`）：Linear + {Quad,Cubic,Back}×{In,Out,InOut}。Back overshoot 常量 1.70158（`:78`）。无 bounce/elastic/circular/exponential/sine/quartic/quintic。
- **单段 Tween**（`tween.rs:165-177`）：start[4]→end[4]+ease/delay/duration/elapsed/tag。无多关键帧、无逐段 ease、无 iteration/alternate。可动属性 6 个（`tween.rs:10-17` Opacity/Translate/Scale/Rotation/BgColor/TextColor）。
- **单矩阵覆盖 bug【已核实真实存在，是"丢失"非"畸变"】**：`NodeAnim.transform` 是单个 `Option<Affine2>`（`scene/node.rs:175`），三通道共用。`apply` 中各通道用 `from_translate/from_scale/from_rotate` 重建**整**矩阵（`tween.rs:298-300`），消费端整体替换（`scene/transform.rs:34-38`）。同节点 translate+scale 并行时，按 Vec 顺序后 apply 的整体覆写前者分量→**完全丢失**。单通道动画无此问题（当前 showcase 能正常演示的原因）。
- **单一时钟**：`stage.rs:637 tweens.update(dt)`，在 solve/compute 之前。悬空 NodeId 兜底 killed（`tween.rs:254`）。
- **CSS 现状**：transition 支持 opacity/color/bg-color（`mapping.rs:675-682`），`ease` 被简化成 QuadOut（非 CSS 标准 cubic-bezier），**明确不支持 transform**（`dynamic.rs:461` 注释）。@keyframes/animation **完全不支持**（@规则全拒 `parse/css.rs:58-63`）。transform 静态属性支持 translate/rotate/scale，skew/matrix/%/3D 静默跳过。
- **Controller** 已实现（v1.5，纯状态机，靠 CSS `[data-page]`+transition）；**Gear** 已砍（CSS 选择器替代）；**Transition 编排** 未实现（拆 v1.5-b）。

### 4.2 RmlUi 动画全景（file:line）

- **11 基础 ease + Callback + 混合方向**（`Tween.h:10`）：Back/Bounce/Circular/Cubic/Elastic/Exponential/Linear/Quadratic/Quartic/Quintic/Sine，支持 in 与 out 用不同类型。组合纯函数（`Tween.cpp:194-210`）：`out(t)=1-f(1-t)`、in_out 以 0.5 镜像。零 Element 依赖，**完全纯算法**。
- **@keyframes**（`StyleSheetParser.cpp:403-461`）：from→0/to→1/N%→0.01N，排序+property 并集。数据结构 `StyleSheetTypes.h:15-24`。
- **animation/transition 属性**（`PropertyParserAnimation.cpp`）：duration/tween/delay/iteration/alternate/paused；transition 私有扩展 `reverse_adjustment_factor`。
- **时间轴求值**（`ElementAnimation.cpp`，有状态非纯函数）：`GetInterpolationFactorAndKeys:694-743`（每段独立 ease）；`UpdateAndGetProperty:745-783`（dt **钳 0.1s** 帧尖峰防护 + iteration/alternate 状态机）。
- **transform 插值**（`TransformUtilities.cpp`，纯数学）：逐 primitive 配对 `TryConvertToMatchingGenericType:421-451`；配不上则 `Decompose:683-772`（css-transforms-2）+`QuaternionSlerp:19-42`。
- **颜色在近似 linear 空间插值**（`ElementAnimation.cpp:27-57` 平方进/开方出）——非 raw sRGB。

### 4.3 可借鉴清单

| # | 项 | RmlUi 源 | LoomGUI 现状 | port | 纯算法 | 难度 | 归属 |
|---|---|---|---|---|---|---|---|
| 1 | **修单矩阵覆盖 bug** | `TransformUtilities.cpp:453-538` | `tween.rs:298-300` 覆写 | `NodeAnim.transform` 拆 translate/scale/rotate 三通道各 lerp，消费端 T·R·S 合成。**2D 仅 4 自由度，无需 3D 分解+四元数** | 是 | 低-中 | **v1.10（最高优先，bug）** |
| 2 | **补 ease + 统一 In/Out 推导** | `Tween.cpp:14-87,194-210` | 10 硬编码分支 | 改 `Ease::evaluate` 为"基函数+方向"两维，存 11 基函数、In/Out/InOut 用 `1-f(1-t)` 生成。扩 `#[repr(u8)]` 枚举+FFI 镜像（坑 34/35） | 是 | 低 | v1.10 |
| 3 | **@keyframes 时间轴** | `StyleSheetParser.cpp:403-461`+`ElementAnimation.cpp:694-743` | 完全无 | (a)`AtRuleParser` 加 @keyframes 识别 (b)核心加 `eval_timeline(keys,t)` 纯函数 (c)Tween 扩关键帧序列或展开成 tween 链。剥离 RmlUi 相对长度百分比 Element 依赖 | 求值是 | 中-高 | v1.10（最大工作量） |
| 4 | **iteration/alternate** | `ElementAnimation.cpp:745-783` | 无 | Tween 加 num_iterations/-1/alternate/reverse_direction/current_iteration，update 越 duration 循环而非 kill。**dt 钳 0.1s 一并抄** | 是 | 中 | v1.10（依赖 #3 结构） |
| 5 | **transition 中断按进度压缩时长** | `Element.cpp:2673-2681` | 已有基础平滑(kill 保 mid-flight `stage.rs:670`)但新 tween 用完整 duration→反向慢半拍 | drain 时读 in-flight 进度，`f=1-(1-progress)*factor;duration*=f`。需 TweenManager 暴露查进度接口 | 公式是 | 低 | 第二梯队 |
| 6 | **颜色 linear 空间插值** | `ElementAnimation.cpp:27-57` | `tween.rs:301-302` sRGB 直接 lerp→中间色偏暗 | apply 里 color lerp 前后套 sRGB↔linear（近似平方/开方够用） | 是 | 低 | v1.10（与 #2 一起） |

### 4.4 别抄（架构冲突）

| 项 | 理由 |
|---|---|
| RmlUi ElementAnimation 分布式时钟 | 每动画自持时间（`ElementAnimation.h:33-36`），与 LoomGUI 单一 TweenManager 时钟冲突。**只抄 iteration/alternate 数学塞进单时钟 update** |
| Unit 驱动可动画判定 | 靠 `Property::Unit` 运行时决定（`ElementAnimation.cpp:638-648`），绑死 RmlUi Property 对象模型。LoomGUI 显式 `TweenProp` 枚举更清晰、AI 更可预测 |
| 相对长度百分比动画解析 | RmlUi 用 Element 宽高做基准（`:174-187`）。LoomGUI transform 只认 px + 布局/transform 分离，此依赖无意义 |
| 3D 矩阵分解+四元数(完整版) | RmlUi 为 3D（16 元素）。LoomGUI 是 2D Affine2，只需三通道各 lerp。除非未来跨类型 keyframe 过渡才需 2D 分解 |
| transition `all`+shorthand 展开 | 走全属性表，对 LoomGUI 极少可动画属性过重，维持白名单 |

## 5. FairyGUI 控件 / 能力 gap

> 批判视角贯穿——"fgui 有"不等于"LoomGUI 该做"。fgui 10 年沉淀里大量"编辑器驱动"能力（Gears/Relations/可视化组装）对 AI-DSL 定位是负担而非目标。

LoomGUI 当前：4 围栏标签 + 4 内部 NodeKind（`scene/node.rs:62-75` Container/Text/Image/Button）。虚拟列表是 driver 层拼的，核心零列表概念。

### 5.1 控件 gap 大表

| fgui 组件 | LoomGUI 对应 | 缺口 | 优先级 |
|---|---|---|---|
| GComponent/GImage/GScrollBar/GLabel/GGroup | div+Instantiate / img / 已有滚动条 / 标签组合 / flex | 无（组合即得或已有） | — |
| **GTextInput** | 无 | 完全缺失 | **上线必备（v1.9）** |
| **GRichTextField** | 无 | 图文混排/超链接/emoji | **上线必备（v1.7）** |
| **GList** | VirtualListDriver | loop/pagination/flow/selection/snap/pull-refresh 全缺 | **上线必备部分（v1.11）** |
| GProgressBar | div+width 手拼 | 无原生但可 CSS 拼 | 上线必备轻量（v1.12） |
| GSlider | 无 | 拖拽+值映射 | 上线常用（v1.12） |
| GButton Check/Radio/selected/音效 | button+伪类 | Check/Radio 可 Controller 拼 | 锦上添花 |
| GComboBox | 无 | 完全缺失 | 锦上添花（driver 拼） |
| GLoader fill/align/占位 | 部分(SetSrc) | fill 模式/错误占位/URL | 锦上添花 |
| GLoader3D(Spine/龙骨/GO) | NativeHost-lite | Spine/龙骨无 | 部分做（v1.13 NativeHost 完整） |
| GMovieClip 序列帧 | 无 | Unity Animator 可替代 | 低优先 |
| GTree | 无 | 游戏运行时 UI 极少用 | **可不做** |
| GGraph 椭圆/多边形 | 仅矩形+圆角 | 椭圆/多边形 mesh | 可不做（v1.16 几何扩展） |
| Window/PopupMenu/Tooltip | 无（driver 手拼叠加） | 无框架级 window/modal/popup | driver 层做（v1.13） |

### 5.2 能力维度 gap 要点

- **列表（最被低估）**：v1.4 只做了 driver 层**单列等高/不等高**（`LoomShowcaseDriver.cs:1044`）。fgui GList 的 flow 多列/pagination 翻页/loop 循环/snap 吸附/selection/pull-refresh 全没做。**flow 多列(背包网格)+pagination(翻页商店)+snap(banner) 是上线必备**；selection/箭头键导航更偏 PC 端，手游未必需要。多数能力可留 driver 层（符合"核心不认识列表"）。
- **动效**：fgui Transition 16 action + 31 ease + repeat/yoyo/path/回调。LoomGUI 差异化路径是 **CSS transition+@keyframes** 而非 fgui 编排对象——AI 更可预测，**别照搬 Transition 对象模型**。补 elastic/bounce ease + @keyframes（v1.10）即可，path 动画罕用。
- **文本**：富文本+输入是真刚需（v1.7/v1.9）。stroke/shadow 上线常用（v1.8）。**BMFont 可不做**（动态字体+SDF 已主流，LoomGUI 走 Unity 光栅化更省）。RTL/shaping 有意砍（亚洲首发）。
- **特效**：fgui 真滤镜也只有 Blur+Color 两个（drop-shadow/glow 是文本顶点生成）。LoomGUI ColorFilter 已对齐。blur/mask/blend 推后期正确（PNG 皮肤能补大部分）。
- **资源**：音效/异步/多语言是运营期能力（后期）。图集交 Unity 是合理简化。**LoomGUI 无任何音频概念**（grep 零命中）。
- **交互**：Window/Popup/DragDrop 都在 driver 层（C#）做，不进核心。**背包拖放(DragDrop 跨对象+drop 目标)优先级高于 window 管理**。

### 5.3 规划里可能遗漏、建议纳入

1. **pivot / transform-origin**：目前 transform 绕左上角（transform-origin 静默忽略 `fence_contract.rs:129`），旋转/缩放动画锚点受限，**真实体验短板**。建议升入围栏。
2. **列表 flow/pagination/snap**：v1.4 只做单列，多列/翻页/吸附未见后续号，游戏 UI 高频（背包网格=flow 多列）。→ v1.11 明确排期。
3. **DragDrop 跨对象拖放**：draggable 有了，但 onDrop 目标匹配（物品拖到装备栏）无框架支持。→ v1.13 driver 层 + drop-target 机制。
4. **`:nth-child` 结构选择器**：AI 写斑马纹/交替行样式很自然，围栏当前禁用。可评估升入围栏（权衡匹配器复杂度）。

### 5.4 LoomGUI 有意不对标（差异化非 gap，勿当缺口补）

| fgui | LoomGUI 替代 | 依据 |
|---|---|---|
| Relations(25 锚点) | flexbox | 差异化核心卖点 |
| Gears(状态→属性映射) | CSS `[data-page]`+set_style | 已砍，AI 更可预测 |
| 可视化编辑器+.fui 二进制 | HTML/CSS 文本 DSL | 核心目的 |
| Transition 编排对象 | CSS transition+@keyframes | 文本 DSL 一致性 |
| RTL/BiDi/复杂 shaping | 砍（亚洲首发） | — |
| skew/matrix transform | 围栏静默忽略 | AI 少写、破可预测性 |
| 图集打包 | 交 Unity SpriteAtlas | 不重复造轮子 |

### 5.5 RmlUi 独有、fgui 也缺、对 AI-DSL 有价值的

- **`:nth-child`/`:not()`/结构选择器**：AI 写交替样式自然，可评估升围栏。
- **@keyframes CSS 动画**：已排 v1.10。
- **多 stop/radial/conic 渐变**：AI 必写 `linear-gradient`，2 色直线渐变 v1.8 先做。
- **data-binding（MVC 双向绑定）**：**不建议做**——与 driver 层命令式 API 定位重叠，绑定模型 AI 不易预测。

## 6. v1.1 以来未记录的 defer / 技术债

考古 git log + 代码注释（TODO/ponytail/中文简化标记）+ pitfalls，捞出 roadmap §1.6/§5 未覆盖的净新增。11 项里 #11(CI) 已确认排除（`bad62c5` 已接线 game-ci EditMode runner，只差配 license secret，是运维前置非代码债）。净剩 10 项。

### 6.1 真 bug / 半成品（写了一半）

- **border-image-slice `%` 不 resolve**（`mapping.rs:145-168`+`render/mod.rs:232`+`mesh.rs:190`；单测只锁 parse `mapping/tests.rs:450-456`）：`%` 九宫格几乎无切片。围栏内合法 CSS 却坏。→ **补丁尽快修**
- **border_color 死字段**（`resolved.rs:108`/`mapping.rs:506`；render 全层零引用）：写了 border-color 无边，AI 可预测性反例。→ **v1.8 彩色边框自然修**（修法=补描边渲染）

### 6.2 有意简化 / 占位

- **sepia→grayscale**（`mapping.rs:116-120` ponytail 注释）：色相错。→ v1.8 附带（棕褐 tint 矩阵）
- **文本 baseline 简化占位**（`text/layout.rs:193`）：多样式行会暴露。→ v1.7 富文本附带（IFC 需精确 baseline）
- **scroll padding 边缘未处理**（`scroll.rs:74,627-628`）：带 padding 的 scroll 容器尺寸偏。→ v1.10 滚动附带
- **scroll content_size 变化 tween 补偿=snap**（`scroll.rs:610-612`）：滚动中内容变化跳变。→ v1.10 附带

### 6.3 技术债 / 架构妥协

- **drag 速度假定 60fps**（`input.rs:86`）：非 60fps fling 速度不准。→ v1.10 滚动重做接真实 dt
- **BMP 外 codepoint 不支持**（`TextRasterizer.cs:37`/`FrameBlob.cs:222` UTF-16 截断）：emoji/CJK 扩展字渲染错。→ v1.6 字体地基（核心自绘用 u32 codepoint 天然解，但 emoji COLR 仍砍）
- **render batch 跨 batch 优化留后**（`batch.rs:3-9`）：仅优化空间未取。→ 性能档，非阻塞
- **transition 不支持 transform 通道**（`dynamic.rs:461`）：已部分被 v1.10 transform 分解覆盖。→ v1.10 条目下注明"含 transition transform 通道补全"

### 6.4 已排除（已在 roadmap 或已修，非净新增）

- tween 单矩阵覆盖 bug → 已被 v1.10 覆盖
- commit-review 报的 5 个 NativeHost/Stage 问题 → 已被 `0490881` 修
- 富文本/软裁剪/字体 fallback/NativeHost 完整/shaping/IL2CPP/grid → §1.6 已记
- 坑 119 CJK advance → §1.6 fallback 链已记（v1.6 搬字体根治）

---

## 7. 别抄清单（架构冲突 / 已领先）

汇总各方向的"别抄"，防照抄倒退：

| 项 | 理由 |
|---|---|
| RmlUi 全 atlas 重建（新字形触发） | O(N²)，源码自认短板。LoomGUI 用 etagere 增量货架+多页图集 |
| filter blur/drop-shadow/backdrop | 需后端离屏 RT，LoomGUI 无。落后期 v1.14+ |
| 真实 box-shadow（RT+stencil+高斯） | 三样都没有，用几何近似（顶点 alpha 渐隐）替代 |
| 任意形状 mask/clip-path | 须 RT/stencil，成本极高，围栏已静默忽略 |
| RmlUi REPEAT-via-GPU-wrap 平铺 | 与 atlas 架构冲突，RmlUi 自己对 sprite 禁用 |
| RmlUi 分布式动画时钟 | 破 LoomGUI 单一 TweenManager 铁律，只抄 iteration/alternate 数学 |
| RmlUi 3D 矩阵分解+四元数(完整版) | LoomGUI 2D Affine2 只需三通道 lerp |
| RmlUi Euler 滚动模型 | 破"所有动画走 cubic_out"体感统一，改 ease 曲线即可 |
| data-binding(MVC) | 与 driver 命令式 API 重叠，AI 不易预测 |
| fgui Relations/Gears/可视化编辑/.fui | 差异化已替代（flex/CSS 选择器/HTML DSL） |
| fgui GTree/GComboBox/BMFont/GMovieClip | 游戏 UI 罕用 / 动态字体已主流 / Unity Animator 可替代 |
| fgui Transition 编排对象 | CSS transition+@keyframes 路径 AI 更可预测 |
| RmlUi skew/matrix/RTL/shaping | AI 少写破可预测性 / 亚洲首发砍 |

**LoomGUI 已领先（勿倒退）**：圆角几何（自适应分段+椭圆+九宫格圆角共存）、filter 色矩阵（预计算单 shader mul 不需 RT）、opacity（`_Alpha` MPB）、ChangeLevel 三档脏标记+mesh 合批、滚动回弹三段式（RmlUi 根本没 overscroll bounce，`Element.cpp:1019` 钳 `[0,max]`；要参考回弹找 iOS UITableView 不是 RmlUi）。

---

## 排期归属速查

| 版本 | 本文对应结论 |
|---|---|
| v1.6 核心自绘字体 | §1 全部（阶段 a/b） |
| v1.7 富文本 IFC | §1.2①③、§5.2 文本、§6.2 baseline |
| v1.8 文字效果+装饰视觉 | §1.6 阶段 c、§2.3、§3.5（退化/取整）、§6.1 border_color、§6.2 sepia |
| v1.9 TextInput/IME | §5.1 GTextInput |
| v1.10 动画+滚动手感 | §4.3（全）、§6.2 scroll、§6.3 drag/transition |
| v1.11 列表强化 | §5.2 列表、§5.3.2 |
| v1.12 轻量控件+pivot | §5.1 进度条/滑块、§5.3.1 pivot |
| v1.13 DragDrop+Window | §5.2 交互、§5.3.3 |
| v1.14+ 离屏 RT/高级滤镜/几何 | §2.4 后端、§2.2 blur/box-shadow |
| 补丁（不占号） | §3.5.1 border-image-slice %、§6.1 |
| 第二梯队（附带） | §4.3.5 中断平滑、§3.5.4 单图 repeat |
