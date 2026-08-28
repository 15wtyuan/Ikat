//! rect-diff semanticTag 配对表单源导出（#98）。
//!
//! `showcase/scripts/rect-diff/` 的 browser-rect.mjs（role→tag）与
//! normalize-dump-scene.mjs（kind→tag）此前各手抄一份 core `kind_to_html_tag`
//! 的对照表，且无测试钉住——新增 NodeKind/role 时静默失配，rect-diff 只表现成
//! idless-unpaired 噪音上涨。本测试从 Rust 真相源（core `kind_to_html_tag` ×
//! fence `ROLE_TO_SEMANTIC` × bridge `semantic_to_kind`）生成
//! `semantic-tags.json`，与入库文件逐字节比对；漂移即红，跑
//! `RECTDIFF_TAG_MAP_REGEN=1 cargo test -p ikat_pkg --test rectdiff_tag_map`
//! 重出。脚本侧改读该 JSON，手抄表删除。

use ikat_core::dump::{kind_to_html_tag, ALL_NODE_KINDS};
use ikat_core::scene::node::NodeKind;
use ikat_fence::schema::tag::ROLE_TO_SEMANTIC;

use ikat_pkg::bridge::semantic_to_kind;

fn tag_map_json() -> String {
    // kind 段：NodeKind 名 → tag（ALL_NODE_KINDS 与 dump.rs 单测互锁，见其注释）。
    let mut kind_lines: Vec<String> = ALL_NODE_KINDS
        .iter()
        .map(|k| format!("    \"{:?}\": \"{}\"", k, kind_to_html_tag(*k)))
        .collect();
    kind_lines.sort();
    // role 段：role → tag = ROLE_TO_SEMANTIC → NodeKind → kind_to_html_tag 单源链。
    // textbox 不在 ROLE_TO_SEMANTIC（aria-multiline 分流 TextArea/TextField，扁平表
    // 表达不了，resolve_semantic 内联处理）——这里补 input 基线，multiline 分流留在
    // browser-rect.mjs（浏览器侧能看到 aria-multiline 属性）。
    let mut role_lines: Vec<String> = ROLE_TO_SEMANTIC
        .iter()
        .map(|(role, sem)| {
            format!(
                "    \"{}\": \"{}\"",
                role,
                kind_to_html_tag(semantic_to_kind(*sem))
            )
        })
        .collect();
    role_lines.push("    \"textbox\": \"input\"".to_string());
    role_lines.sort();
    format!(
        "{{\n  \"kind\": {{\n{}\n  }},\n  \"role\": {{\n{}\n  }}\n}}\n",
        kind_lines.join(",\n"),
        role_lines.join(",\n")
    )
}

#[test]
fn rectdiff_tag_map_fresh() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../showcase/scripts/rect-diff/semantic-tags.json"
    );
    let generated = tag_map_json();
    if std::env::var("RECTDIFF_TAG_MAP_REGEN").is_ok() {
        std::fs::write(path, &generated).expect("write semantic-tags.json");
        return;
    }
    let checked_in = std::fs::read_to_string(path).unwrap_or_else(|e| {
        panic!(
            "read semantic-tags.json ({path}): {e}——入库文件缺失，用 RECTDIFF_TAG_MAP_REGEN=1 重出"
        )
    });
    assert_eq!(
        checked_in, generated,
        "semantic-tags.json 与 Rust 真相源漂移（新增/改动 NodeKind 或 role 映射？）——\
         跑 RECTDIFF_TAG_MAP_REGEN=1 cargo test -p ikat_pkg --test rectdiff_tag_map 重出"
    );
}

/// NodeKind Debug 名与 normalize-dump-scene.mjs 消费的 kind 字符串一致（dump_scene_json
/// 的 kind 字段即 Debug 名）——锁住「导出名 = dump 输出名」这条隐含契约。
#[test]
fn node_kind_debug_names_are_stable() {
    let names: Vec<String> = ALL_NODE_KINDS.iter().map(|k| format!("{:?}", k)).collect();
    for expected in [
        "Container",
        "TextNode",
        "TextElement",
        "Button",
        "Image",
        "TextField",
        "NumberField",
        "Slider",
        "Toggle",
        "RadioButton",
        "TextArea",
        "Dropdown",
        "OptionItem",
        "ProgressBar",
        "ListView",
        "ListItem",
        "Slot",
        "CustomElement",
        "Template",
        "TabList",
        "Tab",
        "Link",
    ] {
        assert!(names.contains(&expected.to_string()), "缺 {expected}");
    }
    let _: Option<NodeKind> = None; // 引 NodeKind，防 unused import 当表改空
}
