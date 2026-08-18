//! 打包器模板（workspace-agent.md / skill SKILL.md）↔ fence schema 交叉校验。
//!
//! 模板是给外部工作区 agent 的围栏规则副本，历史上曾整段漂移（虚构 30 标签
//! 世界、CSS 支持清单乌龙）。真相源是 `crates/fence/src/schema/` 的 const 表；
//! 本测试把模板中的标签/role/CSS 清单拉回与 schema 对账，改 schema 必须同步
//! 模板，否则这里红。

use loomgui_fence::schema::{find_css_prop, find_shorthand, ROLE_TO_SEMANTIC, SHELL_TAGS, TAGS};

const AGENT_MD: &str = include_str!("../templates/workspace-agent.md");
const SKILL_MD: &str = include_str!("../templates/skill/SKILL.md");

/// 提取锚点区之间的文本（begin/end 均为含 `fence-sync:` 的注释锚点）。
fn section_between(md: &'static str, begin: &str, end: &str) -> &'static str {
    let b = md
        .find(begin)
        .unwrap_or_else(|| panic!("anchor `{begin}` missing from template"));
    let e = md
        .find(end)
        .unwrap_or_else(|| panic!("anchor `{end}` missing from template"));
    assert!(b < e, "anchor order wrong: {begin} must precede {end}");
    &md[b..e]
}

/// 收集列表行行首的反引号 token（`- \`prop\` — ...` 形式）。
fn list_line_tokens(section: &str) -> Vec<String> {
    section
        .lines()
        .filter_map(|line| {
            let rest = line.trim_start().strip_prefix("- `")?;
            let token = rest.split('`').next()?;
            Some(token.to_string())
        })
        .collect()
}

#[test]
fn runtime_and_shell_tag_lists_match_schema() {
    let runtime = TAGS
        .iter()
        .map(|t| format!("`{}`", t.name))
        .collect::<Vec<_>>()
        .join(", ");
    assert_eq!(
        runtime,
        "`div`, `span`, `button`, `img`, `template`, `slot`"
    );
    for md in [AGENT_MD, SKILL_MD] {
        assert!(
            md.contains(&runtime),
            "template must list the runtime tags verbatim: {runtime}"
        );
    }

    let shell = SHELL_TAGS
        .iter()
        .map(|t| format!("`{}`", t))
        .collect::<Vec<_>>()
        .join(", ");
    assert_eq!(
        shell,
        "`html`, `head`, `body`, `title`, `meta`, `style`, `link`, `script`"
    );
    for md in [AGENT_MD, SKILL_MD] {
        assert!(
            md.contains(&shell),
            "template must list the shell tags verbatim: {shell}"
        );
    }
}

#[test]
fn every_registered_role_appears_in_templates() {
    // `textbox` 由 resolve_semantic 内联分派（aria-multiline 二分），不在表内。
    let mut roles: Vec<&str> = ROLE_TO_SEMANTIC.iter().map(|(r, _)| *r).collect();
    roles.push("textbox");
    assert_eq!(roles.len(), 13, "role universe changed — update this test");
    for role in &roles {
        for (name, md) in [("workspace-agent.md", AGENT_MD), ("SKILL.md", SKILL_MD)] {
            assert!(
                md.contains(&format!("`{role}`")),
                "{name} missing registered role `{role}`"
            );
        }
    }
}

#[test]
fn supported_css_tokens_are_in_whitelist() {
    let section = section_between(
        SKILL_MD,
        "fence-sync:css-supported-begin",
        "fence-sync:css-supported-end",
    );
    let tokens = list_line_tokens(section);
    assert!(
        tokens.len() >= 30,
        "supported-css anchor section looks too small ({} tokens) — anchors misplaced?",
        tokens.len()
    );
    for token in &tokens {
        assert!(
            find_css_prop(token).is_some() || find_shorthand(token).is_some(),
            "SKILL.md lists `{token}` as supported, but it is in neither \
             CSS_PROPS nor CSS_SHORTHANDS — fix the doc or the schema drifted"
        );
    }
}

#[test]
fn not_supported_css_tokens_are_absent_from_whitelist() {
    let section = section_between(
        SKILL_MD,
        "fence-sync:css-not-supported-begin",
        "fence-sync:css-not-supported-end",
    );
    let tokens = list_line_tokens(section);
    assert!(
        tokens.len() >= 10,
        "not-supported-css anchor section looks too small ({} tokens)",
        tokens.len()
    );
    for token in &tokens {
        assert!(
            find_css_prop(token).is_none() && find_shorthand(token).is_none(),
            "SKILL.md claims `{token}` is unsupported, but the fence schema \
             DOES register it — the doc is stale, update it"
        );
    }
}
