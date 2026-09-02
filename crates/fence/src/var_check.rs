//! CSS 自定义属性（`--*`）与 `var()` 的围栏面（#11 运行时 CSS 与 custom props）。
//!
//! 语义总纲（fence.md「自定义属性与 var()」节为本模块的文档镜像）：
//! - `--x: <declaration-value>` 是合法声明（样式表规则 + 行内 style 都收），值近乎自由
//!   （CSS 规范行为：custom prop 值不做关键字校验，坏值在 var() 消费端暴露为 invalid）。
//! - `var(--x)` / `var(--x, fallback)` 可出现在任意属性的值位；**含 var() 的值打包期不做
//!   字面校验**（终值运行时才定），只做本模块的形状校验（括号配平、名字合法）。
//! - 消费在运行时：rematch 在 var 环境（祖先链 custom props + SetVar 覆盖）解析替换。
//!   环/缺失目标且无 fallback = 该声明 invalid 跳过（分层 fail-loud：打包期同表静态环
//!   发 warning，运行时静默回退 + warn-once 日志）。
//!
//! 本模块是形状与环检测的单一真相源：`<style>` 块（css_rules）、行内 style（css_resolve）
//! 与运行时注入解析（css_rules::parse_runtime_css）共用。

/// prop 名是否为 CSS 自定义属性（`--` 前缀 + 至少一个合法名字符）。
/// `--` 本身（空前缀无名字）不算——CSS 要求 `--` 后有 ident。
pub fn is_custom_prop(prop: &str) -> bool {
    match prop.strip_prefix("--") {
        Some(name) => !name.is_empty() && name.chars().all(is_ident_char),
        None => false,
    }
}

/// custom prop 名字字符集（CSS ident 的围栏收窄：字母/数字/`-`/`_`）。
fn is_ident_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '-' || ch == '_'
}

/// 值里是否含 `var(`（大小写不敏感——CSS 函数名 ASCII 大小写不敏感）。
/// core 运行时用同判定做 deferral 门（含 var 的值不打包烘焙、延后到环境解析）。
pub fn value_has_var(value: &str) -> bool {
    let lowered = value.to_ascii_lowercase();
    lowered.contains("var(")
}

/// var() 形状校验：值中每个 `var(...)` 都必须括号配平、名字是合法 custom prop 名。
/// 返回 Some(错误消息) = FenceBadCssValue；None = 形状合法（或值里没有 var）。
///
/// 只查形状不查语义——`var(--missing)`（目标不存在）合法：运行时 SetVar 可注入，
/// 打包期无法预判。fallback 里可再嵌 var()（运行时递归解析），形状同样只查配平。
pub fn var_shape_error(value: &str) -> Option<String> {
    if !value_has_var(value) {
        return None;
    }
    // 扫描所有 var( 出现点（大小写不敏感），逐个取配平括号内容校验。
    let bytes = value.as_bytes();
    let mut i = 0usize;
    while i + 4 <= bytes.len() {
        if &value[i..i + 4].to_ascii_lowercase() == "var(" {
            // 找配平的 `)`（嵌套括号：fallback 里可含 rgb() 等）
            let mut depth = 1usize;
            let mut j = i + 4;
            let mut closed = None;
            while j < bytes.len() {
                match bytes[j] {
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            closed = Some(j);
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            let Some(close) = closed else {
                return Some(format!(
                    "value \"{value}\" has an unbalanced var() — expected `var(--name)` or \
                     `var(--name, fallback)`"
                ));
            };
            let inner = &value[i + 4..close];
            let name = inner.split(',').next().unwrap_or("").trim();
            if !is_custom_prop(name) {
                return Some(format!(
                    "value \"{value}\": var() name \"{name}\" is not a custom property — \
                     custom property names start with `--` (e.g. var(--accent))"
                ));
            }
            i = close + 1;
        } else {
            i += 1;
        }
    }
    None
}

/// 同一声明集内的 custom prop 引用环检测（打包期 warning 用）。
///
/// 输入 = 一批 `(prop, value)` 声明（一个 `<style>` 块的全部规则，或一个元素的行内
/// 声明）。对每个引用图 `--a → --b`（value 含 `var(--b)` 且 `--b` 在本批声明过）找环，
/// 每个环出一条 warning 消息（列环上属性名）。跨批/跨源的环查不到（运行时 SetVar
/// 可动态拼出）——运行时解析另有 visiting 集兜底（invalid 回退）。
///
/// 注意保守性：不同选择器命中的声明可能落在不同节点上、运行时根本不成环——
/// warning 非阻断，误报可忽略；真环几乎必是作者 bug，静默会让「为什么这条声明
/// 没生效」无从查起。
pub fn custom_prop_cycle_warnings<'a>(
    decls: impl Iterator<Item = (&'a str, &'a str)>,
) -> Vec<String> {
    use std::collections::{HashMap, HashSet};
    // 名字 → 引用的目标名集合（只记本批声明过的目标）。
    let mut declared: HashSet<&str> = HashSet::new();
    let mut raw: Vec<(&str, &str)> = Vec::new();
    for (prop, value) in decls {
        if is_custom_prop(prop) {
            declared.insert(prop);
            raw.push((prop, value));
        }
    }
    if declared.is_empty() {
        return Vec::new();
    }
    let mut graph: HashMap<&str, Vec<&str>> = HashMap::new();
    for (prop, value) in &raw {
        for target in var_refs(value) {
            // 声明侧键是 &str，引用侧 String——按内容查找回 &str 键（目标必在 declared 集）。
            if let Some(key) = declared.get(target.as_str()) {
                graph.entry(prop).or_default().push(key);
            }
        }
    }
    // DFS 找环（每个起点，visiting 栈含起点）；环去重（每环只报一次，按最小名旋转）。
    let mut reported: HashSet<Vec<&str>> = HashSet::new();
    let mut out = Vec::new();
    for start in graph.keys().copied().collect::<Vec<_>>() {
        let mut stack: Vec<&str> = vec![start];
        let mut visited: std::collections::HashSet<&str> = std::collections::HashSet::new();
        // 迭代 DFS（环 = 从 start 回到 start 的路径）。
        while let Some(&cur) = stack.last() {
            let mut descended = false;
            if let Some(nexts) = graph.get(cur) {
                for &n in nexts {
                    if n == start {
                        // 找到环：栈即路径。规范成最小起点旋转去重。
                        let mut cycle = stack.clone();
                        let min_idx = cycle
                            .iter()
                            .enumerate()
                            .min_by_key(|(_, s)| **s)
                            .map(|(i, _)| i)
                            .unwrap_or(0);
                        cycle.rotate_left(min_idx);
                        if reported.insert(cycle.clone()) {
                            out.push(format!(
                                "custom property cycle {} — every property on the cycle is \
                                 invalid at runtime (declarations using them fall back); break \
                                 the cycle",
                                cycle
                                    .iter()
                                    .map(|n| format!("var({n})"))
                                    .collect::<Vec<_>>()
                                    .join(" -> ")
                            ));
                        }
                    } else if visited.insert(n) {
                        stack.push(n);
                        descended = true;
                        break;
                    }
                }
            }
            if !descended {
                stack.pop();
            }
        }
    }
    out
}

/// 取值里全部 `var(--x)` 引用的目标名（大小写不敏感找 `var(`，名字按原样返回）。
pub fn var_refs(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = value.as_bytes();
    let mut i = 0usize;
    while i + 4 <= bytes.len() {
        if &value[i..i + 4].to_ascii_lowercase() == "var(" {
            let mut depth = 1usize;
            let mut j = i + 4;
            let mut closed = None;
            while j < bytes.len() {
                match bytes[j] {
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            closed = Some(j);
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            let Some(close) = closed else { break };
            let name = value[i + 4..close].split(',').next().unwrap_or("").trim();
            if is_custom_prop(name) {
                out.push(name.to_string());
            }
            i = close + 1;
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_prop_identification() {
        assert!(is_custom_prop("--accent"));
        assert!(is_custom_prop("--a-b_c1"));
        assert!(!is_custom_prop("--")); // 空名字不算
        assert!(!is_custom_prop("-single"));
        assert!(!is_custom_prop("color"));
        assert!(!is_custom_prop("--bad name")); // 空格非法
        assert!(!is_custom_prop("--bad.name")); // 点非法（CSS ident 转义围栏不收）
    }

    #[test]
    fn var_shape_valid_and_invalid() {
        // 合法形
        assert!(var_shape_error("#ff0000").is_none());
        assert!(var_shape_error("var(--accent)").is_none());
        assert!(var_shape_error("VAR(--Accent)").is_none()); // 大小写不敏感
        assert!(var_shape_error("1px solid var(--line, #888)").is_none());
        assert!(var_shape_error("var(--a, var(--b, #fff))").is_none()); // 嵌套 fallback
        assert!(var_shape_error("var(--a) var(--b)").is_none());
        // 非法形
        assert!(var_shape_error("var(accent)").is_some()); // 名字缺 -- 前缀
        assert!(var_shape_error("var(--").is_some()); // 未闭合
        assert!(var_shape_error("var()").is_some()); // 空名
        assert!(var_shape_error("var(--a, rgb(1,2,3").is_some()); // fallback 括号不配平
    }

    #[test]
    fn var_refs_extraction() {
        assert_eq!(var_refs("var(--a)"), vec!["--a".to_string()]);
        assert_eq!(
            var_refs("1px solid var(--line, #888) var(--x)"),
            vec!["--line".to_string(), "--x".to_string()]
        );
        assert!(var_refs("#fff").is_empty());
        // 名字不合法（缺 --）不进引用集
        assert!(var_refs("var(bad)").is_empty());
    }

    #[test]
    fn cycle_detection_finds_and_dedups() {
        let decls = [
            ("--a", "var(--b)"),
            ("--b", "var(--a)"),
            ("--c", "var(--c)"),       // 自环
            ("--d", "var(--missing)"), // 目标未声明：无环
            ("color", "var(--a)"),     // 非 custom prop 声明：忽略
        ];
        let warns = custom_prop_cycle_warnings(decls.iter().map(|(p, v)| (*p, *v)));
        assert_eq!(warns.len(), 2, "{warns:?}"); // a↔b 一环 + c 自环
        assert!(warns.iter().any(|w| w.contains("--a") && w.contains("--b")));
        assert!(warns.iter().any(|w| w.contains("--c")));
    }

    #[test]
    fn cycle_detection_empty_when_no_customs() {
        let decls = [("color", "#fff"), ("width", "10px")];
        assert!(custom_prop_cycle_warnings(decls.iter().map(|(p, v)| (*p, *v))).is_empty());
    }
}
