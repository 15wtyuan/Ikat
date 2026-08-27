//! Run 编译器：把 rich-text-block 容器的 inline 子树拍平成 `Vec<RichRun>`。
//!
//! rich-text-block（`Node.rich_text_block=true` 的容器）在 solve/render
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
//!
//! **link 上下文**（#74 `<a>`）：`<a>` 子树所有 run 的 source=该 a、link_id=Some(a)——
//! 嵌套 span 的 run 也归 a（HTML 先验：点链接内任何文字都是点链接，span 自身事件让位）。
//! style 仍取各 inline 节点自己的 computed（span 不声明 color 时按继承拿到 a 的 UA 链接色）。

use crate::layout::ImageSizeTable;
use crate::scene::node::{NodeId, NodeKind, Scene};
use crate::style::resolved::{ResolvedStyle, TextDecoration};
use crate::text::rich::{
    weight_from_font_weight, RichDeco, RichKind, RichRun, RichStyle, RichVAlign, TextDecoLines,
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
                runs.push(run_from_style(style, RichKind::Text { text }, child, None));
            }
            Some(NodeKind::TextElement) => {
                // span：自身 style 作 context，其内 TextNode 的 source=该 span。
                recurse_span(scene, child, image_sizes, &mut runs, None);
            }
            Some(NodeKind::Link) => {
                // `<a>`（#74）：像 span 一样递归子树，但推 a 自己作 source/link 上下文——
                // 子树所有 run（含嵌套 span 的）source=该 a、link_id=Some(a)。style 取各
                // inline 节点自己的 computed（a 的 UA 烙印在其 style，直挂文本直接吃到）。
                recurse_link(scene, child, image_sizes, &mut runs);
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
                runs.push(run_from_style(style, run_kind, child, None));
            }
            _ => {} // fence 保证不可达；防御跳过。
        }
    }
    runs
}

/// 递归 span 子树。span 的 computed style 作其 TextNode 子的 run context；
/// 嵌套 span 推新 context 递归。Image 子按 inline 图处理，source=该 Image 自己。
/// `link` 非 None（本 span 在某 `<a>` 子树内）时 TextNode run 的 source/link_id 覆盖为
/// 该 a（HTML 先验：点链接内任何文字都是点链接）；嵌套 span 递归时透传。
fn recurse_span(
    scene: &Scene,
    span: NodeId,
    image_sizes: &ImageSizeTable,
    runs: &mut Vec<RichRun>,
    link: Option<NodeId>,
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
                // link 上下文里 source 让位给 a（run 仍用 span 的 style——不声明 color 时
                // 按继承拿到 a 的链接色，声明则正常覆盖）。
                let span_style = &scene.get(span).expect("live span").style;
                let source = link.unwrap_or(span);
                runs.push(run_from_style(
                    span_style,
                    RichKind::Text { text },
                    source,
                    link,
                ));
            }
            Some(NodeKind::TextElement) => {
                // 嵌套 span：推新 context 继续递归（link 上下文透传）。
                recurse_span(scene, child, image_sizes, runs, link);
            }
            Some(NodeKind::Link) => {
                // a-in-a 围栏已拒（FenceLinkInvalidChild）；防御跳过，不折双层链接。
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
                runs.push(run_from_style(img_style, run_kind, child, None));
            }
            _ => {}
        }
    }
}

/// 递归 `<a>` 子树（#74）：a 的 computed style 作其直接 TextNode 子的 run context
/// （UA 烙印的链接色/underline 在此吃到），子 span 走 `recurse_span` 并透传 a 作
/// link 上下文。围栏保证 a 子树只含 TextNode 与非 flex span（img-in-a 已拒）；
/// Image 臂防御性保留（照 span 递归的口径）。
fn recurse_link(scene: &Scene, a: NodeId, image_sizes: &ImageSizeTable, runs: &mut Vec<RichRun>) {
    let children: Vec<NodeId> = scene.get(a).map(|n| n.children.clone()).unwrap_or_default();
    for child in children {
        let kind = scene.get(child).map(|n| n.kind);
        match kind {
            Some(NodeKind::TextNode) => {
                let text = scene.text_contents.get(&child).cloned().unwrap_or_default();
                // source=a + link_id=a：命中/事件归链接，不归内部匿名文字。
                let a_style = &scene.get(a).expect("live link").style;
                runs.push(run_from_style(a_style, RichKind::Text { text }, a, Some(a)));
            }
            Some(NodeKind::TextElement) => {
                // 链接内 span：span style 作 run context（未声明 color 按继承拿链接色），
                // source/link 归 a（recurse_span 的 link 覆盖语义）。
                recurse_span(scene, child, image_sizes, runs, Some(a));
            }
            Some(NodeKind::Link) => {
                // a-in-a 围栏已拒；防御跳过。
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
                runs.push(run_from_style(img_style, run_kind, child, None));
            }
            _ => {}
        }
    }
}

/// 从节点 computed style 构造 RichRun 的样式通道。
///
/// MVP 单字体：`font_id` 填 0（`measure_rich_text` 当前忽略 run.font_id，按传入 FontStack
/// 选 face —— per-run family 变体是未来）。`color`/`size_px`/`weight` 来自本节点 style
/// （per-span 变化已生效）。`deco` 接 `ResolvedStyle.text_decoration`（#74，none|underline）。
/// `link_id`：run 所属 `<a>`（编译期由 link 上下文传入），None=非链接 run。
/// `RichStyle`(italic) 围栏未收 font-style:italic，填默认。
fn run_from_style(
    style: &ResolvedStyle,
    kind: RichKind,
    source: NodeId,
    link: Option<NodeId>,
) -> RichRun {
    let deco = match style.text_decoration {
        TextDecoration::None => RichDeco::default(),
        TextDecoration::Underline => RichDeco {
            lines: TextDecoLines::UNDERLINE,
            ..Default::default()
        },
    };
    RichRun {
        kind,
        color: style.color,
        font_id: 0,
        size_px: style.font_size as u16,
        weight: weight_from_font_weight(style.font_weight),
        style: RichStyle::Normal,
        deco,
        // link_id 是 RichRun 的 u32 槽（RichFragment 同宽）；NodeId.0 是 u64
        // （含 generation），按 index 截断——命中侧只把 id 当回查 a 的 key，
        // 真正的节点解析走 run.source（同代 NodeId）。
        link_id: link.map(|id| id.0 as u32),
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

    /// #74：`<div>看<a>商店</a></div>` → a 直接文本 run 的 source=a 且 link_id=Some(a)，
    /// deco/style 取 a 的 computed（UA 烙印 underline 在此吃到）。
    #[test]
    fn link_text_run_source_and_link_id_is_a() {
        // 0:div 1:TextNode "看" 2:a(Link) 3:TextNode "商店"(in a)
        let scene = Scene::from_nodes(
            vec![
                mk(NodeKind::Container),
                mk(NodeKind::TextNode),
                mk(NodeKind::Link),
                mk(NodeKind::TextNode),
            ],
            vec![(0, 1), (0, 2), (2, 3)],
        );
        let div = scene.roots[0];
        let outer_tn = scene.get(div).unwrap().children[0];
        let a = scene.get(div).unwrap().children[1];
        let a_tn = *scene.get(a).unwrap().children.first().unwrap();
        let mut scene = scene;
        scene.text_contents.insert(outer_tn, "看".into());
        scene.text_contents.insert(a_tn, "商店".into());
        // a 的 UA 烙印模拟（打包期产物）：链接色 + underline。
        {
            let an = scene.get_mut(a).unwrap();
            an.style.color = [0.0, 0.0, 0.933_333_34, 1.0];
            an.style.text_decoration = crate::style::resolved::TextDecoration::Underline;
        }
        scene.get_mut(div).unwrap().rich_text_block = true;

        let sizes = ImageSizeTable::new();
        let runs = compile_rich_runs(&scene, div, &sizes);
        assert_eq!(runs.len(), 2);
        // 外层文本 run 无链接归属（source=自身、link_id=None）。
        assert_eq!(runs[0].source, outer_tn);
        assert_eq!(runs[0].link_id, None);
        // a 内文本 run：source=a、link_id=Some(a.0)、色/underline 取 a 的 computed。
        assert_eq!(runs[1].source, a, "链接内文本 source=a（非内部 TextNode）");
        assert_eq!(runs[1].link_id, Some(a.0 as u32));
        assert_eq!(runs[1].color, [0.0, 0.0, 0.933_333_34, 1.0]);
        assert!(runs[1].deco.lines.underline(), "a 的 UA underline 烙进 run");
        assert!(!runs[1].deco.lines.strike());
    }

    /// #74：`<div><a><span>嵌套</span></a></div>` → 嵌套 span 的 run 也归 a
    /// （source=a、link_id=Some(a)）；span 自身声明色照常生效（style 仍取 span）。
    #[test]
    fn nested_span_inside_link_resolves_to_a() {
        // 0:div 1:a(Link) 2:span(in a) 3:TextNode "x"(in span)
        let scene = Scene::from_nodes(
            vec![
                mk(NodeKind::Container),
                mk(NodeKind::Link),
                mk(NodeKind::TextElement),
                mk(NodeKind::TextNode),
            ],
            vec![(0, 1), (1, 2), (2, 3)],
        );
        let div = scene.roots[0];
        let a = scene.get(div).unwrap().children[0];
        let span = *scene.get(a).unwrap().children.first().unwrap();
        let tn = *scene.get(span).unwrap().children.first().unwrap();
        let mut scene = scene;
        scene.text_contents.insert(tn, "x".into());
        scene.get_mut(span).unwrap().style.color = [0.0, 0.5, 0.0, 1.0];
        scene.get_mut(div).unwrap().rich_text_block = true;

        let sizes = ImageSizeTable::new();
        let runs = compile_rich_runs(&scene, div, &sizes);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].source, a, "链接内 span 的 run source=a（让位）");
        assert_eq!(runs[0].link_id, Some(a.0 as u32));
        assert_eq!(runs[0].color, [0.0, 0.5, 0.0, 1.0], "色取 span 自己的声明");
    }

    /// #74 守卫：无 `<a>` 时行为不变（span run 的 link_id=None、source=span）。
    #[test]
    fn span_without_link_keeps_link_id_none() {
        let scene = Scene::from_nodes(
            vec![
                mk(NodeKind::Container),
                mk(NodeKind::TextElement),
                mk(NodeKind::TextNode),
            ],
            vec![(0, 1), (1, 2)],
        );
        let div = scene.roots[0];
        let span = scene.get(div).unwrap().children[0];
        let tn = *scene.get(span).unwrap().children.first().unwrap();
        let mut scene = scene;
        scene.text_contents.insert(tn, "s".into());
        scene.get_mut(div).unwrap().rich_text_block = true;

        let sizes = ImageSizeTable::new();
        let runs = compile_rich_runs(&scene, div, &sizes);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].source, span);
        assert_eq!(runs[0].link_id, None, "无链接上下文 link_id=None");
    }
}
