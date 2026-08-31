//! Render 层入口：遍历 solve 后的 Scene → `Vec<RenderNode>`。
//!
//! 顺序与 `scene.nodes` 索引一致（便于 node_id 对齐），payload 按 kind 决定：
//! - Container/Button → Mesh quad（背景色；无背景色时透明）
//! - Image → Mesh quad + image_path（核心不知图集，path 推给 Unity 查 Sprite）
//! - Text → measure_text 产 TextLayout，装 Text payload
//!
//! 最后调 `batch::assign_sort_keys` 填 sort_key + mask_context。
//!
//! 核心知图尺寸（打包期 PNG IHDR 静态，Stage 持 path→(w,h) 尺寸表）+ 不知图集
//! （运行时纹理/UV 归 Unity）。build_render_nodes 接 `image_sizes: &ImageSizeTable` 查九宫格
//! UV 的 src_w/src_h（slice_px / src_px）。Image/bg-image payload 带 path，UV 全图 (0,0)-(1,1)。

pub mod batch;
pub mod border; // 彩色边框环形 mesh + box-shadow 外扩 quad
pub mod dirty; // dirty hash（header_hash + payload_hash 双轴，跨帧比决定 ChangeLevel）
pub mod gradient; // 渐变像素参数（resolve/sample，shader 与文本渐变 CPU 采样共用）
pub mod merge;
pub mod mesh;
pub mod node;

use crate::layout::ImageSizeTable;
use crate::scene::node::{ControlState, NodeId, NodeKind, Rect, Scene};
use crate::text::atlas::{GlyphAtlas, GlyphKey};
use crate::text::layout::{measure_text, FontTable};
use node::*;

use taffy::style::LengthPercentage;

/// 合成 RenderNode 的 tag 字节（NodeId bits[63:56]，真实节点恒 0）标签区分配总表。
///
/// 每个合成层产一个独立 RenderNode（独立 draw call），其 node_id =
/// `(primary & LOW_56_BITS) | (tag_byte << 56)`，tag 字节编码层类型 + 层内 idx：
/// - 0：真实节点（tag 恒 0——合成 id 与真 id 靠 tag 位型天然区分，无碰撞可能）。
/// - 1..=15：文本跨页子页（sub_page 号）。
/// - 16：V scrollbar thumb、17：H scrollbar thumb（`scroll.rs` 的 V/H_THUMB_FLAG）。
/// - 32..=34：TextField 编辑反馈（光标/选区/composition，`TF_*_SYNTH_BYTE`）。
/// - 35：文本控件文字主体 mesh（`TF_TEXT_SYNTH_BYTE`）。
/// - 36..=43：inset box-shadow（36+idx，最多 8 层/primary，画在 primary 之上、子节点之下）。
/// - 44..=47：outer box-shadow（44+idx，最多 4 层/primary，画在 primary 之下）。
/// - 232..=255：行内图（retired RichText，`INLINE_IMG_SYNTH_ID_BASE`，留 compound-bundle）。
///
/// 选 tag 字节编码（非 bit flag）的理由：outer/inset 多层需在 id 内编码层内 idx
/// 以保证唯一——bit flag 无法编 idx。历史：u32 时代 tag 挤在 bits[31:24]、真实节点
/// index 被迫 < 4096（有 panic 兜底）；u64 拓宽后 idx 全宽 32 bit，该上限不复存在。
const LOW_56_BITS: u64 = 0x00FF_FFFF_FFFF_FFFF;
const FRONT_SHADOW_SYNTH_BYTE: u64 = 36;
const BACK_SHADOW_SYNTH_BYTE: u64 = 44;

/// 生成 inset box-shadow 合成 node_id（tag byte = 36 + idx）。idx = 该 primary 内
/// inset 层的 CSS 序号（保 id 唯一；sort_key 由 propagate 按 push 序另算）。
fn front_shadow_id(primary: u64, idx: u32) -> u64 {
    (primary & LOW_56_BITS) | ((FRONT_SHADOW_SYNTH_BYTE + idx as u64) << 56)
}

/// 生成 outer box-shadow 合成 node_id（tag byte = 44 + idx）。idx = 该 primary 内
/// outer 层的 CSS 序号。
fn back_shadow_id(primary: u64, idx: u32) -> u64 {
    (primary & LOW_56_BITS) | ((BACK_SHADOW_SYNTH_BYTE + idx as u64) << 56)
}

/// 判断 node_id 是否为 inset box-shadow 合成节点（tag byte 36..=43）。
/// propagate_text_sub_page_sort_keys 据此把它们排到 primary 之上（紧随 primary）。
pub(crate) fn is_front_shadow_synth(node_id: u64) -> bool {
    let hi = (node_id >> 56) as u8;
    (FRONT_SHADOW_SYNTH_BYTE as u8..=43).contains(&hi)
}

/// 判断 node_id 是否为 outer box-shadow 合成节点（tag byte 44..=47）。
pub(crate) fn is_back_shadow_synth(node_id: u64) -> bool {
    let hi = (node_id >> 56) as u8;
    (BACK_SHADOW_SYNTH_BYTE as u8..=47).contains(&hi)
}

/// 判断 node_id 是否为任一 box-shadow 合成节点（inset 36..=43 ∪ outer 44..=47）。
/// merge::mesh_key / batch::is_mergeable_mesh 据此排除合批——box-shadow 合成 mesh
/// 须保持独立 node_id（C# MirrorPool 按 node_id 建独立 GO，合批会吞 id）。
pub(crate) fn is_shadow_synth(node_id: u64) -> bool {
    is_front_shadow_synth(node_id) || is_back_shadow_synth(node_id)
}

/// 富文本行内图（inline `<img>`）合成 node_id 子页基址。每个行内图一个独立 RenderNode
/// （image shader + image_path=src），须叠在 primary 文字层之上：sort_key 由
/// `propagate_inline_image_sort_keys` 设为 primary 文字层 max + img_idx + 1。
/// tag byte = (1000 + idx) & 0xFF = 232..=255，不与跨页子页（1..=15）或 box-shadow
/// synth（36..=47）撞——靠 `inline_image_pairs` 显式配对传播 sort_key，不凭 tag 判别。
#[allow(dead_code)] // RichText retired; kept for compound-bundle text model.
pub(crate) const INLINE_IMG_SYNTH_ID_BASE: u64 = 1000;

/// Font-atlas image_path for a given page index. Consumed verbatim by the
/// Unity backend's SpriteResolver. This string format is an ABI-level contract
/// across the FFI boundary — changing it here requires changing the C# side too.
pub(crate) fn font_atlas_path(page: usize) -> String {
    format!("ikat://font-atlas/p{page}")
}

/// gradient text（background-clip:text）每字 quad 的 4 角色：按字形角在文本块 box 内的
/// 坐标采样渐变（`sample_gradient`，与 shader 同一套 t 数学），整段文字作为整体渐变
/// ——而非每字独立全谱渐变。box 取节点 rect（CSS：渐变横跨元素背景区），
/// 字形坐标为文本块局部坐标（与 rect 左上对齐）。
/// 返回 [BL, BR, TR, TL]（quad 顶点序，与 base 字形 push 顺序一致）。
fn gradient_glyph_colors(
    g: &crate::render::gradient::GradientParams,
    glyph_x: f32,
    glyph_advance: f32,
    glyph_top: f32,
    glyph_bottom: f32,
) -> [[f32; 4]; 4] {
    let bl = crate::render::gradient::sample_gradient(g, glyph_x, glyph_bottom);
    let br = crate::render::gradient::sample_gradient(g, glyph_x + glyph_advance, glyph_bottom);
    let tr = crate::render::gradient::sample_gradient(g, glyph_x + glyph_advance, glyph_top);
    let tl = crate::render::gradient::sample_gradient(g, glyph_x, glyph_top);
    [bl, br, tr, tl]
}

/// 查图尺寸表取 src_w/src_h（fallback 64×64）。
/// path 缺失或 w/h=0 → 64.0 兜底（核心不知图集，但知图尺寸）。
fn src_size(image_sizes: &ImageSizeTable, path: &str) -> (f32, f32) {
    image_sizes
        .get(path)
        .filter(|(w, h)| *w != 0 && *h != 0)
        .map(|&(w, h)| (w as f32, h as f32))
        .unwrap_or((64.0, 64.0))
}

/// 九宫格 slice % resolve：值在 (0, 1) 认为是比例（如 25% → 0.25），乘以源图尺寸
/// 转为像素；值 >= 1 认为是已解析的像素值，保持不变。
///
/// 边界区分依据：合法像素切片最小为 1px，故 (0, 1) 区间与像素值无歧义。
/// parse_slice 在解析期把 `25%` 存为 0.25，渲染期必须经此函数 resolve 为像素，
/// 否则 0.25 会被当 0.25px 使用，导致九宫格坍缩。
pub fn resolve_slice_percent(
    s: &crate::style::resolved::SliceInsets,
    src_w: f32,
    src_h: f32,
) -> crate::style::resolved::SliceInsets {
    let r = |v: f32, src: f32| if v > 0.0 && v < 1.0 { v * src } else { v };
    crate::style::resolved::SliceInsets {
        top: r(s.top, src_h),
        bottom: r(s.bottom, src_h),
        left: r(s.left, src_w),
        right: r(s.right, src_w),
    }
}

/// 收集所有 `display:none` 节点 + 其全部后代的 NodeId。
///
/// CSS 语义：`display:none` 整子树不渲染。`build_render_nodes` 遍历时跳过这些节点，
/// 否则 display:none 节点的后代（layout_rect 已是 0 尺寸）仍会产出 RenderNode——
/// 其中 Text 节点的字形会被引擎按自身尺寸渲染，造成"隐藏内容仍显示"。
fn collect_display_none_subtree(scene: &Scene) -> std::collections::HashSet<NodeId> {
    let mut pruned = std::collections::HashSet::new();
    for root in scene.nodes.values() {
        if !matches!(root.style.taffy_style.display, taffy::Display::None) {
            continue;
        }
        let mut stack = vec![root.id];
        while let Some(nid) = stack.pop() {
            if !pruned.insert(nid) {
                continue; // 已收（防环）
            }
            if let Some(node) = scene.get(nid) {
                for c in &node.children {
                    stack.push(*c);
                }
            }
        }
    }
    pruned
}

/// 收集所有 open Dropdown 的 `role="listbox"` 根 NodeId（供末尾浮层追加 DFS 用）。
///
/// 浮层渲染（套 scrollbar thumb 末尾追加模式）：open Dropdown 的 `role="listbox"`
/// 子树跳出正常 DFS 渲染序——正常 DFS 跳过它（不进 id_to_pos），merge 后末尾追加，
/// sort_key 续 max_sort+1，mask_context=MaskContext(0) 跳出祖先 overflow:hidden clip
/// （dropdown 常出现在 scroll 容器/固定高度面板里，展开列表要溢出父边界显示，同 scrollbar
/// thumb 语义）。收起态（open=false）的 popup 已被 `collect_display_none_subtree` 剪掉
/// （display:none），不在此收集——只在 open 时收集根，供 render 末尾 DFS。
///
/// 返回 popup 根 NodeId 列表（调用方再展开整子树进 pruned 集 + 末尾按根 DFS 追加）。
fn collect_open_popup_roots(scene: &Scene) -> Vec<NodeId> {
    let mut roots = Vec::new();
    for n in scene.nodes.values() {
        // 仅 open 的 Dropdown 才收集其 popup（收起态 popup 是 display:none，已被常规剪枝）。
        let is_open_dropdown = matches!(
            scene.controls.get(n.id),
            Some(ControlState::Dropdown { open: true, .. })
        );
        if !is_open_dropdown {
            continue;
        }
        if let Some(popup) = crate::scene::control::find_child_by_role_recursive(
            scene,
            n.id,
            crate::scene::control::ROLE_LISTBOX,
        ) {
            roots.push(popup);
        }
    }
    roots
}

/// 把 `roots` 列表里每个根的整子树 NodeId 并入 `pruned`（防环）。
///
/// 供 open popup 子树：正常 DFS 跳过（不进 id_to_pos）→ assign_sort_keys 的 dfs 遇
/// `!id_to_pos.contains_key(&id)` 早返，自然不赋 sort_key / mask / 不递归。
fn prune_subtrees(scene: &Scene, roots: &[NodeId], pruned: &mut std::collections::HashSet<NodeId>) {
    for &root in roots {
        let mut stack = vec![root];
        while let Some(nid) = stack.pop() {
            if !pruned.insert(nid) {
                continue; // 已收（防环）
            }
            if let Some(node) = scene.get(nid) {
                for c in &node.children {
                    stack.push(*c);
                }
            }
        }
    }
}

/// clip 表条目：context_id（mask_context>0 的层级）→ 该层级的交集绝对 design rect。
///
/// 由 `batch::assign_sort_keys` 在 DFS 时产；`context_id` 与 RenderNode 的
/// `mask_context.0` 对齐（被该 clip 约束的节点引用同一 id）。
///
/// `radii` = 圆角裁剪的四角半径对 `(h, v)`，序 [TL, TR, BR, BL]（与
/// `BorderRadius::as_corners` 同约定）。`None` = 直角裁剪（AABB step）；
/// `Some` = 圆角 SDF 裁剪（shader CLIPPED_ROUNDED 变体）。仅当 clipper 节点
/// 自身 `border_radius` 非全零时填 `Some`——祖先链的圆角不传播到子层级的 clip
/// （每层 clip 只反映该层 clipper 的形状）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipEntry {
    pub context_id: u32,
    pub rect: Rect,
    pub radii: Option<[(f32, f32); 4]>,
}

/// 一帧渲染数据：节点 + clip 表（FFI blob 同帧 emit）。
///
/// `clips` 只含 mask_context>0 的层级；context==0（无 clip）永不入表。
/// 由 `build_render_nodes` 产，`stage::tick_and_render` 透传，`blob::build_blob` 消费。
#[derive(Debug, Clone, Default)]
pub struct FrameData {
    pub nodes: Vec<RenderNode>,
    pub clips: Vec<ClipEntry>,
}

/// 构造合成 scrollbar thumb RenderNode。
/// node_id=sentinel (container|flag)，world_matrix=IDENTITY (design 绝对坐标)，
/// mask_context=0 (不裁剪)，半透明灰 quad。
fn thumb_render_node(node_id: u64, rect: Rect, sort_key: u32) -> RenderNode {
    let (v, uvc, col, idx) =
        crate::render::mesh::quad(&rect, [0.6, 0.6, 0.6, 0.6], [0.0, 0.0], [1.0, 1.0]);
    RenderNode {
        mount_root_id: 0,
        node_id,
        parent_id: None,
        visible: true,
        alpha: 1.0,
        color_tint: [1.0, 1.0, 1.0, 1.0],
        world_matrix: crate::transform::IDENTITY,
        blend: BlendMode::Normal,
        mask_context: MaskContext(0),
        sort_key,
        change_level: ChangeLevel::Full,
        reuse_key: 0,
        effect: EffectBlock::default(),
        shadow_params: [0.0; 6],
        gradient: crate::render::gradient::GradientParams::default(),
        payload: NodePayload::Mesh {
            verts: v,
            uvs: uvc,
            colors: col,
            indices: idx,
            image_path: None,
            program: 0,
            color_matrix: [0.0; 20],
        },
    }
}

/// 遍历 Scene → `FrameData`（nodes + clip 表）。
///
/// 顺序与 `scene.nodes` 同序（node_id == scene 索引），便于 batch DFS 对齐。
/// Text 节点调 `measure_text` 产 TextLayout；Container/Image 产 Mesh quad。
/// `fonts` 按节点 font_family 查字体（Text 节点 fallback 测量用）。clip 表由
/// `batch::assign_sort_keys` 算祖先 clip 链交集后产出。
///
/// `image_sizes` = Stage 持有的 path→(w,h) 尺寸表。九宫格 UV 用此表算 src_w/src_h
/// 构建 Container 背景/边框/渐变 mesh（不含子节点文字）。
///
/// 从原 `k if k.is_container()` 臂抽出，供 TextField/TextArea/NumberField
/// 复用——这些控件需要画背景框（与 Container 一致）并在其上叠加文字。
/// box-shadow 由调用方在 match 外统一处理（检查 `n.kind.is_container()`）。
#[allow(clippy::too_many_arguments)]
fn build_container_mesh(
    visible: bool,
    n: &crate::scene::node::Node,
    node_id: u64,
    parent_id: Option<u64>,
    rect: &Rect,
    wm: [f32; 6],
    alpha: f32,
    color_tint: [f32; 4],
    has_filter: bool,
    color_matrix: [f32; 20],
    anim: Option<&crate::scene::node::NodeAnim>,
    image_sizes: &ImageSizeTable,
) -> RenderNode {
    // background-clip:text：背景裁剪到文字形状。build_text_mesh 已用渐变文字色画字形
    // （gradient_glyph_colors），这里不能再画背景填充——否则文字下面叠一层渐变色矩形。
    // clip_text 时背景色与渐变都抑制。
    let clip_text = n.style.background_clip_text;
    let color = if clip_text {
        [0.0, 0.0, 0.0, 0.0]
    } else {
        anim.and_then(|a| a.bg_color)
            .unwrap_or(n.style.background_color.unwrap_or([0.0, 0.0, 0.0, 0.0]))
    };
    let (image_path, src_w, src_h) = match &n.style.background_image {
        Some(url) => {
            let (sw, sh) = src_size(image_sizes, url);
            (Some(url.clone()), sw, sh)
        }
        None => (None, 64.0f32, 64.0f32),
    };
    let has_image = image_path.is_some();
    let u_min = [0.0, 0.0];
    let u_max = [1.0, 1.0];
    let (rw, rh) = (rect.w, rect.h);
    let radii = n.style.border_radius.as_corners(rw, rh);
    let all_zero = radii.iter().all(|&(rx, ry)| rx <= 0.0 || ry <= 0.0);
    let has_slice = n.style.border_image_slice.is_some();
    // 渐变背景仅在没有背景图（互斥）/ 九宫格切片时启用——program=6/7 per-fragment
    // 渐变 shader（多 stop 分段函数 / radial 非 affine，顶点色不可表达）。**不再要求
    // 直角**：shader 按 box 局部像素坐标采样，圆角几何（rounded_rect）天然兼容（修复
    // background_gradient + border-radius 共存被丢）。clip:text 时背景填充抑制
    // （渐变由字形承载，见 build_text_mesh），避免文字下叠渐变矩形。
    let use_gradient =
        !has_image && !has_slice && !clip_text && n.style.background_gradient.is_some();
    let draw_rect = if !use_gradient
        && !has_slice
        && matches!(
            n.style.background_size,
            crate::style::resolved::BackgroundSize::Contain
        ) {
        let s = (rect.w / src_w.max(1.0)).min(rect.h / src_h.max(1.0));
        crate::scene::node::Rect {
            x: rect.x,
            y: rect.y,
            w: src_w.max(1.0) * s,
            h: src_h.max(1.0) * s,
        }
    } else {
        *rect
    };
    // background-repeat 平铺：图（background-size 后）小于盒时按 repeat/repeat-x/repeat-y
    // 平铺填满（CSS 默认 repeat）。此前 core 只画单张（等价 no-repeat）→ 与浏览器默认
    // repeat 分歧（HTML 平铺填盒、Unity 单张 80×80）。圆角+repeat 退回
    // 单张（圆角裁剪+平铺混合 mesh 留待后续）。
    let repeat = n.style.background_repeat;
    let do_tile = has_image
        && !has_slice
        && !clip_text
        && all_zero
        && repeat != crate::style::resolved::BackgroundRepeat::NoRepeat
        && (draw_rect.w < rect.w - 0.5 || draw_rect.h < rect.h - 0.5);
    // 渐变像素参数（渲染期按当帧 box 解析 %/关键字）；非渐变节点 None → 全零列。
    let grad_params = if use_gradient {
        let g = n
            .style
            .background_gradient
            .as_ref()
            .expect("use_gradient 已校验");
        Some(crate::render::gradient::resolve_gradient(
            g,
            draw_rect.w,
            draw_rect.h,
        ))
    } else {
        None
    };
    let (mut v, mut uvc, mut col, mut idx) = if do_tile {
        crate::render::mesh::tile_image(
            rect,
            draw_rect.w,
            draw_rect.h,
            repeat,
            color,
            [u_min[0], u_max[1]],
            [u_max[0], u_min[1]],
        )
    } else if grad_params.is_some() {
        // uv = box 局部像素坐标（左上原点；shader GRADIENT 分支照 SHADOW_BLUR 的
        // raw-uv 直通先例）。顶点色仍承载 background-color——shader 内做
        // 渐变 over 底色的 source-over 合成（CSS：background-color 垫在渐变下）。
        // 圆角走 rounded_rect 几何（同 uv 口径线性映射——per-fragment 采样不受几何
        // 三角化影响，圆角裁剪与渐变着色正交）。
        if all_zero {
            crate::render::mesh::quad(&draw_rect, color, [0.0, 0.0], [draw_rect.w, draw_rect.h])
        } else {
            crate::render::mesh::rounded_rect(
                &draw_rect,
                color,
                &radii,
                [0.0, 0.0],
                [draw_rect.w, draw_rect.h],
            )
        }
    } else {
        match (has_slice, all_zero) {
            (false, true) => crate::render::mesh::quad(
                &draw_rect,
                color,
                [u_min[0], u_max[1]],
                [u_max[0], u_min[1]],
            ),
            (false, false) => crate::render::mesh::rounded_rect(
                &draw_rect,
                color,
                &radii,
                [u_min[0], u_max[1]],
                [u_max[0], u_min[1]],
            ),
            (true, true) => {
                let resolved_slice = resolve_slice_percent(
                    n.style.border_image_slice.as_ref().unwrap(),
                    src_w,
                    src_h,
                );
                crate::render::mesh::nine_slice(
                    rect,
                    color,
                    &resolved_slice,
                    src_w,
                    src_h,
                    [u_min[0], u_max[1]],
                    [u_max[0], u_min[1]],
                )
            }
            (true, false) => {
                let resolved_slice = resolve_slice_percent(
                    n.style.border_image_slice.as_ref().unwrap(),
                    src_w,
                    src_h,
                );
                crate::render::mesh::nine_slice_rounded(
                    rect,
                    color,
                    &resolved_slice,
                    &radii,
                    src_w,
                    src_h,
                    [u_min[0], u_max[1]],
                    [u_max[0], u_min[1]],
                )
            }
        }
    };
    // 彩色边框激活。无背景图时把边框环形 mesh
    // 拼进同一 payload：纯色 Container 背景与边框同走 program=0（白 1×1 纹理 ×
    // 顶点色），单 draw call，边框三角序在背景之后——重叠的边框环区边框覆盖背景，
    // 内部仅背景，视觉正确。filter（program=3）也走此路：filter 应作用于整元素含边框。
    // 有背景图（program=2/4）时边框需独立 draw call（边框纯色 vs 背景采样
    // 图），此处不做——留待 border + bg-image 共存场景单独处理。渐变（program=6/7）
    // 同理：GRADIENT 分支的 per-fragment 着色会吃掉边框环的顶点色，共存需边框独立
    // draw call，同 bg-image 一起 defer。
    if !has_image && !use_gradient {
        // CSS border-style 默认 none：none 即便 border-width/color 已声明也不渲染边框。
        if n.style.border_style != crate::style::resolved::BorderStyle::None {
            if let Some(border_col) = n.style.border_color {
                let bw = &n.style.taffy_style.border;
                let widths = crate::render::border::BorderWidths {
                    top: resolve_lp(bw.top),
                    right: resolve_lp(bw.right),
                    bottom: resolve_lp(bw.bottom),
                    left: resolve_lp(bw.left),
                };
                if widths.top > 0.0
                    || widths.right > 0.0
                    || widths.bottom > 0.0
                    || widths.left > 0.0
                {
                    let br = crate::render::border::border_ring(rect, &radii, widths, border_col);
                    if !br.3.is_empty() {
                        let base = v.len() as u32;
                        v.extend_from_slice(&br.0);
                        uvc.extend_from_slice(&br.1);
                        col.extend_from_slice(&br.2);
                        idx.extend(br.3.iter().map(|i| i + base));
                    }
                }
            }
        }
    }
    // program：0 纯色 / 2 bg-image / 3 filter / 4 filter+bg-image / 5 box-shadow SDF
    // （调用方合成节点）→ 6 渐变 / 7 渐变+filter。渐变与背景图互斥（6/7 不与 2 组合）。
    let program = if use_gradient {
        if has_filter {
            7u32
        } else {
            6u32
        }
    } else if has_filter {
        if has_image {
            4u32
        } else {
            3u32
        }
    } else if has_image {
        2u32
    } else {
        0u32
    };
    RenderNode {
        mount_root_id: 0,
        node_id,
        parent_id,
        visible,
        alpha,
        color_tint,
        world_matrix: wm,
        blend: BlendMode::Normal,
        mask_context: MaskContext(0),
        sort_key: 0,
        change_level: ChangeLevel::Full,
        reuse_key: n.reuse_key,
        effect: EffectBlock::default(),
        shadow_params: [0.0; 6],
        gradient: grad_params.unwrap_or_default(),
        payload: NodePayload::Mesh {
            verts: v,
            uvs: uvc,
            colors: col,
            indices: idx,
            image_path,
            program,
            color_matrix,
        },
    }
}

/// 每帧算每节点累积 alpha（CSS opacity 父级累积：子整体乘父 alpha）。
/// 返回 Vec 按 NodeId.index() 索引（1 基，len = capacity+1，idx 0 占位——同 world_transforms；
/// 容量而非存活数，remove_node 后槽位可复用但 idx 不变）。roots 不可达的游离节点
/// （父被删的孤儿）兜底 = own alpha（parent_alpha=1.0 的根语义）。
fn accumulate_alpha(scene: &Scene) -> Vec<f32> {
    let cap = scene.nodes.capacity();
    let mut alphas: Vec<f32> = vec![1.0; cap + 1];
    let mut visited: Vec<bool> = vec![false; cap + 1];
    for root in scene.roots.clone() {
        alpha_rec(scene, root, 1.0, &mut alphas, &mut visited);
    }
    for n in scene.nodes.values() {
        if !visited[n.id.index()] {
            alphas[n.id.index()] = own_opacity(scene, n);
        }
    }
    alphas
}

/// DFS 累积：node_alpha = parent_alpha × own；进子树时把 node_alpha 作为子的 parent_alpha。
/// 同 compute_world_transforms 的遍历方式（roots 起按 children 递归，递归深度 = 树深）。
fn alpha_rec(
    scene: &Scene,
    id: NodeId,
    parent_alpha: f32,
    alphas: &mut [f32],
    visited: &mut [bool],
) {
    let node = scene.get_live(id, "render/alpha_rec");
    let acc = parent_alpha * own_opacity(scene, node);
    alphas[id.index()] = acc;
    visited[id.index()] = true;
    for c in node.children.clone() {
        alpha_rec(scene, c, acc, alphas, visited);
    }
}

/// 单节点 own opacity：anim override 优先（动画 player 写 NodeAnim.opacity），None 退回 CSS。
fn own_opacity(scene: &Scene, n: &crate::scene::node::Node) -> f32 {
    scene
        .anim
        .get(n.id)
        .and_then(|a| a.opacity)
        .unwrap_or(n.style.opacity)
}

/// 累积渲染隐藏（世界锚点出屏）：hidden = 祖先任一 render_hidden 或自身——CSS
/// `visibility:hidden` 的继承语义。世界锚点隐藏的是整棵锚定子树（血条=容器+fill+文字），
/// 只看自身标志会留下「容器背景没了、子节点裸奔」的半隐状态。返回表同 accumulate_alpha
/// （capacity+1，1 基索引；孤儿兜底 = 自身标志）。
fn accumulate_render_hidden(scene: &Scene) -> Vec<bool> {
    let cap = scene.nodes.capacity();
    let mut hidden: Vec<bool> = vec![false; cap + 1];
    let mut visited: Vec<bool> = vec![false; cap + 1];
    for root in scene.roots.clone() {
        hidden_rec(scene, root, false, &mut hidden, &mut visited);
    }
    for n in scene.nodes.values() {
        if !visited[n.id.index()] {
            hidden[n.id.index()] = n.render_hidden;
        }
    }
    hidden
}

fn hidden_rec(
    scene: &Scene,
    id: NodeId,
    parent_hidden: bool,
    hidden: &mut [bool],
    visited: &mut [bool],
) {
    let node = scene.get_live(id, "render/hidden_rec");
    let acc = parent_hidden || node.render_hidden;
    hidden[id.index()] = acc;
    visited[id.index()] = true;
    for c in node.children.clone() {
        hidden_rec(scene, c, acc, hidden, visited);
    }
}

/// 累积 world-space 挂载归属（#109 C8）：roots_at[i] = 覆盖节点 i 的挂载根 NodeId raw
///（0 = 屏幕空间）。挂载根自身命中 scene.mounts 表；后代继承最近挂载祖先（嵌套挂载取
/// 最内层）。返回表同 accumulate_alpha 形态（capacity+1，1 基索引；孤儿兜底 = 自身登记）。
fn accumulate_mount_roots(scene: &Scene) -> Vec<u64> {
    let cap = scene.nodes.capacity();
    let mut roots_at: Vec<u64> = vec![0; cap + 1];
    let mut visited: Vec<bool> = vec![false; cap + 1];
    for root in scene.roots.clone() {
        mount_rec(scene, root, 0, &mut roots_at, &mut visited);
    }
    for n in scene.nodes.values() {
        if !visited[n.id.index()] {
            roots_at[n.id.index()] = u64::from(scene.mounts.contains_key(&n.id)) * n.id.0;
        }
    }
    roots_at
}

fn mount_rec(
    scene: &Scene,
    id: NodeId,
    parent_mount: u64,
    roots_at: &mut [u64],
    visited: &mut [bool],
) {
    let node = scene.get_live(id, "render/mount_rec");
    let acc = if scene.mounts.contains_key(&id) {
        id.0
    } else {
        parent_mount
    };
    roots_at[id.index()] = acc;
    visited[id.index()] = true;
    for c in node.children.clone() {
        mount_rec(scene, c, acc, roots_at, visited);
    }
}

/// 挂载行 re-base：把挂载根世界原点 o 从行坐标系剥离——wm 前乘 T(-o)；纯平移行顶点同减
///（与 blob 侧既有纯平移 re-base 合流后语义不变：本地顶点 + 剥离后矩阵）。mask 清理不在
/// 此处——assign_sort_keys / mask 传播会重写 mask_context，真正的清 0 在 merge 后统一
/// post-pass（build_render_nodes_cached 内，v1 挂载内禁 clip）。
fn mount_rebase(rn: &mut RenderNode, slot: u32, ox: f32, oy: f32) {
    rn.mount_root_id = slot;
    rn.world_matrix = crate::transform::mul(
        &crate::transform::from_translate(-ox, -oy),
        &rn.world_matrix,
    );
    if crate::transform::is_pure_translation(&rn.world_matrix) {
        let NodePayload::Mesh { verts, .. } = &mut rn.payload;
        for v in verts.iter_mut() {
            v[0] -= ox;
            v[1] -= oy;
        }
    }
}

pub fn build_render_nodes(
    scene: &Scene,
    fonts: &FontTable,
    prev: &std::collections::HashMap<u64, (u64, u64)>,
    image_sizes: &ImageSizeTable,
    atlas: &mut GlyphAtlas,
) -> (
    FrameData,
    std::collections::HashMap<u64, (u64, u64)>,
    Vec<u32>,
) {
    // 无缓存入口（测试 / example）：一次性 cache = 每帧全量重建（与拆分前等价）。
    let mut cache = dirty::RenderBuildCache::default();
    build_render_nodes_cached(scene, fonts, prev, image_sizes, atlas, &mut cache, 0, 0)
}

/// A2 增量入口（Stage::tick_and_render 用）：输入指纹命中的节点跳过 render_one_node，
/// 整段复用上帧产物（含合成层）；present-set 签名变（结构变化）→ 缓存整表清空兜底。
/// `res_gen` = 宿主资源代数（image_sizes / 字体注册变更全局失效）；`frame_no` 单调
/// 帧号（控件壳永不命中路径的 nonce）。
pub fn build_render_nodes_cached(
    scene: &Scene,
    fonts: &FontTable,
    prev: &std::collections::HashMap<u64, (u64, u64)>,
    image_sizes: &ImageSizeTable,
    atlas: &mut GlyphAtlas,
    cache: &mut dirty::RenderBuildCache,
    res_gen: u64,
    frame_no: u64,
) -> (
    FrameData,
    std::collections::HashMap<u64, (u64, u64)>,
    Vec<u32>,
) {
    // id_to_pos: NodeId → nodes vec 0 基位置。剪 display:none 子树后 nodes 与 scene.nodes
    // 不等长，batch 按此映射索引 nodes；pruned 节点不入表（batch DFS 遇 id_to_pos 没有
    // 的节点即跳过该子树）。
    let mut id_to_pos: std::collections::HashMap<NodeId, usize> = std::collections::HashMap::new();
    // 注：合成 id 与真 id 靠 tag 字节（bits[63:56]）天然区分——真节点恒 0、合成层 ≥ 1，
    // 无碰撞可能。u32 时代「真实节点 index < 4096」的硬上限已随 NodeId u64 拓宽消灭。
    // 直接逐节点构造真 RenderNode。change_level 先占 Full，末尾统一定级。
    // 先剪 display:none 子树——display:none 节点 + 后代不产 RenderNode（CSS 语义）。
    let mut pruned = collect_display_none_subtree(scene);
    // open Dropdown 的 role=listbox 浮层子树也跳过正常 DFS：末尾以浮层模式追加（sort_key 续号、
    // mask=0 跳出祖先 clip）。收集根列表供末尾 DFS 用，同时把整子树并入 pruned。
    let open_popup_roots = collect_open_popup_roots(scene);
    prune_subtrees(scene, &open_popup_roots, &mut pruned);
    let mut nodes: Vec<RenderNode> = Vec::new();
    // 累积 alpha 预计算（父 opacity 逐层乘入子）：RenderNode.alpha 存累积值，后端画时直接用。
    // 主循环是平铺遍历（slotmap 序），父未必先于子，故单独 DFS 一遍把每节点累积值算好。
    let alphas = accumulate_alpha(scene);
    // 累积渲染隐藏同款预计算（继承语义，见 accumulate_render_hidden）。
    let hiddens = accumulate_render_hidden(scene);
    // world-space 挂载归属预计算（#109 C8，见 accumulate_mount_roots）。
    let mount_roots = accumulate_mount_roots(scene);
    // box-shadow outer 阴影合成 RenderNode 追踪：(primary node_id, outer 阴影合成 node_id)。
    // inset 阴影不经此表——由 propagate_text_sub_page_sort_keys 按 tag 字节自动收集。
    let mut back_layer_pairs: Vec<(u64, u64)> = Vec::new();
    // 富文本行内图 RenderNode 追踪：(主节点 node_id, 行内图合成 node_id)。
    let mut inline_image_pairs: Vec<(u64, u64)> = Vec::new();

    // —— A2 present-set 签名（过滤后实际进渲染的节点集）。签名变 = 结构变化（增删/
    // 换父/display 翻转/popup 开合/fold 变化）→ 缓存整表清空（保守兜底，正确性优先）。
    // 过滤谓词与下方渲染循环一致（pruned / 纯空白 / fold）。
    let mut structure_sig: u64 = 0;
    for n in scene.nodes.values() {
        if pruned.contains(&n.id) {
            continue;
        }
        if crate::scene::node::is_whitespace_only_text(scene, n.id) {
            continue;
        }
        if is_folded_into_rich_text(scene, n.id) {
            continue;
        }
        structure_sig = structure_sig.wrapping_mul(31).wrapping_add(n.id.0);
    }
    if cache.structure_sig != structure_sig {
        cache.entries.clear();
        cache.structure_sig = structure_sig;
    }
    // 命中/新存行的 payload_hash 复用表（node_id → ph）：hash 定级 pass 免重算几何 hash。
    // 命中行的几何与缓存帧逐字节相同（指纹含矩阵/alpha 全量）；新存行在存入时顺手算好。
    let mut ph_reuse: std::collections::HashMap<u64, u64> =
        std::collections::HashMap::with_capacity(nodes.capacity());

    for n in scene.nodes.values() {
        if pruned.contains(&n.id) {
            continue;
        }
        // 纯空白 TextNode（HTML tag 间换行+缩进）不画——layout 已过滤，layout_rect 为
        // 默认 0×0，但 content 非空（"\n    "）仍可能产 0 尺寸 mesh，跳过更干净。
        if crate::scene::node::is_whitespace_only_text(scene, n.id) {
            continue;
        }
        // rich-text-block 的 inline 子（TextNode/TextElement/Image，含嵌套 span 子树）在
        // solve 期已折进父的单段 inline flow，render 画进父 mesh（上方 rich arm）。
        // 它们 layout_rect=0、独立画=原点垃圾，跳过。
        if is_folded_into_rich_text(scene, n.id) {
            continue;
        }
        let alpha = alphas[n.id.index()];
        let visible = !hiddens[n.id.index()];
        let mount_root = mount_roots[n.id.index()];
        // 挂载根原点（世界平移）+ 槽位：re-base 与指纹共用——挂载翻转/根移动必须失效
        // 该子树全部缓存行（顶点/矩阵已按根原点改写）。
        let (mount_slot, mount_ox, mount_oy) = if mount_root != 0 {
            let root_id = NodeId(mount_root);
            let slot = scene.mounts.get(&root_id).copied().unwrap_or(0);
            let wt = scene
                .world_transforms
                .get(root_id.index())
                .copied()
                .unwrap_or(crate::transform::IDENTITY);
            (slot, wt[4], wt[5])
        } else {
            (0, 0.0, 0.0)
        };
        // —— A2 增量：输入指纹命中 → 整段复用上帧产物（含合成层与配对追踪）。
        // 指纹输入枚举见 dirty::render_input_fp；跳过 atlas ensure（atlas 只增不重排，
        // 缓存帧已 ensure 过，字形槽永不过期）。
        let fp = dirty::render_input_fp(
            n, scene, alpha, visible, mount_root, mount_slot, mount_ox, mount_oy, res_gen, frame_no,
        );
        if let Some(entry) = cache.entries.get(&n.id) {
            if entry.input_fp == fp {
                cache.hits += 1;
                let base = nodes.len();
                for (i, rn) in entry.nodes.iter().enumerate() {
                    nodes.push(rn.clone());
                    if let Some(&ph) = entry.phs.get(i) {
                        ph_reuse.insert(rn.node_id, ph);
                    }
                }
                id_to_pos.insert(n.id, base + entry.primary_idx);
                back_layer_pairs.extend_from_slice(&entry.back_pairs);
                inline_image_pairs.extend_from_slice(&entry.inline_pairs);
                continue;
            }
        }
        cache.misses += 1;
        let before = nodes.len();
        let pairs_before = back_layer_pairs.len();
        let inline_before = inline_image_pairs.len();
        // 主 DFS 走正常路径：register_id_map=true（登记 id_to_pos 供 assign_sort_keys /
        // NativeHost FFI 查询）。open popup 子树末尾另走 render_one_node(register=false) +
        // 浮层 sort_key/mask 重赋（见下方 popup 追加块）。
        render_one_node(
            scene,
            n,
            fonts,
            image_sizes,
            atlas,
            &mut nodes,
            &mut id_to_pos,
            &mut back_layer_pairs,
            true,
            alpha,
            visible,
        );
        // 挂载子树行 re-base 到挂载根局部系（缓存存 re-base 后形态——命中路径直推）。
        if mount_root != 0 {
            for rn in &mut nodes[before..] {
                mount_rebase(rn, mount_slot, mount_ox, mount_oy);
            }
        }
        // 产物入缓存：该节点产出的全部 RenderNode（含合成层）+ 配对追踪 + primary 下标。
        let emitted = nodes[before..].to_vec();
        let primary_idx = emitted
            .iter()
            .position(|rn| rn.node_id == n.id.0)
            .unwrap_or(0);
        let phs: Vec<u64> = emitted.iter().map(dirty::payload_hash).collect();
        for (rn, &ph) in emitted.iter().zip(phs.iter()) {
            ph_reuse.insert(rn.node_id, ph);
        }
        cache.entries.insert(
            n.id,
            dirty::CachedNodeBuild {
                input_fp: fp,
                nodes: emitted,
                primary_idx,
                back_pairs: back_layer_pairs[pairs_before..].to_vec(),
                inline_pairs: inline_image_pairs[inline_before..].to_vec(),
                phs,
            },
        );
    }
    // batch / merge / thumb
    // sort_keys buffer：按 NodeId.index() 索引（capacity+1，对齐 world_transforms 扩容——
    // slotmap 删后 idx 不变，按 capacity 不按 len）。assign_sort_keys 在 DFS 时填每个节点的
    // pre-merge 序号；merge_meshes 后空 div 的 RenderNode entry 会被吃掉，但 sort_keys
    // 快照保留供 NativeHost FFI 查询。
    let mut sort_keys: Vec<u32> = vec![0u32; scene.nodes.capacity() + 1];
    let clips = batch::assign_sort_keys(scene, &mut nodes, &id_to_pos, &mut sort_keys);
    // 跨页 text 子页 sort_key 传播：assign_sort_keys 只认识真 scene 节点（经 id_to_pos 映射），
    // 不认识合成子页。此处把子页 sort_key 设为 primary.sort_key + page_idx，并把后续真节点
    // 的 sort_key 后移子页个数，保持单调连续。
    propagate_text_sub_page_sort_keys(&mut nodes, &id_to_pos);
    propagate_back_shadow_sort_keys(&mut nodes, &back_layer_pairs);
    // box-shadow synth 节点继承 primary 的 mask_context（overflow 裁剪传播）。
    propagate_shadow_mask_context(&mut nodes);
    propagate_inline_image_sort_keys(&mut nodes, &inline_image_pairs);
    batch::reorder_for_batching(scene, &mut nodes);
    // 控件节点必须保留独立 node_id（Unity 按 node_id 建交互实体/镜像 GameObject）。
    // merge 会把相邻同 DrawState 节点合并成单个（node_id 取 anchor），吞掉被合并者的
    // node_id —— 控件被吞 = Unity 丢失控件实体（不渲染、不可交互）。故控件排除出合并。
    let control_ids: std::collections::HashSet<u64> = scene
        .nodes
        .iter()
        .filter_map(|(_, n)| {
            if matches!(
                n.kind,
                NodeKind::Toggle
                    | NodeKind::RadioButton
                    | NodeKind::Slider
                    | NodeKind::ProgressBar
                    | NodeKind::Dropdown
                    | NodeKind::OptionItem
                    | NodeKind::TextField
                    | NodeKind::TextArea
                    | NodeKind::NumberField
            ) {
                Some(n.id.0)
            } else {
                None
            }
        })
        .collect();
    let (mut nodes, merged_members) = merge::merge_meshes_tracked(&control_ids, nodes);
    // v1 挂载内禁 clip：挂载行 mask 清 0（clip 平面定义在屏幕系，随 3D 容器变换后无
    // 意义）。须在 assign_sort_keys / 各 mask 传播 pass 之后做——那些 pass 按祖先 clip
    // 链重写 mask，build 期清会被覆盖（行自身 mount_root_id 标注在此处仍有效）。
    for rn in &mut nodes {
        if rn.mount_root_id != 0 {
            rn.mask_context = MaskContext(0);
        }
    }
    // post-merge 最大 sort_key：scrollbar thumb / open popup 末尾追加续号用。须在
    // reorder + merge 之后算——reorder 重赋全序 sort_key、merge 可能吞空 mesh entry，
    // pre-merge 的 max 会过期（popup 末尾追加若用过期值，sort_key 会与正常节点交错，
    // 破坏“浮层画在最上层”不变量）。
    let max_sort = nodes.iter().map(|n| n.sort_key).max().unwrap_or(0);
    // 合成 scrollbar thumb
    for n in scene.nodes.values() {
        let nid = n.id;
        if let Some(s) = scene.scroll.get(nid) {
            if crate::scroll::effective(n.style.overflow_y, s.content_size.1, s.viewport_size.1) {
                if let Some(r) = crate::scroll::v_thumb_rect(scene, nid) {
                    let thumb_id = nid.0 | crate::scroll::V_THUMB_FLAG;
                    let mut tn = thumb_render_node(thumb_id, r, max_sort + 1);
                    // 滚动容器继承隐藏（visibility 语义）→ thumb 一并隐藏。
                    tn.visible &= !hiddens[nid.index()];
                    nodes.push(tn);
                }
            }
            if crate::scroll::effective(n.style.overflow_x, s.content_size.0, s.viewport_size.0) {
                if let Some(r) = crate::scroll::h_thumb_rect(scene, nid) {
                    let thumb_id = nid.0 | crate::scroll::H_THUMB_FLAG;
                    let mut tn = thumb_render_node(thumb_id, r, max_sort + 1);
                    tn.visible &= !hiddens[nid.index()];
                    nodes.push(tn);
                }
            }
        }
    }
    // open Dropdown 的 role=listbox 浮层子树：跳出正常 DFS（已在主遍历剪枝），末尾追加。
    // 模式同 scrollbar thumb（上方），但追加整子树 DFS 而非单 quad。sort_key 续 scrollbar
    // thumb 之后（重算 max——thumb 刚 push 进 nodes，占用了 max_sort+1 槽位），mask_context
    // =MaskContext(0) 跳出祖先 overflow:hidden clip（dropdown 常在 scroll 容器/固定高度面板
    // 里，展开列表要溢出父边界显示）。走 render_one_node(register=false)：popup 节点产出
    // 与正常节点几何一致的 RenderNode（背景/文本/图/边框/box-shadow 同路径），但不登记
    // id_to_pos（已在 assign_sort_keys 之后，id_to_pos 不再使用）。
    let mut popup_counter = nodes.iter().map(|n| n.sort_key).max().unwrap_or(0) + 1;
    for &popup_root in &open_popup_roots {
        // 子树绘制序 = stacking context 分层序（主 DFS 同源，popup 内 z/opacity 分层
        // 与 hit 侧一致）。跳过空白 text / rich 折叠子（同主遍历）。
        let order = crate::scene::stacking::paint_order(scene, popup_root, &|_| true);
        for nid in order {
            let Some(node) = scene.get(nid) else {
                continue;
            };
            if crate::scene::node::is_whitespace_only_text(scene, nid) {
                // 空白 text 无子，跳过即可。
                continue;
            }
            // rich-text-block inline 子同样跳过（同主 DFS：已折进父 mesh，独立画=原点垃圾）。
            if is_folded_into_rich_text(scene, nid) {
                continue;
            }
            // 记录 push 前位置——render_one_node 可能 push 多个（跨页 text 子页 / 编辑反馈），
            // 全部需重赋浮层 sort_key + mask。
            let start = nodes.len();
            render_one_node(
                scene,
                node,
                fonts,
                image_sizes,
                atlas,
                &mut nodes,
                &mut id_to_pos,
                &mut back_layer_pairs,
                false,
                alphas[nid.index()],
                !hiddens[nid.index()],
            );
            for rn in &mut nodes[start..] {
                rn.sort_key = popup_counter;
                rn.mask_context = MaskContext(0);
                popup_counter += 1;
            }
        }
    }
    // merge 后按 node_id 算双 hash → 定级别
    let mut new_hashes = std::collections::HashMap::with_capacity(nodes.len());
    for rn in &mut nodes {
        let hh = crate::render::dirty::header_hash(rn);
        // merged 行的 payload 是批内多行 concat——锚自身单行 hash 不代表批内容（非锚
        // 成员变更会漏检成 Skip → 旧 mesh 上屏）。按成员 ph（各自便宜且已算好）拼合
        // 重算：任一成员几何变 / 批成员集变（拆批/并批）→ 批 hash 必变。
        let ph = if let Some(members) = merged_members.get(&rn.node_id) {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            0x4D45_5247u64.hash(&mut h); // "MERG" 判别——与单行 payload hash 空间区分
            for &m in members {
                // ph_reuse 按构造含全部 pre-merge 行（hit 回放缓存 phs / miss 存 emitted
                // phs）；缺项回退 id 自身（两帧同值 → 退化为 Skip 判定，不误伤）。
                let member_ph = ph_reuse.get(&m).copied().unwrap_or(m);
                member_ph.hash(&mut h);
            }
            h.finish()
        } else {
            ph_reuse
                .get(&rn.node_id)
                .copied()
                .unwrap_or_else(|| crate::render::dirty::payload_hash(rn))
        };
        rn.change_level = match prev.get(&rn.node_id) {
            None => ChangeLevel::Full,
            Some(&(prev_hh, prev_ph)) => {
                if prev_ph != ph {
                    ChangeLevel::Full
                } else if prev_hh != hh {
                    ChangeLevel::Header
                } else {
                    ChangeLevel::Skip
                }
            }
        };
        new_hashes.insert(rn.node_id, (hh, ph));
    }
    (FrameData { nodes, clips }, new_hashes, sort_keys)
}

/// 合成 node_id：为跨页 text 子页生成区别于主节点的 id。
/// 编码：bits[63:56] = 子页号（1..=255），bits[55:0] = primary_id 全低位。
/// 真实节点 tag 恒 0，合成子页 tag ≥ 1——位型天然区分，无碰撞可能（u32 时代
/// index < 4096 的硬上限随 u64 拓宽消灭）。
fn synth_text_node_id(primary_id: u64, sub_page: u64) -> u64 {
    (primary_id & LOW_56_BITS) | ((sub_page & 0xFF) << 56)
}

// TextField 编辑反馈 mesh（光标 / 选区背景 / composition 下划线）的合成 node_id 标签。
// 这些 mesh 与背景框、文字 mesh 共属同一节点，但必须各有独立 node_id——否则 dirty
// hash 表（new_hashes，以 node_id 为键）会因键碰撞只保留其一的 hash，导致增量更新
// 漏检。此处用独立合成 id 规避。
//
// 这些 mesh 是逐帧重算的动态反馈（光标闪烁、选区随拖拽变），绝不能与静态背景/文字
// 合批——否则（1）光标闪烁会连累背景每帧重传；（2）cursor 与 composition 变化节奏
// 不同，合批会让 composition 的 node_id 随光标可见性跳变，dirty-tracking 抖动。故
// [`is_tf_edit_synth`] 在 batch/merge 里显式排除它们（靠显式谓词，不靠 tag 滥位）。
//
// tag byte 取值约束（须同时满足）：
//   1. 不在 text 跨页子页范围（1..=15），否则 is_text_sub_page 误判；
//   2. 不在 box-shadow synth 区间（36..=47），否则 is_shadow_synth 误排除合批；
//   3. 不与 retired INLINE_IMG_SYNTH_ID_BASE（tag 232..=255）撞。
// 选 32/33/34，满足全部约束；is_text_sub_page / is_shadow_synth 对此区间均返 false。
const TF_CURSOR_SYNTH_BYTE: u64 = 32;
const TF_SELECTION_SYNTH_BYTE: u64 = 33;
const TF_COMPOSITION_SYNTH_BYTE: u64 = 34;
/// 文字 mesh 合成 id 标签：背景已占真 node_id 时，文字主体 mesh 用此合成 id 区分。
/// 两类节点复用：
/// - 文本控件（TextField/TextArea/NumberField）：先 push 背景框 mesh（真 node_id），
///   再 push 文字 mesh；
/// - rich-text-block 容器：先 push 背景 mesh（真 node_id），再 push
///   多 run 文字 mesh。
/// 若文字也用真 node_id，则 C# MirrorPool 按 node_id 唯一索引 GO 时第二个 mesh 覆盖
/// 第一个 → 渲染残缺/不可见。
/// 文字 mesh 改用此合成 id 与背景区分，C# 各自独立 GO；primary 关联仍 = 真节点 id
///（text_sub_primary_id 可还原），供 sort_key 传播与调试反查。选 35：与子页 1..=15、
/// box-shadow synth 区间（36..=47）、retired 232..=255 均不撞（同 32..=34 安全区间）。
const TF_TEXT_SYNTH_BYTE: u64 = 35;

/// 生成 TextField 编辑反馈 mesh 的合成 node_id（tag byte = tag，低 56 位 = primary）。
/// 编码同 `synth_text_node_id`，仅 tag 字节语义不同（编辑反馈 vs 跨页子页）。
fn tf_synth_id(primary_id: u64, tag_byte: u64) -> u64 {
    (primary_id & LOW_56_BITS) | (tag_byte << 56)
}

/// 判断 node_id 是否为 TextField 编辑反馈 mesh（tag byte 在 32..=34）。
/// 这些 mesh 须保留为独立 RenderNode（不与背景/文字合批，理由见上方常量注释），
/// batch::is_mergeable_mesh 与 merge::mesh_key 据此排除它们。
pub(crate) fn is_tf_edit_synth(node_id: u64) -> bool {
    let hi = (node_id >> 56) as u8;
    (TF_CURSOR_SYNTH_BYTE as u8..=TF_COMPOSITION_SYNTH_BYTE as u8).contains(&hi)
}

/// 判断 node_id 是否为文本控件（TextField/TextArea/NumberField）的文字主体 mesh 合成 id
///（tag byte = 35）。这些 mesh 须独立保留：sort_key 由 propagate_text_sub_page_sort_keys
/// 按 primary 传播（紧跟背景之后），merge::mesh_key 据此排除合批（保持独立 GO）。
pub(crate) fn is_tf_text_synth(node_id: u64) -> bool {
    (node_id >> 56) as u8 == TF_TEXT_SYNTH_BYTE as u8
}

/// 判断 node_id 是否为跨页 text 子页（tag byte 在 1..=15）。
/// box-shadow synth（tag 36..=47）和 INLINE_IMG_SYNTH_ID_BASE（tag 232..=255）
/// 均不在此范围——各自的 propagate 函数单独传播 sort_key，不走子页传播。
fn is_text_sub_page(node_id: u64) -> bool {
    let page = (node_id >> 56) as u8;
    (1..=15).contains(&page)
}

/// 提取跨页 text 子页对应的主节点 id。
fn text_sub_primary_id(node_id: u64) -> u64 {
    node_id & LOW_56_BITS
}

/// box-shadow 的 CSS blur radius → 高斯 σ（RmlUi 映射 σ = blur/2）。
///
/// σ 全程 ≥ 0.5：blur≤1px 时退化为 1px AA 下限，且 blur→σ 单调（blur 越大越糊）。
/// shader erfc 长尾外扩 ≈3σ = 1.5×blur。
fn shadow_sigma(blur: f32) -> f32 {
    (blur * 0.5).max(0.5)
}

/// box-shadow synth 节点继承 primary 的 mask_context（overflow 裁剪传播）。
///
/// assign_sort_keys 按 scene 树 DFS 赋 mask_context，synth 节点（tag 字节合成 id）不在
/// scene 树 → 默认 0（不裁）。本 post-pass 按 synth node_id 的低 56 位找 primary，拷其
/// mask_context，使 shadow 在 overflow 容器内被正确裁剪（shadow 继承主节点
/// mask_context，outer/inset 同传播）。
fn propagate_shadow_mask_context(nodes: &mut [RenderNode]) {
    // 无 shadow synth 节点时早退：绝大多数 UI 帧无 box-shadow，避免每帧分配 HashMap。
    if !nodes.iter().any(|n| is_shadow_synth(n.node_id)) {
        return;
    }
    // primary node_id → mask_context（一遍扫描，避 O(N²)：多层 shadow 共享同 primary 查表）。
    let ctx_by_id: std::collections::HashMap<u64, MaskContext> = nodes
        .iter()
        .filter(|n| !is_shadow_synth(n.node_id))
        .map(|n| (n.node_id, n.mask_context))
        .collect();
    for rn in nodes.iter_mut() {
        if is_shadow_synth(rn.node_id) {
            let primary = rn.node_id & LOW_56_BITS;
            if let Some(mc) = ctx_by_id.get(&primary) {
                rn.mask_context = *mc;
            }
        }
    }
}

/// outer box-shadow 合成节点 sort_key 调整：每个 primary 的 outer 阴影层排在 primary 之下
///（sort_key < primary），CSS 首层最高（最贴 primary 下 = 最后绘 = 最上层）。
///
/// assign_sort_keys 只给 id_to_pos 中的真 scene 节点赋 sort_key；outer 阴影合成节点不在
/// 场景树中，初始 sort_key=0。此函数按 primary 分组调整：每组 B 个阴影，把 sort_key
/// >= primary 的节点后移 B，再逆 CSS 序赋 primary_sk - 1（首层）.. primary_sk - B（末层）。
///
/// 多 primary 按 main_sk DESC 处理，避免累积偏移后的 stale 值污染小 key 比较。
fn propagate_back_shadow_sort_keys(
    nodes: &mut [RenderNode],
    shadow_pairs: &[(u64, u64)], // (primary_node_id, shadow_node_id)，CSS push 序
) {
    if shadow_pairs.is_empty() {
        return;
    }
    // 按 primary 分组 outer 阴影（保 push 序 = CSS 序——outer 按 CSS 序 push）。
    let mut groups: std::collections::HashMap<u64, Vec<u64>> = std::collections::HashMap::new();
    for &(primary, shadow_id) in shadow_pairs {
        groups.entry(primary).or_default().push(shadow_id);
    }
    // 每组采集 main_sk（primary 当前 sort_key），按 DESC 排序处理。
    let mut entries: Vec<(u32, u64, Vec<u64>)> = groups
        .into_iter()
        .map(|(primary, shadow_ids)| {
            let main_sk = nodes
                .iter()
                .find(|n| n.node_id == primary)
                .map(|n| n.sort_key)
                .unwrap_or(0);
            (main_sk, primary, shadow_ids)
        })
        .collect();
    entries.sort_by_key(|&(sk, _, _)| std::cmp::Reverse(sk)); // DESC
                                                              // Loop 1（移位）：每组把 sort_key >= main_sk 且非本组阴影的节点 += B。
                                                              // 降序保证大 key 区移位不污染小 key 原始值。阴影节点当前 sort_key=0，一般 < main_sk
                                                              // 自然排除；显式 set 检查兼底 main_sk=0 的首节点情形（避免阴影被误移位）。
    for &(main_sk, _primary, ref shadow_ids) in &entries {
        let b = shadow_ids.len() as u32;
        let id_set: std::collections::HashSet<u64> = shadow_ids.iter().copied().collect();
        for rn in nodes.iter_mut() {
            if !id_set.contains(&rn.node_id) && rn.sort_key >= main_sk {
                rn.sort_key += b;
            }
        }
    }
    // Loop 2（赋值 + mask 传播）：每组读 primary 移位后 sort_key，逆 CSS 序赋
    // primary_sk - 1（首层最高=最贴 primary）.. primary_sk - B（末层最低）。
    for &(_main_sk, primary, ref shadow_ids) in &entries {
        let Some(main_pos) = nodes.iter().position(|n| n.node_id == primary) else {
            continue;
        };
        let main_sk = nodes[main_pos].sort_key;
        let main_mask = nodes[main_pos].mask_context;
        for (i, &shadow_id) in shadow_ids.iter().enumerate() {
            let Some(shadow_pos) = nodes.iter().position(|n| n.node_id == shadow_id) else {
                continue;
            };
            if main_sk > i as u32 {
                nodes[shadow_pos].sort_key = main_sk - 1 - i as u32;
            } else {
                // primary 在 DFS 首位且层数超过 main_sk：无法全排在 primary 之下不重叠。
                // 兑底赋 0（与 primary 同 key，靠 push 序——shadow 先 push——解析为下层）。
                nodes[shadow_pos].sort_key = 0;
            }
            // 外阴影继承主节点 mask_context（clip 上下文）：阴影是节点的视觉一部分，
            // overflow 容器裁剪时须与主节点同裁（旧实现 push 时硬编码 0 → 不裁、溢出）。
            nodes[shadow_pos].mask_context = main_mask;
        }
    }
}

/// 富文本行内图合成节点 sort_key 传播：每个行内图叠在 primary 的所有文字层
/// （base 子页 + div box-shadow back layer）之上。
///
/// assign_sort_keys 只给 `id_to_pos` 中的真 scene 节点赋 sort_key；行内图是合成节点
/// （INLINE_IMG_SYNTH_ID_BASE 子页），不在场景树中，初始 sort_key=0，会被所有真节点
/// 盖住（行内图"消失"在底色之下）。此函数把行内图 sort_key 设为该 primary 已有最大
/// sort_key + img_idx + 1，并把后续真节点后移行内图个数，保持单调连续。
///
/// 须在 propagate_text_sub_page / box_shadow 之后调用——读取它们产出的最终 sort_key 作为 base。
/// 处理多 primary 时按 base DESC，避免累积偏移污染。
fn propagate_inline_image_sort_keys(nodes: &mut [RenderNode], images: &[(u64, u64)]) {
    if images.is_empty() {
        return;
    }
    // 按 primary 分组行内图（保持声明顺序 = 文本流顺序）。
    let mut groups: std::collections::HashMap<u64, Vec<u64>> = std::collections::HashMap::new();
    for &(_main, img_id) in images {
        let primary = img_id & LOW_56_BITS;
        groups.entry(primary).or_default().push(img_id);
    }
    // base = 该 primary 及其所有合成子节点（子页/shadow/stroke；行内图自身此刻 sk=0）的 max sort_key。
    let mut entries: Vec<(u32, Vec<u64>)> = groups
        .into_iter()
        .map(|(primary, imgs)| {
            let base = nodes
                .iter()
                .filter(|n| (n.node_id & LOW_56_BITS) == primary)
                .map(|n| n.sort_key)
                .max()
                .unwrap_or(0);
            (base, imgs)
        })
        .collect();
    // 按 base DESC：大 base 先处理，其后续节点后移不影响小 base 的原始值。
    entries.sort_by_key(|(base, _)| std::cmp::Reverse(*base));
    for (base, imgs) in &entries {
        let count = imgs.len() as u32;
        let img_set: std::collections::HashSet<u64> = imgs.iter().copied().collect();
        // 后移：sort_key > base 的非本组节点 += count（给行内图腾出 base+1..base+count）。
        for rn in nodes.iter_mut() {
            if !img_set.contains(&rn.node_id) && rn.sort_key > *base {
                rn.sort_key += count;
            }
        }
        // 行内图 sort_key = base + 1 + img_idx（文本流顺序递增）。
        for (i, &img_id) in imgs.iter().enumerate() {
            if let Some(pos) = nodes.iter().position(|n| n.node_id == img_id) {
                nodes[pos].sort_key = base + 1 + i as u32;
            }
        }
    }
}

/// Text 附属 mesh sort_key 传播 + 后续真节点 sort_key 后移。
///
/// assign_sort_keys 只给 `id_to_pos` 中的真 scene 节点赋 sort_key；合成附属 mesh 保持 0。
/// 附属 mesh 有四类（都按 primary = 低 56 位关联真节点）：
/// - 跨页子页（tag 字节 1..=15）：多页文字的后续页。
/// - 文本控件文字首页（tag 字节 35）：TextField/TextArea/NumberField 的文字主体
///   （背景框 mesh 占真 node_id，文字用合成 id 区分，见 TF_TEXT_SYNTH_BYTE）。
/// - 编辑反馈 mesh（tag 字节 32..=34）：光标 / 选区背景 / composition 下划线。
/// - inset box-shadow（tag 字节 36..=43）：内阴影层，画在 primary 之上、子节点之下。
///   render_one_node 按 CSS 逆序 push，使首层 CSS 得最大 offset（画在最上）。
///
/// 此函数：
/// 1. 按 primary 分组统计附属 mesh（遍历 nodes 按 push 序收集 → synth_ids 保 push 序）。
/// 2. 后续真节点 sort_key 后移（按每个 primary 的附属总数）。
/// 3. 附属 sort_key = primary.sort_key + 1 + 序号，mask_context = primary.mask_context。
///    按 nodes 出现序（= push 序 = 绘制层序：选区→文字→composition→光标；子页紧跟首页）
///    依次赋 offset，保留 push 层序且紧跟 primary 之后。
///
/// 步骤 2 保证附属 sort_key 嵌入 primary 与下一个真节点之间，保持单调连续。
fn propagate_text_sub_page_sort_keys(
    nodes: &mut [RenderNode],
    id_to_pos: &std::collections::HashMap<NodeId, usize>,
) {
    // 收集附属 mesh（子页 + 文字首页 + 编辑反馈），按 primary 分组。
    // 遍历 nodes 按 push 序收集 → synth_ids 保 push 序（= 绘制层序）。
    // 同一遍历建 synth_id→位置 映射，供下方赋 sort_key O(1) 查找（合成 id 有意
    // 不进 id_to_pos，见 push_text_sub_pages 注释）。
    let mut groups: std::collections::HashMap<u64, Vec<u64>> = std::collections::HashMap::new();
    let mut synth_pos: std::collections::HashMap<u64, usize> = std::collections::HashMap::new();
    for (i, rn) in nodes.iter().enumerate() {
        if is_text_sub_page(rn.node_id)
            || is_tf_text_synth(rn.node_id)
            || is_tf_edit_synth(rn.node_id)
            || is_front_shadow_synth(rn.node_id)
        {
            let primary = text_sub_primary_id(rn.node_id);
            groups.entry(primary).or_default().push(rn.node_id);
            synth_pos.insert(rn.node_id, i);
        }
    }
    if groups.is_empty() {
        return;
    }
    // 按 primary sort_key 排序（从小到大），count = 该 primary 的附属总数。
    let mut shifts: Vec<(u32, u32)> = groups
        .iter()
        .filter_map(|(&primary, synth_ids)| {
            id_to_pos
                .get(&NodeId(primary))
                .map(|&pos| (nodes[pos].sort_key, synth_ids.len() as u32))
        })
        .collect();
    shifts.sort_by_key(|&(sk, _)| sk);
    // 后移后续真节点 sort_key。使用累积偏移避免 stale primary_sk：shifts 中的
    // primary_sk 是排序前采集的快照；当存在多个带附属的节点时，先处理的节点已把
    // 后续节点（含后面的 text 节点）的 sort_key 向后推，此时再用原始 primary_sk
    // 比较会误判区间，造成 sort_key tie。
    let mut cum_shift: u32 = 0;
    for (primary_sk, n) in &shifts {
        let adjusted_sk = *primary_sk + cum_shift;
        cum_shift += *n;
        for rn in nodes.iter_mut() {
            if is_text_sub_page(rn.node_id)
                || is_tf_text_synth(rn.node_id)
                || is_tf_edit_synth(rn.node_id)
                || is_front_shadow_synth(rn.node_id)
            {
                continue;
            }
            if rn.sort_key > adjusted_sk {
                rn.sort_key += n;
            }
        }
    }
    // 传播附属 sort_key + mask_context：按 synth_ids 序（nodes 出现序 = push 序 = 绘制层序）
    // 依次赋 primary_sk+1, +2, ...。push 序即层序，保留即得正确绘制层序。
    for (&primary, synth_ids) in &groups {
        let pos = match id_to_pos.get(&NodeId(primary)) {
            Some(&p) => p,
            None => continue,
        };
        let primary_sk = nodes[pos].sort_key;
        let primary_mask = nodes[pos].mask_context;
        for (offset, &synth_id) in synth_ids.iter().enumerate() {
            if let Some(&i) = synth_pos.get(&synth_id) {
                nodes[i].sort_key = primary_sk + 1 + offset as u32;
                nodes[i].mask_context = primary_mask;
            }
        }
    }
}

/// 把 taffy `LengthPercentage` 解析为 px。
///
/// - `Length(v)` → v。
/// - `Percent(_)` → 0.0。**已知缺口**：渲染阶段无父 content-box 宽度上下文，
///   无法解析百分比的 padding/border。`style::mapping::parse_four` 对 padding/border
///   只产 `Length`（裸数字/px），故实际不会命中 Percent 分支；若未来 CSS 允许百分比
///   padding/border，需在 layout 阶段把解析结果写回 ResolvedStyle。
pub(crate) fn resolve_lp(lp: LengthPercentage) -> f32 {
    // taffy 0.12：LengthPercentage 是 pub struct(CompactLength) tagged pointer，
    // 内字段私有无法 match 变体——用 into_raw + tag 解构（仅 Length 分支返回值）。
    let cl = lp.into_raw();
    if cl.tag() == taffy::style::CompactLength::LENGTH_TAG {
        cl.value()
    } else {
        0.0
    }
}

/// 烤 content 偏移进 TextLayout 每个 glyph 的 (x, y)（pen = GO-local）。
/// layout 是刚由 measure_text 产的 owned 值，直接 mutate。
pub(crate) fn bake_content_offset(
    layout: &mut crate::text::layout::TextLayout,
    off_x: f32,
    off_y: f32,
) {
    for line in &mut layout.lines {
        // line.y/baseline 也烤 off_y：字形 top = line.baseline - bearing + rect.y 走 line.baseline
        // （不走 g.y），不烤则文字缺 padding/border top（顶到盒顶，非 padding 内）。
        // 行内图世界 y = line.y + y_top 也依赖 line.y 含 off_y。装饰线（underline/strike/overline）
        // 也走 line.baseline / line.y，同理依赖此处烘焙。
        line.y += off_y;
        line.baseline += off_y;
        for run in &mut line.runs {
            for g in &mut run.glyphs {
                g.x += off_x;
                g.y += off_y;
            }
        }
    }
}

/// `build_text_mesh` 产物：base 字形 mesh（按 atlas 页分组）+ effect 参数块。
///
/// SDF 改造后文字效果（shadow/stroke/glow/blur）改由 shader uniform 实现，build_text_mesh
/// 只产 base 字形 mesh；effect 参数由 `pack_effects(text_effects)` 打包成定长 `EffectBlock`，
/// `push_text_meshes` 塞进每个 base/子页/占位 RenderNode.effect，shader 据此在 fragment
/// 阶段重建 outline/underlay/glow/blur。同一文字节点所有页共享同一 effect 配置。
struct TextMeshes {
    /// base 字形，按 atlas 页分组。首页用真 node_id，余页用跨页子页合成 id。
    base: Vec<(u32, Vec<[f32; 2]>, Vec<[f32; 2]>, Vec<[f32; 4]>, Vec<u32>)>,
    /// 节点 text_effects 打包成的 effect 块（build_text_mesh 期算一次，push_text_meshes
    /// 塞进 base/子页/占位 RenderNode.effect）。同一文字节点所有页共享同一 effect 配置。
    effect: EffectBlock,
}

/// 把节点 text_effects 打包成定长 EffectBlock（供 build_text_mesh → RenderNode.effect）。
/// 映射：Shadow→underlay 槽（多重 ≤3，超 3 丢弃；shadow blur→softness、ox/oy→offset）、
/// Stroke→outline（CSS 单值，后到覆盖）、Glow→glow（w→power，起点值，验收精调）、Blur→blur。
/// 同类型多值：shadow 抢 underlay 空槽；stroke/glow/blur 后到覆盖先到。
pub(crate) fn pack_effects(effects: &[crate::text::font_effect::FontEffect]) -> EffectBlock {
    use crate::text::font_effect::FontEffect;
    let mut eb = EffectBlock::default();
    let mut underlay_idx = 0usize;
    for e in effects {
        match e {
            FontEffect::Shadow {
                ox,
                oy,
                blur,
                color,
            } => {
                if underlay_idx < eb.underlay.len() {
                    eb.underlay[underlay_idx] = UnderlaySlot {
                        offset_x: *ox,
                        offset_y: *oy,
                        softness: *blur,
                        color: *color,
                    };
                    underlay_idx += 1;
                }
                // 超 3 个 shadow：静默丢弃（CSS text-shadow 多重尾部，FFI 邻近不 panic）。
            }
            FontEffect::Stroke { w, color } => {
                eb.outline_width = *w;
                eb.outline_color = *color;
            }
            FontEffect::Glow { w, color } => {
                eb.glow_power = *w;
                eb.glow_color = *color;
            }
            FontEffect::Blur { w } => {
                eb.blur_width = *w;
            }
        }
    }
    eb
}

/// 把 TextLayout 每字形展成 quad mesh：4 顶点 + 6 索引，UV 指向核心 atlas。
/// 顶点 = 节点世界空间（pen + bearing + rect.xy）；per-run 颜色烤顶点色（alpha 不烤，走
/// _Alpha uniform）。索引为 2-tri 扇（0-1-2, 0-2-3，与 mesh::quad 同序）。
///
/// per-run 样式（color/font_id/font_size/weight/style）从 `GlyphRun` 读——plain text
/// 是单 run（整段同色），rich text 多 run 各自带色。合成 bold/italic 在此期几何化：
/// - bold：双绘（+1px x 偏移重画一遍），无字体变体。
/// - italic：quad 顶边右偏 0.3×字形高（skew），底边不动。
///
/// 顶点空间：与 mesh::quad 一致用 rect.x/rect.y（纯平移时 = wm[4],wm[5] = 节点绝对位置）。
/// blob re-base 减 (tx,ty)、后端 GO 加回 → 净值留在节点位置。不加 rect 则落 design 原点，
/// 所有文字堆左上角。
///
/// V-flip 方向：atlas v0=顶、v1=底（光栅时 font y-up→bitmap y-down 翻转，v0 对应位图顶）。
/// quad BL（屏幕底，y 更大）采样 atlas 底(v1)、TL（屏幕顶，y 更小）采样 atlas 顶(v0)。
/// 与 Image quad UV 模式一致（BL→(u0,v1)、TR→(u1,v0)）。top=baseline-bearing_y：bearing_y
/// 是 font y-up（顶到 baseline），y-down 坐标里字形顶在 baseline 上方 → 减（加则上下颠倒）。
///
/// 返回按 atlas 页号分组的 mesh 列表。单字体单字号常见一页装下 → 一项（单 draw call）；
/// 超 CJK 字符集才跨页 → 多项（每项独立 image_path = ikat://font-atlas/p{page}）。
///
/// per-glyph 按 `Glyph.font_id` 取 face 光栅（回退字形来自别的 face，必须按字形自己的
/// font_id 取 face + 拼 GlyphKey，否则用错 face 光栅出错字）。装饰线度量走 run 主字体。
///
/// `text_effects`：节点的文字效果配置（INHERITED，来自 ResolvedStyle）。SDF 改造后
/// 由 `pack_effects` 打包成定长 `EffectBlock`，随 base mesh 一并返回；push_text_meshes
/// 把它塞进 base/子页/占位 RenderNode.effect，shader 据此在 fragment 阶段重建效果。
fn build_text_mesh(
    layout: &crate::text::layout::TextLayout,
    atlas: &mut GlyphAtlas,
    fonts: &crate::text::layout::FontTable,
    rect: &crate::scene::node::Rect,
    text_effects: &[crate::text::font_effect::FontEffect],
    background_gradient: Option<&crate::style::resolved::Gradient>,
    background_clip_text: bool,
) -> TextMeshes {
    use std::collections::BTreeMap;
    // 渐变字参数一次解析（box = 节点 rect，CSS：渐变横跨元素背景区）。
    let grad_params =
        background_gradient.map(|g| crate::render::gradient::resolve_gradient(g, rect.w, rect.h));
    // effect 一次打包，base/子页/占位共享——shader 据此 uniform 重建 outline/underlay/glow/blur。
    let effect = pack_effects(text_effects);
    // bearing 来自 glyph bbox，px_w/px_h/UV 含 pad：位图原点 = bbox 原点外扩 pad，
    // left/top 减 pad 对齐位图，否则字形偏 1px。
    let pad = crate::text::atlas::GLYPH_PAD as f32;
    // base 字形 mesh（按 atlas 页分组）。
    let mut base_pages: BTreeMap<u32, (Vec<[f32; 2]>, Vec<[f32; 2]>, Vec<[f32; 4]>, Vec<u32>)> =
        BTreeMap::new();
    for line in &layout.lines {
        for run in &line.runs {
            let italic_skew = if matches!(run.style, crate::text::rich::RichStyle::Italic) {
                0.3
            } else {
                0.0
            };
            // 合成 bold：双绘（offset 0 + offset +1px），模拟加粗，无字体变体。
            // run.weight 在 layout 期就带正确值——plain text 经 measure_text 从 style.font_weight
            // 转（weight_from_font_weight），rich text 经 measure_rich_text 从 base+inline 带。
            let bold_offsets: &[f32] = if matches!(run.weight, crate::text::rich::RichWeight::Bold)
            {
                &[0.0, 1.0]
            } else {
                &[0.0]
            };
            let mut run_x_start: Option<f32> = None;
            let mut run_w = 0.0f32;
            for g in &run.glyphs {
                if run_x_start.is_none() {
                    run_x_start = Some(g.x);
                }
                run_w += g.advance;
                let face = &fonts.font_by_id(g.font_id).face;
                let base_key = GlyphKey {
                    font_id: g.font_id,
                    glyph_id: g.glyph_id,
                };
                let r = atlas.ensure(face, base_key);
                // 无轮廓字形（空格、零宽空格等）atlas 返空 rect → 不产 quad（advance 在
                // layout 已算，pen 已前进，跳过不影响后续字形位置）。
                if r.px_w != 0 && r.px_h != 0 {
                    let p = base_pages
                        .entry(r.page)
                        .or_insert_with(|| (Vec::new(), Vec::new(), Vec::new(), Vec::new()));
                    // SDF：atlas bitmap 固定按 SOURCE_SIZE 光栅，所有 target size 共享同一份 SDF。
                    // quad 按 target/SOURCE 缩放到目标字号；pad(=SPREAD) 是 source 空间随 quad 同比
                    // 缩放；bearing 来自 layout 已是 target 维度，不再乘 scale。
                    let scale = run.font_size / crate::text::atlas::SOURCE_SIZE as f32;
                    let pad_scaled = pad * scale;
                    for &boff in bold_offsets {
                        // pixel snap：原点 round 到整数 design px。flex 居中（align/justify
                        // center）把文字块原点算成亚像素浮点，字形光栅是整数像素，后端 Bilinear
                        // 在亚像素位置混合整个字形 → 模糊。sf=1（按设计分辨率渲染）时整数 design
                        // px = 屏幕像素整数，Bilinear 退化为 Point 采样，字形清晰。
                        let left = (g.x + g.bearing_x - pad_scaled + boff + rect.x).round();
                        let top = (line.baseline - g.bearing_y - pad_scaled + rect.y).round();
                        let right = left + r.px_w as f32 * scale;
                        let bottom = top + r.px_h as f32 * scale;
                        // 合成 italic：quad 顶边右偏（skew × 字形高），底边不动。
                        let skew_top = italic_skew * r.px_h as f32 * scale;
                        let base = p.0.len() as u32;
                        // 顶点序 BL, BR, TR, TL（与 mesh::quad 同序）。
                        p.0.push([left, bottom]);
                        p.0.push([right, bottom]);
                        p.0.push([right + skew_top, top]);
                        p.0.push([left + skew_top, top]);
                        // UV：BL→(u0,v1), BR→(u1,v1), TR→(u1,v0), TL→(u0,v0)。
                        p.1.push([r.u0, r.v1]);
                        p.1.push([r.u1, r.v1]);
                        p.1.push([r.u1, r.v0]);
                        p.1.push([r.u0, r.v0]);
                        // 顶点色：渐变字（background-clip:text）按字形角在文本块 box 内
                        // 采样渐变（整段整体渐变）；否则 run.color。
                        let quad_colors: [[f32; 4]; 4] = if background_clip_text {
                            if let Some(p) = grad_params.as_ref() {
                                let gx0 = g.x + g.bearing_x - pad_scaled;
                                let gx1 = gx0 + r.px_w as f32 * scale;
                                let gy0 = line.baseline - g.bearing_y - pad_scaled;
                                let gy1 = gy0 + r.px_h as f32 * scale;
                                gradient_glyph_colors(p, gx0, gx1 - gx0, gy0, gy1)
                            } else {
                                [run.color; 4]
                            }
                        } else {
                            [run.color; 4]
                        };
                        for c in &quad_colors {
                            p.2.push(*c);
                        }
                        p.3.extend_from_slice(&[
                            base,
                            base + 1,
                            base + 2,
                            base,
                            base + 2,
                            base + 3,
                        ]);
                    }
                }
            }
            // 装饰线（underline/line-through/overline + solid/dashed/dotted/double + 独立色/粗细）。
            // 纯色 quad，用 atlas 唯一白像素 × per-vertex color = run 色。
            if run.deco.lines.0 != 0 {
                if let Some(x0) = run_x_start {
                    let solid = atlas.ensure_solid();
                    let face = &fonts.font_by_id(run.font_id).face;
                    let units = face.units_per_em().max(1) as f32;
                    let scale = run.font_size / units;
                    let font_thickness = (face
                        .underline_metrics()
                        .map(|m| m.thickness as f32 * scale)
                        .unwrap_or(1.0))
                    .max(1.0);
                    let deco_color = run.deco.color.unwrap_or(run.color);
                    let deco_thick = run.deco.thickness.unwrap_or(font_thickness);
                    let x1 = x0 + run_w;
                    // 各线 y 坐标（node-local y-down）：
                    // underline = baseline - font underline position（字体度量）。position 是 y-up
                    // 负值（基线下方），design y-down 下"基线下方"= y 更大 = baseline - position（减负=加）。
                    // line-through = baseline 上方 0.25×font_size（贯穿字形中段）
                    // overline = line 顶边紧靠行顶
                    let underline_y = line.baseline
                        - face
                            .underline_metrics()
                            .map(|m| m.position as f32 * scale)
                            .unwrap_or(run.font_size * 0.1);
                    let strike_y = line.baseline
                        - face
                            .strikeout_metrics()
                            .map(|m| m.position as f32 * scale)
                            .unwrap_or(run.font_size * 0.3);
                    let overline_y = line.y + deco_thick * 0.5;
                    let line_infos: [(f32, bool); 3] = [
                        (underline_y, run.deco.lines.underline()),
                        (strike_y, run.deco.lines.strike()),
                        (overline_y, run.deco.lines.overline()),
                    ];
                    for (line_y, enabled) in line_infos {
                        if !enabled {
                            continue;
                        }
                        match run.deco.style {
                            crate::text::rich::TextDecoStyle::Solid => {
                                emit_deco_quad(
                                    x0,
                                    x1,
                                    rect.x,
                                    rect.y,
                                    line_y,
                                    deco_thick,
                                    deco_color,
                                    &solid,
                                    &mut base_pages,
                                );
                            }
                            crate::text::rich::TextDecoStyle::Dashed => {
                                emit_deco_segments(
                                    x0,
                                    x1,
                                    rect.x,
                                    rect.y,
                                    line_y,
                                    deco_thick,
                                    deco_color,
                                    6.0,
                                    3.0,
                                    &solid,
                                    &mut base_pages,
                                );
                            }
                            crate::text::rich::TextDecoStyle::Dotted => {
                                emit_deco_segments(
                                    x0,
                                    x1,
                                    rect.x,
                                    rect.y,
                                    line_y,
                                    deco_thick,
                                    deco_color,
                                    2.0,
                                    2.0,
                                    &solid,
                                    &mut base_pages,
                                );
                            }
                            crate::text::rich::TextDecoStyle::Double => {
                                let offset = deco_thick * 0.6;
                                let half_thick = deco_thick * 0.4;
                                emit_deco_quad(
                                    x0,
                                    x1,
                                    rect.x,
                                    rect.y,
                                    line_y - offset,
                                    half_thick,
                                    deco_color,
                                    &solid,
                                    &mut base_pages,
                                );
                                emit_deco_quad(
                                    x0,
                                    x1,
                                    rect.x,
                                    rect.y,
                                    line_y + offset,
                                    half_thick,
                                    deco_color,
                                    &solid,
                                    &mut base_pages,
                                );
                            }
                        }
                    }
                } // if let Some(x0)
            }
        }
    }
    let base = base_pages
        .into_iter()
        .map(|(pg, (v, u, c, i))| (pg, v, u, c, i))
        .collect();
    TextMeshes { base, effect }
}

/// 单条实心装饰线 quad：全宽矩形，色 per-vertex。
fn emit_deco_quad(
    x0: f32,
    x1: f32,
    rx: f32,
    ry: f32,
    line_y: f32,
    thick: f32,
    color: [f32; 4],
    solid: &crate::text::atlas::GlyphRect,
    pages: &mut std::collections::BTreeMap<
        u32,
        (Vec<[f32; 2]>, Vec<[f32; 2]>, Vec<[f32; 4]>, Vec<u32>),
    >,
) {
    let y_top = line_y + ry;
    let y_bot = y_top + thick;
    let entry = pages
        .entry(solid.page)
        .or_insert_with(|| (Vec::new(), Vec::new(), Vec::new(), Vec::new()));
    let base = entry.0.len() as u32;
    entry.0.push([x0 + rx, y_bot]);
    entry.0.push([x1 + rx, y_bot]);
    entry.0.push([x1 + rx, y_top]);
    entry.0.push([x0 + rx, y_top]);
    entry.1.push([solid.u0, solid.v1]);
    entry.1.push([solid.u1, solid.v1]);
    entry.1.push([solid.u1, solid.v0]);
    entry.1.push([solid.u0, solid.v0]);
    for _ in 0..4 {
        entry.2.push(color);
    }
    entry
        .3
        .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// 分段装饰线（dashed / dotted）：沿 x 分段画 quad，段长 + 间隔循环。
fn emit_deco_segments(
    x_start: f32,
    x_end: f32,
    rx: f32,
    ry: f32,
    line_y: f32,
    thick: f32,
    color: [f32; 4],
    seg_len: f32,
    gap_len: f32,
    solid: &crate::text::atlas::GlyphRect,
    pages: &mut std::collections::BTreeMap<
        u32,
        (Vec<[f32; 2]>, Vec<[f32; 2]>, Vec<[f32; 4]>, Vec<u32>),
    >,
) {
    let step = seg_len + gap_len;
    let mut x = x_start;
    while x < x_end {
        let seg_end = (x + seg_len).min(x_end);
        let seg_w = seg_end - x;
        if seg_w > 0.0 {
            emit_deco_quad(x, seg_end, rx, ry, line_y, thick, color, solid, pages);
        }
        x += step;
    }
}

/// 把 `build_text_mesh` 产出的 base + effect 推入 `nodes`。
///
/// base 字形走跨页子页机制：首页（page 0）用真 node_id，后续页用 `synth_text_node_id` 合成 id。
/// SDF 改造后文字效果（shadow/stroke/glow/blur）改由 shader uniform 实现——`meshes.effect`
/// 直接塞进 base/子页/占位 RenderNode.effect，不再产 back/front layer 合成节点（原
/// 双层合成机制全废；box-shadow 现走专属 tag 字节 synth，不走此路径）。
fn push_text_meshes(
    visible: bool,
    nodes: &mut Vec<RenderNode>,
    id_to_pos: &mut std::collections::HashMap<NodeId, usize>,
    meshes: TextMeshes,
    n: &crate::scene::node::Node,
    node_id: u64,
    text_primary_id: u64,
    parent_id: Option<u64>,
    alpha: f32,
    color_tint: [f32; 4],
    wm: [f32; 6],
    register_id_map: bool,
) {
    let TextMeshes { base, effect } = meshes;
    if base.is_empty() {
        // 空文本 → 占位 Mesh（无顶点，image_path 用第 0 页兜底）。
        if register_id_map {
            id_to_pos.insert(n.id, nodes.len());
        }
        nodes.push(RenderNode {
            mount_root_id: 0,
            node_id: text_primary_id,
            parent_id,
            visible,
            alpha,
            color_tint,
            world_matrix: wm,
            blend: BlendMode::Normal,
            mask_context: MaskContext(0),
            sort_key: 0,
            change_level: ChangeLevel::Full,
            // 与下方非空首页同规则：合成 id 模式（文本控件：背景已占真 node_id）→
            // reuse_key=0；真 id 模式（普通 TextNode）→ 继承 n.reuse_key。空占位 mesh
            // 同样按 node_id keying 独立 GO，虚拟列表 slot 内不与背景按 reuse_key 冲突。
            reuse_key: if text_primary_id == node_id {
                n.reuse_key
            } else {
                0
            },
            effect,
            shadow_params: [0.0; 6],
            gradient: crate::render::gradient::GradientParams::default(),
            payload: NodePayload::Mesh {
                verts: vec![],
                uvs: vec![],
                colors: vec![],
                indices: vec![],
                image_path: Some(font_atlas_path(0)),
                program: 1,
                color_matrix: [0.0; 20],
            },
        });
        return;
    }
    // 首页（page 0）→ 主 RenderNode。has_rich_bg 时用子页 1 id（bg quad 已占真 node_id），
    // 否则用真 node_id。id_to_pos 仅 register_id_map 时登记（bg quad 已登记则跳过）。
    {
        let (page0, verts0, uvs0, colors0, indices0) = &base[0];
        let path0 = font_atlas_path(*page0 as usize);
        if register_id_map {
            id_to_pos.insert(n.id, nodes.len());
        }
        nodes.push(RenderNode {
            mount_root_id: 0,
            node_id: text_primary_id,
            parent_id,
            visible,
            alpha,
            color_tint,
            world_matrix: wm,
            blend: BlendMode::Normal,
            mask_context: MaskContext(0),
            sort_key: 0,
            change_level: ChangeLevel::Full,
            // 合成 id 模式（文本控件：背景已占真 node_id）→ reuse_key=0：C# MirrorPool
            // 按 node_id keying 独立 GO，不继承主节点 reuse_key（否则虚拟列表 slot 内按
            // reuse_key keying 仍与背景同 GO 冲突）。真 id 模式（普通 TextNode：文字是
            // 唯一 mesh）→ 继承 n.reuse_key（虚拟列表 slot 复用走 reuse_key keying）。
            reuse_key: if text_primary_id == node_id {
                n.reuse_key
            } else {
                0
            },
            effect,
            shadow_params: [0.0; 6],
            gradient: crate::render::gradient::GradientParams::default(),
            payload: NodePayload::Mesh {
                verts: verts0.clone(),
                uvs: uvs0.clone(),
                colors: colors0.clone(),
                indices: indices0.clone(),
                image_path: Some(path0),
                program: 1,
                color_matrix: [0.0; 20],
            },
        });
    }
    // 后续页 → 合成 node_id 的子 RenderNode。
    for (pi, (page, verts, uvs, colors, indices)) in base[1..].iter().enumerate() {
        let sub_id = synth_text_node_id(node_id, (pi + 1) as u64);
        let sub_path = font_atlas_path(*page as usize);
        nodes.push(RenderNode {
            mount_root_id: 0,
            node_id: sub_id,
            parent_id,
            visible,
            alpha,
            color_tint,
            world_matrix: wm,
            blend: BlendMode::Normal,
            mask_context: MaskContext(0),
            sort_key: 0,
            change_level: ChangeLevel::Full,
            // 子页用 reuse_key=0（不继承主节点）：每个子页有独立的合成 node_id，
            // reuse_key=0 时 MirrorPool 按 node_id keying → 独立的 GO，不互相覆盖。
            // 若继承 reuse_key>0（如在虚拟列表 slot 内），所有子页共享同一 reuse_key
            // → MirrorPool 只保留最后一个子页的 mesh，其余页字形丢失。
            reuse_key: 0,
            // 同一文字节点所有页共享同一 effect 配置：fragment shader 按页独立重建效果。
            effect,
            shadow_params: [0.0; 6],
            gradient: crate::render::gradient::GradientParams::default(),
            payload: NodePayload::Mesh {
                verts: verts.clone(),
                uvs: uvs.clone(),
                colors: colors.clone(),
                indices: indices.clone(),
                image_path: Some(sub_path),
                program: 1,
                color_matrix: [0.0; 20],
            },
        });
    }
    // 注意：子页 RenderNode 的合成 node_id 是有意**不**加入 id_to_pos 的。
    // assign_sort_keys / scrollbar thumb / NativeHost 查询均针对真 scene 节点；
    // 合成 id 是纯渲染产物，不应反向映射到 scene node。子页的 sort_key/mask_context
    // 由 propagate_text_sub_page_sort_keys 单独传播。
}

/// 推一个纯色 quad RenderNode（program=0：顶点色 × 白 1×1 纹理）。
///
/// 用于 TextField 编辑反馈 mesh（光标 / 选区背景 / composition 下划线）。顶点坐标已是
/// 世界空间（rect.xy + 内容偏移 + 局部坐标，与文字字形同坐标系），world_matrix 沿用
/// 节点 wm（纯平移时即 identity，坐标已烤进 verts）。
///
/// 每次产独立 RenderNode（调用方给唯一 synth id）。多 quad 场景（选区/下划线跨行）
/// 应一次性收集全部顶点再用 [`push_solid_mesh`] 单次 push，避免多节点同 id 的 hash 碰撞。
#[allow(clippy::too_many_arguments)]
fn push_solid_quad(
    visible: bool,
    nodes: &mut Vec<RenderNode>,
    node_id: u64,
    parent_id: Option<u64>,
    wm: [f32; 6],
    alpha: f32,
    reuse_key: u32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: [f32; 4],
) {
    let verts = vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]];
    let uvs = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let colors = vec![color; 4];
    let indices = vec![0u32, 1, 2, 0, 2, 3];
    push_solid_mesh(
        visible, nodes, node_id, parent_id, wm, alpha, reuse_key, verts, uvs, colors, indices,
    );
}

/// 推一个已组装好的纯色 mesh RenderNode（多 quad 合并为一节点，供选区/下划线跨行用）。
#[allow(clippy::too_many_arguments)]
fn push_solid_mesh(
    visible: bool,
    nodes: &mut Vec<RenderNode>,
    node_id: u64,
    parent_id: Option<u64>,
    wm: [f32; 6],
    alpha: f32,
    reuse_key: u32,
    verts: Vec<[f32; 2]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
) {
    nodes.push(RenderNode {
        mount_root_id: 0,
        node_id,
        parent_id,
        visible,
        alpha,
        color_tint: [1.0; 4],
        world_matrix: wm,
        blend: BlendMode::Normal,
        mask_context: MaskContext(0),
        sort_key: 0,
        change_level: ChangeLevel::Full,
        reuse_key,
        effect: EffectBlock::default(),
        shadow_params: [0.0; 6],
        gradient: crate::render::gradient::GradientParams::default(),
        payload: NodePayload::Mesh {
            verts,
            uvs,
            colors,
            indices,
            image_path: None,
            program: 0,
            color_matrix: [0.0; 20],
        },
    });
}

/// 节点是否被折进某个 rich-text-block 祖先的 inline flow。
///
/// rich-text-block 容器的 inline 子树（TextNode / TextElement(span) / Image，span 可嵌套）
/// 在 solve 期不进 taffy（layout_rect 保持默认 0），在 render 期不独立画——整段折进父
/// 的单条 inline flow mesh（父走 rich_text_block Container arm 读 text_layouts[父]）。
/// 本谓词向上查 parent 链：任一严格祖先 rich_text_block=true 即该节点已折叠，主 render
/// 遍历与 popup 浮层遍历须跳过它（否则在原点画 0 尺寸垃圾 mesh）。
fn is_folded_into_rich_text(scene: &Scene, mut id: NodeId) -> bool {
    while let Some(parent) = scene.get(id).and_then(|n| n.parent) {
        match scene.get(parent) {
            Some(pn) if pn.rich_text_block => return true,
            Some(_) => id = parent,
            None => break,
        }
    }
    false
}

/// 渲染单个 Scene 节点为一个或多个 RenderNode 并推入 `nodes`（共享于主 DFS 与 open popup
/// 末尾追加 DFS）。
///
/// 复用入口——主 DFS 与 [`build_render_nodes`] 末尾的 popup 浮层追加都走此函数，
/// 保证 popup 子树节点与正常节点产出几何一致的 RenderNode（背景/文本/图/边框/box-shadow
/// 完全同一路径）。两者区别仅在 `register_id_map`：
/// - 主 DFS（register=true）：登记 `id_to_pos` 供 `assign_sort_keys` / NativeHost FFI 查询；
/// - popup 浮层追加（register=false）：不登记 id_to_pos（popup 跳出正常 DFS），sort_key /
///   mask_context 由调用方 popup DFS 末尾重赋（续 max_sort+1，MaskContext(0) 跳出祖先 clip）。
///
/// TextNode/TextField 内部多 RenderNode push（跨页子页 / 编辑反馈 mesh）也走此函数——
/// push_text_meshes 的 register_id_map 参数透传本函数入参，与主调用同语义。
///
/// 不跳过任何节点：调用方负责 pruned / 空白 text 过滤（主 DFS）或 popup 子树枚举（浮层）。
///
/// `alpha` 是**累积值**（父链 opacity 逐层乘入，由调用方从 `accumulate_alpha` 表取）——
/// CSS opacity 语义下子节点整体乘父 alpha，后端画时直接用累积值，不做二次累积。
#[allow(clippy::too_many_arguments)]
fn render_one_node(
    scene: &Scene,
    n: &crate::scene::node::Node,
    fonts: &FontTable,
    image_sizes: &ImageSizeTable,
    atlas: &mut GlyphAtlas,
    nodes: &mut Vec<RenderNode>,
    id_to_pos: &mut std::collections::HashMap<NodeId, usize>,
    back_layer_pairs: &mut Vec<(u64, u64)>,
    register_id_map: bool,
    alpha: f32,
    visible: bool,
) {
    let anim = scene.anim.get(n.id);
    // 运行时渲染隐藏（世界锚点出屏）：本节点全部渲染行 visible=0（后端保留 GO 隐藏）。
    // visible 是累积值（祖先任一 render_hidden 即整子树隐藏，visibility:hidden 继承语义），
    // 由调用方从 accumulate_render_hidden 表取。
    let wm = scene
        .world_transforms
        .get(n.id.index())
        .copied()
        .unwrap_or(crate::transform::IDENTITY);
    let rect = if crate::transform::is_pure_translation(&wm) {
        crate::scene::node::Rect {
            x: wm[4],
            y: wm[5],
            w: n.layout_rect.w,
            h: n.layout_rect.h,
        }
    } else {
        crate::scene::node::Rect {
            x: 0.0,
            y: 0.0,
            w: n.layout_rect.w,
            h: n.layout_rect.h,
        }
    };
    let rect = &rect;
    let has_filter = n.style.color_filter.is_some();
    let color_matrix = n.style.color_filter.unwrap_or([0.0; 20]);
    let node_id = n.id.0;
    let parent_id = n.parent.map(|p| p.0);
    let color_tint = anim.and_then(|a| a.text_color).unwrap_or(n.style.color);
    let rn = match n.kind {
        // 控件外壳节点（不在 is_container）但渲染上需要一个背景框：
        // - Dropdown/OptionItem：combobox 外壳 / 选项列表项
        // - Toggle/RadioButton：空 div，勾选样式靠自身 [role]/[aria-checked] 的 background
        // - Slider/ProgressBar：轨道 / 底色（fill/thumb 子节点另自渲染）
        // pivot 后控件视觉子结构由作者按 role/data-slot 自写（core 不注入），
        // 这些控件自身必须画 background（否则空 div 形态的 Toggle/Radio 完全不可见）。
        // 文字由各自的 TextNode 子节点画，本臂不叠加文字。
        k if k.is_container()
            || matches!(
                k,
                NodeKind::Dropdown
                    | NodeKind::OptionItem
                    | NodeKind::Toggle
                    | NodeKind::RadioButton
                    | NodeKind::Slider
                    | NodeKind::ProgressBar
            ) =>
        {
            // rich-text-block 容器：背景框 + 文字 mesh 同帧推，提前返回（同 TextField
            // 文字控件模式）。背景占真 node_id（供 id_to_pos / sort_key 传播 / NativeHost
            // 查询），文字用 TF_TEXT_SYNTH_BYTE 合成 id 区分（C# MirrorPool 按 node_id
            // keying 独立 GO，不与背景互盖）。inline 子不在此递归（render 是平铺遍历），
            // 由主循环 is_folded_into_rich_text 跳过——它们已在 solve 期折进父的单段
            // inline flow，此处画进父 mesh 即它们的全部视觉。
            if n.rich_text_block {
                let bg = build_container_mesh(
                    visible,
                    n,
                    node_id,
                    parent_id,
                    rect,
                    wm,
                    alpha,
                    color_tint,
                    has_filter,
                    color_matrix,
                    anim,
                    image_sizes,
                );
                nodes.push(bg);
                if register_id_map {
                    id_to_pos.insert(n.id, nodes.len() - 1);
                }
                // 读 solve 期存的 TextLayout；缺则现编译 runs + measure_rich_text
                // 兜底（罕见，同 TextNode arm 的 measure_text 兜底防御）。
                let s = &n.style;
                let stack = fonts.stack_for(s.font_family.as_deref());
                let off_left =
                    resolve_lp(s.taffy_style.border.left) + resolve_lp(s.taffy_style.padding.left);
                let off_right = resolve_lp(s.taffy_style.border.right)
                    + resolve_lp(s.taffy_style.padding.right);
                let off_top =
                    resolve_lp(s.taffy_style.border.top) + resolve_lp(s.taffy_style.padding.top);
                // max_width 用 content width（rect.w - 左右 border/padding），与 TextNode arm
                // 同公式：文字在 content area 内断行 + 对齐，不溢出 box。
                let content_w = (rect.w - off_left - off_right).max(0.0);
                let mut layout = scene
                    .text_layouts
                    .get(n.id.index())
                    .cloned()
                    .flatten()
                    .unwrap_or_else(|| {
                        let runs =
                            crate::text::rich_compile::compile_rich_runs(scene, n.id, image_sizes);
                        crate::text::layout::measure_rich_text(
                            &runs,
                            Some(content_w),
                            s.effective_line_height(),
                            s.letter_spacing,
                            s.text_align,
                            s.wrap_control(),
                            &stack,
                        )
                    });
                if off_left != 0.0 || off_top != 0.0 {
                    bake_content_offset(&mut layout, off_left, off_top);
                }
                let meshes = build_text_mesh(
                    &layout,
                    atlas,
                    fonts,
                    rect,
                    &n.style.text_effects,
                    n.style.background_gradient.as_ref(),
                    n.style.background_clip_text,
                );
                push_text_meshes(
                    visible,
                    nodes,
                    id_to_pos,
                    meshes,
                    n,
                    node_id,
                    tf_synth_id(node_id, TF_TEXT_SYNTH_BYTE),
                    parent_id,
                    alpha,
                    color_tint,
                    wm,
                    false, // 背景已登记 n.id → id_to_pos，文字不重复注册
                );
                // box-shadow：rich-text-block 容器提前 return，须在此补推阴影层，否则
                // 背景 + 文字画了但阴影丢（功能缺失，同 Container 正常路径的 post-match 块）。
                let shadows = anim
                    .and_then(|a| a.box_shadow.as_ref())
                    .unwrap_or(&n.style.box_shadow);
                if n.kind.is_container() && !shadows.is_empty() {
                    push_container_shadows(
                        visible,
                        nodes,
                        back_layer_pairs,
                        node_id,
                        parent_id,
                        rect,
                        wm,
                        alpha,
                        color_tint,
                        shadows,
                        &n.style.border_radius,
                    );
                }
                return; // 背景 + 文字 + 阴影已推，跳过末尾 id_to_pos / push；inline 子由主循环跳过。
            }
            build_container_mesh(
                visible,
                n,
                node_id,
                parent_id,
                rect,
                wm,
                alpha,
                color_tint,
                has_filter,
                color_matrix,
                anim,
                image_sizes,
            )
        }
        NodeKind::Image => {
            let src = scene.image_srcs.get(&n.id).cloned().unwrap_or_default();
            let image_path = Some(src.clone());
            let uv_min = [0.0, 0.0];
            let uv_max = [1.0, 1.0];
            let (src_w, src_h) = src_size(image_sizes, &src);
            // bg-color 走 BG_COMPOSITE（shader source-over：图 over 底色，透明像素透出
            // 底色）——与 Container 同路径。无 bg-color 时 program 0（tex×白 = 原图）。
            let bg_opt = anim.and_then(|a| a.bg_color).or(n.style.background_color);
            let has_bg = bg_opt.map(|c| c[3] > 0.0).unwrap_or(false);
            let vertex_color = if has_bg { bg_opt.unwrap() } else { [1.0; 4] };
            let program = if has_filter {
                if has_bg {
                    4u32
                } else {
                    3u32
                }
            } else if has_bg {
                2u32
            } else {
                0u32
            };
            let (v, uvc, col, idx) = match &n.style.border_image_slice {
                Some(slice) => {
                    let resolved = resolve_slice_percent(slice, src_w, src_h);
                    crate::render::mesh::nine_slice(
                        rect,
                        vertex_color,
                        &resolved,
                        src_w,
                        src_h,
                        [uv_min[0], uv_max[1]],
                        [uv_max[0], uv_min[1]],
                    )
                }
                None => crate::render::mesh::quad(
                    rect,
                    vertex_color,
                    [uv_min[0], uv_max[1]],
                    [uv_max[0], uv_min[1]],
                ),
            };
            RenderNode {
                mount_root_id: 0,
                node_id,
                parent_id,
                visible,
                alpha,
                color_tint,
                world_matrix: wm,
                blend: BlendMode::Normal,
                mask_context: MaskContext(0),
                sort_key: 0,
                change_level: ChangeLevel::Full,
                reuse_key: n.reuse_key,
                effect: EffectBlock::default(),
                shadow_params: [0.0; 6],
                gradient: crate::render::gradient::GradientParams::default(),
                payload: NodePayload::Mesh {
                    verts: v,
                    uvs: uvc,
                    colors: col,
                    indices: idx,
                    image_path,
                    program,
                    color_matrix,
                },
            }
        }
        NodeKind::TextNode => {
            let content = scene.text_contents.get(&n.id).cloned().unwrap_or_default();
            // Text 节点单独处理：build_text_mesh 可能按 atlas 页号拆成多 Mesh，
            // 对应推多个 RenderNode（primary 用真 node_id，子页用合成 id）。
            // 避免 match arm 返回单个 RenderNode 的限制，在此处直接 push。
            let s = &n.style;
            let stack = fonts.stack_for(s.font_family.as_deref());
            let text_color = anim.and_then(|a| a.text_color).unwrap_or(s.color);

            let off_left =
                resolve_lp(s.taffy_style.border.left) + resolve_lp(s.taffy_style.padding.left);
            let off_right =
                resolve_lp(s.taffy_style.border.right) + resolve_lp(s.taffy_style.padding.right);
            let off_top =
                resolve_lp(s.taffy_style.border.top) + resolve_lp(s.taffy_style.padding.top);
            // max_width 用 content width（rect.w - 左右 border/padding），文字在 content area
            // 内断行 + 对齐，不溢出 box（修复前传 rect.w → 文字吃到 padding、右对齐/换行超框）。
            let content_w = (rect.w - off_left - off_right).max(0.0);
            let mut layout = scene
                .text_layouts
                .get(n.id.index())
                .cloned()
                .flatten()
                .unwrap_or_else(|| {
                    measure_text(
                        &content,
                        s.font_size,
                        s.effective_line_height(),
                        s.letter_spacing,
                        s.text_align,
                        s.wrap_control(),
                        Some(content_w),
                        &stack,
                        text_color,
                        crate::text::rich::weight_from_font_weight(s.font_weight),
                    )
                });
            if off_left != 0.0 || off_top != 0.0 {
                bake_content_offset(&mut layout, off_left, off_top);
            }
            let meshes = build_text_mesh(
                &layout,
                atlas,
                fonts,
                rect,
                &n.style.text_effects,
                n.style.background_gradient.as_ref(),
                n.style.background_clip_text,
            );
            push_text_meshes(
                visible,
                nodes,
                id_to_pos,
                meshes,
                n,
                node_id,
                node_id,
                parent_id,
                alpha,
                color_tint,
                wm,
                register_id_map,
            );
            return; // 直接推完，跳过末尾的 id_to_pos / push。
        }
        NodeKind::TextField | NodeKind::TextArea | NodeKind::NumberField => {
            // 控件叶子节点：先画背景框（与 Container 相同），再叠加 value/placeholder 文字。
            // 背景 RenderNode 先进 nodes，占住 id_to_pos（供 batch 子节点查找）；
            // 文字走 push_text_meshes 追加，register_id_map=false 避免覆盖背景位置。
            let bg_rn = build_container_mesh(
                visible,
                n,
                node_id,
                parent_id,
                rect,
                wm,
                alpha,
                color_tint,
                has_filter,
                color_matrix,
                anim,
                image_sizes,
            );
            nodes.push(bg_rn);
            if register_id_map {
                id_to_pos.insert(n.id, nodes.len() - 1);
            }
            // 取控件状态；无状态时跳过文字渲染（防御：控件未初始化或误入此臂）。
            // NumberField 是 TextField 的数值约束变体：edit 复用同一 EditState，
            // 故文字/光标/选区渲染与 TextField 完全一致（数值约束只作用在读写门）。
            let Some(
                ControlState::TextField(e)
                | ControlState::TextArea(e)
                | ControlState::NumberField { edit: e, .. },
            ) = scene.controls.get(n.id)
            else {
                return;
            };
            // 显示文本：value 优先（经 display_value 拼接 composition 预提交文本；掩码经
            // display_value_masked 逐字符替换），空时退到 placeholder。display_value 同时给出
            // composition 的 display 字节区间，供下划线对齐预提交文本。measure_text_controls
            // 缓存的 TextLayout 与这里 display 同源（同掩码变体），故文字 mesh 与下划线几何一致。
            let mask = n.style.text_security.map(crate::scene::control::mask_char);
            let (dv, comp_range) = crate::scene::control::display_value_masked(e, mask);
            let value_plain = e.value.clone();
            let is_placeholder = dv.is_empty();
            let display = if is_placeholder {
                e.placeholder.clone()
            } else {
                dv
            };
            let s = &n.style;
            // 单行水平视口（光标跟随滚动）：文字层整体左移 view_x（背景 border 不动）。
            // TextArea 恒 0。
            let vx = e.view_x;
            let stack = fonts.stack_for(s.font_family.as_deref());
            let base_text_color = anim.and_then(|a| a.text_color).unwrap_or(s.color);
            // placeholder 用占位色（声明的 placeholder-color，否则文字色折半，对齐浏览器
            // ::placeholder UA 默认）。与 layout solve 同公式——颜色在 layout 期烘焙进缓存
            // TextLayout，此处 fallback 须同色（见 placeholder_render_color）。
            let text_color = if is_placeholder {
                crate::style::resolved::placeholder_render_color(
                    s.placeholder_color,
                    base_text_color,
                )
            } else {
                base_text_color
            };
            let rect = Rect {
                x: rect.x - vx,
                ..*rect
            };
            let off_left =
                resolve_lp(s.taffy_style.border.left) + resolve_lp(s.taffy_style.padding.left);
            let off_right =
                resolve_lp(s.taffy_style.border.right) + resolve_lp(s.taffy_style.padding.right);
            let off_top =
                resolve_lp(s.taffy_style.border.top) + resolve_lp(s.taffy_style.padding.top);
            let content_w = (rect.w - off_left - off_right).max(0.0);
            // layout 期缓存（measure_text_controls 走同一份 display_value）与这里一致；
            // value 为空时 measure 跳过缓存（display 串为空），此处 lazy fallback 测 placeholder。
            let cached = scene.text_layouts.get(n.id.index()).cloned().flatten();
            let mut layout = cached.unwrap_or_else(|| {
                measure_text(
                    &display,
                    s.font_size,
                    s.effective_line_height(),
                    s.letter_spacing,
                    s.text_align,
                    crate::style::resolved::control_wrap_control(s),
                    Some(content_w),
                    &stack,
                    text_color,
                    crate::text::rich::weight_from_font_weight(s.font_weight),
                )
            });
            if off_left != 0.0 || off_top != 0.0 {
                bake_content_offset(&mut layout, off_left, off_top);
            }
            // 几何用上面烤过 content offset 的 `layout`；坐标与世界文字字形同系
            // （rect.xy + 局部）。各 mesh 用独立合成 node_id，避免与背景/文字 mesh 在
            // dirty hash 表碰撞。
            //
            // push 顺序 = 绘制层序（sort_key 升序，后绘者在上层）：选区 → 文字 →
            // composition → 光标。选区先于文字 push = 落在文字之下（标准编辑器行为：
            // 选区高亮作背景，选中文字保持清晰可读）。光标最后 push = 最上层
            // （caret 压在 composition 下划线与文字之上）。
            //
            // 缺省色：caret = caret-color style（缺省回退文字色），selection-bg =
            // selection-background style（缺省蓝半透），composition-underline = 文字色。
            // selection-color style 字段已解析存储，但选中文字的 per-run 着色需 text run
            // 拆分（独立于本臂的 quad 绘制），留待后续文本渲染细化。
            // 选区背景：sel_begin<sel_end 时逐行画覆盖选中文本的 quad。先于文字 push，
            // 使选区落在文字之下（文字清晰，选区作半透背景）。
            let (sel_b, sel_e) = e.selection_range();
            if sel_b < sel_e {
                let ranges = crate::scene::text_cursor::line_byte_ranges(&layout, &display);
                // sel_b/sel_e 是 value 字节偏移；掩码下显示串字节空间 ≠ value（1 char:1 char
                // 但字节宽变），按字符数换算后取像素几何（无掩码时换算恒等）。
                let sel_b_d =
                    crate::scene::control::value_to_display_byte(&value_plain, &display, sel_b);
                let sel_e_d =
                    crate::scene::control::value_to_display_byte(&value_plain, &display, sel_e);
                let (xb, lib) =
                    crate::scene::text_cursor::cursor_pixel_x(&layout, &ranges, sel_b_d);
                let (xe, lie) =
                    crate::scene::text_cursor::cursor_pixel_x(&layout, &ranges, sel_e_d);
                let sel_color = s.selection_background.unwrap_or([0.0, 0.0, 1.0, 0.5]); // 缺省蓝半透（CSS 未声明 selection-background 时）
                let mut verts = Vec::new();
                let mut uvs = Vec::new();
                let mut colors = Vec::new();
                let mut indices = Vec::new();
                for li in lib..=lie.min(layout.lines.len().saturating_sub(1)) {
                    let Some(line) = layout.lines.get(li) else {
                        break;
                    };
                    // 本行选区起点/终点（advance 相对，加 off_left 入世界）。
                    let line_x0 = if li == lib {
                        off_left + xb
                    } else {
                        off_left // 跨行选区中间行从行首开始
                    };
                    let line_x1 = if li == lie {
                        off_left + xe
                    } else {
                        off_left + line.width // 跨行选区中间行延至行末
                    };
                    if line_x1 <= line_x0 {
                        continue;
                    }
                    let (v, u, c, i) = crate::render::mesh::quad(
                        &Rect {
                            x: rect.x + line_x0,
                            y: rect.y + line.y,
                            w: line_x1 - line_x0,
                            h: line.height,
                        },
                        sel_color,
                        [0.0, 0.0],
                        [1.0, 1.0],
                    );
                    let base = verts.len() as u32;
                    verts.extend(v);
                    uvs.extend(u);
                    colors.extend(c);
                    indices.extend(i.iter().map(|&idx| idx + base));
                }
                if !verts.is_empty() {
                    push_solid_mesh(
                        visible,
                        nodes,
                        tf_synth_id(node_id, TF_SELECTION_SYNTH_BYTE),
                        parent_id,
                        wm,
                        alpha,
                        0,
                        verts,
                        uvs,
                        colors,
                        indices,
                    );
                }
            }
            let meshes = build_text_mesh(
                &layout,
                atlas,
                fonts,
                &rect,
                &n.style.text_effects,
                n.style.background_gradient.as_ref(),
                n.style.background_clip_text,
            );
            // 文字 mesh 用合成 id（TF_TEXT_SYNTH_BYTE）：背景框 mesh 已占真 node_id，若
            // 文字也用真 node_id 则 C# MirrorPool 同 node_id 唯一 GO 把文字覆盖背景（控件
            // 渲染残缺）。合成 id 让文字独立 GO；primary（低 56 位）仍 = node_id，供
            // sort_key 传播还原。register_id_map=false：背景已注册 n.id → id_to_pos。
            push_text_meshes(
                visible,
                nodes,
                id_to_pos,
                meshes,
                n,
                node_id,
                tf_synth_id(node_id, TF_TEXT_SYNTH_BYTE),
                parent_id,
                alpha,
                text_color,
                wm,
                false, // 背景已注册 n.id → id_to_pos，文字不重复注册
            );
            // composition 下划线：有 composition 时在 composition 段下方画 2px 横线。
            // 区间取 display_value 返回的 comp_range（display 坐标里 composition 的真实字节
            // 区间），而非 raw comp.pos——后者可能落在多字节字符中间，导致下划线错位。
            // comp_range 由 char 计数对齐生成，精确覆盖预提交文本。
            if let Some((comp_start, comp_end)) = comp_range {
                if comp_end > comp_start {
                    let ranges = crate::scene::text_cursor::line_byte_ranges(&layout, &display);
                    let (xs, lis) =
                        crate::scene::text_cursor::cursor_pixel_x(&layout, &ranges, comp_start);
                    let (xe, lie) =
                        crate::scene::text_cursor::cursor_pixel_x(&layout, &ranges, comp_end);
                    let ul_color = text_color; // 缺省下划线色 = 文字色
                    let mut verts = Vec::new();
                    let mut uvs = Vec::new();
                    let mut colors = Vec::new();
                    let mut indices = Vec::new();
                    for li in lis..=lie.min(layout.lines.len().saturating_sub(1)) {
                        let Some(line) = layout.lines.get(li) else {
                            break;
                        };
                        let line_x0 = if li == lis { off_left + xs } else { off_left };
                        let line_x1 = if li == lie {
                            off_left + xe
                        } else {
                            off_left + line.width
                        };
                        if line_x1 <= line_x0 {
                            continue;
                        }
                        // 下划线贴行底（line.y + line.height），2px 厚。
                        let (v, u, c, i) = crate::render::mesh::quad(
                            &Rect {
                                x: rect.x + line_x0,
                                y: rect.y + line.y + line.height - 2.0,
                                w: line_x1 - line_x0,
                                h: 2.0,
                            },
                            ul_color,
                            [0.0, 0.0],
                            [1.0, 1.0],
                        );
                        let base = verts.len() as u32;
                        verts.extend(v);
                        uvs.extend(u);
                        colors.extend(c);
                        indices.extend(i.iter().map(|&idx| idx + base));
                    }
                    if !verts.is_empty() {
                        push_solid_mesh(
                            visible,
                            nodes,
                            tf_synth_id(node_id, TF_COMPOSITION_SYNTH_BYTE),
                            parent_id,
                            wm,
                            alpha,
                            0,
                            verts,
                            uvs,
                            colors,
                            indices,
                        );
                    }
                }
            }
            // 光标（focused + cursor_visible + !readonly）：1px × 行高 caret。最后 push =
            // 最上层（sort_key 升序 = 后绘者在上，caret 压在选区/下划线之上）。
            if scene.focused_node == Some(n.id) && !e.readonly && e.cursor_visible {
                let ranges = crate::scene::text_cursor::line_byte_ranges(&layout, &display);
                // e.cursor 是 value 字节偏移；掩码下显示串字节空间 ≠ value，按字符数
                // 换算（无掩码/无 composition 时恒等）。
                let cursor_d =
                    crate::scene::control::value_to_display_byte(&value_plain, &display, e.cursor);
                let (cx, li) =
                    crate::scene::text_cursor::cursor_pixel_x(&layout, &ranges, cursor_d);
                if let Some(line) = layout.lines.get(li) {
                    // cx 是 advance 累计（内容区相对，不含 off_left）；line.y 已含 off_top。
                    let x = rect.x + off_left + cx;
                    let y = rect.y + line.y;
                    push_solid_quad(
                        visible,
                        nodes,
                        tf_synth_id(node_id, TF_CURSOR_SYNTH_BYTE),
                        parent_id,
                        wm,
                        alpha,
                        0, // 合成 id 不继承主节点 reuse_key（独立 GO）
                        x,
                        y,
                        1.0, // 1px caret 宽
                        line.height,
                        s.caret_color.unwrap_or(text_color), // caret-color style（缺省回退文字色）
                    );
                }
            }
            return;
        }
        _ => RenderNode {
            mount_root_id: 0,
            node_id,
            parent_id,
            visible,
            alpha,
            color_tint,
            world_matrix: wm,
            blend: BlendMode::Normal,
            mask_context: MaskContext(0),
            sort_key: 0,
            change_level: ChangeLevel::Full,
            reuse_key: n.reuse_key,
            effect: EffectBlock::default(),
            shadow_params: [0.0; 6],
            gradient: crate::render::gradient::GradientParams::default(),
            payload: NodePayload::Mesh {
                verts: vec![],
                uvs: vec![],
                colors: vec![],
                indices: vec![],
                image_path: None,
                program: 0,
                color_matrix,
            },
        },
    };
    // box-shadow：每层一个合成 RenderNode。outer（外阴影）画在 primary 之下（sort_key
    // < primary，经 propagate_back_shadow_sort_keys 调整）；inset（内阴影）画在 primary
    // 之上、子节点之下（经 propagate_text_sub_page_sort_keys 调整）。统一 SDF 路径
    // （program=5 + shadow_params）：inset 用元素自身 rounded_rect mesh 几何裁圆角，
    // outer 用 shape+3σ pad quad；shader smoothstep 双侧软边。
    //
    // CSS 层序：同一 primary 内，先列出的 outer 层画在最顶（最贴 primary 下），先列出的
    // inset 层画在最顶（最离 primary 上）。outer 按 CSS 序 push（propagate_back_shadow
    // 按 CSS 序赋最高=primary-1 给首层）；inset 按 CSS 逆序 push（propagate_text_sub_page
    // 按 push 序赋 offset，逆序 push 使首层得最大 offset=最上）。
    let shadows = anim
        .and_then(|a| a.box_shadow.as_ref())
        .unwrap_or(&n.style.box_shadow);
    if n.kind.is_container() && !shadows.is_empty() {
        push_container_shadows(
            visible,
            nodes,
            back_layer_pairs,
            node_id,
            parent_id,
            rect,
            wm,
            alpha,
            color_tint,
            shadows,
            &n.style.border_radius,
        );
    }
    if register_id_map {
        id_to_pos.insert(n.id, nodes.len());
    }
    nodes.push(rn);
}

/// Emit one synthetic RenderNode per box-shadow layer (outer back-layers + inset front-layers)
/// for a container. Shared by the normal Container arm (called after the match) and the
/// rich-text-block arm (called before its early `return`, which otherwise skips the post-match
/// shadow block). Each layer is a single SDF shadow quad (program=5 + shadow_params); sort_key
/// stays 0 here and is reassigned later by propagate_back_shadow_sort_keys (outer) and
/// propagate_text_sub_page_sort_keys (inset). No-op when `shadows` is empty.
///
/// CSS 层序：outer 按 CSS 序 push（首层最贴 primary 下）；inset 按 CSS 逆序 push（首层
/// 最离 primary 上）。outer 层进 back_layer_pairs 供 back-shadow sort_key 传播；inset 层
/// 不进（由 front-shadow 传播按 is_front_shadow_synth 自动收集）。
fn push_container_shadows(
    visible: bool,
    nodes: &mut Vec<RenderNode>,
    back_layer_pairs: &mut Vec<(u64, u64)>,
    node_id: u64,
    parent_id: Option<u64>,
    rect: &Rect,
    wm: crate::transform::Affine2,
    alpha: f32,
    color_tint: [f32; 4],
    shadows: &[crate::style::resolved::BoxShadow],
    border_radius: &crate::style::resolved::BorderRadius,
) {
    let radii = border_radius.as_corners(rect.w, rect.h);
    // 超限层兜底跳过：打包期 fence 已拒收，但运行时 inline override 注入的 CSS 不经
    // 打包期校验——超限层合成 id 会撞相邻编码区（错层序/漏 mask 传播），宁可不画。
    // outer（back）层：CSS 序 push。
    for (i, sh) in shadows
        .iter()
        .filter(|s| !s.inset)
        .take(crate::style::resolved::MAX_OUTER_SHADOW_LAYERS)
        .enumerate()
    {
        let sigma = shadow_sigma(sh.blur);
        let sid = back_shadow_id(node_id, i as u32);
        let (v, uvc, col, idx, params) =
            crate::render::border::shadow_quad(rect, &radii, sh, sigma);
        if v.is_empty() {
            continue;
        }
        back_layer_pairs.push((node_id, sid));
        nodes.push(RenderNode {
            mount_root_id: 0,
            node_id: sid,
            parent_id,
            visible,
            alpha,
            color_tint,
            world_matrix: wm,
            blend: BlendMode::Normal,
            mask_context: MaskContext(0),
            sort_key: 0, // propagate_back_shadow_sort_keys 后重调
            change_level: ChangeLevel::Full,
            reuse_key: 0,
            effect: EffectBlock::default(),
            shadow_params: params,
            gradient: crate::render::gradient::GradientParams::default(),
            payload: NodePayload::Mesh {
                verts: v,
                uvs: uvc,
                colors: col,
                indices: idx,
                image_path: None,
                program: 5,
                color_matrix: [0.0; 20],
            },
        });
    }
    // inset（front）层：CSS 逆序 push。idx 仍用 CSS 序（保 id 唯一 + 可调试反查）。
    let inset_layers: Vec<(usize, &crate::style::resolved::BoxShadow)> = shadows
        .iter()
        .enumerate()
        .filter(|(_, s)| s.inset)
        .take(crate::style::resolved::MAX_INSET_SHADOW_LAYERS)
        .collect();
    for &(css_idx, sh) in inset_layers.iter().rev() {
        let sigma = shadow_sigma(sh.blur);
        // inset idx = 该 primary 内 inset 层的 CSS 序（0-based，区别于混合序）。
        let inset_idx = shadows.iter().take(css_idx).filter(|s| s.inset).count() as u32;
        let sid = front_shadow_id(node_id, inset_idx);
        let (v, uvc, col, idx, params) =
            crate::render::border::shadow_quad(rect, &radii, sh, sigma);
        if v.is_empty() {
            continue;
        }
        // front 层不经 back_layer_pairs——由 propagate_text_sub_page_sort_keys 按
        // is_front_shadow_synth 自动收集并赋 sort_key（嵌入 primary 之后、下一真节点之前）。
        nodes.push(RenderNode {
            mount_root_id: 0,
            node_id: sid,
            parent_id,
            visible,
            alpha,
            color_tint,
            world_matrix: wm,
            blend: BlendMode::Normal,
            mask_context: MaskContext(0),
            sort_key: 0, // propagate_text_sub_page_sort_keys 后重调
            change_level: ChangeLevel::Full,
            reuse_key: 0,
            effect: EffectBlock::default(),
            shadow_params: params,
            gradient: crate::render::gradient::GradientParams::default(),
            payload: NodePayload::Mesh {
                verts: v,
                uvs: uvc,
                colors: col,
                indices: idx,
                image_path: None,
                program: 5,
                color_matrix: [0.0; 20],
            },
        });
    }
}
#[cfg(test)]
mod tests;
