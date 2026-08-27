//! #29 solve 基准：增量 solve（稳态帧 / 单点变更帧）vs 每帧全重建（坑 186 基线）。
//! 场景形状 ≈ api-infra 微缩窗 demo（8 窗 × 75 文本行，~2400 节点、文本叶子为主）。
//! 跑法：`cargo bench -p ikat_core`；`cargo test` 不执行 bench（CI 零负担）。

use criterion::{criterion_group, criterion_main, Criterion};
use ikat_core::layout::{solve, solve_rebuild, ImageSizeTable};
use ikat_core::scene::node::{NodeKind, Scene};
use ikat_core::style::resolved::ResolvedStyle;
use ikat_core::text::layout::FontTable;
use taffy::prelude::*;

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
    root_style.taffy_style.flex_direction = taffy::style::FlexDirection::Row; // 窗并排
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
            Some(format!("Window #{w} — api-infra 微缩窗基准")),
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

fn bench_solve(c: &mut Criterion) {
    let fonts = font_table();
    let root = (1280.0_f32, 720.0);
    let sizes = ImageSizeTable::new();

    // 稳态帧：场景零变更——增量 solve 应只剩期望态 diff 开销（无 relayout）。
    {
        let mut scene = api_infra_shape();
        solve(&mut scene, &fonts, root, &sizes);
        solve(&mut scene, &fonts, root, &sizes); // 预热：首帧全量建树、二帧起稳态
        c.bench_function("solve/steady_frame", |b| {
            b.iter(|| solve(&mut scene, &fonts, root, &sizes))
        });
    }
    // 单点变更帧：每迭代改一个容器的宽 → set_style + 受影响子树局部 relayout。
    {
        let mut scene = api_infra_shape();
        solve(&mut scene, &fonts, root, &sizes);
        let nodes: Vec<_> = scene.nodes.values().map(|n| n.id).collect();
        let mut cursor = 0usize;
        c.bench_function("solve/one_style_change", |b| {
            b.iter(|| {
                let id = nodes[cursor % nodes.len()];
                cursor += 1;
                scene
                    .get_live_mut(id, "bench/change")
                    .style
                    .taffy_style
                    .size
                    .width = Dimension::length(40.0 + (cursor % 200) as f32);
                solve(&mut scene, &fonts, root, &sizes)
            })
        });
    }
    // 全重建基线（坑 186 旧路径）：每迭代从零建 taffy 树 + 全量 compute。
    {
        let mut scene = api_infra_shape();
        solve(&mut scene, &fonts, root, &sizes);
        c.bench_function("solve_rebuild/every_frame", |b| {
            b.iter(|| solve_rebuild(&mut scene, &fonts, root, &sizes))
        });
    }
}

criterion_group!(benches, bench_solve);
criterion_main!(benches);
