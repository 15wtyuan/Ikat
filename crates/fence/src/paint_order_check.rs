//! Stage 6.6b：画序声明完整性检查（#101）。两条规则同属「画序意图显式化」：
//!
//! - **E1**（error）：z-index 声明在非定位、非 flex item 的元素上。浏览器对该
//!   声明视而不见（元素留在 static 绘制层），Ikat 运行时却恒生效（fgui 血统：
//!   运行时直改 z 的语义通道）——同一份 HTML，浏览器预览与运行时画序不同，
//!   预览在说谎。围栏硬拒：围栏里写不出来的分歧，preview/运行时就够不着。
//!   运行时 API 直改 z（`set_z`）不受影响——那是 API 层，不是围栏层。
//! - **W1**（warning）：同父兄弟里 static 与 positioned（或声明 z）混排，且
//!   static 侧无显式 z。CSS painting order 里 positioned 元素恒画在 static
//!   内容之上（与树序无关）——漏声明靠「碰巧画对」惯出来的正是 #96/#100
//!   两连发的成因。纯结构判定、零内容猜测（不看有没有文本/touchable——检查器
//!   按启发式沉默 = 检查器说谎）；装饰 overlay 的合法形态有诚实的一行修复：
//!   底图 `position:relative; z-index:0` 显式声明画序意图。
//!
//! 视野边界（同 6.7d 先例）：只看**静态可见**声明——inline style（Stage 4
//! resolve 产物）+ 结构匹配的 class 规则原文（`.overlay{position:absolute}`
//! 类声明必须计入，否则最常见的写法漏拦/误拦）。伪类规则照算（伪类是运行时
//! 开关，结构上就作用在该元素上）；运行时可变属性选择器（hidden/aria-*）规则
//! 跳过（可能永不生效，防误报）。运行时 `add_class` 挂上新 class 的组合不在
//! 打包期视野内。

use crate::diagnostic::{Diagnostic, DiagnosticCode, LineMap};
use crate::ir::{IrNodeKind, IrTree};
use ikat_core::style::dynamic::DynamicRule;
use ikat_core::style::resolved::{PositionDeclared, ResolvedStyle};

/// 一个元素的静态可见画序事实（inline resolve + 结构匹配 class 规则合并）。
#[derive(Debug, Clone, Copy)]
struct StackingFlags {
    positioned: bool,
    z_declared: bool,
    rendered: bool,
}

/// 计算元素的 [`StackingFlags`]。class 规则侧保守合并：任一匹配规则声明
/// `position:relative/absolute` 即视为 positioned、声明 `z-index` 即视为
/// declared、声明 `display:none` 即视为不渲染——多规则互斥时（如一条
/// `position:static` 一条 `absolute`）不猜特异性胜负，取「可能生效」方向
/// （E1 少误报、W1 少噪声）。
fn stacking_flags(
    tree: &IrTree,
    styles: &[ResolvedStyle],
    rules: &[DynamicRule],
    idx: usize,
) -> StackingFlags {
    let s = &styles[idx];
    let mut flags = StackingFlags {
        positioned: s.position_declared != PositionDeclared::Static,
        z_declared: s.z_declared,
        rendered: s.taffy_style.display != taffy::Display::None,
    };
    for r in rules {
        if selector_has_runtime_mutable_attr(r)
            || !crate::control_css_check::selector_matches_node(&r.selector, tree, idx)
        {
            continue;
        }
        for d in &r.declarations {
            match d.prop.as_str() {
                "position" => {
                    let v = d.value.trim().to_ascii_lowercase();
                    if v == "relative" || v == "absolute" {
                        flags.positioned = true;
                    }
                }
                "z-index" => flags.z_declared = true,
                "display" if d.value.trim().eq_ignore_ascii_case("none") => {
                    flags.rendered = false;
                }
                _ => {}
            }
        }
    }
    flags
}

/// 元素是否 flex item：父存在且父的静态可见 display 为 Flex（显式
/// `display:flex`，inline 或 class 规则——浏览器对 flex item 的 z-index 恒
/// 生效，与 Ikat 一致，无需拦）。
fn is_flex_item(
    tree: &IrTree,
    styles: &[ResolvedStyle],
    rules: &[DynamicRule],
    idx: usize,
) -> bool {
    let Some(parent) = tree.nodes[idx].parent else {
        return false;
    };
    let p = &styles[parent.0];
    if p.taffy_style.display == taffy::Display::Flex {
        return true;
    }
    rules.iter().any(|r| {
        !selector_has_runtime_mutable_attr(r)
            && crate::control_css_check::selector_matches_node(&r.selector, tree, parent.0)
            && r.declarations
                .iter()
                .any(|d| d.prop == "display" && d.value.trim().eq_ignore_ascii_case("flex"))
    })
}

/// 规则的选择器复合段是否含运行时可变属性选择器（hidden/aria-* 等）——含则
/// 跳过（与 6.7d 同口径：可能永不生效的规则不进判定，防误报）。
fn selector_has_runtime_mutable_attr(rule: &DynamicRule) -> bool {
    use crate::inline_context_check::RUNTIME_MUTABLE_ATTRS;
    rule.selector.compound.iter().any(|c| {
        c.attrs
            .iter()
            .any(|a| RUNTIME_MUTABLE_ATTRS.contains(&a.name.as_str()))
    })
}

/// 检测画序声明完整性（E1 error + W1 warning）。
///
/// 入参与 [`crate::consistency_check::check_consistency`] 同构 + `rules`（6.7d
/// 同款静态视野）：IrTree（父链 + span 定位）+ Stage 4 css_resolve 产物
/// （styles 按 node index 对齐）+ Stage 4.5 class 规则表。
pub fn check_paint_order(
    tree: &IrTree,
    styles: &[ResolvedStyle],
    rules: &[DynamicRule],
    file: &str,
    line_map: &LineMap,
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();

    // E1：非定位、非 flex item 元素上声明 z-index（`z_declared` 含显式
    // `z-index:0`——声明与否才是意图信号，值本身不是）。
    for (idx, node) in tree.nodes.iter().enumerate() {
        if !matches!(node.kind, IrNodeKind::Element(_)) {
            continue;
        }
        if styles.get(idx).is_none() {
            continue;
        }
        let f = stacking_flags(tree, styles, rules, idx);
        if f.z_declared && !f.positioned && !is_flex_item(tree, styles, rules, idx) {
            diags.push(
                Diagnostic::error(
                    DiagnosticCode::FenceZIndexOnStatic,
                    "z-index declared on a non-positioned element outside a flex \
                     container — browsers ignore z-index there (the element stays \
                     in the static paint layer), but the Ikat runtime honors it, \
                     so the browser preview will not match the runtime paint \
                     order. Add `position:relative` (or `absolute`), or declare \
                     z-index on a child of a `display:flex` container."
                        .to_string(),
                    line_map.source_location(node.span.start, file.to_string()),
                )
                .with_help(
                    "cross-stack-safe stacking recipe: `position:relative` + \
                     explicit `z-index` on the element that needs a specific \
                     paint order.",
                ),
            );
        }
    }

    // W1：同父兄弟 static/positioned(或声明 z) 混排，static 侧无显式 z。
    // 兄弟组 = 同一父的元素子（roots 为一组）；组下标 = 父 node index + 1，
    // 0 号留给 roots（父 index 从 0 起，直接用会与 roots 组撞桶）。
    // display:none 的子不参与画序（不渲染），两侧都排除。
    let mut groups: Vec<Vec<usize>> = vec![Vec::new()];
    for (idx, node) in tree.nodes.iter().enumerate() {
        if !matches!(node.kind, IrNodeKind::Element(_)) {
            continue;
        }
        let group_idx = match node.parent {
            Some(p) => p.0 + 1,
            None => 0,
        };
        if groups.len() <= group_idx {
            groups.resize(group_idx + 1, Vec::new());
        }
        groups[group_idx].push(idx);
    }
    let flags: Vec<Option<StackingFlags>> = tree
        .nodes
        .iter()
        .enumerate()
        .map(|(idx, node)| {
            if matches!(node.kind, IrNodeKind::Element(_)) && styles.get(idx).is_some() {
                Some(stacking_flags(tree, styles, rules, idx))
            } else {
                None
            }
        })
        .collect();
    for group in &groups {
        let rendered: Vec<usize> = group
            .iter()
            .copied()
            .filter(|&i| flags[i].is_some_and(|f| f.rendered))
            .collect();
        let has_stacking_sibling = rendered
            .iter()
            .any(|&i| flags[i].is_some_and(|f| f.positioned || f.z_declared));
        if !has_stacking_sibling {
            continue;
        }
        for &i in &rendered {
            let f = flags[i].unwrap_or_else(|| stacking_flags(tree, styles, rules, i));
            if !f.positioned && !f.z_declared {
                diags.push(
                    Diagnostic::warning(
                        DiagnosticCode::FenceMixedPaintOrder,
                        "static element shares a parent with positioned/z \
                         siblings but declares no z-index — in CSS painting \
                         order, positioned elements always paint above static \
                         content regardless of tree order, so this element may \
                         be silently covered. If the overlap is intended, make \
                         it explicit: `position:relative; z-index:0` (same \
                         visual, declared intent); otherwise give this element \
                         a higher z-index."
                            .to_string(),
                        line_map.source_location(tree.nodes[i].span.start, file.to_string()),
                    )
                    .with_help(
                        "paint order is determined by stacking classes (static \
                         → z0 positioned/opacity/transform → positive z), not \
                         by DOM order — undeclared order only works by luck.",
                    ),
                );
            }
        }
    }

    diags
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Severity;
    use crate::pipeline::parse_template;

    fn has(result: &crate::pipeline::ParsedTemplate, code: DiagnosticCode, sev: Severity) -> bool {
        result
            .diagnostics
            .iter()
            .any(|d| d.code == code && d.severity == sev)
    }

    /// E1：div（block 流）里的 static 子声明 z-index → error。
    #[test]
    fn e1_z_on_static_block_child_errors() {
        let html = r#"<div><div style="z-index:5"></div></div>"#;
        let r = parse_template(html, "t.html");
        assert!(
            has(&r, DiagnosticCode::FenceZIndexOnStatic, Severity::Error),
            "static block 子声明 z 应 error: {:?}",
            r.diagnostics
        );
    }

    /// E1：z-index:0 显式声明同样 error（声明与否是意图信号，值不是）。
    #[test]
    fn e1_explicit_zero_still_errors() {
        let html = r#"<div><div style="z-index:0"></div></div>"#;
        let r = parse_template(html, "t.html");
        assert!(
            has(&r, DiagnosticCode::FenceZIndexOnStatic, Severity::Error),
            "显式 z-index:0 应 error: {:?}",
            r.diagnostics
        );
    }

    /// E1 逃生口一：position:relative + z → 合法。
    #[test]
    fn e1_positioned_z_passes() {
        let html = r#"<div><div style="position:relative;z-index:5"></div></div>"#;
        let r = parse_template(html, "t.html");
        assert!(
            !has(&r, DiagnosticCode::FenceZIndexOnStatic, Severity::Error),
            "positioned 元素声明 z 合法: {:?}",
            r.diagnostics
        );
    }

    /// E1 逃生口二：flex 父的 flex item 声明 z → 合法（浏览器对 flex item 的
    /// z 恒生效，无分歧）。
    #[test]
    fn e1_flex_item_z_passes() {
        let html = r#"<div style="display:flex"><div style="z-index:5"></div></div>"#;
        let r = parse_template(html, "t.html");
        assert!(
            !has(&r, DiagnosticCode::FenceZIndexOnStatic, Severity::Error),
            "flex item 声明 z 合法: {:?}",
            r.diagnostics
        );
    }

    /// E1 静态视野：class 规则声明 position、inline 声明 z（showcase zi-chip
    /// 同款形态）→ 合法，不误报。
    #[test]
    fn e1_class_rule_position_counts() {
        let html = r#"<html><head><style>.chip{position:absolute;width:50px}</style></head><body><div><div class="chip" style="z-index:3">a</div></div></body></html>"#;
        let r = parse_template(html, "t.html");
        assert!(
            !has(&r, DiagnosticCode::FenceZIndexOnStatic, Severity::Error),
            "class 规则的 position 必须计入: {:?}",
            r.diagnostics
        );
    }

    /// E1 反向盲区补齐：class 规则声明 z-index（非 inline）同样 error。
    #[test]
    fn e1_class_rule_z_also_errors() {
        let html = r#"<html><head><style>.card{z-index:3}</style></head><body><div><div class="card">a</div></div></body></html>"#;
        let r = parse_template(html, "t.html");
        assert!(
            has(&r, DiagnosticCode::FenceZIndexOnStatic, Severity::Error),
            "class 规则声明的 z 同样要拦: {:?}",
            r.diagnostics
        );
    }

    /// E1：class 规则 display:flex 的父 → 子是 flex item，声明 z 合法。
    #[test]
    fn e1_class_rule_flex_parent_passes() {
        let html = r#"<html><head><style>.row{display:flex}</style></head><body><div class="row"><div style="z-index:2">a</div></div></body></html>"#;
        let r = parse_template(html, "t.html");
        assert!(
            !has(&r, DiagnosticCode::FenceZIndexOnStatic, Severity::Error),
            "class flex 父的子声明 z 合法: {:?}",
            r.diagnostics
        );
    }

    /// E1：未声明 z 不触发。
    #[test]
    fn e1_no_z_no_error() {
        let html = r#"<div><div style="width:10px"></div></div>"#;
        let r = parse_template(html, "t.html");
        assert!(
            !has(&r, DiagnosticCode::FenceZIndexOnStatic, Severity::Error),
            "无 z 声明不应 error: {:?}",
            r.diagnostics
        );
    }

    /// W1：static 与 positioned 兄弟混排 → static 侧 warning（结构性，不看内容）。
    #[test]
    fn w1_mixed_siblings_warn_static_side() {
        let html = r#"<div><div class="base"></div><div class="overlay" style="position:absolute"></div></div>"#;
        let r = parse_template(html, "t.html");
        assert!(
            has(&r, DiagnosticCode::FenceMixedPaintOrder, Severity::Warning),
            "static/positioned 混排应 warning: {:?}",
            r.diagnostics
        );
    }

    /// W1 静态视野：positioned 由 class 规则声明同样构成触发侧。
    #[test]
    fn w1_class_rule_positioned_triggers() {
        let html = r#"<html><head><style>.overlay{position:absolute}</style></head><body><div><div class="base"></div><div class="overlay"></div></div></body></html>"#;
        let r = parse_template(html, "t.html");
        assert!(
            has(&r, DiagnosticCode::FenceMixedPaintOrder, Severity::Warning),
            "class 规则 positioned 兄弟应触发: {:?}",
            r.diagnostics
        );
    }

    /// W1 修复路径：static 侧补 `position:relative;z-index:0` → 消警告（同视觉，
    /// 显式意图）。
    #[test]
    fn w1_declared_intent_silences() {
        let html = r#"<div><div class="base" style="position:relative;z-index:0"></div><div class="overlay" style="position:absolute"></div></div>"#;
        let r = parse_template(html, "t.html");
        assert!(
            !has(&r, DiagnosticCode::FenceMixedPaintOrder, Severity::Warning),
            "显式声明画序意图后不应 warning: {:?}",
            r.diagnostics
        );
    }

    /// W1：全 static 兄弟（无 positioned/z）→ 不告警。
    #[test]
    fn w1_all_static_no_warn() {
        let html = r#"<div><div class="a"></div><div class="b"></div></div>"#;
        let r = parse_template(html, "t.html");
        assert!(
            !has(&r, DiagnosticCode::FenceMixedPaintOrder, Severity::Warning),
            "无 positioned 兄弟不应 warning: {:?}",
            r.diagnostics
        );
    }

    /// W1：声明 z 的兄弟（flex 上下文合法）也构成「混排」触发侧。
    #[test]
    fn w1_z_declared_sibling_triggers() {
        let html = r#"<div style="display:flex"><div class="a"></div><div class="b" style="z-index:2"></div></div>"#;
        let r = parse_template(html, "t.html");
        assert!(
            has(&r, DiagnosticCode::FenceMixedPaintOrder, Severity::Warning),
            "声明 z 的兄弟应触发混排 warning: {:?}",
            r.diagnostics
        );
    }

    /// W1：display:none 兄弟不参与画序，两侧都排除（inline 与 class 规则两形态）。
    #[test]
    fn w1_display_none_sibling_ignored() {
        let html = r#"<div><div class="hidden" style="display:none;position:absolute"></div><div class="base"></div></div>"#;
        let r = parse_template(html, "t.html");
        assert!(
            !has(&r, DiagnosticCode::FenceMixedPaintOrder, Severity::Warning),
            "display:none 兄弟不构成混排: {:?}",
            r.diagnostics
        );
        let html2 = r#"<html><head><style>.hidden{display:none}</style></head><body><div><div class="hidden" style="position:absolute"></div><div class="base"></div></div></body></html>"#;
        let r2 = parse_template(html2, "t.html");
        assert!(
            !has(&r2, DiagnosticCode::FenceMixedPaintOrder, Severity::Warning),
            "class 规则 display:none 兄弟不构成混排: {:?}",
            r2.diagnostics
        );
    }

    /// W1：模板根元素一组同样适用（overlay 根 + static 根）。
    #[test]
    fn w1_root_group_scanned() {
        let html =
            r#"<div class="base"></div><div class="overlay" style="position:absolute"></div>"#;
        let r = parse_template(html, "t.html");
        assert!(
            has(&r, DiagnosticCode::FenceMixedPaintOrder, Severity::Warning),
            "根级兄弟组同样适用: {:?}",
            r.diagnostics
        );
    }
}
