//! 诊断 mail 虚拟列表覆盖缺口：set_scroll_pos 驱动滚动，测视口顶部是否被 slot 覆盖。
//! gap_top = (视口内首个 active slot 的 world Y) − 视口顶 world Y。> 一行高(~87) 即顶部空白。

use ikat_core::scene::dynamic::append_child;
use ikat_core::scene::node::{NodeKind, Scene};
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
        "{}/../../unity/showcase-unity/Assets/Bundles/fonts/wqy-microhei.ttc.bytes",
        root
    );
    let pkg = std::fs::read(&pkg_path).expect("pkg");
    let mut s = Stage::new((1920.0, 1080.0)).expect("Stage");
    s.register_font("LXGWWenKai", std::fs::read(&fd).unwrap(), true)
        .unwrap();
    s.register_font("wqy-microhei", std::fs::read(&ff).unwrap(), false)
        .unwrap();
    s.set_fallback_families(&["wqy-microhei".to_string()]);
    s.set_image_sizes(&icon_sizes());
    s.load_package("showcase", &pkg).expect("load");
    let r = s.create_root("div", "").expect("root");
    let inst = s.instantiate("showcase", "mail").expect("mail");
    {
        let sc = s.scene.as_mut().unwrap();
        append_child(sc, r, inst).unwrap();
    }
    let _ = s.tick_and_render();

    let mail_list = s
        .scene
        .as_ref()
        .unwrap()
        .find_by_id_attr("mail-list")
        .expect("mail-list");
    // 找 .col-list（class 含 col-list 的节点，是 mail-list 的滚动祖先）
    let col_list = {
        let sc = s.scene.as_ref().unwrap();
        sc.nodes
            .values()
            .find(|n| n.classes.iter().any(|c| c == "col-list"))
            .expect("col-list")
            .id
    };
    ikat_core::list::enter_data_driven(&mut s, mail_list, 1).expect("enter");
    ikat_core::list::set_item_count(&mut s, mail_list, 100);
    for _ in 0..6 {
        let _ = s.tick_and_render();
    } // 让初始 slot 测高度

    println!("mail_list={:?} col_list={:?}", mail_list, col_list);
    for &scroll_y in &[0.0_f32, 400.0, 800.0, 1500.0, 3000.0, 5000.0] {
        s.set_scroll_pos(col_list, 0.0, scroll_y, false);
        for _ in 0..3 {
            let _ = s.tick_and_render();
        }
        let (sx, sy) = s.get_scroll_pos(col_list).unwrap_or((0.0, 0.0));
        let sc = s.scene.as_ref().unwrap();
        let cl = sc.get(col_list).unwrap();
        let cl_top = sc.world_transforms[col_list.index()][5]; // ty
        let cl_h = cl.layout_rect.h;
        let cl_bot = cl_top + cl_h;
        let ls = sc.lists.0.get(&mail_list).expect("liststate");
        let vis = (ls.visible.start, ls.visible.end);
        let head_h = sc
            .get(ls.head_spacer)
            .map(|n| n.layout_rect.h)
            .unwrap_or(-1.0);
        let mut active_ys: Vec<(usize, f32)> = ls
            .slots
            .iter()
            .filter(|sl| !sl.parked)
            .map(|sl| (sl.item_index, sc.world_transforms[sl.node.index()][5]))
            .collect();
        active_ys.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        let n_active = active_ys.len();
        // topmost active slot whose world Y >= cl_top (in-viewport top)
        let gap_top = active_ys
            .iter()
            .find(|(_, y)| *y >= cl_top - 0.5)
            .map(|(_, y)| y - cl_top)
            .unwrap_or(cl_h);
        let first = active_ys
            .first()
            .map(|(i, y)| format!("item{}@y={:.0}", i, y))
            .unwrap_or("none".into());
        let first_in = active_ys
            .iter()
            .find(|(_, y)| *y >= cl_top - 0.5)
            .map(|(i, y)| format!("item{}@y={:.0}", i, y))
            .unwrap_or("none".into());
        println!("req_scroll={:<6} actual=({:.0},{:.0}) | viewport world[{:.0}..{:.0}] h={:.0} | visible[{}..{}) head_spacer_h={:.0} | active={} gap_top={:.0}px {} | first_active={} first_in_viewport={}",
            scroll_y, sx, sy, cl_top, cl_bot, cl_h, vis.0, vis.1, head_h, n_active, gap_top,
            if gap_top > 90.0 { "⚠️TOP-BLANK" } else { "ok" }, first, first_in);
        // 详细：scroll 最大时 dump 高度缓存（estimate vs known）
        if scroll_y >= 5000.0 {
            let ls = s.scene.as_ref().unwrap().lists.0.get(&mail_list).unwrap();
            let est = ls.heights.mean_known().unwrap_or(0.0);
            let (mut known, mut unknown, mut sum_known): (usize, usize, f32) = (0, 0, 0.0);
            for i in 0..ls.visible.start {
                match ls.heights.known.get(i).copied().flatten() {
                    Some(h) => {
                        known += 1;
                        sum_known += h;
                    }
                    None => unknown += 1,
                }
            }
            let ul = s.scene.as_ref().unwrap().get(mail_list).unwrap();
            println!("  [heights] estimate={:.1} | items[0..{}) known={} unknown={} sum_known={:.0} (avg {:.1}) | ul_offset_in_parent(layout.y)={:.0}",
                est, ls.visible.start, known, unknown, sum_known, if known>0 {sum_known/known as f32} else {0.0}, ul.layout_rect.y);
            println!("  spacer expected ≈ scroll - ul_offset = {:.0} - {:.0} = {:.0}; actual head_spacer_h={:.0} (over by {:.0})",
                sy, ul.layout_rect.y, sy - ul.layout_rect.y, head_h, head_h - (sy - ul.layout_rect.y));
        }
    }
}

fn icon_sizes() -> Vec<(String, u32, u32)> {
    [
        "logo",
        "settings",
        "inventory",
        "mail",
        "shop",
        "character",
        "form",
        "lab",
        "item-chest",
        "weapon",
        "item-gem",
        "skill-eye",
        "item-scroll",
        "item-staff",
    ]
    .iter()
    .map(|n| (format!("res/icons/{}.png", n), 128, 128))
    .collect()
}
// avoid unused import
fn _k() {
    let _ = NodeKind::Container;
    let _: &Scene;
}
