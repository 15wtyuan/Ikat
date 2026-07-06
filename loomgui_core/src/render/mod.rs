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
pub mod dirty; // dirty hash（header_hash + payload_hash 双轴，跨帧比决定 ChangeLevel）
pub mod merge;
pub mod mesh;
pub mod node;

use crate::layout::ImageSizeTable;
use crate::scene::node::{NodeId, NodeKind, Rect, Scene};
use crate::text::layout::{measure_text, FontTable};
use node::*;

use taffy::style::LengthPercentage;

/// 查图尺寸表取 src_w/src_h（fallback 64×64）。
/// path 缺失或 w/h=0 → 64.0 兜底（核心不知图集，但知图尺寸）。
fn src_size(image_sizes: &ImageSizeTable, path: &str) -> (f32, f32) {
    image_sizes
        .get(path)
        .filter(|(w, h)| *w != 0 && *h != 0)
        .map(|&(w, h)| (w as f32, h as f32))
        .unwrap_or((64.0, 64.0))
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

/// clip 表条目：context_id（mask_context>0 的层级）→ 该层级的交集绝对 design rect。
///
/// 由 `batch::assign_sort_keys` 在 DFS 时产；`context_id` 与 RenderNode 的
/// `mask_context.0` 对齐（被该 clip 约束的节点引用同一 id）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipEntry {
    pub context_id: u32,
    pub rect: Rect,
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
fn thumb_render_node(node_id: u32, rect: Rect, sort_key: u32) -> RenderNode {
    let (v, uvc, col, idx) =
        crate::render::mesh::quad(&rect, [0.6, 0.6, 0.6, 0.6], [0.0, 0.0], [1.0, 1.0]);
    RenderNode {
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
/// （slice_px / src_px）。path 缺失或 w/h=0 → 64×64 兜底。
pub fn build_render_nodes(
    scene: &Scene,
    fonts: &FontTable,
    prev: &std::collections::HashMap<u32, (u64, u64)>,
    image_sizes: &ImageSizeTable,
) -> (
    FrameData,
    std::collections::HashMap<u32, (u64, u64)>,
    Vec<u32>,
) {
    // id_to_pos: NodeId → nodes vec 0 基位置。剪 display:none 子树后 nodes 与 scene.nodes
    // 不等长，batch 按此映射索引 nodes；pruned 节点不入表（batch DFS 遇 id_to_pos 没有
    // 的节点即跳过该子树）。
    let mut id_to_pos: std::collections::HashMap<NodeId, usize> = std::collections::HashMap::new();
    // 直接逐节点构造真 RenderNode。change_level 先占 Full，末尾统一定级。
    // 先剪 display:none 子树——display:none 节点 + 后代不产 RenderNode（CSS 语义）。
    let pruned = collect_display_none_subtree(scene);
    let mut nodes: Vec<RenderNode> = Vec::new();
    for n in scene.nodes.values() {
        if pruned.contains(&n.id) {
            continue;
        }
        let anim = scene.anim.get(n.id);
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
        let alpha = anim.and_then(|a| a.opacity).unwrap_or(n.style.opacity);
        let color_tint = anim.and_then(|a| a.text_color).unwrap_or(n.style.color);
        let rn = match &n.kind {
            NodeKind::Container | NodeKind::Button => {
                let color = anim
                    .and_then(|a| a.bg_color)
                    .unwrap_or(n.style.background_color.unwrap_or([0.0, 0.0, 0.0, 0.0]));
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
                let resolve = |lp: LengthPercentage, side: f32| -> f32 {
                    match lp {
                        LengthPercentage::Length(v) => v,
                        LengthPercentage::Percent(p) => side * p,
                    }
                };
                let (rw, rh) = (rect.w, rect.h);
                let bc = &n.style.border_radius.corners;
                let radii = [
                    (resolve(bc[0].h, rw), resolve(bc[0].v, rh)),
                    (resolve(bc[1].h, rw), resolve(bc[1].v, rh)),
                    (resolve(bc[2].h, rw), resolve(bc[2].v, rh)),
                    (resolve(bc[3].h, rw), resolve(bc[3].v, rh)),
                ];
                let all_zero = radii.iter().all(|&(rx, ry)| rx <= 0.0 || ry <= 0.0);
                let has_slice = n.style.border_image_slice.is_some();
                let draw_rect = if !has_slice
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
                let (v, uvc, col, idx) = match (has_slice, all_zero) {
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
                    (true, true) => crate::render::mesh::nine_slice(
                        rect,
                        color,
                        n.style.border_image_slice.as_ref().unwrap(),
                        src_w,
                        src_h,
                        [u_min[0], u_max[1]],
                        [u_max[0], u_min[1]],
                    ),
                    (true, false) => crate::render::mesh::nine_slice_rounded(
                        rect,
                        color,
                        n.style.border_image_slice.as_ref().unwrap(),
                        &radii,
                        src_w,
                        src_h,
                        [u_min[0], u_max[1]],
                        [u_max[0], u_min[1]],
                    ),
                };
                let program = if has_filter {
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
                    node_id,
                    parent_id,
                    visible: true,
                    alpha,
                    color_tint,
                    world_matrix: wm,
                    blend: BlendMode::Normal,
                    mask_context: MaskContext(0),
                    sort_key: 0,
                    change_level: ChangeLevel::Full,
                    reuse_key: n.reuse_key,
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
            NodeKind::Image { src } => {
                let image_path = Some(src.clone());
                let uv_min = [0.0, 0.0];
                let uv_max = [1.0, 1.0];
                let (src_w, src_h) = src_size(image_sizes, src);
                let (v, uvc, col, idx) = match &n.style.border_image_slice {
                    Some(slice) => crate::render::mesh::nine_slice(
                        rect,
                        [1.0; 4],
                        slice,
                        src_w,
                        src_h,
                        [uv_min[0], uv_max[1]],
                        [uv_max[0], uv_min[1]],
                    ),
                    None => crate::render::mesh::quad(
                        rect,
                        [1.0, 1.0, 1.0, 1.0],
                        [uv_min[0], uv_max[1]],
                        [uv_max[0], uv_min[1]],
                    ),
                };
                let program = if has_filter { 3u32 } else { 0u32 };
                RenderNode {
                    node_id,
                    parent_id,
                    visible: true,
                    alpha,
                    color_tint,
                    world_matrix: wm,
                    blend: BlendMode::Normal,
                    mask_context: MaskContext(0),
                    sort_key: 0,
                    change_level: ChangeLevel::Full,
                    reuse_key: n.reuse_key,
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
            NodeKind::Text { content } => {
                let s = &n.style;
                let font = fonts.select(s.font_family.as_deref());
                let mut layout = scene
                    .text_layouts
                    .get(n.id.index())
                    .cloned()
                    .flatten()
                    .unwrap_or_else(|| {
                        measure_text(
                            content,
                            s.font_size,
                            s.line_height,
                            s.letter_spacing,
                            s.text_align,
                            s.white_space_nowrap,
                            Some(rect.w),
                            font,
                        )
                    });
                let off_x =
                    resolve_lp(s.taffy_style.border.left) + resolve_lp(s.taffy_style.padding.left);
                let off_y =
                    resolve_lp(s.taffy_style.border.top) + resolve_lp(s.taffy_style.padding.top);
                if off_x != 0.0 || off_y != 0.0 {
                    bake_content_offset(&mut layout, off_x, off_y);
                }
                RenderNode {
                    node_id,
                    parent_id,
                    visible: true,
                    alpha,
                    color_tint,
                    world_matrix: wm,
                    blend: BlendMode::Normal,
                    mask_context: MaskContext(0),
                    sort_key: 0,
                    change_level: ChangeLevel::Full,
                    reuse_key: n.reuse_key,
                    payload: NodePayload::Text {
                        layout,
                        font_size: s.font_size,
                        color: anim.and_then(|a| a.text_color).unwrap_or(s.color),
                        program: 1,
                        family: s.font_family.clone(),
                    },
                }
            }
        };
        id_to_pos.insert(n.id, nodes.len());
        nodes.push(rn);
    }
    // batch / merge / thumb
    // sort_keys buffer：按 NodeId.index() 索引（capacity+1，对齐 world_transforms 扩容——
    // slotmap 删后 idx 不变，按 capacity 不按 len）。assign_sort_keys 在 DFS 时填每个节点的
    // pre-merge 序号；merge_meshes 后空 div 的 RenderNode entry 会被吃掉，但 sort_keys
    // 快照保留供 NativeHost FFI 查询。
    let mut sort_keys: Vec<u32> = vec![0u32; scene.nodes.capacity() + 1];
    let clips = batch::assign_sort_keys(scene, &mut nodes, &id_to_pos, &mut sort_keys);
    let max_sort = nodes.iter().map(|n| n.sort_key).max().unwrap_or(0);
    batch::reorder_for_batching(scene, &mut nodes);
    let mut nodes = merge::merge_meshes(nodes);
    // 合成 scrollbar thumb
    for n in scene.nodes.values() {
        let nid = n.id;
        if let Some(s) = scene.scroll.get(nid) {
            if crate::scroll::effective(n.style.overflow_y, s.content_size.1, s.viewport_size.1) {
                if let Some(r) = crate::scroll::v_thumb_rect(scene, nid) {
                    let thumb_id = nid.0 | crate::scroll::V_THUMB_FLAG;
                    nodes.push(thumb_render_node(thumb_id, r, max_sort + 1));
                }
            }
            if crate::scroll::effective(n.style.overflow_x, s.content_size.0, s.viewport_size.0) {
                if let Some(r) = crate::scroll::h_thumb_rect(scene, nid) {
                    let thumb_id = nid.0 | crate::scroll::H_THUMB_FLAG;
                    nodes.push(thumb_render_node(thumb_id, r, max_sort + 1));
                }
            }
        }
    }
    // merge 后按 node_id 算双 hash → 定级别
    let mut new_hashes = std::collections::HashMap::with_capacity(nodes.len());
    for rn in &mut nodes {
        let hh = crate::render::dirty::header_hash(rn);
        let ph = crate::render::dirty::payload_hash(rn);
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

/// 把 taffy `LengthPercentage` 解析为 px。
///
/// - `Length(v)` → v。
/// - `Percent(_)` → 0.0。**已知缺口**（记 ledger）：渲染阶段无父 content-box 宽度上下文，
///   无法解析百分比的 padding/border。`style::mapping::parse_four` 对 padding/border
///   只产 `Length`（裸数字/px），故实际不会命中 Percent 分支；若未来 CSS 允许百分比
///   padding/border，需在 layout 阶段把解析结果写回 ResolvedStyle。
fn resolve_lp(lp: LengthPercentage) -> f32 {
    match lp {
        LengthPercentage::Length(v) => v,
        LengthPercentage::Percent(_) => 0.0,
    }
}

/// 烤 content 偏移进 TextLayout 每个 glyph 的 (x, y)（pen = GO-local）。
/// layout 是刚由 measure_text 产的 owned 值，直接 mutate。
fn bake_content_offset(layout: &mut crate::text::layout::TextLayout, off_x: f32, off_y: f32) {
    for line in &mut layout.lines {
        for run in &mut line.runs {
            for g in &mut run.glyphs {
                g.x += off_x;
                g.y += off_y;
            }
        }
    }
}

#[cfg(test)]
mod tests;
