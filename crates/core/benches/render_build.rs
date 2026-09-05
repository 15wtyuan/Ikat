//! #109 A2 render build 基准：增量 build（输入指纹命中复用）vs 每帧全量重建。
//! 场景复用 solve.rs 的 api-infra 形状（~2400 节点、文本叶子为主）。
//! 跑法：`cargo bench -p yio_core`；`cargo test` 不执行 bench（CI 零负担）。

use criterion::{criterion_group, criterion_main, Criterion};
use yio_core::layout::{solve, ImageSizeTable};
use yio_core::render::{build_render_nodes, build_render_nodes_cached, dirty::RenderBuildCache};
use yio_core::scene::node::{NodeKind, Scene};
use yio_core::style::resolved::ResolvedStyle;
use yio_core::text::layout::FontTable;

fn font_table() -> FontTable {
    let path = format!(
        "{}/tests/fixtures/DejaVuSans.ttf",
        env!("CARGO_MANIFEST_DIR")
    );
    let bytes = std::fs::read(&path).expect("fixture font");
    let mut ft = FontTable::new();
    ft.register("DejaVu", bytes, true).expect("register font");
    ft
}

/// Scene::build 的 entry 形状（10 元组，含 data_controller 死槽）。
type Entry = (
    Option<usize>,
    NodeKind,
    ResolvedStyle,
    Vec<String>,
    Option<String>,
    bool,
    Option<i32>,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// 8 窗 × 75 行（行容器 + label 文本 + spacer + value 文本），~2400 节点。
fn api_infra_shape() -> Scene {
    let mut entries: Vec<Entry> = Vec::new();
    let mut root_style = ResolvedStyle::default();
    root_style.taffy_style.display = taffy::style::Display::Flex;
    root_style.taffy_style.flex_direction = taffy::style::FlexDirection::Row;
    entries.push((
        None,
        NodeKind::Container,
        root_style,
        Vec::new(),
        None,
        false,
        None,
        None,
        None,
        None,
    ));
    for w in 0..8 {
        let win_idx = entries.len();
        let mut win = ResolvedStyle::default();
        win.taffy_style.display = taffy::style::Display::Flex;
        win.taffy_style.flex_direction = taffy::style::FlexDirection::Column;
        win.taffy_style.flex_grow = 1.0;
        let pad = taffy::style::LengthPercentage::length(8.0);
        win.taffy_style.padding.left = pad;
        win.taffy_style.padding.right = pad;
        win.taffy_style.padding.top = pad;
        win.taffy_style.padding.bottom = pad;
        entries.push((
            Some(0),
            NodeKind::Container,
            win,
            Vec::new(),
            None,
            false,
            None,
            None,
            None,
            None,
        ));
        entries.push((
            Some(win_idx),
            NodeKind::TextNode,
            ResolvedStyle::default(),
            Vec::new(),
            None,
            false,
            None,
            None,
            Some(format!("Window #{w} — render build 基准")),
            None,
        ));
        for r in 0..75 {
            let row_idx = entries.len();
            let mut row = ResolvedStyle::default();
            row.taffy_style.display = taffy::style::Display::Flex;
            row.taffy_style.flex_direction = taffy::style::FlexDirection::Row;
            entries.push((
                Some(win_idx),
                NodeKind::Container,
                row,
                Vec::new(),
                None,
                false,
                None,
                None,
                None,
                None,
            ));
            entries.push((
                Some(row_idx),
                NodeKind::TextNode,
                ResolvedStyle::default(),
                Vec::new(),
                None,
                false,
                None,
                None,
                Some(format!("label.{r:03}")),
                None,
            ));
            entries.push((
                Some(row_idx),
                NodeKind::Container,
                ResolvedStyle::default(),
                Vec::new(),
                None,
                false,
                None,
                None,
                None,
                None,
            ));
            entries.push((
                Some(row_idx),
                NodeKind::TextNode,
                ResolvedStyle::default(),
                Vec::new(),
                None,
                false,
                None,
                None,
                Some(format!("value #{r:03} = {}", r * 31 % 997)),
                None,
            ));
        }
    }
    Scene::build(&entries)
}

fn bench_render_build(c: &mut Criterion) {
    let fonts = font_table();
    let root = (1280.0_f32, 720.0);
    let sizes = ImageSizeTable::new();
    let mut atlas = yio_core::text::atlas::GlyphAtlas::new();

    // 稳态帧（增量命中路径）：预热两帧建缓存与字形，之后每帧全部指纹命中。
    {
        let mut scene = api_infra_shape();
        solve(&mut scene, &fonts, root, [0.0; 4], &sizes);
        yio_core::scene::transform::compute_world_transforms(&mut scene);
        let mut cache = RenderBuildCache::default();
        let prev = std::collections::HashMap::new();
        build_render_nodes_cached(&scene, &fonts, &prev, &sizes, &mut atlas, &mut cache, 0, 1);
        // 第二帧（基线 hash 建立后的稳态起点）。
        let (_, prev, _) = build_render_nodes_cached(
            &scene,
            &fonts,
            &std::collections::HashMap::new(),
            &sizes,
            &mut atlas,
            &mut cache,
            0,
            2,
        );
        c.bench_function("render_build/steady_cached", |b| {
            b.iter(|| {
                build_render_nodes_cached(
                    &scene, &fonts, &prev, &sizes, &mut atlas, &mut cache, 0, 3,
                )
            })
        });
    }
    // 全量重建基线（A2 前行为）：一次性 cache（每帧 miss）。
    {
        let mut scene = api_infra_shape();
        solve(&mut scene, &fonts, root, [0.0; 4], &sizes);
        yio_core::scene::transform::compute_world_transforms(&mut scene);
        let prev = std::collections::HashMap::new();
        c.bench_function("render_build/steady_full_rebuild", |b| {
            b.iter(|| build_render_nodes(&scene, &fonts, &prev, &sizes, &mut atlas))
        });
    }
}

criterion_group!(benches, bench_render_build);
criterion_main!(benches);
