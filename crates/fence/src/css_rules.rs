//! `<style>` 选择器解析 + 规则表产物（fence = 纯解析器）。
//!
//! 路径 c：手搓解析器，直产 core 的 ParsedSelector/Compound（fence 已依赖 core）。
//! 子集：class / tag / id / 后代组合（空格）/ 伪类（hover/active/disabled/focus/checked）。
//! 越界（属性选择器、nth-child、+ ~ 组合子、逗号多选等）返 None，由调用方报错。
use loomgui_core::style::dynamic::{Combinator, Compound, ParsedSelector, Specificity};

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
}
