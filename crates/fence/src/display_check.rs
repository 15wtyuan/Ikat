//! Stage 4.6：inline 元素 display 声明检查。
//!
//! taffy 0.12 不支持 CSS inline flow（inline 元素自动横排换行）。LoomGUI 把 inline 标签
//! 在布局流里当 block-level（撑满父宽、竖排）——与 AI 的浏览器先验（inline 横排）冲突。
//! 若放任"裸 inline 元素"（既没 inline style 声明 display，也没 class 规则声明），AI 会
//! 按浏览器先验预期横排，运行时却是竖排 → 渲染不可预测 → 返工。
//!
//! 本检查在打包期拦截：inline 元素必须**显式声明 display**（来源不限——inline `style=""`
//! 或匹配的 `<style>` class 规则均可）。声明了就说明作者有意确定布局策略，AI 读代码能预测。
//!
//! 判定范围（设计 B）：认 inline style + class 规则的 display。class 匹配用简化逻辑
//! （单 compound class/tag 选择器直接命中元素的 class 列表）；多 compound 选择器（后代/子代）
//! 保守放行（无法廉价确定是否命中 → 不报 error，避免假阳性）。

use crate::diagnostic::{Diagnostic, DiagnosticCode, LineMap};
use crate::ir::{IrNodeId, IrNodeKind, IrTree};
use crate::schema::tag::{find_tag, DisplayDefault, SemanticKind};
use loomgui_core::style::dynamic::{Compound, DynamicRule};

/// 需要 display 的 inline 语义（布局 box）：这些在浏览器是 inline/inline-block，AI 强烈预期横排，
/// 但 LoomGUI 运行时是 block-level → 裸的必报错强制声明。
///
/// 豁免的 inline 标签：
/// - `span/strong/em`（TextElement）：终态是文本 block 内的 TextRun（main-design §10），
///   display 约束它们是错的——它们的行内混排要等复合束文本模型，非 display 能解。
/// - `input/select/textarea/img/canvas/progress`（控件/叶子媒体）：自绘叶子，display 对布局流无意义。
/// - `br/slot`：无 box 概念。
const DISPLAY_REQUIRED_INLINE: &[SemanticKind] = &[
    SemanticKind::Button,
    SemanticKind::Link,
    SemanticKind::Label,
];

/// 文本上下文判定：沿祖先链找 <p>（TextBlock）。在 <p> 内的 inline 元素是文本行内混排的一员
/// （终态 LinkRun/TextRun，main-design §10），display 不适用。
fn in_text_context(tree: &IrTree, mut id: IrNodeId) -> bool {
    while let Some(parent_id) = tree.nodes[id.0].parent {
        if let IrNodeKind::Element(pel) = &tree.nodes[parent_id.0].kind {
            if pel.semantic == Some(SemanticKind::TextBlock) {
                return true;
            }
        }
        id = parent_id;
    }
    false
}

/// 检查所有 inline 元素是否显式声明了 display。返回诊断（error 列表）。
///
/// 入参：
/// - `tree`：IrTree（取元素 tag/class/inline style）
/// - `dynamic_rules`：Stage 4.5 解析的 `<style>` 规则（含 declarations）
/// - `file` / `line_map`：定位诊断
pub fn check_inline_display(
    tree: &IrTree,
    dynamic_rules: &[DynamicRule],
    file: &str,
    line_map: &LineMap,
) -> Vec<Diagnostic> {
    // 预提取"声明了 display 的规则"及其 selector 的单 compound 形态，避免对每个元素全量扫描。
    // 多 compound（后代/子代）的 display 规则：保守放行所有元素（无法廉价判定命中）。
    let mut single_compound_display_rules: Vec<&Compound> = Vec::new();
    let mut has_multi_compound_display_rule = false;
    for rule in dynamic_rules {
        let declares_display = rule.declarations.iter().any(|d| d.prop == "display");
        if !declares_display {
            continue;
        }
        if rule.selector.compound.len() == 1 {
            single_compound_display_rules.push(&rule.selector.compound[0]);
        } else {
            has_multi_compound_display_rule = true;
        }
    }

    let mut diagnostics = Vec::new();
    for (idx, node) in tree.nodes.iter().enumerate() {
        let IrNodeKind::Element(el) = &node.kind else {
            continue;
        };

        // 只检查需要 display 的 inline 语义（Button/Link/Label）。
        // span/strong/em（TextElement，终态 TextRun）、input/select/textarea/img/canvas/progress
        // （叶子控件/媒体）、br/slot 豁免——display 对它们无意义或另有终态处理。
        let Some(spec) = find_tag(&el.tag) else {
            continue;
        };
        if spec.display != DisplayDefault::Inline {
            continue;
        }
        let Some(semantic) = el.semantic else {
            continue;
        };
        if !DISPLAY_REQUIRED_INLINE.contains(&semantic) {
            continue;
        }

        // 文本上下文豁免：若祖先链含 <p>（TextBlock），该 inline 元素是文本行内混排的一员
        // （终态走 LinkRun/TextRun，main-design §10），display 不适用。如 <p>...<a>点此</a>...</p>。
        if in_text_context(tree, IrNodeId(idx)) {
            continue;
        }

        if element_declares_display(el, &single_compound_display_rules) {
            continue;
        }

        // 多 compound display 规则存在 → 保守放行（无法廉价判定命中，避免假阳性）。
        if has_multi_compound_display_rule {
            continue;
        }

        diagnostics.push(Diagnostic::error(
            DiagnosticCode::FenceInlineElementMissingDisplay,
            format!(
                "inline element <{}> must declare `display` explicitly (inline style or matching class rule) — \
                 LoomGUI has no CSS inline-flow, a bare inline element stacks vertically and is unpredictable",
                el.tag
            ),
            line_map.source_location(node.span.start, file.to_string()),
        ));
    }
    diagnostics
}

/// 判定单个元素是否"显式声明了 display"：
/// (1) inline `style=""` 含 display 声明，或
/// (2) 元素 class 命中某条"声明了 display 的单 compound 规则"。
fn element_declares_display(
    el: &crate::ir::IrElement,
    single_compound_display_rules: &[&Compound],
) -> bool {
    // (1) inline style 有 display?
    if let Some(style_attr) = el.attributes.iter().find(|a| a.name == "style") {
        for decl in style_attr.value.split(';') {
            let decl = decl.trim();
            if let Some((prop, _)) = decl.split_once(':') {
                if prop.trim() == "display" {
                    return true;
                }
            }
        }
    }

    // (2) 元素 class 命中"声明了 display 的单 compound 规则"?
    let el_classes: Vec<&str> = el
        .attributes
        .iter()
        .find(|a| a.name == "class")
        .map(|a| a.value.split_whitespace().collect())
        .unwrap_or_default();

    for comp in single_compound_display_rules {
        // tag 限定：规则的 tag 要么没指定，要么匹配元素的 tag。
        let tag_ok = comp
            .tag
            .as_ref()
            .is_none_or(|t| t.eq_ignore_ascii_case(&el.tag));
        // 伪类/属性限定的规则无法廉价判定命中 → 视为不命中（保守，但 display 规则极少带伪类）。
        let no_pseudo = !comp.pseudo_hover
            && !comp.pseudo_active
            && !comp.pseudo_disabled
            && !comp.pseudo_focus
            && comp.attrs.is_empty();
        if !tag_ok || !no_pseudo {
            continue;
        }
        // class 子集匹配：规则的所有 class 都在元素的 class 列表里。
        // id 限定：若规则指定 id，元素的 id 必须匹配（本检查不查 id 属性来源，从严视为不命中）。
        let id_ok = comp.id.is_none();
        let classes_ok = comp
            .classes
            .iter()
            .all(|c| el_classes.contains(&c.as_str()));
        if id_ok && classes_ok {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::parse_template;

    fn has_missing_display_diag(result: &crate::pipeline::ParsedTemplate, tag: &str) -> bool {
        result.diagnostics.iter().any(|d| {
            d.code == DiagnosticCode::FenceInlineElementMissingDisplay
                && d.message.contains(&format!("<{tag}>"))
        })
    }

    /// 裸 inline 元素（无 inline style、无 class）→ error。
    #[test]
    fn bare_inline_element_errors() {
        let result = parse_template(r#"<button>Buy</button>"#, "t.html");
        assert!(
            has_missing_display_diag(&result, "button"),
            "裸 button 应报 FenceInlineElementMissingDisplay: {:?}",
            result.diagnostics
        );
    }

    /// inline style 声明 display → 不报错。
    #[test]
    fn inline_style_display_ok() {
        let result = parse_template(r#"<button style="display:flex">Buy</button>"#, "t.html");
        assert!(
            !has_missing_display_diag(&result, "button"),
            "inline style display:flex 不应报错: {:?}",
            result.diagnostics
        );
    }

    /// class 规则声明 display + 元素带该 class → 不报错。
    #[test]
    fn class_rule_display_ok() {
        let result = parse_template(
            r#"<style>.tab { display:flex }</style><button class="tab">音效</button>"#,
            "t.html",
        );
        assert!(
            !has_missing_display_diag(&result, "button"),
            "class 规则声明 display + 元素带该 class 不应报错: {:?}",
            result.diagnostics
        );
    }

    /// class 规则声明 display 但元素不带该 class → 该元素仍 error（裸的）。
    #[test]
    fn class_rule_display_unmatched_element_errors() {
        let result = parse_template(
            r#"<style>.tab { display:flex }</style><button class="other">音效</button>"#,
            "t.html",
        );
        assert!(
            has_missing_display_diag(&result, "button"),
            "元素 class 不匹配 display 规则 → 仍应报错: {:?}",
            result.diagnostics
        );
    }

    /// block 元素（div）不声明 display → 不报错（block 是 LoomGUI 合理默认）。
    #[test]
    fn block_element_no_display_ok() {
        let result = parse_template(r#"<div></div>"#, "t.html");
        assert!(
            !has_missing_display_diag(&result, "div"),
            "div 不应报 inline display 错: {:?}",
            result.diagnostics
        );
    }

    /// 后代选择器（多 compound）声明 display → 保守放行（不报 error，避免假阳性）。
    #[test]
    fn descendant_selector_display_conservative_pass() {
        let result = parse_template(
            r#"<style>.parent button { display:flex }</style><div class="parent"><button>x</button></div>"#,
            "t.html",
        );
        assert!(
            !has_missing_display_diag(&result, "button"),
            "后代选择器 display 规则应保守放行: {:?}",
            result.diagnostics
        );
    }

    /// 多 class 限定选择器（.a.b）命中 → 不报错。
    #[test]
    fn multi_class_selector_matches() {
        let result = parse_template(
            r#"<style>.btn.primary { display:flex }</style><button class="btn primary">x</button>"#,
            "t.html",
        );
        assert!(
            !has_missing_display_diag(&result, "button"),
            ".btn.primary 命中 class=btn primary 不应报错: {:?}",
            result.diagnostics
        );
    }

    /// tag 限定选择器（button.tab）匹配 → 不报错；不匹配 tag → 视为不命中。
    #[test]
    fn tag_qualified_selector() {
        let result = parse_template(
            r#"<style>button.tab { display:flex }</style><button class="tab">x</button>"#,
            "t.html",
        );
        assert!(
            !has_missing_display_diag(&result, "button"),
            "button.tab 命中 button.tab 不应报错: {:?}",
            result.diagnostics
        );
    }

    /// <p> 内的 <a>（文本行内链接）豁免——终态走 LinkRun，display 不适用。
    #[test]
    fn link_inside_text_block_exempt() {
        let result = parse_template(r##"<p>点此<a href="#">领取</a>奖励</p>"##, "t.html");
        assert!(
            result.diagnostics.is_empty(),
            "<p> 内的 <a> 应豁免 display 检查: {:?}",
            result.diagnostics
        );
    }

    /// <p> 外的 <a>（独立链接 box）仍需 display。
    #[test]
    fn link_outside_text_block_requires_display() {
        let result = parse_template(r##"<div><a href="#">链接</a></div>"##, "t.html");
        assert!(
            has_missing_display_diag(&result, "a"),
            "<p> 外的裸 <a> 应报错: {:?}",
            result.diagnostics
        );
    }
}
