//! `<style>` 选择器解析 + 规则表产物（fence = 纯解析器）。
//!
//! 路径 c：手搓解析器，直产 core 的 ParsedSelector/Compound（fence 已依赖 core）。
//! 子集：class / tag / id / 后代组合（空格）/ 伪类（hover/active/disabled/focus/checked）。
//! 越界（属性选择器、nth-child、+ ~ 组合子、逗号多选等）返 None，由调用方报错。
use crate::diagnostic::{Diagnostic, DiagnosticCode, LineMap, SourceLocation};
use crate::schema::css::{find_css_prop, find_shorthand};
use loomgui_core::style::dynamic::{
    Combinator, Compound, Declaration, DynamicRule, ParsedSelector, Specificity,
};

/// 解析单条选择器串 → ParsedSelector（含 specificity）。越界返 None。
///
/// 子集：空格分隔的若干 compound（后代组合）；每个 compound = tag? + (class/id/pseudo)*。
/// 越界：属性选择器 `[...]`、Child `>`、相邻 `+`/`~`、逗号多选 → None。
pub fn parse_selector(raw: &str) -> Option<ParsedSelector> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    // 越界字符快速判定（本子集不含这些）
    if raw.contains('[')
        || raw.contains(',')
        || raw.contains('>')
        || raw.contains('+')
        || raw.contains('~')
    {
        return None;
    }

    let mut specificity_a = 0u32; // id 数
    let mut specificity_b = 0u32; // class + 伪类 + 属性 数
    let mut specificity_c = 0u32; // tag 数
    let mut compounds: Vec<Compound> = Vec::new();

    for part in raw.split_whitespace() {
        let (c, a, b, cc) = parse_compound(part)?;
        specificity_a += a;
        specificity_b += b;
        specificity_c += cc;
        // 本子集只有后代组合（空格）；首个 compound 的 combinator 字段无前驱，matcher 不读
        let mut c = c;
        c.combinator = Combinator::Descendant;
        compounds.push(c);
    }

    if compounds.is_empty() {
        return None;
    }
    Some(ParsedSelector {
        raw: raw.to_string(),
        compound: compounds,
        specificity: Specificity(specificity_a, specificity_b, specificity_c),
    })
}

/// 解析单个 compound（无空格的一段）。返 (compound, a, b, c) specificity 贡献。
fn parse_compound(part: &str) -> Option<(Compound, u32, u32, u32)> {
    let mut c = Compound {
        tag: None,
        classes: Vec::new(),
        id: None,
        combinator: Combinator::Descendant,
        pseudo_hover: false,
        pseudo_active: false,
        pseudo_disabled: false,
        pseudo_focus: false,
        attrs: Vec::new(),
    };
    let mut a = 0u32;
    let mut b = 0u32;
    let mut cc = 0u32;
    let mut rest = part;
    let mut consumed_tag = false;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix('.') {
            let (name, next) = take_ident(r);
            if name.is_empty() {
                return None;
            }
            c.classes.push(name.to_string());
            b += 1;
            rest = next;
        } else if let Some(r) = rest.strip_prefix('#') {
            let (name, next) = take_ident(r);
            if name.is_empty() {
                return None;
            }
            c.id = Some(name.to_string());
            a += 1;
            rest = next;
        } else if let Some(r) = rest.strip_prefix(':') {
            let (name, next) = take_ident(r);
            match name {
                "hover" => c.pseudo_hover = true,
                "active" => c.pseudo_active = true,
                "disabled" => c.pseudo_disabled = true,
                "focus" => c.pseudo_focus = true,
                "checked" => {
                    // core 的 Compound 无 pseudo_checked 字段：checked 是控件态，由控件束处理（Spec-4）。
                    // 本轮仅计 specificity（b+=1 在下方统一加），不存状态门。
                }
                _ => return None, // 未知伪类越界（含 nth-child 等）
            }
            b += 1; // 伪类算 class 级
            rest = next;
        } else {
            // tag（必须出现在 compound 最前）
            if consumed_tag {
                return None; // tag 后面跟了非 .#: 的字符 → 非法形态
            }
            let (name, next) = take_ident(rest);
            if name.is_empty() {
                return None;
            }
            c.tag = Some(name.to_string());
            cc += 1;
            consumed_tag = true;
            rest = next;
        }
    }
    Some((c, a, b, cc))
}

/// 取一个标识符（字母/数字/`-`/`_`），返回 (标识符, 剩余)。
fn take_ident(s: &str) -> (&str, &str) {
    let end = s
        .find(|ch: char| !ch.is_alphanumeric() && ch != '-' && ch != '_')
        .unwrap_or(s.len());
    (&s[..end], &s[end..])
}

/// 解析一段 `<style>` 文本 → 规则表 + 诊断。
///
/// 文法（子集）：`selector_list? { decl_list }` 重复；`selector_list` 为单选择器（不支持逗号）；
/// `decl_list` = `prop: value;` 重复。CSS 注释 `/* ... */` 剥除。越界选择器 → 该规则丢弃 + 诊断；
/// 声明 prop 名不在 schema（find_css_prop/find_shorthand）→ 诊断（与 css_resolve 一致）。
pub fn parse_style_block(css: &str) -> (Vec<DynamicRule>, Vec<Diagnostic>) {
    let stripped = strip_comments(css);
    // 诊断定位用（粗略）：strip_comments 后 offset 已不对应原文，但行号近似可用。
    let line_map = LineMap::new(&stripped);
    let mut rules = Vec::new();
    let mut diagnostics = Vec::new();
    let mut pos = 0;
    while pos < stripped.len() {
        // 找下一个 '{'
        let Some(brace_open) = stripped[pos..].find('{') else {
            break;
        };
        let sel_start = pos;
        let sel_raw = stripped[pos..pos + brace_open].trim();
        let after_open = pos + brace_open + 1;
        let Some(brace_close_rel) = stripped[after_open..].find('}') else {
            break;
        };
        let body = &stripped[after_open..after_open + brace_close_rel];
        pos = after_open + brace_close_rel + 1;

        if sel_raw.is_empty() {
            continue;
        }
        // <style> 内无精确 per-token span —— 定位用选择器起点近似。
        let loc = line_map.source_location(sel_start, "<style>".to_string());
        let Some(selector) = parse_selector(sel_raw) else {
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::FenceBadCssValue,
                format!("unsupported selector \"{}\" in <style>", sel_raw),
                loc,
            ));
            continue;
        };
        let declarations = parse_declarations(body, &loc, &mut diagnostics);
        if !declarations.is_empty() {
            rules.push(DynamicRule {
                selector,
                declarations,
            });
        }
    }
    (rules, diagnostics)
}

/// 剥除 CSS 注释 `/* ... */`。UTF-8 安全：在 `&str` 上用 `find`（ASCII 针的偏移恒为 char 边界）。
/// 不能逐字节 `u8 as char`——会损坏非 ASCII（CJK font-family、content 文本）。
fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            None => {
                rest = "";
                break;
            } // 未闭合注释 → 丢到末尾
        }
    }
    out.push_str(rest);
    out
}

/// 解析声明块体 → Vec<Declaration>。prop 名校验同 css_resolve（find_css_prop/find_shorthand）。
/// `loc` = 本规则块的近似 SourceLocation（diagnostic 定位用）。
fn parse_declarations(
    body: &str,
    loc: &SourceLocation,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Declaration> {
    let mut decls = Vec::new();
    for raw_decl in body.split(';') {
        let raw_decl = raw_decl.trim();
        if raw_decl.is_empty() {
            continue;
        }
        let Some((prop, value)) = raw_decl.split_once(':') else {
            continue;
        };
        let prop = prop.trim();
        let value = value.trim();
        if prop.is_empty() || value.is_empty() {
            continue;
        }
        if find_css_prop(prop).is_none() && find_shorthand(prop).is_none() {
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::FenceUnknownCssProp,
                format!("CSS property \"{}\" is not in the fence", prop),
                loc.clone(),
            ));
            continue;
        }
        decls.push(Declaration {
            prop: prop.to_string(),
            value: value.to_string(),
        });
    }
    decls
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(raw: &str) -> ParsedSelector {
        parse_selector(raw).unwrap_or_else(|| panic!("parse_selector({raw:?}) 返回 None"))
    }

    #[test]
    fn class_selector() {
        let s = spec(".foo");
        assert_eq!(s.compound.len(), 1);
        assert_eq!(s.compound[0].classes, vec!["foo".to_string()]);
        // specificity (id, class, tag) = (0,1,0)
        assert_eq!(s.specificity.0, 0);
        assert_eq!(s.specificity.1, 1);
        assert_eq!(s.specificity.2, 0);
    }

    #[test]
    fn tag_selector() {
        let s = spec("div");
        assert_eq!(s.compound[0].tag.as_deref(), Some("div"));
        assert_eq!(
            s.specificity,
            loomgui_core::style::dynamic::Specificity(0, 0, 1)
        );
    }

    #[test]
    fn id_selector() {
        let s = spec("#bar");
        assert_eq!(s.compound[0].id.as_deref(), Some("bar"));
        assert_eq!(s.specificity.1, 0);
        assert_eq!(s.specificity.0, 1);
    }

    #[test]
    fn compound_class_tag_id() {
        // div.foo#bar → (id=1, class=1, tag=1)
        let s = spec("div.foo#bar");
        assert_eq!(s.compound[0].tag.as_deref(), Some("div"));
        assert_eq!(s.compound[0].classes, vec!["foo".to_string()]);
        assert_eq!(s.compound[0].id.as_deref(), Some("bar"));
        assert_eq!(
            s.specificity,
            loomgui_core::style::dynamic::Specificity(1, 1, 1)
        );
    }

    #[test]
    fn descendant_combinator() {
        // .a .b → 两个 compound，后者 combinator = Descendant
        let s = spec(".a .b");
        assert_eq!(s.compound.len(), 2);
        assert_eq!(s.compound[1].combinator, Combinator::Descendant);
        assert_eq!(s.specificity.1, 2); // 两个 class
    }

    #[test]
    fn pseudo_class_sets_flag_and_specificity() {
        let s = spec(".btn:hover");
        assert!(s.compound[0].pseudo_hover);
        // 伪类算 class 级 specificity → (0, 2, 0)
        assert_eq!(s.specificity.1, 2);
    }

    #[test]
    fn out_of_subset_returns_none() {
        // 属性选择器、逗号、+ ~ 组合子都不在本子集
        assert!(parse_selector(r#"[type="text"]"#).is_none());
        assert!(parse_selector(".a, .b").is_none());
        assert!(parse_selector(".a > .b").is_none()); // Child 组合子本轮不做（仅后代空格）
        assert!(parse_selector(".a + .b").is_none());
        assert!(parse_selector(":nth-child(2)").is_none());
    }

    use loomgui_core::style::dynamic::Declaration;

    #[test]
    fn parse_style_block_basic() {
        let css = ".foo { color: red; font-size: 24px }\ndiv.bar { width: 100px }";
        let (rules, diags) = parse_style_block(css);
        assert!(diags.is_empty(), "diags: {diags:?}");
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].selector.raw, ".foo");
        assert_eq!(rules[0].declarations.len(), 2);
        assert_eq!(
            rules[0].declarations[0],
            Declaration {
                prop: "color".into(),
                value: "red".into()
            }
        );
        assert_eq!(rules[0].declarations[1].prop, "font-size");
        assert_eq!(rules[1].selector.raw, "div.bar");
        assert_eq!(rules[1].declarations[0].prop, "width");
    }

    #[test]
    fn parse_style_block_skips_unparseable_selector() {
        // .a > .b 越界 → 该规则进 diagnostic，其他规则照常
        let (rules, diags) = parse_style_block(".a > .b { color: red }\n.ok { color: blue }");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selector.raw, ".ok");
        assert!(
            diags.iter().any(|d| d.message.contains(".a > .b")),
            "越界选择器应报错: {diags:?}"
        );
    }

    #[test]
    fn parse_style_block_ignores_comments() {
        let (rules, _diags) = parse_style_block("/* c */ .x { color: red } /* tail */");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selector.raw, ".x");
    }
}
