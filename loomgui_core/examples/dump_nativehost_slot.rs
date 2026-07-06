//! T5 诊断：验证 NativeHost FFI 查询通道的核心修复——空 div slot（nh-stage）
//! 即便被 `merge_meshes` 吞掉（不进 `frame.nodes` 渲染 blob），仍可直查
//! `scene.world_transforms` + `scene.node_sort_keys`（FFI 查询独立于 merge）。
//!
//! 这是 Task 1-4 修复的端到端验证：
//! - Task 1：Scene 新增 `node_sort_keys: Vec<u32>`（merge 前的 DFS 序号快照）
//! - Task 2：Stage 3 个 getter（world_transforms / node_sort_keys / visible）
//! - Task 3：FFI 3 个 extern（同上，给 C# P/Invoke）
//! - Task 4：C# SyncNativeHostSlot 调 FFI 查 world_transforms/sort_key/visible
//!
//! 根因：NativeHost 角色挂 nh-stage 节点，需要查它的 world_matrix 决定 GO 位置。
//! 旧实现走 `frame.nodes` blob——但 nh-stage 是空 div（无 mesh payload），被
//! `merge_meshes` 优化吞掉，blob 里没它的 entry → driver 查不到 → 角色 fallback
//! 屏幕角上。修复：FFI 直查 scene 的并行数组（world_transforms / node_sort_keys），
//! 不走 blob。
//!
//! 用法（默认读 worktree 根 showcase.pkg.bin）：
//! ```bash
//! cargo run -p loomgui_core --example dump_nativehost_slot
//! # 或指定 pkg.bin 路径
//! cargo run -p loomgui_core --example dump_nativehost_slot -- <path-to-pkg.bin>
//! ```
use loomgui_core::asset::read_package;
use loomgui_core::scene::node::NodeId;
use loomgui_core::stage::Stage;
use std::env;

fn main() {
    // 默认 pkg 路径：worktree 根的 showcase.pkg.bin（对齐 PlayMode 从 StreamingAssets 加载）。
    // 命令行第 1 参覆盖——参考 verify_showcase_pkg.rs 的 arg 取法。
    let pkg_path = env::args()
        .nth(1)
        .unwrap_or_else(|| "loomgui_unity/Assets/StreamingAssets/showcase.pkg.bin".to_string());
    let bytes =
        std::fs::read(&pkg_path).unwrap_or_else(|e| panic!("read pkg.bin ({pkg_path}): {e}"));
    let pkg = read_package(&bytes).expect("read_package");
    println!(
        "pkg name={:?} components={}",
        pkg.name,
        pkg.components.len()
    );
    assert!(
        pkg.components.contains_key("page_nativehost"),
        "pkg missing page_nativehost component (重打 pkg：cargo run -p loomgui_pkg)"
    );

    let font = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
    let mut s = Stage::new(font, (1080.0, 1920.0)).expect("Stage::new");
    s.load_package("showcase", &bytes)
        .expect("load_package showcase");
    // root 同 driver 实例化页面的容器（同 PlayMode 实际栈）。
    let root = s
        .create_root(
            "div",
            "width:1080px;height:1920px;background-color:#1a1d2e;flex-direction:column",
        )
        .expect("create_root");
    let page = s
        .instantiate("showcase", "page_nativehost")
        .expect("instantiate page_nativehost");
    s.append_child(root, page).expect("append_child");

    // tick 一次让 layout / scroll / compute_world_transforms / build_render_nodes 全跑。
    // world_transforms + node_sort_keys 在 tick 内填（compute_world_transforms + assign_sort_keys）。
    let frame = s.tick_and_render();
    let scene = s.scene.as_ref().expect("scene post-tick");

    // ──────────────────────────────────────────────────────────────────────
    // 1) frame.nodes（blob 通道）—— nh-stage 期望 NOT IN（merge 吞了）
    // ──────────────────────────────────────────────────────────────────────
    let mut nh_in_frame = false;
    let mut frame_ids: Vec<String> = Vec::new();
    for rn in &frame.nodes {
        let n = match scene.get(NodeId(rn.node_id)) {
            Some(n) => n,
            None => continue, // scrollbar thumb sentinel 等
        };
        if let Some(id) = &n.id_attr {
            if id == "nh-stage" {
                nh_in_frame = true;
            }
            frame_ids.push(id.clone());
        }
    }
    println!("\n=== 1) frame.nodes（渲染 blob 通道）===");
    println!("frame.nodes count = {}", frame.nodes.len());
    println!("frame.nodes ids with id_attr = {:?}", frame_ids);
    println!("nh-stage IN frame.nodes? -> {nh_in_frame}");
    println!("  期望：false（空 div 无 mesh payload，被 merge_meshes 吞，不进 blob）");

    // ──────────────────────────────────────────────────────────────────────
    // 2) 直查 scene 并行数组（绕 blob——NativeHost FFI 查询通道）
    // ──────────────────────────────────────────────────────────────────────
    let nh_id = scene
        .find_by_id_attr("nh-stage")
        .expect("nh-stage id_attr not found in scene");
    let wm = scene.world_transforms.get(nh_id.index()).copied();
    let sk = scene.node_sort_keys.get(nh_id.index()).copied();
    println!("\n=== 2) scene 直查（绕 blob——FFI 查询通道）===");
    println!(
        "  world_transforms[{}]={:?} -> tx,ty={:?}",
        nh_id.index(),
        wm,
        wm.map(|m| (m[4], m[5]))
    );
    println!("  node_sort_keys[{}]={:?}", nh_id.index(), sk);
    println!("  期望：tx/ty 非零（slot 落在 nh-stage 框中部）+ sort_key>0（DFS 序号）");

    // 3) page_nativehost 所有节点 sort_key + layout_rect + bg（看 3D GO sortingOrder 在排序中位置
    //    vs stage-wrap 背景 / 按钮 / hint——决定 3D GO 是否被 UI 遮挡）。
    println!("\n=== 3) page 所有节点 sort_key / layout / bg（sortingOrder 全貌）===");
    let mut nodes: Vec<_> = scene.nodes.values().collect();
    nodes.sort_by_key(|n| scene.node_sort_keys.get(n.id.index()).copied().unwrap_or(0));
    for n in &nodes {
        let sk = scene.node_sort_keys.get(n.id.index()).copied().unwrap_or(0);
        let r = n.layout_rect;
        let id = n.id_attr.clone().unwrap_or_else(|| format!("anon{}", n.id.0));
        let bg = n.style.background_color.is_some();
        println!(
            "  sort_key={:>3} id={:<16} rect=({:>5.0},{:>5.0},{:>4.0},{:>4.0}) bg={}",
            sk, id, r.x, r.y, r.w, r.h, bg
        );
    }

    // ──────────────────────────────────────────────────────────────────────
    // 结论
    // ──────────────────────────────────────────────────────────────────────
    let tx_ty = wm.map(|m| (m[4], m[5]));
    let wm_nonzero = tx_ty
        .map(|(tx, ty)| tx != 0.0 || ty != 0.0)
        .unwrap_or(false);
    let sk_positive = sk.map(|k| k > 0).unwrap_or(false);
    let pass = !nh_in_frame && wm_nonzero && sk_positive;
    println!("\n=== 结论 ===");
    println!("  nh-stage NOT IN frame.nodes (merge 吞): {}", !nh_in_frame);
    println!("  world_transforms tx/ty 非零: {wm_nonzero}");
    println!("  node_sort_keys > 0: {sk_positive}");
    println!(
        "  -> {}",
        if pass {
            "PASS：FFI 查询通道独立于 merge——空 div slot 仍可查 world_transforms + sort_key"
        } else {
            "FAIL：检查上方具体字段值"
        }
    );
}
