//! 诊断 slot 投影布局：投影进组件槽位的 light 子在宿主 flex column 容器下
//! 是否各占一行（flex item）还是被折成 inline 流。dump 全节点 layout_rect +
//! 父子关系 + taffy display，复刻 dump_shop 的打印口径。

use loomgui_core::scene::dynamic::append_child;
use loomgui_core::scene::node::{Node, NodeKind};
use loomgui_core::stage::Stage;

fn main() {
    let pkg_path = std::env::args()
        .nth(1)
        .expect("usage: dump_slot_projection <pkg.bin>");
    let font_default = format!(
        "{}/../../unity/showcase-unity/Assets/Bundles/fonts/LXGWWenKai.ttf.bytes",
        env!("CARGO_MANIFEST_DIR")
    );
    let font_fallback = format!(
        "{}/../../unity/showcase-unity/Assets/Bundles/fonts/DejaVuSans.ttf.bytes",
        env!("CARGO_MANIFEST_DIR")
    );

    let pkg = std::fs::read(&pkg_path).expect("read pkg.bin");
    let mut s = Stage::new((1920.0, 1080.0)).expect("Stage::new");
    s.register_font("LXGWWenKai", std::fs::read(&font_default).unwrap(), true)
        .unwrap();
    s.register_font("DejaVuSans", std::fs::read(&font_fallback).unwrap(), false)
        .unwrap();
    s.set_fallback_families(&["DejaVuSans".to_string()]);
    s.load_package("game", &pkg).expect("load_package");
    let root_id = s.create_root("div", "").expect("create_root");
    let inst = s.instantiate("game", "battle").expect("instantiate battle");
    {
        let scene = s.scene.as_mut().unwrap();
        append_child(scene, root_id, inst).expect("append_child");
    }
    let _frame = s.tick_and_render();
    let scene = s.scene.as_ref().unwrap();

    println!("stage=1920x1080 scene_nodes={}", scene.nodes.len());
    println!(
        "{:<6} {:<6} {:<18} {:<22} {:>7} {:>7} {:>7} {:>7}  {:>5}  parent",
        "nid", "kind", "id", "class", "x", "y", "w", "h", "disp"
    );
    for n in scene.nodes.values() {
        print_row(n);
    }
}

fn print_row(n: &Node) {
    let r = n.layout_rect;
    let id = n.id_attr.clone().unwrap_or_default();
    let class = n.classes.join(",");
    let parent = n
        .parent
        .map(|p| p.index().to_string())
        .unwrap_or_else(|| "-".to_string());
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
        parent
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
        NodeKind::TextElement => "span",
        NodeKind::TextNode => "text",
        NodeKind::Image => "img",
        NodeKind::Button => "button",
        NodeKind::TextField => "input",
        NodeKind::NumberField => "num",
        NodeKind::Slider => "slider",
        NodeKind::Toggle => "toggle",
        NodeKind::RadioButton => "radio",
        NodeKind::TextArea => "textarea",
        NodeKind::Dropdown => "dropdown",
        NodeKind::OptionItem => "option",
        NodeKind::ProgressBar => "progress",
        NodeKind::ListView => "list",
        NodeKind::ListItem => "listitem",
        NodeKind::TabList => "tablist",
        NodeKind::Tab => "tab",
        NodeKind::Slot => "slot",
        NodeKind::CustomElement => "custom",
        NodeKind::Template => "template",
    }
}
