//! Stage 6.5：inline 元素布局上下文检查。
//!
//! taffy 0.12 不支持 CSS inline flow（inline 元素自动横排换行）。LoomGUI 只在一种上下文里
//! 让 inline 元素和浏览器一致：**flex 容器内**——inline 元素是 flex item，按 flex 规则排
//! （两边行为相同）。
//!
//! 在 **block 容器**里（裸 `<div>` 等），LoomGUI 把 inline 标签当 block-level（撑满父宽 + 竖排），
//! 和浏览器的 inline 行为（按内容收缩 + 横排流）**必然不一致**。本检查在打包期拦下这种写法，
//! 并教学改法（父容器加 flex / 元素显式 display:block），让 AI 一次就写对。
//!
//! 判定依据：读 stage 4 css_resolve 产出的解析后 display（tag 默认 + inline style + class 规则
//! 都已 cascade），parent 是 block 还是 flex 是确定的，不靠猜。

use crate::diagnostic::{Diagnostic, DiagnosticCode, LineMap};
use crate::ir::{IrElement, IrNodeKind, IrTree};
use crate::schema::tag::{find_tag, DisplayDefault, SemanticKind};
use loomgui_core::style::dynamic::{AttrOp, Compound, DynamicRule};
use loomgui_core::style::resolved::ResolvedStyle;

/// 文本级 inline 语义豁免。这些标签是“文本片段”（span 终态是 TextRun，main-design §10）
/// 或结构占位（slot）——它们在 block 容器里的“不一致”要等文本模型（roadmap §4 复合束）
/// 解决，不是靠强制作者声明 flex 能修的。报错只会逼作者把 `<div role="listitem"><span>x</span></div>` 改成怪结构，
/// 无益。只拦布局 box（button/img/...）。
/// （strong/em/br 已从围栏移除——它们就是 span/\n 的语义糖，不再单独存在。）
const TEXT_LEVEL_SEMANTICS: &[SemanticKind] = &[SemanticKind::TextElement, SemanticKind::Slot];

/// 从 dynamic_rules 预提取声明 `display:<value>` 的规则，按 selector compound 数分组。
///
/// 单 compound 规则可廉价精确匹配元素的 class/tag；多 compound（后代/子代组合）
/// 命中判定贵且需完整 cascade，无法在打包期静态断言 → 返回 `has_multi_compound_rule`
/// 保守标志，调用方据此「视为命中」放行（避免假阳性）。Stage 6.4（rich-text-block
/// 分类）与 6.5（inline 上下文检查）共享此提取，保证两阶段判定一致。
pub(crate) fn collect_display_class_rules<'a>(
    dynamic_rules: &'a [DynamicRule],
    value: &str,
) -> (Vec<&'a Compound>, bool) {
    let mut single_compound_rules: Vec<&Compound> = Vec::new();
    let mut has_multi_compound_rule = false;
    for rule in dynamic_rules {
        let declares = rule
            .declarations
            .iter()
            .any(|d| d.prop == "display" && d.value.trim() == value);
        if !declares {
            continue;
        }
        if rule.selector.compound.len() == 1 {
            single_compound_rules.push(&rule.selector.compound[0]);
        } else {
            has_multi_compound_rule = true;
        }
    }
    (single_compound_rules, has_multi_compound_rule)
}

/// [`collect_display_class_rules`] 的 `display:flex` 特化（Stage 6.4/6.5 共用）。
pub(crate) fn collect_flex_class_rules(dynamic_rules: &[DynamicRule]) -> (Vec<&Compound>, bool) {
    collect_display_class_rules(dynamic_rules, "flex")
}

/// 值随运行时状态变化的属性：写这些的选择器（如 `[aria-checked="true"]`）是
/// 条件化命中——匹配与否取决于运行时状态，不能当无条件的静态 display 来源。
/// 未列出的属性（role、data-*、type、id ...）字面静态，可安全判定。
const RUNTIME_MUTABLE_ATTRS: &[&str] = &[
    "aria-checked",
    "aria-expanded",
    "aria-selected",
    "aria-disabled",
    "aria-pressed",
    "aria-valuenow",
    "aria-activedescendant",
    "hidden",
    "tabindex",
];

/// 单 compound 选择器是否**无条件**命中该元素：tag / id / class 字面对照 +
/// 静态属性选择器（`[role="tablist"]`、`[data-slot="fill"]` 等）。伪类、结构
/// 伪类、运行时可变状态属性是条件化命中——含这些的 compound 不做静态判定。
pub(crate) fn compound_statically_matches(comp: &Compound, el: &IrElement) -> bool {
    let tag_ok = comp
        .tag
        .as_ref()
        .is_none_or(|t| t.eq_ignore_ascii_case(&el.tag));
    let no_pseudo = !comp.pseudo_hover
        && !comp.pseudo_active
        && !comp.pseudo_disabled
        && !comp.pseudo_focus
        && comp.pseudo_nth_child.is_none(); // 结构伪类条件化命中，不能当无条件静态匹配
    if !tag_ok || !no_pseudo {
        return false;
    }
    if let Some(id) = &comp.id {
        let el_id = el
            .attributes
            .iter()
            .find(|a| a.name == "id")
            .map(|a| a.value.as_str());
        if el_id != Some(id.as_str()) {
            return false;
        }
    }
    let classes: Vec<&str> = el
        .attributes
        .iter()
        .find(|a| a.name == "class")
        .map(|a| a.value.split_whitespace().collect())
        .unwrap_or_default();
    if !comp.classes.iter().all(|c| classes.contains(&c.as_str())) {
        return false;
    }
    comp.attrs.iter().all(|a| {
        if RUNTIME_MUTABLE_ATTRS.contains(&a.name.as_str()) {
            return false; // 条件化 → 本 compound 不做静态判定
        }
        let node_attr = el
            .attributes
            .iter()
            .find(|na| na.name.eq_ignore_ascii_case(&a.name));
        match a.op {
            AttrOp::Exists => node_attr.is_some(),
            AttrOp::Eq => {
                matches!(node_attr, Some(na) if na.value == a.value.as_deref().unwrap_or(""))
            }
        }
    })
}

/// 元素的 class 规则是否声明了指定 display 值。单 compound 静态匹配；存在多
/// compound 的此类规则时保守视为命中（无法廉价判定，避免假阳性误报）。
pub(crate) fn statically_declares_display(
    el: &IrElement,
    rules: &[&Compound],
    has_multi_compound_rule: bool,
) -> bool {
    if rules
        .iter()
        .any(|comp| compound_statically_matches(comp, el))
    {
        return true;
    }
    has_multi_compound_rule
}

/// 判定 parent 是否为 flex 上下文。
///
/// stage 4 css_resolve 只烘 inline `style=""` + tag 默认 display 进 styles——`<style>` class 规则
/// 的 display 走 dynamic_rules（运行时 rematch 应用），不在 styles 里。所以要合并两个来源：
/// (1) parent 的解析后 display（inline style 或 tag 默认）是 Flex，或
/// (2) 匹配 parent class 的单 compound 规则声明了 display:flex。
/// 多 compound（后代/子代）规则无法廉价判定命中 → 保守视为 flex（不报，避免假阳性）。
pub(crate) fn is_flex_context(
    parent_el: &IrElement,
    parent_style: &ResolvedStyle,
    single_compound_flex_rules: &[&Compound],
    has_multi_compound_flex_rule: bool,
) -> bool {
    // (1) inline style / tag 默认已是 Flex。
    if parent_style.taffy_style.display == taffy::Display::Flex {
        return true;
    }
    // (2) class 规则声明 display:flex。
    statically_declares_display(
        parent_el,
        single_compound_flex_rules,
        has_multi_compound_flex_rule,
    )
}

/// 检查所有 inline 元素是否处于合法上下文（flex 容器）。返回诊断（error 列表）。
///
/// 入参：
/// - `tree`：IrTree（取元素 tag + 父子链 + semantic）
/// - `styles`：Stage 4 css_resolve 产物（按 node index 索引，含 inline style + tag 默认 display）
/// - `dynamic_rules`：Stage 4.5 的 `<style>` 规则（补 class 规则 display 判定）
/// - `file` / `line_map`：定位诊断
pub fn check_inline_context(
    tree: &IrTree,
    styles: &[ResolvedStyle],
    dynamic_rules: &[DynamicRule],
    // Stage 6.4 产出的 rich-text-block ir_idx 集合。img 的 parent 在此集合里 → 豁免
    // （img 作为 inline run 走 rich-text inline flow）；button 不豁免（非 inline 级）。
    rich_text_blocks: &[usize],
    file: &str,
    line_map: &LineMap,
) -> Vec<Diagnostic> {
    // 预提取“声明了 display:flex / display:block 的单 compound 规则”，避免每个
    // 元素全量扫描。flex 判定父上下文；block 判定子元素自身（显式块级）。
    // 多 compound 规则 → 保守放行。
    let (single_compound_flex_rules, has_multi_compound_flex_rule) =
        collect_flex_class_rules(dynamic_rules);
    let (single_compound_block_rules, has_multi_compound_block_rule) =
        collect_display_class_rules(dynamic_rules, "block");

    let mut diagnostics = Vec::new();
    for (idx, node) in tree.nodes.iter().enumerate() {
        let IrNodeKind::Element(el) = &node.kind else {
            continue;
        };

        // 只查 inline-origin 标签（find_tag() 判定 display=Inline，动态读注册表）。
        // block-origin（div）在 block 流里本就撑满竖排，和浏览器一致。
        let Some(spec) = find_tag(&el.tag) else {
            continue;
        };
        if spec.display != DisplayDefault::Inline {
            continue;
        }

        // 文本级语义豁免（span/slot）：它们的行内混排要等文本模型（roadmap §4），
        // 不是 flex 能修的；报错只会逼怪结构。只拦布局 box。
        if el
            .semantic
            .is_some_and(|s| TEXT_LEVEL_SEMANTICS.contains(&s))
        {
            continue;
        }

        // 元素自己显式 display:block（inline style 或 class 规则）→ 作者有意当块级
        // （撑满），浏览器也撑满，两边一致 → 放行。class 规则路径与 flex 判定同源
        // （css_resolve 只烘 inline style，class 规则的 display 要查 dynamic_rules）。
        // （display:flex 的 inline 元素在浏览器仍 shrink-to-fit，和 LoomGUI 撑满不一致 → 不放行。）
        if styles[idx].taffy_style.display == taffy::Display::Block
            || statically_declares_display(
                el,
                &single_compound_block_rules,
                has_multi_compound_block_rule,
            )
        {
            continue;
        }

        // 无父（文档根级 inline 元素）——无上下文可判，跳过（极少见）。
        let Some(parent_id) = node.parent else {
            continue;
        };
        let IrNodeKind::Element(parent_el) = &tree.nodes[parent_id.0].kind else {
            continue;
        };

        // parent 是 flex（inline style / tag 默认 / class 规则）→ flex item，放行。
        // （CustomElement host 的判定走下方豁免——light 子打包期投影，不构成布局上下文。）
        if is_flex_context(
            parent_el,
            &styles[parent_id.0],
            &single_compound_flex_rules,
            has_multi_compound_flex_rule,
        ) {
            continue;
        }

        // parent 是 CustomElement host：light 子在打包期被投影进组件 slot
        // （component-system spec），host 最终子树来自组件模板——页面文件里的
        // light 子不构成 host 的布局上下文，混排检查对它是误报。
        if parent_el.semantic == Some(SemanticKind::CustomElement) {
            continue;
        }

        // parent 是 rich-text-block（Stage 6.4 判定：block 容器 + 直接子全 inline 级）
        // → img 作为 inline run 走 rich-text inline flow，与浏览器一致 → 豁免。
        // button 不豁免：button 是控件非 phrasing，不进 inline 级集合，仍报。
        if el.semantic == Some(SemanticKind::Image) && rich_text_blocks.contains(&parent_id.0) {
            continue;
        }

        diagnostics.push(Diagnostic::error(
            DiagnosticCode::FenceInlineElementInBlockContext,
            format!(
                "inline element <{tag}> is directly inside a block container — \
                 LoomGUI renders it block-level (fills width + stacks vertically), \
                 which differs from the browser's inline behavior (shrink-to-fit + horizontal flow). \
                 LoomGUI has no inline flow outside flex. \
                 Fix (pick one): \
                 (1) make the parent a flex container (add display:flex; add flex-wrap:wrap for multi-element rows); \
                 (2) set display:block on the element if you intentionally want it to fill width / behave as a block.",
                tag = el.tag
            ),
            line_map.source_location(node.span.start, file.to_string()),
        ));
    }
    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::parse_template;

    fn has_block_context_diag(result: &crate::pipeline::ParsedTemplate, tag: &str) -> bool {
        result.diagnostics.iter().any(|d| {
            d.code == DiagnosticCode::FenceInlineElementInBlockContext
                && d.message.contains(&format!("<{tag}>"))
        })
    }

    /// 裸 button 在 block div 里 → error。
    #[test]
    fn bare_inline_in_block_errors() {
        let result = parse_template(r#"<div><button>Buy</button></div>"#, "t.html");
        assert!(
            has_block_context_diag(&result, "button"),
            "block div 里的裸 button 应报错: {:?}",
            result.diagnostics
        );
    }

    /// button 在 flex 容器里 → 放行（flex item，两边一致）。
    #[test]
    fn inline_in_flex_ok() {
        let result = parse_template(
            r#"<div style="display:flex"><button>Buy</button></div>"#,
            "t.html",
        );
        assert!(
            !has_block_context_diag(&result, "button"),
            "flex 容器里的 button 不应报错: {:?}",
            result.diagnostics
        );
    }

    /// 父容器由 class 规则设 display:flex → 放行（css_resolve 已 cascade 进 styles）。
    #[test]
    fn inline_in_class_flex_ok() {
        let result = parse_template(
            r#"<style>.row { display:flex; flex-wrap:wrap }</style><div class="row"><button>x</button></div>"#,
            "t.html",
        );
        assert!(
            !has_block_context_diag(&result, "button"),
            "class 规则 flex 容器里的 button 不应报错: {:?}",
            result.diagnostics
        );
    }

    /// 属性选择器声明的 display:flex（`[role="tablist"]` 静态可判定）→ 放行——
    /// 结构检查与控件检查的选择器覆盖须一致。
    #[test]
    fn inline_in_attr_selector_flex_ok() {
        let result = parse_template(
            r#"<style>[role="tablist"] { display:flex }</style><div role="tablist"><button role="tab">a</button><button role="tab">b</button></div>"#,
            "t.html",
        );
        assert!(
            !has_block_context_diag(&result, "button"),
            "属性选择器 flex 容器里的 button 不应报错: {:?}",
            result.diagnostics
        );
    }

    /// id 选择器声明的 display:flex（静态字面匹配）→ 放行。
    #[test]
    fn inline_in_id_selector_flex_ok() {
        let result = parse_template(
            r#"<style>#tabs { display:flex }</style><div id="tabs"><button>x</button></div>"#,
            "t.html",
        );
        assert!(
            !has_block_context_diag(&result, "button"),
            "id 选择器 flex 容器里的 button 不应报错: {:?}",
            result.diagnostics
        );
    }

    /// 运行时可变状态属性（aria-checked）声明的 display:flex 是条件化命中——
    /// 初始态未必 flex，不做静态判定（保守继续报错）。
    #[test]
    fn conditional_state_selector_flex_still_errors() {
        let result = parse_template(
            r#"<style>[aria-checked="true"] { display:flex }</style><div role="switch" aria-checked="true"><button>x</button></div>"#,
            "t.html",
        );
        assert!(
            has_block_context_diag(&result, "button"),
            "条件化 flex（状态属性选择器）不应静态放行: {:?}",
            result.diagnostics
        );
    }

    /// 元素显式 display:block（有意当块级撑满）→ 放行。
    #[test]
    fn explicit_display_block_ok() {
        let result = parse_template(
            r#"<div><button style="display:block">Full-width</button></div>"#,
            "t.html",
        );
        assert!(
            !has_block_context_diag(&result, "button"),
            "显式 display:block 的 button 不应报错（有意块级）: {:?}",
            result.diagnostics
        );
    }

    /// class 规则声明 display:block（对面反馈：修复建议 (2) 的 class CSS 路径须生效，
    /// 与 inline style 同待遇）→ 放行。
    #[test]
    fn class_display_block_ok() {
        let result = parse_template(
            r#"<style>.blk { display: block }</style><div><button class="blk">Full-width</button></div>"#,
            "t.html",
        );
        assert!(
            !has_block_context_diag(&result, "button"),
            "class 规则 display:block 的 button 不应报错: {:?}",
            result.diagnostics
        );
    }

    /// 文本级语义（span/slot）在 block 容器里 → 放行（行内混排要等文本模型，不是 flex 能修的）。
    #[test]
    fn text_level_semantic_in_block_ok() {
        let result = parse_template(r#"<div><span>x</span></div>"#, "t.html");
        assert!(
            !has_block_context_diag(&result, "span"),
            "block div 里的 span 不应报错（文本级语义豁免）: {:?}",
            result.diagnostics
        );
    }

    /// 多个裸 button 在 block div 里 → 每个都报（横排预期会被竖排）。
    #[test]
    fn multiple_inlines_each_error() {
        let result = parse_template(
            r#"<div><button>A</button><button>B</button></div>"#,
            "t.html",
        );
        let count = result
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FenceInlineElementInBlockContext)
            .count();
        assert_eq!(count, 2, "两个裸 button 各报一次，got {count}");
    }

    /// block-origin 元素（div）在 block 容器里 → 不报（block 流本就一致）。
    #[test]
    fn block_element_not_flagged() {
        let result = parse_template(r#"<div><div>inner</div></div>"#, "t.html");
        assert!(
            !has_block_context_diag(&result, "div"),
            "block div 不应报 inline 上下文错: {:?}",
            result.diagnostics
        );
    }

    /// 嵌套 block + flex：button 的直接父是 block（即使祖先有 flex）→ 仍报错。
    /// （直接父决定 formatting context；按钮在 block 里就按 block 走。）
    #[test]
    fn inline_in_block_inside_flex_still_errors() {
        let result = parse_template(
            r#"<div style="display:flex"><div><button>x</button></div></div>"#,
            "t.html",
        );
        assert!(
            has_block_context_diag(&result, "button"),
            "button 的直接父是 block div（即使外层 flex）→ 仍报错: {:?}",
            result.diagnostics
        );
    }
}
