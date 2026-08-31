//! tick 管线顺序门：`Stage::tick_and_render` 的步骤序列是全框架的时序契约
//! （写入顺序 = 动画优先级、rematch 须在 solve 前、solve 须在 collect_heights 前……），
//! 长期只靠注释维护、文档清单与实际脱节。本测试从 stage.rs 源码提取实际调用序列，
//! 与下方登记清单逐项比对——插入、删除、换序任何一步都会红。
//!
//! 有意改管线时：同步更新 EXPECTED_TICK_STEPS（并核对 main-design.md §16 的描述
//! 仍与清单定性一致）。测试与 stage.rs 经 include_str! 锁同一份源码，无路径漂移。

const STAGE_SRC: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/stage.rs"));

/// 登记的 tick 步骤序列（canonical 名 → 源码识别子串，按序比对）。
/// 识别子串须足够特异（含括号或路径前缀），不与注释/局部变量撞。
const EXPECTED_TICK_STEPS: &[(&str, &str)] = &[
    (
        "ffi_events_drain",
        "std::mem::take(&mut self.pending_events)",
    ),
    ("tweens_update", "self.tweens.update"),
    ("players_update", "crate::scene::animation::update_all"),
    (
        "cursor_blink",
        "crate::scene::control::advance_cursor_blink",
    ),
    ("focus_request", "crate::input::focus_node"),
    ("pointer_process", "self.pointer_state.process"),
    ("wheel_apply", "crate::scroll::apply_wheel_to_hit"),
    ("scroll_advance", "crate::scroll::advance_all"),
    ("keys_process", "crate::input::process_keys"),
    ("text_input", "crate::input::process_text_input"),
    ("list_plan_visible", "crate::list::plan_visible"),
    ("list_execute_visible", "crate::list::execute_visible"),
    ("rematch", "rematch_pseudo_classes("),
    ("transition_kill", "self.tweens.kill"),
    ("transition_tween", "self.tweens.tween"),
    ("sync_animation_players", "sync_animation_players("),
    (
        "sync_control_visuals",
        "crate::scene::control::sync_control_visuals",
    ),
    ("solve", "solve("),
    (
        "measure_text_controls",
        "crate::scene::control::measure_text_controls",
    ),
    ("collect_heights", "crate::list::collect_heights"),
    (
        "refresh_content_sizes",
        "crate::scroll::refresh_content_sizes",
    ),
    (
        "compute_world_transforms",
        "crate::scene::transform::compute_world_transforms",
    ),
    // A2 增量后 stage 调 build_render_nodes_cached（输入指纹缓存入口；名字前缀仍含
    // build_render_nodes，语义步骤不变——识别子串不带 "(" 以兼容两入口名）。
    ("build_render_nodes", "build_render_nodes"),
];

/// 从 stage.rs 提取 tick_and_render 函数体：起于签名行、止于下一个方法定义
/// （4 空格缩进的 `pub fn`，rustfmt 稳定形态）。
fn tick_body() -> String {
    let start = STAGE_SRC
        .find("pub fn tick_and_render")
        .expect("stage.rs 须含 tick_and_render");
    let rest = &STAGE_SRC[start..];
    let end = rest.find("\n    pub fn ").unwrap_or(rest.len());
    rest[..end].to_string()
}

/// 逐行提取步骤：剥掉行注释后按登记表顺序找识别子串，一行至多记一步
/// （两步挤一行会让测试红，逼拆行——保证每步位置可考）。
fn extract_steps(body: &str) -> Vec<&'static str> {
    let mut steps = Vec::new();
    for raw in body.lines() {
        let code = raw.split("//").next().unwrap_or("").trim();
        if code.is_empty() {
            continue;
        }
        for &(name, pat) in EXPECTED_TICK_STEPS {
            if code.contains(pat) {
                steps.push(name);
                break;
            }
        }
    }
    steps
}

#[test]
fn tick_step_order_matches_registry() {
    let body = tick_body();
    let actual = extract_steps(&body);
    let expected: Vec<&str> = EXPECTED_TICK_STEPS.iter().map(|(n, _)| *n).collect();
    assert_eq!(
        actual, expected,
        "tick_and_render 步骤序列与登记清单不一致（换序/插入/删除都会在此报错）。\
         有意改管线：同步更新 EXPECTED_TICK_STEPS 并核对 main-design.md §16。"
    );
}

/// 已登记的每一步在函数体内恰好出现一次——重复调用（如两处 solve）须显式
/// 登记为两个不同步骤名，不允许静默混入。
#[test]
fn tick_steps_each_registered_once() {
    let body = tick_body();
    for (name, pat) in EXPECTED_TICK_STEPS {
        let n = body
            .lines()
            .filter(|raw| {
                let code = raw.split("//").next().unwrap_or("");
                code.contains(pat)
            })
            .count();
        assert_eq!(
            n, 1,
            "步骤 {name}（识别 {pat:?}）在 tick_and_render 内出现 {n} 次，应为恰好 1 次"
        );
    }
}
