//! M2 animation integration gate: HTML → fence → pkg → Stage → deterministic tick assertions.
//!
//! These tests deliberately exercise the real packer and Stage pipeline. The player unit tests
//! cover the timeline algorithm in isolation; this file locks the cross-layer wiring that makes
//! CSS declarations and packaged keyframes reach runtime NodeAnim and events.

use yio_core::asset::read_package;
use yio_core::event::{EVT_ANIMATION_END, EVT_ANIMATION_START};
use yio_core::render::node::RenderNode;
use yio_core::scene::node::NodeId;
use yio_core::stage::Stage;
use yio_pkg::build::{pack_components, Component};

const EPS: f32 = 1e-4;

fn close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() <= EPS,
        "expected {expected} ± {EPS}, got {actual}"
    );
}

fn pack(html: &str) -> Vec<u8> {
    pack_components(&[Component {
        name: "animation".to_owned(),
        src: html.to_owned(),
        html_rel: "animation.html".to_owned(),
    }])
    .expect("HTML must pass the fence and packer")
    .bytes
}

fn stage_from_html(html: &str) -> (Stage, NodeId) {
    let bytes = pack(html);
    let mut stage = Stage::new((640.0, 480.0)).expect("Stage::new");
    stage
        .register_font(
            "DejaVu",
            include_bytes!("fixtures/DejaVuSans.ttf").to_vec(),
            true,
        )
        .expect("register default font");
    stage
        .load_package("animation", &bytes)
        .expect("load package");
    let host = stage.create_root("div", "").expect("create host root");
    let root = stage
        .instantiate("animation", "animation")
        .expect("instantiate component");
    stage
        .append_child(host, root)
        .expect("attach component root");
    // Establish the first cascade before measuring time. This mirrors the runtime's startup frame:
    // declaration-triggered players are created during rematch at the end of that frame, so the
    // first explicit tick below advances an already-instantiated player.
    stage.advance_time(0.0);
    stage.tick_and_render();
    // The component root is the first (and only) packaged root. Tests that need to inspect
    // content children use this root; the host itself is intentionally not returned.
    (stage, root)
}

fn tick(stage: &mut Stage, dt: f32) -> yio_core::render::FrameData {
    stage.advance_time(dt);
    stage.tick_and_render()
}

fn node_anim(stage: &Stage, id: NodeId) -> &yio_core::scene::node::NodeAnim {
    stage
        .scene
        .as_ref()
        .expect("scene")
        .anim
        .get(id)
        .unwrap_or_else(|| {
            let scene = stage.scene.as_ref().expect("scene");
            let node = scene.get(id).expect("node");
            panic!(
                "node animation override: class={:?}, animation={:?}, players={:?}",
                node.classes,
                node.style.animation,
                scene
                    .players
                    .values()
                    .map(|p| (&p.spec.name, p.node))
                    .collect::<Vec<_>>()
            )
        })
}

fn child(stage: &Stage, parent: NodeId, index: usize) -> NodeId {
    stage
        .get_children(parent)
        .expect("children")
        .into_iter()
        .nth(index)
        .expect("child index")
}

fn render_node(frame: &yio_core::render::FrameData, id: NodeId) -> &RenderNode {
    frame
        .nodes
        .iter()
        .find(|node| node.node_id == id.0)
        .unwrap_or_else(|| panic!("RenderNode for node {} not found", id.0))
}

fn event_count(stage: &Stage, event_type: u8) -> usize {
    stage
        .last_events()
        .iter()
        .filter(|event| event.event_type == event_type)
        .count()
}

#[test]
fn fade_in_from_html_writes_opacity_and_translate_y_at_fixed_times() {
    let html = r#"<style>
        @keyframes fadeIn { from { opacity:0; transform:translateY(20px); }
                           to { opacity:1; transform:translateY(0); } }
        .card { animation:fadeIn .4s both; width:100px; height:40px; }
    </style><div class="card"></div>"#;
    let (mut stage, root) = stage_from_html(html);

    tick(&mut stage, 0.0);
    let first = node_anim(&stage, root);
    close(first.opacity.expect("opacity at t=0"), 0.0);
    close(first.transform.expect("transform at t=0")[5], 20.0);

    tick(&mut stage, 0.2);
    let middle = node_anim(&stage, root);
    // The declaration omits an easing keyword, so CSS initial `ease` applies — exact
    // bezier(0.25,0.1,0.25,1) since #9 (0.8024 at midpoint; CubicOut's 0.875 was an
    // approximation of the CSS curve).
    close(middle.opacity.expect("opacity at t=.2"), 0.8024);
    // ty lerps 20→0 with the same eased progress: 20 × (1 − 0.8024) ≈ 3.952.
    close(middle.transform.expect("transform at t=.2")[5], 3.9519);

    tick(&mut stage, 0.2);
    let last = node_anim(&stage, root);
    close(last.opacity.expect("opacity retained at t=.4"), 1.0);
    close(last.transform.expect("transform retained at t=.4")[5], 0.0);
}

#[test]
fn pulse_scale_infinite_alternate_reaches_both_extrema_without_end() {
    let html = r#"<style>
        @keyframes pulse { from { transform:scale(1); } 50% { transform:scale(1.1); }
                            to { transform:scale(1); } }
        .pulse { animation:pulse .4s infinite alternate linear; width:40px; height:40px; }
    </style><div class="pulse"></div>"#;
    let (mut stage, root) = stage_from_html(html);

    tick(&mut stage, 0.0);
    close(
        node_anim(&stage, root).transform.expect("scale at t=0")[0],
        1.0,
    );
    tick(&mut stage, 0.2);
    close(
        node_anim(&stage, root).transform.expect("scale at t=.2")[0],
        1.1,
    );
    tick(&mut stage, 0.2);
    close(
        node_anim(&stage, root).transform.expect("scale at t=.4")[0],
        1.0,
    );
    assert_eq!(
        event_count(&stage, EVT_ANIMATION_END),
        0,
        "infinite animation has no END"
    );
    assert_eq!(
        stage.scene.as_ref().expect("scene").players.len(),
        1,
        "infinite player remains active"
    );
}

#[test]
fn spin_rotate_infinite_linear_reaches_half_turn() {
    let html = r#"<style>
        @keyframes spin { from { transform:rotate(0deg); } to { transform:rotate(360deg); } }
        .spin { animation:spin 1s infinite linear; width:40px; height:40px; }
    </style><div class="spin"></div>"#;
    let (mut stage, root) = stage_from_html(html);

    tick(&mut stage, 0.0);
    close(
        node_anim(&stage, root).transform.expect("rotation at t=0")[0],
        1.0,
    );
    tick(&mut stage, 0.5);
    let transform = node_anim(&stage, root).transform.expect("rotation at t=.5");
    close(transform[0], -1.0);
    close(transform[1], 0.0);
}

#[test]
fn hue_three_stop_background_color_lerps_through_middle_stop() {
    let html = r#"<style>
        @keyframes hue { from { background-color:#ff0000; }
                          50% { background-color:#00ff00; }
                          to { background-color:#0000ff; } }
        .hue { animation:hue 1s infinite linear; width:40px; height:40px; }
    </style><div class="hue"></div>"#;
    let (mut stage, root) = stage_from_html(html);

    tick(&mut stage, 0.0);
    assert_eq!(
        node_anim(&stage, root).bg_color.expect("red at t=0"),
        [1.0, 0.0, 0.0, 1.0]
    );
    tick(&mut stage, 0.25);
    let color = node_anim(&stage, root).bg_color.expect("color at t=.25");
    for (actual, expected) in color.into_iter().zip([0.5, 0.5, 0.0, 1.0]) {
        close(actual, expected);
    }
    tick(&mut stage, 0.25);
    assert_eq!(
        node_anim(&stage, root).bg_color.expect("green at t=.5"),
        [0.0, 1.0, 0.0, 1.0]
    );
}

#[test]
fn nth_child_animation_delays_are_preserved_per_item() {
    let html = r#"<style>
        @keyframes slide { from { opacity:0; transform:translateY(10px); }
                            to { opacity:1; transform:translateY(0); } }
        .list { display:flex; width:500px; height:100px; }
        .item { animation:slide .4s both linear; width:20px; height:20px; }
        .item:nth-child(1) { animation:slide .4s 0s both linear; }
        .item:nth-child(2) { animation:slide .4s .1s both linear; }
        .item:nth-child(3) { animation:slide .4s .2s both linear; }
        .item:nth-child(4) { animation:slide .4s .3s both linear; }
        .item:nth-child(5) { animation:slide .4s .4s both linear; }
    </style><div class="list"><div class="item"></div><div class="item"></div><div class="item"></div><div class="item"></div><div class="item"></div></div>"#;
    let (mut stage, root) = stage_from_html(html);

    tick(&mut stage, 0.2);
    let items = stage.get_children(root).expect("list children");
    assert_eq!(items.len(), 5);
    for (index, item) in items.into_iter().enumerate() {
        let expected = match index {
            0 => 0.5,
            1 => 0.25,
            2..=4 => 0.0,
            _ => unreachable!(),
        };
        close(
            node_anim(&stage, item).opacity.expect("item opacity"),
            expected,
        );
    }
    let scene = stage.scene.as_ref().expect("scene");
    let mut delays: Vec<f32> = scene
        .players
        .values()
        .map(|player| player.spec.delay)
        .collect();
    delays.sort_by(|a, b| a.partial_cmp(b).expect("finite delay"));
    assert_eq!(delays, [0.0, 0.1, 0.2, 0.3, 0.4]);
}

#[test]
fn fill_modes_cover_none_forwards_backwards_and_both_end_states() {
    let html = r#"<style>
        @keyframes fade { from { opacity:0; } to { opacity:1; } }
        .wrap { display:flex; width:100px; height:20px; }
        .none { animation:fade .2s none linear; opacity:.3; width:10px; height:10px; }
        .forwards { animation:fade .2s forwards linear; opacity:.3; width:10px; height:10px; }
        .backwards { animation:fade .2s backwards linear; opacity:.3; width:10px; height:10px; }
        .both { animation:fade .2s both linear; opacity:.3; width:10px; height:10px; }
    </style><div class="wrap"><div class="none"></div><div class="forwards"></div><div class="backwards"></div><div class="both"></div></div>"#;
    let (mut stage, host) = stage_from_html(html);
    let nodes = stage.get_children(host).expect("four fill-mode nodes");
    assert_eq!(nodes.len(), 4);

    tick(&mut stage, 0.0);
    close(
        node_anim(&stage, nodes[0]).opacity.expect("none playing"),
        0.0,
    );
    close(
        node_anim(&stage, nodes[1])
            .opacity
            .expect("forwards playing"),
        0.0,
    );
    close(
        node_anim(&stage, nodes[2])
            .opacity
            .expect("backwards playing"),
        0.0,
    );
    close(
        node_anim(&stage, nodes[3]).opacity.expect("both playing"),
        0.0,
    );

    tick(&mut stage, 0.2);
    close(
        stage
            .scene
            .as_ref()
            .expect("scene")
            .get(nodes[0])
            .expect("none node")
            .style
            .opacity,
        0.3,
    );
    assert_eq!(
        stage
            .scene
            .as_ref()
            .expect("scene")
            .get(nodes[1])
            .expect("forwards node")
            .style
            .opacity,
        0.3,
        "style remains the base; forwards is held in NodeAnim"
    );
    close(
        node_anim(&stage, nodes[1])
            .opacity
            .expect("forwards retains"),
        1.0,
    );
    close(
        stage
            .scene
            .as_ref()
            .expect("scene")
            .get(nodes[2])
            .expect("backwards node")
            .style
            .opacity,
        0.3,
    );
    close(
        node_anim(&stage, nodes[3]).opacity.expect("both retains"),
        1.0,
    );
}

#[test]
fn backwards_fill_during_delay_and_direction_variants_are_cross_layer() {
    let html = r#"<style>
        @keyframes fade { from { opacity:0; } to { opacity:1; } }
        .wrap { display:flex; width:100px; height:20px; }
        .delayed { animation:fade .4s .2s backwards linear; width:10px; height:10px; }
        .normal { animation:fade .4s normal both linear; width:10px; height:10px; }
        .reverse { animation:fade .4s reverse both linear; width:10px; height:10px; }
        .alternate { animation:fade .2s 2 alternate both linear; width:10px; height:10px; }
    </style><div class="wrap"><div class="delayed"></div><div class="normal"></div><div class="reverse"></div><div class="alternate"></div></div>"#;
    let (mut stage, host) = stage_from_html(html);
    let nodes = stage.get_children(host).expect("direction nodes");
    tick(&mut stage, 0.0);
    close(
        node_anim(&stage, nodes[0])
            .opacity
            .expect("backwards delay"),
        0.0,
    );
    close(node_anim(&stage, nodes[1]).opacity.expect("normal"), 0.0);
    close(node_anim(&stage, nodes[2]).opacity.expect("reverse"), 1.0);
    close(node_anim(&stage, nodes[3]).opacity.expect("alternate"), 0.0);

    tick(&mut stage, 0.1);
    close(
        node_anim(&stage, nodes[3])
            .opacity
            .expect("alternate iter0"),
        0.5,
    );
    tick(&mut stage, 0.1);
    close(
        node_anim(&stage, nodes[3])
            .opacity
            .expect("alternate iter1"),
        1.0,
    );
}

#[test]
fn linear_and_step_easing_produce_different_packaged_node_values() {
    let html = r#"<style>
        @keyframes fade { from { opacity:0; } to { opacity:1; } }
        .wrap { display:flex; width:100px; height:20px; }
        .linear { animation:fade .4s both linear; width:10px; height:10px; }
        .step { animation:fade .4s both step-end; width:10px; height:10px; }
    </style><div class="wrap"><div class="linear"></div><div class="step"></div></div>"#;
    let (mut stage, host) = stage_from_html(html);
    let nodes = stage.get_children(host).expect("ease nodes");
    tick(&mut stage, 0.2);
    close(
        node_anim(&stage, nodes[0])
            .opacity
            .expect("linear midpoint"),
        0.5,
    );
    close(
        node_anim(&stage, nodes[1]).opacity.expect("step midpoint"),
        0.0,
    );
}

#[test]
fn transition_class_change_starts_real_tween_and_reaches_midpoint() {
    let html = r#"<style>
        .button { opacity:.2; transition:opacity .4s linear; width:20px; height:20px; }
        .button.active { opacity:1; }
    </style><div class="button"></div>"#;
    let (mut stage, button) = stage_from_html(html);
    tick(&mut stage, 0.0);
    close(
        stage
            .scene
            .as_ref()
            .expect("scene")
            .get(button)
            .expect("button")
            .style
            .opacity,
        0.2,
    );
    stage.add_class(button, "active").expect("add active class");
    // rematch creates the transition request during this frame; the next fixed tick advances it.
    tick(&mut stage, 0.0);
    tick(&mut stage, 0.2);
    close(
        node_anim(&stage, button)
            .opacity
            .expect("transition midpoint"),
        0.6,
    );
    tick(&mut stage, 0.2);
    close(
        node_anim(&stage, button).opacity.expect("transition end"),
        1.0,
    );
}

/// transition 提交帧必须预写起始值进 anim（drain 内 n=0 apply）：本帧 solve 读
/// override 而非级联终点。回归：曾缺预写 → 首帧闪现端点值（layout-anim 折叠面板
/// 展开先满高 200px 一帧再塌回 5px 起播；反向则先消失一帧）。
#[test]
fn transition_first_frame_holds_start_value_not_endpoint() {
    let html = r#"<style>
        .button { opacity:.2; transition:opacity .4s linear; width:20px; height:20px; }
        .button.active { opacity:1; }
    </style><div class="button"></div>"#;
    let (mut stage, button) = stage_from_html(html);
    tick(&mut stage, 0.0);
    stage.add_class(button, "active").expect("add active class");
    // 提交帧（tween 尚未推进）：anim 应持起始值 0.2，不是级联终点 1.0。
    tick(&mut stage, 0.0);
    close(
        node_anim(&stage, button)
            .opacity
            .expect("start pre-written"),
        0.2,
    );
    // 后续帧正常推进到中点（预写不影响推进语义）。
    tick(&mut stage, 0.2);
    close(
        node_anim(&stage, button)
            .opacity
            .expect("transition midpoint"),
        0.6,
    );
}

#[test]
fn transition_transform_class_change_interpolates_trs() {
    // transform 通道全链：class 翻转 transform → rematch diff → 复合 tween →
    // NodeAnim.transform 写入 SRT 合成矩阵（translate + scale 同时插值）。
    let html = r#"<style>
        .card { transform:translate(0px,0px) scale(1,1); transition:transform .4s linear; width:20px; height:20px; }
        .card.lifted { transform:translate(10px,4px) scale(2,1); }
    </style><div class="card"></div>"#;
    let (mut stage, card) = stage_from_html(html);
    tick(&mut stage, 0.0);
    stage.add_class(card, "lifted").expect("add lifted class");
    tick(&mut stage, 0.0); // rematch 检测 diff → drain 提交 tween
    tick(&mut stage, 0.2); // 半程
    let m = node_anim(&stage, card)
        .transform
        .expect("transform override at midpoint");
    close(m[4], 5.0); // tx: 0→10 的半程
    close(m[5], 2.0); // ty: 0→4 的半程
    close(m[0], 1.5); // a = sx（无旋转）: 1→2 的半程
    close(m[3], 1.0); // d = sy: 恒 1
    tick(&mut stage, 0.2); // 终值
    let m = node_anim(&stage, card)
        .transform
        .expect("transform override at end");
    close(m[4], 10.0);
    close(m[0], 2.0);
    close(m[3], 1.0);
}

#[test]
fn parent_animation_opacity_is_accumulated_into_child_render_node() {
    let html = r#"<style>
        @keyframes fade { from { opacity:0; } to { opacity:1; } }
        .parent { animation:fade .4s both linear; width:100px; height:100px; }
        .child { opacity:.4; width:20px; height:20px; }
    </style><div class="parent"><div class="child"></div></div>"#;
    let (mut stage, parent) = stage_from_html(html);
    let child = child(&stage, parent, 0);
    let frame = tick(&mut stage, 0.2);
    close(render_node(&frame, parent).alpha, 0.5);
    close(render_node(&frame, child).alpha, 0.2);
}

#[test]
fn class_animation_emits_start_once_and_end_at_deterministic_completion() {
    let html = r#"<style>
        @keyframes fade { from { opacity:0; } to { opacity:1; } }
        .animated { animation:fade .2s both linear; width:20px; height:20px; }
    </style><div id="target" style="width:20px;height:20px"></div>"#;
    let (mut stage, target) = stage_from_html(html);
    stage.add_class(target, "animated").expect("class trigger");
    // The class is rematched at this tick; START is emitted on the next tick when update_all
    // first advances the newly synchronized player.
    tick(&mut stage, 0.0);
    tick(&mut stage, 0.0);
    assert_eq!(event_count(&stage, EVT_ANIMATION_START), 1);
    assert_eq!(event_count(&stage, EVT_ANIMATION_END), 0);
    tick(&mut stage, 0.2);
    assert_eq!(event_count(&stage, EVT_ANIMATION_START), 0);
    assert_eq!(event_count(&stage, EVT_ANIMATION_END), 1);
    tick(&mut stage, 0.2);
    assert_eq!(
        event_count(&stage, EVT_ANIMATION_END),
        0,
        "END is transition-only"
    );
}

#[test]
fn package_roundtrip_keeps_keyframes_and_dynamic_animation_declaration() {
    let html = r#"<style>
        @keyframes fade { from { opacity:0; } to { opacity:1; } }
        .card { animation:fade .4s both; width:20px; height:20px; }
    </style><div class="card"></div>"#;
    let pkg = read_package(&pack(html)).expect("read generated package");
    let component = pkg.components.get("animation").expect("component");
    assert_eq!(component.keyframes.len(), 1);
    assert_eq!(component.keyframes[0].name, "fade");
    assert!(component.dynamic_rules.rules.iter().any(|rule| {
        rule.selector.raw == ".card"
            && rule
                .declarations
                .iter()
                .any(|decl| decl.prop == "animation")
    }));
}
