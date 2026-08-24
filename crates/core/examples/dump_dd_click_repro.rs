//! 精确复现「点击下拉列表触发 FFI panic（rematch live node [dynamic:675]）」：
//! 时钟 churn 并行 + 真实指针事件点 combobox（展开）→ 点 option（提交+收起）→ 连续 tick。

use loomgui_core::input::{PointerEvent, PointerKind};
use loomgui_core::list::{enter_data_driven, set_item_count};
use loomgui_core::scene::dynamic::{append_child, create_node, remove_node, set_text};
use loomgui_core::stage::Stage;

fn click(s: &mut Stage, x: f32, y: f32) {
    s.set_input(&[PointerEvent {
        kind: PointerKind::Down,
        button: 0,
        pad: [0, 0],
        touch_id: -1,
        x,
        y,
    }]);
    let _ = s.tick_and_render();
    s.set_input(&[PointerEvent {
        kind: PointerKind::Up,
        button: 0,
        pad: [0, 0],
        touch_id: -1,
        x,
        y,
    }]);
    let _ = s.tick_and_render();
}

fn main() {
    let root = env!("CARGO_MANIFEST_DIR");
    let pkg_path = format!(
        "{}/../../unity/showcase-unity/Assets/Bundles/ui/showcase.pkg.bin",
        root
    );
    let fd = format!(
        "{}/../../unity/showcase-unity/Assets/Bundles/fonts/LXGWWenKai.ttf.bytes",
        root
    );
    let ff = format!(
        "{}/../../unity/showcase-unity/Assets/Bundles/fonts/DejaVuSans.ttf.bytes",
        root
    );
    let pkg = std::fs::read(&pkg_path).expect("pkg");
    let mut s = Stage::new((1920.0, 1080.0)).expect("Stage");
    s.register_font("LXGWWenKai", std::fs::read(&fd).unwrap(), true)
        .unwrap();
    s.register_font("DejaVuSans", std::fs::read(&ff).unwrap(), false)
        .unwrap();
    s.set_fallback_families(&["DejaVuSans".to_string()]);
    s.load_package("showcase", &pkg).expect("load");
    let r = s.create_root("div", "").expect("root");
    let page = s.instantiate("showcase", "api-infra").expect("page");
    {
        let sc = s.scene.as_mut().unwrap();
        append_child(sc, r, page).unwrap();
    }
    let _ = s.tick_and_render();

    let list_id = s
        .scene
        .as_ref()
        .unwrap()
        .find_by_id_attr("infra-mt-list")
        .expect("list");
    enter_data_driven(&mut s, list_id, 1).expect("enter");
    set_item_count(&mut s, list_id, 30);

    let clock = s
        .scene
        .as_ref()
        .unwrap()
        .find_by_id_attr("infra-clock")
        .expect("clock");
    let frames = s
        .scene
        .as_ref()
        .unwrap()
        .find_by_id_attr("infra-frames")
        .expect("frames");
    let dd = s
        .scene
        .as_ref()
        .unwrap()
        .find_by_id_attr("infra-dd")
        .expect("dd");

    let churn = |s: &mut Stage, f: usize| {
        for target in [clock, frames] {
            let sc = s.scene.as_mut().unwrap();
            let kids: Vec<_> = sc.get(target).unwrap().children.clone();
            for k in kids {
                remove_node(sc, &mut Default::default(), k);
            }
            let tn = create_node(sc, "span", "").expect("span");
            set_text(sc, tn, &format!("{}.{}", f / 10, f % 10)).unwrap();
            append_child(sc, target, tn).unwrap();
        }
    };

    // 30 帧 churn 预热
    for f in 0..30 {
        churn(&mut s, f);
        let _ = s.tick_and_render();
    }

    // 用户会话上下文：Load ×2（mini 存活、无 display:none 优化——复现时的旧形态）
    // + .body 滚到 430（下拉在视口内、点选时的真实滚动位）
    s.load_package("infra-copy", &pkg).expect("load copy");
    let ul_stage = s
        .scene
        .as_ref()
        .unwrap()
        .find_by_id_attr("infra-ul-stage")
        .expect("ul-stage");
    for _ in 0..2 {
        let win = s.instantiate("infra-copy", "api-infra").expect("mini");
        {
            let sc = s.scene.as_mut().unwrap();
            append_child(sc, ul_stage, win).unwrap();
        }
        let _ = s.tick_and_render();
    }
    let body = s
        .scene
        .as_ref()
        .unwrap()
        .nodes
        .values()
        .find(|n| n.classes.iter().any(|c| c == "body"))
        .expect("body")
        .id;
    s.set_scroll_pos(body, 0.0, 430.0, false);
    for _ in 0..3 {
        let _ = s.tick_and_render();
    }

    // dd 展开前的世界位置（无滚动 = layout 即世界）
    let (ddx, ddy, ddw) = {
        let sc = s.scene.as_ref().unwrap();
        let n = sc.get(dd).unwrap();
        let wt = sc.world_transforms[dd.index()];
        (wt[4], wt[5], n.layout_rect.w)
    };
    println!("dd world=({ddx:.0},{ddy:.0},{ddw:.0}) — 点击展开");
    click(&mut s, ddx + ddw / 2.0, ddy + 8.0);

    // 找第一个 option 的世界位
    let opt_pos = {
        let sc = s.scene.as_ref().unwrap();
        let listbox = sc
            .get(dd)
            .unwrap()
            .children
            .iter()
            .copied()
            .find(|&c| sc.roles.role_of(c) == Some(loomgui_core::scene::control::ROLE_LISTBOX))
            .or_else(|| sc.get(dd).unwrap().children.iter().copied().nth(1));
        let lb = listbox.expect("listbox");
        let opt = sc
            .get(lb)
            .unwrap()
            .children
            .iter()
            .copied()
            .find(|&c| {
                sc.get(c)
                    .map(|n| n.kind == loomgui_core::scene::node::NodeKind::OptionItem)
                    == Some(true)
            })
            .expect("option");
        let wt = sc.world_transforms[opt.index()];
        (wt[4] + 40.0, wt[5] + 10.0)
    };
    println!(
        "option world=({:.0},{:.0}) — 点击提交",
        opt_pos.0, opt_pos.1
    );
    click(&mut s, opt_pos.0, opt_pos.1);

    // 提交后 60 帧 churn——panic 若在后续帧复发，这里会炸
    for f in 0..60 {
        churn(&mut s, f);
        let _ = s.tick_and_render();
    }
    println!("DONE — no panic");
}
