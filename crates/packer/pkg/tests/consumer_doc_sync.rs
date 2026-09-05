//! 消费侧文档 ↔ schema 交叉校验门（防对外文档漂移）。
//!
//! 真相源 = `crates/fence/src/schema/` 的 const 注册表；`fence.md`（内部权威）已有
//! fence crate 的 `doc_schema_sync` 门。本门把同一范式延伸到**随 scaffold 分发给
//! 消费侧 AI 的模板文档**（`crates/packer/pkg/templates/editor/references/`）——
//! 历史上 schema 改了消费文档没跟上（#93 cursor 批实锤：fence.md 写了、
//! css-reference.md 漏了），因为模板住在 templates/ 下不像文档、也不在任何测试
//! 覆盖里。
//!
//! 覆盖口径（诚实局限：锁**名字集合**，锁不住语义措辞——措辞漂移靠 AGENTS 的
//! 文档涟漪表）：
//! - fence-schema.md 标签表与 `TAGS`/`SHELL_TAGS` 双向精确集：新标签没写文档、
//!   文档还提着已删标签，都红。
//! - css-reference.md 必须以反引号形式提及每个 `CSS_PROPS`/`CSS_SHORTHANDS` 名
//!   （消费文档的属性名格式契约就是反引号，本门顺带锁住该契约）。

use yio_fence::schema::css::{CSS_PROPS, CSS_SHORTHANDS};
use yio_fence::schema::tag::{SHELL_TAGS, TAGS};

/// 解析 md 表格首个单元格里的反引号名（与 fence `doc_schema_sync` 同款口径的本地
/// 实现——跨 crate 测试共享不便，两边各自 40 行可接受）。
///
/// `header_pred` 在每个 `|` 开头的行上求值，首个满足的行被当作表头；随后跳过分隔行
/// （`|---|`），收集每个数据行首个单元格里的反引号名，直到遇到非表格行（表结束）。
fn table_first_cells<F>(md: &str, header_pred: F) -> Vec<String>
where
    F: Fn(&str) -> bool,
{
    let md = md.strip_prefix('\u{feff}').unwrap_or(md);
    let mut out = Vec::new();
    let mut in_table = false;
    for raw in md.lines() {
        let line = raw.trim_end_matches('\r');
        if !in_table {
            if line.starts_with('|') && header_pred(line) {
                in_table = true;
            }
            continue;
        }
        if !line.starts_with('|') {
            break;
        }
        if line.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ')) {
            continue;
        }
        let mut cells = line.split('|');
        let _ = cells.next(); // 前导空串
        if let Some(first) = cells.next() {
            if let Some(name) = first
                .trim()
                .strip_prefix('`')
                .and_then(|s| s.strip_suffix('`'))
            {
                out.push(name.to_string());
            }
        }
    }
    out
}

fn sorted_dedup(v: Vec<String>) -> Vec<String> {
    let mut v = v;
    v.sort();
    v.dedup();
    v
}

/// fence-schema.md 两张标签表与注册表双向精确集：
/// 主表（表头「Tag + Default display」）== `TAGS`；壳标签表（表头「Shell tag」）
/// == `SHELL_TAGS`。新标签没写文档、文档还提着已删标签，都红。
#[test]
fn fence_schema_md_tags_match_schema() {
    let md = include_str!("../templates/editor/references/fence-schema.md");
    let runtime_doc = sorted_dedup(table_first_cells(md, |h| {
        h.contains("Tag") && h.contains("Default display")
    }));
    let shell_doc = sorted_dedup(table_first_cells(md, |h| h.contains("Shell tag")));
    let runtime_schema = sorted_dedup(TAGS.iter().map(|t| t.name.to_string()).collect());
    let shell_schema = sorted_dedup(SHELL_TAGS.iter().map(|s| s.to_string()).collect());
    assert!(
        !runtime_doc.is_empty() && !shell_doc.is_empty(),
        "未从 fence-schema.md 解析到标签——表头锚点或表格结构可能已变"
    );
    for (doc, schema, table) in [
        (&runtime_doc, &runtime_schema, "runtime"),
        (&shell_doc, &shell_schema, "shell"),
    ] {
        let undocumented: Vec<&String> = schema.iter().filter(|t| !doc.contains(t)).collect();
        let stale: Vec<&String> = doc.iter().filter(|t| !schema.contains(t)).collect();
        assert!(
            undocumented.is_empty() && stale.is_empty(),
            "fence-schema.md {table} 标签表与 schema 注册表不一致（对外文档漂移）：\n  schema 有而文档缺: {undocumented:?}\n  文档有而 schema 无: {stale:?}"
        );
    }
}

/// css-reference.md 必须反引号提及每个 schema 属性/简写名。
#[test]
fn css_reference_md_covers_all_props() {
    let md = include_str!("../templates/editor/references/css-reference.md");
    let missing: Vec<&str> = CSS_PROPS
        .iter()
        .map(|p| p.name)
        .chain(CSS_SHORTHANDS.iter().map(|s| s.name))
        .filter(|n| !md.contains(&format!("`{n}`")))
        .collect();
    assert!(
        missing.is_empty(),
        "css-reference.md 未提及的 schema 属性（对外文档漂移，须补条目）：{missing:?}"
    );
}
