//! 诊断（mail 滚动 item 消失 + 掉帧，就算不滚动也低帧）：
//! 实例化 showcase mail，set ItemCount=100，测 tick 耗时 + 动画 player 数 + slot 结构
//! （复现 core 侧的 per-frame 开销 + 定位 blank gap）。

use std::time::Instant;
use yio_core::scene::dynamic::append_child;
use yio_core::stage::Stage;

fn main() {
    let root = env!("CARGO_MANIFEST_DIR");
    let pkg_path = format!(
        "{}/../../unity/showcase-unity/Assets/Bundles/ui/showcase.pkg.bin",
        root
    );
    let font_default = format!(
        "{}/../../unity/showcase-unity/Assets/Bundles/fonts/LXGWWenKai.ttf.bytes",
        root
    );
    let font_fallback = format!(
        "{}/../../unity/showcase-unity/Assets/Bundles/fonts/wqy-microhei.ttc.bytes",
        root
    );

    let pkg = std::fs::read(&pkg_path).expect("read pkg");
    let mut s = Stage::new((1920.0, 1080.0)).expect("Stage::new");
    s.register_font("LXGWWenKai", std::fs::read(&font_default).unwrap(), true)
        .unwrap();
    s.register_font(
        "wqy-microhei",
        std::fs::read(&font_fallback).unwrap(),
        false,
    )
    .unwrap();
    s.set_fallback_families(&["wqy-microhei".to_string()]);
    s.set_image_sizes(&icon_sizes());
    s.load_package("showcase", &pkg).expect("load_package");
    let root_id = s.create_root("div", "").expect("create_root");
    let inst = s.instantiate("showcase", "mail").expect("instantiate mail");
    {
        let scene = s.scene.as_mut().unwrap();
        append_child(scene, root_id, inst).expect("append_child");
    }
    let _ = s.tick_and_render();

    let mail_list = s
        .scene
        .as_ref()
        .unwrap()
        .find_by_id_attr("mail-list")
        .expect("mail-list node");
    // 进数据驱动模式（FFI yio_list_set_item_count 首调自动做这步；这里显式调），
    // 再 set_item_count=100（触发虚拟化）。
    yio_core::list::enter_data_driven(&mut s, mail_list, 1).expect("enter_data_driven");
    yio_core::list::set_item_count(&mut s, mail_list, 100);
    // 跑几帧让虚拟化落定 + bind + 高度回填
    for _ in 0..5 {
        let _ = s.tick_and_render();
    }

    // slot 结构 + player 数：scope the immutable borrow so timing loop can &mut s
    let (player_count, has_breathe, total_slots, parked_slots) = {
        let scene = s.scene.as_ref().unwrap();
        let player_count = scene.players.len();
        let has_breathe = scene.keyframes.contains_key("breathe");
        let mut total_slots = 0usize;
        let mut parked_slots = 0usize;
        for ls in scene.lists.0.values() {
            for sl in &ls.slots {
                total_slots += 1;
                if sl.parked {
                    parked_slots += 1;
                }
            }
        }
        (player_count, has_breathe, total_slots, parked_slots)
    };

    println!("========== mail headless 复现 ==========");
    println!(
        "player_count={} has_breathe={} | slots total={} parked={}",
        player_count, has_breathe, total_slots, parked_slots
    );
    // 场景规模：节点数 / 动态规则数（rematch 是 rules×nodes）/ render 节点数
    {
        let scene = s.scene.as_ref().unwrap();
        let n_nodes = scene.nodes.len();
        let n_rules = scene.dynamic_rules.entries.len();
        let n_scoped = scene.dynamic_rules.entries.len();
        println!(
            "scene: nodes={} scoped_rules={} total_rules={}",
            n_nodes, n_scoped, n_rules
        );
    }
    let frame = s.tick_and_render();
    println!("render_nodes={} (frame.nodes.len)", frame.nodes.len());

    // 缩放测试：tick 耗时是否随 ItemCount 增长？（只密 14 slot，若有东西 O(items) 就暴露）
    for &ic in &[10usize, 100, 1000, 5000] {
        yio_core::list::set_item_count(&mut s, mail_list, ic);
        for _ in 0..3 {
            let _ = s.tick_and_render();
        } // settle
        let t = time_ticks(&mut s, 20);
        let sc = s
            .scene
            .as_ref()
            .unwrap()
            .lists
            .0
            .values()
            .next()
            .map(|ls| ls.slots.len())
            .unwrap_or(0);
        println!("ItemCount={:<5} tick={:.3} ms/frame  (slots={})", ic, t, sc);
    }

    // 对比：home 页（无 list）
    let home_inst = s.instantiate("showcase", "home").expect("instantiate home");
    let home_root = s.create_root("div", "").expect("root2");
    {
        let sc = s.scene.as_mut().unwrap();
        append_child(sc, home_root, home_inst).unwrap();
    }
    for _ in 0..3 {
        let _ = s.tick_and_render();
    }
    let t_home = time_ticks(&mut s, 20);
    println!("home (no list)        tick={:.3} ms/frame", t_home);

    println!("(参考：单帧 16.7ms = 60fps 预算)");
}

fn time_ticks(s: &mut Stage, n: u32) -> f32 {
    let start = Instant::now();
    for _ in 0..n {
        let _ = s.tick_and_render();
    }
    start.elapsed().as_secs_f32() * 1000.0 / n as f32
}

fn icon_sizes() -> Vec<(String, u32, u32)> {
    let names = [
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
    ];
    names
        .iter()
        .map(|n| (format!("res/icons/{}.png", n), 128, 128))
        .collect()
}
