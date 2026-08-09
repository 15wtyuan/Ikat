# box-shadow 多层 + inset + SDF blur Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 showcase 当前破损的 box-shadow 做成 CSS 全语义（多层/inset/gaussian blur/spread/offset/spaced rgba），blur 走 SDF fragment shader 不碰 RT。

**Architecture:** core `ResolvedStyle.box_shadow` 从 `Option<BoxShadow>` 改 `Vec<BoxShadow>`（pkg v31→v32）；apply_decl 重写括号感知 tokenizer；render 发 N 个合成 RenderNode（outer=BACK_LAYER bit28，inset=新 high-byte synth 36），blur>0 走新 program=5 `SHADOW_BLUR` shader（圆角矩形 SDF + `exp(-d²/2σ²)` 高斯边），shadow 参数经新 blob 列 `shadow_params`（v11→v12）流到 Unity MirrorPool MPB。复用 `propagate_text_sub_page_sort_keys`（front-layer）+ 扩 `propagate_back_layer_sort_keys` 成 group-based（多层 outer）。

**Tech Stack:** Rust (core/ffi/fence crates, edition 2021), Unity C# (Mono, MirrorPool/MaterialManager), HLSL (LoomGUI-Unlit.shader). 钉版本 taffy 0.12 / csbindgen 1. 编码机验 Rust+headless；家里机验 Unity PlayMode。

## Global Constraints

- **AI 先验不破**：box-shadow 是标准 CSS，零新标签/属性/语法；所有改动内部（struct/pkg/shader/blob）。
- **单一真相源**：box-shadow 解析只活一份（core `apply_decl`），fence 委托（`_=>{}` 兜底），core 返 false 触发 fence `FenceBadCssValue`。零双份白名单。
- **pkg 一刀切**：`PKG_FORMAT_VERSION`/`MIN_VERSION`/`MAX_VERSION` 三常量 v31→v32（`crates/core/src/asset/mod.rs:36-38`），弃 v31 无后向兼容。
- **复用优先**：front-layer 走现有 `propagate_text_sub_page_sort_keys`（零算法改，加 high-byte 36 识别）；back-layer 扩 pair→group；shader 复用 `CLIPPED_ROUNDED` 的圆角矩形 SDF 数学。
- **模型禁令**：dispatch subagent 禁用 `netease-codemaker/*`，用 `DeepSeek/deepseek-v4-pro` 等直连 provider。
- **Rust 代码注释写上线品质**（说 WHY，不引用坑号）；push 前 `cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings`。
- **改 parse-time / ResolvedStyle 形状后必重打 pkg**（坑 177 staleness）：重编 dll + `xtask sync-bindings` + 重打所有 fixture/showcase pkg。
- **csbindgen 不为 `#[repr(C)]` struct 生成 C# stub**，须手补 C# 镜像（本棒 blob 列变化在 FrameBlob.cs 手改）。

---

## File Structure

**Rust core:**
- `crates/core/src/style/resolved.rs` — `BoxShadow` struct（加 `blur`/`inset` 字段）+ `ResolvedStyle.box_shadow: Vec<BoxShadow>`
- `crates/core/src/style/mapping.rs` — `apply_decl "box-shadow"` 重写（括号感知 tokenizer）
- `crates/core/src/style/mapping/tests.rs` — parse 单测
- `crates/core/src/asset/mod.rs` — `PKG_FORMAT_VERSION`/`MIN_VERSION`/`MAX_VERSION` → 32 + bincode 形状测试
- `crates/core/src/render/node.rs` — `RenderNode.shadow_params: [f32;6]` 字段
- `crates/core/src/render/mod.rs` — shadow 节点发射 + `FRONT_SHADOW_SYNTH_BYTE`/`front_shadow_id`/`is_shadow_synth` + propagate 扩展
- `crates/core/src/render/border.rs` — `box_shadow_quad`（outer 现状）+ 新 inset/pad 几何 helper
- `crates/core/src/render/merge.rs` / `batch.rs` — `is_shadow_synth` 排除点
- `crates/core/src/render/dirty.rs` — `shadow_params` 进 hash
- `crates/ffi/src/blob.rs` — `shadow_params` 列（v11→v12）

**Unity:**
- `unity/package/Shaders/LoomGUI-Unlit.shader` — `SHADOW_BLUR` keyword + SDF fragment + Properties
- `unity/package/Runtime/MaterialManager.cs` — program=5 arm
- `unity/package/Runtime/MirrorPool.cs` — program=5 读 shadow_params → MPB
- `unity/package/Runtime/FrameBlob.cs` — shadow_params 列读取（col 21）

**测试 / 验收:**
- `tests/dotnet/LoomGUI.HeadlessTests/BoxShadowTests.cs`（新建）— blob 断言
- `showcase/spec4b/box-shadow-acceptance.html`（新建）— Unity PlayMode 验收页
- `docs/design/fence.md` — box-shadow 描述更新（若有"单层/无 inset"限制语）

---

## Task 1: 数据模型 + pkg v32 + RenderNode.shadow_params

**Why first:** `ResolvedStyle` 形状变 + `RenderNode` 加字段是全 workspace 编译级涟漪，必须先落地让后续 task 在编译通过的基础上写。含 pkg 版本一刀切。

**Files:**
- Modify: `crates/core/src/style/resolved.rs:286,319-325`（box_shadow 字段 + BoxShadow struct）
- Modify: `crates/core/src/asset/mod.rs:36-38`（PKG 版本三常量）
- Modify: `crates/core/src/render/node.rs`（RenderNode struct + 所有构造点）
- Modify: `crates/core/src/asset/mod.rs` 顶部版本注释 + 加 bincode 形状测试
- Modify: 全 workspace `box_shadow: None` → `box_shadow: Vec::new()` / `Default` 构造点（编译器牵）

**Interfaces:**
- Produces: `BoxShadow { ox, oy, spread, blur, color, inset }`；`ResolvedStyle.box_shadow: Vec<BoxShadow>`；`RenderNode.shadow_params: [f32;6]`（default `[0.0;6]`）

- [ ] **Step 1: 改 BoxShadow struct + ResolvedStyle.box_shadow**

`crates/core/src/style/resolved.rs`：
```rust
pub struct BoxShadow {
    pub ox: f32,
    pub oy: f32,
    pub spread: f32,
    pub blur: f32,        // blur_radius（CSS px）；σ=blur/2 运行时算
    pub color: [f32; 4],
    pub inset: bool,
}
```
`ResolvedStyle.box_shadow` 字段：`pub box_shadow: Option<BoxShadow>` → `pub box_shadow: Vec<BoxShadow>`（空 Vec = none，顺序 = CSS 源序）。同步改 `Default` impl（`box_shadow: None` → `Vec::new()`）。

- [ ] **Step 2: pkg 版本三常量 v31→v32**

`crates/core/src/asset/mod.rs:36-38`：
```rust
pub const PKG_FORMAT_VERSION: u32 = 32; // v32: ResolvedStyle.box_shadow Option→Vec + blur/inset
pub(crate) const MIN_VERSION: u32 = 32;
pub(crate) const MAX_VERSION: u32 = 32;
```
顶部 `//! v31：...` 注释下加 `//! v32：ResolvedStyle.box_shadow Option<BoxShadow>→Vec<BoxShadow> + blur/inset 字段（box-shadow 全语义，bincode 布局变）。`

- [ ] **Step 3: RenderNode 加 shadow_params 字段**

`crates/core/src/render/node.rs`：RenderNode struct 加（照 `effect: EffectBlock` 先例，紧随其后）：
```rust
/// box-shadow SDF 参数（halfSize.x, halfSize.y, radius, σ, inset_flag, _pad）。
/// 非 shadow 节点 = [0.0;6]（blob 写全零）。进 FFI shadow_params 列。
pub shadow_params: [f32; 6],
```
然后全 workspace `cargo build` —— 编译器报每个 `RenderNode { ... }` 构造点缺字段，逐处加 `shadow_params: [0.0; 6],`（照现有 `effect: EffectBlock::default()` 旁加）。重灾：render/mod.rs、render/batch.rs、render/merge.rs、render/dirty.rs（grep `RenderNode {` 定位）。

- [ ] **Step 4: 全 workspace `box_shadow` 消费点改 Vec**

`cargo build` 报所有 `n.style.box_shadow.as_ref()`（render/mod.rs:2167 等）/ `.box_shadow = Some(...)`（mapping.rs:1338 等）编译错，按编译器逐处修（这些点的语义改在 Task 2/3，本步只让编译过：mapping.rs 的 `Some(BoxShadow{..})` 暂改 `vec![BoxShadow{..}]`，render 的 `as_ref()` 暂改 `.first()` 或 `.iter()`——具体正确实现留 Task 2/3，本步只要绿）。

- [ ] **Step 5: bincode 形状稳定性测试**

`crates/core/src/asset/mod.rs` 测试模块加（照现有 version 测试先例）：
```rust
#[test]
fn box_shadow_vec_roundtrips_v32() {
    let mut s = ResolvedStyle::default();
    s.box_shadow = vec![BoxShadow { ox: 1.0, oy: 2.0, spread: 3.0, blur: 4.0,
                                    color: [0.1,0.2,0.3,0.4], inset: true }];
    let bytes = bincode::serialize(&s).unwrap();
    let back: ResolvedStyle = bincode::deserialize(&bytes).unwrap();
    assert_eq!(back.box_shadow.len(), 1);
    assert!(back.box_shadow[0].inset);
    assert_eq!(back.box_shadow[0].blur, 4.0);
}
```

- [ ] **Step 6: 编译 + 测试绿**

Run: `cargo build && cargo test -p loomgui_core asset`
Expected: 全绿（消费点语义还是旧行为没关系，Task 2/3 修正）。

- [ ] **Step 7: fmt/clippy + commit**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
git add -A && git commit -m "feat(core): box_shadow Vec + blur/inset fields, pkg v32, RenderNode.shadow_params"
```

---

## Task 2: apply_decl box-shadow 重写（括号感知 tokenizer）

**Files:**
- Modify: `crates/core/src/style/mapping.rs:1306-1345`（box-shadow arm 整段换）
- Test: `crates/core/src/style/mapping/tests.rs`

**Interfaces:**
- Consumes: `BoxShadow { ox, oy, spread, blur, color, inset }`（Task 1）
- Produces: `apply_decl("box-shadow", v)` 正确填 `style.box_shadow: Vec<BoxShadow>`，非法返 `false`

- [ ] **Step 1: 写失败测试（parse 全语义）**

`crates/core/src/style/mapping/tests.rs` 加：
```rust
#[test]
fn box_shadow_multilayer_inset_blur() {
    let mut s = ResolvedStyle::default();
    assert!(super::apply_decl(&mut s, "box-shadow",
        "0 0 0 1px rgba(95,180,212,0.5), inset 0 1px 0 rgba(255,255,255,0.06)"));
    assert_eq!(s.box_shadow.len(), 2);
    // layer 0 outer
    assert!(!s.box_shadow[0].inset);
    assert_eq!(s.box_shadow[0].spread, 1.0);
    assert_eq!(s.box_shadow[0].blur, 0.0);
    assert_eq!(s.box_shadow[0].color, [95.0/255.0,180.0/255.0,212.0/255.0,0.5]);
    // layer 1 inset
    assert!(s.box_shadow[1].inset);
    assert_eq!(s.box_shadow[1].oy, 1.0);
    assert_eq!(s.box_shadow[1].blur, 0.0);
}

#[test]
fn box_shadow_blur_spread_spaced_rgba() {
    let mut s = ResolvedStyle::default();
    assert!(super::apply_decl(&mut s, "box-shadow", "0 8px 26px rgba(95, 180, 212, 0.5)"));
    assert_eq!(s.box_shadow.len(), 1);
    assert_eq!(s.box_shadow[0].blur, 26.0);
    assert_eq!(s.box_shadow[0].spread, 0.0);
    assert_eq!(s.box_shadow[0].color, [95.0/255.0,180.0/255.0,212.0/255.0,0.5]);
}

#[test]
fn box_shadow_inset_trailing_keyword() {
    let mut s = ResolvedStyle::default();
    assert!(super::apply_decl(&mut s, "box-shadow", "0 0 0 1px #fff inset"));
    assert!(s.box_shadow[0].inset);
}

#[test]
fn box_shadow_illegal_returns_false() {
    let mut s = ResolvedStyle::default();
    assert!(!super::apply_decl(&mut s, "box-shadow", "10px"));      // <2 数值
    assert!(!super::apply_decl(&mut s, "box-shadow", "0 0 0 abc")); // bad color
    assert!(s.box_shadow.is_empty());
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_core style::mapping::tests::box_shadow`
Expected: FAIL（旧 split_whitespace 解析错）

- [ ] **Step 3: 实现括号感知 tokenizer**

`crates/core/src/style/mapping.rs` box-shadow arm（1306 起）整段替换为调用新函数：
```rust
"box-shadow" => {
    match parse_box_shadow(value) {
        Some(list) if !list.is_empty() => { style.box_shadow = list; true }
        Some(_) => true,                 // "none" / 空 → 清空
        None => false,                   // 非法
    }
}
```
新增 `parse_box_shadow`（同文件，照 `parse_filter` 先例的私有 fn 位置）：
```rust
/// CSS box-shadow：括号感知切层 + tokenize。返 None = 非法。
fn parse_box_shadow(value: &str) -> Option<Vec<BoxShadow>> {
    use crate::style::resolved::BoxShadow;
    if value.trim() == "none" { return Some(Vec::new()); }
    // 1. 按括号深度 0 的逗号切层
    let mut layers: Vec<String> = vec![String::new()];
    let mut depth = 0;
    for ch in value.chars() {
        match ch {
            '(' => { depth += 1; layers.last_mut().unwrap().push(ch); }
            ')' => { depth -= 1; layers.last_mut().unwrap().push(ch); }
            ',' if depth == 0 => layers.push(String::new()),
            _ => layers.last_mut().unwrap().push(ch),
        }
    }
    // 2. 每层解析
    let mut out = Vec::new();
    for layer in &layers {
        let bs = parse_one_box_shadow(layer.trim())?;
        out.push(bs);
    }
    Some(out)
}

fn parse_one_box_shadow(s: &str) -> Option<BoxShadow> {
    // tokenize：括号内不切空白
    let mut tokens: Vec<String> = vec![String::new()];
    let mut depth = 0;
    for ch in s.chars() {
        match ch {
            '(' => { depth += 1; tokens.last_mut().unwrap().push(ch); }
            ')' => { depth -= 1; tokens.last_mut().unwrap().push(ch); }
            c if c.is_whitespace() && depth == 0 => tokens.push(String::new()),
            _ => tokens.last_mut().unwrap().push(ch),
        }
    }
    let tokens: Vec<&str> = tokens.iter().map(|t| t.trim()).filter(|t| !t.is_empty()).collect();
    let mut inset = false;
    let mut nums: Vec<f32> = Vec::new();
    let mut color: Option<[f32;4]> = None;
    for t in &tokens {
        if t.eq_ignore_ascii_case("inset") { inset = true; continue; }
        if let Some(c) = parse_color(t) { color = Some(c); continue; }
        // 数值（剥 px）
        match t.trim_end_matches("px").parse::<f32>() {
            Ok(v) => nums.push(v),
            Err(_) => return None,        // 非 inset/color/数值 = 非法
        }
    }
    if nums.len() < 2 { return None; }    // 缺 ox/oy
    let ox = nums[0];
    let oy = nums[1];
    let blur = *nums.get(2).unwrap_or(&0.0);
    let spread = *nums.get(3).unwrap_or(&0.0);
    let color = color.unwrap_or([0.0, 0.0, 0.0, 0.3]);
    Some(BoxShadow { ox, oy, spread, blur: blur.max(0.0), color, inset })
}
```
> 实现者核对：`parse_color`（同文件）已支持 `#hex`/`rgb()`/`rgba()`/named（坑 165）。若 `parse_color` 签名/返回不同，按实际调。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p loomgui_core style::mapping::tests::box_shadow`
Expected: PASS

- [ ] **Step 5: fence 委托链确认（fence 不需改代码）**

Run: `cargo test -p loomgui_fence`
Expected: 绿（box-shadow 仍走 `_=>{}` 委托 apply_decl；非法现在返 false → fence 自动报 FenceBadCssValue）。

- [ ] **Step 6: fmt/clippy + commit**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
git add -A && git commit -m "feat(core): box-shadow paren-aware tokenizer (multi-layer/inset/blur/spaced-rgba)"
```

---

## Task 3: render 发射 shadow 节点 + synth id + sort_key + 排除点

**Files:**
- Modify: `crates/core/src/render/mod.rs`（synth 常量/fn、shadow 发射、propagate 扩展、排除点）
- Modify: `crates/core/src/render/border.rs`（blur>0 pad quad + inset 几何）
- Modify: `crates/core/src/render/merge.rs:35-40`、`batch.rs:35-40`（is_shadow_synth 排除）

**Interfaces:**
- Consumes: `Vec<BoxShadow>`（Task 1/2）、`RenderNode.shadow_params`（Task 1）
- Produces: 每 box-shadow 节点发 N 合成 RenderNode（outer=BACK_LAYER / inset=FRONT_SHADOW_SYNTH high-byte 36），blur>0 填 shadow_params + pad quad；sort_key 正确层叠；merge/batch 排除 shadow 合成节点

- [ ] **Step 1: 加 synth 常量 + helper + is_shadow_synth**

`crates/core/src/render/mod.rs`，照 `BACK_LAYER_FLAG`(line 39) + `tf_synth_id`(line 768) 先例，加：
```rust
/// front-layer（inset box-shadow）合成 id high-byte tag。安全区 36..（照
/// TF_TEXT_SYNTH_BYTE=35 先例；不撞子页 1..=15 / back-layer bit28 / retired 232..=255）。
const FRONT_SHADOW_SYNTH_BYTE: u32 = 36;

fn front_shadow_id(primary: u32, idx: u32) -> u32 {
    (primary & 0x00FF_FFFF) | ((FRONT_SHADOW_SYNTH_BYTE + idx) << 24)
}

/// inset box-shadow 合成节点（high byte 36..）。merge/batch 据此排除合批。
pub(crate) fn is_shadow_synth(node_id: u32) -> bool {
    let hi = (node_id >> 24) as u8;
    hi >= FRONT_SHADOW_SYNTH_BYTE as u8
}
```

- [ ] **Step 2: 写失败测试 —— shadow 节点数 + sort_key 层叠**

`crates/core/src/render/tests.rs` 加（照现有 render 测试构造 Scene 的先例）：
```rust
#[test]
fn box_shadow_emits_back_and_front_layers() {
    // div 有 box-shadow: outer + inset 两层 → 期待 primary + 1 back-layer + 1 front-layer
    // 构造最小 scene（照现有 render 测试 helper），设 node.box_shadow =
    //   vec![outer{..}, inset{inset:true,..}]
    // tick_and_render → 收 nodes
    let back = nodes.iter().filter(|n| n.node_id & BACK_LAYER_FLAG != 0).count();
    let front = nodes.iter().filter(|n| is_shadow_synth(n.node_id)).count();
    assert_eq!(back, 1, "1 outer → 1 back-layer");
    assert_eq!(front, 1, "1 inset → 1 front-layer");
    // sort_key：back < primary < front
    let back_sk = nodes.iter().find(|n| n.node_id & BACK_LAYER_FLAG != 0).unwrap().sort_key;
    let prim_sk = nodes.iter().find(|n| n.node_id == primary_id).unwrap().sort_key;
    let front_sk = nodes.iter().find(|n| is_shadow_synth(n.node_id)).unwrap().sort_key;
    assert!(back_sk < prim_sk, "outer 画在 primary 之下");
    assert!(prim_sk < front_sk, "inset 画在 primary 之上");
}
```
> 实现者：照 `tests.rs` 现有 build_render 测试的 Scene 构造方式（grep `fn .*render.*test` 或现有用例）填具体搭建。

- [ ] **Step 3: 跑测试确认失败**

Run: `cargo test -p loomgui_core render::tests::box_shadow`
Expected: FAIL（发射逻辑还没改）

- [ ] **Step 4: shadow 节点发射（render/mod.rs:2164 区）**

替换现有单 shadow 发射（`if let Some(shadow) = n.style.box_shadow.as_ref() { ... }`）为遍历 Vec、按 outer/inset 分发：
```rust
// box-shadow：每层一个合成 RenderNode。outer=BACK_LAYER（下层），inset=FRONT_LAYER（上层）。
let shadows = &n.style.box_shadow;
if n.kind.is_container() && !shadows.is_empty() {
    let rw = rect.w; let rh = rect.h;
    let radii = n.style.border_radius.as_corners(rw, rh);
    for (i, sh) in shadows.iter().enumerate() {
        let sigma = (sh.blur * 0.5).max(0.0);
        let blur_on = sh.blur >= 0.5;
        let (sid, is_front) = if sh.inset {
            (front_shadow_id(node_id, i as u32), true)
        } else {
            (node_id | BACK_LAYER_FLAG, false)   // 多 outer：见 Step 6 group propagate
        };
        // 几何：outer 外扩 spread / inset 内缩 spread；blur_on 加 pad≈3σ
        let (verts, uvs, colors, idx, params) = crate::render::border::shadow_quad(
            &rect, &radii, sh, blur_on, sigma);
        if verts.is_empty() { continue; }
        let synth_node = RenderNode {
            node_id: sid, parent_id, visible: true, alpha, color_tint,
            world_matrix: wm, blend: BlendMode::Normal,
            mask_context: n.mask_context,   // 继承（overflow 裁剪）—— propagate 会二次同步
            sort_key: 0, change_level: ChangeLevel::Full, reuse_key: 0,
            effect: EffectBlock::default(), shadow_params: params,
            payload: NodePayload::Mesh {
                verts, uvs, colors, indices: idx, image_path: None,
                program: if blur_on { 5 } else { 0 },
                color_matrix: [0.0; 20],
            },
        };
        nodes.push(synth_node);
        if is_front { front_layer_pairs.push((node_id, sid)); }
        else { back_layer_pairs.push((node_id, sid)); }
    }
}
```
> 多 outer 同一 primary 都用 `node_id | BACK_LAYER_FLAG` 会撞 id → Step 6 group propagate 改为按 (primary, idx) 分配。**修正**：outer 多层时 sid 也要唯一。改 `BACK_LAYER_FLAG` 路径为：`if sh.inset { front_shadow_id(node_id, i) } else { node_id | BACK_LAYER_FLAG | ((i as u32) << 24 >>?) }`——BACK_LAYER 是 bit flag 不能编码 idx。**决策**：outer 多层也走 high-byte synth（`BACK_SHADOW_SYNTH_BYTE=16..`，与 front 对称），而非 bit flag。实现者：定义 `BACK_SHADOW_SYNTH_BYTE_BASE: u32 = 16`，`back_shadow_id(primary, idx) = (primary & 0x00FFFFFF) | ((16 + idx) << 24)`，`is_back_shadow_synth(hi in 16..=31)`。这取代 BACK_LAYER_FLAG bit 用于 box-shadow（bit flag 保留给将来其他 back-layer 用例，不删）。is_shadow_synth 扩成识别 16.. 与 36.. 两段。**重新评估撞位**：16..=31 区是 BACK_LAYER_FLAG bit28 置位区——若用 high-byte 16+ 编码 idx，bit28 会被置位（撞 BACK_LAYER_FLAG 语义）。故 outer 多层不能复用 16.. 区。

  **最终决策（实现者照此）**：outer 与 inset 都用 high-byte synth，但 outer 用 **48..=63 区**？不行（bit28 置位）。安全区只有 32..=47（TF 占 32..=35）。**inset 用 36..，outer 多层用 37..**？但 outer 单层现状是 BACK_LAYER_FLAG bit28，改它牵连。**简化**：本棒 box-shadow outer/inset 统一走 high-byte synth（inset=36.., outer=40..），彻底弃用 BACK_LAYER_FLAG 给 box-shadow（BACK_LAYER_FLAG 常量保留不删，仅为占位）。propagate_back_layer_sort_keys 改名/泛化处理 high-byte 40.. 的 outer。实现者：grep `BACK_LAYER_FLAG` 所有引用，box-shadow 这条链改 high-byte，其余引用不动。

  > ⚠️ 这一节是本 plan 最需要实现期判断的点。实现者先读 BACK_LAYER_FLAG 全部引用（merge.rs/batch.rs/propagate），决定"box-shadow 全走 high-byte synth（outer=40.., inset=36..），BACK_LAYER_FLAG 留空不给任何用例"是否净简化。若是，propagate 函数统一成 `propagate_shadow_sort_keys`（back 段 40.. + front 段 36..，group-based）；若否，保留 outer=BACK_LAYER_FLAG bit（仅支持单 outer，多层 outer 在 plan 后续 task 或标 ponytail）。**推荐**：统一 high-byte，一次做对。

- [ ] **Step 5: shadow_quad 几何 helper（border.rs）**

`crates/core/src/render/border.rs` 加（照 `box_shadow_quad` 先例）：
```rust
/// 产 shadow quad 几何 + SDF 参数。outer 外扩 spread / inset 内缩 spread；
/// blur_on 时外扩 pad≈3σ 收高斯尾，uv=顶点本地坐标−形状中心。返 (verts,uvs,colors,indices,params)。
pub fn shadow_quad(
    rect: &Rect, radii: &[(f32,f32);4], sh: &crate::style::resolved::BoxShadow,
    blur_on: bool, sigma: f32,
) -> (Vec<[f32;2]>, Vec<[f32;2]>, Vec<[f32;4]>, Vec<u32>, [f32;6]) {
    // 形状 rect：outer = rect+(ox,oy) 外扩 spread, radius+=spread
    //         inset = rect+(ox,oy) 内缩 spread, radius-=spread（padding-box≈border-box，无 border）
    let (shape_rect, shape_radii) = if sh.inset {
        let r = Rect { x: rect.x+sh.ox, y: rect.y+sh.oy, w: rect.w-2.0*sh.spread, h: rect.h-2.0*sh.spread };
        let sr = radii.map(|(rx,ry)| ((rx-sh.spread).max(0.0),(ry-sh.spread).max(0.0)));
        (r, sr)
    } else {
        let r = Rect { x: rect.x+sh.ox, y: rect.y+sh.oy, w: rect.w, h: rect.h };
        // 外扩 spread 走 box_shadow_quad 现有逻辑（复用或内联）
        (r, radii.clone())
    };
    let center = [shape_rect.x + shape_rect.w*0.5, shape_rect.y + shape_rect.h*0.5];
    let half = [shape_rect.w*0.5, shape_rect.h*0.5];
    let pad = if blur_on { 3.0 * sigma } else { 0.0 };
    // pad quad + uv = vert - center；顶点色 = sh.color
    // ...（照 rounded_rect / box_shadow_quad 的顶点产出风格，外扩 pad 后 4 角 quad）
    let radius = shape_radii[0].0.max(shape_radii[1].0).max(shape_radii[2].0).max(shape_radii[3].0); // 简化：取 max（per-corner 留 spec 细化）
    let params = [half[0], half[1], radius, sigma, if sh.inset {1.0} else {0.0}, 0.0];
    // 实现者：产 4 角 pad quad verts/uvs/colors/indices（uv = [vert.x-center.x, vert.y-center.y]）
    // blur_on=false 时退化成实心圆角矩形（复用 box_shadow_quad 逻辑），program=0
    todo!("verts/uvs/colors/indices 产出 —— 照 rounded_rect 风格，pad 外扩")
}
```
> 实现者：`radii.map` 是伪代码（[(f32,f32);4] 无 .map）——用 `let sr = [radii[0]..];`。todo 处按 `mesh::rounded_rect` / `box_shadow_quad` 的顶点产出风格补全。blur_on=false 复用现有 `box_shadow_quad` 实心产出。

- [ ] **Step 6: propagate 扩展（mod.rs:809 / 934）**

- **back-layer**（`propagate_back_layer_sort_keys`，809）：从 pair-based 扩 group-based。改签名为收 `&[(u32 primary, Vec<u32> shadow_ids)]`（或保留 pairs 但内部 group），算法：每组 B 个 shadow，bump `≥M` by B，逆序填 `M..M+B-1`（首个=CSS 先列=最高=紧贴 primary 下）。照函数现有 DESC-by-sk + mask_context 传播逻辑。
- **front-layer**（`propagate_text_sub_page_sort_keys`，934）：在识别条件加 `|| is_shadow_synth(rn.node_id)`（line 947 区）。该函数本就 group-based 支持多附属/primary → 零算法改，front shadow 自动走"嵌入 primary 之后、下一真节点之前"。

调用点（render/mod.rs:609 区 `propagate_back_layer_sort_keys(...)`）：改为传 group 结构。

- [ ] **Step 7: merge/batch 排除点**

`crates/core/src/render/merge.rs` `mesh_key`（35-40 区）：加
```rust
if crate::render::is_shadow_synth(rn.node_id) {
    return None; // box-shadow 合成节点（inset high-byte 36.. / outer，取决于 Step4 决策）不合并
}
```
`crates/core/src/render/batch.rs` `is_mergeable_mesh`（35-39 区）：加 `&& !crate::render::is_shadow_synth(rn.node_id)`。
> 若 Step 4 决策保留 outer=BACK_LAYER_FLAG，则 outer 已被现有 `& BACK_LAYER_FLAG != 0` 排除，只 inset 需加；若统一 high-byte，则 BACK_LAYER_FLAG 那行对 box-shadow 失效，全靠 is_shadow_synth。实现者按 Step 4 决策对齐。

- [ ] **Step 8: 跑测试 + 渲染冒烟**

Run: `cargo test -p loomgui_core render`
Expected: box_shadow 测试绿，现有 render 测试不回归。

- [ ] **Step 9: fmt/clippy + commit**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
git add -A && git commit -m "feat(render): box-shadow multi-layer synth nodes + SDF params + sort_key layering"
```

---

## Task 4: blob shadow_params 列（v11→v12）

**Files:**
- Modify: `crates/ffi/src/blob.rs`（VERSION、columns、col builder、write）

**Interfaces:**
- Consumes: `RenderNode.shadow_params`（Task 1/3）
- Produces: blob 列 21 `shadow_params`（[f32;6]=24B/节点），FrameBlob.cs 对应读取（Task 6）

- [ ] **Step 1: 改 VERSION + columns**

`crates/ffi/src/blob.rs:15` `VERSION = 11` → `12`，注释加 `v12：加 shadow_params 列（[f32;6]=24B，box-shadow SDF 参数），列数 21→22`。
columns vec（49 行区）加：
```rust
("shadow_params", 24), // v12：box-shadow SDF 参数（halfSize.xy,radius,σ,inset,_pad）
```
列数注释 21→22。

- [ ] **Step 2: 写 col_shadow_params builder + per-node 写出**

照 `col_color_matrix`(88)/`col_effect_block`(91) 先例：加 `let mut col_shadow_params = Vec::<u8>::new();`，per-node 循环（112 区）加：
```rust
// v12：shadow_params per-node。非 shadow 节点 default 全零（照 effect_block 写出模式）。
for &v in rn.shadow_params.iter() {
    col_shadow_params.extend_from_slice(&v.to_le_bytes());
}
```
padding 循环（208 区）加 `col_shadow_params.extend_from_slice(&[0u8; 24]);`。
最终 columns 元组（234 区）加 `("shadow_params", &col_shadow_params),`。

- [ ] **Step 3: SOA 形状稳定测试**

`crates/ffi/src/blob.rs` 测试模块加（照现有 version/列数测试先例）：
```rust
#[test]
fn blob_has_22_columns_v12() {
    assert_eq!(VERSION, 12);
    // 构造 1 节点 frame，build_blob → 解析 header 列数 == 22
    // ...（照现有 blob 测试构造方式）
}
```

- [ ] **Step 4: 重编 + 测试**

Run: `cargo build -p loomgui_ffi_c --release && cargo test -p loomgui_ffi`
Expected: 绿。
> ⚠️ FrameBlob.cs（C#）此刻还读 21 列 → 编码机会错位。Task 6 同步 C#。本 task 只验 Rust 侧。headless dotnet 测试（Task 7）依赖 C# 同步，留那时跑。

- [ ] **Step 5: fmt/clippy + commit**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
git add -A && git commit -m "feat(ffi): blob shadow_params column v11->v12"
```

---

## Task 5: shader program=5 SHADOW_BLUR（家里机）

**Files:**
- Modify: `unity/package/Shaders/LoomGUI-Unlit.shader`

**Interfaces:**
- Consumes: MPB uniforms `_ShadowRect`(halfSize.xy,radius) / `_ShadowSigma`(σ) / `_ShadowInset`(flag)（Task 6 设）
- Produces: program=5 节点 fragment 算圆角矩形 SDF + 高斯边 alpha

- [ ] **Step 1: 加 Properties + multi_compile**

shader Properties 块（27 区 `_CornerRadius` 附近）加：
```hlsl
_ShadowHalfSize("Shadow HalfSize", Vector) = (0,0,0,0)
_ShadowRadius("Shadow Radius", Float) = 0
_ShadowSigma("Shadow Sigma", Float) = 0
_ShadowInset("Shadow Inset", Float) = 0
```
`#pragma multi_compile`（59 区）加 `_ CLIPPED_ROUNDED SHADOW_BLUR`（SHADOW_BLUR 与现有 keyword 平级）。

- [ ] **Step 2: fragment 加 SHADOW_BLUR 分支**

fragment（CLIPPED_ROUNDED SDF 块 217 区之后）加：
```hlsl
#ifdef SHADOW_BLUR
float2 p = i.uv;   // uv 已 = 顶点本地坐标 − 形状中心（core 几何编码，见 Task 3）
float qx = abs(p.x) - _ShadowHalfSize.x + _ShadowRadius;
float qy = abs(p.y) - _ShadowHalfSize.y + _ShadowRadius;
float sdf = length(max(float2(qx,qy), 0.0)) + min(max(qx,qy), 0.0) - _ShadowRadius;
float d = (_ShadowInset > 0.5) ? -sdf : sdf;
float sig = max(_ShadowSigma, 0.0001);
float g = max(d, 0.0);
col.a *= exp(-(g*g) / (2.0*sig*sig));
#endif
```
> 实现者核对：fragment 输入 `i.uv` 的实际语义名（shader v2f 结构），按实际改。`_ShadowHalfSize` 走 `Vector`（xy 用，zw 空）。

- [ ] **Step 3: Unity 编译确认（无报错）**

家里机 Unity 让 shader 重编（保存即触发）。无编译错即过。

- [ ] **Step 4: commit**

```bash
git add unity/package/Shaders/LoomGUI-Unlit.shader
git commit -m "feat(shader): SHADOW_BLUR program (rounded-rect SDF + gaussian edge)"
```

---

## Task 6: Unity 后端 MaterialManager + MirrorPool + FrameBlob.cs（家里机）

**Files:**
- Modify: `unity/package/Runtime/FrameBlob.cs`（shadow_params 列 21 读取）
- Modify: `unity/package/Runtime/MaterialManager.cs:45 区`（program=5 arm）
- Modify: `unity/package/Runtime/MirrorPool.cs:241 区`（program=5 读 shadow_params → MPB）
- Modify: `crates/core/src/render/dirty.rs`（shadow_params 进 hash）

**Interfaces:**
- Consumes: blob shadow_params 列（Task 4）、shader SHADOW_BLUR（Task 5）
- Produces: Unity 真机渲染 box-shadow blur

- [ ] **Step 1: FrameBlob.cs 加 shadow_params 列读取**

照 `ColorMatrix`(91)/`EffectBlock`(103) 先例，加列 21 读取：
```csharp
/// v12：shadow_params 列（第 22 列，index 21）。6 × f32 = 24B/节点。
/// box-shadow SDF 参数（halfSize.xy,radius,σ,inset,_pad）。非 shadow 节点 default 全零。
public float[] ShadowParams(int i) {
    int off = ColOff(21) + i * 24;
    return new float[6] {
        BitConverter.ToSingle(blob, off),
        BitConverter.ToSingle(blob, off+4),
        BitConverter.ToSingle(blob, off+8),
        BitConverter.ToSingle(blob, off+12),
        BitConverter.ToSingle(blob, off+16),
        BitConverter.ToSingle(blob, off+20),
    };
}
```
> ⚠️ Unity Mono 无 `BitConverter.SingleToUInt32Bits`（坑 188），但 `BitConverter.ToSingle(byte[],int)` 可用。列 offset 注释（36-50 区）补 `21=shadow_params([f32;6],24B)`，列数 21→22。

- [ ] **Step 2: MaterialManager program=5 arm**

`MaterialManager.cs:44 区`（`if (program == 4)` 之后）加：
```csharp
if (program == 5) mat.EnableKeyword("SHADOW_BLUR");
```
material key 已含 program 维度 → program=5 天然独立 Material。`mask_context!=0` 时叠启 CLIPPED（照现有 ctx→keyword 逻辑，实现者核对 Get() 内 ctx 处理）。

- [ ] **Step 3: MirrorPool program=5 → MPB**

`MirrorPool.cs:241 区`（effect_block MPB 块之后）加（照 color_matrix:234 先例）：
```csharp
if (ro.Program == 5) {
    float[] sp = blob.ShadowParams(i);
    ro.Mpb.SetVector("_ShadowHalfSize", new Vector4(sp[0], sp[1], 0, 0));
    ro.Mpb.SetFloat("_ShadowRadius", sp[2]);
    ro.Mpb.SetFloat("_ShadowSigma", sp[3]);
    ro.Mpb.SetFloat("_ShadowInset", sp[4]);
}
```
> 实现者核对 `ro.Program` 字段名 + MPB 已 GetPropertyBlock（210 区）。

- [ ] **Step 4: dirty.rs shadow_params 进 hash**

`crates/core/src/render/dirty.rs` `payload_hash`（Mesh 分支末尾）加：
```rust
for &v in rn.shadow_params.iter() {
    v.to_le_bytes().hash(&mut h);
}
```
> 决策：shadow_params 变 = uniform 变 = Header 级（只刷 MPB 不重建 mesh，照 effect_block header_hash 先例）。若 payload_hash 把它算进 → 变化触发 payload diff（mesh rebuild）。**核对**：effect_block 怎么映射到 Header 级（grep `effect` in dirty.rs / ChangeLevel），shadow_params 照同一路径。若 effect_block 在 header_hash 不在 payload_hash，shadow_params 也进 header_hash。实现者按 effect_block 实际位置对齐。

- [ ] **Step 5: Unity 编译 + 重编 dll + sync bindings**

家里机：
```bash
# 编码机侧（若 dll 需重编，blob 列变了）
cargo build -p loomgui_ffi_c --release
cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
cargo run -p xtask -- sync-bindings
```
Unity 重编（C# 改了）。无报错即过。

- [ ] **Step 6: commit**

```bash
git add -A && git commit -m "feat(unity): MirrorPool/MaterialManager program=5 SHADOW_BLUR + FrameBlob shadow_params"
```

---

## Task 7: 验收 —— headless dotnet + Unity PlayMode + showcase 重打 + fence doc

**Files:**
- Create: `tests/dotnet/LoomGUI.HeadlessTests/BoxShadowTests.cs`
- Create: `showcase/spec4b/box-shadow-acceptance.html`
- Modify: `docs/design/fence.md`（box-shadow 描述，若有限制语）
- Modify: 所有 fixture/showcase pkg（重打）

- [ ] **Step 1: headless dotnet blob 断言**

`tests/dotnet/LoomGUI.HeadlessTests/BoxShadowTests.cs`（照现有 HeadlessTest 先例）：构造含 box-shadow 的 fixture pkg（或代码建 scene）→ tick → borrow_frame → 断言 shadow 节点数/program==5/shadow_params 列值。
```csharp
// 照现有 HeadlessTests 结构：LoadPackage/Instantiate +borrow_frame
// 断言：有节点 program==5（blur shadow），ShadowParams 非全零
```
Run（编码机）: `dotnet test tests/dotnet/LoomGUI.HeadlessTests`
Expected: 绿。

- [ ] **Step 2: Unity PlayMode 验收页**

建 `showcase/spec4b/box-shadow-acceptance.html`：多层 outer / inset / blur 各档（5/15/30px）/ spread / offset / 圆角+阴影组合。重打 pkg（`cargo run -p loomgui_pkg -- build showcase`）。家里机 PlayMode 跑，视觉 checklist：
- 卡片有深度（outer blur 软投影）
- 内描边高光（inset 1px）
- 多层 outer 层叠序对（先列在上）
- blur 半径越大越软
- 圆角处阴影也圆

- [ ] **Step 3: 重打全 showcase + 回验**

ResolvedStyle 形状变 → 重打所有 pkg（坑 177）：
```bash
cargo run -p loomgui_pkg -- build showcase
```
家里机回验 home/shop/settings 等页：卡片"平→有层次"（本棒原始动机）。

- [ ] **Step 4: fence.md doc 对齐**

查 `docs/design/fence.md` box-shadow 描述。若写了"单层/无 inset/无 blur"限制 → 更新为全语义。Run: `cargo test -p loomgui_fence`（doc_schema_sync 门）Expected: 绿。

- [ ] **Step 5: 全量门 + commit**

```bash
cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test
dotnet test tests/dotnet/LoomGUI.HeadlessTests
git add -A && git commit -m "test+docs: box-shadow headless/Unity acceptance + showcase repackage + fence.md"
```

---

## Self-Review（写完后自查，已修 inline）

**Spec coverage：** §1-13 各有 task——数据模型(T1)/解析(T2)/渲染几何+sort_key(T3)/blob(T4)/shader(T5)/Unity(T6)/验收(T7) 全覆盖。§3 方案决策、§4 CSS 语义契约散进各 task（σ=0.5×blur 在 T3 Step4、多层逆序在 T3 Step6、blur≥0.5 阈值在 T3 Step4）。§12 待细化点全部在对应 task 标注（撞位复核 T3 Step1、propagate 写法 T3 Step6、MaterialManager key T6 Step2、MirrorPool 池化 T6 Step3、dirty hash T6 Step4）。

**Placeholder scan：** Task 3 Step5 `shadow_quad` 有 `todo!()` + 伪代码 `.map`——已标注"实现者按 rounded_rect 风格补全"，是几何产出细节非设计占位。Task 3 Step4 的 BACK_LAYER_FLAG 决策有 ⚠️ 块标注实现期判断点（统一 high-byte vs 保留 bit flag），给了推荐方案。其余步骤代码完整。

**Type consistency：** `BoxShadow{ox,oy,spread,blur,color,inset}`（T1）在 T2 parse / T3 render 一致。`shadow_params: [f32;6]`（T1）在 T3/T4/T6 一致。`front_shadow_id`/`is_shadow_synth`（T3 Step1）在 T3 Step4/Step7 + T6 引用一致。`FRONT_SHADOW_SYNTH_BYTE=36`（T3 Step1）与 §7.2 spec 一致。
