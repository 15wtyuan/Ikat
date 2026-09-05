//! Stage 6.8：控件结构契约校验（必需子角色）。
//!
//! 旧模式下框架运行时注入 `.yio-*` 子节点（fill/track/thumb/listbox/...），
//! 控件结构必然完整，无需校验。role 化重构后，作者自己写
//! `<div role="combobox"><div role="listbox"><div role="option">`——作者可能漏写
//! 必需子节点。本 pass 在打包期（annotate 之后）严格拦截这种缺陷：缺必需子角色
//! = `FenceMissingControlChild` error，不依赖运行时 reparent 兜底。
//!
//! 校验只看**直接子节点**。

use crate::diagnostic::{Diagnostic, DiagnosticCode, LineMap};
use crate::ir::{IrElement, IrNodeKind, IrTree};

/// 必需子节点的判定方式：按 role 或按 data-slot。
/// pub：templates_sync 交叉校验用（分发的 fence-schema.md role registry 行与本表
/// 对账——契约行漏写 data-slot=value 会让人照抄文档 build 失败，#90 实证）。
#[derive(Debug, Clone, Copy)]
pub enum CheckSpec {
    /// 直接子节点带某个 `role` 值。
    Role(&'static str),
    /// 直接子节点带某个 `data-slot` 值。
    Slot(&'static str),
}

/// 每控件 role 的必需子角色/slot 契约。
///
/// 只列有必需子角色的控件；textbox/spinbutton/switch/radio 无必需子角色，缺席。
/// `listbox` 在 `ROLE_TO_SEMANTIC` 映射成 Container，但结构契约按 role 字面值
/// 校验（`<div role="listbox">`），与 SemanticKind 无关。
/// combobox 的 `data-slot=value` 是选中值显示区——运行时 sync 把选中 option 文本
/// 写进它内嵌的 TextNode，漏写 = 选中值静默无显示（此前无任何拦截）。
pub const REQUIRED_CHILDREN: &[(&str, &[CheckSpec])] = &[
    (
        "combobox",
        &[CheckSpec::Role("listbox"), CheckSpec::Slot("value")],
    ),
    ("listbox", &[CheckSpec::Role("option")]),
    ("slider", &[CheckSpec::Slot("thumb")]),
    ("progressbar", &[CheckSpec::Slot("fill")]),
    ("list", &[CheckSpec::Role("listitem")]),
    ("tablist", &[CheckSpec::Role("tab")]),
    ("tree", &[CheckSpec::Role("treeitem")]),
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

/// 直接子节点中满足 spec 的全部实例（按 role 或 data-slot 匹配，返回节点 idx 列表）。
///
/// 数据驱动 ListView 把 item 蓝图写在 `<template>` 子节点里（运行时克隆产 slot），
/// list 节点本身没有直接 `role="listitem"` 子节点。本函数把直接 `<template>`
/// 子节点的首个元素子节点视同直接子节点一并返回（template 蓝图模式），与
/// list→listitem 契约一致——作者两种写法（直接 listitem / template>listitem）都合法。
/// 供两处消费：结构契约（非空即满足）+ CSS 命中校验（每个实例都须被规则命中）。
pub(crate) fn required_child_instances(
    tree: &IrTree,
    parent_idx: usize,
    spec: CheckSpec,
) -> Vec<usize> {
    let mut found = Vec::new();
    let matches_spec = |el: &IrElement| match spec {
        CheckSpec::Role(r) => node_role(el) == Some(r),
        CheckSpec::Slot(s) => node_slot(el) == Some(s),
    };
    for &child in &tree.nodes[parent_idx].children {
        let IrNodeKind::Element(el) = &tree.nodes[child.0].kind else {
            continue;
        };
        if matches_spec(el) {
            found.push(child.0);
        }
        // template 蓝图模式：直接子是 <template> 时，看其首个元素子节点是否满足 spec
        // （ListView item 蓝图 `role=list > template > role=listitem`）。
        if el.tag == "template" {
            if let Some(&tpl_child) = tree.nodes[child.0]
                .children
                .iter()
                .find(|c| matches!(tree.nodes[c.0].kind, IrNodeKind::Element(_)))
            {
                if let IrNodeKind::Element(tpl_el) = &tree.nodes[tpl_child.0].kind {
                    if matches_spec(tpl_el) {
                        found.push(tpl_child.0);
                    }
                }
            }
        }
    }
    found
}

/// 直接子节点中是否存在满足 spec 的节点（按 role 或 data-slot 匹配）。
fn has_required_child(tree: &IrTree, parent_idx: usize, spec: CheckSpec) -> bool {
    !required_child_instances(tree, parent_idx, spec).is_empty()
}

/// 把 CheckSpec 渲染成教学文案片段（如 `` `role="listbox"` `` 或 `` `data-slot="thumb"` ``）。
fn spec_label(spec: CheckSpec) -> String {
    match spec {
        CheckSpec::Role(r) => format!("`role=\"{r}\"`"),
        CheckSpec::Slot(s) => format!("`data-slot=\"{s}\"`"),
    }
}

/// 每个 role 的教学文案：作者应写的完整结构（诊断 message 用）。
/// 与契约表一致——告诉作者「该怎么写」，而非「漏了什么」。
fn structure_hint(role: &str) -> Option<&'static str> {
    Some(match role {
        "combobox" => {
            "a `<div role=\"combobox\">` needs a `role=\"listbox\"` child (which in turn \
             contains one or more `role=\"option\"` children) and a `data-slot=\"value\"` \
             child (the selected-value display area)"
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
        "tablist" => "a `<div role=\"tablist\">` needs at least one `role=\"tab\"` child (panels link via aria-controls)",
        "tree" => "a `<div role=\"tree\">` needs at least one `role=\"treeitem\"` child (items nest directly inside treeitem — no group wrapper)",
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
            let hint = structure_hint(role).unwrap_or(
                "see the role registry in the scaffolded \
                 yio-editor skill (`references/fence-schema.md`)",
            );
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::FenceMissingControlChild,
                format!(
                    "Yio control `<{tag} role=\"{role}\">` is missing its required \
                     {label} child element. Controls have NO framework-injected children — \
                     the author writes the full structure. Expected: {hint}. \
                     Contract table: the role registry in the scaffolded yio-editor \
                     skill (`references/fence-schema.md`).",
                    tag = el.tag,
                    label = spec_label(*spec),
                ),
                line_map.source_location(node.span.start, file.to_string()),
            ));
        }
    }
    diagnostics
}

/// style 属性串里是否声明了 `display: none`（按 `;` 分隔逐条比对；属性名/值均
/// 大小写不敏感，空白容忍——照 CSS 声明解析惯例的子集）。
fn style_declares_display_none(style: &str) -> bool {
    style.split(';').any(|decl| {
        let Some((prop, val)) = decl.split_once(':') else {
            return false;
        };
        prop.trim().eq_ignore_ascii_case("display") && val.trim().eq_ignore_ascii_case("none")
    })
}

/// 校验 `role="tabpanel"` 未手写内联 `display:none`。返回诊断（error 列表）。
///
/// TabList 运行时切面板 = 激活面板 **unset inline display** 回落作者样式——作者把
/// `display:none` 写进 panel 的 style 属性会烙进打包期 base_style，unset 清不掉：
/// 激活面板永久不可见且无运行时症状可查。旧 runtime 用 display:block 覆写曾掩盖
/// 此写法，面板显隐所有权收归控件后（#48）存量写法静默坏——打包期点破。非激活
/// 面板的初始隐藏由控件运行时首帧负责，作者不可（也无需）手写。
pub fn check_tabpanel_author_hidden(
    tree: &IrTree,
    file: &str,
    line_map: &LineMap,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for node in tree.nodes.iter() {
        let IrNodeKind::Element(el) = &node.kind else {
            continue;
        };
        if node_role(el) != Some("tabpanel") {
            continue;
        }
        let Some(hidden) = el
            .attributes
            .iter()
            .find(|a| a.name == "style")
            .map(|a| style_declares_display_none(&a.value))
        else {
            continue;
        };
        if hidden {
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::FenceTabpanelHiddenByAuthor,
                format!(
                    "Yio `<{tag} role=\"tabpanel\">` declares inline `display:none`. \
                     The TabList runtime shows the ACTIVE panel by clearing its own inline \
                     display override and falling back to author styles — an author-baked \
                     display:none survives that (baked into the packed base style) and keeps \
                     the active panel permanently invisible. Hiding inactive panels is the \
                     control runtime's job (applied on the first frame) — remove the \
                     declaration. The `tabpanel` row of the role registry in the scaffolded \
                     yio-editor skill (`references/fence-schema.md`) states this rule.",
                    tag = el.tag,
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
        // role=combobox 无 role=listbox 子且无 data-slot=value 子 → 两条 error（各缺一样）
        let diags = struct_diags(r#"<div role="combobox"></div>"#);
        assert_eq!(diags.len(), 2, "{diags:?}");
        assert!(diags.iter().any(|d| d.message.contains("listbox")));
    }

    #[test]
    fn combobox_missing_value_slot_errors() {
        // 有 listbox 但缺 data-slot=value（选中值显示区）→ error（此前静默：选中值无显示）
        let diags = struct_diags(
            r#"<div role="combobox"><div role="listbox"><div role="option">A</div></div></div>"#,
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("value"));
    }

    #[test]
    fn combobox_with_option_but_no_listbox_errors() {
        // option 直接挂 combobox（缺 listbox 中间层 + 缺 value）→ 两条 error，不依赖运行时兜底
        let diags = struct_diags(r#"<div role="combobox"><div role="option">A</div></div>"#);
        assert_eq!(diags.len(), 2, "{diags:?}");
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
    fn tree_without_treeitem_child_is_error() {
        // role=tree 无直接 role=treeitem 子 → error（Tree 结构契约，#8）
        let diags = struct_diags(r#"<div role="tree"><div>placeholder</div></div>"#);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("treeitem"));
    }

    #[test]
    fn tree_with_treeitem_child_passes() {
        // 直接子含 treeitem → 过；嵌套深度不设限（treeitem 内直接嵌 treeitem 是声明契约）
        let diags = struct_diags(
            r#"<div role="tree"><div role="treeitem"><div>A</div><div role="treeitem">B</div></div></div>"#,
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn tablist_without_tab_child_is_error() {
        // role=tablist 无 role=tab 子 → error（TabList 结构契约）
        let diags = struct_diags(r#"<div role="tablist"></div>"#);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("tab"));
    }

    #[test]
    fn tablist_with_tab_child_passes() {
        // role=tablist 含 role=tab 子 → 无 error（panel 靠 aria-controls 关联，不需在此校验）
        let diags = struct_diags(
            r#"<div role="tablist"><button role="tab" aria-controls="p1">A</button></div><div id="p1"></div>"#,
        );
        assert!(
            diags.is_empty(),
            "tablist with tab child should pass: {diags:?}"
        );
    }

    #[test]
    fn combobox_with_full_structure_ok() {
        // 完整结构：combobox > value + listbox > option → 无 error
        let diags = struct_diags(
            r#"<div role="combobox"><div data-slot="value">A</div><div role="listbox"><div role="option">A</div></div></div>"#,
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
        // 必需子节点必须是直接子；嵌套进 wrapper 不算
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

    /// 端到端跑 parse_template，只取 tabpanel 手写隐藏诊断。
    fn panel_diags(html: &str) -> Vec<Diagnostic> {
        let result = parse_template(html, "t.html");
        result
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FenceTabpanelHiddenByAuthor)
            .cloned()
            .collect()
    }

    #[test]
    fn tabpanel_author_display_none_is_error() {
        // 回归（review 抓出）：作者把 display:none 烙进 tabpanel 的 style 属性 →
        // base_style 永久隐藏激活面板（运行时 unset inline 清不掉它），打包期拦截。
        let diags = panel_diags(
            r#"<div role="tablist"><button role="tab" aria-controls="p1">A</button></div><div role="tabpanel" id="p1" style="display:none"></div>"#,
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].severity, crate::diagnostic::Severity::Error);
    }

    #[test]
    fn tabpanel_other_inline_styles_pass() {
        // 非 display 声明 / 非.none 值 / 无 style 属性 → 不误报。
        let diags = panel_diags(
            r#"<div role="tabpanel" style="padding:10px"></div><div role="tabpanel" style="display:flex"></div><div role="tabpanel"></div>"#,
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn tabpanel_display_none_variants_caught() {
        // 大小写/空白容忍：`DISPLAY:NONE`、`; display : None ;` 均拦；普通 div 同写法不拦
        //（只查 role=tabpanel——普通节点手写 display:none 是作者自己的显隐逻辑）。
        let diags = panel_diags(
            r#"<div role="tabpanel" style="DISPLAY:NONE"></div><div role="tabpanel" style="color:red; display : None;"></div><div style="display:none"></div>"#,
        );
        assert_eq!(diags.len(), 2, "{diags:?}");
    }
}
