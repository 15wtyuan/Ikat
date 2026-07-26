//! 诊断 "showcase/shop 不能返回"：dump shop 页节点的 layout_rect / display / touchable，
//! 并对 header 左侧（back-home 区域）跑 hit_test，确认点击被谁接走。
//!
//! 待证假设：`.dialog-overlay#buy-dialog` 虽 inline display:none，但若其 layout_rect 仍非零
//! 且 touchable，而 hit_test 不跳 display:none 节点 → 顶层 overlay 误命中，吞掉 back-home 点击。

use loomgui_core::hit::hit_test;
use loomgui_core::scene::dynamic::append_child;
use loomgui_core::scene::node::{Node, NodeKind};
use loomgui_core::stage::Stage;

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
        "{}/../../unity/showcase-unity/Assets/Bundles/fonts/DejaVuSans.ttf.bytes",
        root
    );

    let pkg = std::fs::read(&pkg_path).expect("read showcase.pkg.bin");
    let mut s = Stage::new((1920.0, 1080.0)).expect("Stage::new");
    s.register_font("LXGWWenKai", std::fs::read(&font_default).unwrap(), true)
        .unwrap();
    s.register_font("DejaVuSans", std::fs::read(&font_fallback).unwrap(), false)
        .unwrap();
    s.set_fallback_families(&["DejaVuSans".to_string()]);
    s.set_image_sizes(&[
        ("res/icons/item-wand.png".to_string(), 128, 128),
        ("res/icons/character.png".to_string(), 128, 128),
        ("res/icons/item-chest.png".to_string(), 128, 128),
    ]);
    s.load_package("showcase", &pkg)
        .expect("load_package showcase");
    let root_id = s.create_root("div", "").expect("create_root");
    let inst = s.instantiate("showcase", "shop").expect("instantiate shop");
    {
        let scene = s.scene.as_mut().unwrap();
        append_child(scene, root_id, inst).expect("append_child");
    }
    let _frame = s.tick_and_render();
    let scene = s.scene.as_ref().unwrap();

    println!("stage=1920x1080 scene_nodes={}", scene.nodes.len());
    println!(
        "{:<6} {:<6} {:<18} {:<22} {:>7} {:>7} {:>7} {:>7}  {:>5}  touch",
        "nid", "kind", "id", "class", "x", "y", "w", "h", "disp"
    );
    for n in scene.nodes.values() {
        print_row(n);
    }

    println!("\n── hit_test 探针（back-home 在 header 左侧 padding 20,32 区域）──");
    for (px, py) in [
        (30.0, 30.0),
        (60.0, 30.0),
        (100.0, 30.0),
        (1600.0, 30.0),
        (960.0, 540.0),
    ] {
        let hit = hit_test(scene, (px, py));
        let desc = hit
            .map(|h| {
                let n = scene.get(h).expect("live");
                format!(
                    "nid={} {} id={:?} class={} rect=({:.0},{:.0},{:.0},{:.0}) disp={} touch={}",
                    h.index(),
                    kind_str(n.kind),
                    n.id_attr,
                    n.classes.join(","),
                    n.layout_rect.x,
                    n.layout_rect.y,
                    n.layout_rect.w,
                    n.layout_rect.h,
                    disp_str(n),
                    n.interaction.touchable
                )
            })
            .unwrap_or_else(|| "None".to_string());
        println!("hit({:>6.1},{:>5.1}) -> {}", px, py, desc);
    }
}

fn print_row(n: &Node) {
    let r = n.layout_rect;
    let id = n.id_attr.clone().unwrap_or_default();
    let class = n.classes.join(",");
    println!(
        "{:<6} {:<6} {:<18} {:<22} {:>7.1} {:>7.1} {:>7.1} {:>7.1}  {:>5}  {}",
        n.id.index(),
        kind_str(n.kind),
        id,
        class,
        r.x,
        r.y,
        r.w,
        r.h,
        disp_str(n),
        n.interaction.touchable
    );
}

fn disp_str(n: &Node) -> &'static str {
    if matches!(n.style.taffy_style.display, taffy::style::Display::None) {
        "none"
    } else {
        "-"
    }
}

fn kind_str(k: NodeKind) -> &'static str {
    match k {
        NodeKind::Container => "div",
        NodeKind::Button => "btn",
        NodeKind::Image => "img",
        NodeKind::TextNode => "#text",
        NodeKind::TextElement => "span",
        _ => "?",
    }
}
