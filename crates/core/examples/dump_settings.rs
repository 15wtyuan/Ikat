//! settings showcase 诊断：定位「spinbutton 无法渲染」「switch 文字黑色」两个问题。
//!
//! 用法：cargo run -p loomgui_core --example dump_settings
//!
//! 输出三块：
//! 1. 全节点表（nid/kind/id/class/rect/color/meshes/verts）——看 spinbutton 是否有尺寸、是否产生 mesh
//! 2. 控件 ControlState 详情——看 spinbutton 是否有 NumberField 状态、value 是否 "32"
//! 3. switch 旁 span 的 color——验证是否继承到默认黑色 [0,0,0,1]

use loomgui_core::render::node::NodePayload;
use loomgui_core::scene::dynamic::append_child;
use loomgui_core::scene::node::{ControlState, NodeKind};
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
    let inst = s.instantiate("showcase", "settings").expect("instantiate");
    {
        let scene = s.scene.as_mut().unwrap();
        append_child(scene, root_id, inst).expect("append");
    }
    let frame = s.tick_and_render();
    let scene = s.scene.as_ref().unwrap();

    println!(
        "stage=1920x1080 scene_nodes={} render_nodes={}",
        scene.nodes.len(),
        frame.nodes.len()
    );

    let mut mesh_verts: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    for rn in &frame.nodes {
        let NodePayload::Mesh { verts, .. } = &rn.payload;
        *mesh_verts.entry(rn.node_id).or_default() += verts.len();
    }

    println!("\n=== all nodes ===");
    println!(
        "{:>4} {:<9} {:<16} {:<24} {:>23}  {:<22}  meshes",
        "nid", "kind", "id", "class", "rect", "color"
    );
    for n in scene.nodes.values() {
        let r = n.layout_rect;
        let id = n.id_attr.clone().unwrap_or_default();
        let c = n.style.color;
        let mv = mesh_verts.get(&(n.id.0)).copied().unwrap_or(0);
        let flag = if is_target(&id) { "  <<<" } else { "" };
        let extra = if n.kind == NodeKind::TextNode {
            let t = scene.text_contents.get(&n.id).cloned().unwrap_or_default();
            format!("  text={:?}", t.chars().take(12).collect::<String>())
        } else {
            String::new()
        };
        println!(
            "{:>4} {:<9} {:<16} {:<24} [{:>7.1},{:>7.1},{:>5.0},{:>5.0}]  [{:.2},{:.2},{:.2},{:.2}]  v={}{}{}",
            n.id.index(),
            kind_short(n.kind),
            id,
            n.classes.join(","),
            r.x,
            r.y,
            r.w,
            r.h,
            c[0],
            c[1],
            c[2],
            c[3],
            mv,
            extra,
            flag
        );
    }

    println!("\n=== render nodes (node_id, verts, program, image) ===");
    for (i, rn) in frame.nodes.iter().enumerate() {
        let NodePayload::Mesh {
            verts,
            image_path,
            program,
            ..
        } = &rn.payload;
        println!(
            "[{:>3}] node_id={:>4} verts={:>3} prog={} sk={:>3} img={:?}",
            i,
            rn.node_id,
            verts.len(),
            program,
            rn.sort_key,
            image_path
        );
    }

    // 检测同 node_id 多 mesh：C# MirrorPool 按 node_id 唯一索引 GO，
    // 同 node_id 的第 2+ 个 mesh 会被覆盖/跳过 = 渲染丢失。
    let mut id_count: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    for rn in &frame.nodes {
        *id_count.entry(rn.node_id).or_default() += 1;
    }
    let mut dups: Vec<u32> = id_count
        .iter()
        .filter(|(_, c)| **c >= 2)
        .map(|(k, _)| *k)
        .collect();
    dups.sort();
    println!("\n=== node_ids appearing >= 2 times (C# MirrorPool conflict) ===");
    if dups.is_empty() {
        println!("  (none)");
    }
    for id in &dups {
        let c = id_count[id];
        let info = scene
            .nodes
            .values()
            .find(|n| n.id.0 == *id)
            .map(|n| format!("kind={:?} id={:?} class={:?}", n.kind, n.id_attr, n.classes))
            .unwrap_or_else(|| "(synth/sub-page id)".to_string());
        println!("  node_id={} x{}  :: {}", id, c, info);
    }

    println!("\n=== control states ===");
    for (nid, cs) in scene.controls.iter() {
        let n = scene.get(nid).expect("control node exists");
        let id = n.id_attr.clone().unwrap_or_default();
        let r = n.layout_rect;
        let mv = mesh_verts.get(&(n.id.0)).copied().unwrap_or(0);
        println!(
            "nid={} kind={:?} id={} rect[{:.1},{:.1},{:.0},{:.0}] mesh_verts={} tabindex={:?} :: {}",
            nid.index(),
            n.kind,
            id,
            r.x,
            r.y,
            r.w,
            r.h,
            mv,
            n.interaction.tabindex,
            cs_summary(cs)
        );
    }
}

fn kind_short(k: NodeKind) -> &'static str {
    match k {
        NodeKind::Container => "div",
        NodeKind::TextNode => "#text",
        NodeKind::TextElement => "span",
        NodeKind::Button => "btn",
        NodeKind::Image => "img",
        NodeKind::TextField => "TextField",
        NodeKind::TextArea => "TextArea",
        NodeKind::NumberField => "NumberF",
        NodeKind::Slider => "Slider",
        NodeKind::Toggle => "Toggle",
        NodeKind::RadioButton => "Radio",
        NodeKind::Dropdown => "Dropdown",
        NodeKind::OptionItem => "Option",
        NodeKind::ProgressBar => "Progress",
        NodeKind::ListView => "ListView",
        NodeKind::ListItem => "ListItem",
        NodeKind::Slot => "Slot",
        NodeKind::CustomElement => "Custom",
        NodeKind::Template => "Template",
        NodeKind::TabList => "TabList",
        NodeKind::Tab => "Tab",
    }
}

fn is_target(id: &str) -> bool {
    matches!(id, "snd-voices" | "gfx-fullscreen" | "gfx-vsync")
}

fn cs_summary(cs: &ControlState) -> String {
    match cs {
        ControlState::NumberField {
            edit,
            min,
            max,
            step,
        } => {
            format!(
                "NumberField{{value={:?}, placeholder={:?}, min={}, max={}, step={}}}",
                edit.value, edit.placeholder, min, max, step
            )
        }
        ControlState::Toggle { checked } => format!("Toggle{{checked={}}}", checked),
        ControlState::TextField(e) => {
            format!(
                "TextField{{value={:?}, placeholder={:?}}}",
                e.value, e.placeholder
            )
        }
        ControlState::TextArea(e) => {
            format!(
                "TextArea{{value={:?}, placeholder={:?}}}",
                e.value, e.placeholder
            )
        }
        ControlState::Slider {
            value,
            min,
            max,
            step,
            dragging,
        } => {
            format!(
                "Slider{{value={},min={},max={},step={},drag={}}}",
                value, min, max, step, dragging
            )
        }
        ControlState::Radio { checked, name } => {
            format!("Radio{{checked={},name={:?}}}", checked, name)
        }
        ControlState::Dropdown {
            selected_index,
            open,
            ..
        } => {
            format!("Dropdown{{sel={},open={}}}", selected_index, open)
        }
        ControlState::Progress {
            value,
            max,
            indeterminate,
        } => {
            format!(
                "Progress{{value={},max={},indet={}}}",
                value, max, indeterminate
            )
        }
        ControlState::TabList { selected_index } => {
            format!("TabList{{sel={}}}", selected_index)
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
