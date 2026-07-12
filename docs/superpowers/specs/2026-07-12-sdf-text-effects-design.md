# SDF 文字效果（Plan 2）设计

> brainstorming 定稿（2026-07-12）。接续 `2026-07-11-sdf-font-rendering-design.md`（Plan 1 base SDF）。
> 对标 TextMeshPro：`temp/com.unity.textmeshpro/Editor Resources/Shaders/TMP_SDF_SSD.cginc`（fragment 核心）+ `TMP_Properties.cginc`（uniform 定义）。
> 权威契约 = `loomgui_core/tests/fence_contract.rs`；本 spec 是设计意图 + 实现真相，不一致时测试赢（同项目围栏原则）。

## 1. 背景

Plan 1（base SDF）已完成并 merge 回 main：8SSEDT 光栅、单一 SDF（`GlyphKey=(font_id,glyph_id)`）、quad 按 `target/SOURCE_SIZE` 缩放、shader ALPHA_MASK 重建 + `_FaceDilate`。字体消失 bug（R8 texture 默认 sRGB）已修（`LoomStage.cs` `linear=true`）。

Plan 1 Task 3 把 `font_effect.rs` 的 bitmap 后处理（dilate/erode/gaussian_blur）+ 文字 shadow/stroke/glow/blur 的多 layer mesh 产出**降级为 known regression**（DSL 解析保留 `FontEffect`，渲染忽略）。showcase `page_text` C1-C4 的文字效果因此不渲染。

**Plan 2 目标**：把这 4 类文字效果搬到 shader（单一 SDF 后 atlas 存 distance，bitmap 域后处理已废，effect 必须在 GPU 端从 distance 重建），恢复 C1-C4，不退化。spec §5（Plan 1）的"一次性迁移全部 4 effect 不分 phase"立场在此兑现。

showcase 实测精确集（`page_text.html`）：
- **C1 text-shadow**：硬边 `3px 3px #000`、柔光 `0 0 12px #5fb2c4`、多重 `2/4/6px` ×3
- **C2 -webkit-text-stroke**：`2px #000`、`2px #5fb2c4`
- **C3 font-effect:glow**：`glow(4 #5fb2c4)`、`glow(6 #ff6b6b)`（LoomGUI 私有 CSS）
- **C4 font-effect:blur**：`blur(2)`、`blur(4)`（LoomGUI 私有 CSS）

## 2. 关键决策（brainstorming 定稿，含理由 + 否决项）

### 2.1 单 quad 多 pass（对标 TMP，否决多 layer）

一个文字节点 = 一个 quad = 一次 draw，shader fragment 内同一 distance 顺序合成 face + outline + underlay×3 + glow（blur = face softness）。draw order（underlay 在 face 下、outline 覆盖边缘、glow 晕开 face 外）靠 fragment 内合成顺序，不靠 sort_key。

**对标取证**：`TMP_SDF_SSD.cginc` 的 `PixShader`（:90-132）就是单 fragment 内叠 face（:105）→ outline（:107-111）→ underlay（:113-116，偏移 uv 重采 d + `(1-faceColor.a)` 画 face 下），return 一个合成色。TMP 一字符 = 一 quad，所有 effect 是 material 级 uniform（一整段文字共享），等价 LoomGUI 的 per-renderer MPB。TMP 无 multi-pass / multi-layer。

**否决多 layer**（复用 Plan 1 残留的 `back/front_layers` + `propagate_*_sort_keys` 骨架）：① 逆 Plan 1 spec §3.6（layer 废弃）；② 多重 shadow ×3 在多 layer 下要 3 个 back draw，比 shader 内 for 循环更费 draw call；③ effect 参数要按 layer 节点分散打包，不如 per-renderer MPB 干净。

**超出 TMP 的两点（在单 pass 内扩展，不破坏结构）**：
- 多重 shadow（C1 ×3）：TMP underlay 单槽不支持多重；LoomGUI shader 内 `for N≤3` 偏移重采，uniform 数组。
- blur（C4）：TMP 无整字高斯 blur；LoomGUI 用 face softness 加大近似（SDF 固有，偏硬，验收接受）。

### 2.2 effect_block SOA 列（照 color_matrix 先例，否决 effect_arena）

每节点加一列定长 `effect_block`（含 outline + underlay×3 + glow + blur 槽位）。非文字节点填全 0（无 effect）。MirrorPool 读列 → MPB SetVector/SetFloat。effect 进 `header_hash`，effect 变 = Header 级（只更 MPB 不重建 mesh）。

**理由（范式分工）**：LoomGUI FFI 已有两种数据形态——变长 geometry 走 arena（mesh_arena），定长 per-node struct 走 SOA 列（`color_matrix`/`alpha`/`reuse_key`）。`color_matrix`（ColorFilter，每节点带，非 filter 填 0）就是"定长 per-node struct 列"的现成先例，effect_block 与之同构。effect 因 spec 定 N≤3 固定槽是**定长** per-node struct，归 SOA 列那类，不归 arena（arena 为变长设计）。effect_block 照抄 `color_matrix` 三件套（blob.rs 列 + FrameBlob.cs 访问器 + MirrorPool MPB），零新概念，与 `color_matrix` 模式一致（不会出现"两个 per-node struct 一个走列一个走 arena"的分裂）。

**否决 effect_arena**（独立 arena + effect_idx 列）：① 给定长数据上 arena 是错配；② `effect_idx` 不反映内容变化（idx 稳定时 shadow blur 12→8 检测不到，要么 idx 不稳定 arena 不可去重膨胀，要么 hash 跨 arena 采样破坏 per-node 自包含）；③ 引入第三个 FFI 打法 + 第二条 MirrorPool 解析路径。带宽优势（省非文字节点约百余字节）只在"移动端大场景 + 文字节点占比低"兑现，当前为未必到来的场景预付长期复杂度，违反 YAGNI。真遇到那天，列→arena 迁移成本主要在 dirty hash（可承受）。

### 2.3 去 flags，参数隐含启用

`effect_block` 不含 bitfield flags。每个 effect 的启用由其参数隐含：`_OutlineWidth>0` / `_UnderlayColor.a>0` / `_GlowColor.a>0` / `_BlurWidth>0` = 启用。CSS 不会声明 `width:0` 或 `alpha:0` 的 effect（无意义），所以参数为 0 = 该 effect 不启用成立。

**理由**：省 bitfield 解码（HLSL `asuint` 位运算跨平台顾虑）+ effect_block 少一个字段。shader 内 `if (_OutlineWidth > 0.001)` 判断，可读性高。

### 2.4 `_FaceDilate` 保持 material 默认（不进 effect_block）

Plan 1 验收 `_FaceDilate=0` 已对齐 HTML（字消失修复后）。它是全局"字细补偿"，无 per-node 需求（showcase 各文字字号同区）。YAGNI——若未来要 per-node 字重补偿（如 bold 不同 dilate）再扩进 effect_block。

### 2.5 `BOX_SHADOW_FLAG` 共用保留（div box-shadow）

`BOX_SHADOW_FLAG` / `shadow_pairs` / `propagate_box_shadow_sort_keys` 是 **div box-shadow**（v1.8 装饰，Container/Button，`program=0`）和文字 back layer 共用的机制（`render/mod.rs:755-799` div box-shadow 产 `node_id | BOX_SHADOW_FLAG`）。Plan 2 删文字 back layer 后，这套机制**保留给 div box-shadow**——不能无脑删（grep 取证救了一次）。

只删文字专用的 `TEXT_STROKE_FRONT_FLAG` / `stroke_pairs` / `propagate_text_stroke_sort_keys`（div 不用 stroke front）。

### 2.6 effect 进 header_hash（effect 动画廉价）

effect 是 per-frame 可变属性（非几何），进 `header_hash`（`dirty.rs`），不进 `payload_hash`。effect 变 → ChangeLevel::Header（只更 MPB，不重建 mesh）。文字 effect 动画（如 transition text-shadow）与 `_Alpha`/transform 动画同档廉价——这是单 quad 多 pass 模型的红利。

## 3. 设计

### 3.1 数据流

```
ResolvedStyle.text_effects（DSL 解析已有，INHERITED）
  → build_text_mesh 打包成定长 effect_block（新）
  → RenderNode.effect 字段 carry
  → blob.rs：SOA 加 effect_block 列（v10→v11）
  → FrameBlob.cs：EffectBlock(i) 访问器
  → MirrorPool：读列 → MPB SetVector/SetFloat（照 _CF 模式）
  → shader ALPHA_MASK uniform → fragment 单 pass 合成 face+outline+underlay×3+glow（blur=face softness）
```

### 3.2 effect_block 字段布局

定长，纯 f32（便于 SOA 列字节布局 + MPB SetVector 拆 4 打包），无 flags。槽位对标 `TMP_Properties.cginc`：

| 槽位 | 字段 | 对标 TMP |
|---|---|---|
| outline | width + color RGBA | `_OutlineWidth/Color` |
| underlay[3] | 每槽 offset_x/y + softness（dilate 并进）+ color RGBA | `_UnderlayOffset*/Softness/Color` |
| glow | power + color RGBA | `_GlowPower/Color` |
| blur | width（face softness 加大量）| TMP 无（LoomGUI 私有近似）|

shadow `blur` → underlay `softness`、`ox/oy` → `offset_x/y`。多重 shadow 填 underlay[0..N]（N≤3，超 3 丢弃 + log）。确切字段数 / 字节数以 `EffectBlock` 定义（core）+ fence_contract 列数断言为准，不在此写死数字防漂移。

### 3.3 core 改动 + 清旧 layer

**RenderNode 加 effect**（`render/node.rs`）：新增 `EffectBlock` struct + `RenderNode.effect: EffectBlock`（`Default` = 全 0 = 纯 face）。非文字节点 effect=default。

**build_text_mesh 重新启用 `_text_effects`**（`render/mod.rs`）：当前 `_text_effects` 标 `_` 未用（Plan 1 Task 3）。改为把 `&[FontEffect]` 打包成 `EffectBlock`（§3.2 映射），`TextMeshes` 加 `effect: EffectBlock` 字段 carry。

**emit_text_node**：base + 跨页子页 RenderNode 都带 `effect = TextMeshes.effect`（同一文字节点所有页共享 effect 配置）。渐变字 bg quad + 装饰线 mesh 路径不动。

**清旧 layer（精确范围）**：

| 删 | 保留 |
|---|---|
| `TextMeshes.back_layers/front_layers` 字段 | `BOX_SHADOW_FLAG`（div box-shadow 共用）|
| emit_text_node 的 back/front push 循环 | `shadow_pairs` + `propagate_box_shadow_sort_keys`（div box-shadow）|
| `stroke_pairs` 参数 + `propagate_text_stroke_sort_keys` + `TEXT_STROKE_FRONT_FLAG` | `synth_text_node_id`（跨页子页 + 行内图）|
| emit_text_node 的 `shadow_pairs`/`stroke_pairs` 参数 | `propagate_text_sub_page_sort_keys`、`propagate_inline_image_sort_keys` |
| `merge.rs` 同步去掉 `TEXT_STROKE_FRONT_FLAG` 引用 | |

### 3.4 shader fragment（ALPHA_MASK 单 pass 合成）

现状 fragment（line 109-123）只算 `faceAlpha`。Plan 2 在同一 fragment 内叠加，合成顺序对标 TMP：underlay 画 face 下 → face → outline 覆盖边缘环 → glow 晕开 face 外。

```hlsl
float d = tex.r;
float scale = pxSize * (1.3333 * _GradientScale) / _MainTex_TexelSize.z;
float threshold = 0.5 - _FaceDilate * 0.5;
// blur 近似：软化 face 过渡带（SDF 无整字高斯 blur，偏硬，验收接受）
if (_BlurWidth > 0.001) scale /= 1.0 + _BlurWidth * scale;
float face = saturate((d - threshold) * scale + 0.5);

// underlay×3（对标 SSD.cginc:113-116：偏移 uv 重采 d，画 face 下）
float3 rgb = vcol.rgb; float a = face * vcol.a;
[unroll] for (i = 0; i < 3; i++) {
  if (_UnderlayColor[i].a > 0.001) {
    float du = tex2D(_MainTex, uv + _UnderlayOffset[i]).r;
    float ls = scale / (1.0 + _UnderlaySoftness[i] * scale);
    float um = saturate((du - threshold) * ls + 0.5);
    float ua = _UnderlayColor[i].a * um;
    rgb = lerp(rgb, _UnderlayColor[i].rgb, ua * (1.0 - a));
    a += ua * (1.0 - a);
  }
}
// outline（对标 SSD.cginc:107-111：face ± width 环形）
if (_OutlineWidth > 0.001) {
  float outer = saturate((d - threshold + _OutlineWidth) * scale + 0.5);
  float inner = saturate((d - threshold - _OutlineWidth) * scale + 0.5);
  rgb = lerp(rgb, _OutlineColor.rgb, saturate((outer - inner) * _OutlineColor.a));
}
// glow（face 外 distance power 衰减，对标完整 TMP_SDF glow）
if (_GlowColor.a > 0.001) {
  float gm = 1.0 - saturate((d - threshold) * scale + 0.5);
  float ga = pow(gm, _GlowPower) * _GlowColor.a;
  rgb = lerp(rgb, _GlowColor.rgb, ga * (1.0 - a));
  a += ga * (1.0 - a);
}
col = half4(rgb, a);
```

新增 uniform（Properties + CBUFFER + MPB，per-renderer）：`_OutlineWidth/_OutlineColor`、`_UnderlayOffset[3]/_UnderlaySoftness[3]/_UnderlayColor[3]`、`_GlowPower/_GlowColor`、`_BlurWidth`。effect 开关走 uniform 参数阈值（§2.3），不走 shader keyword——LoomGUI 所有文字节点共用一个 `program=1` material，`multi_compile` keyword 是 material 级做不了 per-renderer。

shader 数学（shadow blur→softness 系数、glow power、blur softness）是起点值，家里验收精调（§5 风险）。

### 3.5 FFI 改动

照 `color_matrix` 三件套：

| 层 | 改动 |
|---|---|
| blob.rs | columns 加 `effect_block` 列（定长，字节数 = `EffectBlock` 序列化尺寸）；列数 +1；`VERSION` bump（v10→v11）。写出循环每节点 extend effect_block 字节（非文字写全 0）|
| RenderNode | 加 `effect: EffectBlock`（§3.3）|
| FrameBlob.cs | 加 `EffectBlock(i)` 访问器（照 `ColorMatrix(i)` 手写镜像）|
| MirrorPool.cs | `program==1` 节点读 effect_block → MPB SetVector/SetFloat（照 `_CF` 拆 Vector 模式）。非文字节点不设（material 默认全 0 = 纯 face）|
| dirty.rs | `header_hash` 加 effect 采样；`payload_hash` 不动 |
| fence_contract / size_of 断言 | 列数 / blob 版本断言同步 |

## 4. 测试 + 验收

**core 单测**（验数据通路，非视觉）：
- `EffectBlock` DSL→槽位映射（Shadow→underlay、Stroke→outline、Glow→glow、Blur→blur；多重 ≤3；超 3 丢弃）
- `dirty`：effect 变 → `header_hash` 变；`payload_hash` 不含 effect；全 0 hash 稳定
- `blob`：effect_block 列写出（非文字全 0、文字有值）；列数 + VERSION 断言
- 清旧 layer：`build_text_mesh` 只产 base（+渐变字/装饰线），无 `TEXT_STROKE_FRONT_FLAG` 合成节点

**shader 视觉验收（家里 PlayMode，两机工作流）**——shader 不可单测，核心验收在这（CLAUDE.md SDD 教训：别只靠单测绿就 merge）：
- C1-C4 全部例对齐 HTML 预览
- 已知 trade-off：blur 偏硬（SDF 固有，接受）；shadow blur→softness 系数精调

**不回归验收**：page_image 3.1-3.3 字细（Plan 1 成果）、渐变字、装饰线、**div box-shadow**（BOX_SHADOW_FLAG 机制保留）、无 effect 纯文字（effect_block 全 0 = 纯 face）。

**诊断**：`dump_showcase_text.rs` 量化 core effect_block 数据，确认 core 对后再查 Unity 侧（双机调试方法论）。

**fence_contract 门**：不改围栏标签/属性集，`cargo test -p loomgui_core --test fence_contract` 须绿。

## 5. 风险

- **shader 数学精调**：shadow blur→softness、glow power、blur softness 是起点值，家里 PlayMode 验收精调（对标 HTML 视觉重量）。blur 偏硬是 SDF 固有，与 v1.8 bitmap 高斯 blur 有可感差异，验收接受。
- **多重 shadow MPB uniform 数组**：`_Underlay[3]` 数组在 Unity MPB 用 `SetVectorArray` 或拆 3 个 `SetVector`，N 限 ≤3。
- **effect 编码/解码一致性**：core `EffectBlock` 打包 ↔ blob 序列化 ↔ FrameBlob.cs 解析 ↔ MirrorPool MPB ↔ shader uniform 五处须对齐，错一则 effect 错位/丢字段。
- **清旧 layer 边界**：`BOX_SHADOW_FLAG` 是 div box-shadow 共用，删文字 layer 时不能误删（§2.5）。
- **两机工作流**：公司机改 Rust + 重编 .dll；shader + C# 改动家里机拉代码即生效。Rust 改后必须重编 + commit .dll（CLAUDE.md：Rust 改动后必重编 .dll）。

## 6. 附录：TMP SDF 取证（`temp/com.unity.textmeshpro/`）

- `TMP_SDF_SSD.cginc:90-132` `PixShader`：单 fragment 合成 face + outline + underlay，return 一个色——证明单 pass 多 effect。
- :105 face = `saturate((d - param.x) * scale + 0.5)`。
- :107-111 outline = face ± param.z 环形（lerp outlineColor/face）。
- :113-116 underlay = 偏移 uv（`texcoord2`）重采 d + `(1-faceColor.a)` 画 face 下。
- `TMP_Properties.cginc:40-44` underlay 单槽（`_UnderlayColor/OffsetX/OffsetY/Dilate/Softness`，无数组）——TMP 不支持多重 shadow，LoomGUI 扩展为 [3]。
- :46-50 glow 单组（`_GlowColor/Offset/Outer/Inner/Power`）。
- effect 全是 material 级 uniform（per-object 共享）= LoomGUI per-renderer MPB。
- distance 存 `.a`（TMP RGBA32）；LoomGUI 存 `.r`（Unity R8 `.a` 恒 1）。
