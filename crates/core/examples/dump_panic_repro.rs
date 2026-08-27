//! 复现 api-infra 验收时的 FFI panic（rematch_pseudo_classes "live node" expect 每帧炸）。
//! 完整镜像 driver 行为链：页面实例化 + 数据驱动列表 + 时钟每帧 remove_node/create/set_text
//! churn（新 TextContent 释放语义）+ 副本包 Load/Instantiate/Unload/重载 + 下拉开关。

use ikat_core::list::{enter_data_driven, set_item_count};
use ikat_core::scene::dynamic::{
    append_child, create_node, remove_child, remove_node, set_inline_override, set_text,
};
use ikat_core::scene::node::NodeKind;
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
    let page = s.instantiate("showcase", "api-infra").expect("page");
    {
        let sc = s.scene.as_mut().unwrap();
        append_child(sc, r, page).unwrap();
    }
    let _ = s.tick_and_render();

    // 数据驱动列表（driver WireInfraDrivers 同款）
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

    // 时钟读数 span（driver Tick 每帧 TextContent = 清子释放 + 建新 TextNode + 挂）
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

    // 副本包 + 2 个微缩窗（driver 同款覆写）
    s.load_package("infra-copy", &pkg).expect("load copy");
    let ul_stage = s
        .scene
        .as_ref()
        .unwrap()
        .find_by_id_attr("infra-ul-stage")
        .expect("ul-stage");
    let mut minis = Vec::new();
    for _ in 0..2 {
        let win = s.instantiate("infra-copy", "api-infra").expect("mini");
        {
            let sc = s.scene.as_mut().unwrap();
            let _ = set_inline_override(sc, win, "width:420px");
            let _ = set_inline_override(sc, win, "height:88px");
            let _ = set_inline_override(sc, win, "overflow:clip");
            append_child(sc, ul_stage, win).unwrap();
        }
        minis.push(win);
        let _ = s.tick_and_render();
    }

    // 下拉开关一轮（flip 路径）
    let dd = s
        .scene
        .as_ref()
        .unwrap()
        .find_by_id_attr("infra-dd")
        .expect("dd");
    let _ = s.tick_and_render();

    // 主循环：时钟 churn × 240 帧（4 秒 @60fps）+ 中途 unload/重载 + 列表滚动
    for f in 0..240 {
        // clock/frames 的 TextContent 写（清子[真释放] + 建新 + set_text + append）
        for target in [clock, frames] {
            let sc = s.scene.as_mut().unwrap();
            let kids: Vec<_> = sc.get(target).unwrap().children.clone();
            for k in kids {
                if sc.get(k).map(|n| n.kind) == Some(NodeKind::TextNode) {
                    remove_node(sc, &mut Default::default(), k);
                } else {
                    let _ = remove_child(sc, target, k);
                }
            }
            let tn = create_node(sc, "span", "").expect("span");
            set_text(sc, tn, &format!("{}.{} s", f / 60, f % 60)).unwrap();
            append_child(sc, target, tn).unwrap();
        }
        if f == 60 {
            // 卸载 + 旧句柄实例化应失败 + 重载
            let _ = remove_child(s.scene.as_mut().unwrap(), ul_stage, minis[0]);
            let sc = s.scene.as_mut().unwrap();
            remove_node(sc, &mut Default::default(), minis[0]);
            s.unload_package("infra-copy").expect("unload");
            let w = s.instantiate("infra-copy", "api-infra"); // 应 Err
            println!("[f60] stale instantiate -> {:?}", w.is_err());
            s.load_package("infra-copy", &pkg).expect("reload");
            let win = s
                .instantiate("infra-copy", "api-infra")
                .expect("mini again");
            {
                let sc = s.scene.as_mut().unwrap();
                let _ = set_inline_override(sc, win, "width:420px");
                let _ = set_inline_override(sc, win, "height:88px");
                let _ = set_inline_override(sc, win, "overflow:clip");
                append_child(sc, ul_stage, win).unwrap();
            }
            minis.push(win);
        }
        if f == 120 {
            // 滚列表到底（自滚路径）
            s.set_scroll_pos(list_id, 0.0, 2000.0, false);
        }
        let _ = s.tick_and_render();
        if f % 60 == 0 {
            println!("[f{f}] ok, nodes={}", s.scene.as_ref().unwrap().nodes.len());
        }
    }
    // 下拉 open/close 一轮
    let sc = s.scene.as_mut().unwrap();
    let _ = set_inline_override(sc, dd, "overflow:visible");
    let _ = sc;
    let _ = s.tick_and_render();
    println!("DONE — no panic");
}
