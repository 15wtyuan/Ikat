//! bridge 把 fence `rich_text_blocks`（ir_idx 集合）烘成 TemplateNode.rich_text_block flag。
//!
//! rich-text-block = block 容器（div 等）或 inline 级文本容器（span）且其直接子全是
//! inline 级（text/span/img）——运行时把这些 inline 子拍平成 RichRun 走 inline flow。
//! span 默认归入（inline→flex 是 taffy 兼容 hack，且
//! flex 容器+padding+被测文字子会丢测量），使 span+padding+文字 走 text+padding 整体测量。
//! bridge 是 fence ir_idx → core TemplateNode 的唯一翻译入口，flag 必须在此烘入。

use loomgui_core::asset::TemplateNode;
use loomgui_core::scene::node::NodeKind;

/// bridge 一个组件 HTML，断无 fence 诊断（rich_text 分类是打包期分类，diagnostics 空才算正常通过）。
fn bridged(html: &str) -> Vec<TemplateNode> {
    let parsed = loomgui_fence::parse_template(html, "test.html");
    assert!(
        parsed.diagnostics.is_empty(),
        "fence diags: {:?}",
        parsed.diagnostics
    );
    loomgui_pkg::bridge::bridge(&parsed).expect("bridge ok")
}

/// `<div>hello <span>world</span></div>`：根 div 是 block 容器，直接子全是 inline
/// （text "hello " + span(TextElement)）→ fence 解析把根 div 的 ir_idx 标进
/// rich_text_blocks，bridge 把根 Container 的 rich_text_block flag 烘成 true。
#[test]
fn bridge_sets_rich_text_block_flag() {
    let nodes = bridged("<div>hello <span>world</span></div>");
    // bridge 按 IrTree 顺序建节点，根 div 是 nodes[0]（Container）。
    assert_eq!(nodes[0].kind, NodeKind::Container, "nodes[0] is root div");
    assert!(
        nodes[0].rich_text_block,
        "root rich-text-block container must carry the flag"
    );
    // 内层 span(TextElement)+text 现在也归 rich_text_block（span 是 inline 级文本容器，
    // span+padding+文字 走 text+padding 整体测量，不再变形）。它被折叠进根的
    // inline flow，flag 虽设但运行时由根 consumed——这里仅校验 TextNode 叶子不持 flag。
    let non_root: Vec<&TemplateNode> = nodes.iter().skip(1).collect();
    assert!(
        non_root
            .iter()
            .all(|n| n.kind != NodeKind::TextNode || !n.rich_text_block),
        "TextNode 叶子不应持 rich_text_block flag: {:?}",
        non_root
            .iter()
            .map(|n| (n.kind, n.rich_text_block))
            .collect::<Vec<_>>()
    );
    // 内层 span 本身应是 rich_text_block（span 默认归入 rich_text 的预期）。
    let inner_span = non_root
        .iter()
        .find(|n| n.kind == NodeKind::TextElement)
        .expect("inner span exists");
    assert!(
        inner_span.rich_text_block,
        "内层 span+text 应归 rich_text_block（A 修复）"
    );
}

/// `<div><div>a</div><div>b</div></div>`：根 div 的直接子全是 block（两个 inner div），
/// fence 解析不把根标进 rich_text_blocks（block 容器无 inline 子）→ bridge flag=false。
#[test]
fn bridge_no_flag_for_structural_block() {
    let nodes = bridged("<div><div>a</div><div>b</div></div>");
    assert!(
        !nodes[0].rich_text_block,
        "structural block container must not carry the flag"
    );
}

/// #74 `<a>`：bridge 把 semantic Link 映射成 NodeKind::Link、提取 href 进
/// TemplateNode.href，并把 fence UA 烙印（#0000EE + underline + INH_COLOR bit）
/// 一并烘进 base_style；作者 color 声明覆盖 UA 蓝。
#[test]
fn bridge_maps_link_kind_href_and_ua_style() {
    use loomgui_core::style::resolved::TextDecoration;
    let nodes = bridged(r#"<div>看<a href="open-shop">商店</a></div>"#);
    assert_eq!(nodes[0].kind, NodeKind::Container);
    assert!(
        nodes[0].rich_text_block,
        "含 a 的全 inline 容器烘 rich flag"
    );
    let a = nodes
        .iter()
        .find(|n| n.kind == NodeKind::Link)
        .expect("Link node exists");
    assert_eq!(a.href.as_deref(), Some("open-shop"));
    assert_eq!(a.style.color, [0.0, 0.0, 238.0 / 255.0, 1.0], "UA 链接色");
    assert_eq!(a.style.text_decoration, TextDecoration::Underline);
    let color_bit = loomgui_core::style::dynamic::inherited_bit("color").unwrap();
    assert_eq!(
        a.style.inherited_set.0 & color_bit,
        color_bit,
        "UA 色烙 INH_COLOR bit（防 propagate 覆盖）"
    );

    // 作者 color 覆盖 UA（tag 默认先烙、作者后应用即赢）。
    let authored = bridged(r#"<div>看<a href="x" style="color:#ff0000">商店</a></div>"#);
    let a2 = authored
        .iter()
        .find(|n| n.kind == NodeKind::Link)
        .expect("Link node exists");
    assert_eq!(a2.style.color, [1.0, 0.0, 0.0, 1.0], "作者色覆盖 UA 蓝");
    assert!(
        authored
            .iter()
            .all(|n| n.href.is_none() || n.kind == NodeKind::Link),
        "href 只在 Link 节点"
    );
}
