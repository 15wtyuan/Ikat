//! `:nth-child(An+B | odd | even | N)` selector 解析。
//!
//! 覆盖：An+B 系数/常数提取、odd/even 简写、纯整数 N、空格容错、specificity
//! （伪类 = class 级 1 档）、越界形态拒绝、与其他简单选择器组合。

use ikat_core::style::dynamic::{NthChildExpr, Specificity};
use ikat_fence::css_rules::parse_selector;

/// 解析单 compound selector，取 `:nth-child` 表达式。selector 必须合法且带 nth-child。
fn nth(raw: &str) -> NthChildExpr {
    let sel = parse_selector(raw).unwrap_or_else(|| panic!("parse_selector({raw:?}) 返回 None"));
    sel.compound[0]
        .pseudo_nth_child
        .expect("selector 应带 :nth-child")
}

#[test]
fn nth_child_an_plus_b_parses_a_and_b() {
    assert_eq!(nth(".item:nth-child(2n+1)"), NthChildExpr { a: 2, b: 1 });
    // specificity：`.item`(class 1) + `:nth-child`(伪类 1) = class 级 2 档
    let sel = parse_selector(".item:nth-child(2n+1)").unwrap();
    assert_eq!(sel.specificity, Specificity(0, 2, 0));
}

#[test]
fn nth_child_odd_even_n_shorthands() {
    // odd = 2n+1；even = 2n；纯整数 N = 0n+N
    assert_eq!(nth(":nth-child(odd)"), NthChildExpr { a: 2, b: 1 });
    assert_eq!(nth(":nth-child(even)"), NthChildExpr { a: 2, b: 0 });
    assert_eq!(nth(":nth-child(3)"), NthChildExpr { a: 0, b: 3 });
    assert_eq!(nth(":nth-child(1)"), NthChildExpr { a: 0, b: 1 });
}

#[test]
fn nth_child_whitespace_and_bare_n() {
    // CSS 允许 An+B 内部空格（`2n + 1`）与负常数（`2n-1`）；裸 `n` = 1n+0
    assert_eq!(nth(":nth-child(2n + 1)"), NthChildExpr { a: 2, b: 1 });
    assert_eq!(nth(":nth-child(2n+ 1)"), NthChildExpr { a: 2, b: 1 });
    assert_eq!(nth(":nth-child(2n-1)"), NthChildExpr { a: 2, b: -1 });
    assert_eq!(nth(":nth-child(n)"), NthChildExpr { a: 1, b: 0 });
    assert_eq!(nth(":nth-child( 2n+1 )"), NthChildExpr { a: 2, b: 1 });
}

#[test]
fn nth_child_bad_args_rejected() {
    // 围栏外形态显式拒（防静默降级）：空参 / 缺符号 / 小数 / 未知关键字 / 缺 B 符号
    assert!(parse_selector(":nth-child()").is_none());
    assert!(parse_selector(":nth-child(2n1)").is_none());
    assert!(parse_selector(":nth-child(2.5)").is_none());
    assert!(parse_selector(":nth-child(foo)").is_none());
    assert!(parse_selector(":nth-child(2n+)").is_none());
    assert!(parse_selector(":nth-child(2n+1").is_none()); // 缺闭合括号
    assert!(parse_selector(":nth-child").is_none()); // 缺参数
    assert!(parse_selector(":nth-of-type(2)").is_none()); // 其他 nth-* 不在子集
}

#[test]
fn nth_child_composes_with_other_simple_selectors() {
    // :nth-child 后继续挂伪类/class 组合（compound 内多简单选择器）
    let sel = parse_selector(".card:nth-child(2):hover").unwrap();
    assert_eq!(
        sel.compound[0].pseudo_nth_child,
        Some(NthChildExpr { a: 0, b: 2 })
    );
    assert!(sel.compound[0].pseudo_hover);
    assert_eq!(sel.specificity, Specificity(0, 3, 0)); // class + nth-child + hover
}

#[test]
fn nth_child_with_descendant_combinator() {
    // 后代选择器里 nth-child 在前段/末段均可
    let sel = parse_selector(".list :nth-child(2n)").unwrap();
    assert_eq!(sel.compound.len(), 2);
    assert_eq!(
        sel.compound[1].pseudo_nth_child,
        Some(NthChildExpr { a: 2, b: 0 })
    );
    assert_eq!(sel.specificity, Specificity(0, 2, 0));
}
