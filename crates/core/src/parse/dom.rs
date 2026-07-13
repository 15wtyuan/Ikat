use scraper::{Html, Selector as ScraperSelector};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ElementId(pub usize);

#[derive(Debug, Clone)]
pub struct ElementData {
    pub tag: String,
    pub classes: Vec<String>,
    pub id: Option<String>,
    /// 所有 HTML 属性（src/href/data-* 等）。class/id 单独解析到上面字段，
    /// 但也保留在 attrs 里（原样字符串值）以便需要时取。
    pub attrs: HashMap<String, String>,
    pub text: Option<String>,
    pub children: Vec<ElementId>,
    pub parent: Option<ElementId>,
    /// v1.7：inline `display:block` div 的 inner HTML 原文（parse 期捕获，不经子元素解析）。
    /// desugar 期 parse_rich_markup → runs 写入 `rich_runs`。None=普通元素。
    /// 仅当 style attr 含 `display:block` 且 tag=div 时填——绕过 FENCE_TAGS 让 b/i/span/a 进 raw。
    pub raw_rich: Option<String>,
    /// desugar 期填：raw_rich 经 parse_rich_markup 的产物。build_scene 据此产 NodeKind::RichText。
    /// None=普通元素（非 block div 或 desugar 未跑）。
    pub rich_runs: Option<Vec<crate::text::rich::RichRun>>,
}

#[derive(Debug, Clone)]
pub struct ElementTree {
    pub roots: Vec<ElementId>,
    pub nodes: Vec<ElementData>,
}

/// 围栏内支持的 tag 白名单。
/// 未识别 tag（`<video>`/`<input>`/`<b>`/…）一律报错，不降级——
/// AI 可预测性口径：写什么得到什么，围栏外即失败。
const FENCE_TAGS: &[&str] = &["div", "span", "img", "button"];

/// 相邻文本节点合并为一个 Text 叶子；行内混排（元素内"文本+元素+文本"）报错。
pub fn parse_html(html: &str) -> Result<ElementTree, String> {
    let document = Html::parse_document(html);
    // 取根（body 子节点，忽略 html/head/body 包裹）
    let body_sel = ScraperSelector::parse("body").unwrap();
    let body = document.select(&body_sel).next().ok_or("no body")?;

    let mut tree = ElementTree {
        roots: Vec::new(),
        nodes: Vec::new(),
    };
    for child_node in body.children() {
        // 只关心元素子节点；文本/注释等忽略
        if child_node.value().is_element() {
            if let Some(child_el) = scraper::ElementRef::wrap(child_node) {
                let id = build_element(&mut tree, child_el, None)?;
                tree.roots.push(id);
            }
        }
    }
    Ok(tree)
}

fn build_element(
    tree: &mut ElementTree,
    el_node: scraper::ElementRef,
    parent: Option<ElementId>,
) -> Result<ElementId, String> {
    let el_val = el_node.value();
    let tag = el_val.name().to_string();
    // 围栏白名单检查：未识别 tag 一律报错，不降级。
    // parse 保证只来围栏内 tag，scene 层 match 可无 fallback。
    if !FENCE_TAGS.contains(&tag.as_str()) {
        return Err(format!(
            "围栏外元素 <{}> 不支持，用 div/span/img/button 或 l-rich",
            tag
        ));
    }
    let id_attr = el_val.attr("id").map(|s| s.to_string());
    let classes = el_val
        .attr("class")
        .map(|s| s.split_whitespace().map(|w| w.to_string()).collect())
        .unwrap_or_default();
    // 收集全部属性（含 class/id，原样字符串）——img src 等从这取。
    let attrs: HashMap<String, String> = el_val
        .attrs()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    // v1.7：inline `display:block` div → 富文本叶。捕获 inner HTML 原文（含 b/i/span/a 等未解析
    // 标签），不递归子元素（避 FENCE_TAGS 挡 b/i）。class-based display:block 不触发（MVP 限定：
    // parse 期未 cascade，无法知 class→display:block；须内联写）。raw_rich 交打包器 desugar 期
    // parse_rich_markup → runs。
    if tag == "div" && is_inline_display_block(&attrs) {
        let raw = el_node.inner_html();
        let idx = tree.nodes.len();
        tree.nodes.push(ElementData {
            tag,
            classes,
            id: id_attr,
            attrs,
            text: None,
            children: Vec::new(),
            parent,
            raw_rich: Some(raw),
            rich_runs: None,
        });
        return Ok(ElementId(idx));
    }

    // 收集直接文本子节点；若同时有文本子节点和元素子节点 → 行内混排报错
    let mut text_parts: Vec<String> = Vec::new();
    let mut element_children: Vec<scraper::ElementRef> = Vec::new();
    for child in el_node.children() {
        match child.value() {
            scraper::node::Node::Text(t) => {
                let s = t.text.trim();
                if !s.is_empty() {
                    text_parts.push(s.to_string());
                }
            }
            scraper::node::Node::Element(_) => {
                if let Some(eref) = scraper::ElementRef::wrap(child) {
                    element_children.push(eref);
                }
            }
            _ => {}
        }
    }

    let has_text = !text_parts.is_empty();
    let has_elements = !element_children.is_empty();
    // 叶子文本元素（span/裸文本/button 文本）允许 text 无 element 子；
    // 容器（div 等）允许 element 子无 text；混排报错。
    if has_text && has_elements {
        return Err(format!(
            "行内混排不支持（div 默认 flex 只装 flex item）：元素 <{}> 同时含文本与子元素；富文本用 <div style=\"display:block\">",
            tag
        ));
    }

    let text = if has_text {
        Some(text_parts.join(" "))
    } else {
        None
    };

    let idx = tree.nodes.len();
    tree.nodes.push(ElementData {
        tag,
        classes,
        id: id_attr,
        attrs,
        text,
        children: Vec::new(),
        parent,
        raw_rich: None,
        rich_runs: None,
    });
    let my_id = ElementId(idx);

    let mut children_ids = Vec::new();
    for child_el in element_children {
        let cid = build_element(tree, child_el, Some(my_id))?;
        children_ids.push(cid);
    }
    tree.nodes[idx].children = children_ids;
    Ok(my_id)
}

/// 检测元素的 inline style attr 是否含 `display:block`（MVP 限定：仅内联声明触发 rich 转换）。
/// 容忍空白变体（`display: block`/`display :block`/`display:block`），不做完整 CSS 解析——
/// 打包器 desugar 期 resolve_styles 才 cascade class 规则，parse 期只见 inline attr。
fn is_inline_display_block(attrs: &HashMap<String, String>) -> bool {
    let Some(s) = attrs.get("style") else {
        return false;
    };
    // 去所有空白后查子串 "display:block"。容忍 `display : block` / `display:block ;` 等。
    let compact: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    compact.contains("display:block")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_div_with_text() {
        let tree = parse_html(r#"<div class="panel">hello</div>"#).unwrap();
        assert_eq!(tree.roots.len(), 1);
        let root = &tree.nodes[tree.roots[0].0];
        assert_eq!(root.tag, "div");
        assert_eq!(root.classes, vec!["panel"]);
        assert_eq!(root.text.as_deref(), Some("hello"));
    }

    #[test]
    fn merges_adjacent_text_into_one_leaf() {
        let tree = parse_html(r#"<div>foo bar baz</div>"#).unwrap();
        let root = &tree.nodes[tree.roots[0].0];
        assert_eq!(root.text.as_deref(), Some("foo bar baz"));
        assert!(root.children.is_empty());
    }

    #[test]
    fn rejects_inline_mix_of_text_and_element() {
        // div 只装 flex item；文本+元素混排编译期报错
        let result = parse_html(r#"<div>hello <img src="a.png"> world</div>"#);
        assert!(result.is_err());
    }

    #[test]
    fn nested_containers() {
        let tree = parse_html(r#"<div><div><span>x</span></div></div>"#).unwrap();
        let outer = &tree.nodes[tree.roots[0].0];
        assert_eq!(outer.children.len(), 1);
        let inner = &tree.nodes[outer.children[0].0];
        assert_eq!(inner.children.len(), 1);
        let span = &tree.nodes[inner.children[0].0];
        assert_eq!(span.tag, "span");
    }

    #[test]
    fn captures_all_attributes_including_src() {
        // img 的 src 走属性而非 text；ElementData.attrs 必须保留所有 attr
        let tree = parse_html(r#"<img src="logo.png" alt="logo" data-id="42">"#).unwrap();
        let img = &tree.nodes[tree.roots[0].0];
        assert_eq!(img.tag, "img");
        assert_eq!(img.attrs.get("src").map(|s| s.as_str()), Some("logo.png"));
        assert_eq!(img.attrs.get("alt").map(|s| s.as_str()), Some("logo"));
        assert_eq!(img.attrs.get("data-id").map(|s| s.as_str()), Some("42"));
        // text 不被属性污染
        assert!(img.text.is_none());
    }

    #[test]
    fn class_and_id_also_land_in_attrs() {
        // class/id 单独字段，但 attrs 也要保留原值（不丢信息）
        let tree = parse_html(r#"<div id="main" class="a b"></div>"#).unwrap();
        let div = &tree.nodes[tree.roots[0].0];
        assert_eq!(div.id.as_deref(), Some("main"));
        assert_eq!(div.classes, vec!["a", "b"]);
        assert_eq!(div.attrs.get("id").map(|s| s.as_str()), Some("main"));
        assert_eq!(div.attrs.get("class").map(|s| s.as_str()), Some("a b"));
    }

    #[test]
    fn rejects_fence_out_element() {
        // 围栏白名单：div/span/img/button。其余 tag 一律报错，不降级。
        // AI 可预测性核心：写什么得到什么，围栏外即失败。
        let video = parse_html(r#"<video src="x.mp4"></video>"#);
        assert!(video.is_err(), "<video> 应被围栏拒绝");
        let input = parse_html(r#"<input type="text">"#);
        assert!(input.is_err(), "<input> 应被围栏拒绝");
        // 嵌套里的围栏外 tag 同样拒绝（递归 build_element 一致执法）
        let nested = parse_html(r#"<div><b>bold</b></div>"#);
        assert!(
            nested.is_err(),
            "<b> 应被围栏拒绝（富文本用 <div style=\"display:block\">）"
        );
    }

    #[test]
    fn block_div_captures_raw_rich_without_recursing() {
        // inline display:block div → 捕获 inner HTML 原文到 raw_rich，不递归子元素。
        // b/i/a 等围栏外 tag 不进 FENCE_TAGS 检查（绕过），原文保留供 desugar 期 parse_rich_markup。
        let html = r#"<div style="display:block">a <b>b</b> <a href="x">c</a></div>"#;
        let tree = parse_html(html).unwrap();
        assert_eq!(tree.roots.len(), 1);
        let div = &tree.nodes[tree.roots[0].0];
        assert_eq!(div.tag, "div");
        assert!(div.children.is_empty(), "block div 不递归子元素");
        let raw = div.raw_rich.as_ref().expect("raw_rich 已捕获");
        assert!(raw.contains("<b>b</b>"), "raw_rich 含 b 标签原文: {raw}");
        assert!(
            raw.contains(r#"<a href="x">c</a>"#),
            "raw_rich 含 a 标签原文: {raw}"
        );
        assert!(
            div.rich_runs.is_none(),
            "rich_runs 由 desugar 期填，parse 期 None"
        );
    }

    #[test]
    fn block_div_tolerates_whitespace_in_display_decl() {
        // `display: block`（冒号后空格）/ `display :block` 等变体都触发 rich 转换。
        for style in ["display:block", "display: block", "display :block"] {
            let html = format!(r#"<div style="{style}">x</div>"#);
            let tree = parse_html(&html).unwrap_or_else(|e| panic!("parse {style}: {e}"));
            let div = &tree.nodes[tree.roots[0].0];
            assert!(div.raw_rich.is_some(), "style=\"{style}\" 应触发 rich 转换");
        }
    }

    #[test]
    fn class_based_display_block_does_not_trigger_rich_at_parse() {
        // MVP 限定：class 规则里的 display:block 不触发（parse 期未 cascade class）。
        // 只内联 style attr 触发。class-based 是 follow-up（需两遍 parse）。
        let html = r#"<div class="rich">a <b>b</b></div>"#;
        // class=rich 的 div 走普通 flex 路径 → b 是围栏外 tag → 报错。
        assert!(
            parse_html(html).is_err(),
            "class-based display:block 不触发 rich，b 被围栏挡"
        );
    }

    #[test]
    fn fence_tags_all_accepted() {
        // 白名单内四种 tag 均通过（l-container 砍，与 div 同映射冗余）
        let html = r#"<div><span>x</span><img src="a.png"><button>ok</button></div>"#;
        let tree = parse_html(html).unwrap();
        assert_eq!(tree.roots.len(), 1);
    }

    #[test]
    fn ignores_head_when_parsing_body() {
        // 浏览器预览往 showcase 页 head 塞 <base>/<link>/<script>/<style>（preview-only）。
        // parse_html 只遍历 body（dom.rs:35 body_sel），head 整个不碰 → pkg.bin 不受影响。
        // 本测试锁定该假设：head 里的围栏外 tag（base/link/script）不报错，body 树正确解析。
        let html = r#"<!DOCTYPE html><html><head>
            <meta charset="utf-8">
            <base href="..">
            <link rel="stylesheet" href="preview/preview-base.css">
            <script src="preview/loom-preview.js"></script>
            <style>.x{color:red}</style>
            </head><body><div class="root">hi</div></body></html>"#;
        let tree = parse_html(html).unwrap();
        assert_eq!(tree.roots.len(), 1, "head 内容不应产生额外 root");
        let root = &tree.nodes[tree.roots[0].0];
        assert_eq!(root.tag, "div");
        assert_eq!(root.classes, vec!["root"]);
        assert_eq!(root.text.as_deref(), Some("hi"));
    }
}
