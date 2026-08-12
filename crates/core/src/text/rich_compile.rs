//! Run 编译器：把 rich-text-block 容器的 inline 子树拍平成 `Vec<RichRun>`。
//!
//! rich-text-block（fence 阶段 6.4 标记、`Node.rich_text_block=true` 的容器）在 solve/render
//! 阶段不把 inline 子当独立 box 渲染，而是折进父节点的单段 inline flow。本编译器在
//! measure 前遍历该容器的直接子（含空白 TextNode —— 与普通 layout/render 的
//! `is_whitespace_only_text` 过滤互斥：rich-text-block 的 inline 子既不进 taffy 树也不进
//! render 子遍历，过滤器碰不到，故编译器必须自己保留全部原始文本，空白折叠交给
//! `measure_rich_text` 按 HTML 语义做）。
//!
//! **source 规则**（命中测试事件路由）：每个 run 带 `source: NodeId` —— 直接挂在
//! rich-text-block 下的 TextNode → source=该 TextNode 自己；嵌套在 span（TextElement）
//! 内的 TextNode → source=最近 span（事件命 span 而非匿名文字）；Image → source=该 Image。
//! recurse_span 把当前 span 推为 source context，其 TextNode 子的 run 全部挂此 span。

use crate::layout::ImageSizeTable;
use crate::scene::node::{NodeId, NodeKind, Scene};
use crate::style::resolved::ResolvedStyle;
use crate::text::rich::{
    weight_from_font_weight, RichDeco, RichKind, RichRun, RichStyle, RichVAlign,
};

/// 编译 rich-text-block 容器的 inline 子树为扁平 run 流。
///
/// `parent` 须是 `rich_text_block=true` 的容器；fence 保证其子全为 inline
/// （TextNode / TextElement(span) / Image）。非 inline kind 在合法 scene 中不可达，
/// 防御性跳过（编译器是纯读 pass，不 panic）。
///
/// `image_sizes` 为行内图查 intrinsic 尺寸（与 `layout::solve` 的 Image measure 同表同源）；
/// 缺项或零维回退 64×64（核心不知图集，但知图尺寸 —— 与 MeasureContext::Image 的兜底一致）。
pub fn compile_rich_runs(
    scene: &Scene,
    parent: NodeId,
    image_sizes: &ImageSizeTable,
) -> Vec<RichRun> {
    let mut runs = Vec::new();
    // 取子列表副本，避免持有 parent 借用的同时再 scene.get(child)（虽同属共享借用，
    // 先拷出来让循环体每次干净地重新借 scene，零心智负担）。
    let children: Vec<NodeId> = scene
        .get(parent)
        .map(|n| n.children.clone())
        .unwrap_or_default();
    for child in children {
        let kind = scene.get(child).map(|n| n.kind);
        match kind {
            Some(NodeKind::TextNode) => {
                let text = scene.text_contents.get(&child).cloned().unwrap_or_default();
                let style = &scene.get(child).expect("live child").style;
                // 直接子 TextNode：source=自身（事件命该 TextNode）。
                runs.push(run_from_style(style, RichKind::Text { text }, child));
            }
            Some(NodeKind::TextElement) => {
                // span：自身 style 作 context，其内 TextNode 的 source=该 span。
                recurse_span(scene, child, image_sizes, &mut runs);
            }
            Some(NodeKind::Image) => {
                let src = scene.image_srcs.get(&child).cloned().unwrap_or_default();
                let (w, h) = image_run_dims(image_sizes, &src);
                let run_kind = RichKind::Image {
                    src,
                    w,
                    h,
                    valign: RichVAlign::Baseline,
                };
                let style = &scene.get(child).expect("live child").style;
                // Image 是自带语义的 inline 元素：source=该 Image 自己。
                runs.push(run_from_style(style, run_kind, child));
            }
            _ => {} // fence 保证不可达；防御跳过。
        }
    }
    runs
}

/// 递归 span 子树。span 的 computed style 作其 TextNode 子的 run context；
/// 嵌套 span 推新 context 递归。Image 子按 inline 图处理，source=该 Image 自己。
fn recurse_span(
    scene: &Scene,
    span: NodeId,
    image_sizes: &ImageSizeTable,
    runs: &mut Vec<RichRun>,
) {
    let children: Vec<NodeId> = scene
        .get(span)
        .map(|n| n.children.clone())
        .unwrap_or_default();
    for child in children {
        let kind = scene.get(child).map(|n| n.kind);
        match kind {
            Some(NodeKind::TextNode) => {
                let text = scene.text_contents.get(&child).cloned().unwrap_or_default();
                // source=span（事件命 span，不命其内匿名文字）；style=span 的 computed。
                let span_style = &scene.get(span).expect("live span").style;
                runs.push(run_from_style(span_style, RichKind::Text { text }, span));
            }
            Some(NodeKind::TextElement) => {
                // 嵌套 span：推新 context 继续递归。
                recurse_span(scene, child, image_sizes, runs);
            }
            Some(NodeKind::Image) => {
                let src = scene.image_srcs.get(&child).cloned().unwrap_or_default();
                let (w, h) = image_run_dims(image_sizes, &src);
                let run_kind = RichKind::Image {
                    src,
                    w,
                    h,
                    valign: RichVAlign::Baseline,
                };
                let img_style = &scene.get(child).expect("live image").style;
                runs.push(run_from_style(img_style, run_kind, child));
            }
            _ => {}
        }
    }
}

/// 从节点 computed style 构造 RichRun 的样式通道。
///
/// MVP 单字体：`font_id` 填 0（`measure_rich_text` 当前忽略 run.font_id，按传入 FontStack
/// 选 face —— per-run family 变体是未来）。`color`/`size_px`/`weight` 来自本节点 style
/// （per-span 变化已生效）。`RichStyle`(italic)/`RichDeco`(text-decoration) 当前
/// `ResolvedStyle` 尚无对应字段（围栏未解析这两条 CSS 属性），填默认；待围栏加这两条
/// 属性后在此接通。
fn run_from_style(style: &ResolvedStyle, kind: RichKind, source: NodeId) -> RichRun {
    RichRun {
        kind,
        color: style.color,
        font_id: 0,
        size_px: style.font_size as u16,
        weight: weight_from_font_weight(style.font_weight),
        style: RichStyle::Normal,
        deco: RichDeco::default(),
        link_id: None,
        source,
    }
}

/// 行内图 intrinsic 尺寸。复用 `layout::solve` 的 Image measure 同一套查表 + 兜底：
/// 尺寸表命中且非零 → 用之；否则 64×64（核心不知图集，但知图尺寸）。
fn image_run_dims(image_sizes: &ImageSizeTable, src: &str) -> (f32, f32) {
    image_sizes
        .get(src)
        .filter(|(w, h)| *w != 0 && *h != 0)
        .map(|&(w, h)| (w as f32, h as f32))
        .unwrap_or((64.0, 64.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::node::Node;
    use std::collections::HashMap;

    /// 构造单节点并返回其 NodeId（按 roots/children 结构定位）。test 专用便捷。
    fn mk(kind: NodeKind) -> Node {
        Node {
            kind,
            ..Default::default()
        }
    }

    /// div(rich_text_block) + TextNode "hello" → 1 run，source=TextNode 自己。
    #[test]
    fn plain_text_compiles_to_one_run() {
        let scene = Scene::from_nodes(
            vec![mk(NodeKind::Container), mk(NodeKind::TextNode)],
            vec![(0, 1)],
        );
        let div = scene.roots[0];
        let tn = scene.get(div).unwrap().children[0];
        let mut scene = scene;
        scene.text_contents.insert(tn, "hello".into());
        scene.get_mut(div).unwrap().rich_text_block = true;

        let sizes = ImageSizeTable::new();
        let runs = compile_rich_runs(&scene, div, &sizes);
        assert_eq!(runs.len(), 1);
        match &runs[0].kind {
            RichKind::Text { text } => assert_eq!(text, "hello"),
            RichKind::Image { .. } => panic!("expected Text run"),
        }
        assert_eq!(runs[0].source, tn, "直接 TextNode 子的 source=自身");
    }

    /// `<div>a <span>b</span></div>` → 2 runs：run[0] source=外层 TextNode，
    /// run[1] source=span（不是 span 内的 TextNode）。
    #[test]
    fn span_text_run_source_is_span_not_textnode() {
        // 0:div  1:TextNode "a "  2:span(TextElement)  3:TextNode "b"(in span)
        let scene = Scene::from_nodes(
            vec![
                mk(NodeKind::Container),
                mk(NodeKind::TextNode),
                mk(NodeKind::TextElement),
                mk(NodeKind::TextNode),
            ],
            vec![(0, 1), (0, 2), (2, 3)],
        );
        let div = scene.roots[0];
        let outer_tn = scene.get(div).unwrap().children[0]; // node 1
        let span = scene.get(div).unwrap().children[1]; // node 2
        let mut scene = scene;
        scene.text_contents.insert(outer_tn, "a ".into());
        scene.text_contents.insert(
            *scene.get(span).unwrap().children.first().unwrap(),
            "b".into(),
        );
        scene.get_mut(div).unwrap().rich_text_block = true;

        let sizes = ImageSizeTable::new();
        let runs = compile_rich_runs(&scene, div, &sizes);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].source, outer_tn, "外层 TextNode run source=自身");
        assert_eq!(runs[1].source, span, "span 内文字 run source=span");
        match &runs[1].kind {
            RichKind::Text { text } => assert_eq!(text, "b"),
            RichKind::Image { .. } => panic!("expected Text run"),
        }
    }

    /// `<div><span>a<span>b</span></span></div>` → 2 runs，source 各为对应 span。
    #[test]
    fn nested_span_recurses() {
        // 0:div 1:outer span 2:TextNode "a"(in 1) 3:inner span(in 1) 4:TextNode "b"(in 3)
        let scene = Scene::from_nodes(
            vec![
                mk(NodeKind::Container),
                mk(NodeKind::TextElement),
                mk(NodeKind::TextNode),
                mk(NodeKind::TextElement),
                mk(NodeKind::TextNode),
            ],
            vec![(0, 1), (1, 2), (1, 3), (3, 4)],
        );
        let div = scene.roots[0];
        let outer = scene.get(div).unwrap().children[0]; // node 1
        let inner = scene.get(outer).unwrap().children[1]; // node 3
        let tn_a = scene.get(outer).unwrap().children[0]; // node 2
        let tn_b = scene.get(inner).unwrap().children[0]; // node 4
        let mut scene = scene;
        scene.text_contents.insert(tn_a, "a".into());
        scene.text_contents.insert(tn_b, "b".into());
        scene.get_mut(div).unwrap().rich_text_block = true;

        let sizes = ImageSizeTable::new();
        let runs = compile_rich_runs(&scene, div, &sizes);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].source, outer, "外层 span 内文字 source=外层 span");
        assert_eq!(runs[1].source, inner, "内层 span 内文字 source=内层 span");
    }

    /// `<div>text <img></div>` → text run + image run（source=img 自己，w/h 查表）。
    #[test]
    fn image_run_inline() {
        // 0:div 1:TextNode "text " 2:Image
        let scene = Scene::from_nodes(
            vec![
                mk(NodeKind::Container),
                mk(NodeKind::TextNode),
                mk(NodeKind::Image),
            ],
            vec![(0, 1), (0, 2)],
        );
        let div = scene.roots[0];
        let tn = scene.get(div).unwrap().children[0]; // node 1
        let img = scene.get(div).unwrap().children[1]; // node 2
        let mut scene = scene;
        scene.text_contents.insert(tn, "text ".into());
        scene.image_srcs.insert(img, "icon.png".into());
        scene.get_mut(div).unwrap().rich_text_block = true;

        let mut sizes: ImageSizeTable = HashMap::new();
        sizes.insert("icon.png".to_string(), (32, 24));
        let runs = compile_rich_runs(&scene, div, &sizes);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].source, tn);
        match &runs[1].kind {
            RichKind::Image { src, w, h, valign } => {
                assert_eq!(src, "icon.png");
                assert!((*w - 32.0).abs() < 1e-6, "w 查表=32");
                assert!((*h - 24.0).abs() < 1e-6, "h 查表=24");
                assert_eq!(*valign, RichVAlign::Baseline);
            }
            RichKind::Text { .. } => panic!("expected Image run"),
        }
        assert_eq!(runs[1].source, img, "Image run source=Image 自己");
    }

    /// 纯空白 TextNode 子必须保留为 run（编译器不套 is_whitespace_only_text；
    /// 该过滤只作用于普通 taffy/render 子遍历，rich-text-block 的 inline 子两处都碰不到）。
    /// 这是 whitespace 折叠交给 measure_rich_text 的回归守卫 —— 若有人误在编译器里加
    /// is_whitespace_only_text 过滤，此纯空白 TextNode 会被丢，run 数变少。
    #[test]
    fn whitespace_textnode_preserved() {
        // 0:div 1:TextNode " "(纯空白) 2:span 3:TextNode "x"(in span)
        let scene = Scene::from_nodes(
            vec![
                mk(NodeKind::Container),
                mk(NodeKind::TextNode),
                mk(NodeKind::TextElement),
                mk(NodeKind::TextNode),
            ],
            vec![(0, 1), (0, 2), (2, 3)],
        );
        let div = scene.roots[0];
        let ws_tn = scene.get(div).unwrap().children[0]; // node 1（纯空白）
        let span = scene.get(div).unwrap().children[1]; // node 2
        let inner_tn = *scene.get(span).unwrap().children.first().unwrap(); // node 3
        let mut scene = scene;
        scene.text_contents.insert(ws_tn, " ".into());
        scene.text_contents.insert(inner_tn, "x".into());
        scene.get_mut(div).unwrap().rich_text_block = true;

        let sizes = ImageSizeTable::new();
        let runs = compile_rich_runs(&scene, div, &sizes);
        // 纯空白 TextNode 未被过滤 → 2 runs（空白 run + span run）。
        assert_eq!(runs.len(), 2, "纯空白 TextNode 保留为 run");
        match &runs[0].kind {
            RichKind::Text { text } => assert_eq!(text, " ", "空白原文保留（折叠在 measure 阶段）"),
            RichKind::Image { .. } => panic!("expected Text run"),
        }
        assert_eq!(runs[1].source, span);
    }

    /// image 尺寸表缺项 → 回退 64×64（与 MeasureContext::Image 兜底一致）。
    #[test]
    fn image_run_falls_back_to_64_when_no_size_entry() {
        let scene = Scene::from_nodes(
            vec![mk(NodeKind::Container), mk(NodeKind::Image)],
            vec![(0, 1)],
        );
        let div = scene.roots[0];
        let img = scene.get(div).unwrap().children[0];
        let mut scene = scene;
        scene.image_srcs.insert(img, "missing.png".into());
        scene.get_mut(div).unwrap().rich_text_block = true;

        let sizes = ImageSizeTable::new(); // 空：missing.png 不在表
        let runs = compile_rich_runs(&scene, div, &sizes);
        assert_eq!(runs.len(), 1);
        match &runs[0].kind {
            RichKind::Image { w, h, .. } => {
                assert!((*w - 64.0).abs() < 1e-6, "缺项 w=64 兜底");
                assert!((*h - 64.0).abs() < 1e-6, "缺项 h=64 兜底");
            }
            RichKind::Text { .. } => panic!("expected Image run"),
        }
    }
}
