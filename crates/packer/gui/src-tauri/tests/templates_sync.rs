//! 打包器模板（三个 skill + editor references）↔ 实现交叉校验。
//!
//! 模板是给外部工作区 agent 的围栏规则副本，历史上曾整段漂移（虚构 30 标签
//! 世界、CSS 支持清单乌龙）。真相源是 `crates/fence/src/schema/` 的 const 表；
//! 本测试把模板中的标签/role/CSS 清单拉回与 schema 对账，改 schema 必须同步
//! 模板，否则这里红。loom skill 的命令面/退出码与 CLI main.rs 对账（同一模式）。

use loomgui_fence::control_structure_check::{CheckSpec, REQUIRED_CHILDREN};
use loomgui_fence::schema::{find_css_prop, find_shorthand, ROLE_TO_SEMANTIC, SHELL_TAGS, TAGS};
use loomgui_fence::value_check::TRANSITION_PROPS;

const EDITOR_SKILL_MD: &str = include_str!("../../../pkg/templates/editor/SKILL.md");
const EDITOR_SCHEMA_MD: &str =
    include_str!("../../../pkg/templates/editor/references/fence-schema.md");
const EDITOR_CSS_MD: &str =
    include_str!("../../../pkg/templates/editor/references/css-reference.md");
const RUNTIME_SKILL_MD: &str = include_str!("../../../pkg/templates/runtime/SKILL.md");
const RUNTIME_API_MD: &str =
    include_str!("../../../pkg/templates/runtime/references/api-reference.md");
const LOOM_SKILL_MD: &str = include_str!("../../../pkg/templates/loom/SKILL.md");

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
        "`div`, `span`, `button`, `img`, `a`, `template`, `slot`"
    );
    for (name, md) in [
        ("editor SKILL.md", EDITOR_SKILL_MD),
        ("fence-schema.md", EDITOR_SCHEMA_MD),
    ] {
        assert!(
            md.contains(&runtime),
            "{name} must list the runtime tags verbatim: {runtime}"
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
    assert!(
        EDITOR_SCHEMA_MD.contains(&shell),
        "fence-schema.md must list the shell tags verbatim: {shell}"
    );
}

#[test]
fn every_registered_role_appears_in_templates() {
    // 表外例外：textbox（resolve_semantic 内联分派）、tabpanel / dialog（纯容器
    // 语义，div tag 回退）。role 表须覆盖全宇宙。
    let mut roles: Vec<&str> = ROLE_TO_SEMANTIC.iter().map(|(r, _)| *r).collect();
    roles.push("textbox");
    roles.push("tabpanel");
    roles.push("dialog");
    assert_eq!(roles.len(), 15, "role universe changed — update this test");
    for role in &roles {
        assert!(
            EDITOR_SCHEMA_MD.contains(&format!("`{role}`")),
            "fence-schema.md missing registered role `{role}`"
        );
    }
}

#[test]
fn supported_css_tokens_are_in_whitelist() {
    let section = section_between(
        EDITOR_CSS_MD,
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
            "css-reference.md lists `{token}` as supported, but it is in neither \
             CSS_PROPS nor CSS_SHORTHANDS — fix the doc or the schema drifted"
        );
    }
}

#[test]
fn not_supported_css_tokens_are_absent_from_whitelist() {
    let section = section_between(
        EDITOR_CSS_MD,
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
            "css-reference.md claims `{token}` is unsupported, but the fence schema \
             DOES register it — the doc is stale, update it"
        );
    }
}

/// transition 通道集 ↔ fence `TRANSITION_PROPS` 双向对账（#90 P0-1：#10 把通道
/// 扩到 8 个后分发文档仍写 4 个——作者信文档就不会去试文档说没有的能力，
/// 静默损失比「没写」更危险）。
#[test]
fn transition_channels_match_engine_set() {
    let section = section_between(
        EDITOR_CSS_MD,
        "fence-sync:transition-channels-begin",
        "fence-sync:transition-channels-end",
    );
    let mut doc: Vec<String> = list_line_tokens(section);
    doc.sort();
    let mut engine: Vec<String> = TRANSITION_PROPS.iter().map(|s| s.to_string()).collect();
    engine.sort();
    assert_eq!(
        doc, engine,
        "css-reference.md transition channel list drifted from fence TRANSITION_PROPS \
         — missing channel = doc stale (update the doc); extra channel = engine set \
         changed (update both)"
    );
}

/// 控件必需子契约 ↔ fence-schema.md role registry 行对账（#90 P0-2：combobox 行
/// 漏写 `data-slot=value`，作者照抄文档的契约表直接 build 失败）。
#[test]
fn required_children_contracts_in_schema_doc() {
    for (role, specs) in REQUIRED_CHILDREN {
        let row_start = EDITOR_SCHEMA_MD
            .find(&format!("| `{role}` |"))
            .unwrap_or_else(|| panic!("fence-schema.md missing role registry row for `{role}`"));
        let row_end = EDITOR_SCHEMA_MD[row_start..]
            .find('\n')
            .map(|i| row_start + i)
            .unwrap_or(EDITOR_SCHEMA_MD.len());
        let row = &EDITOR_SCHEMA_MD[row_start..row_end];
        for spec in *specs {
            match spec {
                CheckSpec::Role(r) => assert!(
                    row.contains(&format!("role={r}")),
                    "fence-schema.md row for `{role}` must state its required `role={r}` \
                     child (fence enforces it: FenceMissingControlChild)"
                ),
                CheckSpec::Slot(s) => assert!(
                    row.contains(&format!("data-slot={s}")),
                    "fence-schema.md row for `{role}` must state its required \
                     `data-slot={s}` child (fence enforces it: FenceMissingControlChild)"
                ),
            }
        }
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
        "scaffold",
        "version",
    ];
    for cmd in &commands {
        assert!(
            LOOM_SKILL_MD.contains(&format!("loom {cmd}")),
            "loom skill must document `loom {cmd}` (CLI implements it)"
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

/// 三个 skill 的路由互指（Boundaries）：发现机制靠 description 匹配，skill 间
/// 边界靠互指文本兜底——缺了互指，agent 会用错手册（拿 runtime 手册改 HTML）。
#[test]
fn skills_cross_reference_each_other() {
    assert!(
        EDITOR_SKILL_MD.contains("loomgui-runtime"),
        "editor skill must route runtime work to the loomgui-runtime skill"
    );
    assert!(
        RUNTIME_SKILL_MD.contains("loomgui-editor"),
        "runtime skill must route authoring work to the loomgui-editor skill"
    );
    assert!(
        RUNTIME_SKILL_MD.contains(".loom/config.json"),
        "runtime skill must teach reading .loom/config.json"
    );
    assert!(
        EDITOR_SKILL_MD.contains(".loom/config.json"),
        "editor skill must teach reading .loom/config.json"
    );
    // editor 的 references/ 三件必须被主文件指名（渐进披露的入口）。
    for r in ["fence-schema.md", "css-reference.md", "patterns.md"] {
        assert!(
            EDITOR_SKILL_MD.contains(r),
            "editor SKILL.md must point at references/{r}"
        );
    }
    // runtime 的 API 查找表同理；且 skill 不得再把消费者指回 LoomGUI 源码仓库
    // （消费者装的是 Unity 包 + loom init 工作区，不应被迫 clone 源码翻文档）。
    assert!(
        RUNTIME_SKILL_MD.contains("api-reference.md"),
        "runtime SKILL.md must point at references/api-reference.md"
    );
    assert!(
        !RUNTIME_SKILL_MD.contains("docs/design/public-api.md"),
        "runtime SKILL.md must not route consumers to the repository's docs — \
         references/api-reference.md is the shipped contract copy"
    );
}

/// runtime API 查找表的控件 role 宇宙 ↔ fence schema 对账：新增 role 只改 schema
/// 不同步查找表 = 消费者 agent 在表里查不到新控件类型。
#[test]
fn runtime_api_reference_covers_role_universe() {
    let mut roles: Vec<&str> = ROLE_TO_SEMANTIC.iter().map(|(r, _)| *r).collect();
    roles.push("textbox");
    roles.push("tabpanel");
    roles.push("dialog");
    for role in &roles {
        assert!(
            RUNTIME_API_MD.contains(&format!("role={role}")),
            "api-reference.md missing control role `{role}`"
        );
    }
    // 三个异常/类型关键词抽查（防整段被误删后测试仍绿）。
    for needle in ["UIContractException", "ItemExitClass", "IsPointerOnUI"] {
        assert!(
            RUNTIME_API_MD.contains(needle),
            "api-reference.md missing key contract `{needle}`"
        );
    }
}
