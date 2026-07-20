//! `<style>` 选择器解析 + 规则表产物（fence = 纯解析器）。
//!
//! 路径 c：手搓解析器，直产 core 的 ParsedSelector/Compound（fence 已依赖 core）。
//! 子集：class / tag / id / 后代组合（空格）/ 伪类（hover/active/disabled/focus/checked）
//! / 属性选择器（[attr] / [attr="val"]，仅 Exists + Eq）。
//! 越界（nth-child、+ ~ 组合子等）返 None，由调用方报错。
//!
//! @keyframes at-rule（对齐 public-api.md §9「动画定义全在 CSS」终态）：fence 解析
//! `@keyframes <name> { <stop-selector> { decls } ... }` 产 `KeyframesRule`。runtime
//! 驱动（tween 发射）在 §4 视觉束实现（v1.10）；本轮 fence 接受语法 + bridge 静默丢弃——
//! pkg.bin 格式不变（runtime 收不到 keyframes；animation 属性走 cascade 静默跳过）。
use crate::diagnostic::{Diagnostic, DiagnosticCode, LineMap, SourceLocation};
use crate::schema::css::{find_css_prop, find_shorthand};
use loomgui_core::style::dynamic::{
    AttrOp, AttrSelector, Combinator, Compound, Declaration, DynamicRule, ParsedSelector,
    Specificity,
};

// ── @keyframes 类型（fence-local；pkg.bin 暂不序列化）──────────────────────────

/// `@keyframes` 一条 stop 的选择器位置。CSS 标准：`from`=`0%`，`to`=`100%`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyframeStopSelector {
    From,
    To,
    /// 0..=100，CSS 允许小数百分比但本围栏子集只接受整数（showcase 用法覆盖）。
    Percent(u8),
}

/// `@keyframes` 内一条 stop：选择器位置 + 声明块（如 `from { opacity:0 }`）。
#[derive(Debug, Clone, PartialEq)]
pub struct KeyframeStop {
    pub selector: KeyframeStopSelector,
    pub declarations: Vec<Declaration>,
}

/// `@keyframes <name> { ... }` 整体规则。stops 按 source 顺序保留（runtime 按需插值）。
#[derive(Debug, Clone, PartialEq)]
pub struct KeyframesRule {
    pub name: String,
    pub stops: Vec<KeyframeStop>,
}

/// 解析单条选择器串 → ParsedSelector（含 specificity）。越界返 None。
///
/// 子集：空格分隔的若干 compound（后代组合）；每个 compound =
/// tag? + (class/id/pseudo/attr)*。
/// 越界：Child `>`、相邻 `+`/`~`、逗号多选（逗号在 parse_style_block 预切分）→ None。
pub fn parse_selector(raw: &str) -> Option<ParsedSelector> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    // 越界字符快速判定：逗号 / > + ~ 组合子不在本子集（属性选择器 `[...]` 已支持，见 parse_compound）。
    if raw.contains(',') || raw.contains('>') || raw.contains('+') || raw.contains('~') {
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
        } else if let Some(r) = rest.strip_prefix('[') {
            // 属性选择器：[attr] / [attr="val"] / [attr=val]。仅 Eq + Exists；高阶运算符
            // (^= ~= $= *= |=) 不在围栏子集 → 返 None 让 parse_style_block 报错。
            let close = r.find(']')?;
            let inner = r[..close].trim();
            let after = &r[close + 1..];
            let (name, op, value) = match inner.find('=') {
                Some(eq_pos) => {
                    let name_part = inner[..eq_pos].trim();
                    // 高阶属性运算符的修饰字符紧贴 = 前 → 围栏外，直接拒绝。
                    if name_part.ends_with(['~', '^', '$', '*', '|']) {
                        return None;
                    }
                    if name_part.is_empty() {
                        return None;
                    }
                    let val = inner[eq_pos + 1..]
                        .trim()
                        .trim_matches('"')
                        .trim_matches('\'');
                    (
                        name_part.to_ascii_lowercase(),
                        AttrOp::Eq,
                        Some(val.to_string()),
                    )
                }
                None => {
                    if inner.is_empty() {
                        return None;
                    }
                    (inner.to_ascii_lowercase(), AttrOp::Exists, None)
                }
            };
            c.attrs.push(AttrSelector { name, op, value });
            b += 1; // 属性选择器算 class 级
            rest = after;
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

/// 解析一段 `<style>` 文本 → (动态规则表, keyframes 规则表, 诊断)。
///
/// 文法（子集）：
/// - 普通规则：`selector { decl_list }`，selector 为单选择器（不支持逗号）。
/// - At-rule：`@keyframes <name> { <stop>{decls} ... }`（嵌套大括号）→ KeyframesRule。
///   其他 `@xxx` at-rule 不在围栏子集，整块丢弃 + 诊断。
/// - `decl_list` = `prop: value;` 重复。CSS 注释 `/* ... */` 剥除。
/// - 越界 selector / at-rule → 丢弃 + 诊断；声明 prop 名不在 schema（find_css_prop/find_shorthand）
///   → 诊断（与 css_resolve 一致）。
///
/// @keyframes 解析后产出 KeyframesRule；打包器 bridge 当前**静默丢弃**（pkg.bin 格式与
/// runtime §4 视觉束 v1.10 一同落地，本轮不序列化以避免 pkg 版本 bump + dll 重编）。
pub fn parse_style_block(css: &str) -> (Vec<DynamicRule>, Vec<KeyframesRule>, Vec<Diagnostic>) {
    let stripped = strip_comments(css);
    // 诊断定位用（粗略）：strip_comments 后 offset 已不对应原文，但行号近似可用。
    let line_map = LineMap::new(&stripped);
    let mut rules = Vec::new();
    let mut keyframes = Vec::new();
    let mut diagnostics = Vec::new();
    let mut pos = 0;
    while pos < stripped.len() {
        // 找下一个 '{'
        let Some(brace_open_rel) = stripped[pos..].find('{') else {
            break;
        };
        let brace_open = pos + brace_open_rel;
        let prelude = stripped[pos..brace_open].trim();
        let after_open = brace_open + 1;
        let sel_start = pos;

        // At-rule 分支：prelude 以 `@` 开头
        if prelude.starts_with('@') {
            let loc = line_map.source_location(sel_start, "<style>".to_string());
            // 找匹配的 `}`（@keyframes 体含嵌套大括号，必须按深度匹配）
            let Some((body, end_pos)) = find_matching_brace(&stripped, after_open) else {
                break;
            };
            pos = end_pos;
            let at_body = body;
            // 拆 @keyword + 后续 token（如 `@keyframes charge` → `keyframes` + `charge`）
            let at_kw_str = prelude.trim_start_matches('@').trim_start();
            let (at_name, at_rest) = split_at_keyword(at_kw_str);
            match at_name.as_str() {
                "keyframes" => match parse_keyframes_rule(&at_rest, at_body, &loc) {
                    Ok(kf) => keyframes.push(kf),
                    Err(d) => diagnostics.push(d),
                },
                _ => diagnostics.push(Diagnostic::error(
                    DiagnosticCode::FenceBadCssValue,
                    format!("unsupported at-rule @{at_name} in <style>"),
                    loc,
                )),
            }
            continue;
        }

        // 普通选择器分支：用首 `}`（无嵌套）取 body
        let Some(brace_close_rel) = stripped[after_open..].find('}') else {
            break;
        };
        let body = &stripped[after_open..after_open + brace_close_rel];
        pos = after_open + brace_close_rel + 1;

        if prelude.is_empty() {
            continue;
        }
        // <style> 内无精确 per-token span —— 定位用选择器起点近似。
        let loc = line_map.source_location(sel_start, "<style>".to_string());
        // 声明块只解析一次，逗号 selector list 的每段共享同一 declarations（clone）。
        let declarations = parse_declarations(body, &loc, &mut diagnostics);
        if declarations.is_empty() {
            continue;
        }
        // 逗号 selector list：`a, b, c { decls }` → 每段独立 parse_selector，共享声明块。
        // parse_selector 自身仍拒逗号（越界），由这里先 split，每段不再含逗号。
        for sel_raw in prelude.split(',') {
            let sel_raw = sel_raw.trim();
            if sel_raw.is_empty() {
                continue;
            }
            match parse_selector(sel_raw) {
                Some(selector) => rules.push(DynamicRule {
                    selector,
                    declarations: declarations.clone(),
                }),
                None => diagnostics.push(Diagnostic::error(
                    DiagnosticCode::FenceBadCssValue,
                    format!("unsupported selector \"{sel_raw}\" in <style>"),
                    loc.clone(),
                )),
            }
        }
    }
    (rules, keyframes, diagnostics)
}

/// 在 `s[start..]` 中找与（已消费的）`{` 匹配的 `}`。返回 (body_slice, position_after_close)。
///
/// 用于 @keyframes 这类含嵌套大括号的 at-rule body：朴素 `find('}')` 会停在第一个内层 `}`，
/// 切错 body。按深度计数：`{` +1 / `}` -1，归 0 即匹配。
fn find_matching_brace(s: &str, start: usize) -> Option<(&str, usize)> {
    let bytes = s.as_bytes();
    let mut depth: i32 = 1;
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&s[start..i], i + 1));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// 把 `@<keyword>` 后的首标识符分出来，剩下作为 prelude 余部（trim 后）。
/// `keyframes charge` → (`keyframes`, `charge`)；`media screen` → (`media`, `screen`)。
fn split_at_keyword(s: &str) -> (String, String) {
    let s = s.trim();
    let end = s.find(|ch: char| ch.is_whitespace()).unwrap_or(s.len());
    (s[..end].to_string(), s[end..].trim().to_string())
}

/// 解析 `@keyframes <name> { <body> }` 的 body → KeyframesRule。
///
/// body 文法：`<stop-selector-list> { decl_list }` 重复，stop-selector-list = 逗号分隔的
/// `from` / `to` / `<N>%`。逗号多 stop（`0%,100%{...}`）展开为多个 KeyframeStop 共享同声明块。
/// 任一 stop-selector 非法 → 整个 @keyframes 块丢弃 + 诊断（CSS 严格失败模式）。
fn parse_keyframes_rule(
    name: &str,
    body: &str,
    loc: &SourceLocation,
) -> Result<KeyframesRule, Diagnostic> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Diagnostic::error(
            DiagnosticCode::FenceBadCssValue,
            "@keyframes 缺少 name",
            loc.clone(),
        ));
    }
    let mut stops: Vec<KeyframeStop> = Vec::new();
    let mut pos = 0;
    while pos < body.len() {
        let Some(brace_open_rel) = body[pos..].find('{') else {
            break;
        };
        let brace_open = pos + brace_open_rel;
        let stop_sel_raw = body[pos..brace_open].trim();
        let after_open = brace_open + 1;
        let Some((inner, end_pos)) = find_matching_brace(body, after_open) else {
            break;
        };
        pos = end_pos;
        if stop_sel_raw.is_empty() {
            continue;
        }
        // 逗号多 stop：`0%,100%` → 展开为 [Percent(0), Percent(100)]，每 stop 共享同 declarations
        let mut sel_parsed: Vec<KeyframeStopSelector> = Vec::new();
        for raw in stop_sel_raw.split(',') {
            let s = parse_stop_selector(raw.trim(), loc)?;
            sel_parsed.push(s);
        }
        let decls = parse_declarations(inner, loc, &mut Vec::new()); // stops 内 prop 名错误 tolerable
        for sel in sel_parsed {
            stops.push(KeyframeStop {
                selector: sel,
                declarations: decls.clone(),
            });
        }
    }
    if stops.is_empty() {
        return Err(Diagnostic::error(
            DiagnosticCode::FenceBadCssValue,
            format!("@keyframes {name} 缺少 stop（from/to/N% 块）"),
            loc.clone(),
        ));
    }
    Ok(KeyframesRule {
        name: name.to_string(),
        stops,
    })
}

/// 解析单个 stop 选择器：`from` / `to` / `<N>%`（0..=100 整数）。
fn parse_stop_selector(
    raw: &str,
    loc: &SourceLocation,
) -> Result<KeyframeStopSelector, Diagnostic> {
    match raw {
        "from" => Ok(KeyframeStopSelector::From),
        "to" => Ok(KeyframeStopSelector::To),
        _ => {
            let Some(num_str) = raw.strip_suffix('%') else {
                return Err(Diagnostic::error(
                    DiagnosticCode::FenceBadCssValue,
                    format!("@keyframes stop \"{}\" 不合法（应为 from / to / N%）", raw),
                    loc.clone(),
                ));
            };
            let pct: u32 = num_str.parse().map_err(|_| {
                Diagnostic::error(
                    DiagnosticCode::FenceBadCssValue,
                    format!("@keyframes stop \"{}\" 百分比非数字", raw),
                    loc.clone(),
                )
            })?;
            if pct > 100 {
                return Err(Diagnostic::error(
                    DiagnosticCode::FenceBadCssValue,
                    format!("@keyframes stop \"{}\" 超过 100%", raw),
                    loc.clone(),
                ));
            }
            Ok(KeyframeStopSelector::Percent(pct as u8))
        }
    }
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
        // 属性选择器现已支持（[attr]/[attr="val"]）；逗号在 parse_style_block 预切分，
        // parse_selector 自身仍拒；> + ~ 组合子、nth-child 仍越界。
        assert!(parse_selector(r#"[type="text"]"#).is_some());
        assert!(parse_selector(".a, .b").is_none());
        assert!(parse_selector(".a > .b").is_none()); // Child 组合子本轮不做（仅后代空格）
        assert!(parse_selector(".a + .b").is_none());
        assert!(parse_selector(":nth-child(2)").is_none());
    }

    #[test]
    fn parse_attr_selector_eq() {
        let s = parse_selector(r#"input[type="text"]"#).unwrap();
        assert_eq!(s.compound[0].tag.as_deref(), Some("input"));
        assert_eq!(s.compound[0].attrs.len(), 1);
        let a = &s.compound[0].attrs[0];
        assert_eq!(a.name, "type");
        assert_eq!(a.op, AttrOp::Eq);
        assert_eq!(a.value.as_deref(), Some("text"));
        // 属性选择器算 class 级 specificity → (id=0, class+attr=1, tag=1)
        assert_eq!(s.specificity, Specificity(0, 1, 1));
    }

    #[test]
    fn parse_attr_selector_unquoted_and_exists() {
        assert_eq!(
            parse_selector(r#"input[type=password]"#).unwrap().compound[0].attrs[0]
                .value
                .as_deref(),
            Some("password")
        );
        // [attr] 存在形式
        let s = parse_selector(r#"[disabled]"#).unwrap();
        assert_eq!(s.compound[0].attrs[0].op, AttrOp::Exists);
        assert!(s.compound[0].attrs[0].value.is_none());
    }

    use loomgui_core::style::dynamic::Declaration;

    #[test]
    fn parse_style_block_basic() {
        let css = ".foo { color: red; font-size: 24px }\ndiv.bar { width: 100px }";
        let (rules, _kf, diags) = parse_style_block(css);
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
        let (rules, _kf, diags) = parse_style_block(".a > .b { color: red }\n.ok { color: blue }");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selector.raw, ".ok");
        assert!(
            diags.iter().any(|d| d.message.contains(".a > .b")),
            "越界选择器应报错: {diags:?}"
        );
    }

    #[test]
    fn parse_comma_selector_list_expands_to_shared_declarations() {
        // 逗号 selector list：`a, b, c { decls }` → 3 条 DynamicRule 共享同一声明块。
        // 用纯 tag 选择器隔离逗号展开机制本身（属性选择器 [type="..."] 是另一个 task）。
        let (rules, _, diags) = parse_style_block("input, select, textarea { color: red }");
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(rules.len(), 3, "逗号 list 展开为 3 条规则");
        assert_eq!(rules[0].declarations, rules[1].declarations);
        assert_eq!(rules[1].declarations, rules[2].declarations);
    }

    #[test]
    fn parse_style_block_ignores_comments() {
        let (rules, _kf, _diags) = parse_style_block("/* c */ .x { color: red } /* tail */");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selector.raw, ".x");
    }

    #[test]
    fn parse_style_block_preserves_cjk_and_strips_comment() {
        // UTF-8 safety: 注释 + CJK font-family 都不能被 strip_comments 损坏
        // （旧的 bytes[i] as char 字节循环会破坏多字节序列）。
        let (rules, _kf, diags) = parse_style_block("/* 注释 */ .x { font-family: \"微软雅黑\" }");
        assert!(diags.is_empty(), "diags: {diags:?}");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selector.raw, ".x");
        assert_eq!(rules[0].declarations.len(), 1);
        assert_eq!(rules[0].declarations[0].prop, "font-family");
        assert_eq!(rules[0].declarations[0].value, "\"微软雅黑\"");
    }

    // ── @keyframes at-rule 解析测 ──

    #[test]
    fn parse_style_block_keyframes_from_to() {
        // character.html 用法
        let css = "@keyframes charge { from{filter:brightness(.7)} to{filter:brightness(1)} }";
        let (_rules, keyframes, diags) = parse_style_block(css);
        assert!(diags.is_empty(), "diags: {diags:?}");
        assert_eq!(keyframes.len(), 1, "应解析出 1 个 @keyframes");
        let kf = &keyframes[0];
        assert_eq!(kf.name, "charge");
        assert_eq!(kf.stops.len(), 2, "from + to → 2 stops");
        assert_eq!(kf.stops[0].selector, KeyframeStopSelector::From);
        assert_eq!(kf.stops[1].selector, KeyframeStopSelector::To);
        assert_eq!(kf.stops[0].declarations.len(), 1);
        assert_eq!(kf.stops[0].declarations[0].prop, "filter");
    }

    #[test]
    fn parse_style_block_keyframes_multi_percent_stops() {
        // 多 stop 百分比（home/lab 类）
        let css = "@keyframes fade { 0%{opacity:0} 50%{opacity:.5} 100%{opacity:1} }";
        let (_rules, keyframes, diags) = parse_style_block(css);
        assert!(diags.is_empty(), "diags: {diags:?}");
        assert_eq!(keyframes.len(), 1);
        let kf = &keyframes[0];
        assert_eq!(kf.name, "fade");
        assert_eq!(kf.stops.len(), 3);
        assert_eq!(kf.stops[0].selector, KeyframeStopSelector::Percent(0));
        assert_eq!(kf.stops[1].selector, KeyframeStopSelector::Percent(50));
        assert_eq!(kf.stops[2].selector, KeyframeStopSelector::Percent(100));
    }

    #[test]
    fn parse_style_block_keyframes_comma_stop_selector_expands() {
        // mail.html 用法：`0%,100%{opacity:1} 50%{opacity:.4}` → 展开 3 stops
        let css = "@keyframes breathe { 0%,100%{opacity:1} 50%{opacity:.4} }";
        let (_rules, keyframes, diags) = parse_style_block(css);
        assert!(diags.is_empty(), "diags: {diags:?}");
        assert_eq!(keyframes.len(), 1);
        let kf = &keyframes[0];
        assert_eq!(kf.name, "breathe");
        assert_eq!(kf.stops.len(), 3, "0%,100% 展开为 2 + 1 = 3 stops");
        // 按 source 顺序：Percent(0), Percent(100), Percent(50)
        assert_eq!(kf.stops[0].selector, KeyframeStopSelector::Percent(0));
        assert_eq!(kf.stops[1].selector, KeyframeStopSelector::Percent(100));
        assert_eq!(kf.stops[2].selector, KeyframeStopSelector::Percent(50));
        // 0% 与 100% 共享同 declarations（来自同一块）
        assert_eq!(kf.stops[0].declarations, kf.stops[1].declarations);
    }

    #[test]
    fn parse_style_block_keyframes_with_other_rules_interleaved() {
        // home.html 用法：@keyframes 块 + 后续 selector 规则混合
        let css = "@keyframes fadeIn { from{opacity:0} to{opacity:1} }\n.nav-card { color:red }";
        let (rules, keyframes, diags) = parse_style_block(css);
        assert!(diags.is_empty(), "diags: {diags:?}");
        assert_eq!(keyframes.len(), 1, "@keyframes 解析");
        assert_eq!(keyframes[0].name, "fadeIn");
        assert_eq!(rules.len(), 1, "普通 selector 规则照常解析");
        assert_eq!(rules[0].selector.raw, ".nav-card");
    }

    #[test]
    fn parse_style_block_keyframes_single_to_stop() {
        // lab.html 用法：只有 to stop（CSS 合法：from 隐式 = 当前状态）
        let css = "@keyframes shimmer { to { background-position:200% center; } }";
        let (_rules, keyframes, diags) = parse_style_block(css);
        assert!(diags.is_empty(), "diags: {diags:?}");
        assert_eq!(keyframes.len(), 1);
        assert_eq!(keyframes[0].stops.len(), 1);
        assert_eq!(keyframes[0].stops[0].selector, KeyframeStopSelector::To);
    }

    #[test]
    fn parse_style_block_unknown_at_rule_errors() {
        // @media / @font-face 不在围栏子集 → diagnostic
        let (_rules, _kf, diags) = parse_style_block("@media screen { .x { color:red } }");
        assert!(
            diags.iter().any(|d| d.message.contains("@media")),
            "未知 at-rule 应报错: {diags:?}"
        );
    }

    #[test]
    fn parse_style_block_keyframes_missing_name_errors() {
        let (_rules, _kf, diags) = parse_style_block("@keyframes { from{opacity:0} }");
        assert!(!diags.is_empty(), "无名 @keyframes 应报错");
    }

    #[test]
    fn parse_style_block_keyframes_over_100_pct_errors() {
        let (_rules, _kf, diags) = parse_style_block("@keyframes x { 150%{opacity:0} }");
        assert!(!diags.is_empty(), "百分比 > 100 应报错");
    }
}
