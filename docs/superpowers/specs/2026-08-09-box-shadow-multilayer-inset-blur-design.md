# box-shadow 多层 + inset + 真 blur（SDF，不碰 RT）

> 视觉束第一棒。把 showcase 当前**破损**的 box-shadow（多层被吃成乱码、`inset` 完全没处理、blur 静默忽略）做成符合 CSS 规范的全语义：多层、`inset`、gaussian blur、spread、offset、spaced rgba。
>
> **关键决策**：blur 走 **SDF fragment shader**（圆角矩形解析 SDF + 高斯边衰减），**不引入离屏 RT 基建**。理由见 §3——box-shadow 的 blur 目标永远是圆角矩形，可解析求解，比 fgui/RmlUi 的通用 RT 方案更优（真高斯质量 / 不破批合 / 零 RT / 零额外内存）。
>
> 关联：roadmap §4 视觉束；参考调研 fgui（`temp/FairyGUI-unity/`）+ RmlUi（`temp/RmlUi/`），只读。

---

## 1. 背景：为什么是这个

### 1.1 缺口证据（showcase 驱动）

showcase 视觉 CSS 用量统计：

| 特性 | 用量 | 当前状态 |
|---|---|---|
| **box-shadow** | **15 处，几乎全是多层/inset** | 🔴 **破损**（见 1.2） |
| border-radius | 68 处 | ✅ 已做（Phase1 Spec-P2） |
| filter (brightness/grayscale/hue-rotate) | 9 处 | ✅ 已支持（`parse_filter`→4×5 矩阵） |
| transform 静态声明 | 17 处（多为 @keyframes 内） | ✅ 已支持（`style.transform.matrix` 喂 world_matrix） |
| linear-gradient | 2 处 | 🟡 `to right` 2 色已支持；gradient-text 未支持（本棒不碰） |
| radial-gradient | 2 处 | 🔴 静默丢弃（本棒不碰） |

**box-shadow 是唯一的大坑**，且不是"缺失"是"破损"：showcase 每张卡/按钮依赖的 `inset 0 0 0 1px rgba(...)` 内描边高光 + 外层投影，当前要么解析出垃圾黑影、要么没效果，所以卡片看着"平"。修好它，整个 showcase 视觉层级立起来。

### 1.2 当前破损根因（core `apply_decl "box-shadow"`，mapping.rs:1306）

`split_whitespace()` 三个病同源：
1. **逗号多层被吃成乱码**：`box-shadow:0 0 0 1px rgba(...), 0 8px 26px rgba(...)` 只出第一层，第二层静默丢。
2. **`inset` 关键字当 ox 解析**：`inset 0 0 0 1px ...` → `ox=parse_number("inset")=0`，解析走错路出垃圾/默认黑影。
3. **spaced `rgba(95, 180, 212, 0.5)` 被空格切碎**：`split_whitespace` 把括号内参数拆成多 token，color 永远解析失败 → 默认 `[0,0,0,0.3]`。

且 fence 对 box-shadow **零自有校验**（`css_resolve.rs:137` 的 `match spec.parser` 对 `BoxShadow` 走 `_ => {}` 兜底，委托 `apply_decl`），而 apply_decl 一路 `unwrap_or` 默认值**永远返回 true** → fence 把多层/inset 全当合法放行。所以"fence 放行 + core 解析坏"同时成立，showcase 打包不报错但渲染错。

---

## 2. 范围

### 2.1 做

CSS box-shadow 全语义：多层（逗号）、`inset`、blur（SDF gaussian）、spread、offset、`rgba()/rgb()/#hex/named` color（含 spaced rgba）。

### 2.2 不做（ponytail / 另一笔）

- **`filter:blur(任意内容)`**：要离屏 RT 基建，roadmap v1.14，独立一笔。本棒的 SDF 方案只 blur 圆角矩形（= box-shadow 全部场景），覆盖不到任意内容 blur。
- **box-shadow blur 的像素级浏览器 diff**：护城河是布局可预测，不是滤镜像素。`exp(-d²/2σ²)` 视觉≈真高斯即可。
- **gradient-text（`background-clip:text`）/ radial-gradient**：独立缺口，本棒不碰。

### 2.3 AI 先验不破

box-shadow 是纯标准 CSS（已在围栏 `CssValueParser::BoxShadow`，css.rs:421）。本棒改动**全部内部**（BoxShadow struct 字段、Vec、pkg 格式、shader/blob 列），零用户可见 HTML/CSS 语法新增、零新标签、零新属性。修 box-shadow 是**强化** AI 先验：现在解析破损（AI 按标准 CSS 写的渲染错），做成 CSS 规范后 AI 的标准 CSS 先验真能预测渲染。

---

## 3. 参考调研与方案决策

### 3.1 参考（fgui + RmlUi，只读，AGENTS.md 硬规则）

| 参考 | blur 怎么做 | box-shadow | 关键点 |
|---|---|---|---|
| **fgui** | RT 离屏 + **4-tap cone（假高斯，非可分离）**，8× 降采样 2 轮 | **完全没有**（只 TextField 硬偏移文字阴影，无 blur/dilation） | 通用 image filter 架构，filter=独立 RT=破批合；box-shadow 给不了借鉴 |
| **RmlUi** | RT 离屏 + **真可分离高斯**（H+V 两 pass + 大 σ mip 降采样），`σ=0.5×blur_radius` | **全套**（outer+inset+多层+blur） | 几何=实心圆角矩形 + **clip-mask 区分 inset/outer**；多层逆序；整组冻成一张 texture 画一个 quad；重（每独特 shadow 签名一张离屏 texture + 后端实现 `CompileFilter("blur")`） |

**两者都走 RT**，因为都是通用渲染器，把 blur 当"对任意图像的后处理"。

### 3.2 关键认知：box-shadow blur 不需要通用图像 blur

圆角矩形的高斯模糊 = 对圆角矩形 indicator 函数做高斯卷积。**圆角矩形 SDF 解析可算**，高斯衰减 `alpha ≈ exp(-d²/2σ²)` 是 SDF 软阴影标准式（凸形圆角矩形上与真高斯视觉无差）。**而这套 SDF 数学已在我们 shader 里**（`CLIPPED_ROUNDED`，`LoomGUI-Unlit.shader:219-223`：`sdf = length(max(q,0)) + min(max(q.x,q.y),0) - r`）。

### 3.3 方案对比（确认 A）

- **A（采纳）：SDF 高斯，不碰 RT** —— 真（高质量）高斯 / 不破批合 / 零 RT / 零额外内存 / 大 blur 半径零额外成本（per-fragment 数学，不多采样）。局限：只 blur 圆角矩形（= box-shadow 全场景）。
- B（否决）：RT 可分离高斯（RmlUi 式）—— 通用但破批合 / 内存 / 要造 RT 基建，对只 blur 圆角矩形的 box-shadow 杀鸡用牛刀。
- C（否决）：假 blur（堆叠偏移 alpha quad）—— 大半径难看、N× 几何，RmlUi 明确弃用。

**A 超出两个参考做法（它们走 RT），但 SDF 高斯是 UI/字体渲染成熟技术，数学已在我们 shader。**"借鉴 fgui、对照 RmlUi"≠照抄——LoomGUI 差异化（紧批合 + 自控 shader + box-shadow 必为圆角矩形）正是 A 优于 B 的根因。

从 RmlUi 吸收的 CSS 正确性细节（与 blur 方案无关，A/B 都用）：多层逆序绘制、outer 用 border-box+spread / inset 用 padding-box−spread、blur 仅在 `radius≥0.5` 启用、`σ=0.5×blur_radius`。

---

## 4. CSS 语义契约（实现必须遵守）

1. **语法**：`none | [inset?] <ox> <oy> [blur]? [spread]? <color>` 逗号分隔多层。`inset`/color 可在任意位置；数值按位置语义（第1=ox 第2=oy 第3=blur 第4=spread）。对齐 RmlUi `PropertyParserBoxShadow`。
2. **层叠顺序**：**先列出画在最上**。
3. **绘制位置**（决定 sort_key）：
   - **outer**：画在元素**下层**（bg 之后盖住中心，只露外扩）→ `BACK_LAYER`。
   - **inset**：画在 bg **之上**、content/子节点 **之下** → `FRONT_LAYER`。
4. **blur → σ**：仅 `blur_radius ≥ 0.5` 才启用 blur（σ=0 退化为硬边），`σ = 0.5 × blur_radius`。
5. **spread**：外扩/内缩 shadow 形状（outer 加、inset 减），每角 radius 随 spread 同步。
6. **裁剪**：shadow 继承主节点 `mask_context`（overflow 裁剪，outer/inset 同传播）。

---

## 5. 数据模型（core）

```rust
// crates/core/src/style/resolved.rs
pub struct BoxShadow {
    pub ox: f32, pub oy: f32,   // offset
    pub spread: f32,
    pub blur: f32,              // blur_radius（CSS 像素）；σ = blur/2 运行时算
    pub color: [f32; 4],
    pub inset: bool,
}

// ResolvedStyle.box_shadow:
pub box_shadow: Vec<BoxShadow>   // 原 Option<BoxShadow>；空 Vec = none；顺序 = CSS 源序
```

**pkg.bin v31 → v32**：`ResolvedStyle` bincode 形状变（`Option<BoxShadow>` → `Vec<BoxShadow>` + 新字段 `blur`/`inset`）。`asset/mod.rs` 的 `PKG_FORMAT_VERSION` 升 v32、`MIN=MAX=32` 弃 v31，加 bincode 形状稳定性测试。

---

## 6. 解析层（单一真相源 = core，fence 委托）

### 6.1 边界：box-shadow 解析只活一份（core），fence 不自造校验器

沿用现状 `_ => {}` 兜底委托给 `apply_decl`（css_resolve.rs:137）。区别：重写后 core 对**真非法输入返回 `false`** → fence 的 fallthrough 自动报 `FenceBadCssValue`（css_resolve.rs:190）。零双份白名单，零新 fence 代码。

> 与 animation/transition 那种 fence 自解析不同——box-shadow 无"打包期需解析存值"需求，纯运行时 cascade 消费，保持现状委托即可。

### 6.2 core `apply_decl "box-shadow"` 重写（mapping.rs:1306 整段换）

一个**括号感知 tokenizer** 三病同治：

```
parse_box_shadow(value) -> Vec<BoxShadow>   // 空 = none
  1. 按「括号深度 0 的逗号」切层 → layers       // rgba(...) 内的逗号不切
  2. 每层 tokenize：括号内不切空白 → tokens      // rgba(95,180,212,0.5) 保持一个 token
  3. 识别：
     - "inset" 关键字（首/尾任意位置）→ inset=true，移出
     - color = rgba()/rgb()/#hex/named token（任意位置）
     - 剩余数值按位置语义：ox/oy/blur/spread
  4. <2 数值（缺 ox/oy）→ 整层 false（非法）
  5. σ 不在此算（运行时 render 侧 σ=blur/2）
  6. 顺序保留 = CSS 源序
```

`parse_color` 已支持 8-hex + rgb/rgba（坑 165 修过），此处复用。

### 6.3 非法处理（让 fence 自动报错）

`apply_decl` 返 `false`（→ fence 报 `FenceBadCssValue`）：层内数值 <2、color token 解析不出。负 blur 视为 0（不报错）；spread 负值合法保留。

### 6.4 单测

多层 / inset（首置+尾置）/ spaced rgba / blur+spread / 非法（<2 数值、bad color）。

---

## 7. 渲染层 —— 几何 & sort_key（复用优先，不造新算法）

### 7.1 调研结论：front-layer 机制已存在，复用

- 多层/inset box-shadow：❌ 未实现（`Option<BoxShadow>` 单层）。
- **front-layer sort_key 机制 ✅ 已存在**：`propagate_text_sub_page_sort_keys`（mod.rs:934）就是"附属 mesh 嵌入 primary 与下一个真节点之间"——TextField 的 cursor/selection/composition/文字主体全靠它画在 primary 之上、children 之下。**inset box-shadow 是同类需求 → 复用，不造新算法。**
- back-layer：现状 `propagate_back_layer_sort_keys` + `BACK_LAYER_FLAG`（bit28）不动。
- **id-encoding 空位**：high-byte 分配 1..=15 子页 / 16..=31 back-layer 区 / 32..=35 TextField synth / 232..=255 retired / bit29-30 thumbs。**36..=47 空闲且安全**（bit4 清零不撞 back-layer、不在子页区）。

### 7.2 front-layer id-encoding：走 high-byte synth 约定（非新 bit flag）

```rust
const FRONT_SHADOW_SYNTH_BYTE: u32 = 36;
fn front_shadow_id(primary: u32, idx: u32) -> u32 { (primary & 0x00FF_FFFF) | ((36 + idx) << 24) }
// 36,37,38... = 该 primary 的第 0,1,2... 个 inset（多层各自独立 high byte，避坑 170 同类撞位）
```

照 `tf_synth_id` 编码先例（high-byte tag + low-24 primary）。**为何不沿用 BACK_LAYER_FLAG bit flag**：bit 空间已挤（bit28 back、29/30 thumbs），新 bit 要做全套撞位分析（坑 170 那类）；high-byte 36 是现成安全区，零碰撞风险。

> 诚实声明：back 用 bit-flag、front 用 high-byte，两套约定并存略不对称；但改 back 牵连现有活代码、blast radius 大，新功能走新约定更稳。spec 实现时复核 `is_text_sub_page`（1..=15）对 36 返 false、与 `is_tf_*_synth`（32..=35）不撞。

### 7.3 sort_key：复用 + 扩展现有 propagate

- **front-layer（inset，多层）**：**`propagate_text_sub_page_sort_keys` 本就按 primary 分组、支持多附属 mesh/primary**（计数 + bump + 按 push 序赋 offset）→ 只需把 high byte 36+ 纳入识别，**零算法改动**即支持多层 inset。
- **back-layer（outer，多层）**：现状 `propagate_back_layer_sort_keys` 是 **pair-based、每 main 只容 1 shadow**（多 outer 会撞 sort_key —— 两个 shadow 都被设成 `main_sk-1`）。**需扩成 group-based**：按 main 分组、计数 B、bump `≥M` by B、逆 CSS 序填 `M .. M+B-1`。这是本棒 sort_key 唯一的真算法改动（front 那侧零改）。
- **多层排序**：按 paint 序 push（outer 逆 CSS 序、inset 逆 CSS 序），两个 propagate 各自按出现序赋 offset，天然得到 CSS「先列出在最上」。

CSS 层叠目标（sort_key 升序 = 先绘 = 底）：
```
... outer[last] ... outer[0]  [primary]  inset[last] ... inset[0]  [children...]
```

### 7.4 merge/batch 排除

现有 `is_tf_edit_synth`/`is_text_sub_page` 按 high-byte 排除合成节点不参与合批（merge.rs:35、batch.rs:38 + is_mergeable_mesh）。加 `is_shadow_synth`（high byte 36..）谓词接进同套排除点，零新排除逻辑。

### 7.5 几何

shadow 形状（CSS 正确，照 RmlUi 核对）：
- **outer**：`rect + (ox,oy)`，外扩 `spread`，每角 `radius += spread`。
- **inset**：`padding-box`（border-box 内缩 border），内缩 `spread`，每角 `radius -= spread`，再按 `(ox,oy)` 偏移。注：LoomGUI 围栏无 `border-style`（边框用 box-shadow 模拟，见坑），故无真实 border 宽度时 `padding-box ≡ border-box`；inset 形状 = border-box − spread。

quad 顶点：
- **blur=0**（硬边，showcase 的 `inset 0 0 0 1px` 多属此类）：复用现有 `box_shadow_quad`（实心圆角矩形，**program=0，和普通 quad 同批合**——保住最常见 case 的批合）。
- **blur>0**：外扩 pad ≈ `3σ`（收高斯尾）的 quad；`uv = 顶点本地坐标 − shadow 形状中心`（让 fragment 直接拿 uv 算 SDF，transform 无关）；顶点色 = shadow.color；走新 shadow program（§8）。

---

## 8. 渲染层 —— shader SDF blur（program=5 + `SHADOW_BLUR` keyword）

### 8.1 独立 program 号（避 material key 撞 keyword）

MaterialManager 按 `(program, tex, ctx, matrix, rounded)` 缓存 Material 实例，keyword 不进 key。若 shadow 蹭 program=0，"带 SHADOW_BLUR 的阴影"和"不带 keyword 的普通 quad"会拿**同一个 Material 实例**→ keyword 冲突。照 program=1/2/3/4 各启各 keyword 先例，**program=5 = `SHADOW_BLUR`**，天然独立 Material。

### 8.2 fragment（复用 CLIPPED_ROUNDED 的圆角矩形 SDF，换像素空间）

```hlsl
// p = uv（像素空间，相对 shadow 形状中心，§7.5 几何已编码）
// halfSize/radius/σ/inset 来自 MPB uniform（shadow_params 列，§9）
float qx = abs(p.x) - halfSize.x + radius;
float qy = abs(p.y) - halfSize.y + radius;
float sdf = length(max(float2(qx,qy), 0)) + min(max(qx,qy), 0) - radius;
// sdf<0 形状内，>0 形状外
float d = inset ? -sdf : sdf;                              // inset 翻转：从内边缘向中心衰减
float alpha = exp(-max(d, 0.0)*max(d,0.0) / (2*σ*σ));     // 高斯边衰减（SDF soft-shadow 标准式）
col.a *= alpha;
```

**为何 `exp(-d²/2σ²)` 而非 `erfc`**：Unity HLSL 无 erfc 内建，需自写有理近似（多代码）；`exp(-d²/2σ²)` 是 SDF 软阴影标准式，凸形圆角矩形上与真高斯视觉无差。`σ = 0.5 × blur_radius`（RmlUi 验证值），blur<0.5 视为 0（走 §7.5 实心路径）。

### 8.3 clip 与 mask_context（spec 细化）

shadow 继承 primary 的 `mask_context`（§4.6，overflow 裁剪）。program=5 的 Material 在 `mask_context != 0` 时叠启 `CLIPPED` keyword（keyword 可叠加，program=4 双 keyword 先例）。具体 key 组合留 §10 Unity 后端定。

---

## 9. 数据路径 —— shadow_params blob 新列（照 color_matrix 先例）

`crates/ffi/src/blob.rs`：
- 加列 `("shadow_params", 24)` = `[f32;6]`：`halfSize.x, halfSize.y, radius, σ, inset_flag, _pad`。
- `VERSION 11 → 12`，列数 21 → 22。
- 每 shadow 节点写自身参数；非 shadow 节点写全零（照 effect_block 写出模式）。
- 加 SOA 形状稳定性测试（列数变了就红）。

Unity MirrorPool（照 color_matrix:234 / effect_block:241 双先例）：`if (program == 5)` → `blob.ShadowParams(i)` → `Mpb.SetVector("_ShadowRect", halfSize.xy,radius) / SetFloat("_ShadowSigma",σ) / SetFloat("_ShadowInset",flag)`。

---

## 10. Unity 后端（MirrorPool + MaterialManager）

**MaterialManager**：加 program=5 arm → `EnableKeyword("SHADOW_BLUR")`。material key 不加新维度（program=5 自带独立 Material；`mask_context`(ctx) 已在 key 里 → ancestor overflow 裁剪靠 ctx 维度天然分流；`mask_context≠0` 时叠启 `CLIPPED`，照 program=4 双 keyword 先例）。shadow 自身圆角走 SDF uniform（非 `CLIPPED_ROUNDED`），故 program=5 的 `rounded` 恒 false。

**MirrorPool**：
- `if (program == 5)` → 读 `blob.ShadowParams(i)` → 设 MPB（§9）。
- shadow GO 生命周期：照现有 back-layer 配对模式（`BACK_LAYER_FLAG` bit / high-byte 36 各自独立 GO，按 node_id 池化）。**多层 = 多 GO/primary**——复用现有 back-layer GO 池化，实现时验别撞坑 182 类 churn（shadow 是逐帧重生 synth 节点）。

**dirty/change tracking**：`shadow_params` 是新 blob 列 → `dirty.rs` 几何 hash 须纳入（shadow 参数变 = Header 级，只刷 MPB 不重建 mesh，照 effect_block header_hash 先例）。

---

## 11. 围栏 / 测试 / 验收

**围栏**：box-shadow 保持委托 core（§6），fence 零自有校验代码。唯一动作：查 `docs/design/fence.md` box-shadow 描述，若写了"单层/无 inset/无 blur"限制则更新（doc_schema_sync 门保持绿）。

**core 单测**：
- parse（§6.4）。
- render 几何——shadow 节点数（2 层=1 outer+1 inset）、sort_key 序（CSS 先列出在最上）、pad 范围（3σ）、uv 编码。
- `propagate_shadow`——front-layer sort_key 落在 primary 与首个 child 之间（断言）；多层逆序。
- blob——shadow_params 列按节点填充正确、非 shadow 节点全零。

**headless harness（编码机验，dotnet）**：fixture pkg + `box-shadow-acceptance.html` → 断言 blob 里 shadow 节点数/序/参数（照 4a headless 先例，不碰 Unity）。

**Unity PlayMode 验收（家里机）**：
- `showcase/spec4b/box-shadow-acceptance.html`：多层 outer / inset / blur 各档半径 / spread / offset / 圆角+阴影组合。视觉 checklist：卡片有深度（outer blur）、内描边高光（inset）、层叠序对、blur 软硬合理。
- **重打全 showcase pkg**（ResolvedStyle 形状变 v31→v32，坑 177 staleness）→ 回验 home/shop 等页卡片"平→有层次"（本棒原始动机）。

**工程闭环**：pkg v31→v32 + `MIN=MAX=32` + bincode 形状稳定测试 + 重编 dll + `xtask sync-bindings` + 重打所有 fixture/showcase pkg。

---

## 12. 待 spec/plan 细化点（实现期钉）

- `FRONT_SHADOW_SYNTH_BYTE=36` 全套撞位复核（`is_text_sub_page`/`is_tf_*_synth`/thumbs）。
- `propagate_text_sub_page_sort_keys` 扩展识别 high byte 36+ 的具体 arm 写法（保持 push 序→offset 语义，零算法改）。
- `propagate_back_layer_sort_keys` 从 pair-based 扩成 group-based（多 outer/main）——本棒 sort_key 唯一真算法改动，逆 CSS 序填槽。
- program=5 + `CLIPPED` keyword 组合的 MaterialManager key 行为（是否需 ctx 维度内再分 clipped/非 clipped）。
- MirrorPool 多 shadow GO/primary 的池化键（node_id 合成 id 稳定性）。
- `dirty.rs` shadow_params 进 hash 的具体字段集。
- gradient/SDF 边界 AA（blur=0 实心路径已有 1px AA；blur>0 高斯边天然软，AA 非问题）。

---

## 13. 关键决策记录

1. **blur 走 SDF shader 不走 RT**（§3）：box-shadow 必为圆角矩形 → 解析 SDF + 高斯边，真高斯 / 不破批合 / 零 RT。两个参考都走 RT 是通用渲染器惯性，非 box-shadow 需要。A 超出参考但有据（SDF 数学已在我们 shader）。
2. **单一真相源 = core apply_decl，fence 委托**（§6）：box-shadow 无打包期存值需求，保持现状 `_=>{}` 委托 + apply_decl 返 false 触发 fence 报错。零双份白名单。
3. **front-layer 复用 `propagate_text_sub_page_sort_keys` 技术，不造新算法**（§7）：TextField cursor/selection 本质就是 front-layer，机制已验证。
4. **front-layer id 走 high-byte synth（36），非新 bit flag**（§7.2）：bit 空间挤，high-byte 36 安全区零碰撞；照 `tf_synth_id` 先例。
5. **program=5 独立号 + `SHADOW_BLUR` keyword**（§8.1）：避 material key 撞 keyword。
6. **blur=0 走实心 program=0、blur>0 走 program=5**（§7.5）：保住常见 inset（blur=0）的批合，只有真 blur 付 shadow program 代价。
7. **pkg v31→v32 一刀切**（§5）：ResolvedStyle bincode 形状变，弃 v31 无后向兼容（个人项目惯例）。
8. **σ=0.5×blur + `exp(-d²/2σ²)` 高斯边**（§8.2）：不上 erfc（HLSL 无内建 + 多代码），够好且省；RmlUi σ 映射验证。
