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
pub mod merge;
pub mod mesh;
pub mod node;

use crate::layout::ImageSizeTable;
use crate::scene::node::{NodeId, NodeKind, Rect, Scene};
use crate::text::atlas::{GlyphAtlas, GlyphKey};
use crate::text::layout::{measure_text, FontTable};
use node::*;

use taffy::style::LengthPercentage;

/// 合成 node_id 标志位：div box-shadow 独立 RenderNode（独立 draw call 画在主节点下层）。
/// 0x1000_0000（bit 28），不与 V_THUMB_FLAG（0x4000_0000）、H_THUMB_FLAG（0x2000_0000）、
/// 跨页 text 子页（bits [31:24]，实际值 1..~10）冲突。Back layer sort_key < primary（先绘 = 下层）。
///
/// 仅 div box-shadow 使用——文字效果（shadow/stroke/glow/blur）SDF 改造后改由 shader
/// uniform 实现，不再产 back/front layer 合成节点。
pub(crate) const BOX_SHADOW_FLAG: u32 = 0x1000_0000;

/// 富文本行内图（inline `<img>`）合成 node_id 子页基址。每个行内图一个独立 RenderNode
/// （image shader + image_path=src），须叠在 primary 文字层之上：sort_key 由
/// `propagate_inline_image_sort_keys` 设为 primary 文字层 max + img_idx + 1。
/// synth_text_node_id 编码后 high byte = (1000 + idx) & 0xFF = 232..=255，不与跨页子页
/// （1..=15）或 BOX_SHADOW_FLAG（high byte 16）撞——靠 `inline_image_pairs` 显式配对
/// 传播 sort_key，不凭 high byte 判别。
#[allow(dead_code)] // RichText retired in Spec-2; kept for compound-bundle text model.
pub(crate) const INLINE_IMG_SYNTH_ID_BASE: u32 = 1000;

/// Font-atlas image_path for a given page index. Consumed verbatim by the
/// Unity backend's SpriteResolver. This string format is an ABI-level contract
/// across the FFI boundary — changing it here requires changing the C# side too.
pub(crate) fn font_atlas_path(page: usize) -> String {
    format!("loomgui://font-atlas/p{page}")
}

/// 把 2 色线性渐变映射到 quad 4 角顶点色（顶点序 TL, TR, BR, BL）。
///
/// GPU 顶点色光栅插值在 4 角间线性过渡——将 2 色 (a, b) 按方向放到对应的"起点/终点"角，
/// 整个 quad 内即呈现线性渐变。映射严格遵循 CSS `to <dir>` 语义：a 是渐变起点色，b 是终点色。
///
/// - ToRight: 左 a 右 b → [TL=a, TR=b, BR=b, BL=a]
/// - ToLeft:  右 a 左 b → [TL=b, TR=a, BR=a, BL=b]
/// - ToTop:   下 a 上 b → [TL=b, TR=b, BR=a, BL=a]
/// - ToBottom:上 a 下 b → [TL=a, TR=a, BR=b, BL=b]
fn gradient_corner_colors(g: crate::style::resolved::Gradient2) -> [[f32; 4]; 4] {
    let (a, b) = (g.color_a, g.color_b);
    use crate::style::resolved::GradientDir as G;
    match g.dir {
        G::ToRight => [a, b, b, a],
        G::ToLeft => [b, a, a, b],
        G::ToTop => [b, b, a, a],
        G::ToBottom => [a, a, b, b],
    }
}

/// gradient text（background-clip:text）每字 quad 的 4 角色：按字在文本块的位置插值
/// color_a→color_b，使整段文字作为一个整体渐变——而非每字独立 a→b（否则 N 字 = N 个
/// 小渐变，每字各自从首色到末色）。水平方向跨行宽（每行各自 a→b），垂直跨文本块高。
/// 返回 [BL, BR, TR, TL]（quad 顶点序，与 base 字形 push 顺序一致）。
fn gradient_glyph_colors(
    g: &crate::style::resolved::Gradient2,
    glyph_x: f32,
    glyph_advance: f32,
    line_width: f32,
    line_y: f32,
    line_height: f32,
    text_height: f32,
) -> [[f32; 4]; 4] {
    let a = g.color_a;
    let b = g.color_b;
    let lerp = |t: f32| {
        let t = t.clamp(0.0, 1.0);
        [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
            a[3] + (b[3] - a[3]) * t,
        ]
    };
    let lw = line_width.max(1.0);
    let th = text_height.max(1.0);
    let x_l = glyph_x / lw;
    let x_r = (glyph_x + glyph_advance) / lw;
    let y_t = line_y / th;
    let y_b = (line_y + line_height) / th;
    use crate::style::resolved::GradientDir as G;
    match g.dir {
        G::ToRight => {
            let (cl, cr) = (lerp(x_l), lerp(x_r));
            [cl, cr, cr, cl]
        }
        G::ToLeft => {
            let (cl, cr) = (lerp(1.0 - x_l), lerp(1.0 - x_r));
            [cl, cr, cr, cl]
        }
        G::ToBottom => {
            let (ct, cb) = (lerp(y_t), lerp(y_b));
            [cb, cb, ct, ct]
        }
        G::ToTop => {
            let (ct, cb) = (lerp(1.0 - y_t), lerp(1.0 - y_b));
            [cb, cb, ct, ct]
        }
    }
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
        effect: EffectBlock::default(),
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
    // box-shadow 独立 RenderNode 追踪：(主节点 node_id, 阴影合成 node_id)。
    let mut shadow_pairs: Vec<(u32, u32)> = Vec::new();
    // 富文本行内图 RenderNode 追踪：(主节点 node_id, 行内图合成 node_id)。
    let inline_image_pairs: Vec<(u32, u32)> = Vec::new();
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
        let rn = match n.kind {
            k if k.is_container() => {
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
                let (rw, rh) = (rect.w, rect.h);
                let radii = n.style.border_radius.as_corners(rw, rh);
                let all_zero = radii.iter().all(|&(rx, ry)| rx <= 0.0 || ry <= 0.0);
                let has_slice = n.style.border_image_slice.is_some();
                // 渐变背景仅在没有背景图（互斥）、没有九宫格切片、直角 quad 时启用——
                // quad_gradient 是 4 角独立色 quad，GPU 顶点色插值得 2 色线性渐变。
                // 与圆角 / 切片共存需 gradient + rounded_rect/slice 混合 mesh，留待后续 task。
                let use_gradient =
                    !has_image && !has_slice && all_zero && n.style.background_gradient.is_some();
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
                let (mut v, mut uvc, mut col, mut idx) = if use_gradient {
                    let g = n.style.background_gradient.expect("use_gradient 已校验");
                    crate::render::mesh::quad_gradient(
                        &draw_rect,
                        gradient_corner_colors(g),
                        [u_min[0], u_max[1]],
                        [u_max[0], u_min[1]],
                    )
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
                // 彩色边框激活（v1.8 修 border_color 死字段）。无背景图时把边框环形 mesh
                // 拼进同一 payload：纯色 Container 背景与边框同走 program=0（白 1×1 纹理 ×
                // 顶点色），单 draw call，边框三角序在背景之后——重叠的边框环区边框覆盖背景，
                // 内部仅背景，视觉正确。filter（program=3）也走此路：filter 应作用于整元素含边框。
                // ponytail: 有背景图（program=2/4）时边框需独立 draw call（边框纯色 vs 背景采样
                // 图），本 task 不做——留待 border + bg-image 共存场景单独处理。
                if !has_image {
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
                            let br = crate::render::border::border_ring(
                                rect, &radii, widths, border_col,
                            );
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
                    effect: EffectBlock::default(),
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
            NodeKind::Image => {
                let src = scene.image_srcs.get(&n.id).cloned().unwrap_or_default();
                let image_path = Some(src.clone());
                let uv_min = [0.0, 0.0];
                let uv_max = [1.0, 1.0];
                let (src_w, src_h) = src_size(image_sizes, &src);
                let (v, uvc, col, idx) = match &n.style.border_image_slice {
                    Some(slice) => {
                        let resolved = resolve_slice_percent(slice, src_w, src_h);
                        crate::render::mesh::nine_slice(
                            rect,
                            [1.0; 4],
                            &resolved,
                            src_w,
                            src_h,
                            [uv_min[0], uv_max[1]],
                            [uv_max[0], uv_min[1]],
                        )
                    }
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
                    effect: EffectBlock::default(),
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
                let off_right = resolve_lp(s.taffy_style.border.right)
                    + resolve_lp(s.taffy_style.padding.right);
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
                            s.line_height,
                            s.letter_spacing,
                            s.text_align,
                            s.white_space_nowrap,
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
                    n.style.background_gradient,
                    n.style.background_clip_text,
                );
                push_text_meshes(
                    &mut nodes,
                    &mut id_to_pos,
                    meshes,
                    n,
                    node_id,
                    node_id,
                    parent_id,
                    alpha,
                    color_tint,
                    wm,
                    true,
                );
                continue; // 直接推完，跳过末尾的 id_to_pos / push。
            }
            _ => RenderNode {
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
                effect: EffectBlock::default(),
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
        // box-shadow：独立 RenderNode 画在节点下层（sort_key 更小 = 先绘 = 在下）。
        // 阴影节点不入 id_to_pos（不在场景树中），sort_key 在 assign_sort_keys 后
        // 由 propagate_box_shadow_sort_keys 调整为主节点 sort_key（主节点后移一位）。
        if let Some(shadow) = n.style.box_shadow.as_ref() {
            if n.kind.is_container() {
                let rw = rect.w;
                let rh = rect.h;
                let radii = n.style.border_radius.as_corners(rw, rh);
                let shadow_rect = Rect {
                    x: rect.x + shadow.ox,
                    y: rect.y + shadow.oy,
                    w: rect.w,
                    h: rect.h,
                };
                let (v, uvc, col, idx) = crate::render::border::box_shadow_quad(
                    &shadow_rect,
                    &radii,
                    shadow.spread,
                    shadow.color,
                );
                if !v.is_empty() {
                    let sid = node_id | BOX_SHADOW_FLAG;
                    shadow_pairs.push((node_id, sid));
                    nodes.push(RenderNode {
                        node_id: sid,
                        parent_id,
                        visible: true,
                        alpha,
                        color_tint,
                        world_matrix: wm,
                        blend: BlendMode::Normal,
                        mask_context: MaskContext(0),
                        sort_key: 0, // assign_sort_keys 后重调
                        change_level: ChangeLevel::Full,
                        reuse_key: 0,
                        effect: EffectBlock::default(),
                        payload: NodePayload::Mesh {
                            verts: v,
                            uvs: uvc,
                            colors: col,
                            indices: idx,
                            image_path: None,
                            program: 0,
                            color_matrix: [0.0; 20],
                        },
                    });
                }
            }
        }
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
    propagate_box_shadow_sort_keys(&mut nodes, &shadow_pairs);
    propagate_inline_image_sort_keys(&mut nodes, &inline_image_pairs);
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

/// 判断 node_id 是否为跨页 text 子页（high byte 在 1..=15，即 bits [31:24] 值 1-15）。
/// BOX_SHADOW_FLAG（bit 28，对应 high byte >= 16）和 INLINE_IMG_SYNTH_ID_BASE（high byte
/// = 232..=255）均不在此范围——各自的 propagate 函数单独传播 sort_key，不走子页传播。
fn is_text_sub_page(node_id: u32) -> bool {
    let page = (node_id >> 24) as u8;
    (1..=15).contains(&page)
}

/// 提取跨页 text 子页对应的主节点 id。
fn text_sub_primary_id(node_id: u32) -> u32 {
    node_id & 0x00FF_FFFF
}

/// 提取跨页 text 子页的页号（1 基）。
fn text_sub_page_idx(node_id: u32) -> u32 {
    node_id >> 24
}

/// box-shadow 合成节点 sort_key 调整：阴影节点继承主节点 sort_key，
/// 主节点及后续节点后移一位（阴影在背景层之下 = sort_key 更小 = 先绘）。
///
/// assign_sort_keys 只给 id_to_pos 中的真 scene 节点赋 sort_key；
/// box-shadow 合成节点不在场景树中，初始 sort_key=0。此函数将其调整到
/// 主节点 sort_key 位置，保证阴影在主节点背景之前渲染。
///
/// 处理多个阴影节点时从最大 sort_key 开始（降序），避免累积偏移后的 stale 值。
fn propagate_box_shadow_sort_keys(
    nodes: &mut [RenderNode],
    shadows: &[(u32, u32)], // (main_node_id, shadow_node_id)
) {
    if shadows.is_empty() {
        return;
    }
    // 构建 (main_sort_key_on_entry, main_id, shadow_id) 三元组。
    // 按 main_sort_key DESC 处理，避免先处理大 key 的移位影响后续小 key 比较。
    let mut triples: Vec<(u32, u32, u32)> = shadows
        .iter()
        .map(|&(main_id, shadow_id)| {
            // 在 nodes 中查找主节点 sort_key（shadow 在 nodes 中排在主节点之前，
            // 且主节点已由 assign_sort_keys 赋过值）。
            let main_sk = nodes
                .iter()
                .find(|n| n.node_id == main_id)
                .map(|n| n.sort_key)
                .unwrap_or(0);
            (main_sk, main_id, shadow_id)
        })
        .collect();
    triples.sort_by_key(|&(sk, _, _)| std::cmp::Reverse(sk)); // descending
    for &(main_sk, _main_id, shadow_id) in &triples {
        // 移位：所有 sort_key >= main_sk 且非本阴影的节点 +1。
        // 降序遍历保证大 sort_key 区域的移位不会污染小 key 的原始值。
        for rn in nodes.iter_mut() {
            if rn.node_id != shadow_id && rn.sort_key >= main_sk {
                rn.sort_key += 1;
            }
        }
    }
    // 设阴影 sort_key = 主节点原始 sort_key。
    // 上述降序移位后主节点已后移 +1（每个经过它的阴影对其移了一次），
    // 阴影节点初始 sort_key=0。
    // 后处理：对每个 shadow，找它的主节点（现在 sort_key = 原始 + 经过的阴影数），
    // 设为 主_sk - 1。
    for &(main_id, shadow_id) in shadows {
        if let (Some(main_pos), Some(shadow_pos)) = (
            nodes.iter().position(|n| n.node_id == main_id),
            nodes.iter().position(|n| n.node_id == shadow_id),
        ) {
            let main_sk = nodes[main_pos].sort_key;
            if main_sk > 0 {
                nodes[shadow_pos].sort_key = main_sk - 1;
            }
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
fn propagate_inline_image_sort_keys(nodes: &mut [RenderNode], images: &[(u32, u32)]) {
    if images.is_empty() {
        return;
    }
    // 按 primary 分组行内图（保持声明顺序 = 文本流顺序）。
    let mut groups: std::collections::HashMap<u32, Vec<u32>> = std::collections::HashMap::new();
    for &(_main, img_id) in images {
        let primary = img_id & 0x00FF_FFFF;
        groups.entry(primary).or_default().push(img_id);
    }
    // base = 该 primary 及其所有合成子节点（子页/shadow/stroke；行内图自身此刻 sk=0）的 max sort_key。
    let mut entries: Vec<(u32, Vec<u32>)> = groups
        .into_iter()
        .map(|(primary, imgs)| {
            let base = nodes
                .iter()
                .filter(|n| (n.node_id & 0x00FF_FFFF) == primary)
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
        let img_set: std::collections::HashSet<u32> = imgs.iter().copied().collect();
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
/// 超 CJK 字符集才跨页 → 多项（每项独立 image_path = loomgui://font-atlas/p{page}）。
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
    background_gradient: Option<crate::style::resolved::Gradient2>,
    background_clip_text: bool,
) -> TextMeshes {
    use std::collections::BTreeMap;
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
                        // 顶点色：渐变字（background-clip:text）按字在文本块的位置插值
                        // color_a→color_b，整段作为一个整体渐变；否则 run.color。
                        let quad_colors: [[f32; 4]; 4] = if background_clip_text {
                            if let Some(grad) = background_gradient {
                                gradient_glyph_colors(
                                    &grad,
                                    g.x,
                                    g.advance,
                                    line.width,
                                    line.y,
                                    line.height,
                                    layout.text_height,
                                )
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
            // 装饰线（v1.8：underline/line-through/overline + solid/dashed/dotted/double + 独立色/粗细）。
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
/// 直接塞进 base/子页/占位 RenderNode.effect，不再产 back/front layer 合成节点（原 BOX_SHADOW_FLAG
/// + TEXT_STROKE_FRONT_FLAG 双层机制全废；BOX_SHADOW_FLAG 留给 div box-shadow 单独使用）。
fn push_text_meshes(
    nodes: &mut Vec<RenderNode>,
    id_to_pos: &mut std::collections::HashMap<NodeId, usize>,
    meshes: TextMeshes,
    n: &crate::scene::node::Node,
    node_id: u32,
    text_primary_id: u32,
    parent_id: Option<u32>,
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
            node_id: text_primary_id,
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
            effect,
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
            node_id: text_primary_id,
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
            effect,
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
        let sub_id = synth_text_node_id(node_id, (pi + 1) as u32);
        let sub_path = font_atlas_path(*page as usize);
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
            // 同一文字节点所有页共享同一 effect 配置：fragment shader 按页独立重建效果。
            effect,
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
