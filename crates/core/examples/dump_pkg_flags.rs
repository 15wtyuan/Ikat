//! 读 pkg.bin 全部 TemplateNode，打印 kind/class/id/rich_text_block，
//! 定位 rich 折叠 flag 的打包期来源。

use loomgui_core::asset::read_package;
use loomgui_core::scene::NodeKind;

fn main() {
    let pkg_path = std::env::args()
        .nth(1)
        .expect("usage: dump_pkg_flags <pkg.bin>");
    let bytes = std::fs::read(&pkg_path).expect("read pkg");
    let pkg = read_package(&bytes).expect("read_package");
    for (cname, comp) in &pkg.components {
        println!(
            "component {cname}: {} nodes, {} scopes",
            comp.nodes.len(),
            comp.component_scopes.len()
        );
        for (i, n) in comp.nodes.iter().enumerate() {
            println!(
                "  [{i:>3}] parent={:<3} kind={:<10} class={:<12} id={:<10} rich_text_block={} taffy_display={:?}",
                n.parent_idx.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
                kind_str(n.kind),
                n.classes.join(","),
                n.id_attr.clone().unwrap_or_default(),
                n.rich_text_block,
                n.style.taffy_style.display,
            );
        }
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
        NodeKind::Link => "a",
    }
}
