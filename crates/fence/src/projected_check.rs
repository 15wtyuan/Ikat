//! 页面侧 CSS 死规则警告（W5）：只可能命中 slot 投射内容的类规则。
//!
//! 样式墙语义：自定义元素（host）子树的 CSS 宇宙 = 组件文件自己的 `<style>`；页面
//! 规则不穿 host 边界。页面侧写给投影内容（host 严格后代里的 light 子）的类规则
//! 运行时恒为死代码——浏览器（无 shadow 语义，规则全命中）预览正常，上线即坏。
//! 此处静态判定「该类只出现在投影内容上」并警告，把发现提前到打包期。
//!
//! 豁免：类同时出现在页面自有元素（含 host 本身——host 归页面作用域）→ 规则有
//! 有效命中，不警告；运行时 `Classes.Add` 动态挂类的场景静态不可见，不在此列
//! （宁可漏报不误报）。

use crate::diagnostic::{Diagnostic, DiagnosticCode, LineMap};
use crate::ir::{IrNodeKind, IrTree};
use crate::schema::tag::SemanticKind;
use std::collections::HashSet;

pub(crate) fn warn_projected_only_rules(
    tree: &IrTree,
    dynamic_rules: &[loomgui_core::style::dynamic::DynamicRule],
    file: &str,
    line_map: &LineMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // 类名按「是否只出现在投影内容（custom host 严格后代）」分桶。host 自身的类
    // 归页面侧（HOST_IN_PARENT_SCOPE：页面规则可样式化 host 本体）。
    let mut projected_classes: HashSet<String> = HashSet::new();
    let mut page_classes: HashSet<String> = HashSet::new();
    let mut stack: Vec<(usize, bool)> = tree.roots.iter().map(|r| (r.0, false)).collect();
    while let Some((idx, under_host)) = stack.pop() {
        let node = &tree.nodes[idx];
        if let IrNodeKind::Element(el) = &node.kind {
            let is_host = matches!(el.semantic, Some(SemanticKind::CustomElement));
            if let Some(class_attr) = el.attributes.iter().find(|a| a.name == "class") {
                let bucket = if under_host {
                    &mut projected_classes
                } else {
                    &mut page_classes
                };
                bucket.extend(class_attr.value.split_whitespace().map(str::to_string));
            }
            for child in &node.children {
                stack.push((child.0, under_host || is_host));
            }
        } else {
            for child in &node.children {
                stack.push((child.0, under_host));
            }
        }
    }
    // 页面侧出现过的类从投影桶剔除（两边都有 = 规则有有效命中）。
    let only_projected: HashSet<&str> = projected_classes
        .iter()
        .map(String::as_str)
        .filter(|c| !page_classes.contains(*c))
        .collect();
    if only_projected.is_empty() {
        return;
    }

    for rule in dynamic_rules {
        // 只判纯类选择器（.foo / .foo.bar，无 tag/id/属性/伪类）——其它形态命中
        // 条件复杂，静态断死容易误报。
        let comp = &rule.selector.compound;
        if comp.len() != 1
            || comp[0].tag.is_some()
            || comp[0].id.is_some()
            || !comp[0].attrs.is_empty()
            || comp[0].pseudo_hover
            || comp[0].pseudo_active
            || comp[0].pseudo_disabled
            || comp[0].pseudo_focus
            || comp[0].pseudo_nth_child.is_some()
            || comp[0].classes.is_empty()
        {
            continue;
        }
        if comp[0]
            .classes
            .iter()
            .any(|c| only_projected.contains(c.as_str()))
        {
            diagnostics.push(Diagnostic::warning(
                DiagnosticCode::FencePageRuleProjectedOnly,
                format!(
                    "rule \"{}\" only matches content projected into component slots — \
                     page CSS does not cross the component style wall, so it never applies \
                     at runtime (browser previews show it working). Style projected content \
                     in the component's own <style>",
                    rule.selector.raw
                ),
                line_map.source_location(0, file.to_string()),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::parse_template;

    /// W5：页面侧纯类规则只命中投影内容 → FencePageRuleProjectedOnly 警告。
    #[test]
    fn page_rule_matching_only_projected_content_warns() {
        let out = parse_template(
            "<style>.qis { width: 10px }</style>\
             <my-widget><span slot=\"cost\" class=\"qis\"></span></my-widget>",
            "page.html",
        );
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::FencePageRuleProjectedOnly),
            "应出 W5 警告: {:?}",
            out.diagnostics
        );
    }

    /// 类同时出现在页面自有元素 → 规则有有效命中，不警告。
    #[test]
    fn shared_class_rule_does_not_warn() {
        let out = parse_template(
            "<style>.qis { width: 10px }</style>\
             <div class=\"qis\"></div>\
             <my-widget><span slot=\"cost\" class=\"qis\"></span></my-widget>",
            "page.html",
        );
        assert!(
            out.diagnostics
                .iter()
                .all(|d| d.code != DiagnosticCode::FencePageRuleProjectedOnly),
            "共享类规则不应警告: {:?}",
            out.diagnostics
        );
    }

    /// W3：transition 声明属性域外 → FenceTransitionUnsupportedProp 警告。
    /// （width 自 #10 起是支持通道——本测试换 margin 代表域外属性。）
    #[test]
    fn transition_width_warns() {
        let out = parse_template(
            "<style>.a { transition: margin 0.3s }</style><div class=\"a\">x</div>",
            "page.html",
        );
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::FenceTransitionUnsupportedProp),
            "应出 transition 警告: {:?}",
            out.diagnostics
        );
    }

    /// W3b：transform 在 transition 白名单内（TRS 分解通道）→ 不警告。
    #[test]
    fn transition_transform_is_supported() {
        let out = parse_template(
            "<style>.a { transition: transform 0.15s ease-out } .a:hover { transform: translateY(-2px) }</style><div class=\"a\">x</div>",
            "page.html",
        );
        assert!(
            out.diagnostics
                .iter()
                .all(|d| d.code != DiagnosticCode::FenceTransitionUnsupportedProp),
            "transform transition 不应警告: {:?}",
            out.diagnostics
        );
    }

    /// W4：rich 子树内 span 声明尺寸 → FenceInlineSizing 警告。
    #[test]
    fn inline_sizing_in_rich_subtree_warns() {
        let out = parse_template(
            "<style>.dot { width: 10px; height: 10px }</style>\
             <div>hi <span class=\"dot\"></span></div>",
            "page.html",
        );
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::FenceInlineSizing),
            "应出 inline sizing 警告: {:?}",
            out.diagnostics
        );
    }

    /// W4 负例：flex 容器里的 span 是 flex item（可定尺寸），不警告。
    #[test]
    fn flex_item_span_does_not_warn() {
        let out = parse_template(
            "<style>.row { display: flex } .dot { width: 10px }</style>\
             <div class=\"row\"><span class=\"dot\"></span></div>",
            "page.html",
        );
        assert!(
            out.diagnostics
                .iter()
                .all(|d| d.code != DiagnosticCode::FenceInlineSizing),
            "flex item 不应警告: {:?}",
            out.diagnostics
        );
    }
}
