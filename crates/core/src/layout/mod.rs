//! Layout 层：taffy 集成。
//!
//! 消费 `Scene`（Node 树 + `ResolvedStyle`），建 taffy 树，注册叶子节点的
//! 测量上下文（Text/Image），solve 后把 taffy 的 `Layout.location`/`size`
//! 回写进 `Node.layout_rect`/`clip_rect`。
//!
//! # taffy 0.12 API 边界
//!
//! taffy 0.12 用 trait 对象模式（无 `MeasureFunc` 枚举）：
//! - `TaffyTree<NodeContext>`：节点上下文是泛型，叶子节点用
//!   `new_leaf_with_context(style, ctx)` 存一个 owned `NodeContext`。
//! - 单个 `compute_layout_with_measure(root, avail, FnMut(...))` 闭包负责按
//!   `Option<&mut NodeContext>` 分派到 Text/Image 测量。
//!
//! 测量是单个 `FnMut`（非 'static），生命周期与 `compute_layout_with_measure`
//! 调用同界——闭包内借 `fonts: &FontTable` 合法。每个叶子的文本参数（content/font_size +
//! family 等）已 owned 进 `NodeContext::Text`（不含 Font 实例），font 在闭包内按 family
//! 查 FontTable 取得。`solve` 签名收 `fonts: &FontTable`（不破下游 stage 契约）。
//!
//! taffy 0.12 的 `Style` 无 `order`，不做 flex order 排序（render 层按 DOM 顺序 /
//! layout 输出的 `Layout.order` 渲染）。
//!
//! 核心知图尺寸（打包期 PNG IHDR 静态，Stage 持 path→(w,h) 尺寸表）+ 不知图集
//! （运行时纹理/UV 归 Unity）。solve 接 `image_sizes: &HashMap<String,(u32,u32)>` 查 Image intrinsic
//! 尺寸（三档：CSS > 真实像素 > 64×64）。render payload 带 path，UV 全图 (0,0)-(1,1)。

use crate::scene::node::{is_whitespace_only_text, NodeId, NodeKind, Rect, Scene};
use crate::style::resolved::{OverflowMode, TextAlign};
use crate::text::layout::{measure_text, FontTable, TextLayout};
use std::collections::HashMap;
use taffy::prelude::*;

/// 图尺寸表类型别名：归一化 path → (w, h) 像素（打包期 PNG IHDR 静态）。
/// `solve`/`build_render_nodes` 接 `&HashMap<String, (u32, u32)>` 查 Image intrinsic 尺寸。
pub type ImageSizeTable = HashMap<String, (u32, u32)>;

/// LoomGUI OverflowMode → taffy Overflow（Auto→Scroll，taffy 无 Auto 变体）。
/// Hidden/Scroll 让 taffy flex automatic min-size=0（CSS flex §4.5，taffy style/mod.rs:124）——
/// 容器不被 content min-content 撑开，content 可溢出 scroll。不设则 taffy 默认 Visible →
/// 容器被 content 撑开（viewport=content）→ overlap=0 → scroll 失效。
fn map_overflow(m: OverflowMode) -> taffy::style::Overflow {
    match m {
        OverflowMode::Visible => taffy::style::Overflow::Visible,
        OverflowMode::Hidden => taffy::style::Overflow::Hidden,
        OverflowMode::Scroll => taffy::style::Overflow::Scroll,
        OverflowMode::Auto => taffy::style::Overflow::Scroll,
    }
}

/// taffy LengthPercentage → f32（固定尺寸节点 Percent 罕见，按 0 处理）。
fn lp(v: taffy::style::LengthPercentage) -> f32 {
    // taffy 0.12：LengthPercentage 是 pub struct(CompactLength) tagged pointer，
    // 内字段私有无法 match 变体——用 into_raw + tag 解构（只要 Length 分支）。
    let cl = v.into_raw();
    if cl.tag() == taffy::style::CompactLength::LENGTH_TAG {
        cl.value()
    } else {
        0.0
    }
}

/// 叶子节点的测量上下文。Container/Button 无上下文（用 None 叶子或 new_with_children）。
enum MeasureContext {
    /// Text 叶子：存全部测量参数（content owned）+ 字体度量字段 + 字体族。
    /// font 实例 *不* 进 context——调用方在测量闭包中按 family 查 FontTable 取 Font。
    Text {
        content: String,
        font_size: f32,
        line_height: f32,
        letter_spacing: f32,
        align: TextAlign,
        nowrap: bool,
        /// 节点的 font_family。None 表示用 FontTable 的 default。
        family: Option<String>,
        /// 节点 style.color（plain text 整段同色；进 GlyphRun.color 供 build per-vertex）。
        color: [f32; 4],
        /// 节点 style.font_weight（≥700 → Bold，经 weight_from_font_weight 转 RichWeight 进 GlyphRun.weight）。
        font_weight: u16,
        /// 水平 padding+border 总 inset（左+右）。taffy 传 known.width = 节点 border-box 宽；
        /// 文字须在 content area（known - inset）内换行 + 对齐，否则吃到 padding 超框。
        h_inset: f32,
    },
    /// RichText 叶子（v1.7）：inline flow 封装在 measure_rich_text。
    /// runs owned（parse 期产的扁平 run 流，含 per-run 样式）。
    /// `align` 传入 measure_rich_text（每行容器内偏移）；`nowrap` 暂未接线（rich 不支持）。
    RichText {
        runs: Vec<crate::text::rich::RichRun>,
        line_height: f32,
        /// CSS letter-spacing（px）。rich inline flow 的 token 宽/glyph 定位均计入。
        letter_spacing: f32,
        align: TextAlign,
        /// 暂未接线：rich 还未支持 white-space:nowrap。先携值（来自节点 style），待 measure_rich_text
        /// 加 nowrap 路径后此字段即被读，避免加/删字段的连带改动。
        #[allow(dead_code)]
        nowrap: bool,
        /// 节点的 font_family。None 表示用 FontTable 的 default。
        family: Option<String>,
        /// 水平 padding+border 总 inset（左+右）。同 Text：文字在 content area 内换行/对齐。
        h_inset: f32,
    },
    /// Image 叶子：intrinsic 像素 + css width/height 维度。闭包消费 taffy 的 known 解析
    /// Percent/fit（Percent width taffy 传 known.width=Some(解析宽)，闭包据此等比 height）。
    Image {
        iw: f32,
        ih: f32,
        w_dim: taffy::style::Dimension,
        h_dim: taffy::style::Dimension,
    },
}

/// 就地 solve：建 taffy 树 → 注册测量上下文 → compute_layout → 回写 layout_rect/clip_rect。
///
/// `root_size` 是根节点固定尺寸（viewport / surface 尺寸）。`fonts` borrows
/// FontTable 到 `compute_layout_with_measure` 结束，闭包内按 family 查字体喂给 `measure_text`。
///
/// `image_sizes` = Stage 持有的 path→(w,h) 尺寸表（打包期 PNG IHDR 静态）。
/// Image measure 查此表算 intrinsic 尺寸（三档：CSS > 真实像素 > 64×64）。
/// path 缺失或 w/h=0 → fallback 64×64（核心不知图集，但知图尺寸）。
pub fn solve(
    scene: &mut Scene,
    fonts: &FontTable,
    root_size: (f32, f32),
    image_sizes: &ImageSizeTable,
) {
    // 防御：空 roots（空 scene）无几何可 solve——直接返回，避免 roots[0] 越界 panic。
    // Stage 可能在 scene 未装内容时 tick（如测/边界），不应 panic。
    if scene.roots.is_empty() {
        return;
    }
    let mut taffy_tree: TaffyTree<MeasureContext> = TaffyTree::new();
    // scene NodeId → taffy NodeId 映射（按 NodeId.index() 索引，1 基故 capacity+1）。
    // **容量而非存活数**：remove_node 后 slotmap idx 不变但存活数减——按 len 分配会越界。
    let mut taffy_ids: Vec<Option<taffy::NodeId>> = vec![None; scene.nodes.capacity() + 1];

    /// 返回 (自身 tid, 冒泡中的 absolute escapee tids)。escapee = 「声明 absolute 且
    /// 任一 inset 显式」的子项，其 taffy 父不是 scene 父而是最近 positioned 祖先——
    /// 未遇到 positioned 祖先前随递归向上冒泡，positioned 节点收编（含根兜底）。
    fn build(
        scene: &Scene,
        tree: &mut TaffyTree<MeasureContext>,
        taffy_ids: &mut Vec<Option<taffy::NodeId>>,
        id: NodeId,
        parent_overflow: bool,
        image_sizes: &ImageSizeTable,
        root_size: (f32, f32),
    ) -> (taffy::NodeId, Vec<taffy::NodeId>) {
        let node = scene.get_live(id, "layout/build");
        let mut style = node.style.taffy_style.clone();
        // 视口相对长度（vw/vh/vmin/vmax）按当帧 root_size 换算覆写（分辨率适配的
        // 重排语言——root_size 随屏幕/适配模式变，声明 vw 的通道跟画布走）。
        if !node.style.viewport.is_empty() {
            node.style.viewport.apply(&mut style, root_size);
        }
        // overflow != visible → 设 taffy overflow，让 flex automatic min-size=0（CSS flex §4.5）。
        // 不设则 taffy 默认 Visible → min-size=min-content → 容器被 content 撑开（viewport=content）
        // → overlap=0 → scroll 失效。
        style.overflow = taffy::geometry::Point {
            x: map_overflow(node.style.overflow_x),
            y: map_overflow(node.style.overflow_y),
        };
        // overflow 容器的直接子 flex-shrink=0：保持显式尺寸/min-content 溢出（scroll 有效）。
        // 否则空内容子（如 .filler{height:300} min-content=0）被 shrink 到 viewport → overlap=0 → 不能滚。
        if parent_overflow {
            style.flex_shrink = 0.0;
        }
        let self_overflow = node.style.overflow_x != OverflowMode::Visible
            || node.style.overflow_y != OverflowMode::Visible;
        // 叶子：Text/Image/文本控件装 MeasureContext。
        // TextField/TextArea/NumberField 是控件叶子（value/placeholder 存 ControlState，
        // 非 text_contents），须装 Text measure——否则 taffy content=0、高度只剩 padding，
        // 文字不参与布局（pivot 后空 div 形态暴露：高度塌成 padding-only）。
        //
        // rich-text-block 容器：编译 inline 子树成 RichRun，作 RichText 叶子测——inline
        // 子折进父的单段 inline flow（不递归进 taffy）。build 下方 children_ids 对
        // rich_text_block 返空 Vec 实现「不递归」。
        // display:flex 的策略切换（不折叠）在 rematch 应用 display 声明处翻转本 flag
        //（见 dynamic.rs rematch_pseudo_classes）——build 只认 flag，单一真相源。
        let ctx: Option<MeasureContext> = if node.rich_text_block {
            let s = &node.style;
            let runs = crate::text::rich_compile::compile_rich_runs(scene, id, image_sizes);
            Some(MeasureContext::RichText {
                runs,
                line_height: s.line_height,
                letter_spacing: s.letter_spacing,
                align: s.text_align,
                nowrap: s.white_space_nowrap,
                family: s.font_family.clone(),
                h_inset: lp(s.taffy_style.padding.left)
                    + lp(s.taffy_style.padding.right)
                    + lp(s.taffy_style.border.left)
                    + lp(s.taffy_style.border.right),
            })
        } else {
            match &node.kind {
                NodeKind::TextNode => {
                    let s = &node.style;
                    Some(MeasureContext::Text {
                        content: scene.text_contents.get(&id).cloned().unwrap_or_default(),
                        font_size: s.font_size,
                        line_height: s.line_height,
                        letter_spacing: s.letter_spacing,
                        align: s.text_align,
                        nowrap: s.white_space_nowrap,
                        family: s.font_family.clone(),
                        color: s.color,
                        font_weight: s.font_weight,
                        h_inset: lp(s.taffy_style.padding.left)
                            + lp(s.taffy_style.padding.right)
                            + lp(s.taffy_style.border.left)
                            + lp(s.taffy_style.border.right),
                    })
                }
                NodeKind::TextField | NodeKind::TextArea | NodeKind::NumberField => {
                    let s = &node.style;
                    // value 优先，空时用 placeholder（与 render 显示一致）；measure 用显示文本
                    // 算 intrinsic size，taffy 再加 padding/border → border-box 高度含文字行高。
                    // 追踪 is_placeholder：颜色用占位色（placeholder_render_color），与 render 一致
                    // ——颜色在此烘焙进缓存 TextLayout 的 per-run 色，render 复用缓存，故两处须同色。
                    let (content, is_placeholder) = scene
                        .controls
                        .get(id)
                        .and_then(|cs| match cs {
                            crate::scene::node::ControlState::TextField(e)
                            | crate::scene::node::ControlState::TextArea(e) => {
                                // 掩码与 measure_text_controls/render 同源（-webkit-text-security）。
                                let dv = crate::scene::control::display_value_masked(
                                    e,
                                    s.text_security.map(crate::scene::control::mask_char),
                                )
                                .0;
                                if dv.is_empty() {
                                    Some((e.placeholder.clone(), true))
                                } else {
                                    Some((dv, false))
                                }
                            }
                            crate::scene::node::ControlState::NumberField { edit, .. } => {
                                let dv = crate::scene::control::display_value_masked(
                                    edit,
                                    s.text_security.map(crate::scene::control::mask_char),
                                )
                                .0;
                                if dv.is_empty() {
                                    Some((edit.placeholder.clone(), true))
                                } else {
                                    Some((dv, false))
                                }
                            }
                            _ => None,
                        })
                        .unwrap_or_default();
                    Some(MeasureContext::Text {
                        content,
                        font_size: s.font_size,
                        line_height: s.line_height,
                        letter_spacing: s.letter_spacing,
                        align: s.text_align,
                        nowrap: s.white_space_nowrap,
                        family: s.font_family.clone(),
                        color: if is_placeholder {
                            crate::style::resolved::placeholder_render_color(
                                s.placeholder_color,
                                s.color,
                            )
                        } else {
                            s.color
                        },
                        font_weight: s.font_weight,
                        h_inset: lp(s.taffy_style.padding.left)
                            + lp(s.taffy_style.padding.right)
                            + lp(s.taffy_style.border.left)
                            + lp(s.taffy_style.border.right),
                    })
                }
                NodeKind::Image => {
                    // Look up real intrinsic dims via the node's image src (side table).
                    // 借引用查 image_sizes——src 仅用于查表，无需每帧每图节点克隆 String。
                    let src = scene.image_srcs.get(&id).map(String::as_str).unwrap_or("");
                    let s = &node.style.taffy_style;
                    let (iw, ih) = image_sizes
                        .get(src)
                        .filter(|(w, h)| *w != 0 && *h != 0)
                        .map(|&(w, h)| (w as f32, h as f32))
                        .unwrap_or((64.0, 64.0));
                    Some(MeasureContext::Image {
                        iw,
                        ih,
                        w_dim: s.size.width,
                        h_dim: s.size.height,
                    })
                }
                _ => None,
            }
        };
        // 递归子节点（先建子，再建父以便 new_with_children）。
        // 过滤纯空白 TextNode（HTML tag 间换行+缩进）——它们不应成 flex item 撑开父容器
        // 主轴或挤压兄弟（HTML 标准空白折叠行为）。被过滤的节点 taffy_ids[id.index()]
        // 保持 None，write_back 跳过、layout_rect 保持默认 0。
        //
        // rich-text-block：inline 子已被 compile_rich_runs 折进 RichText 叶子测，
        // **不递归进 taffy**——它们的 taffy_ids 保持 None，write_back 跳过、layout_rect
        // 保持默认 0（它们渲染进父 mesh，无独立 box；render 消费 text_layouts[父]）。
        // absolute 包含块（CSS 浏览器语义）：声明 absolute 且任一 inset 显式的子项，
        // taffy 父挂最近 positioned 祖先（position_declared != Static）而非 scene 父——
        // taffy 0.12 原生只按直接父布局 absolute，无「最近 positioned 祖先」概念，这里在
        // 建树期重挂补齐。inset 全 auto 的 absolute 保持直接父（浏览器 hypothetical-box
        // 静态位置语义不做，见 fence 文档已知限制）。
        let positioned =
            node.style.position_declared != crate::style::resolved::PositionDeclared::Static;
        let mut children_ids: Vec<taffy::NodeId> = Vec::new();
        let mut escaped: Vec<taffy::NodeId> = Vec::new();
        if !node.rich_text_block {
            for c in node.children.iter() {
                if is_whitespace_only_text(scene, *c) {
                    continue;
                }
                let (ctid, cesc) = build(
                    scene,
                    tree,
                    taffy_ids,
                    *c,
                    self_overflow,
                    image_sizes,
                    root_size,
                );
                escaped.extend(cesc); // 下层冒上来的，随本层定位性收编或继续上浮
                let child = scene.get_live(*c, "layout/build");
                let abs_escapee = child.style.taffy_style.position
                    == taffy::style::Position::Absolute
                    && child.style.position_declared
                        == crate::style::resolved::PositionDeclared::Absolute
                    && inset_any_explicit(&child.style.taffy_style.inset);
                if abs_escapee && !positioned {
                    escaped.push(ctid); // 本层非 positioned：继续向包含块候选上浮
                } else {
                    children_ids.push(ctid); // 含「absolute 子的包含块就是本层」的情形
                }
            }
        }
        if positioned {
            // 本层是包含块候选：收编全部下浮 escapee，一并挂名下（taffy 按 absolute 通道布局）。
            children_ids.append(&mut escaped);
        }

        let tid = if let Some(mctx) = ctx {
            // min-width=0 让 flex-shrink 生效：taffy 默认 min-size:auto 会把 measure(None) 的
            // max-content 当 min-content，阻止 shrink → 长文本不收缩、超框。设 0 放开宽度。
            // 只设宽度：文本不纵向 shrink，min-height=0 无收益却有副作用——让 flex column 父
            // 容器主轴尺寸算大（按钮等容器被撑高、底图下沿往下拉），所以 height 保留 Auto。
            // 作者显式声明的 min-width 保留（如 stat-bar 的 label/val 固定列宽）——只在
            // 未声明（Auto）时才放开 shrink。
            if style.min_size.width == taffy::style::Dimension::AUTO {
                style.min_size.width = taffy::style::Dimension::length(0.0);
            }
            // 叶子：装测量上下文。children 应为空（Text/Image 是叶子）。
            tree.new_leaf_with_context(style, mctx).unwrap()
        } else {
            tree.new_with_children(style, &children_ids).unwrap()
        };
        taffy_ids[id.index()] = Some(tid);
        (tid, escaped)
    }

    /// 任一 inset 边显式（非 auto）——absolute escapee 的判定条件之一。
    fn inset_any_explicit(
        inset: &taffy::geometry::Rect<taffy::style::LengthPercentageAuto>,
    ) -> bool {
        use taffy::style::LengthPercentageAuto;
        let auto = LengthPercentageAuto::AUTO;
        inset.left != auto || inset.right != auto || inset.top != auto || inset.bottom != auto
    }

    let (root_tid, escaped) = build(
        scene,
        &mut taffy_tree,
        &mut taffy_ids,
        scene.roots[0],
        false,
        image_sizes,
        root_size,
    );
    // 根收编余下 escapee：无任何 positioned 祖先时包含块 = 初始包含块（视口），CSS 语义。
    if !escaped.is_empty() {
        let mut kids = taffy_tree.children(root_tid).unwrap_or_default();
        kids.extend(escaped);
        taffy_tree.set_children(root_tid, &kids).unwrap();
    }

    // taffy NodeId → scene NodeId 反查，供 measure 闭包按 taffy nid 把 TextLayout
    // 存进 scene 索引的 text_layouts。render 复用，消除 layout/render 双测量不一致。
    let mut taffy_to_scene: HashMap<taffy::NodeId, NodeId> = HashMap::new();
    for n in scene.nodes.values() {
        if let Some(tid) = taffy_ids[n.id.index()] {
            taffy_to_scene.insert(tid, n.id);
        }
    }
    let mut text_layouts: Vec<Option<TextLayout>> = vec![None; scene.nodes.capacity() + 1];
    // measure memo：跨帧 carry-over。mem::take 出 scene（期间 scene.text_measure_cache 空），
    // 闭包用完在末尾写回——与 text_layouts 同模式，避 borrow 冲突（build 已在上方借过 scene）。
    let mut measure_cache: Vec<Option<crate::text::layout::TextMeasureCache>> =
        std::mem::take(&mut scene.text_measure_cache);
    let cap_need = scene.nodes.capacity() + 1;
    if measure_cache.len() < cap_need {
        measure_cache.resize(cap_need, None);
    }

    // 设根 size：覆盖为调用方给的 root_size（viewport）。
    // Style.size 字段类型是 Size<Dimension>（不是 LengthPercentageAuto）。
    let root_style = taffy_tree.style(root_tid).unwrap().clone();
    taffy_tree
        .set_style(
            root_tid,
            Style {
                size: Size {
                    width: Dimension::length(root_size.0),
                    height: Dimension::length(root_size.1),
                },
                ..root_style
            },
        )
        .ok();

    // solve：单一 FnMut 闭包按 context 分派。
    // known.width: Option<f32> —— Some=约束宽，None=不限（→ measure_text max_width=None）。
    taffy_tree
        .compute_layout_with_measure(
            root_tid,
            Size::MAX_CONTENT,
            |known: Size<Option<f32>>,
             avail: Size<AvailableSpace>,
             nid: taffy::NodeId,
             node_ctx: Option<&mut MeasureContext>,
             _style: &Style|
             -> Size<f32> {
                // 定宽容器里 auto 宽文本子（flex column 的 span 等）：taffy 传
                // known=None + avail=Definite(容器内容宽)。只用 known 会按 max-content
                // 量出单行超框（浏览器按可用宽换行）。known 缺席时回退 avail 的 Definite 宽
                // 作换行约束；MaxContent/MinContent 保持 None（走 intrinsic 测量）。
                // Definite(0)（taffy 某些 sizing 轮次会传）与 known=Some(0) 一律视作
                // 无约束：0 宽盒内浏览器文本横向溢出而非逐字竖排，且首个 Some(0) 测量
                // 会经 render 槽 Some-优先策略钉死成多行布局。
                let wrap_width = known
                    .width
                    .or(match avail.width {
                        AvailableSpace::Definite(w) => Some(w),
                        _ => None,
                    })
                    .filter(|w| *w > f32::EPSILON);
                match node_ctx {
                    None => Size::ZERO,
                    Some(MeasureContext::Image {
                        iw,
                        ih,
                        w_dim,
                        h_dim,
                    }) => {
                        let (iw, ih, wd, hd) = (*iw, *ih, *w_dim, *h_dim);
                        // taffy 0.12：Dimension 是 pub struct(CompactLength) tagged pointer，
                        // 内字段私有无法 match 变体。先取 tag/value 再用 if-else 分派。
                        // width：known.width（Percent/fit 解析后，taffy 传）> css Length > 等比 height > intrinsic。
                        //   Percent width：taffy 第二次传 known.width=Some(解析宽)。
                        //
                        // 等比分支精确复刻升级前 match 臂 `(None, Dimension::Auto, Dimension::Length(h)) => h*iw/ih`：
                        // 仅 wd==Auto 时按 height 推宽。Percent width（无可解析父）落 intrinsic iw，
                        // 不混进 height-derive。
                        let wd_is_length = wd.tag() == taffy::style::CompactLength::LENGTH_TAG;
                        let hd_is_length = hd.tag() == taffy::style::CompactLength::LENGTH_TAG;
                        let w = if let Some(v) = known.width {
                            v
                        } else if wd_is_length {
                            wd.value()
                        } else if hd_is_length && wd.is_auto() {
                            hd.value() * iw / ih
                        } else {
                            iw
                        };
                        // height：css Length > known.height > 等比 width（CSS img height:auto 默认）。
                        let h = if hd_is_length {
                            hd.value()
                        } else if let Some(v) = known.height {
                            v
                        } else {
                            w * ih / iw
                        };
                        Size {
                            width: w,
                            height: h,
                        }
                    }
                    Some(MeasureContext::Text {
                        content,
                        font_size,
                        line_height,
                        letter_spacing,
                        align,
                        nowrap,
                        family,
                        color,
                        font_weight,
                        h_inset,
                    }) => {
                        let stack = fonts.stack_for(family.as_deref());
                        // taffy 传 known.width = 节点 border-box 宽（含 padding/border）；
                        // 文字在 content area（wrap 宽 - h_inset）内换行 + 对齐，否则吃到 padding 超框。
                        let mw = wrap_width.map(|w| (w - *h_inset).max(0.0));
                        let sid_opt = taffy_to_scene.get(&nid).copied();
                        // measure memo：fingerprint 命中 → 复用 TextLayout 跳过 shaping。
                        // 两槽：mw=None→intrinsic（max-content），mw=Some→constrained（换行）。
                        // fingerprint 含 content hash → set_text / slot 换内容自动 miss。
                        let fp = crate::text::layout::text_fingerprint(
                            content,
                            *font_size,
                            *line_height,
                            *letter_spacing,
                            *align,
                            *nowrap,
                            *font_weight,
                            family.as_deref(),
                            mw,
                        );
                        let layout = if let Some(sid) = sid_opt {
                            let entry = measure_cache[sid.index()]
                                .get_or_insert_with(crate::text::layout::TextMeasureCache::default);
                            let slot = if mw.is_none() {
                                &mut entry.intrinsic
                            } else {
                                &mut entry.constrained
                            };
                            if slot.as_ref().is_some_and(|(f, _)| *f == fp) {
                                slot.as_ref().unwrap().1.clone()
                            } else {
                                let l = measure_text(
                                    content,
                                    *font_size,
                                    *line_height,
                                    *letter_spacing,
                                    *align,
                                    *nowrap,
                                    mw,
                                    &stack,
                                    *color,
                                    crate::text::rich::weight_from_font_weight(*font_weight),
                                );
                                *slot = Some((fp, l.clone()));
                                l
                            }
                        } else {
                            // 无 scene 节点映射（文本过滤/边角）：不缓存。
                            measure_text(
                                content,
                                *font_size,
                                *line_height,
                                *letter_spacing,
                                *align,
                                *nowrap,
                                mw,
                                &stack,
                                *color,
                                crate::text::rich::weight_from_font_weight(*font_weight),
                            )
                        };
                        // render 槽：存 TextLayout 供 render 复用。Some（available 测量）优先——
                        // 短文本 taffy 只传 None（max-content ≤ available，不换行），长文本传
                        // Some(available)（换行）。一旦存了 Some，后续 None 不覆盖。
                        if let Some(sid) = sid_opt {
                            let rslot = &mut text_layouts[sid.index()];
                            if rslot.is_none() || known.width.is_some() {
                                *rslot = Some(layout.clone());
                            }
                        }
                        Size {
                            width: layout.text_width,
                            height: layout.text_height,
                        }
                    }
                    Some(MeasureContext::RichText {
                        runs,
                        line_height,
                        letter_spacing,
                        align,
                        family,
                        h_inset,
                        ..
                    }) => {
                        // RichText 走 measure_rich_text（简化 inline flow）。
                        // 回退走 FontStack（per-glyph 选字体）；run.font_id 仍是主字体 id。
                        let stack = fonts.stack_for(family.as_deref());
                        // 同 Text：content area = wrap 宽（border-box）- h_inset。
                        let mw = wrap_width.map(|w| (w - *h_inset).max(0.0));
                        let sid_opt = taffy_to_scene.get(&nid).copied();
                        // 指纹 memo：runs 每帧现编译（便宜，O(inline 子)），
                        // 算指纹命中缓存跳过贵的 measure_rich_text（shaping）。span 换色/换内容
                        // → runs 变 → fp 变 → 自动 miss 重测（不依赖 dirty_text 传播）。
                        // 两槽 intrinsic/constrained（同 Text）：mw=None 走 intrinsic，
                        // mw=Some 走 constrained；约束宽量化进 fp 避亚像素抖动 thrash。
                        let fp = crate::text::layout::rich_text_fingerprint(
                            runs,
                            *line_height,
                            *letter_spacing,
                            *align,
                            family.as_deref(),
                            mw,
                        );
                        let layout = if let Some(sid) = sid_opt {
                            let entry = measure_cache[sid.index()]
                                .get_or_insert_with(crate::text::layout::TextMeasureCache::default);
                            let slot = if mw.is_none() {
                                &mut entry.intrinsic
                            } else {
                                &mut entry.constrained
                            };
                            if slot.as_ref().is_some_and(|(f, _)| *f == fp) {
                                slot.as_ref().unwrap().1.clone()
                            } else {
                                let l = crate::text::layout::measure_rich_text(
                                    runs,
                                    mw,
                                    *line_height,
                                    *letter_spacing,
                                    *align,
                                    &stack,
                                );
                                *slot = Some((fp, l.clone()));
                                l
                            }
                        } else {
                            // 无 scene 节点映射（边角）：不缓存，直接测。
                            crate::text::layout::measure_rich_text(
                                runs,
                                mw,
                                *line_height,
                                *letter_spacing,
                                *align,
                                &stack,
                            )
                        };
                        // render 槽：存 TextLayout 供 render 复用（同 Text 的 Some 优先策略：
                        // 已存 Some 且本次 None 不覆盖；本次 Some 则覆盖）。
                        if let Some(sid) = sid_opt {
                            let slot = &mut text_layouts[sid.index()];
                            if slot.is_none() || known.width.is_some() {
                                *slot = Some(layout.clone());
                            }
                        }
                        Size {
                            width: layout.text_width,
                            height: layout.text_height,
                        }
                    }
                }
            },
        )
        .ok();

    // taffy 树绝对坐标预计算：absolute escapee 的 taffy 父 ≠ scene 父，layout.location
    // 相对的是 taffy 父——scene 递归累加父 origin 的旧算法对重挂节点会错位。沿 taffy
    // 树一次性累加得每节点绝对坐标（非重挂时与 scene 树累加同值，两树同构）。
    let mut taffy_abs: HashMap<taffy::NodeId, (f32, f32)> = HashMap::new();
    fn walk_taffy_abs(
        tree: &TaffyTree<MeasureContext>,
        tid: taffy::NodeId,
        origin: (f32, f32),
        out: &mut HashMap<taffy::NodeId, (f32, f32)>,
    ) {
        let Ok(layout) = tree.layout(tid) else {
            return;
        };
        let abs = (origin.0 + layout.location.x, origin.1 + layout.location.y);
        out.insert(tid, abs);
        if let Ok(kids) = tree.children(tid) {
            for c in kids {
                walk_taffy_abs(tree, c, abs, out);
            }
        }
    }
    walk_taffy_abs(&taffy_tree, root_tid, (0.0, 0.0), &mut taffy_abs);

    // 回写 layout_rect + clip_rect（绝对坐标取 taffy 树累加值）。
    fn write_back(
        scene: &mut Scene,
        tree: &TaffyTree<MeasureContext>,
        taffy_ids: &[Option<taffy::NodeId>],
        taffy_abs: &HashMap<taffy::NodeId, (f32, f32)>,
        id: NodeId,
    ) {
        // 被过滤的节点（纯空白 TextNode）：taffy_ids 槽为 None，layout_rect 保持默认 0。
        // 早返，跳过 solve 结果回写——但递归子节点（无，TextNode 是叶子），安全。
        let tid = match taffy_ids[id.index()] {
            Some(tid) => tid,
            None => return,
        };
        let layout = tree.layout(tid).unwrap();
        let (x, y) = taffy_abs
            .get(&tid)
            .copied()
            .unwrap_or((layout.location.x, layout.location.y));
        let (w, h) = (layout.size.width, layout.size.height);
        let node = scene.get_live_mut(id, "layout/write_back");
        node.layout_rect = Rect { x, y, w, h };
        // clip_rect 按 rematch 后的 style.overflow 重派生（而非仅填充 create 时建的 Some 槽）。
        // 原因：<style> class 规则设的 overflow 走 dynamic_rules 运行时应用，打包期
        // base_style 无 overflow → create_node_from_template 时 clip_rect=None。若这里
        // 只填已有的 Some，rematch 后 overflow 虽设上但 clip_rect 仍 None → render 不裁剪。
        // 现在：任一轴 非 Visible → clip = 自身 border 框（解出的 layout_rect）；否则 None。
        let should_clip = node.style.overflow_x != OverflowMode::Visible
            || node.style.overflow_y != OverflowMode::Visible;
        node.clip_rect = if should_clip {
            Some(Rect { x, y, w, h })
        } else {
            None
        };
        let kids = node.children.clone();
        for c in kids {
            write_back(scene, tree, taffy_ids, taffy_abs, c);
        }
    }
    write_back(scene, &taffy_tree, &taffy_ids, &taffy_abs, scene.roots[0]);
    // layout 阶段 TextLayout 缓存交还 scene，供 render 复用（不重测）。
    scene.text_layouts = text_layouts;
    // measure memo 写回（跨帧持久）。
    scene.text_measure_cache = measure_cache;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{NodeKind, Scene};
    use crate::style::resolved::ResolvedStyle;

    fn font_table() -> Option<FontTable> {
        let path = format!(
            "{}/tests/fixtures/DejaVuSans.ttf",
            env!("CARGO_MANIFEST_DIR")
        );
        let bytes = std::fs::read(&path).ok()?;
        let mut ft = FontTable::new();
        ft.register("DejaVu", bytes, true).ok()?;
        Some(ft)
    }

    /// 测试辅助：空图尺寸表（无 path → 全 64×64 兜底）。
    fn empty_sizes() -> ImageSizeTable {
        HashMap::new()
    }

    /// 测试辅助：建单条 path→(w,h) 尺寸表。
    fn sizes(path: &str, w: u32, h: u32) -> ImageSizeTable {
        let mut m = HashMap::new();
        m.insert(path.to_string(), (w, h));
        m
    }

    /// Image measure 三档优先级（CSS Length > 真实像素 > 64×64 兜底）。
    /// 用 Scene::build 手搓 Image scene。
    ///
    /// **布局陷阱**：`solve` 会用 `root_size` 覆盖根节点的 taffy size（见 prod
    /// `set_style(... size: Length(root_size) ...)`），故 Image 不能做根——否则
    /// 其 MeasureContext 的 intrinsic 尺寸被 root_size 强制覆盖，测不出三档。
    /// 包一层 Container 根（idx 0），Image 做 leaf 子（idx 1），其 measure 值才生效。
    #[test]
    fn image_css_length_overrides_intrinsic() {
        // CSS width:100px height:50px → CSS 声明赢（覆盖 intrinsic 真实像素 / 64×64 兜底）。
        let mut img_style = ResolvedStyle::default();
        img_style.taffy_style.size.width = Dimension::length(100.0);
        img_style.taffy_style.size.height = Dimension::length(50.0);
        let entries = [
            (
                None,
                NodeKind::Container,
                ResolvedStyle::default(),
                Vec::new(),
                None,
                false,
                None,
                None,
                None,
                None,
            ),
            (
                Some(0),
                NodeKind::Image,
                img_style,
                Vec::new(),
                None,
                false,
                None,
                None,
                None,
                Some("x.png".into()),
            ),
        ];
        let mut scene = Scene::build(&entries);
        let fonts = font_table().expect("need font");
        solve(&mut scene, &fonts, (300.0, 300.0), &sizes("x.png", 40, 20));
        let img_id = scene.get(scene.roots[0]).unwrap().children[0];
        let r = &scene.get(img_id).unwrap().layout_rect; // Image 是 root 唯一子
        assert!(
            (r.w - 100.0).abs() < 0.1,
            "CSS length 赢：w=100，got {}",
            r.w
        );
        assert!((r.h - 50.0).abs() < 0.1, "CSS length 赢：h=50，got {}", r.h);
    }

    /// 辅助：手搓 .screen > .wrap(relative, margin-left) > .card > btn(absolute) 形态。
    /// 返回 (scene, btn NodeId)。wrap_off = wrap 相对根的 x 偏移（margin 实现）。
    fn abs_containing_block_scene(
        wrap_relative: bool,
        mid_relative: bool,
        btn_inset: bool,
    ) -> (Scene, crate::scene::NodeId) {
        use crate::style::resolved::PositionDeclared;
        use taffy::style::LengthPercentageAuto;

        let mut wrap_style = ResolvedStyle::default();
        wrap_style.taffy_style.size.width = Dimension::length(500.0);
        wrap_style.taffy_style.size.height = Dimension::length(400.0);
        wrap_style.taffy_style.margin.left = LengthPercentageAuto::length(100.0);
        if wrap_relative {
            wrap_style.position_declared = PositionDeclared::Relative;
        }

        let mut mid_style = ResolvedStyle::default(); // card：无定位
        mid_style.taffy_style.size.width = Dimension::length(200.0);
        mid_style.taffy_style.size.height = Dimension::length(100.0);
        if mid_relative {
            mid_style.taffy_style.margin.top = LengthPercentageAuto::length(30.0);
            mid_style.position_declared = PositionDeclared::Relative;
        }

        let mut btn_style = ResolvedStyle::default();
        btn_style.taffy_style.position = taffy::style::Position::Absolute;
        btn_style.position_declared = PositionDeclared::Absolute;
        btn_style.taffy_style.size.width = Dimension::length(20.0);
        btn_style.taffy_style.size.height = Dimension::length(10.0);
        if btn_inset {
            btn_style.taffy_style.inset.top = LengthPercentageAuto::length(40.0);
            btn_style.taffy_style.inset.right = LengthPercentageAuto::length(56.0);
        }

        let e = |p, kind, st| (p, kind, st, Vec::new(), None, false, None, None, None, None);
        let entries = [
            e(None, NodeKind::Container, ResolvedStyle::default()),
            e(Some(0), NodeKind::Container, wrap_style),
            e(Some(1), NodeKind::Container, mid_style),
            e(Some(2), NodeKind::Container, btn_style),
        ];
        let scene = Scene::build(&entries);
        let root = scene.roots[0];
        let wrap = scene.get(root).unwrap().children[0];
        let card = scene.get(wrap).unwrap().children[0];
        let btn = scene.get(card).unwrap().children[0];
        (scene, btn)
    }

    /// btn 的包含块 = 最近 positioned 祖先 .wrap（非直接父 .card）。
    /// 浏览器：x = wrap.x + wrap.w - right - btn.w；旧实现（直接父）= card.x + card.w - ...
    #[test]
    fn absolute_resolves_against_nearest_positioned_ancestor() {
        let (mut scene, btn) = abs_containing_block_scene(true, false, true);
        let fonts = FontTable::new();
        solve(&mut scene, &fonts, (1920.0, 1080.0), &empty_sizes());
        let r = scene.get(btn).unwrap().layout_rect;
        let expect_x = 100.0 + 500.0 - 56.0 - 20.0; // wrap 右内缘 - right - 宽
        assert!(
            (r.x - expect_x).abs() < 0.5,
            "包含块 = wrap：x≈{expect_x}（直接父 card 会得 ≈224），got {}",
            r.x
        );
        assert!(
            (r.y - 40.0).abs() < 0.5,
            "top 相对 wrap 顶部（wrap 无上偏移），got {}",
            r.y
        );
    }

    /// 中间还有一个 positioned 节点时，最近者胜（mid.margin-top=30 参与坐标）。
    #[test]
    fn absolute_nearest_positioned_wins_over_outer_one() {
        let (mut scene, btn) = abs_containing_block_scene(true, true, true);
        let fonts = FontTable::new();
        solve(&mut scene, &fonts, (1920.0, 1080.0), &empty_sizes());
        let r = scene.get(btn).unwrap().layout_rect;
        // mid 含上边距 30：top 相对 mid（border box 顶）= 30 + 40 = 70。
        assert!(
            (r.y - 70.0).abs() < 0.5,
            "最近 positioned（mid, margin-top 30）赢：y≈70，got {}",
            r.y
        );
    }

    /// 无任何 positioned 祖先 → 初始包含块（视口）：相对根而非中间层。
    #[test]
    fn absolute_without_positioned_ancestor_uses_viewport() {
        let (mut scene, btn) = abs_containing_block_scene(false, false, true);
        // 场景只设了 top/right：top 相对视口 = 40（wrap/card 偏移不参与）。
        let fonts = FontTable::new();
        solve(&mut scene, &fonts, (1920.0, 1080.0), &empty_sizes());
        let r = scene.get(btn).unwrap().layout_rect;
        assert!(
            (r.y - 40.0).abs() < 0.5,
            "无 positioned 祖先 → 视口：y≈40，got {}",
            r.y
        );
        let expect_x = 1920.0 - 56.0 - 20.0; // right 相对视口右缘
        assert!(
            (r.x - expect_x).abs() < 0.5,
            "right 相对视口：x≈{expect_x}，got {}",
            r.x
        );
    }

    /// inset 全 auto 的 absolute 不重挂（保持直接父的静态位置语义，fence 已知限制）。
    #[test]
    fn absolute_without_inset_stays_with_direct_parent() {
        let (mut scene, btn) = abs_containing_block_scene(true, false, false);
        let fonts = FontTable::new();
        solve(&mut scene, &fonts, (1920.0, 1080.0), &empty_sizes());
        let r = scene.get(btn).unwrap().layout_rect;
        let root = scene.roots[0];
        let wrap = scene.get(root).unwrap().children[0];
        let card = scene.get(wrap).unwrap().children[0];
        let card_r = scene.get(card).unwrap().layout_rect;
        assert!(
            (r.x - card_r.x).abs() < 0.5 && (r.y - card_r.y).abs() < 0.5,
            "无 inset absolute 静态位置在直接父 card 内容区起点 ({}, {})，got ({}, {})",
            card_r.x,
            card_r.y,
            r.x,
            r.y
        );
    }

    /// 无 CSS 尺寸 → 用尺寸表真实像素（40×20）。
    #[test]
    fn image_measure_uses_real_dims_when_no_css() {
        // 无 CSS 尺寸 + 尺寸表有 x.png=40×20 → intrinsic = 40×20（真实像素）。
        let mut img_style = ResolvedStyle::default();
        img_style.taffy_style.align_self = Some(AlignSelf::FLEX_START);
        let entries = [
            (
                None,
                NodeKind::Container,
                ResolvedStyle::default(),
                Vec::new(),
                None,
                false,
                None,
                None,
                None,
                None,
            ),
            (
                Some(0),
                NodeKind::Image,
                img_style,
                Vec::new(),
                None,
                false,
                None,
                None,
                None,
                Some("x.png".into()),
            ),
        ];
        let mut scene = Scene::build(&entries);
        let fonts = font_table().expect("need font");
        solve(&mut scene, &fonts, (300.0, 300.0), &sizes("x.png", 40, 20));
        let img_id = scene.get(scene.roots[0]).unwrap().children[0];
        let r = &scene.get(img_id).unwrap().layout_rect; // Image 是 root 唯一子
        assert!((r.w - 40.0).abs() < 0.1, "真实像素：w=40，got {}", r.w);
        assert!((r.h - 20.0).abs() < 0.1, "真实像素：h=20，got {}", r.h);
    }

    /// 无 CSS + 尺寸表无 path / w,h=0 → 64×64 兜底（三档第三档）。
    #[test]
    fn image_measure_uses_64_fallback_when_no_size_entry() {
        // 无 CSS + 尺寸表无 x.png → 64×64 兜底。
        let mut img_style = ResolvedStyle::default();
        img_style.taffy_style.align_self = Some(AlignSelf::FLEX_START);
        let entries = [
            (
                None,
                NodeKind::Container,
                ResolvedStyle::default(),
                Vec::new(),
                None,
                false,
                None,
                None,
                None,
                None,
            ),
            (
                Some(0),
                NodeKind::Image,
                img_style,
                Vec::new(),
                None,
                false,
                None,
                None,
                None,
                Some("x.png".into()),
            ),
        ];
        let mut scene = Scene::build(&entries);
        let fonts = font_table().expect("need font");
        solve(&mut scene, &fonts, (300.0, 300.0), &empty_sizes());
        let img_id = scene.get(scene.roots[0]).unwrap().children[0];
        let r = &scene.get(img_id).unwrap().layout_rect;
        assert!((r.w - 64.0).abs() < 0.1, "兜底：w=64，got {}", r.w);
        assert!((r.h - 64.0).abs() < 0.1, "兜底：h=64，got {}", r.h);
    }

    /// 尺寸表 w/h=0（非 PNG / 读失败）→ fallback 64×64。
    #[test]
    fn image_measure_falls_back_to_64_when_zero_dims() {
        // 尺寸表 x.png=(0,0)（非 PNG 兜底）→ fallback 64×64。
        let mut img_style = ResolvedStyle::default();
        img_style.taffy_style.align_self = Some(AlignSelf::FLEX_START);
        let entries = [
            (
                None,
                NodeKind::Container,
                ResolvedStyle::default(),
                Vec::new(),
                None,
                false,
                None,
                None,
                None,
                None,
            ),
            (
                Some(0),
                NodeKind::Image,
                img_style,
                Vec::new(),
                None,
                false,
                None,
                None,
                None,
                Some("x.png".into()),
            ),
        ];
        let mut scene = Scene::build(&entries);
        let fonts = font_table().expect("need font");
        solve(&mut scene, &fonts, (300.0, 300.0), &sizes("x.png", 0, 0));
        let img_id = scene.get(scene.roots[0]).unwrap().children[0];
        let r = &scene.get(img_id).unwrap().layout_rect;
        assert!((r.w - 64.0).abs() < 0.1, "w/h=0 → 兜底 w=64，got {}", r.w);
        assert!((r.h - 64.0).abs() < 0.1, "w/h=0 → 兜底 h=64，got {}", r.h);
    }

    /// img style="width:80px" + 真实 40×20 → height 等比 = 40（80×20/40，2:1 aspect）。
    #[test]
    fn image_measure_scales_height_to_width_aspect() {
        // img style="width:80px" intrinsic 40×20（真实，2:1）→ height 等比 = 40（80×20/40）。
        let mut img_style = ResolvedStyle::default();
        img_style.taffy_style.size.width = Dimension::length(80.0);
        img_style.taffy_style.align_self = Some(AlignSelf::FLEX_START);
        let entries = [
            (
                None,
                NodeKind::Container,
                ResolvedStyle::default(),
                Vec::new(),
                None,
                false,
                None,
                None,
                None,
                None,
            ),
            (
                Some(0),
                NodeKind::Image,
                img_style,
                Vec::new(),
                None,
                false,
                None,
                None,
                None,
                Some("x.png".into()),
            ),
        ];
        let mut scene = Scene::build(&entries);
        let fonts = font_table().expect("need font");
        solve(&mut scene, &fonts, (300.0, 300.0), &sizes("x.png", 40, 20));
        let img_id = scene.get(scene.roots[0]).unwrap().children[0];
        let r = &scene.get(img_id).unwrap().layout_rect;
        assert!((r.w - 80.0).abs() < 0.1, "w=80 (CSS)");
        assert!(
            (r.h - 40.0).abs() < 0.1,
            "h 等比=40（80×20/40，2:1 真实 aspect），got {}",
            r.h
        );
    }

    /// img style="height:60px" + 真实 40×20 → width 等比 = 120（60×40/20，2:1 aspect）。
    #[test]
    fn image_measure_scales_width_to_height_aspect() {
        // 只设 height：style="height:60px" intrinsic 40×20（真实，2:1）→ width 等比 = 120（60×40/20）。
        let mut img_style = ResolvedStyle::default();
        img_style.taffy_style.size.height = Dimension::length(60.0);
        img_style.taffy_style.align_self = Some(AlignSelf::FLEX_START);
        let entries = [
            (
                None,
                NodeKind::Container,
                ResolvedStyle::default(),
                Vec::new(),
                None,
                false,
                None,
                None,
                None,
                None,
            ),
            (
                Some(0),
                NodeKind::Image,
                img_style,
                Vec::new(),
                None,
                false,
                None,
                None,
                None,
                Some("x.png".into()),
            ),
        ];
        let mut scene = Scene::build(&entries);
        let fonts = font_table().expect("need font");
        solve(&mut scene, &fonts, (300.0, 300.0), &sizes("x.png", 40, 20));
        let img_id = scene.get(scene.roots[0]).unwrap().children[0];
        let r = &scene.get(img_id).unwrap().layout_rect;
        assert!((r.h - 60.0).abs() < 0.1, "h=60 (CSS)");
        assert!(
            (r.w - 120.0).abs() < 0.1,
            "w 等比=120（60×40/20，2:1 真实 aspect），got {}",
            r.w
        );
    }

    /// 纯空白 TextNode（HTML 元素间的换行+缩进）不应成 flex item 撑开父容器。
    ///
    /// HTML 标准行为：block/flex 容器子节点间的纯空白应折叠，不成 box/item。
    /// 修前根因：layout::build 把空白 TextNode 当 flex item，每个占一行行高
    /// （line-height 撑高）→ 后续兄弟节点被推下去 + flex-shrink:1 把它当
    /// shrinkable 内容压缩 → 卡片 img 被压成 19×48（应 48×48）。
    /// 修后：空白 TextNode 不进 taffy 树，layout_rect 保持默认 0。
    #[test]
    fn whitespace_only_text_does_not_open_flex_item() {
        // 建模：flex column 容器 > [空白 TextNode, Button]。
        // 期望：Button.y == 0（空白 text 不撑开父容器主轴）。
        let entries = [
            (
                None,
                NodeKind::Container,
                ResolvedStyle::default(),
                Vec::new(),
                None,
                false,
                None,
                None,
                None,
                None,
            ),
            (
                Some(0),
                NodeKind::TextNode,
                ResolvedStyle::default(),
                Vec::new(),
                None,
                false,
                None,
                None,
                Some("\n    ".into()),
                None,
            ),
            (
                Some(0),
                NodeKind::Button,
                ResolvedStyle::default(),
                Vec::new(),
                None,
                false,
                None,
                None,
                None,
                None,
            ),
        ];
        let mut scene = Scene::build(&entries);
        let fonts = font_table().expect("need font");
        solve(&mut scene, &fonts, (300.0, 300.0), &empty_sizes());
        let children = &scene.get(scene.roots[0]).unwrap().children;
        // TextNode 在 children[0]，Button 在 children[1]。
        let ws_id = children[0];
        let btn_id = children[1];
        let ws = scene.get(ws_id).unwrap();
        let btn = scene.get(btn_id).unwrap();
        // 空白 text 不应占主轴空间——layout_rect.h 应保持默认 0。
        assert!(
            ws.layout_rect.h.abs() < 0.1,
            "空白 TextNode h 应 0（不撑开），got {}",
            ws.layout_rect.h
        );
        // Button 应顶在 y=0（不被空白 text 推下去）。
        assert!(
            btn.layout_rect.y.abs() < 0.1,
            "Button y 应 0（空白 text 不撑开），got {}",
            btn.layout_rect.y
        );
    }

    /// 含非空白字符的 TextNode 不被过滤（防误伤 inline 间的有意空格）。
    #[test]
    fn non_whitespace_text_keeps_layout_space() {
        // "Buy" 含字母 → 正常占 flex item 空间。
        let entries = [
            (
                None,
                NodeKind::Container,
                ResolvedStyle::default(),
                Vec::new(),
                None,
                false,
                None,
                None,
                None,
                None,
            ),
            (
                Some(0),
                NodeKind::TextNode,
                ResolvedStyle::default(),
                Vec::new(),
                None,
                false,
                None,
                None,
                Some("Buy".into()),
                None,
            ),
        ];
        let mut scene = Scene::build(&entries);
        let fonts = font_table().expect("need font");
        solve(&mut scene, &fonts, (300.0, 300.0), &empty_sizes());
        let text_id = scene.get(scene.roots[0]).unwrap().children[0];
        let r = scene.get(text_id).unwrap().layout_rect;
        assert!(
            r.w > 1.0 && r.h > 1.0,
            "非空白 text 应正常测出尺寸，got w={} h={}",
            r.w,
            r.h
        );
    }

    /// rich-text-block 容器在 solve 期折叠 inline 子为单段 inline flow：build() 编译 runs
    /// → `MeasureContext::RichText` 叶子（子不递归进 taffy）→ measure 闭包走 RichText arm
    /// 调 `measure_rich_text` → TextLayout 存 `scene.text_layouts[div]`。
    ///
    /// 验收：长 ASCII 文本在窄宽（100px）下换行 → text_height / layout_rect.h 反映多行
    /// （远大于单行行高）；inline 子（TextNode）保持默认 layout_rect（无独立 box）。
    /// solve 折叠的核心契约。
    #[test]
    fn rich_text_block_measures_as_leaf_with_wrapping() {
        // root(structural Container) > div(rich_text_block, explicit width 100) > TextNode
        // 长文本。div 显式宽 100 → taffy 以 known.width=Some(100) 测 → measure_rich_text
        // 换行 → 多行。作 root 固定尺寸叶子测不到约束宽（taffy 不重测固定尺寸），故
        // 必须作子+显式宽驱动。
        let mut div_style = ResolvedStyle::default();
        div_style.taffy_style.size.width = Dimension::length(100.0);
        let entries = [
            (
                None,
                NodeKind::Container,
                ResolvedStyle::default(),
                Vec::new(),
                None,
                false,
                None,
                None,
                None,
                None,
            ),
            (
                Some(0),
                NodeKind::Container,
                div_style,
                Vec::new(),
                None,
                false,
                None,
                None,
                None,
                None,
            ),
            (
                Some(1),
                NodeKind::TextNode,
                ResolvedStyle::default(),
                Vec::new(),
                None,
                false,
                None,
                None,
                Some("The quick brown fox jumps over the lazy dog".into()),
                None,
            ),
        ];
        let mut scene = Scene::build(&entries);
        let div = scene.get(scene.roots[0]).unwrap().children[0];
        scene.get_mut(div).unwrap().rich_text_block = true;
        let fonts = font_table().expect("need font");
        solve(&mut scene, &fonts, (300.0, 1000.0), &empty_sizes());
        let layout = scene.text_layouts[div.index()]
            .as_ref()
            .expect("rich-text-block solve 应填 text_layouts[div]");
        // 单行行高（font 16 × NORMAL_LINE_HEIGHT 1.31 ≈ 21）。多行 text_height 远大于此。
        let single_line_h = 16.0 * 1.31;
        assert!(
            layout.text_height > single_line_h * 2.0,
            "rich text 应换行多行，text_height={:.1} 应 > 2×单行({:.1})",
            layout.text_height,
            single_line_h * 2.0
        );
        // layout_rect.h（taffy 解出的 border-box 高）= measure 返的 height，同样反映多行。
        let r = &scene.get(div).unwrap().layout_rect;
        assert!(
            r.h > single_line_h * 2.0,
            "div layout_rect.h={:.1} 应 > 2×单行({:.1})，反映多行换行",
            r.h,
            single_line_h * 2.0
        );
        // 折叠的 inline 子（TextNode）保持默认 layout_rect（不进 taffy，无独立 box；
        // write_back 跳过 taffy_ids=None 的节点）。
        let tn = scene.get(div).unwrap().children[0];
        let tn_rect = scene.get(tn).unwrap().layout_rect;
        assert!(
            tn_rect.w.abs() < 0.1 && tn_rect.h.abs() < 0.1,
            "folded inline child 应无独立 layout_rect（保持默认 0），got {:?}",
            tn_rect
        );
    }

    /// 回归守卫：rich_text_block=false 的 Container 仍走 `new_with_children`，
    /// 子 TextNode 正常进 taffy 测 + 走 Text measure arm（不被 rich 分支误伤）。
    /// 与上一个 rich 测试互为正反：rich 折叠 / 非 rich 正常递归。
    #[test]
    fn non_rich_text_container_recurses_children_into_taffy() {
        let entries = [
            (
                None,
                NodeKind::Container,
                ResolvedStyle::default(),
                Vec::new(),
                None,
                false,
                None,
                None,
                None,
                None,
            ),
            (
                Some(0),
                NodeKind::TextNode,
                ResolvedStyle::default(),
                Vec::new(),
                None,
                false,
                None,
                None,
                Some("Buy".into()),
                None,
            ),
        ];
        let mut scene = Scene::build(&entries);
        // rich_text_block 保持默认 false → 子 TextNode 走原 Text measure（独立 box）。
        let fonts = font_table().expect("need font");
        solve(&mut scene, &fonts, (300.0, 300.0), &empty_sizes());
        let text_id = scene.get(scene.roots[0]).unwrap().children[0];
        let r = scene.get(text_id).unwrap().layout_rect;
        assert!(
            r.w > 1.0 && r.h > 1.0,
            "structural container 子 TextNode 应正常测出尺寸，got w={} h={}",
            r.w,
            r.h
        );
        // TextNode 走 Text measure arm → 有独立 text_layouts 条目（非父 div 的 RichText 槽）。
        assert!(
            scene.text_layouts[text_id.index()].is_some(),
            "structural TextNode 应有独立 text_layouts 条目（走 Text arm，非 fold）"
        );
    }

    /// 回归（showcase quick-bar 裁剪丢失）：overflow 由 <style> class 规则设（运行时
    /// rematch 应用），打包期 base_style 无 overflow → create_node_from_template 时
    /// clip_rect=None。rematch 后 style.overflow 被设上，但 clip_rect 若不重派生 →
    /// render 不开 clip mask → 内容溢出可见。solve 的 write_back 必须按 rematch 后的
    /// style.overflow 重派生 clip_rect（而非仅填充已有 Some 槽）。
    #[test]
    fn clip_rect_rederived_from_rematched_overflow() {
        // 建 root：base_style overflow 双轴 Visible（clip_rect=None，模拟 class 规则未烘进 base）。
        let mut root_style = ResolvedStyle::default();
        root_style.overflow_x = OverflowMode::Visible;
        root_style.overflow_y = OverflowMode::Visible;
        let entries = [
            (
                None,
                NodeKind::Container,
                root_style,
                Vec::new(),
                None,
                false,
                None,
                None,
                None,
                None,
            ),
            (
                Some(0),
                NodeKind::Container,
                ResolvedStyle::default(),
                Vec::new(),
                None,
                false,
                None,
                None,
                None,
                None,
            ),
        ];
        let mut scene = Scene::build(&entries);
        // 模拟 rematch 把 overflow-y 设上（class 规则 .quick-bar{overflow-x:auto} 在运行时应用）。
        let root = scene.roots[0];
        scene.get_mut(root).unwrap().style.overflow_x = OverflowMode::Auto;
        scene.get_mut(root).unwrap().style.overflow_y = OverflowMode::Visible;
        // create 时 base 无 overflow → clip_rect 是 None（重现在 bug 现场）。
        assert!(
            scene.get(root).unwrap().clip_rect.is_none(),
            "建节点时 base 无 overflow → clip None"
        );
        let fonts = font_table().expect("need font");
        solve(&mut scene, &fonts, (300.0, 300.0), &empty_sizes());
        // solve 后：style.overflow_x=Auto（rematched）→ clip_rect 应被重派生为 Some(解出的 rect)。
        let clip = scene.get(root).unwrap().clip_rect;
        assert!(
            clip.is_some(),
            "rematch 后 overflow 非 Visible → solve 应重派生 clip_rect"
        );
        let r = clip.unwrap();
        assert!(
            (r.w - 300.0).abs() < 1e-2 && (r.h - 300.0).abs() < 1e-2,
            "clip_rect 应=root border box (300,300)，got {:?}",
            r
        );
    }

    /// flex column + align-items:center + 定宽容器内的无宽
    /// rich-text-block 文本必须单行横排（浏览器一致先验）。
    ///
    /// 回归动机：测宽链路曾把可用宽度解析成 0 → `measure_text` 以 max_w=0 逐字换行
    /// （运行时竖排、浏览器预览横排）。缓解写法 `width:100%` 之所以有效，正是因为它
    /// 给了确定的 known width 绕开了该链路。
    #[test]
    fn flex_column_centered_auto_width_text_stays_single_line() {
        // root(structural) > .qi-pool(flex column, align-items:center, width:190)
        //   > .qi-label(rich_text_block, 无显式宽) > TextNode "气 3 / 4"
        let mut pool_style = ResolvedStyle::default();
        pool_style.taffy_style.display = taffy::Display::Flex;
        pool_style.taffy_style.flex_direction = taffy::FlexDirection::Column;
        pool_style.taffy_style.align_items = Some(taffy::AlignItems::CENTER);
        pool_style.taffy_style.size.width = Dimension::length(190.0);
        let entries = [
            (
                None,
                NodeKind::Container,
                ResolvedStyle::default(),
                Vec::new(),
                None,
                false,
                None,
                None,
                None,
                None,
            ),
            (
                Some(0),
                NodeKind::Container,
                pool_style,
                Vec::new(),
                None,
                false,
                None,
                None,
                None,
                None,
            ),
            (
                Some(1),
                NodeKind::Container,
                ResolvedStyle::default(),
                Vec::new(),
                None,
                false,
                None,
                None,
                Some("qi 3 / 4".into()),
                None,
            ),
            (
                Some(2),
                NodeKind::TextNode,
                ResolvedStyle::default(),
                Vec::new(),
                None,
                false,
                None,
                None,
                Some("qi 3 / 4".into()),
                None,
            ),
        ];
        let mut scene = Scene::build(&entries);
        let pool = scene.get(scene.roots[0]).unwrap().children[0];
        let label = scene.get(pool).unwrap().children[0];
        scene.get_mut(label).unwrap().rich_text_block = true;
        let fonts = font_table().expect("need font");
        solve(&mut scene, &fonts, (300.0, 1000.0), &empty_sizes());
        let layout = scene.text_layouts[label.index()]
            .as_ref()
            .expect("rich-text-block solve 应填 text_layouts[label]");
        let single_line_h = 16.0 * 1.31;
        assert!(
            layout.text_height <= single_line_h * 1.5,
            "无宽文本在 flex column 居中容器下应单行横排，text_height={:.1} \
             （逐字竖排 ≈ {} 行）",
            layout.text_height,
            (layout.text_height / single_line_h).round()
        );
    }

    /// 视口相对长度端到端：width:50vw 在 root (800,600) solve → 400px；root_size
    /// 变（分辨率适配 set_root_size / resize）→ 下次 solve 跟随。分辨率适配的重排语言。
    #[test]
    fn viewport_width_resolves_against_root_size() {
        use crate::style::mapping::apply_decl;
        let mut st = ResolvedStyle::default();
        assert!(apply_decl(&mut st, "width", "50vw"));
        assert!(apply_decl(&mut st, "height", "10vh"));
        let entries = [
            (
                None,
                NodeKind::Container,
                ResolvedStyle::default(),
                Vec::new(),
                None,
                false,
                None,
                None,
                None,
                None,
            ),
            (
                Some(0),
                NodeKind::Container,
                st,
                Vec::new(),
                None,
                false,
                None,
                None,
                None,
                None,
            ),
        ];
        let mut scene = Scene::build(&entries);
        let fonts = font_table().expect("need font");
        solve(&mut scene, &fonts, (800.0, 600.0), &HashMap::new());
        let id = scene.get(scene.roots[0]).unwrap().children[0];
        let r = &scene.get(id).unwrap().layout_rect;
        assert!((r.w - 400.0).abs() < 0.1, "50vw @800 -> 400, got {}", r.w);
        assert!((r.h - 60.0).abs() < 0.1, "10vh @600 -> 60, got {}", r.h);
        // resize 后重排跟随
        solve(&mut scene, &fonts, (1000.0, 500.0), &HashMap::new());
        let r = &scene.get(id).unwrap().layout_rect;
        assert!((r.w - 500.0).abs() < 0.1, "50vw @1000 -> 500, got {}", r.w);
        assert!((r.h - 50.0).abs() < 0.1, "10vh @500 -> 50, got {}", r.h);
    }
}
