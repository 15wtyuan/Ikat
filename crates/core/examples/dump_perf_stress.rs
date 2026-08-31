//! 压测 api-infra 的持续掉帧复现：Load/Unload 反复循环 + 时钟 churn，每 60 帧量化
//! tick_and_render 耗时（μs）。掉帧 = 单帧耗时随交互次数增长（泄漏/累积）或绝对值异常。

use ikat_core::list::{enter_data_driven, set_item_count};
use ikat_core::scene::dynamic::{
    append_child, create_node, remove_node, set_inline_override, set_text,
};
use ikat_core::stage::Stage;
use std::time::Instant;

fn tick_ms(s: &mut Stage) -> f64 {
    let t = Instant::now();
    let _ = s.tick_and_render();
    t.elapsed().as_secs_f64() * 1000.0
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
    for _ in 0..4 {
        let _ = s.tick_and_render();
    }

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
    let ul_stage = s
        .scene
        .as_ref()
        .unwrap()
        .find_by_id_attr("infra-ul-stage")
        .expect("ul-stage");

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

    // 基线：60 帧 churn + 滚动
    let mut worst = 0.0f64;
    for f in 0..60 {
        churn(&mut s, f);
        if f == 30 {
            s.set_scroll_pos(list_id, 0.0, 1000.0, false);
        }
        let ms = tick_ms(&mut s);
        worst = worst.max(ms);
    }
    println!(
        "baseline 60帧: worst tick = {worst:.2} ms, nodes = {}",
        s.scene.as_ref().unwrap().nodes.len()
    );

    // Load/Unload 循环 ×8（用户「点了几下load和卸载」）：
    // Load = 载别名包 + 实例化 1 mini；Unload = 卸载注册表（mini 存活）+ 旧句柄实例化（应 Err）。
    for cycle in 0..8 {
        s.load_package("infra-copy", &pkg).expect("load copy");
        let win = s.instantiate("infra-copy", "api-infra").expect("mini");
        {
            let sc = s.scene.as_mut().unwrap();
            let _ = set_inline_override(sc, win, "width:420px");
            let _ = set_inline_override(sc, win, "height:88px");
            let _ = set_inline_override(sc, win, "overflow:clip");
            // driver 同款优化：裁剪后不可见的正文 display:none（solve 跳过 display:none 子树）
            let kids = sc.get(win).unwrap().children.clone();
            for k in kids {
                let is_body = {
                    let n = sc.get(k).unwrap();
                    n.classes.iter().any(|c| c == "body")
                };
                if is_body {
                    let _ = set_inline_override(sc, k, "display:none");
                }
            }
            append_child(sc, ul_stage, win).unwrap();
        }
        for f in 0..30 {
            churn(&mut s, f + cycle * 30);
            let ms = tick_ms(&mut s);
            worst = worst.max(ms);
        }
        s.unload_package("infra-copy").expect("unload");
        let stale = s.instantiate("infra-copy", "api-infra");
        println!(
            "cycle {cycle}: stale instantiate Err={} nodes={} worst_tick={worst:.2} ms",
            stale.is_err(),
            s.scene.as_ref().unwrap().nodes.len()
        );
    }

    // 尾态：再 120 帧 churn 看是否随时间恶化
    let mut tail_worst = 0.0f64;
    for f in 0..120 {
        churn(&mut s, f);
        tail_worst = tail_worst.max(tick_ms(&mut s));
    }
    println!(
        "tail 120帧: worst tick = {tail_worst:.2} ms, nodes = {}",
        s.scene.as_ref().unwrap().nodes.len()
    );

    let n = 30;
    let (mut t_scroll, mut t_list, mut t_rematch, mut t_anim, mut t_ctrl) =
        (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let (mut t_solve, mut t_measure, mut t_heights, mut t_content, mut t_world, mut t_full) =
        (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for _ in 0..n {
        let t = Instant::now();
        ikat_core::scroll::advance_all(0.016, s.scene.as_mut().unwrap());
        t_scroll += t.elapsed().as_secs_f64() * 1000.0;

        let t = Instant::now();
        let ops = ikat_core::list::plan_visible(s.scene.as_mut().unwrap());
        ikat_core::list::execute_visible(s.scene.as_mut().unwrap(), ops);
        t_list += t.elapsed().as_secs_f64() * 1000.0;

        let t = Instant::now();
        ikat_core::style::dynamic::rematch_pseudo_classes(
            s.scene.as_mut().unwrap(),
            s.root_size,
            s.safe_insets,
        );
        t_rematch += t.elapsed().as_secs_f64() * 1000.0;

        let t = Instant::now();
        ikat_core::style::dynamic::sync_animation_players(s.scene.as_mut().unwrap());
        t_anim += t.elapsed().as_secs_f64() * 1000.0;

        let t = Instant::now();
        let ctrl_ids: Vec<_> = s
            .scene
            .as_ref()
            .unwrap()
            .controls
            .0
            .keys()
            .copied()
            .collect();
        for cid in ctrl_ids {
            ikat_core::scene::control::sync_control_visuals(s.scene.as_mut().unwrap(), cid, 1080.0);
        }
        t_ctrl += t.elapsed().as_secs_f64() * 1000.0;

        let t = Instant::now();
        {
            let host = s.host.borrow();
            ikat_core::layout::solve(
                s.scene.as_mut().unwrap(),
                &host.fonts,
                s.root_size,
                s.safe_insets,
                &host.image_sizes,
            );
        }
        t_solve += t.elapsed().as_secs_f64() * 1000.0;

        let t = Instant::now();
        ikat_core::scene::control::measure_text_controls(
            s.scene.as_mut().unwrap(),
            &s.host.borrow().fonts,
        );
        t_measure += t.elapsed().as_secs_f64() * 1000.0;

        let t = Instant::now();
        ikat_core::list::collect_heights(s.scene.as_mut().unwrap());
        t_heights += t.elapsed().as_secs_f64() * 1000.0;

        let t = Instant::now();
        ikat_core::scroll::refresh_content_sizes(s.scene.as_mut().unwrap());
        t_content += t.elapsed().as_secs_f64() * 1000.0;

        let t = Instant::now();
        ikat_core::scene::transform::compute_world_transforms(s.scene.as_mut().unwrap());
        t_world += t.elapsed().as_secs_f64() * 1000.0;

        let t = Instant::now();
        let _ = s.tick_and_render();
        t_full += t.elapsed().as_secs_f64() * 1000.0;
    }
    let d = n as f64;
    println!("── 阶段均值（{n} 帧, 2384 节点态）──");
    println!("scroll_advance  {:7.2} ms", t_scroll / d);
    println!("list_plan_exec {:7.2} ms", t_list / d);
    println!("rematch        {:7.2} ms", t_rematch / d);
    println!("anim_players   {:7.2} ms", t_anim / d);
    println!("ctrl_visuals   {:7.2} ms", t_ctrl / d);
    println!("solve          {:7.2} ms", t_solve / d);
    println!("measure_text   {:7.2} ms", t_measure / d);
    println!("collect_h      {:7.2} ms", t_heights / d);
    println!("content_size   {:7.2} ms", t_content / d);
    println!("world_transf   {:7.2} ms", t_world / d);
    println!(
        "FULL tick      {:7.2} ms  (render+input ≈ full − Σ其余)",
        t_full / d
    );
    println!("DONE — no panic");
}
