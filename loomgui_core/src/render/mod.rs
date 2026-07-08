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
use crate::text::atlas::{GlyphAtlas, GlyphKey};
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
    atlas: &mut GlyphAtlas,
) -> (
    FrameData,
    std::collections::HashMap<u32, (u64, u64)>,
    Vec<u32>,
) {
    // id_to_pos: NodeId → nodes vec 0 基位置。剪 display:none 子树后 nodes 与 scene.nodes
    // 不等长，batch 按此映射索引 nodes；pruned 节点不入表（batch DFS 遇 id_to_pos 没有
    // 的节点即跳过该子树）。
    let mut id_to_pos: std::collections::HashMap<NodeId, usize> = std::collections::HashMap::new();
    // 合成 node_id 方案依赖真实节点 index < 4096（bits [31:12] 全零时 bits [31:24] = 0，
    // 不与子页的 page 编码冲突）。真实场景节点超 4096 时合成 id 会与真节点 id 碰撞。
    // 硬上限：build 时如果任何真节点 index >= 4096，debug 构建 panic，release 继续
    // （此时 is_text_sub_page 可能误判真节点为子页，sort_key 乱序）。
    debug_assert!(
        scene.nodes.values().all(|n| (n.id.0 >> 12) < 4096),
        "node index >= 4096: synthetic text sub-page id scheme hard limit exceeded. \
         See synth_text_node_id / is_text_sub_page in render/mod.rs."
    );
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
                // Text 节点单独处理：build_text_mesh 可能按 atlas 页号拆成多 Mesh，
                // 对应推多个 RenderNode（primary 用真 node_id，子页用合成 id）。
                // 避免 match arm 返回单个 RenderNode 的限制，在此处直接 push。
                let s = &n.style;
                let font = fonts.select(s.font_family.as_deref());
                let text_color = anim.and_then(|a| a.text_color).unwrap_or(s.color);
                let font_id = fonts.font_id(s.font_family.as_deref());
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
                            font_id,
                            text_color,
                        )
                    });
                let off_x =
                    resolve_lp(s.taffy_style.border.left) + resolve_lp(s.taffy_style.padding.left);
                let off_y =
                    resolve_lp(s.taffy_style.border.top) + resolve_lp(s.taffy_style.padding.top);
                if off_x != 0.0 || off_y != 0.0 {
                    bake_content_offset(&mut layout, off_x, off_y);
                }
                let meshes = build_text_mesh(&layout, atlas, font, font_id);
                push_text_meshes(
                    &mut nodes,
                    &mut id_to_pos,
                    meshes,
                    n,
                    node_id,
                    parent_id,
                    alpha,
                    color_tint,
                    wm,
                    font_id,
                );
                continue; // 直接推完，跳过末尾的 id_to_pos / push。
            }
            NodeKind::RichText { runs } => {
                // RichText 与 Text 同走 build_text_mesh（per-run 样式从 GlyphRun 读）。
                // MVP 单字体：所有 run 共用节点 font_family 选的 face + default_font_id；
                // 合成 bold（双绘 +1px）/italic（quad 顶边右偏）在 build 期几何化。
                let s = &n.style;
                let font = fonts.select(s.font_family.as_deref());
                let default_font_id = fonts.font_id(s.font_family.as_deref());
                let mut layout = scene
                    .text_layouts
                    .get(n.id.index())
                    .cloned()
                    .flatten()
                    .unwrap_or_else(|| {
                        crate::text::layout::measure_rich_text(
                            runs,
                            Some(rect.w),
                            s.line_height,
                            font,
                            default_font_id,
                        )
                    });
                let off_x =
                    resolve_lp(s.taffy_style.border.left) + resolve_lp(s.taffy_style.padding.left);
                let off_y =
                    resolve_lp(s.taffy_style.border.top) + resolve_lp(s.taffy_style.padding.top);
                if off_x != 0.0 || off_y != 0.0 {
                    bake_content_offset(&mut layout, off_x, off_y);
                }
                let meshes = build_text_mesh(&layout, atlas, font, default_font_id);
                push_text_meshes(
                    &mut nodes,
                    &mut id_to_pos,
                    meshes,
                    n,
                    node_id,
                    parent_id,
                    alpha,
                    color_tint,
                    wm,
                    default_font_id,
                );
                continue;
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
    // 跨页 text 子页 sort_key 传播：assign_sort_keys 只认识真 scene 节点（经 id_to_pos 映射），
    // 不认识合成子页。此处把子页 sort_key 设为 primary.sort_key + page_idx，并把后续真节点
    // 的 sort_key 后移子页个数，保持单调连续。
    propagate_text_sub_page_sort_keys(&mut nodes, &id_to_pos);
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

/// 合成 node_id：为跨页 text 子页生成区别于主节点的 id。
/// 编码：bits [31:24] = 子页号（1..255），bits [23:0] = primary_id 的低 24 位。
/// 真实场景 node index < 2^12 时 bits [23:12] 均为零，不会与其它 scene 节点碰撞。
fn synth_text_node_id(primary_id: u32, sub_page: u32) -> u32 {
    (primary_id & 0x00FF_FFFF) | ((sub_page & 0xFF) << 24)
}

/// 判断 node_id 是否为跨页 text 子页（bits [31:24] 非零）。
fn is_text_sub_page(node_id: u32) -> bool {
    (node_id >> 24) > 0
}

/// 提取跨页 text 子页对应的主节点 id。
fn text_sub_primary_id(node_id: u32) -> u32 {
    node_id & 0x00FF_FFFF
}

/// 提取跨页 text 子页的页号（1 基）。
fn text_sub_page_idx(node_id: u32) -> u32 {
    node_id >> 24
}

/// 跨页 text 子页 sort_key 传播 + 后续真节点 sort_key 后移。
///
/// assign_sort_keys 只给 `id_to_pos` 中的真 scene 节点赋 sort_key；合成子页保持 0。
/// 此函数：
/// 1. 统计每对 (primary_sort_key, num_sub_pages)
/// 2. 后续真节点 sort_key 后移 num_sub_pages
/// 3. 子页 sort_key = primary.sort_key + page_idx, mask_context = primary.mask_context
///
/// 步骤 2 保证子页 sort_key 嵌入 primary 与下一个真节点之间，保持单调连续。
fn propagate_text_sub_page_sort_keys(
    nodes: &mut [RenderNode],
    id_to_pos: &std::collections::HashMap<NodeId, usize>,
) {
    // 统计：真 node_id 为 key 的 primary text 节点及其子页数。
    let mut sub_counts: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    for rn in nodes.iter() {
        if is_text_sub_page(rn.node_id) {
            let primary = text_sub_primary_id(rn.node_id);
            *sub_counts.entry(primary).or_default() += 1;
        }
    }
    if sub_counts.is_empty() {
        return;
    }
    // 按 primary sort_key 排序（从小到大），保证 shift 按序累积。
    let mut shifts: Vec<(u32, u32)> = sub_counts
        .iter()
        .filter_map(|(&primary, &count)| {
            id_to_pos
                .get(&NodeId(primary))
                .map(|&pos| (nodes[pos].sort_key, count))
        })
        .collect();
    shifts.sort_by_key(|&(sk, _)| sk);
    // 后移后续真节点 sort_key。使用累积偏移避免 stale primary_sk：shifts 中的
    // primary_sk 是排序前采集的快照；当存在多个带子页的 text 节点时，先处理的节点
    // 已把后续节点（含后面的 text 节点）的 sort_key 向后推，此时再用原始 primary_sk
    // 比较会误判区间，造成 sort_key tie。
    let mut cum_shift: u32 = 0;
    for (primary_sk, n) in &shifts {
        let adjusted_sk = *primary_sk + cum_shift;
        cum_shift += *n;
        for rn in nodes.iter_mut() {
            if is_text_sub_page(rn.node_id) {
                continue;
            }
            if rn.sort_key > adjusted_sk {
                rn.sort_key += n;
            }
        }
    }
    // 传播子页 sort_key + mask_context。
    for &primary in sub_counts.keys() {
        let pos = match id_to_pos.get(&NodeId(primary)) {
            Some(&p) => p,
            None => continue,
        };
        let primary_sk = nodes[pos].sort_key;
        let primary_mask = nodes[pos].mask_context;
        for rn in nodes.iter_mut() {
            if text_sub_primary_id(rn.node_id) == primary && is_text_sub_page(rn.node_id) {
                let page = text_sub_page_idx(rn.node_id);
                rn.sort_key = primary_sk + page;
                rn.mask_context = primary_mask;
            }
        }
    }
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

/// 把 TextLayout 每字形展成 quad mesh：4 顶点 + 6 索引，UV 指向核心 atlas。
/// 顶点 = pen 坐标 + glyph bbox（bearing）；per-run 颜色烤顶点色（alpha 不烤，走
/// _Alpha uniform）。索引为 2-tri 扇（0-1-2, 0-2-3，与 mesh::quad 同序）。
///
/// per-run 样式（color/font_id/font_size/weight/style）从 `GlyphRun` 读——plain text
/// 是单 run（整段同色），rich text 多 run 各自带色。合成 bold/italic 在此期几何化：
/// - bold：双绘（+1px x 偏移重画一遍），无字体变体。
/// - italic：quad 顶边右偏 0.3×字形高（skew），底边不动。
///
/// V-flip 方向：atlas v0=顶(v0)、v1=底(v1)；quad 底采样 atlas 底(v1)、
/// 顶采样 atlas 顶(v0)。与现有 Image quad UV 翻转模式一致（quad BL→(u0,v1)、
/// TR→(u1,v0)）。PlayMode 实测校准（如果 text 上下颠倒，翻转 UV v 方向）。
///
/// 返回按 atlas 页号分组的 mesh 列表。单字体单字号常见一页装下 → 一项（单 draw call）；
/// 超 CJK 字符集才跨页 → 多项（每项独立 image_path = f<id>/p<page>）。
///
/// `default_font_id`：节点 font_family 对应的 font_id（atlas key + 合成 image_path 用）。
/// MVP 单字体：所有 run 共用此 face，`GlyphRun.font_id` 字段保留给将来多 family，本函数不读它。
fn build_text_mesh(
    layout: &crate::text::layout::TextLayout,
    atlas: &mut GlyphAtlas,
    font: &crate::text::layout::Font,
    default_font_id: u32,
) -> Vec<(u32, Vec<[f32; 2]>, Vec<[f32; 2]>, Vec<[f32; 4]>, Vec<u32>)> {
    use std::collections::BTreeMap;
    let face = &font.face;
    // 按 atlas 页号分组：每页独立 mesh。存 per-glyph 展开所需全部数据（含 per-run 色/样式）。
    let mut pages: BTreeMap<u32, (Vec<[f32; 2]>, Vec<[f32; 2]>, Vec<[f32; 4]>, Vec<u32>)> =
        BTreeMap::new();
    for line in &layout.lines {
        for run in &line.runs {
            let size_px = run.font_size.round().max(1.0) as u16;
            let italic_skew = if matches!(run.style, crate::text::rich::RichStyle::Italic) {
                0.3
            } else {
                0.0
            };
            // 合成 bold：双绘（offset 0 + offset +1px），模拟加粗，无字体变体。
            let bold_offsets: &[f32] = if matches!(run.weight, crate::text::rich::RichWeight::Bold)
            {
                &[0.0, 1.0]
            } else {
                &[0.0]
            };
            for g in &run.glyphs {
                let key = GlyphKey {
                    font_id: default_font_id,
                    glyph_id: g.glyph_id,
                    size_px,
                    effect_sig: 0,
                };
                let r = atlas.ensure(face, key);
                // 合成 italic：quad 顶边右偏（skew × 字形高），底边不动。
                let skew_top = italic_skew * r.px_h as f32;
                let p = pages
                    .entry(r.page)
                    .or_insert_with(|| (Vec::new(), Vec::new(), Vec::new(), Vec::new()));
                for &boff in bold_offsets {
                    let left = g.x + g.bearing_x + boff;
                    let top = line.baseline + g.bearing_y;
                    let right = left + r.px_w as f32;
                    let bottom = top - r.px_h as f32;
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
                    for _ in 0..4 {
                        p.2.push(run.color);
                    }
                    p.3.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
                }
            }
        }
    }
    pages
        .into_iter()
        .map(|(pg, (v, u, c, i))| (pg, v, u, c, i))
        .collect()
}

/// 把 `build_text_mesh` 产出的多页 mesh 列表推入 `nodes`，复用 Text/RichText 共同的
/// 跨页子页机制：首页（page 0）用真 node_id，后续页用 `synth_text_node_id` 合成 id。
/// 子页 reuse_key=0（独立 GO，不继承主节点——见 synth_text_node_id 注释）。
///
/// 抽成 helper 让 Text arm 与 RichText arm 共用，避免复制粘贴 push 逻辑。
fn push_text_meshes(
    nodes: &mut Vec<RenderNode>,
    id_to_pos: &mut std::collections::HashMap<NodeId, usize>,
    meshes: Vec<(u32, Vec<[f32; 2]>, Vec<[f32; 2]>, Vec<[f32; 4]>, Vec<u32>)>,
    n: &crate::scene::node::Node,
    node_id: u32,
    parent_id: Option<u32>,
    alpha: f32,
    color_tint: [f32; 4],
    wm: [f32; 6],
    font_id: u32,
) {
    if meshes.is_empty() {
        // 空文本 → 占位 Mesh（无顶点，image_path 用第 0 页兜底）。
        id_to_pos.insert(n.id, nodes.len());
        nodes.push(RenderNode {
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
                verts: vec![],
                uvs: vec![],
                colors: vec![],
                indices: vec![],
                image_path: Some(format!("loomgui://font-atlas/f{}/p0", font_id)),
                program: 1,
                color_matrix: [0.0; 20],
            },
        });
        return;
    }
    // 首页（page 0）→ 主 RenderNode，用真 node_id。
    {
        let (page0, verts0, uvs0, colors0, indices0) = &meshes[0];
        let path0 = format!("loomgui://font-atlas/f{}/p{}", font_id, page0);
        id_to_pos.insert(n.id, nodes.len());
        nodes.push(RenderNode {
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
    for (pi, (page, verts, uvs, colors, indices)) in meshes[1..].iter().enumerate() {
        let sub_id = synth_text_node_id(node_id, (pi + 1) as u32);
        let sub_path = format!("loomgui://font-atlas/f{}/p{}", font_id, page);
        nodes.push(RenderNode {
            node_id: sub_id,
            parent_id,
            visible: true,
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

#[cfg(test)]
mod tests;
