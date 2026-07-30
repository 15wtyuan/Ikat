//! Stage 6.8：控件结构契约校验（必需子角色）。
//!
//! 旧模式下框架运行时注入 `.loom-*` 子节点（fill/track/thumb/listbox/...），
//! 控件结构必然完整，无需校验。role 化重构后（spec §2.2），作者自己写
//! `<div role="combobox"><div role="listbox"><div role="option">`——作者可能漏写
//! 必需子节点。本 pass 在打包期（annotate 之后）严格拦截这种缺陷：缺必需子角色
//! = `FenceMissingControlChild` error，不依赖运行时 reparent 兜底。
//!
//! 契约表见 spec §2.2：combobox→listbox、listbox→option、slider→data-slot=thumb、
//! progressbar→data-slot=fill、list→listitem。textbox/spinbutton/switch/radio
//! 无必需子角色（不校验）。校验只看**直接子节点**，与 spec §2.2 结构字面对齐。

use crate::diagnostic::{Diagnostic, DiagnosticCode, LineMap};
use crate::ir::{IrElement, IrNodeKind, IrTree};

/// 必需子节点的判定方式：按 role 或按 data-slot。
#[derive(Debug, Clone, Copy)]
enum CheckSpec {
    /// 直接子节点带某个 `role` 值。
    Role(&'static str),
    /// 直接子节点带某个 `data-slot` 值。
    Slot(&'static str),
}

/// 每控件 role 的必需子角色/slot 契约（spec §2.2 权威）。
///
/// 只列有必需子角色的控件；textbox/spinbutton/switch/radio 无必需子角色，缺席。
/// `listbox` 在 `ROLE_TO_SEMANTIC` 映射成 Container，但结构契约按 role 字面值
/// 校验（`<div role="listbox">`），与 SemanticKind 无关。
const REQUIRED_CHILDREN: &[(&str, &[CheckSpec])] = &[
    ("combobox", &[CheckSpec::Role("listbox")]),
    ("listbox", &[CheckSpec::Role("option")]),
    ("slider", &[CheckSpec::Slot("thumb")]),
    ("progressbar", &[CheckSpec::Slot("fill")]),
    ("list", &[CheckSpec::Role("listitem")]),
];

/// 读元素的 `role` 属性值（若存在）。
fn node_role(el: &IrElement) -> Option<&str> {
    el.attributes
        .iter()
        .find(|a| a.name == "role")
        .map(|a| a.value.as_str())
}

/// 读元素的 `data-slot` 属性值（若存在）。
fn node_slot(el: &IrElement) -> Option<&str> {
    el.attributes
        .iter()
        .find(|a| a.name == "data-slot")
        .map(|a| a.value.as_str())
}

/// 直接子节点中是否存在满足 spec 的节点（按 role 或 data-slot 匹配）。
///
/// 数据驱动 ListView 把 item 蓝图写在 `<template>` 子节点里（运行时克隆产 slot），
/// list 节点本身没有直接 `role="listitem"` 子节点。本校验因此把直接 `<template>`
/// 子节点的首个元素子节点视同直接子节点一并检查（template 蓝图模式），与 spec §2.2
/// list→listitem 契约一致——作者两种写法（直接 listitem / template>listitem）都合法。
fn has_required_child(tree: &IrTree, parent_idx: usize, spec: CheckSpec) -> bool {
    let children: Vec<usize> = tree.nodes[parent_idx]
        .children
        .iter()
        .map(|c| c.0)
        .collect();
    for child_idx in children {
        let IrNodeKind::Element(el) = &tree.nodes[child_idx].kind else {
            continue;
        };
        let matched = match spec {
            CheckSpec::Role(r) => node_role(el) == Some(r),
            CheckSpec::Slot(s) => node_slot(el) == Some(s),
        };
        if matched {
            return true;
        }
        // template 蓝图模式：直接子是 <template> 时，看其首个元素子节点是否满足 spec
        // （ListView item 蓝图 `role=list > template > role=listitem`）。
        if el.tag == "template" {
            if let Some(&tpl_child) = tree.nodes[child_idx]
                .children
                .iter()
                .find(|c| matches!(tree.nodes[c.0].kind, IrNodeKind::Element(_)))
            {
                if let IrNodeKind::Element(tpl_el) = &tree.nodes[tpl_child.0].kind {
                    let tpl_matched = match spec {
                        CheckSpec::Role(r) => node_role(tpl_el) == Some(r),
                        CheckSpec::Slot(s) => node_slot(tpl_el) == Some(s),
                    };
                    if tpl_matched {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// 把 CheckSpec 渲染成教学文案片段（如 `` `role="listbox"` `` 或 `` `data-slot="thumb"` ``）。
fn spec_label(spec: CheckSpec) -> String {
    match spec {
        CheckSpec::Role(r) => format!("`role=\"{r}\"`"),
        CheckSpec::Slot(s) => format!("`data-slot=\"{s}\"`"),
    }
}

/// 每个 role 的教学文案：作者应写的完整结构（诊断 message 用）。
/// 与 spec §2.2 表一致——告诉作者「该怎么写」，而非「漏了什么」。
fn structure_hint(role: &str) -> Option<&'static str> {
    Some(match role {
        "combobox" => {
            "a `<div role=\"combobox\">` needs a `role=\"listbox\"` child, \
             which in turn contains one or more `role=\"option\"` children"
        }
        "listbox" => "a `<div role=\"listbox\">` needs at least one `role=\"option\"` child",
        "slider" => {
            "a `<div role=\"slider\">` needs a `data-slot=\"thumb\"` child \
            (the draggable handle); a `data-slot=\"fill\"` child is optional for the \
            filled portion"
        }
        "progressbar" => {
            "a `<div role=\"progressbar\">` needs a `data-slot=\"fill\"` child \
            (the fill bar); the progress element itself acts as the track"
        }
        "list" => "a `<div role=\"list\">` needs at least one `role=\"listitem\"` child",
        _ => return None,
    })
}

/// 校验所有 role 驱动控件的必需子角色。返回诊断（error 列表）。
///
/// 入参：
/// - `tree`：IrTree（已过 Annotate）
/// - `file` / `line_map`：定位诊断
pub fn check_control_structure(tree: &IrTree, file: &str, line_map: &LineMap) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (idx, node) in tree.nodes.iter().enumerate() {
        let IrNodeKind::Element(el) = &node.kind else {
            continue;
        };
        // 只校验 role 驱动节点（带 role 属性）。
        let Some(role) = node_role(el) else {
            continue;
        };
        let Some((_, specs)) = REQUIRED_CHILDREN.iter().find(|(r, _)| *r == role) else {
            // role 不在契约表（textbox/spinbutton/switch/radio/...）→ 无必需子角色。
            continue;
        };
        for spec in *specs {
            if has_required_child(tree, idx, *spec) {
                continue;
            }
            let hint = structure_hint(role).unwrap_or("see docs/design/fence.md §2.2");
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::FenceMissingControlChild,
                format!(
                    "LoomGUI control `<{tag} role=\"{role}\">` is missing its required \
                     {label} child element. Controls have NO framework-injected children — \
                     the author writes the full structure. Expected: {hint}. See \
                     docs/superpowers/specs/2026-07-30-control-role-refactor-and-fence-tightening-design.md §2.2.",
                    tag = el.tag,
                    label = spec_label(*spec),
                ),
                line_map.source_location(node.span.start, file.to_string()),
            ));
        }
    }
    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::parse_template;

    /// 端到端跑 parse_template，只取结构契约诊断。
    fn struct_diags(html: &str) -> Vec<Diagnostic> {
        let result = parse_template(html, "t.html");
        result
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FenceMissingControlChild)
            .cloned()
            .collect()
    }

    #[test]
    fn combobox_missing_listbox_errors() {
        // role=combobox 无 role=listbox 子 → error
        let diags = struct_diags(r#"<div role="combobox"></div>"#);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("combobox"));
        assert!(diags[0].message.contains("listbox"));
    }

    #[test]
    fn combobox_with_option_but_no_listbox_errors() {
        // option 直接挂 combobox（缺 listbox 中间层）→ 打包期报 error，不依赖运行时 reparent 兜底
        let diags = struct_diags(r#"<div role="combobox"><div role="option">A</div></div>"#);
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    #[test]
    fn listbox_without_option_errors() {
        let diags = struct_diags(r#"<div role="listbox"></div>"#);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("option"));
    }

    #[test]
    fn slider_missing_thumb_errors() {
        let diags = struct_diags(r#"<div role="slider"></div>"#);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("thumb"));
    }

    #[test]
    fn progressbar_missing_fill_errors() {
        let diags = struct_diags(r#"<div role="progressbar"></div>"#);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("fill"));
    }

    #[test]
    fn list_missing_listitem_errors() {
        let diags = struct_diags(r#"<div role="list"></div>"#);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("listitem"));
    }

    #[test]
    fn combobox_with_full_structure_ok() {
        // 完整结构：combobox > listbox > option → 无 error
        let diags = struct_diags(
            r#"<div role="combobox"><div role="listbox"><div role="option">A</div></div></div>"#,
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn slider_with_thumb_ok() {
        let diags = struct_diags(r#"<div role="slider"><div data-slot="thumb"></div></div>"#);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn progressbar_with_fill_ok() {
        let diags = struct_diags(r#"<div role="progressbar"><div data-slot="fill"></div></div>"#);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn list_with_listitem_ok() {
        let diags = struct_diags(r#"<div role="list"><div role="listitem">A</div></div>"#);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn list_with_template_listitem_ok() {
        // 数据驱动 ListView 蓝图模式：role=list > template > role=listitem（运行时克隆产 slot）。
        // list 无直接 listitem 子节点，但 template 蓝图里的 listitem 满足结构契约。
        let diags = struct_diags(
            r#"<div role="list" data-fill="3"><template><div role="listitem" class="item">A</div></template></div>"#,
        );
        assert!(
            diags.is_empty(),
            "template 蓝图模式应满足 list→listitem: {diags:?}"
        );
    }

    #[test]
    fn list_template_with_non_listitem_root_errors() {
        // template 存在但根不是 listitem → 仍报 error（防止作者写错蓝图根）
        let diags = struct_diags(
            r#"<div role="list"><template><div class="wrong">A</div></template></div>"#,
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("listitem"));
    }

    #[test]
    fn no_required_children_controls_not_checked() {
        // textbox / spinbutton / switch / radio 无必需子角色，裸节点不报错
        let diags = struct_diags(
            r#"<div role="textbox"></div><div role="spinbutton"></div><div role="switch"></div><div role="radio"></div>"#,
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn required_child_must_be_direct() {
        // 必需子节点必须是直接子（spec §2.2 字面结构）；嵌套进 wrapper 不算
        let diags = struct_diags(
            r#"<div role="slider"><div class="wrap"><div data-slot="thumb"></div></div></div>"#,
        );
        assert_eq!(diags.len(), 1, "thumb 嵌在 wrapper 里不算直接子: {diags:?}");
    }

    #[test]
    fn diagnostic_severity_is_error() {
        let diags = struct_diags(r#"<div role="slider"></div>"#);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, crate::diagnostic::Severity::Error);
    }
}
