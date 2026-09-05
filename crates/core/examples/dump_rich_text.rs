//! 诊断（text-model T9 Phase C）：headless 验证 rich-text-block 的 inline flow 真落地。
//!
//! 读 showcase.pkg.bin（v33），实例化 form + mail，遍历 Scene 找所有 `rich_text_block=true`
//! 的 Container 节点，dump：
//!   ① 容器自身 layout_rect + 直系 inline 子的 layout_rect（验证 inline 子被折进父，
//!      layout_rect=(0,0,0,0)，不再各自当 block item 竖排）。
//!   ② 容器的 TextLayout（scene.text_layouts[parent]）：lines 数 / text_width / text_height /
//!      run_rects 数 —— 验证多 run 拍平成 inline 流 + 宽度受限换行（多行）。
//!
//! 关键判据：
//!   - inline 子 layout_rect=(0,0,0,0)（被折进父，不独立排版）——「折进父」成立。
//!   - TextLayout.lines.len() >= 1 且多 run（run_rects > 1）——inline 流动成立。
//!   - 对长文本块（mail .read-body 各段），lines.len() > 1 ——宽度受限换行成立（非单行溢出）。
//!   - 反例（旧 bug）：inline 子各自 layout_rect 非零 + 竖排堆叠（每个一行）——已消除。

use yio_core::scene::dynamic::append_child;
use yio_core::scene::node::{NodeId, NodeKind, Scene};
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

    for comp in &["form", "mail"] {
        println!("\n################ component: {comp} ################");
        let stage_root = s.create_root("div", "").expect("create_root");
        let inst = s
            .instantiate("showcase", comp)
            .unwrap_or_else(|e| panic!("instantiate {comp}: {e}"));
        {
            let scene = s.scene.as_mut().unwrap();
            append_child(scene, stage_root, inst).expect("append_child");
        }
        // 跑两帧让 layout + text_layouts 落定（rematch → solve → build）。
        let _ = s.tick_and_render();
        let _ = s.tick_and_render();

        let scene = s.scene.as_ref().unwrap();
        dump_rich_blocks(scene, comp);

        // detach 以便下一个组件干净挂载（Stage 单 root 树）。
        s.remove_node(stage_root);
    }

    println!("\n========== 汇总判据 ==========");
    println!("见各组件 dump：rich_text_block 容器的 inline 子 layout_rect 应全 (0,0,0,0)；");
    println!("TextLayout.run_rects 数 > 1 = 多 run 拍平；lines.len() > 1 = 换行（长文本）。");
}

fn dump_rich_blocks(scene: &Scene, comp: &str) {
    // 收集所有 rich_text_block 容器（Container + flag）。先快照 id 列表避免借用问题。
    let rich_ids: Vec<NodeId> = scene
        .nodes
        .values()
        .filter(|n| n.rich_text_block)
        .map(|n| n.id)
        .collect();

    let total_nodes = scene.nodes.len();
    println!(
        "[{comp}] scene nodes={total_nodes}  rich_text_block containers={}",
        rich_ids.len()
    );

    let mut multi_run = 0usize;
    let mut multi_line = 0usize;
    let mut all_children_folded = true;
    let mut shown = 0usize;

    for id in &rich_ids {
        let n = match scene.get(*id) {
            Some(n) => n,
            None => continue,
        };
        let r = n.layout_rect;
        let id_attr = n.id_attr.clone().unwrap_or_default();
        let classes = n.classes.join(".");
        let tag_label = if !classes.is_empty() {
            format!(".{classes}")
        } else if !id_attr.is_empty() {
            format!("#{id_attr}")
        } else {
            "(anonymous)".to_string()
        };

        // inline 子快照
        let children: Vec<(NodeId, String, [f32; 4])> = n
            .children
            .iter()
            .map(|c| {
                let cn = scene.get(*c);
                let kind = cn
                    .map(|x| short_kind(&x.kind))
                    .unwrap_or_else(|| "?".into());
                let cr = cn.map(|x| x.layout_rect).unwrap_or_default();
                (*c, kind, [cr.x, cr.y, cr.w, cr.h])
            })
            .collect();

        // 验证 inline 子是否全折进父（layout_rect=(0,0,0,0)）
        let folded = children.iter().all(|(_, _, r)| r[2] == 0.0 && r[3] == 0.0);
        if !folded {
            all_children_folded = false;
        }

        // TextLayout（inline flow 的产物）
        let layout = scene.text_layouts.get(id.index()).cloned().flatten();
        let (n_lines, n_runs, tw, th) = layout
            .as_ref()
            .map(|l| {
                (
                    l.lines.len(),
                    l.run_rects.len(),
                    l.text_width,
                    l.text_height,
                )
            })
            .unwrap_or((0usize, 0usize, 0.0f32, 0.0f32));
        if n_runs > 1 {
            multi_run += 1;
        }
        if n_lines > 1 {
            multi_line += 1;
        }

        // 拼全段 inline 文本（供肉眼对齐预期）
        let full = collect_text(scene, *id);

        // 只详打前 8 个 + 含多 run 的（避免刷屏；mail .read-body 各段必打）
        let detailed = n_runs > 1 || shown < 8;
        if detailed {
            shown += 1;
            println!(
                "\n  [{tag_label}] rect=({:.0},{:.0},{:.0},{:.0})  children={}  TextLayout: lines={} runs={} tw={:.1} th={:.1}",
                r.x, r.y, r.w, r.h, children.len(), n_lines, n_runs, tw, th
            );
            println!("    inline folded(children rect全0)={folded}");
            for (cid, kind, cr) in &children {
                let folded_flag = cr[2] == 0.0 && cr[3] == 0.0;
                println!(
                    "      - {kind:<10} id={} rect=({:.0},{:.0},{:.0},{:.0}) {}",
                    cid.0,
                    cr[0],
                    cr[1],
                    cr[2],
                    cr[3],
                    if folded_flag {
                        "[folded]"
                    } else {
                        "[NOT folded]"
                    }
                );
            }
            if !full.is_empty() {
                let preview: String = full.chars().take(60).collect();
                println!("    text: {preview:?}");
            }
        }
    }

    println!("\n  [{comp} 汇总] rich_blocks={}  multi_run={}  multi_line={}  all_children_folded={all_children_folded}",
             rich_ids.len(), multi_run, multi_line);
    if !all_children_folded {
        println!("  ⚠ 警告：部分 rich_text_block 的 inline 子 layout_rect 非零 —— 折叠未生效！");
    }
}

fn short_kind(k: &NodeKind) -> String {
    match k {
        NodeKind::TextNode => "TextNode".into(),
        NodeKind::TextElement => "span".into(),
        NodeKind::Image => "img".into(),
        NodeKind::Container => "div".into(),
        NodeKind::Button => "button".into(),
        NodeKind::Template => "template".into(),
        _ => format!("{:?}", k),
    }
}

fn collect_text(scene: &Scene, id: NodeId) -> String {
    let mut out = String::new();
    collect_text_rec(scene, id, &mut out);
    out
}

fn collect_text_rec(scene: &Scene, id: NodeId, out: &mut String) {
    if let Some(n) = scene.get(id) {
        if matches!(n.kind, NodeKind::TextNode) {
            if let Some(c) = scene.text_contents.get(&id) {
                out.push_str(c);
            }
        }
        for c in &n.children {
            collect_text_rec(scene, *c, out);
        }
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
