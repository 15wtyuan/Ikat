//! 诊断：dump home 页 btn-primary 的 primary 渲染节点 + 其 box-shadow synth 节点，
//! 取证「阴影位置偏下」+「按钮底色不对」的 core 侧实际状态。
//!
//! 看：primary verts bbox + color（底色对不对）、shadow synth verts bbox（偏移多少）、
//! sort_key（shadow 是否在 primary 后=behind）、shadow_params（σ/inset）、program。

use yio_core::render::node::{NodePayload, RenderNode};
use yio_core::scene::dynamic::append_child;
use yio_core::stage::Stage;

fn bbox(verts: &[[f32; 2]]) -> (f32, f32, f32, f32) {
    if verts.is_empty() {
        return (0.0, 0.0, 0.0, 0.0);
    }
    let (mut minx, mut miny, mut maxx, mut maxy) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for v in verts {
        minx = minx.min(v[0]);
        miny = miny.min(v[1]);
        maxx = maxx.max(v[0]);
        maxy = maxy.max(v[1]);
    }
    (minx, miny, maxx - minx, maxy - miny)
}

fn dump_mesh(rn: &RenderNode, indent: &str) {
    let NodePayload::Mesh {
        verts,
        colors,
        program,
        ..
    } = &rn.payload;
    let (x, y, w, h) = bbox(verts);
    let cx = x + w * 0.5;
    let cy = y + h * 0.5;
    println!(
        "{indent}verts bbox=({:.0},{:.0},{:.0},{:.0}) center=({:.0},{:.0})",
        x, y, w, h, cx, cy
    );
    let c0 = colors.first().copied().unwrap_or([0.0; 4]);
    println!(
        "{indent}color[0]=rgba({:.3},{:.3},{:.3},{:.3}) program={}",
        c0[0], c0[1], c0[2], c0[3], program
    );
}

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
    let inst = s.instantiate("showcase", "home").expect("instantiate home");
    {
        let scene = s.scene.as_mut().unwrap();
        append_child(scene, root_id, inst).expect("append_child");
    }
    let frame = s.tick_and_render();
    let scene = s.scene.as_ref().unwrap();

    let btn = scene
        .nodes
        .values()
        .find(|n| n.classes.iter().any(|c| c == "btn-primary"))
        .expect("btn-primary not found");
    let btn_id = btn.id.0;
    println!("btn-primary scene node_id={}", btn_id);
    let bs = &btn.style.box_shadow;
    println!("btn-primary box_shadow layers: {}", bs.len());
    for (i, sh) in bs.iter().enumerate() {
        println!(
            "  layer{} ox={} oy={} blur={} spread={} inset={} color=rgba({:.3},{:.3},{:.3},{:.3})",
            i,
            sh.ox,
            sh.oy,
            sh.blur,
            sh.spread,
            sh.inset,
            sh.color[0],
            sh.color[1],
            sh.color[2],
            sh.color[3]
        );
    }
    println!();

    println!("========== btn-primary 渲染节点 ==========");
    let mut primary_rn: Option<&RenderNode> = None;
    for rn in &frame.nodes {
        if rn.node_id == btn_id {
            primary_rn = Some(rn);
            break;
        }
    }
    if let Some(rn) = primary_rn {
        println!(
            "[primary] node_id={} sort_key={} mask_ctx={}",
            rn.node_id, rn.sort_key, rn.mask_context.0
        );
        dump_mesh(rn, "  ");
    } else {
        println!("[primary] 未找到 node_id=={} 的渲染节点！", btn_id);
    }
    println!();

    println!("========== btn-primary shadow synth 节点 ==========");
    for rn in &frame.nodes {
        let hi = rn.node_id >> 56;
        let lo = rn.node_id & 0x00FF_FFFF_FFFF_FFFF;
        if !(36..=47).contains(&hi) || lo != btn_id {
            continue;
        }
        let kind = if (36..=43).contains(&hi) {
            "INSET(front)"
        } else {
            "OUTER(back)"
        };
        println!(
            "[synth {}] node_id={} (hi={}) sort_key={} mask_ctx={} shadow_params=[half=({:.1},{:.1}) r={:.1} σ={:.1} inset={:.0}]",
            kind, rn.node_id, hi, rn.sort_key, rn.mask_context.0,
            rn.shadow_params[0], rn.shadow_params[1], rn.shadow_params[2], rn.shadow_params[3], rn.shadow_params[4]
        );
        dump_mesh(rn, "  ");
    }
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
