//! 诊断 showcase/api-infra 验收问题四件：
//! 1) infra-clock/infra-frames 节点 kind（driver TryGet<TextNode> miss 假设取证）
//! 2) infra-dd combobox 子树矩形（收起态可见性：无 data-slot=value 时高度是否塌 0）
//! 3) infra-tabs 点击模拟（core T7：Down+Up 注入 → controls selected_index 是否变化）
//! 4) infra-mt-list 虚拟列表滚动覆盖（set_item_count(30) + set_scroll_pos 逐档量化，
//!    active slot 是否覆盖视口 / 是否只渲染前 N 行）

use ikat_core::hit::hit_test;
use ikat_core::input::{PointerEvent, PointerKind};
use ikat_core::list::{enter_data_driven, set_item_count};
use ikat_core::scene::dynamic::append_child;
use ikat_core::scene::node::{ControlState, NodeKind};
use ikat_core::stage::Stage;

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
    let inst = s.instantiate("showcase", "api-infra").expect("api-infra");
    {
        let sc = s.scene.as_mut().unwrap();
        append_child(sc, r, inst).unwrap();
    }
    let _ = s.tick_and_render();
    let _ = s.tick_and_render();

    println!("── 1) 关键节点 kind ──");
    let sc = s.scene.as_ref().unwrap();
    for id_name in [
        "infra-clock",
        "infra-frames",
        "infra-later",
        "infra-nf",
        "dd-sel",
        "tab-r1",
    ] {
        match sc.find_by_id_attr(id_name) {
            Some(nid) => {
                let n = sc.get(nid).unwrap();
                println!(
                    "  {:<14} -> {:?}  rect=({:.0},{:.0},{:.0},{:.0})",
                    id_name,
                    n.kind,
                    n.layout_rect.x,
                    n.layout_rect.y,
                    n.layout_rect.w,
                    n.layout_rect.h
                );
            }
            None => println!("  {:<14} -> NOT FOUND", id_name),
        }
    }

    println!("\n── 2) infra-dd combobox 子树 ──");
    let dd = sc.find_by_id_attr("infra-dd").expect("infra-dd");
    dump_subtree(sc, dd, 0);
    let ddr = sc.get(dd).unwrap().layout_rect;
    let (cx, cy) = (ddr.x + ddr.w / 2.0, ddr.y + 6.0);
    let hit = hit_test(sc, (cx, cy));
    println!(
        "  hit_test({:.0},{:.0}) -> {:?}",
        cx,
        cy,
        hit.map(|h| (h.index(), sc.get(h).unwrap().kind))
    );

    // 先滚 .body 把 tabs 带进 1080 视口再点
    println!("\n── 3) infra-tabs 点击模拟 ──");
    let tabs_id = sc.find_by_id_attr("infra-tabs").expect("infra-tabs");
    let body_id = {
        let sc = s.scene.as_ref().unwrap();
        sc.nodes
            .values()
            .find(|n| n.classes.iter().any(|c| c == "body"))
            .expect(".body scroller")
            .id
    };
    let tab_ids: Vec<_> = {
        let sc = s.scene.as_ref().unwrap();
        sc.get(tabs_id).unwrap().children.clone()
    };
    let sel_before = tablist_selected(s.scene.as_ref().unwrap(), tabs_id);
    s.set_scroll_pos(body_id, 0.0, 700.0, false);
    for _ in 0..3 {
        let _ = s.tick_and_render();
    }
    for (i, &t) in tab_ids.iter().enumerate() {
        let (kind, ida, ty) = {
            let sc = s.scene.as_ref().unwrap();
            let n = sc.get(t).unwrap();
            (n.kind, n.id_attr.clone(), sc.world_transforms[t.index()][5])
        };
        if matches!(kind, NodeKind::Tab) {
            println!(
                "  tab[{i}] nid={} kind={kind:?} id={ida:?} world_y={ty:.0}",
                t.index()
            );
        }
    }
    // 点第二个 Tab（itab-2）：世界坐标 = 自身 layout_rect 换世界（.body 已滚）。
    let itab2 = s
        .scene
        .as_ref()
        .unwrap()
        .find_by_id_attr("itab-2")
        .expect("itab-2");
    let (px, py) = {
        let sc = s.scene.as_ref().unwrap();
        let wt = &sc.world_transforms[itab2.index()];
        let n = sc.get(itab2).unwrap();
        // Affine2 = [a,b,c,d,tx,ty]：世界位 = (tx,ty) + 半宽高偏移（tab 无自变换）
        (wt[4] + n.layout_rect.w * 0.5, wt[5] + n.layout_rect.h * 0.5)
    };
    println!("  点击 itab-2 中心 ({px:.0},{py:.0})（.body 已滚 700）");
    {
        let sc = s.scene.as_ref().unwrap();
        let hit = hit_test(sc, (px, py));
        println!(
            "  hit_test -> {:?}",
            hit.map(|h| (
                h.index(),
                sc.get(h).unwrap().kind,
                sc.get(h).unwrap().id_attr.clone()
            ))
        );
    }
    s.set_input(&[PointerEvent {
        kind: PointerKind::Down,
        button: 0,
        pad: [0, 0],
        touch_id: -1,
        x: px,
        y: py,
    }]);
    let _ = s.tick_and_render();
    s.set_input(&[PointerEvent {
        kind: PointerKind::Up,
        button: 0,
        pad: [0, 0],
        touch_id: -1,
        x: px,
        y: py,
    }]);
    let _ = s.tick_and_render();
    let sel_after = tablist_selected(s.scene.as_ref().unwrap(), tabs_id);
    println!(
        "  selected_index: before={sel_before:?} after={sel_after:?} {}",
        if sel_before != sel_after {
            "✅ T7 生效"
        } else {
            "❌ 未变（T7 没触发）"
        }
    );

    println!("\n── 4) infra-mt-list 虚拟列表（count=30）──");
    let list_id = s
        .scene
        .as_ref()
        .unwrap()
        .find_by_id_attr("infra-mt-list")
        .expect("infra-mt-list");
    enter_data_driven(&mut s, list_id, 1).expect("enter_data_driven");
    set_item_count(&mut s, list_id, 30);
    for _ in 0..6 {
        let _ = s.tick_and_render();
    }
    // 仪表：列表自身 scroll 状态 vs plan 实际读到的祖先 viewport + 高度缓存。
    {
        let sc = s.scene.as_ref().unwrap();
        let own = sc.scroll.get(list_id);
        println!(
            "  [own scroll]    {:?}",
            own.map(|st| (st.scroll_pos, st.viewport_size, st.content_size))
        );
        let mut cur = sc.get(list_id).and_then(|n| n.parent);
        while let Some(pid) = cur {
            if let Some(st) = sc.scroll.get(pid) {
                let n = sc.get(pid).unwrap();
                println!(
                    "  [ancestor pane] nid={} class={} scroll_pos=({:.0},{:.0}) viewport_h={:.0}",
                    pid.index(),
                    n.classes.join(","),
                    st.scroll_pos.0,
                    st.scroll_pos.1,
                    st.viewport_size.1
                );
            }
            cur = sc.get(pid).and_then(|n| n.parent);
        }
        let ls = sc.lists.0.get(&list_id).unwrap();
        let known_cnt = ls.heights.known.iter().filter(|h| h.is_some()).count();
        println!(
            "  [heights] item_count={} estimate={:.1} known={}/{} visible={:?}",
            ls.item_count,
            ls.heights.estimate,
            known_cnt,
            ls.heights.known.len(),
            ls.visible
        );
    }
    for &scroll_y in &[0.0_f32, 200.0, 400.0, 800.0, 1200.0] {
        s.set_scroll_pos(list_id, 0.0, scroll_y, false);
        for _ in 0..3 {
            let _ = s.tick_and_render();
        }
        let sc = s.scene.as_ref().unwrap();
        let ls = sc.lists.0.get(&list_id).expect("liststate");
        let top = sc.world_transforms[list_id.index()][5];
        let h = sc.get(list_id).unwrap().layout_rect.h;
        let mut active: Vec<(usize, f32)> = ls
            .slots
            .iter()
            .filter(|sl| !sl.parked)
            .map(|sl| (sl.item_index, sc.world_transforms[sl.node.index()][5]))
            .collect();
        active.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        let gap_top = active
            .iter()
            .find(|(_, y)| *y >= top - 0.5)
            .map(|(_, y)| y - top)
            .unwrap_or(h);
        let idxs: Vec<String> = active
            .iter()
            .map(|(i, y)| format!("{}@{:.0}", i, y))
            .collect();
        println!(
            "  scroll={:<6} viewport[{:.0}..{:.0}] visible[{}..{}) active={} gap_top={:.0} {} slots=[{}]",
            scroll_y, top, top + h, ls.visible.start, ls.visible.end, active.len(), gap_top,
            if gap_top > 60.0 { "⚠️TOP-BLANK" } else { "ok" },
            idxs.join(" ")
        );
        // 视口内 slot 数（用户症状：滚下去整块空白）
        let in_view = active
            .iter()
            .filter(|(_, y)| *y >= top - 0.5 && *y < top + h)
            .count();
        println!("    in-viewport slots = {in_view}");
    }

    // UnloadPackage 演示复现：副本包连续实例化 3 个微缩窗（driver 同款
    // Style 覆写 width/height/overflow），对比第 1 个与后续的子树可见性。
    println!("\n── 5) 副本包微缩窗 ×3（Load 演示复现）──");
    s.load_package("infra-copy", &pkg).expect("load infra-copy");
    let stage_root = s.create_root("div", "").expect("stage root 2");
    {
        let sc = s.scene.as_mut().unwrap();
        use ikat_core::scene::dynamic::set_inline_override;
        let _ = set_inline_override(sc, stage_root, "display:flex;flex-direction:row");
    }
    for k in 0..3 {
        let win = s.instantiate("infra-copy", "api-infra").expect("mini");
        {
            let sc = s.scene.as_mut().unwrap();
            use ikat_core::scene::dynamic::set_inline_override;
            let _ = set_inline_override(sc, win, "width:420px");
            let _ = set_inline_override(sc, win, "height:88px");
            let _ = set_inline_override(sc, win, "overflow:clip");
            append_child(sc, stage_root, win).expect("append mini");
        }
        let _ = s.tick_and_render();
        let _ = s.tick_and_render();
        let sc = s.scene.as_ref().unwrap();
        let r = sc.get(win).unwrap().layout_rect;
        let mut visible = 0usize;
        let mut total = 0usize;
        count_visible(sc, win, &mut visible, &mut total);
        println!(
            "  mini[{k}] nid={} rect=({:.0},{:.0},{:.0},{:.0}) 节点可见 {visible}/{total}",
            win.index(),
            r.x,
            r.y,
            r.w,
            r.h
        );
    }
}

fn count_visible(
    sc: &ikat_core::scene::node::Scene,
    id: ikat_core::scene::node::NodeId,
    visible: &mut usize,
    total: &mut usize,
) {
    let n = sc.get(id).unwrap();
    *total += 1;
    let shown = !matches!(n.style.taffy_style.display, taffy::style::Display::None);
    if shown {
        *visible += 1;
    }
    for &c in &n.children {
        count_visible(sc, c, visible, total);
    }
}

fn tablist_selected(
    sc: &ikat_core::scene::node::Scene,
    id: ikat_core::scene::node::NodeId,
) -> Option<usize> {
    match sc.controls.get(id) {
        Some(ControlState::TabList { selected_index, .. }) => Some(*selected_index),
        other => {
            println!("  (controls[{:.0}] = {other:?})", id.index() as f32);
            None
        }
    }
}

fn dump_subtree(
    sc: &ikat_core::scene::node::Scene,
    id: ikat_core::scene::node::NodeId,
    depth: usize,
) {
    let n = sc.get(id).unwrap();
    let r = n.layout_rect;
    println!(
        "{:indent$}nid={} {:?} id={:?} class={} rect=({:.0},{:.0},{:.0},{:.0})",
        "",
        id.index(),
        n.kind,
        n.id_attr,
        n.classes.join(","),
        r.x,
        r.y,
        r.w,
        r.h,
        indent = depth * 2
    );
    for &c in &n.children {
        dump_subtree(sc, c, depth + 1);
    }
}
