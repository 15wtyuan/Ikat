//! 运行时 CSS 自定义属性（`--*`）环境与 `var()` 解析（#11 运行时 CSS 与 custom props）。
//!
//! 消费模型（CSS computed-value 语义的围栏收窄）：
//! - 每节点一个 var 环境 = 父环境 + 本节点声明覆盖。声明源按优先级：样式表规则
//!   （rematch 匹配件里的 `--*` 声明）< 行内 style（base_style.deferred_inline 的
//!   `--*`）< 运行时 SetVar（Scene::node_vars）。
//! - 同节点多名声明的引用解析**不看声明顺序**：先按级联选每名胜出声明，再对胜出集
//!   递归解析 var 引用（`--a: var(--b); --b: red` 同节点合法，--a 解析为 red）。
//! - 环 = 该名 invalid（引用环 / 引用链断裂且无 fallback）：invalid 随继承传播；
//!   消费端 `var()` 命中 invalid → 用 fallback，无 fallback → 整条声明跳过
//!   （分层 fail-loud：跳过事件经 warn-once 进 `Scene::warnings`，不抛异常——
//!   主题系统不能变运行时炸弹）。
//! - `var()` 名字按字面匹配，`var(` 探针 ASCII 大小写不敏感（CSS 函数名大小写不
//!   敏感）；fallback 到配平右括号为止、可含逗号（`rgb(1,2,3)`）与嵌套 `var()`
//!   （递归解析）。

use std::collections::{HashMap, HashSet};

/// 解析后的 custom prop 值。`Invalid` = CSS guaranteed-invalid（环/链断）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VarVal {
    Val(String),
    Invalid,
}

/// 一个节点的 var 环境（名字含 `--` 前缀 → 解析后值）。
pub type VarEnv = HashMap<String, VarVal>;

/// 值里是否含 `var(`（ASCII 大小写不敏感，零分配字节扫描）。
/// fence `var_check::value_has_var` 的 core 侧同判定（gate + deferral 用）。
pub fn value_has_var(value: &str) -> bool {
    next_var_at(value, 0).is_some()
}

/// 从 `pos` 起找下一个 `var(` 起点（ASCII 大小写不敏感）。None = 无。
fn next_var_at(value: &str, pos: usize) -> Option<usize> {
    let b = value.as_bytes();
    let mut i = pos;
    while i + 3 < b.len() {
        if (b[i] | 0x20) == b'v'
            && (b[i + 1] | 0x20) == b'a'
            && (b[i + 2] | 0x20) == b'r'
            && b[i + 3] == b'('
        {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 从 `var(` 起点（`value[start..start+4]` 须是 var(）解析调用体。
/// 返回 (名字, fallback 原文, 右括号后位置)；括号不配平返 None（防御——fence 形状
/// 门应已拦，运行时坏数据按解析失败处理）。首层逗号切 name/fallback（嵌套括号内
/// 逗号不算——`var(--x, rgb(1, 2, 3))` 的 fallback 是整个 `rgb(1, 2, 3)`）。
fn parse_var_call(value: &str, start: usize) -> Option<(&str, Option<&str>, usize)> {
    let b = value.as_bytes();
    let mut depth = 1usize;
    let mut j = start + 4;
    let mut comma_at = None;
    while j < b.len() {
        match b[j] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    let name = value[start + 4..comma_at.unwrap_or(j)].trim();
                    let fallback = comma_at.map(|c| value[c + 1..j].trim());
                    return Some((name, fallback, j + 1));
                }
            }
            b',' if depth == 1 && comma_at.is_none() => comma_at = Some(j),
            _ => {}
        }
        j += 1;
    }
    None
}

/// 合并一层声明进父环境，产出子环境。`layer` 内后者覆盖前者（调用方按优先级序喂：
/// 规则声明 → 行内 → SetVar）。`on_invalid` 收解析失败（环/链断）消息——调用方
/// warn-once；每名字至多回调一次（memo）。
pub fn merge_env_layer(
    parent: &VarEnv,
    layer: &[(String, String)],
    on_invalid: &mut dyn FnMut(&str),
) -> VarEnv {
    if layer.is_empty() {
        return parent.clone();
    }
    // 胜出原始值：同名取 layer 最后一条。
    let mut raws: HashMap<&str, &str> = HashMap::with_capacity(layer.len());
    for (p, v) in layer {
        raws.insert(p.as_str(), v.as_str());
    }
    let mut resolver = Resolver {
        raws,
        parent,
        resolved: HashMap::with_capacity(layer.len()),
        visiting: HashSet::new(),
    };
    let names: Vec<String> = resolver.raws.keys().map(|k| k.to_string()).collect();
    let mut env = parent.clone();
    for name in names {
        if let Some(v) = resolver.resolve_name(&name, on_invalid) {
            env.insert(name, v);
        }
    }
    env
}

/// 对纯环境查表做 var() 替换（typed 声明消费端：`color: var(--accent)`）。
/// Some = 替换后值；None = 解析失败（目标缺失/invalid 且无 fallback）→ 声明跳过。
pub fn substitute_value(value: &str, env: &VarEnv) -> Option<String> {
    substitute_with(value, &mut |name| env.get(name).cloned())
}

/// var() 替换核心：`lookup` 返回名字的当前值（Val / Invalid / None=未定义）。
/// Val → 代入；Invalid/None → fallback（递归解析）→ 无 fallback 返 None。
fn substitute_with(value: &str, lookup: &mut dyn FnMut(&str) -> Option<VarVal>) -> Option<String> {
    if !value_has_var(value) {
        return Some(value.to_string());
    }
    let mut out = String::with_capacity(value.len());
    let mut i = 0usize;
    loop {
        let Some(var_at) = next_var_at(value, i) else {
            out.push_str(&value[i..]);
            return Some(out);
        };
        out.push_str(&value[i..var_at]);
        let (name, fallback, after) = parse_var_call(value, var_at)?; // 括号不配平 = 防御性失败
        let piece = match lookup(name) {
            Some(VarVal::Val(s)) => s,
            _ => substitute_with(fallback?, lookup)?,
        };
        out.push_str(&piece);
        i = after;
    }
}

/// 胜出集递归解析器。`raws` = 本层胜出原始值；引用先查本层（递归解析 + memo +
/// visiting 环检测），本层无此名再查父环境（父值已解析）。
struct Resolver<'a> {
    raws: HashMap<&'a str, &'a str>,
    parent: &'a VarEnv,
    resolved: HashMap<String, VarVal>,
    visiting: HashSet<String>,
}

impl Resolver<'_> {
    /// 名字 → 解析结果。None = 名字既不在本层也不在父环境（未定义）。
    /// Some(Invalid) = 定义了但坏（环/链断）——二者对消费端等价走 fallback，
    /// 但 invalid 会作为环境条目传播给子孙。
    fn resolve_name(&mut self, name: &str, on_invalid: &mut dyn FnMut(&str)) -> Option<VarVal> {
        if let Some(v) = self.resolved.get(name) {
            return Some(v.clone());
        }
        let raw = match self.raws.get(name) {
            Some(r) => *r,
            None => return self.parent.get(name).cloned(),
        };
        if !self.visiting.insert(name.to_string()) {
            on_invalid(&format!(
                "custom property cycle: var({name}) resolves back to itself — every property \
                 on the cycle is invalid"
            ));
            return Some(VarVal::Invalid);
        }
        let sub = self.resolve_value(raw, on_invalid);
        self.visiting.remove(name);
        let v = match sub {
            Some(s) => VarVal::Val(s),
            None => {
                on_invalid(&format!(
                    "custom property var({name}) is invalid (broken reference chain) — \
                     declarations using it fall back"
                ));
                VarVal::Invalid
            }
        };
        self.resolved.insert(name.to_string(), v.clone());
        Some(v)
    }

    /// raw 值内的 var() 替换（本层引用经 `resolve_name` 递归——mutual recursion，
    /// 环经 visiting 集截断）。
    fn resolve_value(&mut self, value: &str, on_invalid: &mut dyn FnMut(&str)) -> Option<String> {
        if !value_has_var(value) {
            return Some(value.to_string());
        }
        let mut out = String::with_capacity(value.len());
        let mut i = 0usize;
        loop {
            let Some(var_at) = next_var_at(value, i) else {
                out.push_str(&value[i..]);
                return Some(out);
            };
            out.push_str(&value[i..var_at]);
            let (name, fallback, after) = parse_var_call(value, var_at)?;
            let piece = match self.resolve_name(name, on_invalid) {
                Some(VarVal::Val(s)) => s,
                // fallback 里的 var 同在本层解析（CSS：fallback 是声明值的一部分）。
                _ => self.resolve_value(fallback?, on_invalid)?,
            };
            out.push_str(&piece);
            i = after;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> VarEnv {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), VarVal::Val(v.to_string())))
            .collect()
    }

    #[test]
    fn plain_value_passthrough() {
        assert_eq!(
            substitute_value("#fff", &VarEnv::new()),
            Some("#fff".to_string())
        );
        assert!(!value_has_var("#fff"));
        assert!(value_has_var("var(--x)"));
        assert!(value_has_var("1px solid VAR(--x)"));
        assert!(!value_has_var("vari(--x)")); // var 后必须紧跟 (
        assert!(!value_has_var("--x"));
    }

    #[test]
    fn substitute_basic_fallback_and_missing() {
        let e = env(&[("--accent", "#f00")]);
        assert_eq!(
            substitute_value("var(--accent)", &e),
            Some("#f00".to_string())
        );
        assert_eq!(
            substitute_value("1px solid var(--line, #888)", &e),
            Some("1px solid #888".to_string())
        );
        assert_eq!(substitute_value("var(--missing)", &e), None);
        assert_eq!(
            substitute_value("var(--accent) var(--accent)", &e),
            Some("#f00 #f00".to_string())
        );
    }

    #[test]
    fn invalid_val_consumes_fallback() {
        let mut e = VarEnv::new();
        e.insert("--bad".to_string(), VarVal::Invalid);
        assert_eq!(
            substitute_value("var(--bad, #123)", &e),
            Some("#123".to_string())
        );
        assert_eq!(substitute_value("var(--bad)", &e), None);
    }

    #[test]
    fn nested_fallback_resolves_vars() {
        let e = env(&[("--a", "#111"), ("--b", "#222")]);
        assert_eq!(
            substitute_value("var(--missing, var(--a))", &e),
            Some("#111".to_string())
        );
        assert_eq!(
            substitute_value("var(--missing, var(--missing2, var(--b)))", &e),
            Some("#222".to_string())
        );
    }

    #[test]
    fn merge_layer_overrides_and_inherits() {
        let parent = env(&[("--accent", "#f00"), ("--keep", "#0f0")]);
        let layer = vec![
            ("--accent".to_string(), "#00f".to_string()),
            ("--new".to_string(), "10px".to_string()),
        ];
        let mut warnings = Vec::new();
        let child = merge_env_layer(&parent, &layer, &mut |m| warnings.push(m.to_string()));
        assert_eq!(child.get("--accent"), Some(&VarVal::Val("#00f".into())));
        assert_eq!(child.get("--keep"), Some(&VarVal::Val("#0f0".into())));
        assert_eq!(child.get("--new"), Some(&VarVal::Val("10px".into())));
        assert!(warnings.is_empty());
    }

    #[test]
    fn merge_layer_same_node_reference_ignores_declaration_order() {
        // CSS：先选胜出再解析——--b 声明在 --a 之后（或之前）不影响 --a 看到 --b。
        let layer = vec![
            ("--a".to_string(), "var(--b)".to_string()),
            ("--b".to_string(), "red".to_string()),
        ];
        let mut warnings = Vec::new();
        let e = merge_env_layer(&VarEnv::new(), &layer, &mut |m| {
            warnings.push(m.to_string())
        });
        assert_eq!(e.get("--a"), Some(&VarVal::Val("red".into())));
        assert!(warnings.is_empty());
    }

    #[test]
    fn merge_layer_cycle_makes_both_invalid_with_warning() {
        let layer = vec![
            ("--a".to_string(), "var(--b)".to_string()),
            ("--b".to_string(), "var(--a)".to_string()),
        ];
        let mut warnings = Vec::new();
        let e = merge_env_layer(&VarEnv::new(), &layer, &mut |m| {
            warnings.push(m.to_string())
        });
        assert_eq!(e.get("--a"), Some(&VarVal::Invalid));
        assert_eq!(e.get("--b"), Some(&VarVal::Invalid));
        assert!(!warnings.is_empty());
        assert_eq!(
            substitute_value("var(--a, #abc)", &e),
            Some("#abc".to_string())
        );
    }

    #[test]
    fn merge_layer_later_same_name_wins() {
        let layer = vec![
            ("--x".to_string(), "1px".to_string()),
            ("--x".to_string(), "2px".to_string()),
        ];
        let mut warnings = Vec::new();
        let e = merge_env_layer(&VarEnv::new(), &layer, &mut |m| {
            warnings.push(m.to_string())
        });
        assert_eq!(e.get("--x"), Some(&VarVal::Val("2px".into())));
    }

    #[test]
    fn merge_layer_broken_chain_invalid() {
        // --a: var(--missing) → --a invalid（不是未定义——有声明但链断）
        let layer = vec![("--a".to_string(), "var(--missing)".to_string())];
        let mut warnings = Vec::new();
        let e = merge_env_layer(&VarEnv::new(), &layer, &mut |m| {
            warnings.push(m.to_string())
        });
        assert_eq!(e.get("--a"), Some(&VarVal::Invalid));
        assert!(!warnings.is_empty());
        assert_eq!(substitute_value("var(--a)", &e), None);
    }

    #[test]
    fn merge_layer_invalid_inherits_to_child() {
        // invalid 随继承传播：子环境里 --bad 仍是 invalid。
        let mut parent = VarEnv::new();
        parent.insert("--bad".to_string(), VarVal::Invalid);
        let mut warnings = Vec::new();
        let child = merge_env_layer(&parent, &[], &mut |m| warnings.push(m.to_string()));
        assert_eq!(substitute_value("var(--bad)", &child), None);
        assert_eq!(
            substitute_value("var(--bad, #fff)", &child),
            Some("#fff".to_string())
        );
    }

    #[test]
    fn merge_layer_empty_layer_returns_parent() {
        let parent = env(&[("--x", "1")]);
        let mut warnings = Vec::new();
        let child = merge_env_layer(&parent, &[], &mut |m| warnings.push(m.to_string()));
        assert_eq!(child, parent);
    }
}
