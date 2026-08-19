//! 打包器模板（workspace-agent.md / skill SKILL.md / loom SKILL.md）↔ 实现交叉校验。
//!
//! 模板是给外部工作区 agent 的围栏规则副本，历史上曾整段漂移（虚构 30 标签
//! 世界、CSS 支持清单乌龙）。真相源是 `crates/fence/src/schema/` 的 const 表；
//! 本测试把模板中的标签/role/CSS 清单拉回与 schema 对账，改 schema 必须同步
//! 模板，否则这里红。loom skill 的命令面/退出码与 CLI main.rs 对账（同一模式）。

use loomgui_fence::schema::{find_css_prop, find_shorthand, ROLE_TO_SEMANTIC, SHELL_TAGS, TAGS};

const AGENT_MD: &str = include_str!("../../../pkg/templates/workspace-agent.md");
const SKILL_MD: &str = include_str!("../../../pkg/templates/skill/SKILL.md");
const LOOM_SKILL_MD: &str = include_str!("../../../pkg/templates/loom-skill/SKILL.md");

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
    // 表外例外：textbox（resolve_semantic 内联分派）、tabpanel / dialog（纯容器
    // 语义，div tag 回退）。模板 role 表须覆盖全宇宙。
    let mut roles: Vec<&str> = ROLE_TO_SEMANTIC.iter().map(|(r, _)| *r).collect();
    roles.push("textbox");
    roles.push("tabpanel");
    roles.push("dialog");
    assert_eq!(roles.len(), 15, "role universe changed — update this test");
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

/// loom skill 命令面 ↔ CLI 实现对账：skill 教的每个子命令必须真实存在（main.rs
/// 分发表），退出码语义不得漂移。改命令面不同步 skill = agent 照手册撞墙。
#[test]
fn loom_skill_commands_match_cli_surface() {
    // 与 crates/packer/pkg/src/main.rs parse_cmd 的分发集一致。
    let commands = [
        "check",
        "build",
        "init",
        "new",
        "list",
        "show",
        "font add",
        "atlas add",
        "version",
    ];
    for cmd in &commands {
        assert!(
            LOOM_SKILL_MD.contains(&format!("loom {cmd}")),
            "loom skill must document `loom {cmd}` (CLI implements it)"
        );
    }
    // workspace-agent.md 的编排段同样教这些命令。
    for cmd in [
        "loom new",
        "loom font add",
        "loom atlas add",
        "loom list",
        "loom show",
        "loom check",
        "loom build",
    ] {
        assert!(
            AGENT_MD.contains(cmd),
            "workspace-agent.md must mention `{cmd}`"
        );
    }
    // 退出码三段语义：skill 的表与 CLI 的 exit_code 契约对齐。
    assert!(LOOM_SKILL_MD.contains("| 0 |"), "exit 0 row present");
    assert!(LOOM_SKILL_MD.contains("| 1 |"), "exit 1 row present");
    assert!(LOOM_SKILL_MD.contains("| 2 |"), "exit 2 row present");
    assert!(
        LOOM_SKILL_MD.contains("format_version"),
        "skill must document the JSON format_version contract"
    );
    // 机读约定：stdout 数据 / stderr 进度。
    assert!(
        LOOM_SKILL_MD.contains("stdout"),
        "skill must teach the stdout=data convention"
    );
}
