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
use crate::scene::node::{ControlState, NodeId, NodeKind, Rect, Scene};
use crate::text::atlas::{GlyphAtlas, GlyphKey};
use crate::text::layout::{measure_text, FontTable};
use node::*;

use taffy::style::LengthPercentage;

/// box-shadow 合成 RenderNode 的 high-byte 标签区。
///
/// 每层 box-shadow 产一个合成 RenderNode（独立 draw call），其 node_id =
/// `(primary & 0x00FF_FFFF) | (synth_byte << 24)`，high byte 编码层类型 + 层内 idx：
/// - 外阴影（outer）：画在 primary 之下（sort_key < primary）。high byte = 44 + idx
///   （最多 4 层/primary，44..=47）。
/// - 内阴影（inset）：画在 primary 之上、子节点之下。high byte = 36 + idx
///   （最多 8 层/primary，36..=43）。
///
/// 选 high-byte 编码（非 bit flag）的理由：bit 空间已挤（V/H_THUMB_FLAG 占 bit 30/29），
/// 且 outer 多层需在 id 内编码层内 idx 以保证唯一——bit flag 无法编 idx。high byte
/// 36..=47 全落在安全区：bit 28（旧 BACK_LAYER_FLAG 位）清零、不在跨页子页 1..=15、
/// 不在 retired INLINE_IMG 232..=255、不在 TF synth 32..=35。故彻底弃用旧
/// BACK_LAYER_FLAG bit（box-shadow 是其唯一使用者，现已迁出）。
const FRONT_SHADOW_SYNTH_BYTE: u32 = 36;
const BACK_SHADOW_SYNTH_BYTE: u32 = 44;

/// 生成 inset box-shadow 合成 node_id（high byte = 36 + idx）。idx = 该 primary 内
/// inset 层的 CSS 序号（保 id 唯一；sort_key 由 propagate 按 push 序另算）。
fn front_shadow_id(primary: u32, idx: u32) -> u32 {
    (primary & 0x00FF_FFFF) | ((FRONT_SHADOW_SYNTH_BYTE + idx) << 24)
}

/// 生成 outer box-shadow 合成 node_id（high byte = 44 + idx）。idx = 该 primary 内
/// outer 层的 CSS 序号。
fn back_shadow_id(primary: u32, idx: u32) -> u32 {
    (primary & 0x00FF_FFFF) | ((BACK_SHADOW_SYNTH_BYTE + idx) << 24)
}

/// 判断 node_id 是否为 inset box-shadow 合成节点（high byte 36..=43）。
/// propagate_text_sub_page_sort_keys 据此把它们排到 primary 之上（紧随 primary）。
pub(crate) fn is_front_shadow_synth(node_id: u32) -> bool {
    let hi = (node_id >> 24) as u8;
    (FRONT_SHADOW_SYNTH_BYTE as u8..=43).contains(&hi)
}

/// 判断 node_id 是否为 outer box-shadow 合成节点（high byte 44..=47）。
pub(crate) fn is_back_shadow_synth(node_id: u32) -> bool {
    let hi = (node_id >> 24) as u8;
    (BACK_SHADOW_SYNTH_BYTE as u8..=47).contains(&hi)
}

/// 判断 node_id 是否为任一 box-shadow 合成节点（inset 36..=43 ∪ outer 44..=47）。
/// merge::mesh_key / batch::is_mergeable_mesh 据此排除合批——box-shadow 合成 mesh
/// 须保持独立 node_id（C# MirrorPool 按 node_id 建独立 GO，合批会吞 id）。
pub(crate) fn is_shadow_synth(node_id: u32) -> bool {
    is_front_shadow_synth(node_id) || is_back_shadow_synth(node_id)
}

/// 富文本行内图（inline `<img>`）合成 node_id 子页基址。每个行内图一个独立 RenderNode
/// （image shader + image_path=src），须叠在 primary 文字层之上：sort_key 由
/// `propagate_inline_image_sort_keys` 设为 primary 文字层 max + img_idx + 1。
/// synth_text_node_id 编码后 high byte = (1000 + idx) & 0xFF = 232..=255，不与跨页子页
/// （1..=15）或 box-shadow synth（high byte 36..=47）撞——靠 `inline_image_pairs` 显式
/// 配对传播 sort_key，不凭 high byte 判别。
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

/// 收集所有 open Dropdown 的 `role="listbox"` 根 NodeId（供末尾浮层追加 DFS 用）。
///
/// 浮层渲染（Task 11，套 scrollbar thumb 末尾追加模式）：open Dropdown 的 `role="listbox"`
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
        shadow_params: [0.0; 6],
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
    n: &crate::scene::node::Node,
    node_id: u32,
    parent_id: Option<u32>,
    rect: &Rect,
    wm: [f32; 6],
    alpha: f32,
    color_tint: [f32; 4],
    has_filter: bool,
    color_matrix: [f32; 20],
    anim: Option<&crate::scene::node::NodeAnim>,
    image_sizes: &ImageSizeTable,
) -> RenderNode {
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
        shadow_params: [0.0; 6],
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

/// 每帧算每节点累积 alpha（CSS opacity 父级累积：子整体乘父 alpha，spec §3.3）。
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
    let node = scene.get(id).expect("live node");
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
    let mut pruned = collect_display_none_subtree(scene);
    // open Dropdown 的 role=listbox 浮层子树也跳过正常 DFS：末尾以浮层模式追加（sort_key 续号、
    // mask=0 跳出祖先 clip）。收集根列表供末尾 DFS 用，同时把整子树并入 pruned。
    let open_popup_roots = collect_open_popup_roots(scene);
    prune_subtrees(scene, &open_popup_roots, &mut pruned);
    let mut nodes: Vec<RenderNode> = Vec::new();
    // 累积 alpha 预计算（父 opacity 逐层乘入子）：RenderNode.alpha 存累积值，后端画时直接用。
    // 主循环是平铺遍历（slotmap 序），父未必先于子，故单独 DFS 一遍把每节点累积值算好。
    let alphas = accumulate_alpha(scene);
    // box-shadow outer 阴影合成 RenderNode 追踪：(primary node_id, outer 阴影合成 node_id)。
    // inset 阴影不经此表——由 propagate_text_sub_page_sort_keys 按 high-byte 自动收集。
    let mut back_layer_pairs: Vec<(u32, u32)> = Vec::new();
    // 富文本行内图 RenderNode 追踪：(主节点 node_id, 行内图合成 node_id)。
    let inline_image_pairs: Vec<(u32, u32)> = Vec::new();
    for n in scene.nodes.values() {
        if pruned.contains(&n.id) {
            continue;
        }
        // 纯空白 TextNode（HTML tag 间换行+缩进）不画——layout 已过滤，layout_rect 为
        // 默认 0×0，但 content 非空（"\n    "）仍可能产 0 尺寸 mesh，跳过更干净。
        if crate::scene::node::is_whitespace_only_text(scene, n.id) {
            continue;
        }
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
            alphas[n.id.index()],
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
    // box-shadow synth 节点继承 primary 的 mask_context（overflow 裁剪传播，spec §4.6）。
    propagate_shadow_mask_context(&mut nodes);
    propagate_inline_image_sort_keys(&mut nodes, &inline_image_pairs);
    batch::reorder_for_batching(scene, &mut nodes);
    // 控件节点必须保留独立 node_id（Unity 按 node_id 建交互实体/镜像 GameObject）。
    // merge 会把相邻同 DrawState 节点合并成单个（node_id 取 anchor），吞掉被合并者的
    // node_id —— 控件被吞 = Unity 丢失控件实体（不渲染、不可交互）。故控件排除出合并。
    let control_ids: std::collections::HashSet<u32> = scene
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
    let mut nodes = merge::merge_meshes(&control_ids, nodes);
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
    // open Dropdown 的 role=listbox 浮层子树：跳出正常 DFS（已在主遍历剪枝），末尾追加。
    // 模式同 scrollbar thumb（上方），但追加整子树 DFS 而非单 quad。sort_key 续 scrollbar
    // thumb 之后（重算 max——thumb 刚 push 进 nodes，占用了 max_sort+1 槽位），mask_context
    // =MaskContext(0) 跳出祖先 overflow:hidden clip（dropdown 常在 scroll 容器/固定高度面板
    // 里，展开列表要溢出父边界显示）。走 render_one_node(register=false)：popup 节点产出
    // 与正常节点几何一致的 RenderNode（背景/文本/图/边框/box-shadow 同路径），但不登记
    // id_to_pos（已在 assign_sort_keys 之后，id_to_pos 不再使用）。
    let mut popup_counter = nodes.iter().map(|n| n.sort_key).max().unwrap_or(0) + 1;
    for &popup_root in &open_popup_roots {
        // DFS 子树（先序=绘制序）。跳过空白 text（同主遍历）。
        let mut stack: Vec<NodeId> = vec![popup_root];
        while let Some(nid) = stack.pop() {
            // 子节点逆序入栈 → 出栈保跨子树先序（同 assign_sort_keys dfs 风格）。
            // 但 popup 先序只影响同 popup 内的绘制序（父子 / 前后兄弟），不跨 popup。
            let Some(node) = scene.get(nid) else {
                continue;
            };
            if crate::scene::node::is_whitespace_only_text(scene, nid) {
                // 仍需递归子节点（空白 text 无子，此分支实际不进）。
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
            );
            for rn in &mut nodes[start..] {
                rn.sort_key = popup_counter;
                rn.mask_context = MaskContext(0);
                popup_counter += 1;
            }
            // 逆序 push 子节点（stack LIFO → 先序出栈）。
            let kids = node.children.clone();
            for c in kids.into_iter().rev() {
                stack.push(c);
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

// TextField 编辑反馈 mesh（光标 / 选区背景 / composition 下划线）的合成 node_id 标签。
// 这些 mesh 与背景框、文字 mesh 共属同一节点，但必须各有独立 node_id——否则 dirty
// hash 表（new_hashes，以 node_id 为键）会因键碰撞只保留其一的 hash，导致增量更新
// 漏检（Task 4 parked 的背景+文字共用 node_id 问题即此机制）。此处用独立合成 id 规避。
//
// 这些 mesh 是逐帧重算的动态反馈（光标闪烁、选区随拖拽变），绝不能与静态背景/文字
// 合批——否则（1）光标闪烁会连累背景每帧重传；（2）cursor 与 composition 变化节奏
// 不同，合批会让 composition 的 node_id 随光标可见性跳变，dirty-tracking 抖动。故
// [`is_tf_edit_synth`] 在 batch/merge 里显式排除它们（靠显式谓词，不靠 high-byte 滥位）。
//
// high-byte 取值约束（须同时满足）：
//   1. 不在 text 跨页子页范围（1..=15），否则 is_text_sub_page 误判；
//   2. 不在 box-shadow synth 区间（36..=47），否则 is_shadow_synth 误排除合批；
//   3. 不与 retired INLINE_IMG_SYNTH_ID_BASE（high byte 232..=255）撞。
// 选 32/33/34，满足全部约束；is_text_sub_page / is_shadow_synth 对此区间均返 false。
const TF_CURSOR_SYNTH_BYTE: u32 = 32;
const TF_SELECTION_SYNTH_BYTE: u32 = 33;
const TF_COMPOSITION_SYNTH_BYTE: u32 = 34;
/// 文本控件（TextField/TextArea/NumberField）的文字主体 mesh 合成 id 标签。
/// 这些控件先 push 背景框 mesh（占真 node_id），再 push 文字 mesh；若文字也用真
/// node_id，则 C# MirrorPool 按 node_id 唯一索引 GO 时第二个 mesh 覆盖第一个
/// → 控件渲染残缺/不可见（settings showcase 的 spinbutton “无法渲染” 根因）。
/// 文字 mesh 改用此合成 id 与背景区分，C# 各自独立 GO；primary 关联仍 = 真节点 id
///（text_sub_primary_id 可还原），供 sort_key 传播与调试反查。选 35：与子页 1..=15、
/// box-shadow synth 区间（36..=47）、retired 232..=255 均不撞（同 32..=34 安全区间）。
const TF_TEXT_SYNTH_BYTE: u32 = 35;

/// 生成 TextField 编辑反馈 mesh 的合成 node_id（high byte = tag，低 24 位 = primary）。
/// 编码同 `synth_text_node_id`，仅 high-byte 标签语义不同（编辑反馈 vs 跨页子页）。
fn tf_synth_id(primary_id: u32, tag_byte: u32) -> u32 {
    (primary_id & 0x00FF_FFFF) | (tag_byte << 24)
}

/// 判断 node_id 是否为 TextField 编辑反馈 mesh（high byte 在 32..=34）。
/// 这些 mesh 须保留为独立 RenderNode（不与背景/文字合批，理由见上方常量注释），
/// batch::is_mergeable_mesh 与 merge::mesh_key 据此排除它们。
pub(crate) fn is_tf_edit_synth(node_id: u32) -> bool {
    let hi = (node_id >> 24) as u8;
    (TF_CURSOR_SYNTH_BYTE as u8..=TF_COMPOSITION_SYNTH_BYTE as u8).contains(&hi)
}

/// 判断 node_id 是否为文本控件（TextField/TextArea/NumberField）的文字主体 mesh 合成 id
///（high byte = 35）。这些 mesh 须独立保留：sort_key 由 propagate_text_sub_page_sort_keys
/// 按 primary 传播（紧跟背景之后），merge::mesh_key 据此排除合批（保持独立 GO）。
pub(crate) fn is_tf_text_synth(node_id: u32) -> bool {
    (node_id >> 24) as u8 == TF_TEXT_SYNTH_BYTE as u8
}

/// 判断 node_id 是否为跨页 text 子页（high byte 在 1..=15，即 bits [31:24] 值 1-15）。
/// box-shadow synth（high byte 36..=47）和 INLINE_IMG_SYNTH_ID_BASE（high byte
/// = 232..=255）均不在此范围——各自的 propagate 函数单独传播 sort_key，不走子页传播。
fn is_text_sub_page(node_id: u32) -> bool {
    let page = (node_id >> 24) as u8;
    (1..=15).contains(&page)
}

/// 提取跨页 text 子页对应的主节点 id。
fn text_sub_primary_id(node_id: u32) -> u32 {
    node_id & 0x00FF_FFFF
}

/// box-shadow synth 节点继承 primary 的 mask_context（overflow 裁剪传播）。
///
/// assign_sort_keys 按 scene 树 DFS 赋 mask_context，synth 节点（high-byte 假 id）不在
/// scene 树 → 默认 0（不裁）。本 post-pass 按 synth node_id 的 low-24 位找 primary，拷其
/// mask_context，使 shadow 在 overflow 容器内被正确裁剪（spec §4.6：shadow 继承主节点
/// mask_context，outer/inset 同传播）。
fn propagate_shadow_mask_context(nodes: &mut [RenderNode]) {
    // primary node_id → mask_context（一遍扫描，避 O(N²)：多层 shadow 共享同 primary 查表）。
    let ctx_by_id: std::collections::HashMap<u32, MaskContext> = nodes
        .iter()
        .filter(|n| !is_shadow_synth(n.node_id))
        .map(|n| (n.node_id, n.mask_context))
        .collect();
    for rn in nodes.iter_mut() {
        if is_shadow_synth(rn.node_id) {
            let primary = rn.node_id & 0x00FF_FFFF;
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
    shadow_pairs: &[(u32, u32)], // (primary_node_id, shadow_node_id)，CSS push 序
) {
    if shadow_pairs.is_empty() {
        return;
    }
    // 按 primary 分组 outer 阴影（保 push 序 = CSS 序——outer 按 CSS 序 push）。
    let mut groups: std::collections::HashMap<u32, Vec<u32>> = std::collections::HashMap::new();
    for &(primary, shadow_id) in shadow_pairs {
        groups.entry(primary).or_default().push(shadow_id);
    }
    // 每组采集 main_sk（primary 当前 sort_key），按 DESC 排序处理。
    let mut entries: Vec<(u32, u32, Vec<u32>)> = groups
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
        let id_set: std::collections::HashSet<u32> = shadow_ids.iter().copied().collect();
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

/// Text 附属 mesh sort_key 传播 + 后续真节点 sort_key 后移。
///
/// assign_sort_keys 只给 `id_to_pos` 中的真 scene 节点赋 sort_key；合成附属 mesh 保持 0。
/// 附属 mesh 有四类（都按 primary = low 24 bit 关联真节点）：
/// - 跨页子页（high byte 1..=15）：多页文字的后续页。
/// - 文本控件文字首页（high byte 35）：TextField/TextArea/NumberField 的文字主体
///   （背景框 mesh 占真 node_id，文字用合成 id 区分，见 TF_TEXT_SYNTH_BYTE）。
/// - 编辑反馈 mesh（high byte 32..=34）：光标 / 选区背景 / composition 下划线。
/// - inset box-shadow（high byte 36..=43）：内阴影层，画在 primary 之上、子节点之下。
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
    let mut groups: std::collections::HashMap<u32, Vec<u32>> = std::collections::HashMap::new();
    let mut synth_pos: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
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
/// - `Percent(_)` → 0.0。**已知缺口**（记 ledger）：渲染阶段无父 content-box 宽度上下文，
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
/// 直接塞进 base/子页/占位 RenderNode.effect，不再产 back/front layer 合成节点（原
/// 双层合成机制全废；box-shadow 现走专属 high-byte synth，不走此路径）。
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
            shadow_params: [0.0; 6],
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
    nodes: &mut Vec<RenderNode>,
    node_id: u32,
    parent_id: Option<u32>,
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
        nodes, node_id, parent_id, wm, alpha, reuse_key, verts, uvs, colors, indices,
    );
}

/// 推一个已组装好的纯色 mesh RenderNode（多 quad 合并为一节点，供选区/下划线跨行用）。
#[allow(clippy::too_many_arguments)]
fn push_solid_mesh(
    nodes: &mut Vec<RenderNode>,
    node_id: u32,
    parent_id: Option<u32>,
    wm: [f32; 6],
    alpha: f32,
    reuse_key: u32,
    verts: Vec<[f32; 2]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
) {
    nodes.push(RenderNode {
        node_id,
        parent_id,
        visible: true,
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

/// 渲染单个 Scene 节点为一个或多个 RenderNode 并推入 `nodes`（共享于主 DFS 与 open popup
/// 末尾追加 DFS）。
///
/// 复用入口——主 DFS 与 [`build_render_nodes`] 末尾的 popup 浮层追加（Task 11）都走此函数，
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
    back_layer_pairs: &mut Vec<(u32, u32)>,
    register_id_map: bool,
    alpha: f32,
) {
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
    let color_tint = anim.and_then(|a| a.text_color).unwrap_or(n.style.color);
    let rn = match n.kind {
        // 控件外壳节点（不在 is_container）但渲染上需要一个背景框：
        // - Dropdown/OptionItem：combobox 外壳 / 选项列表项
        // - Toggle/RadioButton：空 div，勾选样式靠自身 [role]/[aria-checked] 的 background
        // - Slider/ProgressBar：轨道 / 底色（fill/thumb 子节点另自渲染）
        // pivot 后控件视觉子结构由作者按 role/data-slot 自写（§2.3，core 不注入），
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
            build_container_mesh(
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
                shadow_params: [0.0; 6],
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
            // 显示文本：value 优先（经 display_value 拼接 composition 预提交文本），空时
            // 退到 placeholder。display_value 同时给出 composition 的 display 字节区间，供
            // 下划线对齐预提交文本。measure_text_controls 缓存的 TextLayout 与这里 display
            // 同源（都走 display_value），故文字 mesh 与下划线几何一致。
            let (dv, comp_range) = crate::scene::control::display_value(e);
            let is_placeholder = dv.is_empty();
            let display = if is_placeholder {
                e.placeholder.clone()
            } else {
                dv
            };
            let s = &n.style;
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
            // ── 编辑反馈 mesh：选区背景 / composition 下划线 / 光标 ──
            // 几何用上面烤过 content offset 的 `layout`；坐标与世界文字字形同系
            // （rect.xy + 局部）。各 mesh 用独立合成 node_id，避免与背景/文字 mesh 在
            // dirty hash 表碰撞（Task 4 parked 的同 id 问题）。
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
                // sel_b/sel_e 是 value 字节偏移；显示串与 value 同字节布局（无掩码），
                // 直接取像素几何。
                let (xb, lib) = crate::scene::text_cursor::cursor_pixel_x(&layout, &ranges, sel_b);
                let (xe, lie) = crate::scene::text_cursor::cursor_pixel_x(&layout, &ranges, sel_e);
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
                rect,
                &n.style.text_effects,
                n.style.background_gradient,
                n.style.background_clip_text,
            );
            // 文字 mesh 用合成 id（TF_TEXT_SYNTH_BYTE）：背景框 mesh 已占真 node_id，若
            // 文字也用真 node_id 则 C# MirrorPool 同 node_id 唯一 GO 把文字覆盖背景（控件
            // 渲染残缺）。合成 id 让文字独立 GO；primary（low 24 bit）仍 = node_id，供
            // sort_key 传播还原。register_id_map=false：背景已注册 n.id → id_to_pos。
            push_text_meshes(
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
                    let ul_color = text_color; // 缺省下划线色 = 文字色（Task 15 可换）
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
                // e.cursor 是 value 字节偏移；显示串与 value 同字节布局（无掩码），
                // 直接取像素 x。
                let (cx, li) =
                    crate::scene::text_cursor::cursor_pixel_x(&layout, &ranges, e.cursor);
                if let Some(line) = layout.lines.get(li) {
                    // cx 是 advance 累计（内容区相对，不含 off_left）；line.y 已含 off_top。
                    let x = rect.x + off_left + cx;
                    let y = rect.y + line.y;
                    push_solid_quad(
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
            shadow_params: [0.0; 6],
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
    let shadows = &n.style.box_shadow;
    if n.kind.is_container() && !shadows.is_empty() {
        let radii = n.style.border_radius.as_corners(rect.w, rect.h);
        // outer（back）层：CSS 序 push。
        for (i, sh) in shadows.iter().filter(|s| !s.inset).enumerate() {
            // blur<0.5 → σ=0.5（1px AA 硬边）；否则 σ=blur/2（RmlUi 映射）。
            let sigma = if sh.blur < 0.5 { 0.5 } else { sh.blur * 0.5 };
            let sid = back_shadow_id(node_id, i as u32);
            let (v, uvc, col, idx, params) =
                crate::render::border::shadow_quad(rect, &radii, sh, sigma);
            if v.is_empty() {
                continue;
            }
            back_layer_pairs.push((node_id, sid));
            nodes.push(RenderNode {
                node_id: sid,
                parent_id,
                visible: true,
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
            .collect();
        for &(css_idx, sh) in inset_layers.iter().rev() {
            // blur<0.5 → σ=0.5（1px AA 硬边）；否则 σ=blur/2（RmlUi 映射）。
            let sigma = if sh.blur < 0.5 { 0.5 } else { sh.blur * 0.5 };
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
                node_id: sid,
                parent_id,
                visible: true,
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
    if register_id_map {
        id_to_pos.insert(n.id, nodes.len());
    }
    nodes.push(rn);
}
#[cfg(test)]
mod tests;
