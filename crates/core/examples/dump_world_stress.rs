//! #109 P6 压测：500 血条（track+fill 双节点）世界锚点场景的 core 侧成本。
//!
//! 四个工况（各 120 帧取均值/最差）：
//! 1. steady    —— 零变更稳态帧（A2 增量 render 全命中态；对照 forced full rebuild）
//! 2. fill      —— 每帧 50 条 fill 宽度 inline override（扣血/回血交互形态）
//! 3. hidden    —— 每帧 50 条整条 render_hidden 翻转（出屏/回屏自动隐藏形态）
//! 4. follow    —— 每帧 500 条 track set_user_transform（世界锚点投影跟随形态——
//!    C# 侧 SetWorldAnchor→Transform.Position flush 的 core 落点同款）
//!
//! 运行：cargo run -p yio_core --example dump_world_stress --release

use std::time::Instant;
use yio_core::scene::dynamic::{
    append_child, set_inline_override, set_node_render_hidden, set_user_transform,
};
use yio_core::stage::Stage;
use yio_core::transform::NodeTransform;

const BARS: usize = 500;
const COLS: usize = 25;
const FRAMES: usize = 120;

fn main() {
    let mut s = Stage::new((1920.0, 1080.0)).expect("Stage");
    let root = s
        .create_root("div", "width:1920px;height:1080px")
        .expect("root");

    let mut tracks = Vec::with_capacity(BARS);
    let mut fills = Vec::with_capacity(BARS);
    for i in 0..BARS {
        let left = (i % COLS) * 74 + 16;
        let top = (i / COLS) * 52 + 16;
        let track = s
            .create_node(
                "div",
                &format!(
                    "position:absolute;left:{left}px;top:{top}px;width:120px;height:10px;\
                     background-color:#1a2836;border-radius:5px"
                ),
            )
            .unwrap();
        let fill = s
            .create_node(
                "div",
                "position:absolute;left:0;top:0;width:120px;height:100%;background-color:#e05555;border-radius:4px",
            )
            .unwrap();
        {
            let sc = s.scene.as_mut().unwrap();
            append_child(sc, root, track).unwrap();
            append_child(sc, track, fill).unwrap();
        }
        tracks.push(track);
        fills.push(fill);
    }
    for _ in 0..5 {
        let _ = s.tick_and_render();
    }
    let nodes = s.scene.as_ref().unwrap().nodes.len();
    println!("nodes = {nodes} ({BARS} bars × track+fill + root)");

    // 1. steady：零变更稳态（A2 全命中）vs 强制全量重建。
    let (mut mean, mut worst) = (0.0f64, 0.0f64);
    for _ in 0..FRAMES {
        let t = Instant::now();
        let _ = s.tick_and_render();
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        mean += ms;
        worst = worst.max(ms);
    }
    println!(
        "steady(增量)      mean {:6.2} ms  worst {:6.2} ms",
        mean / FRAMES as f64,
        worst
    );

    s.incremental_render = false;
    let (mut mean, mut worst) = (0.0f64, 0.0f64);
    for _ in 0..FRAMES {
        let t = Instant::now();
        let _ = s.tick_and_render();
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        mean += ms;
        worst = worst.max(ms);
    }
    println!(
        "steady(全量重建)  mean {:6.2} ms  worst {:6.2} ms",
        mean / FRAMES as f64,
        worst
    );
    s.incremental_render = true;

    // 2. fill churn：每帧 50 条 fill 宽度 override（滑动窗口走遍 500 条）。
    let (mut mean, mut worst, mut base) = (0.0f64, 0.0f64, 0usize);
    for f in 0..FRAMES {
        let sc = s.scene.as_mut().unwrap();
        for k in 0..50 {
            let idx = (base + k) % BARS;
            let w = 36 + ((idx + f) % 85);
            set_inline_override(sc, fills[idx], &format!("width:{w}px")).unwrap();
        }
        base = (base + 50) % BARS;
        let t = Instant::now();
        let _ = s.tick_and_render();
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        mean += ms;
        worst = worst.max(ms);
    }
    println!(
        "fill×50/帧        mean {:6.2} ms  worst {:6.2} ms",
        mean / FRAMES as f64,
        worst
    );

    // 3. hidden churn：每帧 50 条整条隐藏翻转（出屏/回屏自动隐藏）。
    let (mut mean, mut worst) = (0.0f64, 0.0f64);
    for f in 0..FRAMES {
        let sc = s.scene.as_mut().unwrap();
        for k in 0..50 {
            let idx = (f * 50 + k) % BARS;
            set_node_render_hidden(sc, tracks[idx], (f + idx).is_multiple_of(2)).unwrap();
        }
        let t = Instant::now();
        let _ = s.tick_and_render();
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        mean += ms;
        worst = worst.max(ms);
    }
    println!(
        "hidden×50/帧      mean {:6.2} ms  worst {:6.2} ms",
        mean / FRAMES as f64,
        worst
    );
    // 清干净隐藏态再进 follow 工况。
    {
        let sc = s.scene.as_mut().unwrap();
        for &t in &tracks {
            set_node_render_hidden(sc, t, false).unwrap();
        }
    }

    // 4. follow：每帧 500 条 track set_user_transform（世界锚点投影跟随；positions 模拟
    //    圆周轨道让 world matrix 每帧真变——指纹含 wm，500 条全 miss 重建才是真实上界）。
    let (mut mean, mut worst) = (0.0f64, 0.0f64);
    for f in 0..FRAMES {
        let sc = s.scene.as_mut().unwrap();
        let a = f as f32 * 0.05;
        for (i, &tr) in tracks.iter().enumerate() {
            let t = NodeTransform {
                translate: [
                    (i as f32 * 3.7 + a.sin() * 40.0) % 1800.0,
                    (i as f32 * 2.1 + a.cos() * 30.0) % 1000.0,
                ],
                ..Default::default()
            };
            set_user_transform(sc, tr, t).unwrap();
        }
        let t = Instant::now();
        let _ = s.tick_and_render();
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        mean += ms;
        worst = worst.max(ms);
    }
    println!(
        "follow×500/帧     mean {:6.2} ms  worst {:6.2} ms",
        mean / FRAMES as f64,
        worst
    );

    // 稳态阶段拆分（归因用）：solve / 世界矩阵 / 全 tick。
    let (mut t_solve, mut t_world, mut t_full) = (0.0f64, 0.0f64, 0.0f64);
    let n = 30usize;
    for _ in 0..n {
        let t = Instant::now();
        {
            let host = s.host.borrow();
            yio_core::layout::solve(
                s.scene.as_mut().unwrap(),
                &host.fonts,
                s.root_size,
                s.safe_insets,
                &host.image_sizes,
            );
        }
        t_solve += t.elapsed().as_secs_f64() * 1000.0;
        let t = Instant::now();
        yio_core::scene::transform::compute_world_transforms(s.scene.as_mut().unwrap());
        t_world += t.elapsed().as_secs_f64() * 1000.0;
        let t = Instant::now();
        let _ = s.tick_and_render();
        t_full += t.elapsed().as_secs_f64() * 1000.0;
    }
    let d = n as f64;
    println!("── 稳态阶段均值（{n} 帧）──");
    println!("solve          {:6.2} ms", t_solve / d);
    println!("world_transf   {:6.2} ms", t_world / d);
    println!("FULL tick      {:6.2} ms", t_full / d);
    println!("DONE — no panic");
}
