# SDF 字体渲染 设计

> brainstorming 定稿（2026-07-11）。对标 TextMeshPro。
> 权威契约 = `loomgui_core/tests/fence_contract.rs`；本 spec 是设计意图 + 实现真相，不一致时测试赢（同项目围栏原则）。
> 对标依据 = TMP 源码（`temp/com.unity.textmeshpro/`，`TMP_SDF_SSD.cginc` + `TMP_Properties.cginc`）+ showcase page_image / page_text 取证。
> 取代当前 bitmap coverage AA（v1.6 起的核心自绘字体路径）。

## 1. 背景与目标

### 1.1 为什么做

当前文字渲染 = `ab_glyph_rasterizer` 出 coverage×255 → R8 atlas → shader 线性 alpha 合成。两个固有缺陷：

- **字细**：grayscale 线性 AA 视觉偏细（vs 浏览器 subpixel + gamma 合成）。同字体（`LXGWWenKai.ttf`）HTML 预览明显比 Unity 渲染粗。
- **缩放必糊**：coverage 是固定分辨率位图，transform scale/rotate（showcase 3.3）下 Bilinear 重采样必然糊。

pixel snap（`build_text_mesh` quad 原点 `.round()`，已做未 commit）修了 flex 居中亚像素糊，但字细与缩放糊是 coverage 模型本身的问题，不是对齐问题。

SDF（Signed Distance Field）在**屏幕空间重建边缘**：atlas 存"到字形边缘的有符号距离"而非 coverage，shader 用 `smoothstep` 在运行时重构锐利边缘。收益：任意缩放锐利 + 厚度可调（`_FaceDilate`，治字细）+ 无 gamma AA 问题 + outline/glow/underlay 在 shader 从 distance 直接算（廉价）。对标 TextMeshPro。

### 1.2 首要判据

字体品质是框架核心卖点——HTML/CSS DSL 让 AI 预测渲染结果，文字清晰度直接影响可预测性。SDF 是长期最优解；兴趣项目追求最优线路，与简洁实现不抵触（技术选型最优 + 实现守 YAGNI）。

### 1.3 非目标（推后）

- **MSDF（多通道）**：CJK 圆滑曲线用不上其锐角优势 + cdylib 不引 C++ 依赖。
- **R16 atlas**：R8 精度对 SDF 充分（TMP 同为 8bit/通道）；大字 banding 出现再升。
- **glow / 纯字形 blur**：Phase 2（showcase 无）。
- **per-size SDF 分片**：被单一 SDF 取代（单一 SDF 已含缩放容差）。

## 2. 关键决策（brainstorming 定稿，含理由）

### 2.1 单一 SDF（TMP 风格）

每字形只光栅一份固定 source 尺寸的 SDF，运行时 quad 按 `target_size / SOURCE_SIZE` 缩放。`GlyphKey (font_id, glyph_id, size_px, effect_sig)` → `(font_id, glyph_id)`。

- **vs per-size SDF**：per-size 最小改动（GlyphKey/quad/FFI 不变）但不省 atlas、缩放红利没拿到；单一 SDF 一步到位、atlas 大幅省、任意尺寸锐利。
- **理由**：长期内存优化必走到单一 SDF；兴趣项目倾向最优线路，接受较长验收周期。SDF 生成代码（8SSEDT）两方案共用，非死路。

### 2.2 自写 8SSEDT

`ab_glyph` coverage → 二值化（threshold 0.5）→ Felzenszwalb 二维 EDT 两遍扫描 → signed distance。

- **vs msdfgen crate**：MSDF 锐角优势对 LXGWWenKai（圆滑）用不上；msdfgen 是 C++ binding / 重 Rust crate，与 cdylib 轻依赖调性冲突。CJK 单通道 SDF 足够，TMP 也是单通道。
- 无新依赖，纯 Rust，两遍扫描。

### 2.3 R8 atlas（通道数不变）

distance 量化 8bit。TMP atlas 为 RGBA32（distance 在其一通道），同为 8bit/通道，证明对 SDF 充分。

- atlas `Vec<u8>` / FFI page bytes / Unity `TextureFormat.R8` **全不变**，零摩擦。
- distance 采样 `.r`（Unity R8 的 `.a` 恒 1，**非** TMP 的 `.a`）。
- R16 是清晰升级路径（atlas/FFI/Unity 纹理格式翻倍），banding 实证出现再升。

### 2.4 effect 搬 shader（`font_effect.rs` 重构）

`font_effect.rs` 的 bitmap 后处理（dilate/erode/gaussian_blur + 按 `effect_sig` 分槽）**废弃**，重构为 per-renderer uniform 打包（MPB）。`GlyphKey` 去 `effect_sig`（同一字形 distance 复用所有 effect）。

- **必然性**：单一 SDF 后 atlas 存 distance 不存 coverage，bitmap 域的 dilate/erode/blur 失去前提；effect 必须搬 shader。
- **红利**：shader 从 distance 算 outline/underlay/glow 比 bitmap 后处理更廉价（TMP 证明），且不再为 effect 分槽缓存。
- **映射**（覆盖 showcase 全部 effect）：
  - `Stroke`（`-webkit-text-stroke`）→ outline（face ± width 环形 + 描边色）
  - `Shadow`（`text-shadow`）→ underlay（偏移 uv 重采样 d + dilate + softness；blur 半径 → softness）
  - **多重 shadow**（逗号 ×N）→ shader 内 N 次重采样叠加（N ≤ 3，per-renderer uniform 数组）
  - `Glow` / `Blur` → Phase 2（showcase 无）

### 2.5 参数：SOURCE_SIZE = 48 / SPREAD = 12

- **TMP 参考**：`_GradientScale = atlasPadding + 1`（`TMP_PropertyDrawerUtilities.cs:176`）= spread；param.y 系数 1.3333（=4/3，`SSD.cginc:73`）。
- **SDF 硬约束**：spread ≥ 最大 effect 宽度（spread 之外 distance 饱和 → effect 硬切）。
- showcase C1 `"0 0 12px #5fb2c4"` → blur 12px → spread ≥ 12。TMP 典型 spread 6 是因其 effect 多为细 outline + 小 shadow；我们有 blur 12。
- **SOURCE 48**：TMP 中档，CJK 笔画清晰，放大到 72+ 仍锐利（SDF 容差）。
- **SPREAD 12**：覆盖 blur 12 + stroke 2 + AA 过渡带。
- atlas 代价：CJK 字形 bitmap ≈ 48(bbox) + 2×12(spread) ≈ 72px²，4096² 约 3000 字形/页，常用字 1-2 页（远省于 per-size）。
- distance 精度：8bit over ±12px ≈ 0.094px，smoothstep 1px 过渡带约 10 级，充分。
- **起点值**，Phase 2 视家里验收调（大字锯齿 → 升 source；shadow 硬切 → 升 spread）。

## 3. 设计

### 3.1 数据流（改造后）

```
layout（不变：纯 face metrics，按 target size 排版；measure_text/measure_rich_text 零改）
 → build_text_mesh: atlas.ensure(font_id, glyph_id) → source SDF（去 size_px/effect_sig）
   quad = (source SDF 维度含 spread) × (target/source)；定位用 layout bearing（target）
 → shader: 采 distance(.r) + ddx/ddy 屏幕空间 scale + smoothstep + _FaceDilate
            outline/underlay 同一 distance 多阈值（effect 红利）
```

### 3.2 光栅层（`atlas.rs::rasterize_glyph`）

流程改造（光栅 scale 从 target size 改固定 `SOURCE_SIZE`；输出语义 coverage → distance；pad 从 1 扩到 SPREAD）：

1. `ab_glyph` Rasterizer 出 coverage（复用现 `OutlineToRaster` builder）。
2. 二值化（coverage threshold 0.5）→ inside/outside mask。
3. 8SSEDT 两遍扫描（Felzenszwalb 二维 EDT）→ 每像素 signed distance（像素单位，inside 正 / outside 负）。
4. 编码到 [0,1]：`0.5 + d / (2 * SPREAD)`，写入 R8。

- 固定 `SOURCE_SIZE` 光栅（不再按 target size）；bitmap = `bbox(source) + 2*SPREAD`。
- `GLYPH_PAD`（现 1）→ SPREAD（12）。
- `empty_or_tofu` 路径：空字形（空格）仍返空 bitmap；tofu 框（缺字占位）走同 SDF 编码（矩形 outline 的 distance）。
- `OutlineToRaster` 的 scale 用 `SOURCE_SIZE / units_per_em`（固定），非 target。

### 3.3 GlyphKey / atlas

- `GlyphKey` 字段精简为 `(font_id, glyph_id)`（去 `size_px`、去 `effect_sig`）。
- atlas 存 source SDF，每字形一份（所有 target size 共享）。
- `AtlasPage` / etagere shelf 分配 / dirty page / 多页溢出 / `ensure` 缓存命中逻辑**全不变**（只像素语义 coverage → distance）。
- `GlyphAtlas::register_effect` / `effects` 表废弃（effect 搬 shader，不再 bitmap 后处理分槽）。

### 3.4 layout / quad

- **layout 完全不变**。已核实 `measure_text` / `measure_rich_text` 是纯函数（`layout.rs:577` 注释明写"不光栅、不读 atlas"），所有度量来自 face metrics（`ascent/descent/line_gap`、`glyph_hor_advance`、`kern`、`glyph_bounding_box`），不依赖光栅化尺寸 → 单一 SDF 去 `size_px` 不破坏测量。`Glyph.bearing_x/y` 已是 target size 下的值。
- `build_text_mesh` 改：
  - `base_key` 去 `size_px` / `effect_sig`。
  - `scale = run.font_size / SOURCE_SIZE`。
  - quad 宽高 = `r.px_w * scale` × `r.px_h * scale`（`r.px_w/h` 现为 source SDF 维度含 SPREAD）。
  - 原点对齐 pad：`GLYPH_PAD`(1) → `SPREAD * scale`。
  - 定位用 layout 的 `g.bearing_x/y`（target size，不乘 scale）。
  - **pixel snap 保留**（`.round()`，屏幕像素对齐仍需要）。
- italic skew（`skew × quad 高`）/ bold 双绘（offset 0 + 1px）随 quad 几何自适应。

### 3.5 shader（`LoomGUI-Unlit.shader` ALPHA_MASK fragment）

对标 `TMP_SDF_SSD.cginc`：

```glsl
float d = _MainTex.r;                    // distance（.r，非 TMP 的 .a）
float scale = rsqrt(abs(ddx(uv).x*ddy(uv).y - ddy(uv).x*ddx(uv).y)) * gradient; // 屏幕自适应
float face = saturate((d - (0.5 - _FaceDilate*0.5)) * scale + 0.5);  // _FaceDilate 调厚
// outline: face ± outline_width 环形 + 描边色（_OutlineWidth / _OutlineColor）
// underlay: 偏移 uv 重采样 d + dilate + softness，画 face 下（多重 N≤3 for 循环）
```

- distance 采 `.r`（Unity R8 `.a` 恒 1）。
- `scale` 用 `ddx/ddy`（HLSL 标准导数，URP 可用）——这是"屏幕空间重建边缘、任意尺寸锐利"的核心机制。
- `gradient` 常量对标 TMP `_GradientScale`（= SPREAD + 1），param.y 系数 1.3333 照搬。
- `_FaceDilate` 治字细（正值调粗，对齐 HTML 视觉重量），per-renderer MPB。
- 新增 uniforms 走 per-renderer MPB（同 `_Alpha` / `_ObjM` 模式）：`_FaceDilate` / `_OutlineWidth` / `_OutlineColor` / `_UnderlayOffset` / `_UnderlayColor` / `_UnderlaySoftness` + 多重 underlay 参数（N≤3）。

### 3.6 effect DSL → uniform 打包（`font_effect.rs` 重构）

- `FontEffect` 枚举保留（DSL 解析层不变），但不再产 bitmap 后处理。
- `build_text_mesh` 把节点 `text_effects` 打包成 per-renderer uniform（MPB），不再为每个 effect 注册 `effect_sig` + atlas 分槽 + 独立 mesh layer（shadow_back / stroke_front 等 layer 机制废弃）。
- `text-shadow`（多值逗号）→ 多组 underlay uniform（N≤3）。
- `-webkit-text-stroke` → outline uniform。
- `font-effect: glow / blur` → Phase 2（解析保留，渲染忽略 + log 警告）。

### 3.7 FFI / 后端

- R8 atlas 不变（Unity `TextureFormat.R8` 不变，`LoomStage.cs:260`）。
- blob re-base 不变（`blob.rs:104-111`，mesh 顶点 Rust 侧已含 target 尺寸，FFI 透明）。
- `payload_hash` 采样不变（mesh_arena 仍 verts/uvs/colors；effect 不再产生独立 layer mesh）。
- 新增 SDF uniforms 走 MPB；shader program=1（ALPHA_MASK）fragment 改 + 新 Properties。

## 4. 测试策略

- **8SSEDT 单测**（`atlas.rs`）：distance 单调性（越靠近边缘 d → 0）、zero-crossing 落字形轮廓、inside d>0 / outside d<0、编码往返（d → [0,1] → d）误差可忽略。
- **build_text_mesh**：`target = source` 时 quad 同维度；`target = 2*source` 时 quad 翻倍 + pad 翻倍。
- **snapshot**：SDF atlas 像素分布（insta，distance 渐变形态）。
- **shader 视觉验收**（家里 PlayMode）：page_image 3.1/3.2/3.3 字细改善 + `_FaceDilate` 对齐 HTML；page_text C1 shadow（含多重）/ C2 stroke 不回归。
- **fence_contract 不受影响**（不改围栏标签/属性集）。
- 改代码后搜 `docs/` 是否引用了改动的 struct/字段（防漂移，CLAUDE.md）。

## 5. 分阶段

- **Phase 1**（完整覆盖 showcase，零回归）：光栅 SDF + `GlyphKey` 简化 + quad 缩放 + shader base（`_FaceDilate`）+ outline + underlay（含多重 N≤3）。公司 Rust 改 + .dll / 家里 PlayMode 验收。
- **Phase 2**：glow / 纯字形 blur + 多重 shadow 收尾 + SOURCE/SPREAD 精调。
- pixel snap（已做未 commit）在 SDF 后仍需要，保留。

## 6. 风险

- **8SSEDT 精度**：SOURCE/SPREAD 选值不当 → 边缘 artifact（source 太小笔画糊、spread 太大 distance 精度降）。
- **多重 shadow MPB 打包**：uniform 数组在 Unity MPB 麻烦，N 限 ≤3，可能需拆多个 SetVector。
- **ddx/ddy URP 可用性**：标准 HLSL 导数，低风险。
- **SDF 编码/解码约定一致**：atlas 编码（`0.5 + d/(2*SPREAD)`）↔ shader 解码 ↔ quad scale（target/SOURCE）三处须对齐，错一则位移/厚度/锐利度全错。
- **effect 搬 shader 后视觉对齐**：softness 替高斯 blur（shadow blur 12 → underlay softness），与 HTML 柔光投影可能有可感差异，验收时对齐。
- **两机工作流**：公司改 Rust + 重编 .dll，家里 Unity PlayMode 验收视觉（[[two-machine-workflow]]）。

## 附录 A：TMP SDF 取证（`temp/com.unity.textmeshpro/`）

- `_GradientScale = atlasPadding + 1`（`TMP_PropertyDrawerUtilities.cs:176`）——TMP 的 spread。
- param.y = `1.3333 * _GradientScale * (_Sharpness+1) / texelSize.z`（`SSD.cginc:73`）——distance → 屏幕换算。
- weight = `(WeightNormal/Bold + _FaceDilate) * ScaleRatioA * 0.5`，param.x = `0.5 - weight`（`SSD.cginc:45-46`）——face threshold + 字重/调厚。
- face = `saturate((d - param.x) * scale + 0.5)`（`SSD.cginc:105`）——smoothstep 的线性饱和版（1px 过渡带）。
- outline = face ± param.z 环形（`SSD.cginc:108-110`）。
- underlay = 偏移 uv 重采样 d + dilate + softness，画 face 下（`SSD.cginc:80-81,113-116`）。
- distance 存 `.a`（TMP RGBA32）；我们存 `.r`（Unity R8 `.a` 恒 1）。
- scale = `rsqrt(abs(ddx(uv)*ddy(uv) - ddy(uv)*ddx(uv))) * param.y`（`SSD.cginc:95`）——屏幕梯度自适应。
