//! 诊断：实例化 showcase home 页，取证用户报告的 3 个问题：
//!   问题1 (文本换行): hero-sub 段落含 <span class=hero-version>，浏览器一行不换，
//!                     Unity 在 "·" 后换行。dump hero-sub + span 的 layout_rect + 文本测量。
//!   问题2 (色块更深): dump nav-card / 按钮 / quick-chip 的 computed background-color。
//!   问题3 (列表四向拖动+裁剪失效): dump quick-bar 的 overflow_x/y + content/viewport/overlap。
//!
//! 复现 Unity 的 core solve（编码机本地），定位 bug 在 core 还是 Unity 后端。

use yio_core::scene::dynamic::append_child;
use yio_core::scene::node::{NodeKind, Scene};
use yio_core::stage::Stage;
use yio_core::text::layout::measure_text;

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
    // 1920x1080 与 .root 设计尺寸一致（home.html .root width:1920px height:1080px）。
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
    // 图标尺寸：showcase res/icons 都是 128x128 PNG（nav-card-icon / hero-logo / quick-icon）。
    s.set_image_sizes(&icon_sizes());
    s.load_package("showcase", &pkg).expect("load_package");
    let root_id = s.create_root("div", "").expect("create_root");
    let inst = s.instantiate("showcase", "home").expect("instantiate home");
    {
        let scene = s.scene.as_mut().unwrap();
        append_child(scene, root_id, inst).expect("append_child");
    }
    let _frame = s.tick_and_render();
    let scene = s.scene.as_ref().unwrap();

    println!("stage=1920x1080 scene_nodes={}", scene.nodes.len());
    println!();

    issue1_text_wrap(scene, &s);
    issue2_colors(scene);
    issue3_scroll(scene, &s);
}

/// 问题1：hero-sub 文本换行。定位 hero-sub（class 含 hero-sub）+ 其 span（hero-version）
/// 子节点，dump layout_rect + 用 measure_text 测量 intrinsic 宽度（不换行）vs rect.w。
fn issue1_text_wrap(scene: &Scene, s: &Stage) {
    println!("========== 问题1：hero-sub 文本换行 ==========");
    for n in scene.nodes.values() {
        if !n.classes.iter().any(|c| c == "hero-sub") {
            continue;
        }
        let r = n.layout_rect;
        println!(
            "[hero-sub] rect=({:.0},{:.0},{:.0},{:.0})",
            r.x, r.y, r.w, r.h
        );
        // 它的子节点（TextNode "围栏...· " + span.hero-version）
        for c in &n.children {
            dump_text_node(scene, s, *c, "  ");
        }
        // 也把整段文本拼起来测一次（模拟浏览器 inline 流，不按 run 切）
        let full = collect_text(scene, n.id);
        println!("  [full inline text] {:?}", full);
        let st = &n.style;
        let m = measure_text(
            &full,
            st.font_size,
            st.line_height,
            st.letter_spacing,
            st.text_align,
            st.wrap_control(),
            None,
            &s.host.borrow().fonts.stack_for(st.font_family.as_deref()),
            st.color,
            yio_core::text::rich::weight_from_font_weight(st.font_weight),
        );
        println!(
            "  [full intrinsic] width={:.1}  lines={}  (rect.w={:.0} → {})",
            m.text_width,
            m.lines.len(),
            r.w,
            if m.text_width > r.w {
                "超宽→应换行"
            } else {
                "放得下→不应换行"
            },
        );
    }
    println!();
}

/// 问题2：色块背景色。dump nav-card / quick-chip / 按钮的 computed background-color。
fn issue2_colors(scene: &Scene) {
    println!("========== 问题2：色块背景色 ==========");
    println!(
        "{:<14} {:<10} {:<10} {:<14} {:<14}",
        "class", "bg_r", "bg_a", "bg_hex", "rect"
    );
    for n in scene.nodes.values() {
        let interesting = n.classes.iter().any(|c| {
            matches!(
                c.as_str(),
                "nav-card" | "quick-chip" | "btn-primary" | "btn-ghost" | "root"
            )
        });
        if !interesting {
            continue;
        }
        let st = &n.style;
        let r = n.layout_rect;
        match st.background_color {
            Some(bg) => {
                // bg = [r, g, b, a] in 0..1
                let (rr, gg, bb, aa) = (bg[0], bg[1], bg[2], bg[3]);
                let hex = format!(
                    "#{:02x}{:02x}{:02x}",
                    (rr * 255.0) as u8,
                    (gg * 255.0) as u8,
                    (bb * 255.0) as u8
                );
                println!(
                    "{:<14} rgba({:>5.3},{:>5.3},{:>5.3},{:>5.3}) {:<14} ({:.0},{:.0},{:.0},{:.0})",
                    n.classes.join(","),
                    rr,
                    gg,
                    bb,
                    aa,
                    hex,
                    r.x,
                    r.y,
                    r.w,
                    r.h,
                );
            }
            None => {
                println!(
                    "{:<14} bg=None                              ({:.0},{:.0},{:.0},{:.0})",
                    n.classes.join(","),
                    r.x,
                    r.y,
                    r.w,
                    r.h,
                );
            }
        }
    }
    println!();
}

/// 问题3：quick-bar 滚动容器。dump overflow_x/y + content/viewport/overlap。
fn issue3_scroll(scene: &Scene, _s: &Stage) {
    println!("========== 问题3：quick-bar 滚动容器 ==========");
    for n in scene.nodes.values() {
        if !n.classes.iter().any(|c| c == "quick-bar") {
            continue;
        }
        let r = n.layout_rect;
        let st = &n.style;
        println!(
            "[quick-bar] rect=({:.0},{:.0},{:.0},{:.0}) overflow_x={:?} overflow_y={:?}",
            r.x, r.y, r.w, r.h, st.overflow_x, st.overflow_y
        );
        // 子节点 AABB（content_size 来源）
        let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
        let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
        for c in &n.children {
            if let Some(cn) = scene.get(*c) {
                let cr = cn.layout_rect;
                min_x = min_x.min(cr.x);
                min_y = min_y.min(cr.y);
                max_x = max_x.max(cr.x + cr.w);
                max_y = max_y.max(cr.y + cr.h);
            }
        }
        let content = if n.children.is_empty() {
            (0.0, 0.0)
        } else {
            ((max_x - min_x).max(0.0), (max_y - min_y).max(0.0))
        };
        let overlap = ((content.0 - r.w).max(0.0), (content.1 - r.h).max(0.0));
        println!(
            "  children={} content_size=({:.0},{:.0}) viewport=({:.0},{:.0}) overlap=({:.0},{:.0})",
            n.children.len(),
            content.0,
            content.1,
            r.w,
            r.h,
            overlap.0,
            overlap.1
        );
        // scroll 表里实际存的几何（core refresh_content_sizes 写的）
        if let Some(ss) = scene.scroll.get(n.id) {
            println!(
                "  [scroll表] content=({:.0},{:.0}) viewport=({:.0},{:.0}) overlap=({:.0},{:.0})",
                ss.content_size.0,
                ss.content_size.1,
                ss.viewport_size.0,
                ss.viewport_size.1,
                ss.overlap.0,
                ss.overlap.1
            );
        } else {
            println!("  [scroll表] 无（节点未被视为滚动容器！）");
        }
        println!("  [clip_rect] {:?}", n.clip_rect);
        // 检查污染 AABB 的零尺寸子节点
        let zero_kids: Vec<_> = n
            .children
            .iter()
            .filter_map(|c| scene.get(*c))
            .filter(|cn| cn.layout_rect.w == 0.0 && cn.layout_rect.h == 0.0)
            .collect();
        if !zero_kids.is_empty() {
            println!(
                "  [!] 有 {} 个 (0,0,0,0) 子节点污染 content AABB（whitespace TextNode）",
                zero_kids.len()
            );
        }
        // 打印每个 chip 的 rect（看是否单行、是否溢出）
        println!("  chips:");
        for c in &n.children {
            if let Some(cn) = scene.get(*c) {
                let cr = cn.layout_rect;
                println!(
                    "    {:<10} ({:.0},{:.0},{:.0},{:.0})",
                    cn.classes.join(","),
                    cr.x,
                    cr.y,
                    cr.w,
                    cr.h
                );
            }
        }
    }
    println!();
}

fn dump_text_node(scene: &Scene, s: &Stage, id: yio_core::scene::node::NodeId, indent: &str) {
    let n = match scene.get(id) {
        Some(n) => n,
        None => return,
    };
    let r = n.layout_rect;
    let st = &n.style;
    let content = scene.text_contents.get(&id).cloned().unwrap_or_default();
    let cls = if n.kind == NodeKind::TextElement {
        format!("span.{}", n.classes.join(","))
    } else {
        format!("{:?}", n.kind)
    };
    println!(
        "{}{} rect=({:.0},{:.0},{:.0},{:.0}) fs={} nowrap={}",
        indent,
        cls,
        r.x,
        r.y,
        r.w,
        r.h,
        st.font_size,
        st.white_space != yio_core::style::resolved::WhiteSpace::Normal
    );
    println!("{}  content={:?}", indent, content);
    if !content.is_empty() {
        let m = measure_text(
            &content,
            st.font_size,
            st.line_height,
            st.letter_spacing,
            st.text_align,
            st.wrap_control(),
            None,
            &s.host.borrow().fonts.stack_for(st.font_family.as_deref()),
            st.color,
            yio_core::text::rich::weight_from_font_weight(st.font_weight),
        );
        println!(
            "{}  intrinsic width={:.1}  vs rect.w={:.0} → {}",
            indent,
            m.text_width,
            r.w,
            if m.text_width > r.w {
                "超宽(会换行)"
            } else {
                "放得下"
            },
        );
    }
    for c in &n.children {
        dump_text_node(scene, s, *c, &format!("{}  ", indent));
    }
}

fn collect_text(scene: &Scene, id: yio_core::scene::node::NodeId) -> String {
    let mut out = String::new();
    collect_text_rec(scene, id, &mut out);
    out
}

fn collect_text_rec(scene: &Scene, id: yio_core::scene::node::NodeId, out: &mut String) {
    if let Some(n) = scene.get(id) {
        if matches!(n.kind, NodeKind::TextNode | NodeKind::TextElement) {
            if let Some(c) = scene.text_contents.get(&id) {
                out.push_str(c);
            }
        }
        for c in &n.children {
            collect_text_rec(scene, *c, out);
        }
    }
}

/// showcase res/icons 下全是 128x128 PNG（nav-card-icon / hero-logo / quick-icon 引用）。
/// 统一灌 128x128 让 layout 走真实图尺寸（img rect 不再 fallback 64x64）。
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
