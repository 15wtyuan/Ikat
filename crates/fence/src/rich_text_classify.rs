//! Stage 6.4：rich-text-block 分类 + mixed inline/block 报错。必须在 Stage 6.5
//! （`inline_context_check`）之前跑——6.5 读本阶段的 `rich_text_blocks` 集合豁免 img。
//!
//! LoomGUI 运行时只在一种上下文里实现浏览器式 inline flow：**rich-text-block**——
//! `display:block` 容器且其直接子**全是 inline 级**（text / span(TextElement) / img(Image)）。
//! 此分类记进 `ParsedTemplate.rich_text_blocks`，packer bridge 据此烘 flag，runtime 把这些
//! inline 子拍平成 `RichRun` 走 `measure_rich_text`。
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

/// span（TextElement）子是否声明了显式 `display:flex`（inline style 或静态可判定的
/// 单 compound class 规则；多 compound 规则保守视为 flex）。
///
/// 显式 flex 的 span 在浏览器里外层显示类型是块级（`display:flex` 同时改内外显示
/// 类型），且运行时它被豁免出 rich 折叠、自身是 flex 容器——它在父容器的分类里
/// 必须算 **block 子**：若仍按 inline 折叠进父的 rich 流，slot 投影进来的块级内容
/// 会被整棵折成一行（或被防御性跳过而隐身）。
fn span_declares_flex(
    el: &crate::ir::IrElement,
    single_compound_flex_rules: &[&loomgui_core::style::dynamic::Compound],
    has_multi_compound_flex_rule: bool,
) -> bool {
    has_explicit_display_flex(el)
        || crate::inline_context_check::statically_declares_display(
            el,
            single_compound_flex_rules,
            has_multi_compound_flex_rule,
        )
}

/// 一个直接子在 rich-text 分类里的角色。
#[derive(Clone, Copy, PartialEq, Eq)]
enum ChildRole {
    /// text(非纯空白) / span(TextElement) / img(Image) —— rich-text inline run 候选。
    Inline,
    /// div / 控件 / template / 自定义元素 / 显式 flex 的 span —— 撑满竖排的 block-level。
    Block,
    /// 纯空白文本 / 注释 / doctype —— 不参与分类（见模块文档「空白折叠」）。
    Neutral,
}

fn classify_child(
    kind: &IrNodeKind,
    single_compound_flex_rules: &[&loomgui_core::style::dynamic::Compound],
    has_multi_compound_flex_rule: bool,
) -> ChildRole {
    match kind {
        IrNodeKind::Text(t) => {
            if t.trim().is_empty() {
                ChildRole::Neutral
            } else {
                ChildRole::Inline
            }
        }
        IrNodeKind::Element(el) => match el.semantic {
            Some(SemanticKind::Image) => ChildRole::Inline,
            Some(SemanticKind::TextElement) => {
                if span_declares_flex(el, single_compound_flex_rules, has_multi_compound_flex_rule)
                {
                    ChildRole::Block
                } else {
                    ChildRole::Inline
                }
            }
            // `<a>`（#74）：inline 级 run 候选（同 span 口径，含 flex 门控——声明
            // display:flex 的 a 外层块级，算 Block 子）。
            Some(SemanticKind::Link) => {
                if span_declares_flex(el, single_compound_flex_rules, has_multi_compound_flex_rule)
                {
                    ChildRole::Block
                } else {
                    ChildRole::Inline
                }
            }
            _ => ChildRole::Block,
        },
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
    // span：inline 级文本容器，默认 rich_text 候选。作者要 flex 排版时不折叠——
    // inline style 显式 flex 或 class 规则 flex 都算（与浏览器 flex item blockify
    // 对齐）。注意不能走 is_flex_context：它把「tag 默认 Flex」也算 flex，而 span
    // 的默认恰是 Flex（inline→flex hack），会把所有 span 误判成显式 flex。跨宇宙
    // （投影内容对组件 CSS）的 flex 由运行时 rematch 翻转兜底。
    if matches!(el.semantic, Some(SemanticKind::TextElement)) {
        return !span_declares_flex(el, single_compound_flex_rules, has_multi_compound_flex_rule);
    }
    // CustomElement host：light 子在打包期被投影进组件 slot，
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

        // 纯 span（无显式 flex 的 TextElement）直接包 <slot>：投影进来的 light 子
        // 落在 inline 上下文——块级子被折进 rich 流或按 flex-row hack 横排，无法按
        // 自身 display 参与宿主布局（浏览器里 slotted 节点正常参与布局）。打包期
        // 报错，逼作者把 slot 放进 div 或给 span 显式 display:flex。
        if let IrNodeKind::Element(el) = &node.kind {
            if el.semantic == Some(SemanticKind::TextElement)
                && node.children.iter().any(|c| {
                    matches!(
                        &tree.nodes[c.0].kind,
                        IrNodeKind::Element(ce) if ce.semantic == Some(SemanticKind::Slot)
                    )
                })
            {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::FenceSlotInInlineContext,
                    format!(
                        "<{}> hosts <slot> without display:flex: slot-projected children \
                         land in an inline context and cannot participate in host layout \
                         with their own display (block children fold into one inline line \
                         or get skipped). Fix: move the slot into a <div>, or declare \
                         display:flex on the span",
                        el.tag
                    ),
                    line_map.source_location(node.span.start, file.to_string()),
                ));
                continue;
            }
        }

        let mut inline_cnt = 0usize;
        let mut block_cnt = 0usize;
        for child in &node.children {
            match classify_child(
                &tree.nodes[child.0].kind,
                &single_compound_flex_rules,
                has_multi_compound_flex_rule,
            ) {
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
                     (2) set display:flex on this container so every child becomes a flex item. \
                     For decorated frames (absolute-positioned background image behind foreground \
                     content), the canonical pattern is display:flex + align-items:center + \
                     justify-content:center on the frame.",
                    tag = el.tag
                ),
                line_map.source_location(node.span.start, file.to_string()),
            ));
        }
    }

    (rich, diagnostics)
}

/// #74 `<a>` 链接专项检查：href 必填、rich-text-block 上下文、子内容白名单。
///
/// `<a>` 的折叠渲染模型（子树折进父 inline flow，runs 烙 link_id/source=a）只在
/// rich-text-block 上下文成立，故三项都在打包期显式拒绝（不静默降级成普通 span）：
///
/// 1. **href**：缺失或 trim 后为空 → `FenceLinkHrefRequired`。href 是链接的身份
///    标识（opaque 字符串，无 URI 解析语义），空链接是不可交互的死元素。
/// 2. **上下文**：直接父必须是「非 flex 的 TextElement（span 自身就是 rich 候选）」
///    或 `rich_text_blocks` 集合内的容器（a 是其 inline 子）。其余（flex 容器 /
///    裸 block 容器 / slot / template / 链接内链接）→ `FenceLinkOutsideRich`。
///    只查直接父即足够：合法上下文经 rich 容器或 span 逐层嵌套，合法性必然在
///    直接父 manifested；a-in-a 的诊断归 3（外层报 `FenceLinkInvalidChild`），
///    内层 a 跳过上下文报错避免双报。
/// 3. **子内容**：直接子元素只许非 flex TextElement（span）；`<a>`/`<img>`/其它
///    元素 → `FenceLinkInvalidChild`（文案点名 a-in-a 与 img-in-a 两种写法）。
///
/// 须在 `classify_rich_text` 之后跑（消费其 `rich_text_blocks` 产物）。
pub(crate) fn check_links(
    tree: &IrTree,
    rich_text_blocks: &[usize],
    dynamic_rules: &[loomgui_core::style::dynamic::DynamicRule],
    file: &str,
    line_map: &LineMap,
) -> Vec<Diagnostic> {
    let (single_compound_flex_rules, has_multi_compound_flex_rule) =
        collect_flex_class_rules(dynamic_rules);
    let mut diagnostics = Vec::new();
    for node in tree.nodes.iter() {
        let IrNodeKind::Element(el) = &node.kind else {
            continue;
        };
        if el.semantic != Some(SemanticKind::Link) {
            continue;
        }

        // 1. href 必填且非空白（opaque 标识符；trim 后空等同缺失）。
        let href_ok = el
            .attributes
            .iter()
            .find(|a| a.name == "href")
            .is_some_and(|a| !a.value.trim().is_empty());
        if !href_ok {
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::FenceLinkHrefRequired,
                "<a> requires a non-empty href attribute (opaque link id; no URI \
                 semantics). An <a> without href is a dead element that can never \
                 raise a link event — write the target id, e.g. <a href=\"open-shop\">"
                    .to_string(),
                line_map.source_location(node.span.start, file.to_string()),
            ));
        }

        // 2. 上下文：直接父 = 非 flex TextElement 或 rich_text_blocks 容器。
        //    a-in-a 跳过（外层已报 FenceLinkInvalidChild，避免双报）；根级 a
        //    （无父，不在任何 rich 上下文）照报。
        let outside = match node.parent {
            None => true,
            Some(parent_id) => {
                let parent = &tree.nodes[parent_id.0];
                let parent_legal = match &parent.kind {
                    IrNodeKind::Element(pel) => {
                        (matches!(pel.semantic, Some(SemanticKind::TextElement))
                            && !span_declares_flex(
                                pel,
                                &single_compound_flex_rules,
                                has_multi_compound_flex_rule,
                            ))
                            || rich_text_blocks.contains(&parent_id.0)
                    }
                    _ => false,
                };
                // a-in-a：内层 a 的父是 Link → 上下文错误跳过（外层的子检查已报）。
                !parent_legal
                    && !matches!(
                        &parent.kind,
                        IrNodeKind::Element(p) if p.semantic == Some(SemanticKind::Link)
                    )
            }
        };
        if outside {
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::FenceLinkOutsideRich,
                "<a> is only valid inside a rich-text-block context (a block \
                 container whose direct children are all inline: text/span/img, or a \
                 non-flex <span>). LoomGUI folds the <a> subtree into the parent's \
                 inline flow there; anywhere else it renders as a plain block child \
                 and raises no link semantics. Fix: move the <a> into a block \
                 container holding only inline children"
                    .to_string(),
                line_map.source_location(node.span.start, file.to_string()),
            ));
        }

        // 3. 子内容白名单：直接子元素只许非 flex span；TextNode 天然合法。
        for child in &node.children {
            let IrNodeKind::Element(cel) = &tree.nodes[child.0].kind else {
                continue; // TextNode 合法（链接文字）。
            };
            let legal_span = matches!(cel.semantic, Some(SemanticKind::TextElement))
                && !span_declares_flex(
                    cel,
                    &single_compound_flex_rules,
                    has_multi_compound_flex_rule,
                );
            if !legal_span {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::FenceLinkInvalidChild,
                    format!(
                        "<{}> is not allowed inside <a> — links are text-level: nest only \
                         text and non-flex <span>. Nested links (<a><a>) and image links \
                         (<a><img>) are both rejected; the hit model resolves link runs to \
                         the <a> node and only defines text runs",
                        cel.tag
                    ),
                    line_map.source_location(tree.nodes[child.0].span.start, file.to_string()),
                ));
            }
        }
    }
    diagnostics
}

/// 尺寸声明族（W4 死尺寸判定）。
const SIZING_PROPS: &[&str] = &[
    "width",
    "height",
    "min-width",
    "min-height",
    "max-width",
    "max-height",
];
/// 单 compound class 规则中声明了尺寸的集合（W4 用，模式同 collect_flex_class_rules）。
fn collect_sizing_class_rules(
    dynamic_rules: &[loomgui_core::style::dynamic::DynamicRule],
) -> Vec<&loomgui_core::style::dynamic::Compound> {
    dynamic_rules
        .iter()
        .filter(|r| {
            r.selector.compound.len() == 1
                && r.declarations
                    .iter()
                    .any(|d| SIZING_PROPS.contains(&d.prop.as_str()))
        })
        .map(|r| &r.selector.compound[0])
        .collect()
}

/// 元素是否声明了尺寸（inline style 或被无条件命中的单 compound class 规则）。
fn el_declares_sizing(
    el: &crate::ir::IrElement,
    sizing_rules: &[&loomgui_core::style::dynamic::Compound],
) -> bool {
    if let Some(style) = el.attributes.iter().find(|a| a.name == "style") {
        if style.value.split(';').any(|d| {
            d.split(':')
                .next()
                .is_some_and(|p| SIZING_PROPS.contains(&p.trim()))
        }) {
            return true;
        }
    }
    sizing_rules
        .iter()
        .any(|c| crate::inline_context_check::compound_statically_matches(c, el))
}

/// 节点祖先链上是否有 CustomElement host（投影 light 子标记——组件 `<style>` 的
/// display:flex 在页面宇宙不可见，运行时 rematch 会翻转折叠，静态不可判死活）。
fn under_custom_host(mut idx: usize, tree: &IrTree) -> bool {
    while let Some(p) = tree.nodes[idx].parent {
        if matches!(
            tree.nodes[p.0].kind,
            IrNodeKind::Element(ref el)
                if matches!(el.semantic, Some(SemanticKind::CustomElement))
        ) {
            return true;
        }
        idx = p.0;
    }
    false
}

/// W4 第二遍：rich-text-block 子树内的行内元素声明尺寸 → warning。该子树被折进
/// 父级 inline flow，行内后代无独立盒子——尺寸声明恒无效（与浏览器对 inline 元素
/// 一致，但先验常以为会生效）。rich 根自身的尺寸有效（RichText 叶子 own box），
/// 不警告；投影子树（under_custom_host）跳过——组件规则可能运行时解折叠。
pub(crate) fn warn_inline_sizing(
    tree: &IrTree,
    rich: &[usize],
    dynamic_rules: &[loomgui_core::style::dynamic::DynamicRule],
    file: &str,
    line_map: &LineMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let sizing_rules = collect_sizing_class_rules(dynamic_rules);
    let mut stack: Vec<usize> = Vec::new();
    for &root_idx in rich {
        if under_custom_host(root_idx, tree) {
            continue;
        }
        stack.extend(tree.nodes[root_idx].children.iter().map(|c| c.0));
        while let Some(idx) = stack.pop() {
            if let IrNodeKind::Element(el) = &tree.nodes[idx].kind {
                if matches!(el.semantic, Some(SemanticKind::TextElement))
                    && el_declares_sizing(el, &sizing_rules)
                {
                    diagnostics.push(Diagnostic::warning(
                        DiagnosticCode::FenceInlineSizing,
                        format!(
                            "<span class=\"{}\"> declares width/height inside a rich-text \
                             inline flow — inline elements have no box of their own and the \
                             declaration has no effect (same as browsers). Size it as a flex \
                             item (a div child of a display:flex container) or use <img>.",
                            el.attributes
                                .iter()
                                .find(|a| a.name == "class")
                                .map(|a| a.value.as_str())
                                .unwrap_or("")
                        ),
                        line_map.source_location(tree.nodes[idx].span.start, file.to_string()),
                    ));
                }
            }
            stack.extend(tree.nodes[idx].children.iter().map(|c| c.0));
        }
    }
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

    /// 显式 flex 的 span 在父分类里算 block 子（浏览器 display:flex 外层块级）：
    /// 父容器不再因此被标 rich-text-block——否则 slot 投影进 span 的块级内容会被
    /// 整棵折进父的 inline 流（tip 行"堆一起"/div 行隐身）。
    #[test]
    fn explicit_flex_span_is_block_child() {
        // 纯 flex-span 子（+装饰空白）→ 父不标 rich、不报 mixed：span 是块级 flex 容器。
        let out = parse_template(
            r#"<div>
    <span style="display:flex;flex-direction:column"><span>a</span></span>
</div>"#,
            "t.html",
        );
        let root = out.tree.roots[0].0;
        assert!(
            !out.rich_text_blocks.contains(&root),
            "flex-span 是 block 子，父不得标 rich-text-block"
        );
        assert!(
            out.diagnostics
                .iter()
                .all(|d| d.code != DiagnosticCode::FenceMixedInlineBlock),
            "无 inline 子不应报 mixed: {:?}",
            out.diagnostics
        );

        // text + flex-span 混排 → 与 div 子同款 mixed error（fail-loud，作者选边）。
        let out2 = parse_template(
            r#"<div>名字 <span style="display:flex">a</span></div>"#,
            "t.html",
        );
        let root2 = out2.tree.roots[0].0;
        assert!(
            !out2.rich_text_blocks.contains(&root2),
            "mixed 容器不得标 rich-text-block"
        );
        assert!(
            out2.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::FenceMixedInlineBlock),
            "text + flex-span 须按 mixed 报错（修前 flex-span 被算 inline、静默折一行）: {:?}",
            out2.diagnostics
        );

        // 对照：无 flex 的普通 span 仍是 inline 子（全 inline → rich 标记照旧）。
        let out3 = parse_template(r#"<div>Hello <span>x</span></div>"#, "t.html");
        let root3 = out3.tree.roots[0].0;
        assert!(out3.rich_text_blocks.contains(&root3));
    }

    /// class 规则声明 flex 的 span 同样算 block 子（与 inline style 同口径）。
    #[test]
    fn class_flex_span_is_block_child() {
        let out = parse_template(
            r#"<style>.tip-desc { display: flex; flex-direction: column }</style>
<div>
    <span class="tip-desc"><span>a</span><span>b</span></span>
</div>"#,
            "t.html",
        );
        let root = out.tree.roots[0].0;
        assert!(
            !out.rich_text_blocks.contains(&root),
            "class-flex span 是 block 子，父不得标 rich-text-block"
        );
        assert!(out
            .diagnostics
            .iter()
            .all(|d| d.code != DiagnosticCode::FenceMixedInlineBlock));
    }

    /// slot 在纯 span（无显式 flex）内 → FenceSlotInInlineContext error；
    /// 在显式 flex 的 span 或 div 内 → 静默。
    #[test]
    fn slot_in_plain_span_errors() {
        let bad = parse_template(
            r#"<div><span class="tip-desc"><slot name="desc"></slot></span></div>"#,
            "t.html",
        );
        assert!(
            bad.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::FenceSlotInInlineContext),
            "纯 span 包 slot 须报错（投影块级子无法按自身 display 布局）: {:?}",
            bad.diagnostics
        );

        let flex_ok = parse_template(
            r#"<div><span class="tip-desc" style="display:flex;flex-direction:column"><slot name="desc"></slot></span></div>"#,
            "t.html",
        );
        assert!(
            flex_ok
                .diagnostics
                .iter()
                .all(|d| d.code != DiagnosticCode::FenceSlotInInlineContext),
            "显式 flex 的 span 包 slot 合法: {:?}",
            flex_ok.diagnostics
        );

        let div_ok = parse_template(
            r#"<div><div class="tip-desc"><slot name="desc"></slot></div></div>"#,
            "t.html",
        );
        assert!(
            div_ok
                .diagnostics
                .iter()
                .all(|d| d.code != DiagnosticCode::FenceSlotInInlineContext),
            "div 包 slot 合法: {:?}",
            div_ok.diagnostics
        );
    }

    /// #74 合法路径：`<div>看<a href="open-shop">商店</a></div>` 无 error，
    /// 且父 div 分类进 rich_text_blocks（a 是 inline 子）。
    #[test]
    fn link_in_rich_block_is_legal() {
        let out = parse_template(r#"<div>看<a href="open-shop">商店</a></div>"#, "t.html");
        assert!(
            out.diagnostics.is_empty(),
            "合法链接不应报错: {:?}",
            out.diagnostics
        );
        let root = out.tree.roots[0].0;
        assert!(
            out.rich_text_blocks.contains(&root),
            "含 a 的全 inline 容器应分类进 rich_text_blocks"
        );
        // 链接内嵌 span（非 flex）同样合法。
        let nested = parse_template(
            r#"<div>看<a href="x">商<span>店</span></a></div>"#,
            "t.html",
        );
        assert!(
            nested.diagnostics.is_empty(),
            "a 内嵌非 flex span 合法: {:?}",
            nested.diagnostics
        );
    }

    /// #74：href 缺失 / trim 空 → FenceLinkHrefRequired。
    #[test]
    fn link_missing_or_blank_href_errors() {
        for html in [
            r#"<div>看<a>商店</a></div>"#,
            r#"<div>看<a href="">商店</a></div>"#,
            r#"<div>看<a href="   ">商店</a></div>"#,
        ] {
            let out = parse_template(html, "t.html");
            assert!(
                out.diagnostics
                    .iter()
                    .any(|d| d.code == DiagnosticCode::FenceLinkHrefRequired),
                "缺/空 href 应报 FenceLinkHrefRequired（{html}）: {:?}",
                out.diagnostics
            );
        }
    }

    /// #74：rich 上下文之外（flex 容器 / 裸 block 容器内与 block 子混排）→
    /// FenceLinkOutsideRich。
    #[test]
    fn link_outside_rich_errors() {
        // flex 容器：子是 flex item，不折叠 inline flow。
        let flex = parse_template(
            r#"<div style="display:flex"><a href="x">商店</a></div>"#,
            "t.html",
        );
        assert!(
            flex.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::FenceLinkOutsideRich),
            "flex 容器里的 a 应报 FenceLinkOutsideRich: {:?}",
            flex.diagnostics
        );
        // mixed 容器（a + div 子）：父不进 rich_text_blocks → a 无合法上下文。
        let mixed = parse_template(r#"<div><a href="x">商店</a><div>块</div></div>"#, "t.html");
        assert!(
            mixed
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::FenceLinkOutsideRich),
            "mixed 容器里的 a 应报 FenceLinkOutsideRich: {:?}",
            mixed.diagnostics
        );
        // 文档根级 a（无父）。
        let root = parse_template(r#"<a href="x">孤立链接</a>"#, "t.html");
        assert!(
            root.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::FenceLinkOutsideRich),
            "根级 a 应报 FenceLinkOutsideRich: {:?}",
            root.diagnostics
        );
    }

    /// #74：子内容白名单——a-in-a / img-in-a / 其它元素子 → FenceLinkInvalidChild
    ///（文案点名两种写法）。
    #[test]
    fn link_invalid_children_error() {
        for html in [
            r#"<div>看<a href="a"><a href="b">双层</a></a></div>"#,
            r#"<div>看<a href="x"><img src="i.png"></a></div>"#,
            r#"<div>看<a href="x"><button>按钮</button></a></div>"#,
        ] {
            let out = parse_template(html, "t.html");
            let d = out
                .diagnostics
                .iter()
                .find(|d| d.code == DiagnosticCode::FenceLinkInvalidChild)
                .unwrap_or_else(|| {
                    panic!(
                        "非法子应报 FenceLinkInvalidChild（{html}）: {:?}",
                        out.diagnostics
                    )
                });
            assert!(
                d.message.contains("<a><a>") && d.message.contains("<a><img>"),
                "报错文案须点名 a-in-a 与 img-in-a 写法: {}",
                d.message
            );
        }
    }
}
