//! Cascade-finalization probe: drive a hand-authored HTML fixture (kept inside
//! the current selector/property subset) end-to-end and lock the
//! HTML -> pkg -> Stage -> layout -> rect/visible chain.
//!
//! Asserts only what the Stage public API can observe today (rect / visible /
//! find_node_by_id). Inheritance (font-size/color), computed style, and
//! node-kind fidelity need query exits that cascade-finalization must add;
//! those are tracked as gaps, not covered here.
use loomgui_core::scene::node::NodeKind;
use loomgui_core::scene::NodeId;
use loomgui_core::stage::Stage;
use loomgui_pkg::build::pack_components;

const HTML: &str = include_str!("fixtures/cascade-probe.html");

/// Pack the fixture -> load -> instantiate under a stage root -> tick once.
/// `create_root` owns the scene + stage_root that `instantiate` mounts under;
/// without `append_child` the component is orphaned and never reaches layout
/// (solve walks scene roots only).
fn build_stage(html: &str) -> (Stage, NodeId) {
    let (bytes, _refs) =
        pack_components(&[("probe".to_string(), html.to_string())]).expect("pack_components");
    let mut stage = Stage::new((400.0, 300.0)).expect("Stage::new");
    // Text layout needs a default font at tick time; the fixture has Chinese
    // text, so register a CJK font. Embedded at compile time to keep the test
    // independent of the working directory.
    stage
        .register_font(
            "LXGWWenKai",
            include_bytes!("../../../core/tests/fixtures/LXGWWenKai.ttf").to_vec(),
            true,
        )
        .expect("register_font");
    stage.load_package("p", &bytes).expect("load_package");
    let stage_root = stage.create_root("div", "").expect("create_root");
    let comp_root = stage.instantiate("p", "probe").expect("instantiate");
    stage
        .append_child(stage_root, comp_root)
        .expect("append_child");
    stage.advance_time(0.0);
    stage.tick_and_render();
    (stage, comp_root)
}

#[test]
fn probe_root_width_from_id_rule() {
    // `#root { width: 320px }` is an id-selector rule resolved through cascade;
    // a correct width proves the rule table reached the engine and was applied.
    let (stage, _) = build_stage(HTML);
    let root = stage.find_node_by_id("root").expect("root");
    let r = stage.get_node_layout_rect(root).expect("root rect");
    assert!(
        (r.w - 320.0).abs() < 0.5,
        "#root width:320 not applied (cascade broken?): w={}",
        r.w
    );
}

#[test]
fn probe_display_none_prunes_hidden() {
    let (stage, _) = build_stage(HTML);
    let ghost = stage.find_node_by_id("ghost").expect("ghost");
    assert!(
        !stage.get_node_visible(ghost),
        ".hidden display:none should prune the node"
    );
}

#[test]
fn probe_full_fence_tag_set_instantiates() {
    // Every representative fence tag (containers, text, controls, list, progress)
    // must round-trip and lay out without panic. Node-kind fidelity (controls not
    // collapsing to Container) needs a kind query exit not yet present; this test
    // only proves the chain does not crash on the full tag set.
    let (stage, _) = build_stage(HTML);
    for id in [
        "title",
        "lbl-master",
        "vol",
        "vol-val",
        "mute",
        "pb",
        "quality",
        "li1",
        "save",
    ] {
        assert!(stage.find_node_by_id(id).is_some(), "node {id} missing");
    }
}

#[test]
fn probe_cascade_inheritance_and_specificity() {
    let (stage, _) = build_stage(HTML);
    // 继承 + 后代覆盖：`.row .lbl { font-size:12 }` 命中 span.lbl-master，
    // 覆盖继承自 #root 的 14。一次断言同时验「后代选择器匹配」+「继承基线」+「显式声明胜继承」。
    let lbl = stage.find_node_by_id("lbl-master").expect("lbl-master");
    let c = stage.get_node_computed_style(lbl).expect("lbl computed");
    assert!(
        (c.font_size - 12.0).abs() < 0.5,
        ".row .lbl should set font-size 12 (overriding inherited 14): got {}",
        c.font_size
    );
    // 继承（无覆盖）：span.vol-val 无 font-size 规则 → 继承 #root 的 14。
    let vol_val = stage.find_node_by_id("vol-val").expect("vol-val");
    let c = stage
        .get_node_computed_style(vol_val)
        .expect("vol-val computed");
    assert!(
        (c.font_size - 14.0).abs() < 0.5,
        "vol-val inherits #root font-size 14: got {}",
        c.font_size
    );
    // class 命中：`.muted { color:#888 }` 命中 vol-val（r=136/255≈0.533）。
    assert!(
        (c.color[0] - 136.0 / 255.0).abs() < 0.01,
        ".muted color #888 (r≈0.533): got {}",
        c.color[0]
    );
    // specificity：`#root .title { color:#0066aa }`（id+class=0,2,0）胜 `.title { color:#114488 }`（class=0,1,0）。
    // #0066aa = r=0, b=170/255≈0.667。
    let title = stage.find_node_by_id("title").expect("title");
    let c = stage
        .get_node_computed_style(title)
        .expect("title computed");
    assert!(
        c.color[0] < 0.01 && (c.color[2] - 170.0 / 255.0).abs() < 0.01,
        "#root .title should win specificity (color #0066aa): got {:?}",
        c.color
    );
}

#[test]
fn probe_control_kinds_do_not_collapse() {
    // kind 保真（防 §3.3「假绿」）：控件不塌成 Container。get_node_kind 是 ③ 新出口，
    // smoke 推迟的「kind 保真」断言在此兑现。
    let (stage, _) = build_stage(HTML);
    let kind = |id: &str| stage.get_node_kind(stage.find_node_by_id(id).expect(id));
    assert_eq!(kind("vol"), Some(NodeKind::Slider), "vol == Slider");
    assert_eq!(kind("mute"), Some(NodeKind::Toggle), "mute == Toggle");
    assert_eq!(
        kind("quality"),
        Some(NodeKind::Dropdown),
        "quality == Dropdown"
    );
    assert_eq!(kind("pb"), Some(NodeKind::ProgressBar), "pb == ProgressBar");
    assert_eq!(kind("save"), Some(NodeKind::Button), "save == Button");
    assert_eq!(kind("li1"), Some(NodeKind::ListItem), "li1 == ListItem");
    assert_eq!(
        kind("root"),
        Some(NodeKind::Container),
        "root still Container"
    );
}
