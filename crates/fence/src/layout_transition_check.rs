//! layout transition 端点扫描（#10）：`transition: width/height` 声明 × 元素静态可见的
//! 同属性声明全集，端点域必须一致且显式（px↔px / %↔% / vw↔vw；auto 不可动画）。
//!
//! 为什么是打包期 error 而非运行时处理：异域/auto 端点的运行时语义是离散跳变
//! （CSS calc 双域插值与 auto 自然尺寸测量都不在本期范围），而浏览器先验是平滑
//! 过渡——静默放行 = 预览（浏览器）与运行时不可预测地分歧。围栏在此硬拒。
//!
//! 视野边界：本检查只看**静态可见**端点——inline style 声明 + 结构匹配的 class 规则
//! 声明。伪类不算结构（`.open:hover{height:auto}` 照样进池——伪类是运行时开关，
//! 规则结构上就作用在该元素上）；运行时可变属性选择器（hidden/aria-*）规则跳过
//! （可能永不生效，进池防误报）。运行时 `add_class` 挂上新 class 组合出的端点
//! 打包期不可判，由 core rematch 兜底：跨域 snap + EVT_TRANSITION_SNAP 警告事件。

use crate::diagnostic::{Diagnostic, DiagnosticCode, LineMap};
use crate::ir::{IrNodeKind, IrTree};
use yio_core::style::dynamic::DynamicRule;
use yio_core::style::mapping::parse_transition;
use yio_core::style::resolved::ResolvedStyle;
use yio_core::tween::TweenProp;

/// 值文本的端点域归类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EndpointDomain {
    Px,
    Pct,
    Vw,
    Vh,
    Vmin,
    Vmax,
    /// `auto`——不可动画端点（自然尺寸测量不在本期范围）。
    Auto,
    /// calc()/inherit/怪形等——按不可动画报（保守，失败响亮）。
    Other,
}

impl EndpointDomain {
    pub(crate) fn label(self) -> &'static str {
        match self {
            EndpointDomain::Px => "px",
            EndpointDomain::Pct => "%",
            EndpointDomain::Vw => "vw",
            EndpointDomain::Vh => "vh",
            EndpointDomain::Vmin => "vmin",
            EndpointDomain::Vmax => "vmax",
            EndpointDomain::Auto => "auto",
            EndpointDomain::Other => "unsupported value",
        }
    }
}

/// 从声明值文本提取端点域。裸数字按 px（core parse_length 同语义）。
/// `pub(crate)`：@keyframes 停靠点端点校验（css_rules）与本检查共用同一域归类。
pub(crate) fn endpoint_domain_of(value: &str) -> EndpointDomain {
    let v = value.trim();
    if v.is_empty() {
        return EndpointDomain::Other;
    }
    for (suffix, dom) in [
        ("px", EndpointDomain::Px),
        ("%", EndpointDomain::Pct),
        ("vw", EndpointDomain::Vw),
        ("vh", EndpointDomain::Vh),
        ("vmin", EndpointDomain::Vmin),
        ("vmax", EndpointDomain::Vmax),
    ] {
        if let Some(stripped) = v.strip_suffix(suffix) {
            let stripped = stripped.trim();
            if !stripped.is_empty() && stripped.parse::<f32>().is_ok() {
                return dom;
            }
        }
    }
    if v.eq_ignore_ascii_case("auto") {
        return EndpointDomain::Auto;
    }
    if v.parse::<f32>().is_ok() {
        return EndpointDomain::Px;
    }
    EndpointDomain::Other
}

/// transition specs 是否覆盖该通道（all / 显式同名；duration=0 无过渡语义不算）。
fn specs_cover(specs: &[yio_core::style::resolved::TransitionSpec], prop: TweenProp) -> bool {
    specs
        .iter()
        .any(|ts| ts.duration > 0.0 && (ts.prop.is_none() || ts.prop == Some(prop)))
}

/// 元素级端点扫描主入口。`styles` = inline resolve 产物（transition 与 width/height
/// 内联声明都从中读）；`rules` = 全部 class 规则（组件/页面宇宙都扫——域不一致在
/// 任何宇宙都是先验分歧）。
pub fn check_layout_transition_endpoints(
    tree: &IrTree,
    styles: &[ResolvedStyle],
    rules: &[DynamicRule],
    file: &str,
    line_map: &LineMap,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (idx, node) in tree.nodes.iter().enumerate() {
        let IrNodeKind::Element(el) = &node.kind else {
            continue;
        };
        let Some(style) = styles.get(idx) else {
            continue;
        };
        // 结构匹配规则池（伪类规则进池；运行时可变属性选择器规则跳过）。
        let matched: Vec<&DynamicRule> = rules
            .iter()
            .filter(|r| {
                !selector_has_runtime_mutable_attr(r)
                    && crate::control_css_check::selector_matches_node(&r.selector, tree, idx)
            })
            .collect();
        for (prop, prop_name) in [(TweenProp::Width, "width"), (TweenProp::Height, "height")] {
            // 覆盖判定：inline transition（解析产物）或任一匹配规则里的 transition 声明
            //（原文走 core parse_transition，与运行时同一真相源）。
            let rule_side = matched.iter().any(|r| {
                r.declarations.iter().any(|d| {
                    d.prop == "transition" && specs_cover(&parse_transition(&d.value), prop)
                })
            });
            if !rule_side && !specs_cover(&style.transition, prop) {
                continue;
            }
            // 收集静态可见的全部同属性端点值。
            let mut endpoints: Vec<(EndpointDomain, String)> = Vec::new();
            if let Some(attr) = el.attributes.iter().find(|a| a.name == "style") {
                for decl in attr.value.split(';') {
                    let mut it = decl.splitn(2, ':');
                    if it.next().map(|p| p.trim()) == Some(prop_name) {
                        let v = it.next().unwrap_or("").trim().to_string();
                        endpoints.push((endpoint_domain_of(&v), v));
                    }
                }
            }
            for r in &matched {
                for d in &r.declarations {
                    if d.prop == prop_name {
                        endpoints.push((endpoint_domain_of(&d.value), d.value.clone()));
                    }
                }
            }
            check_endpoint_set(prop_name, &endpoints, node, file, line_map, &mut out);
        }
    }
    out
}

/// 端点集合校验：≥2 个域 → 域不一致 error；单域但为 auto/other → 不可动画端点 error；
/// 单一显式域 → 放行（另一半端点可能是运行时态，由 core rematch 兜底覆盖）。
fn check_endpoint_set(
    prop_name: &str,
    endpoints: &[(EndpointDomain, String)],
    node: &crate::ir::IrNode,
    file: &str,
    line_map: &LineMap,
    out: &mut Vec<Diagnostic>,
) {
    if endpoints.is_empty() {
        return;
    }
    let mut domains: Vec<EndpointDomain> = endpoints.iter().map(|(d, _)| *d).collect();
    domains.sort_by_key(|d| d.label());
    domains.dedup();
    let values: Vec<String> = endpoints.iter().map(|(_, v)| format!("`{v}`")).collect();
    let loc = line_map.source_location(node.span.start, file.to_string());
    if domains.len() > 1 {
        out.push(Diagnostic::error(
            DiagnosticCode::FenceLayoutTransitionEndpoint,
            format!(
                "transition {prop_name}: endpoint domains differ ({}) — layout animation \
                 endpoints must stay in ONE domain (px↔px, %↔%, vw↔vw). Values seen: {}. \
                 Mixed-domain endpoints jump instantly instead of animating (browsers \
                 animate them), so the fence rejects them.",
                domains
                    .iter()
                    .map(|d| d.label())
                    .collect::<Vec<_>>()
                    .join(" vs "),
                values.join(", ")
            ),
            loc,
        ));
        return;
    }
    let d = domains[0];
    if matches!(d, EndpointDomain::Auto | EndpointDomain::Other) {
        out.push(Diagnostic::error(
            DiagnosticCode::FenceLayoutTransitionEndpoint,
            format!(
                "transition {prop_name}: endpoint {} is not animatable ({}) — use explicit \
                 px / % / vw / vh / vmin / vmax values. `auto` and non-length values jump \
                 instantly instead of animating (browsers animate to auto via natural-size \
                 measurement, which Yio does not do yet), so the fence rejects them.",
                d.label(),
                values.join(", ")
            ),
            loc,
        ));
    }
}

/// 规则的选择器复合段是否含运行时可变属性选择器（hidden/aria-* 等）——含则本检查
/// 跳过该规则（不把可能永不生效的规则算进端点池，防误报）。
fn selector_has_runtime_mutable_attr(rule: &DynamicRule) -> bool {
    use crate::inline_context_check::RUNTIME_MUTABLE_ATTRS;
    rule.selector.compound.iter().any(|c| {
        c.attrs
            .iter()
            .any(|a| RUNTIME_MUTABLE_ATTRS.contains(&a.name.as_str()))
    })
}
