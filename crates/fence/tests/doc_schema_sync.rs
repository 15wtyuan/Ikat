//! 文档 ↔ schema 交叉校验门（防描述层漂移）。
//!
//! 围栏的单一真相源是 `crates/fence/src/schema/` 下的 Rust const 注册表。
//! `docs/design/fence.md` 是人类可读副本。历史上 schema 改了文档没跟上
//! （标签下线后 `AGENTS.md` 仍写「31 标签」、`main-design.md` 仍列已删标签），
//! 因为既有防漂移门（`schema_contract.rs`）只锁 schema 自身，描述层完全不在测试覆盖下。
//!
//! 本测试从 `fence.md` 主表（§2.1 壳标签表 + §2.2 运行时标签表）解析出标签清单，
//! 与 `TAGS` / `SHELL_TAGS` 注册表比对，不一致即 fail。这样「改 schema 必同步 fence.md」
//! 有了可执行保证。

use loomgui_fence::schema::tag::{SHELL_TAGS, TAGS};

/// 从 `fence.md` 解析指定主表的标签清单。
///
/// `header_pred` 在每个 `|` 开头的行上求值，首个满足的行被当作表头；随后跳过分隔行
/// （`|---|`），收集每个数据行首个单元格里的反引号标签名，直到遇到非表格行（表结束）。
fn parse_table_tags<F>(md: &str, header_pred: F) -> Vec<String>
where
    F: Fn(&str) -> bool,
{
    let md = md.strip_prefix('\u{feff}').unwrap_or(md);
    let mut tags = Vec::new();
    let mut in_table = false;
    for raw in md.lines() {
        let line = raw.trim_end_matches('\r');
        if !in_table {
            if line.starts_with('|') && header_pred(line) {
                in_table = true;
            }
            continue;
        }
        // 表格内的行必须以 `|` 开头；否则表已结束。
        if !line.starts_with('|') {
            break;
        }
        // 跳过分隔行（`|---|---|`、对齐冒号变体）。
        if line.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ')) {
            continue;
        }
        // 首个单元格 = 第一个与第二个 `|` 之间。split('|') 首元素是空前导空串。
        let mut cells = line.split('|');
        let _ = cells.next(); // 前导空串
        let Some(first_cell) = cells.next() else {
            continue;
        };
        if let Some(tag) = first_cell
            .trim()
            .strip_prefix('`')
            .and_then(|s| s.strip_suffix('`'))
            .map(|s| s.to_string())
        {
            tags.push(tag);
        }
    }
    tags
}

fn sorted(v: Vec<String>) -> Vec<String> {
    let mut v = v;
    v.sort();
    v
}

/// §2.2 运行时标签主表必须与 `TAGS` 注册表一致。
#[test]
fn fence_md_runtime_tags_match_schema() {
    let md = include_str!("../../../docs/design/fence.md");
    // §2.2 表头唯一标识：同时含「标签」和「SemanticKind」（§2.3 是 role 列、§3.1 是签名列）。
    let parsed = parse_table_tags(md, |h| h.contains("标签") && h.contains("SemanticKind"));
    let schema: Vec<String> = TAGS.iter().map(|t| t.name.to_string()).collect();

    assert_eq!(
        sorted(parsed.clone()),
        sorted(schema.clone()),
        "fence.md §2.2 运行时标签表与 TAGS 注册表不一致（文档漂移）"
    );
    assert!(
        !parsed.is_empty(),
        "未从 fence.md §2.2 解析到任何标签——表头锚点或表格结构可能已变"
    );
}

/// §2.1 壳标签主表必须与 `SHELL_TAGS` 注册表一致。
#[test]
fn fence_md_shell_tags_match_schema() {
    let md = include_str!("../../../docs/design/fence.md");
    // §2.1 表头唯一标识：同时含「标签」和「用途」。
    let parsed = parse_table_tags(md, |h| h.contains("标签") && h.contains("用途"));
    let schema: Vec<String> = SHELL_TAGS.iter().map(|s| s.to_string()).collect();

    assert_eq!(
        sorted(parsed.clone()),
        sorted(schema.clone()),
        "fence.md §2.1 壳标签表与 SHELL_TAGS 注册表不一致（文档漂移）"
    );
    assert!(
        !parsed.is_empty(),
        "未从 fence.md §2.1 解析到任何标签——表头锚点或表格结构可能已变"
    );
}

/// 关键 CSS 属性必须在 fence.md 提及（防「schema 新增非标准/私有属性，文档没跟上」）。
///
/// 非全量 CSS_PROPS 覆盖——标准 CSS longhand（padding/margin/border 四向）靠作者/AI 的
/// CSS 先验，fence.md 用合并写法 `padding-top/right/bottom/left` 表达即可；本门只锁
/// 「LoomGUI 特有 / 易漂移 / 漏了会坑 AI」的关键属性（resize noop、动画、文本控件私有、
/// 九宫格、filter/transform/box-shadow 等非直觉项）。新增此类属性时须同步 fence.md。
#[test]
fn fence_md_covers_critical_css_props() {
    let md = include_str!("../../../docs/design/fence.md");
    const CRITICAL: &[&str] = &[
        "resize",
        "animation",
        "transition",
        "box-shadow",
        "filter",
        "transform",
        "overflow",
        "overflow-x",
        "overflow-y",
        "flex-wrap",
        "background-clip",
        "-webkit-background-clip",
        "background-repeat",
        "-webkit-text-stroke",
        "font-effect",
        "caret-color",
        "placeholder-color",
        "selection-background",
        "selection-color",
        "border-image-slice",
        "aspect-ratio",
        "pointer-events",
        "z-index",
        "animation-name",
        "animation-duration",
        "animation-timing-function",
        "animation-delay",
        "animation-iteration-count",
        "animation-direction",
        "animation-fill-mode",
        "animation-play-state",
    ];
    for &name in CRITICAL {
        let independent = format!("`{}`", name);
        assert!(
            md.contains(&independent),
            "关键 CSS 属性 `{}` 未在 fence.md 提及（文档漂移）",
            name
        );
    }
}

/// 具体全局属性（`is_global_attr` 白名单里的非通配项）必须在 fence.md 提及。
///
/// 通配前缀（`aria-*` / `data-*` / `--*`）靠先验，不在机械检查范围；只锁具体名（含 `type`，
/// 它在 input[type] 结构分派退役后变普通全局属性，易漏文档）。
#[test]
fn fence_md_covers_global_attrs() {
    let md = include_str!("../../../docs/design/fence.md");
    const CONCRETE_GLOBAL: &[&str] = &[
        "id", "class", "style", "slot", "hidden", "tabindex", "role", "type",
    ];
    for &name in CONCRETE_GLOBAL {
        let independent = format!("`{}`", name);
        assert!(
            md.contains(&independent),
            "全局属性 `{}` 未在 fence.md 提及（文档漂移）",
            name
        );
    }
}
