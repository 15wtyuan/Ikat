//! 诊断：把 showcase `page_text.html` 跑过核心全管线（parse→style→scene→layout→render），
//! dump 所有文本渲染节点的 mesh 数据 + atlas 脏页。不依赖 Unity/pkg——直接 HTML+CSS+字体。
//!
//! 目的：定位"字渲染成实心色块"。看 core 发的 mesh 数据本身是否异常：
//!   - image_path 是否合法（loomgui://font-atlas/p{page}，page 在脏页集合里）
//!   - UV 是否是紧凑字形子区（多字文本 vert 数应 ≈ 4×字数；只有 4 = 退化成单 quad）
//!   - program 是否 =1（text），per-vertex color 是否 = 文字 color
//!
//! 跑：`cargo run -p loomgui_core --example dump_showcase_text`

use loomgui_core::parse::css::parse_css;
use loomgui_core::parse::dom::parse_html;
use loomgui_core::render::node::NodePayload;
use loomgui_core::scene::node::{build_scene, NodeId, NodeKind};
use loomgui_core::stage::Stage;
use loomgui_core::style::cascade::resolve_styles;

fn main() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let html_path = format!("{}/../../showcase_project/showcase/home.html", manifest);
    let base_css_path = format!(
        "{}/../../showcase_project/showcase/preview/preview-base.css",
        manifest
    );
    let font_path = format!("{}/tests/fixtures/LXGWWenKai.ttf", manifest);

    let html = std::fs::read_to_string(&html_path).expect("read page_text.html");
    let base_css = std::fs::read_to_string(&base_css_path).expect("read preview-base.css");
    let inline_css = extract_style(&html);
    let css = format!("{}\n{}", base_css, inline_css);

    let mut stage = Stage::new((1080.0, 1920.0)).expect("Stage::new");
    stage
        .register_font(
            "LXGWWenKai",
            std::fs::read(&font_path).expect("read font"),
            true,
        )
        .unwrap();

    let tree = parse_html(&html).expect("parse_html");
    let sheet = parse_css(&css).expect("parse_css");
    let styles = resolve_styles(&tree, &sheet);
    stage.tweens.clear();
    stage.prev_node_hashes.clear();
    stage.scene = Some(build_scene(&tree, &styles));

    let frame = stage.tick_and_render();

    let dirty: Vec<u32> = stage.glyph_atlas.dirty_pages().to_vec();
    println!(
        "=== atlas dirty pages: {:?}（count={}）===",
        dirty,
        dirty.len()
    );
    let (atlas_bytes, atlas_w, atlas_h) = if let Some(&p0) = dirty.first() {
        let (bytes, pw, ph) = stage.glyph_atlas.page_bytes(p0);
        let mx = bytes.iter().copied().max().unwrap_or(0);
        let mn = bytes.iter().copied().min().unwrap_or(0);
        let sum: u64 = bytes.iter().map(|&b| b as u64).sum();
        let mean = sum as f64 / bytes.len().max(1) as f64;
        let inside = bytes.iter().filter(|&&b| b > 140).count();
        println!(
            "=== page{} core bytes: len={} min={} max={} mean={:.1} inside(>140)={} ===",
            p0,
            bytes.len(),
            mn,
            mx,
            mean,
            inside
        );
        (bytes.to_vec(), pw, ph)
    } else {
        (Vec::new(), 0u32, 0u32)
    };

    let mut first_text_uvs: Option<Vec<[f32; 2]>> = None;

    let scene = stage.scene.as_ref().expect("scene");

    println!("\n=== 全部渲染节点 program 分布 ===");
    let mut prog_counts = std::collections::HashMap::<u32, usize>::new();
    for rn in &frame.nodes {
        let NodePayload::Mesh { program, .. } = &rn.payload;
        *prog_counts.entry(*program).or_default() += 1;
    }
    for (p, c) in &prog_counts {
        println!("  program={} : {} 节点", p, c);
    }

    println!("\n=== 文本渲染节点（program=1 / font-atlas path）===");
    let mut text_count = 0;
    for rn in &frame.nodes {
        let (verts, uvs, colors, image_path, program) = {
            let NodePayload::Mesh {
                verts,
                uvs,
                colors,
                image_path,
                program,
                ..
            } = &rn.payload;
            (verts, uvs, colors, image_path, program)
        };
        let is_text = *program == 1
            || image_path
                .as_deref()
                .is_some_and(|p| p.starts_with("loomgui://font-atlas"));
        if !is_text {
            continue;
        }
        text_count += 1;
        if first_text_uvs.is_none() {
            first_text_uvs = Some(uvs.clone());
        }

        let (id_attr, content_snip, font_size, css_color, kind_tag) = match scene
            .get(NodeId(rn.node_id))
        {
            Some(n) => {
                let (snip, ktag) = match &n.kind {
                    NodeKind::Text { content } => {
                        (content.chars().take(18).collect::<String>(), "Text")
                    }
                    NodeKind::RichText { runs } => (format!("[rich {} runs]", runs.len()), "Rich"),
                    _ => ("?".into(), "?"),
                };
                (
                    n.id_attr.clone().unwrap_or_default(),
                    snip,
                    n.style.font_size,
                    n.style.color,
                    ktag,
                )
            }
            None => (
                "<synth?>".to_string(),
                String::new(),
                0.0,
                [0.0; 4],
                "synth",
            ),
        };

        let (umin, umax, vmin, vmax) = uvs.iter().fold(
            (f32::MAX, f32::MIN, f32::MAX, f32::MIN),
            |(a, b, c, d), &[u, v]| (a.min(u), b.max(u), c.min(v), d.max(v)),
        );
        let fc = colors.first().copied().unwrap_or([0.0; 4]);
        let path = image_path.as_deref().unwrap_or("None");
        let page_in_dirty = path
            .rsplit('p')
            .next()
            .and_then(|s| s.parse::<u32>().ok())
            .is_some_and(|pg| dirty.contains(&pg));
        println!(
            "id={:<10} kind={:<5} prog={} mask={} path={:<28} verts={:>3} UV=({:.3},{:.3})-({:.3},{:.3}) vcol0=[{:.2},{:.2},{:.2},{:.2}] tint=[{:.2},{:.2},{:.2},{:.2}] fs={:.0} csscolor=[{:.2},{:.2},{:.2},{:.2}] pg_dirty={} content={:?}",
            id_attr,
            kind_tag,
            program,
            rn.mask_context.0,
            path,
            verts.len(),
            umin,
            vmin,
            umax,
            vmax,
            fc[0],
            fc[1],
            fc[2],
            fc[3],
            rn.color_tint[0],
            rn.color_tint[1],
            rn.color_tint[2],
            rn.color_tint[3],
            font_size,
            css_color[0],
            css_color[1],
            css_color[2],
            css_color[3],
            page_in_dirty,
            content_snip,
        );
    }
    println!("\n文本节点数：{}", text_count);

    // 首 glyph emit UV → 采样 core atlas 像素，验 emit UV 是否对准字形 inside 区。
    // >140 有值 → emit UV 对准字形（core 对，问题在 Unity v-flip/采样）；全 <140 → emit UV 没对准（core UV/atlas 错）。
    if let Some(uvs) = &first_text_uvs {
        if !atlas_bytes.is_empty() && uvs.len() >= 4 {
            let w = atlas_w as usize;
            let (gu0, gv0) = (uvs[0][0], uvs[0][1]);
            let (gu1, gv1) = (uvs[2][0], uvs[2][1]);
            let (lo_u, hi_u) = (gu0.min(gu1), gu0.max(gu1));
            let (lo_v, hi_v) = (gv0.min(gv1), gv0.max(gv1));
            let px_x0 = (lo_u * atlas_w as f32) as usize;
            let px_x1 = ((hi_u * atlas_w as f32) as usize + 1).min(atlas_w as usize);
            let px_y0 = (lo_v * atlas_h as f32) as usize;
            let px_y1 = ((hi_v * atlas_h as f32) as usize + 1).min(atlas_h as usize);
            let mut gmin = 255u8;
            let mut gmax = 0u8;
            let mut g140 = 0i32;
            let mut gcnt = 0i32;
            for y in px_y0..px_y1 {
                for x in px_x0..px_x1 {
                    let b = atlas_bytes[y * w + x];
                    if b < gmin {
                        gmin = b;
                    }
                    if b > gmax {
                        gmax = b;
                    }
                    if b > 140 {
                        g140 += 1;
                    }
                    gcnt += 1;
                }
            }
            println!(
                "=== 首 glyph emitUV({:.4},{:.4})-({:.4},{:.4}) → atlasPx[{},{}]-[{},{}]: min={} max={} >140={}/{} ===",
                lo_u, lo_v, hi_u, hi_v, px_x0, px_y0, px_x1, px_y1, gmin, gmax, g140, gcnt
            );
            // 笔画 bounding + 固定 UV(0.005,0.007)=atlas(20,28) 实际值
            let mut bmin_x = 999i32;
            let mut bmin_y = 999i32;
            let mut bmax_x = -1i32;
            let mut bmax_y = -1i32;
            for y in px_y0..px_y1 {
                for x in px_x0..px_x1 {
                    if atlas_bytes[y * w + x] > 140 {
                        if (x as i32) < bmin_x {
                            bmin_x = x as i32;
                        }
                        if (x as i32) > bmax_x {
                            bmax_x = x as i32;
                        }
                        if (y as i32) < bmin_y {
                            bmin_y = y as i32;
                        }
                        if (y as i32) > bmax_y {
                            bmax_y = y as i32;
                        }
                    }
                }
            }
            let p_20_28 = atlas_bytes.get(28 * w + 20).copied().unwrap_or(0);
            let p_center = atlas_bytes
                .get(((px_y0 + px_y1) / 2) * w + (px_x0 + px_x1) / 2)
                .copied()
                .unwrap_or(0);
            println!(
                "=== 首 glyph 笔画 bounding x[{},{}] y[{},{}]；atlas(20,28)={} atlas(bitmap中心)={} ===",
                bmin_x, bmax_x, bmin_y, bmax_y, p_20_28, p_center
            );
        }
    }
}

fn extract_style(html: &str) -> String {
    if let Some(start) = html.find("<style>") {
        if let Some(end) = html[start..].find("</style>") {
            return html[start + 7..start + end].to_string();
        }
    }
    String::new()
}
