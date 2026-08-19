//! Stage 6.4：rich-text-block 分类 + mixed inline/block 报错。必须在 Stage 6.5
//! （`inline_context_check`）之前跑——6.5 读本阶段的 `rich_text_blocks` 集合豁免 img。
//!
//! LoomGUI 运行时只在一种上下文里实现浏览器式 inline flow：**rich-text-block**——
//! `display:block` 容器且其直接子**全是 inline 级**（text / span(TextElement) / img(Image)）。
//! 此分类记进 `ParsedTemplate.rich_text_blocks`，packer bridge 据此烘 flag，runtime 把这些
//! inline 子拍平成 `RichRun` 走 `measure_rich_text`（见 main-design 文本模型）。
//!
//! 若 block 容器的直接子**既有 inline 级又有 block 级**（如 `<div><span>x</span><div>y</div></div>`），
//! inline flow 不可定义（一部分要横排流、一部分要撑满竖排，同一 formatting context 无解）→
//! 打包期 `FenceMixedInlineBlock` error，逼作者显式选边。`display:flex` 容器不参与（其子是
//! flex item，走 flex 排版），故 showcase 的 label+控件 flex 布局不会触发本检查。
//!
//! **空白折叠**：纯空白文本子（缩进/换行，HTML 里块元素之间的常见装饰空白）视为**中性**——
//! 既不算 inline 也不算 block。否则任何缩进的 block 模板都会被误判成 mixed。这与浏览器
//! 对块间空白的折叠语义一致。

use crate::diagnostic::{Diagnostic, DiagnosticCode, LineMap};
use crate::inline_context_check::{collect_flex_class_rules, is_flex_context};
use crate::ir::{IrNodeKind, IrTree};
use crate::schema::tag::SemanticKind;
use loomgui_core::style::resolved::ResolvedStyle;

/// 一个直接子在 rich-text 分类里的角色。
#[derive(Clone, Copy, PartialEq, Eq)]
enum ChildRole {
    /// text(非纯空白) / span(TextElement) / img(Image) —— rich-text inline run 候选。
    Inline,
    /// div / 控件 / template / 自定义元素等 —— 撑满竖排的 block-level。
    Block,
    /// 纯空白文本 / 注释 / doctype —— 不参与分类（见模块文档「空白折叠」）。
    Neutral,
}

fn classify_child(kind: &IrNodeKind) -> ChildRole {
    match kind {
        IrNodeKind::Text(t) => {
            if t.trim().is_empty() {
                ChildRole::Neutral
            } else {
                ChildRole::Inline
            }
        }
        IrNodeKind::Element(el) => {
            if matches!(
                el.semantic,
                Some(SemanticKind::TextElement | SemanticKind::Image)
            ) {
                ChildRole::Inline
            } else {
                ChildRole::Block
            }
        }
        IrNodeKind::Comment(_) | IrNodeKind::Doctype { .. } => ChildRole::Neutral,
    }
}

/// 节点是否为 rich-text 分类候选：block 容器（div 等）或 inline 级文本容器（span）。
///
/// - div 等：须 `display:block` 且未被 class 规则改成 flex（复用 6.5 的 flex 判定）。
/// - span（TextElement）：inline 级文本容器，默认走 rich_text inline flow（text+padding
///   整体测量）。LoomGUI 的 inline→flex 是 taffy 兼容 hack，但「flex 容器 + padding +
///   被测文字子」会丢子节点测量（span+padding+文字 变形 bug），故 span 默认归
///   rich_text_block。仅当作者显式 `display:flex` 才保留 flex（作者要 flex 排版）。
fn is_block_container(
    idx: usize,
    tree: &IrTree,
    styles: &[ResolvedStyle],
    single_compound_flex_rules: &[&loomgui_core::style::dynamic::Compound],
    has_multi_compound_flex_rule: bool,
) -> bool {
    let IrNodeKind::Element(el) = &tree.nodes[idx].kind else {
        return false;
    };
    // span：inline 级文本容器，默认 rich_text 候选（除非作者显式 display:flex）。
    if matches!(el.semantic, Some(SemanticKind::TextElement)) {
        return !has_explicit_display_flex(el);
    }
    // CustomElement host：light 子在打包期被投影进组件 slot（component-system spec），
    // host 最终子树来自组件模板——页面文件里的 light 子混排不是 inline-flow 上下文。
    if matches!(el.semantic, Some(SemanticKind::CustomElement)) {
        return false;
    }
    // 非 span（div 等）：css_resolve 已把 tag 默认 + inline style 的 display 烘进
    // taffy_style.display。只 Block 参与（Flex → flex item；None(template) → 不排版）。
    if styles[idx].taffy_style.display != taffy::Display::Block {
        return false;
    }
    // class 规则把容器改成 flex → 子是 flex item，不是 inline flow 上下文。
    !is_flex_context(
        el,
        &styles[idx],
        single_compound_flex_rules,
        has_multi_compound_flex_rule,
    )
}

/// 元素的 inline style 是否显式声明 `display:flex`（作者要 flex 排版，非 inline 默认 hack）。
/// 用于区分 span 的「默认 inline→flex」（应归 rich_text）与「作者显式 flex」（保留 flex）。
/// 粗扫 style 串里 `display:` 后跟 `flex`（容空白/大小写）——围栏值已规范化，无需全解析。
fn has_explicit_display_flex(el: &crate::ir::IrElement) -> bool {
    let Some(style) = el.attributes.iter().find(|a| a.name == "style") else {
        return false;
    };
    for decl in style.value.split(';') {
        let mut parts = decl.split(':');
        if parts
            .next()
            .map(|p| p.trim().eq_ignore_ascii_case("display"))
            .unwrap_or(false)
        {
            if let Some(v) = parts.next() {
                if v.trim().eq_ignore_ascii_case("flex") {
                    return true;
                }
            }
        }
    }
    false
}

/// 分类所有 block 容器：全 inline 直接子 → rich-text-block；inline+block 混合 → error。
///
/// 返回 `(rich_text_block ir_idx 集合, mixed 诊断)`。`is_block_container` 的 display 判定
/// 复用 Stage 6.5 的 helper（inline style + tag 默认 + 单 compound class flex 规则；多 compound
/// 规则保守放行），保证两阶段对「parent 是 block 还是 flex」结论一致。
pub fn classify_rich_text(
    tree: &IrTree,
    styles: &[ResolvedStyle],
    dynamic_rules: &[loomgui_core::style::dynamic::DynamicRule],
    file: &str,
    line_map: &LineMap,
) -> (Vec<usize>, Vec<Diagnostic>) {
    let (single_compound_flex_rules, has_multi_compound_flex_rule) =
        collect_flex_class_rules(dynamic_rules);

    let mut rich = Vec::new();
    let mut diagnostics = Vec::new();

    for (idx, node) in tree.nodes.iter().enumerate() {
        if !is_block_container(
            idx,
            tree,
            styles,
            &single_compound_flex_rules,
            has_multi_compound_flex_rule,
        ) {
            continue;
        }

        let mut inline_cnt = 0usize;
        let mut block_cnt = 0usize;
        for child in &node.children {
            match classify_child(&tree.nodes[child.0].kind) {
                ChildRole::Inline => inline_cnt += 1,
                ChildRole::Block => block_cnt += 1,
                ChildRole::Neutral => {}
            }
        }

        // 无 inline 子（全 block / 空 / 仅中性）→ 不分类。
        if inline_cnt == 0 {
            continue;
        }
        if block_cnt == 0 {
            // 全 inline → rich-text-block。
            rich.push(idx);
        } else {
            // inline + block 混合 → 报错。定位在容器本身（作者要改的就是这里）。
            let IrNodeKind::Element(el) = &node.kind else {
                continue;
            };
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::FenceMixedInlineBlock,
                format!(
                    "<{tag}> mixes inline children (text/span/img) with block children \
                     (div/controls/template/slot). LoomGUI rich-text inline flow requires the \
                     direct children to be ALL inline; a mix is undefined (some want horizontal \
                     flow, some want to fill width and stack). Fix (pick one): \
                     (1) wrap the inline children inside a single child <div> so this container's \
                     direct children are all block-level; \
                     (2) set display:flex on this container so every child becomes a flex item.",
                    tag = el.tag
                ),
                line_map.source_location(node.span.start, file.to_string()),
            ));
        }
    }

    (rich, diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::parse_template;

    /// 全 inline 直接子 → 根 div 进 rich_text_blocks，且无 mixed 诊断。
    #[test]
    fn all_inline_classified() {
        let out = parse_template(r#"<div>Hello <span>x</span></div>"#, "t.html");
        let root = out.tree.roots[0].0;
        assert!(out.rich_text_blocks.contains(&root));
        assert!(out
            .diagnostics
            .iter()
            .all(|d| d.code != DiagnosticCode::FenceMixedInlineBlock));
    }

    /// 纯空白文本子视为中性：缩进的 block 结构不被误判 mixed，也不标 rich-text-block。
    #[test]
    fn whitespace_only_text_is_neutral() {
        let out = parse_template(
            "<div>\n    <div>a</div>\n    <div>b</div>\n</div>",
            "t.html",
        );
        let root = out.tree.roots[0].0;
        assert!(
            !out.rich_text_blocks.contains(&root),
            "全 block 子（含装饰空白）不应标 rich-text-block"
        );
        assert!(
            out.diagnostics
                .iter()
                .all(|d| d.code != DiagnosticCode::FenceMixedInlineBlock),
            "块间装饰空白不应触发 mixed: {:?}",
            out.diagnostics
        );
    }
}
