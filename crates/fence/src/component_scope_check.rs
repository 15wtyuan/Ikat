//! 组件侧 CSS 死规则警告：组件 `<style>` 纯类规则在样式墙外恒无命中。
//!
//! 样式墙的反向缺口：组件规则不穿出 host——页面作用域的元素永远够不着组件
//! `<style>`。作者把规则写进组件文件、元素却留在页面作用域时，规则运行时恒死
//! ——浏览器预览（无 shadow 语义，组件 CSS 全局命中）却看着正常，上线即坏。
//!
//! 判定须跨文件证据（单文件 pipeline 拿不到），由 packer 聚合全部页面树后调用：
//! 纯类规则（单 compound、无 tag/id/属性/伪类——与 FencePageRuleProjectedOnly
//! 同一过滤口径）的类名 ∉（本组件模板 ∪ 投影进本组件 host 的内容）且 ∈ 墙外
//! 区域（页面 host 外元素、或投影进其它组件 host 的内容）→ 死规则警告。类名
//! 全库不出现（运行时 `Classes.Add` 动态挂类，静态不可见）→ 静默，宁漏报不误报。

use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceLocation};
use crate::ir::{IrNodeKind, IrTree};
use crate::schema::tag::SemanticKind;
use loomgui_core::style::dynamic::DynamicRule;
use std::collections::{HashMap, HashSet};

/// 页面树类名分桶（跨文件聚合：`add_page_tree` 逐页累加）。
#[derive(Default)]
pub struct ScopeClassBuckets {
    /// host 标签名 → 投影进该 host 的 light 子类名（host 严格后代；嵌套 host
    /// 的后代计入每一层 host——它是各层的投影内容）。
    pub projected_by_host: HashMap<String, HashSet<String>>,
    /// host 外区域的类名（含 host 本身——host 归页面作用域，页面规则可样式化）。
    pub page_scope: HashSet<String>,
}

impl ScopeClassBuckets {
    pub fn new() -> Self {
        Self::default()
    }

    /// 累加一棵页面树：沿树传播「祖先 host 栈」，元素类名按所在区域分桶。
    pub fn add_page_tree(&mut self, tree: &IrTree) {
        let mut stack: Vec<(usize, Vec<String>)> =
            tree.roots.iter().map(|r| (r.0, Vec::new())).collect();
        while let Some((idx, hosts)) = stack.pop() {
            let node = &tree.nodes[idx];
            if let IrNodeKind::Element(el) = &node.kind {
                let is_host = matches!(el.semantic, Some(SemanticKind::CustomElement));
                if let Some(class_attr) = el.attributes.iter().find(|a| a.name == "class") {
                    if hosts.is_empty() {
                        self.page_scope
                            .extend(class_attr.value.split_whitespace().map(str::to_string));
                    } else {
                        for host in &hosts {
                            self.projected_by_host
                                .entry(host.clone())
                                .or_default()
                                .extend(class_attr.value.split_whitespace().map(str::to_string));
                        }
                    }
                }
                let mut child_hosts = hosts.clone();
                if is_host {
                    child_hosts.push(el.tag.clone());
                }
                for child in &node.children {
                    stack.push((child.0, child_hosts.clone()));
                }
            } else {
                for child in &node.children {
                    stack.push((child.0, hosts.clone()));
                }
            }
        }
    }
}

/// 一个待检的组件文件（packer 从注册表取出：模板树 + `<style>` 动态规则）。
pub struct ComponentScopeInput<'a> {
    /// 组件名（= 文件 stem = host 标签名，注册表键）。
    pub name: &'a str,
    pub html_rel: &'a str,
    pub tree: &'a IrTree,
    pub rules: &'a [DynamicRule],
}

/// 死规则判定：组件纯类规则的类名在本组件样式宇宙（模板 + 本组件投影内容）
/// 外、且在墙外区域（页面 host 外元素、或投影进其它组件 host 的内容——本组件
/// 均够不着）有真实元素 → warning。宇宙内命中或全库无元素（动态挂类嫌疑）
/// → 静默。嵌套 host（`<a-comp><b-comp>`）的后代计入每层 host 桶——对祖先
/// 组件是自己的投影内容，不算墙外证据。
pub fn warn_component_rules_out_of_scope(
    components: &[ComponentScopeInput<'_>],
    buckets: &ScopeClassBuckets,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for comp in components {
        // 本组件可达类名 = 模板全部元素（含嵌套 host 的 light 子与 host 本体）
        // ∪ 投影进本组件 host 的页面内容。
        let mut reachable: HashSet<String> = collect_template_classes(comp.tree);
        if let Some(projected) = buckets.projected_by_host.get(comp.name) {
            reachable.extend(projected.iter().cloned());
        }
        for rule in comp.rules {
            // 纯类选择器过滤（与 FencePageRuleProjectedOnly 同口径）——其它形态
            // 命中条件复杂，静态断死容易误报。
            let sel = &rule.selector.compound;
            if sel.len() != 1
                || sel[0].tag.is_some()
                || sel[0].id.is_some()
                || !sel[0].attrs.is_empty()
                || sel[0].pseudo_hover
                || sel[0].pseudo_active
                || sel[0].pseudo_disabled
                || sel[0].pseudo_focus
                || sel[0].pseudo_nth_child.is_some()
                || sel[0].classes.is_empty()
            {
                continue;
            }
            if sel[0].classes.iter().any(|c| reachable.contains(c)) {
                continue; // 宇宙内有命中可能 → 活规则
            }
            // 墙外证据：页面 host 外元素，或投影进其它组件 host 的内容。
            let unreachable_here = |c: &str| {
                buckets.page_scope.contains(c)
                    || buckets
                        .projected_by_host
                        .iter()
                        .any(|(host, classes)| host != comp.name && classes.contains(c))
            };
            let outside: Vec<&str> = sel[0]
                .classes
                .iter()
                .map(String::as_str)
                .filter(|c| unreachable_here(c))
                .collect();
            if outside.is_empty() {
                continue; // 全库无墙外静态元素（运行时挂类）→ 静默
            }
            diagnostics.push(Diagnostic::warning(
                DiagnosticCode::FenceComponentRuleOutOfScope,
                format!(
                    "rule \"{}\" in component \"{}\" can never match at runtime: class(es) \
                     [{}] only appear outside this component's style universe (page-scope \
                     elements or content projected into other components), and component CSS \
                     does not cross the host boundary (browser previews show it working). \
                     Move the rule to the page <style>, or the element into the component \
                     template",
                    rule.selector.raw,
                    comp.name,
                    outside.join(", ")
                ),
                file_level_location(comp.html_rel),
            ));
        }
    }
}

/// 组件模板内全部元素的类名（模板是组件自己的宇宙：host 本体、slot fallback、
/// 嵌套 host 的 light 子都归它样式化）。
fn collect_template_classes(tree: &IrTree) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut stack: Vec<usize> = tree.roots.iter().map(|r| r.0).collect();
    while let Some(idx) = stack.pop() {
        let node = &tree.nodes[idx];
        if let IrNodeKind::Element(el) = &node.kind {
            if let Some(class_attr) = el.attributes.iter().find(|a| a.name == "class") {
                out.extend(class_attr.value.split_whitespace().map(str::to_string));
            }
            for child in &node.children {
                stack.push(child.0);
            }
        } else {
            for child in &node.children {
                stack.push(child.0);
            }
        }
    }
    out
}

/// 文件级定位（规则模型不带字节偏移，指向文件头——与 FencePageRuleProjectedOnly
/// 的 offset-0 近似同口径）。
fn file_level_location(html_rel: &str) -> SourceLocation {
    SourceLocation {
        file: html_rel.to_string(),
        offset: 0,
        line: 1,
        column: 1,
        source_text: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::parse_template;

    /// tip-stem 场景（Tripawd N22 实录）：规则写进 tip-panel 组件、元素在页面
    /// host 外区域 → 死规则警告。
    #[test]
    fn page_scope_rule_in_component_warns() {
        let comp = parse_template(
            "<style>.tip-stem { width: 10px }</style><div class=\"panel\"><slot></slot></div>",
            "ui/game/components/tip-panel.html",
        );
        let page = parse_template("<div class=\"tip-stem\"></div>", "ui/game/battle.html");
        let mut buckets = ScopeClassBuckets::new();
        buckets.add_page_tree(&page.tree);
        let components = [ComponentScopeInput {
            name: "tip-panel",
            html_rel: "ui/game/components/tip-panel.html",
            tree: &comp.tree,
            rules: &comp.dynamic_rules,
        }];
        let mut diags = Vec::new();
        warn_component_rules_out_of_scope(&components, &buckets, &mut diags);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(matches!(
            diags[0].code,
            DiagnosticCode::FenceComponentRuleOutOfScope
        ));
        assert!(diags[0].message.contains("tip-stem"));
    }

    /// 运行时挂类嫌疑（is-hover 场景）：类名全库不出现 → 静默。
    #[test]
    fn runtime_only_class_silent() {
        let comp = parse_template(
            "<style>.is-hover { opacity: 0.5 }</style><div class=\"slot\"></div>",
            "c/tip-panel.html",
        );
        let page = parse_template("<div class=\"x\"></div>", "p.html");
        let mut buckets = ScopeClassBuckets::new();
        buckets.add_page_tree(&page.tree);
        let components = [ComponentScopeInput {
            name: "tip-panel",
            html_rel: "c/tip-panel.html",
            tree: &comp.tree,
            rules: &comp.dynamic_rules,
        }];
        let mut diags = Vec::new();
        warn_component_rules_out_of_scope(&components, &buckets, &mut diags);
        assert!(diags.is_empty(), "无静态证据不断死: {diags:?}");
    }

    /// 类名在本组件模板出现 → 活规则静默（宁漏报）。
    #[test]
    fn template_hit_silent() {
        let comp = parse_template(
            "<style>.stem { width: 10px }</style><div class=\"stem\"></div>",
            "c/tip-panel.html",
        );
        let page = parse_template("<div class=\"stem\"></div>", "p.html");
        let mut buckets = ScopeClassBuckets::new();
        buckets.add_page_tree(&page.tree);
        let components = [ComponentScopeInput {
            name: "tip-panel",
            html_rel: "c/tip-panel.html",
            tree: &comp.tree,
            rules: &comp.dynamic_rules,
        }];
        let mut diags = Vec::new();
        warn_component_rules_out_of_scope(&components, &buckets, &mut diags);
        assert!(diags.is_empty(), "模板有命中: {diags:?}");
    }

    /// 类名只在投影进本组件 host 的内容上 → 活规则静默（组件样式宇宙含投影）。
    /// 对照：只投影进「别的」组件 → 对本组件是死规则（页面证据须按 host 分桶）。
    #[test]
    fn projected_into_own_host_live_but_other_host_dead() {
        let comp = parse_template(
            "<style>.own-proj { width: 1px } .other-proj { width: 2px }</style>\
             <div class=\"panel\"><slot></slot></div>",
            "c/tip-panel.html",
        );
        let page = parse_template(
            "<tip-panel><span class=\"own-proj\"></span></tip-panel>\
             <skill-slot><span class=\"other-proj\"></span></skill-slot>",
            "p.html",
        );
        let mut buckets = ScopeClassBuckets::new();
        buckets.add_page_tree(&page.tree);
        let components = [ComponentScopeInput {
            name: "tip-panel",
            html_rel: "c/tip-panel.html",
            tree: &comp.tree,
            rules: &comp.dynamic_rules,
        }];
        let mut diags = Vec::new();
        warn_component_rules_out_of_scope(&components, &buckets, &mut diags);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            diags[0].message.contains("other-proj"),
            "投进别家 host 的规则对本组件是死规则: {:?}",
            diags[0].message
        );
    }

    /// 带后代组合（两 compound）的规则不过纯类过滤 → 静默（skill-slot.is-hover .slot
    /// 这类带 tag 形态不在此检）。
    #[test]
    fn descendant_rule_skipped() {
        let comp = parse_template(
            "<style>.act.is-hover .slot { opacity: 1 } .page-only { width: 3px }</style>\
             <div class=\"slot\"></div>",
            "c/tip-panel.html",
        );
        let page = parse_template("<div class=\"act page-only\"></div>", "p.html");
        let mut buckets = ScopeClassBuckets::new();
        buckets.add_page_tree(&page.tree);
        let components = [ComponentScopeInput {
            name: "tip-panel",
            html_rel: "c/tip-panel.html",
            tree: &comp.tree,
            rules: &comp.dynamic_rules,
        }];
        let mut diags = Vec::new();
        warn_component_rules_out_of_scope(&components, &buckets, &mut diags);
        // .act.is-hover .slot 被形态过滤跳过；.page-only 命中页面证据 → 恰 1 条。
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("page-only"));
    }
}
