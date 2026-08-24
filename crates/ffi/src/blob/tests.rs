use super::*;
use loomgui_core::render::ClipEntry;
use loomgui_core::scene::node::Rect;
use loomgui_core::transform::Affine2Ext;

/// 测试层 build_blob：多数用例不涉 list 池，用空 Scene（无 parked slot → 无 keepalive 段）。
/// 本层名遮蔽 glob 导入的 `super::build_blob`；parked 用例直调 `super::build_blob(frame, scene)`。
fn build_blob(frame: &FrameData) -> Vec<u8> {
    super::build_blob(frame, &Scene::default())
}

/// 把 nodes 包成无 clip 的 FrameData（多数 blob 测试不需要 clip 表）。
fn frame(nodes: &[RenderNode]) -> FrameData {
    FrameData {
        nodes: nodes.to_vec(),
        clips: Vec::new(),
    }
}

fn mesh_node(id: u32, parent: Option<u32>, x: f32, y: f32, w: f32, h: f32) -> RenderNode {
    RenderNode {
        node_id: id,
        parent_id: parent,
        visible: true,
        alpha: 1.0,
        color_tint: [1.0; 4],
        world_matrix: transform::from_translate(x, y),
        blend: BlendMode::Normal,
        mask_context: MaskContext(0),
        sort_key: id,
        change_level: ChangeLevel::Full,
        reuse_key: 0,
        effect: EffectBlock::default(),
        shadow_params: [0.0; 6],
        gradient: loomgui_core::render::gradient::GradientParams::default(),
        payload: NodePayload::Mesh {
            // 父坐标系顶点：(x,y)(x+w,y)(x+w,y+h)(x,y+h)
            verts: vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            colors: vec![[1.0; 4]; 4],
            indices: vec![0, 1, 2, 0, 2, 3],
            image_path: None, // v7：纯色 mesh（无图）
            program: 0,
            color_matrix: [0.0; 20],
        },
    }
}

/// 同 mesh_node 但可指定 image_path（验 path_idx 列 round-trip）。
/// path=None → idx=0；path=Some(p) → idx>0。
fn mesh_node_with_path(id: u32, path: Option<&str>) -> RenderNode {
    let mut n = mesh_node(id, None, 0.0, 0.0, 5.0, 5.0);
    let NodePayload::Mesh { image_path, .. } = &mut n.payload;
    *image_path = path.map(|s| s.to_string());
    n
}

/// 同 mesh_node 但可指定 program（验 program 列 round-trip）。program 是着色契约：
/// shader 乘法 tint（tex×vcol）在图透明区不透 bg-color，img 与 Container+bg-image
/// 共用纹理须靠 program 分流（色图共存 = 加法合成 tex.rgb×tex.a + vcol.rgb×(1-tex.a)）。
fn mesh_node_with_program(id: u32, program: u32) -> RenderNode {
    let mut n = mesh_node(id, None, 0.0, 0.0, 5.0, 5.0);
    let NodePayload::Mesh { program: p, .. } = &mut n.payload;
    *p = program;
    n
}

/// 同 mesh_node 但可指定 color_tint / alpha / vertex colors（用于 alpha 不烘焙测试，alpha 走 _Alpha uniform）。
fn mesh_node_tinted(id: u32, tint: [f32; 4], alpha: f32, bg: [f32; 4]) -> RenderNode {
    RenderNode {
        node_id: id,
        parent_id: None,
        visible: true,
        alpha,
        color_tint: tint,
        world_matrix: transform::IDENTITY,
        blend: BlendMode::Normal,
        mask_context: MaskContext(0),
        sort_key: id,
        change_level: ChangeLevel::Full,
        reuse_key: 0,
        effect: EffectBlock::default(),
        shadow_params: [0.0; 6],
        gradient: loomgui_core::render::gradient::GradientParams::default(),
        payload: NodePayload::Mesh {
            verts: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            colors: vec![bg; 4],
            indices: vec![0, 1, 2, 0, 2, 3],
            image_path: None, // v7：纯色 mesh（tint 测试不需图）
            program: 0,
            color_matrix: [0.0; 20],
        },
    }
}

/// 构造任意 mesh payload 节点（变顶点）——border-radius 圆角 mesh round-trip 测试用。
fn mesh_node_raw(verts: Vec<[f32; 2]>, indices: Vec<u32>, tx: f32, ty: f32) -> RenderNode {
    let n = verts.len();
    RenderNode {
        node_id: 0,
        parent_id: None,
        visible: true,
        alpha: 1.0,
        color_tint: [1.0; 4],
        world_matrix: transform::from_translate(tx, ty),
        blend: BlendMode::Normal,
        mask_context: MaskContext(0),
        sort_key: 0,
        change_level: ChangeLevel::Full,
        reuse_key: 0,
        effect: EffectBlock::default(),
        shadow_params: [0.0; 6],
        gradient: loomgui_core::render::gradient::GradientParams::default(),
        payload: NodePayload::Mesh {
            verts,
            uvs: vec![[0.0, 0.0]; n],
            colors: vec![[1.0; 4]; n],
            indices,
            image_path: None, // v7：纯色 mesh
            program: 0,
            color_matrix: [0.0; 20],
        },
    }
}

#[test]
fn rounded_rect_mesh_round_trips_n_verts() {
    // border-radius 产的 37 顶点圆角 mesh 经 build_blob 序列化 + TestView 反序列化：
    // vert_count / idx_count / 顶点坐标 re-base 全保真（验证变顶点 FFI 链）。
    use loomgui_core::scene::node::Rect;
    let rect = Rect {
        x: 10.0,
        y: 20.0,
        w: 80.0,
        h: 80.0,
    };
    let (verts, _uvs, _colors, indices) = loomgui_core::render::mesh::rounded_rect(
        &rect,
        [1.0; 4],
        &[(8.0, 8.0); 4],
        [0.0, 0.0],
        [1.0, 1.0],
    );
    assert_eq!(verts.len(), 37, "r=8 80×80 → 37 顶点（/4 加密分段）");
    let ic = indices.len();
    let node = mesh_node_raw(verts, indices, 10.0, 20.0);
    let blob = build_blob(&frame(&[node]));
    let view = TestView::parse(&blob);
    assert_eq!(view.payload_kind(0), 1, "Mesh kind=1");
    let (vc, ic2) = view.mesh_vert_count(0);
    assert_eq!(vc, 37, "vert_count round-trip 37");
    assert_eq!(ic2 as usize, ic, "idx_count round-trip");
    // 中心顶点 v[0] = rect 中心 (50,60)，re-base 减 (tx=10,ty=20) → (40,40)。
    let (cx, cy) = view.mesh_vert(0, 0);
    assert!(
        (cx - 40.0).abs() < 1e-4 && (cy - 40.0).abs() < 1e-4,
        "中心顶点 re-base (40,40)，得 ({},{})",
        cx,
        cy
    );
}

/// §3.1 双类条目：build_blob 在 render 条目后追加每个 parked slot 的 keepalive 条目。
/// parked slot 走 display:none 不进 render 管线，靠这段告诉后端镜像池「休眠别销毁」。
/// 契约：node_count = render 条目数 + parked 数；parked 条目 visible 字节 = 0b10
/// （bit1 置位、bit0 清）、reuse_key > 0（永久 ordinal）、mesh_len = 0（无 mesh）。
#[test]
fn blob_emits_parked_keepalive_entries() {
    use loomgui_core::list::{ListState, Slot};
    use loomgui_core::scene::dynamic;

    // 场景：ul + 3 个 slot，其中 2 个 parked、1 个 active（active 不产 keepalive 条目）。
    let mut scene = Scene::default();
    let ul = dynamic::create_root(&mut scene, "div", "").unwrap();
    let mut slots = Vec::new();
    for (i, parked) in [true, false, true].into_iter().enumerate() {
        let node = dynamic::create_node(&mut scene, "div", "").unwrap();
        dynamic::append_child(&mut scene, ul, node).unwrap();
        dynamic::set_reuse_key(&mut scene, node, 0x0001_0000 | (i as u32));
        slots.push(Slot {
            node,
            item_index: i,
            parked,
        });
    }
    let mut want: Vec<u32> = slots
        .iter()
        .filter(|s| s.parked)
        .map(|s| s.node.0)
        .collect();
    want.sort_unstable();
    scene.lists.0.insert(
        ul,
        ListState {
            slots,
            ..Default::default()
        },
    );

    // render 条目 2 个（active 管线产物，reuse_key=0 的普通节点）。
    let blob = super::build_blob(
        &frame(&[
            mesh_node(1, None, 0.0, 0.0, 5.0, 5.0),
            mesh_node(2, None, 0.0, 0.0, 5.0, 5.0),
        ]),
        &scene,
    );
    let view = TestView::parse(&blob);
    assert_eq!(
        view.node_count(),
        2 + 2,
        "node_count = render 条目 2 + parked keepalive 2"
    );
    assert_eq!(
        view.version(),
        13,
        "parked 复用 visible 字节 bit1，不 bump version"
    );

    let parked: Vec<usize> = (0..view.node_count() as usize)
        .filter(|&i| view.parked(i))
        .collect();
    assert_eq!(parked.len(), 2, "恰 2 条 parked keepalive");
    for &i in &parked {
        assert!(!view.visible(i), "parked 条目 bit0 必清（不渲染）");
        assert!(
            view.reuse_key(i) > 0,
            "parked 条目带非零 reuse_key（后端据此认镜像对象）"
        );
        assert_eq!(view.mesh_len_col(i), 0, "parked 无 mesh");
        assert_eq!(view.payload_kind(i), 0, "parked payload_kind=0");
        assert_eq!(view.change_level(i), 0, "parked change_level=Skip");
        assert_eq!(view.parent_id(i), -1, "parked 不参与父子渲染关系");
    }
    let mut got: Vec<u32> = parked.iter().map(|&i| view.node_id(i)).collect();
    got.sort_unstable();
    assert_eq!(got, want, "keepalive 条目的 node_id 即 parked slot 节点");

    // active 条目（前 2 条）不受影响：bit0 置位、bit1 清。
    for i in 0..2 {
        assert!(view.visible(i), "render 条目 bit0 置位");
        assert!(!view.parked(i), "render 条目 bit1 必清");
    }
}

/// parked slot 的 keepalive 必须覆盖整子树（根 + 后代），不止根。否则 park 剪子树后
/// 后代 GO（文本 mesh 等）被后端 stale 销毁，reactivate 重建，每帧滚动 churn。
#[test]
fn blob_emits_parked_keepalive_for_slot_subtree() {
    use loomgui_core::list::{ListState, Slot};
    use loomgui_core::scene::dynamic;

    // 场景：1 个 parked slot，子树含 2 个 div 后代（模拟 mail-item 的 dot + body）。
    let mut scene = Scene::default();
    let ul = dynamic::create_root(&mut scene, "div", "").unwrap();
    let slot = dynamic::create_node(&mut scene, "div", "").unwrap();
    dynamic::append_child(&mut scene, ul, slot).unwrap();
    dynamic::set_reuse_key(&mut scene, slot, 0x0001_0000); // 根 reuse_key（永久 ordinal）
    let dot = dynamic::create_node(&mut scene, "div", "").unwrap();
    dynamic::append_child(&mut scene, slot, dot).unwrap();
    let body = dynamic::create_node(&mut scene, "div", "").unwrap();
    dynamic::append_child(&mut scene, slot, body).unwrap();
    // 后代不挂 reuse_key（runtime 只挂 slot 根）

    scene.lists.0.insert(
        ul,
        ListState {
            slots: vec![Slot {
                node: slot,
                item_index: 0,
                parked: true,
            }],
            ..Default::default()
        },
    );

    let blob = super::build_blob(&frame(&[]), &scene); // 无 active render 节点
    let view = TestView::parse(&blob);

    let mut parked_ids: Vec<u32> = (0..view.node_count() as usize)
        .filter(|&i| view.parked(i))
        .map(|i| view.node_id(i))
        .collect();
    parked_ids.sort_unstable();
    let mut want = [slot.0, dot.0, body.0];
    want.sort_unstable();
    assert_eq!(
        parked_ids.len(),
        3,
        "keepalive 覆盖整子树（根 + 2 后代），不止根"
    );
    assert_eq!(parked_ids, want, "根 + 两后代都发 keepalive");

    // 根带 reuse_key，后代 reuse_key=0（后端按 node_id 保留）
    for i in 0..view.node_count() as usize {
        if !view.parked(i) {
            continue;
        }
        let nid = view.node_id(i);
        let rk = view.reuse_key(i);
        if nid == slot.0 {
            assert_eq!(rk, 0x0001_0000, "slot 根带永久 reuse_key");
        } else {
            assert_eq!(rk, 0, "后代 reuse_key=0（后端按 node_id 池化）");
        }
    }
}

/// 无 parked slot 时零追加：全 active 的 list 不产 keepalive 条目（node_count 不胀）。
#[test]
fn blob_no_keepalive_when_all_slots_active() {
    use loomgui_core::list::{ListState, Slot};
    use loomgui_core::scene::dynamic;

    let mut scene = Scene::default();
    let ul = dynamic::create_root(&mut scene, "div", "").unwrap();
    let node = dynamic::create_node(&mut scene, "div", "").unwrap();
    dynamic::append_child(&mut scene, ul, node).unwrap();
    dynamic::set_reuse_key(&mut scene, node, 0x0001_0000);
    scene.lists.0.insert(
        ul,
        ListState {
            slots: vec![Slot {
                node,
                item_index: 0,
                parked: false,
            }],
            ..Default::default()
        },
    );

    let blob = super::build_blob(&frame(&[mesh_node(1, None, 0.0, 0.0, 5.0, 5.0)]), &scene);
    let view = TestView::parse(&blob);
    assert_eq!(view.node_count(), 1, "无 parked slot → 零追加");
    assert!(!view.parked(0));
    assert!(view.visible(0));
}

#[test]
fn build_blob_has_magic_and_count() {
    let blob = build_blob(&frame(&[mesh_node(0, None, 10.0, 20.0, 5.0, 5.0)]));
    assert_eq!(&blob[0..4], &MAGIC.to_le_bytes());
    let v = u32::from_le_bytes(blob[4..8].try_into().unwrap());
    assert_eq!(v, VERSION);
    assert_eq!(v, 13, "blob 版本应为 13（v13：加 grad_params 列）");
    let n = u32::from_le_bytes(blob[8..12].try_into().unwrap());
    assert_eq!(n, 1);
}

/// v13：grad_params 列（第 23 列，[u8;208]）round-trip——program=6 渐变节点参数
/// 全量保真，非渐变节点恒全零。
#[test]
fn grad_params_column_round_trips() {
    let mut grad = loomgui_core::render::gradient::GradientParams {
        kind: 1,
        angle_deg: 137.0,
        dir: [0.68, -0.73],
        t0: -70.7,
        inv_span: 0.00707,
        center: [1574.4, -129.6],
        radii: [1100.0, 560.0],
        stop_count: 3,
        ..Default::default()
    };
    grad.stops[0] = [0.37, 0.71, 0.83, 0.1, 0.0];
    grad.stops[1] = [0.1, 0.2, 0.3, 0.5, 0.6];
    grad.stops[2] = [0.0, 0.0, 0.0, 0.0, 1.0];

    let mut grad_node = mesh_node_with_program(0, 6);
    grad_node.gradient = grad;
    let plain = mesh_node_with_program(1, 0);

    let blob = build_blob(&frame(&[grad_node, plain]));
    let view = TestView::parse(&blob);
    // 渐变节点：208B 列 round-trip 全保真（from_bytes 对照）。
    let off = view.col_off[22];
    let back =
        loomgui_core::render::gradient::GradientParams::from_bytes(&view.buf[off..off + 208]);
    assert_eq!(back, grad, "grad_params 208B 列 round-trip");
    // 非渐变节点：列偏移 + stride 后仍全零（default 序列化 = 全零字节）。
    let off1 = view.col_off[22] + 208;
    assert!(
        view.buf[off1..off1 + 208].iter().all(|&b| b == 0),
        "非渐变节点 grad_params 恒全零"
    );
}

/// path_idx 列（第 18 列，u32，v7）round-trip。
///   Some(path) → idx>0（path 已 intern 进 path string table，1-based）；
///   None（纯色）→ idx=0。同 path 复用同一 idx（去重）。
#[test]
fn path_idx_column_round_trips() {
    // 三节点：Some(path) / None / Some(同 path)（验去重 → idx 复用）。
    let blob = build_blob(&frame(&[
        mesh_node_with_path(0, Some("icons/skin.png")),
        mesh_node_with_path(1, None),                   // 纯色 → idx=0
        mesh_node_with_path(2, Some("icons/skin.png")), // 同 path → idx 复用
    ]));
    let view = TestView::parse(&blob);
    let idx0 = view.path_idx(0);
    assert!(idx0 > 0, "节点 0 Some(path) → path_idx 应 > 0，实={}", idx0);
    assert_eq!(view.path_idx(1), 0, "节点 1 None（纯色）→ path_idx=0");
    assert_eq!(view.path_idx(2), idx0, "节点 2 同 path → idx 复用（去重）");
    // path string table round-trip：读回第 idx0 条 path == "icons/skin.png"。
    let path = view.read_path(idx0).expect("path_idx>0 应能读回 path");
    assert_eq!(path, "icons/skin.png", "path string table round-trip");
    assert_eq!(view.path_count(), 1, "单 path 去重后 path_count=1");
}

/// program 列（u8，第 19 列，v5）：Mesh program=2（Container+bg-image 合成）/ 纯色 mesh program=0。
/// round-trip。
#[test]
fn program_column_round_trips() {
    let blob = build_blob(&frame(&[
        mesh_node_with_program(0, 2),           // Container+bg-image 命中
        mesh_node(1, None, 0.0, 0.0, 5.0, 5.0), // 纯色 mesh program=0 占位
        mesh_node_with_program(2, 0),           // 无图 Container / Image
    ]));
    let view = TestView::parse(&blob);
    assert_eq!(view.version(), 13, "VERSION=13（v13：加 grad_params 列）");
    assert_eq!(view.program(0), 2, "Mesh program=2 round-trip");
    assert_eq!(view.program(1), 0, "Mesh program=0 占位");
    assert_eq!(view.program(2), 0, "Mesh program=0 round-trip");
}

/// §4.1 v13 header：23 col offsets + mesh/clip/path 三 arena header。
/// 无 clip 时 clip 表仅 4B clip_count=0，
/// 无 image_path 时 path table 仅 4B path_count=0。
#[test]
fn blob_header_has_text_and_clip_arena_fields() {
    let blob = build_blob(&frame(&[mesh_node(0, None, 0.0, 0.0, 1.0, 1.0)]));

    assert_eq!(u32::from_le_bytes(blob[0..4].try_into().unwrap()), MAGIC);
    assert_eq!(
        u32::from_le_bytes(blob[4..8].try_into().unwrap()),
        13,
        "version=13"
    );

    // 23 col offset @ [12 .. 12+23*4)。每 col_offset 非零且单调递增。
    let header_len = 12 + 23 * 4; // = 104
    let mut prev = header_len;
    for i in 0..23usize {
        let o = 12 + i * 4;
        let off = u32::from_le_bytes(blob[o..o + 4].try_into().unwrap()) as usize;
        assert!(
            off >= prev,
            "col_offset[{}] 应 >= header_len({}), 实={}",
            i,
            prev,
            off
        );
        prev = off;
    }

    // mesh_arena header @ [104..112)：off/len（mesh 节点有内容，len>0）。
    let mesh_arena_off = u32::from_le_bytes(blob[104..108].try_into().unwrap()) as usize;
    let mesh_arena_len = u32::from_le_bytes(blob[108..112].try_into().unwrap()) as usize;
    assert!(mesh_arena_len > 0, "单 mesh 节点：mesh_arena_len 应 > 0");

    // clip_table header @ [112..120)：clip 紧跟 mesh。无 clip 时仅 4B clip_count=0。
    let clip_table_off = u32::from_le_bytes(blob[112..116].try_into().unwrap()) as usize;
    let clip_table_len = u32::from_le_bytes(blob[116..120].try_into().unwrap());
    assert_eq!(
        clip_table_len, 4,
        "clip 表至少含 clip_count(u32)=0，故 len=4"
    );
    assert_eq!(
        clip_table_off,
        mesh_arena_off + mesh_arena_len,
        "clip_table 紧跟 mesh_arena"
    );
    let clip_count =
        u32::from_le_bytes(blob[clip_table_off..clip_table_off + 4].try_into().unwrap());
    assert_eq!(clip_count, 0, "clip_count=0");

    // v7 path_table header @ [120..128)：无 image_path 时仅 4B path_count=0。
    let path_table_off = u32::from_le_bytes(blob[120..124].try_into().unwrap()) as usize;
    let path_table_len = u32::from_le_bytes(blob[124..128].try_into().unwrap());
    assert_eq!(
        path_table_len, 4,
        "无 image_path：path table 仅 path_count=0，len=4"
    );
    assert_eq!(
        path_table_off,
        clip_table_off + clip_table_len as usize,
        "path_table 紧跟 clip_table"
    );
    let path_count =
        u32::from_le_bytes(blob[path_table_off..path_table_off + 4].try_into().unwrap());
    assert_eq!(path_count, 0, "path_count=0");
    assert_eq!(
        path_table_off + path_table_len as usize,
        blob.len(),
        "path_table 应是 blob 末段"
    );
}

/// clip_count=0 占位验证。
#[test]
fn test_view_parses_layout_and_text_placeholders() {
    let blob = build_blob(&frame(&[mesh_node(0, None, 0.0, 0.0, 1.0, 1.0)]));
    let view = TestView::parse(&blob);
    assert_eq!(view.clip_count(), 0, "clip_count=0");
    assert_eq!(view.payload_kind(0), 1, "Mesh payload_kind=1");
    assert_eq!(view.version(), 13, "VERSION=13");
}

#[test]
fn mesh_verts_are_rebased_to_local() {
    // 顶点原本在 (10,20)..(15,25)；world_matrix 纯平移 (10,20) → re-base 后应 (0,0)..(5,5)。
    let blob = build_blob(&frame(&[mesh_node(0, None, 10.0, 20.0, 5.0, 5.0)]));
    let view = TestView::parse(&blob);
    let verts = view.mesh_verts(0);
    assert_eq!(verts[0], [0.0, 0.0]);
    assert_eq!(verts[2], [5.0, 5.0]);
    // m_tx/m_ty 列保留平移分量（10,20），供 GO 本地置放。
    let mtx = f32::from_le_bytes(
        view.buf[view.col_off[10]..view.col_off[10] + 4]
            .try_into()
            .unwrap(),
    );
    let mty = f32::from_le_bytes(
        view.buf[view.col_off[11]..view.col_off[11] + 4]
            .try_into()
            .unwrap(),
    );
    assert_eq!(mtx, 10.0);
    assert_eq!(mty, 20.0);
}

#[test]
fn parent_id_minus_one_for_none() {
    let blob = build_blob(&frame(&[mesh_node(0, None, 0.0, 0.0, 1.0, 1.0)]));
    let view = TestView::parse(&blob);
    assert_eq!(view.parent_id(0), -1);
}

/// §4.2b：mesh 顶点色 = background_color（**不乘 color_tint**——那是前景/文本色，
/// 默认黑，乘了会把红背景涂黑），仅 alpha 分量 × node opacity。shader 做 tex2D*v.color。
/// tint=[0.5,0.5,0.5,1.0]（应被忽略）alpha=0.5 bg=[1,0,0,1]
/// → 首顶点色 = [1.0, 0.0, 0.0, 1.0×0.5] = [1.0, 0.0, 0.0, 0.5]（红，半透明）。
#[test]
fn mesh_colors_no_longer_bake_alpha() {
    // alpha 剥离后：colors.a = 原始 bg.a（不乘节点 alpha）。节点 alpha 走 _Alpha uniform。
    let blob = build_blob(&frame(&[mesh_node_tinted(
        0,
        [0.5, 0.5, 0.5, 1.0],
        0.5,
        [1.0, 0.0, 0.0, 1.0],
    )]));
    let view = TestView::parse(&blob);
    let colors = view.mesh_colors(0);
    assert_eq!(colors.len(), 4);
    assert_eq!(
        colors[0],
        [1.0, 0.0, 0.0, 1.0],
        "colors.a=原始1.0（alpha 0.5 不烤，走 uniform）"
    );
    // alpha 列仍保留 0.5（供 C# SetPropertyBlock _Alpha）。
    let alpha_o = view.col_off[3];
    assert_eq!(
        f32::from_le_bytes(view.buf[alpha_o..alpha_o + 4].try_into().unwrap()),
        0.5
    );
}

// col_off 索引：0=node_id 1=parent_id 2=visible 3=alpha 4=sort_key
//              5=mask_context 6=m_a 7=m_b 8=m_c 9=m_d 10=m_tx 11=m_ty
//              12=payload_kind 13=mesh_off 14=mesh_len
//              15=path_idx (v7) 16=program (v5) 17=color_matrix (v6)
//              18=change_level (v8) 19=reuse_key (v9) 20=effect_block (v11)
//              21=shadow_params (v12) 22=grad_params (v13)
// v13：加 grad_params 列（22→23）。
struct TestView<'a> {
    buf: &'a [u8],
    col_off: [usize; 23],
    mesh_arena_off: usize,
    clip_table_off: usize,
    clip_table_len: u32,
    path_table_off: usize, // v7
    path_table_len: u32,   // v7
}
impl<'a> TestView<'a> {
    fn parse(buf: &'a [u8]) -> Self {
        assert_eq!(&buf[0..4], &MAGIC.to_le_bytes());
        let mut col_off = [0usize; 23];
        let mut h = 12;
        for i in 0..23 {
            col_off[i] = u32::from_le_bytes(buf[h..h + 4].try_into().unwrap()) as usize;
            h += 4;
        }
        let mesh_arena_off = u32::from_le_bytes(buf[h..h + 4].try_into().unwrap()) as usize;
        h += 4;
        let _mesh_arena_len = u32::from_le_bytes(buf[h..h + 4].try_into().unwrap());
        h += 4;
        // v10：text_arena 已删 → 跳过
        let clip_table_off = u32::from_le_bytes(buf[h..h + 4].try_into().unwrap()) as usize;
        h += 4;
        let clip_table_len = u32::from_le_bytes(buf[h..h + 4].try_into().unwrap());
        h += 4;
        let path_table_off = u32::from_le_bytes(buf[h..h + 4].try_into().unwrap()) as usize;
        h += 4;
        let path_table_len = u32::from_le_bytes(buf[h..h + 4].try_into().unwrap());
        TestView {
            buf,
            col_off,
            mesh_arena_off,
            clip_table_off,
            clip_table_len,
            path_table_off,
            path_table_len,
        }
    }
    fn parent_id(&self, i: usize) -> i32 {
        let o = self.col_off[1] + i * 4;
        i32::from_le_bytes(self.buf[o..o + 4].try_into().unwrap())
    }
    /// 第 1 列 node_id（u32）。
    fn node_id(&self, i: usize) -> u32 {
        let o = self.col_off[0] + i * 4;
        u32::from_le_bytes(self.buf[o..o + 4].try_into().unwrap())
    }
    /// 第 3 列 visible 字节 bit0：本帧要渲染（镐 C# FrameBlob.Visible）。
    fn visible(&self, i: usize) -> bool {
        self.buf[self.col_off[2] + i] & 0x01 != 0
    }
    /// 第 3 列 visible 字节 bit1：parked keepalive（留镜像对象、不渲染，镐 C# FrameBlob.Parked）。
    fn parked(&self, i: usize) -> bool {
        self.buf[self.col_off[2] + i] & 0x02 != 0
    }
    /// 读节点 i 的 mesh 顶点（arena 段：vert_count, idx_count, verts[], uvs[], colors[], indices[]）。
    fn mesh_verts(&self, i: usize) -> Vec<[f32; 2]> {
        let (seg, vc) = self.mesh_seg(i);
        let mut p = seg + 8; // 跳 vert_count + idx_count，直接读 verts[]
        (0..vc)
            .map(|_| {
                let vx = f32::from_le_bytes(self.buf[p..p + 4].try_into().unwrap());
                p += 4;
                let vy = f32::from_le_bytes(self.buf[p..p + 4].try_into().unwrap());
                p += 4;
                [vx, vy]
            })
            .collect()
    }
    /// 读节点 i 的 mesh 顶点色（alpha 剥离：不乘节点 alpha，alpha 走 _Alpha uniform）。
    fn mesh_colors(&self, i: usize) -> Vec<[f32; 4]> {
        let (seg, vc) = self.mesh_seg(i);
        let mut p = seg + 8;
        // verts + uvs 各 vc*2 f32。
        p += vc * 2 * 4 * 2;
        (0..vc)
            .map(|_| {
                let r = f32::from_le_bytes(self.buf[p..p + 4].try_into().unwrap());
                p += 4;
                let g = f32::from_le_bytes(self.buf[p..p + 4].try_into().unwrap());
                p += 4;
                let b = f32::from_le_bytes(self.buf[p..p + 4].try_into().unwrap());
                p += 4;
                let a = f32::from_le_bytes(self.buf[p..p + 4].try_into().unwrap());
                p += 4;
                [r, g, b, a]
            })
            .collect()
    }
    /// 返回节点 i 的 mesh 段起始偏移 + vert_count。
    fn mesh_seg(&self, i: usize) -> (usize, usize) {
        let seg = self.mesh_arena_off
            + u32::from_le_bytes(
                self.buf[self.col_off[13] + i * 4..][0..4]
                    .try_into()
                    .unwrap(),
            ) as usize; // mesh_off
        let vc = u32::from_le_bytes(self.buf[seg..seg + 4].try_into().unwrap()) as usize;
        (seg, vc)
    }
    /// v7：第 15 列 path_idx（u32，v10 删 text_off/text_len 后从第 17→15）。Mesh→path 表 1-based 索引（0=纯色无图）。
    fn path_idx(&self, i: usize) -> u32 {
        u32::from_le_bytes(
            self.buf[self.col_off[15] + i * 4..][0..4]
                .try_into()
                .unwrap(),
        )
    }
    /// v7：path string table 的 path_count（path table 首 4B）。
    fn path_count(&self) -> u32 {
        if self.path_table_len >= 4 {
            u32::from_le_bytes(
                self.buf[self.path_table_off..self.path_table_off + 4]
                    .try_into()
                    .unwrap(),
            )
        } else {
            0
        }
    }
    /// v7：读 path string table 第 idx（1-based）条 path。
    ///   idx=0 → None（纯色无图）；idx>0 → 读 path_table 内第 idx 条 length-prefixed UTF-8。
    ///   table layout：path_count:u32 后跟 count × {path_len:u32, path_bytes:u8[path_len]}。
    fn read_path(&self, idx: u32) -> Option<String> {
        if idx == 0 {
            return None;
        }
        let count = self.path_count();
        assert!(idx <= count, "path_idx {} 超出 path_count {}", idx, count);
        let mut p = self.path_table_off + 4; // 跳 path_count
                                             // 顺序扫到第 idx 条（1-based）。
        for n in 1..=idx {
            let len = u32::from_le_bytes(self.buf[p..p + 4].try_into().unwrap()) as usize;
            p += 4;
            if n == idx {
                let bytes = &self.buf[p..p + len];
                return Some(std::str::from_utf8(bytes).ok()?.to_string());
            }
            p += len;
        }
        None
    }
    /// v5：第 16 列 program（u8，v10 删 text_off/text_len 后从第 18→16）。0=img/无图 Container，1=Text，2=Container+bg-image，3=filter无bg-image，4=filter+bg-image。
    fn program(&self, i: usize) -> u8 {
        self.buf[self.col_off[16] + i]
    }
    /// v6：第 17 列 color_matrix（[f32;20]，col_off[17] + i*80）。program≠3/4 全零。
    fn color_matrix(&self, i: usize) -> [f32; 20] {
        let off = self.col_off[17] + i * 80;
        let mut m = [0.0; 20];
        for j in 0..20 {
            m[j] = f32::from_le_bytes(self.buf[off + j * 4..off + j * 4 + 4].try_into().unwrap());
        }
        m
    }
    /// v8：第 18 列 change_level（u8，col_off[18] + i，v10 删 text_off/text_len 后从第 20→18）。
    fn change_level(&self, i: usize) -> u8 {
        self.buf[self.col_off[18] + i]
    }
    /// v9：第 19 列 reuse_key（u32，col_off[19] + i*4，v10 删 text_off/text_len 后从第 21→19）。
    fn reuse_key(&self, i: usize) -> u32 {
        let o = self.col_off[19] + i * 4;
        u32::from_le_bytes(self.buf[o..o + 4].try_into().unwrap())
    }
    /// v11：第 20 列 effect_block（[u8;128]，col_off[20] + i*128）。读节点 i 第 j 个 f32
    /// （小端）。字段顺序见 EffectBlock::to_bytes：outline_width / outline_color[4] /
    /// underlay[3]×{ox,oy,softness,color[4]} / glow_power / glow_color[4] / blur_width。
    fn effect_block_f32(&self, i: usize, j: usize) -> f32 {
        let off = self.col_off[20] + i * EffectBlock::SIZE + j * 4;
        f32::from_le_bytes(self.buf[off..off + 4].try_into().unwrap())
    }
    /// v12：第 21 列 shadow_params（[f32;6]，col_off[21] + i*24 + j*4）。box-shadow SDF 参数
    /// （halfSize.xy,radius,σ,inset,_pad）。非 shadow 节点 = [0.0;6]（全零占位）。
    fn shadow_params_f32(&self, i: usize, j: usize) -> f32 {
        let off = self.col_off[21] + i * 24 + j * 4;
        f32::from_le_bytes(self.buf[off..off + 4].try_into().unwrap())
    }
    /// v8：读 mesh_len 列（u32 @ col_off[14] + i*4），SKIP/HEADER 节点 =0。
    fn mesh_len_col(&self, i: usize) -> u32 {
        u32::from_le_bytes(
            self.buf[self.col_off[14] + i * 4..][0..4]
                .try_into()
                .unwrap(),
        )
    }
    /// blob VERSION（u32 @ offset 4）。
    fn version(&self) -> u32 {
        u32::from_le_bytes(self.buf[4..8].try_into().unwrap())
    }
    /// 节点数（从 header 读 n:u32 @ offset 8）。
    fn node_count(&self) -> u32 {
        u32::from_le_bytes(self.buf[8..12].try_into().unwrap())
    }
    /// 节点 i 的 payload_kind（u8 列，col_off[12] + i*1）。
    fn payload_kind(&self, i: usize) -> u8 {
        self.buf[self.col_off[12] + i]
    }
    /// 节点 i 的 mesh segment vert_count + idx_count（segment 首 8B）。
    fn mesh_vert_count(&self, i: usize) -> (u32, u32) {
        let (seg, _vc) = self.mesh_seg(i);
        let vc = u32::from_le_bytes(self.buf[seg..seg + 4].try_into().unwrap());
        let ic = u32::from_le_bytes(self.buf[seg + 4..seg + 8].try_into().unwrap());
        (vc, ic)
    }
    /// 节点 i 的第 vi 个 mesh 顶点 (vx, vy)（已 re-base 后的本地坐标）。
    fn mesh_vert(&self, i: usize, vi: usize) -> (f32, f32) {
        let (seg, _vc) = self.mesh_seg(i);
        // seg+8 起为 verts[vc×2 f32]；第 vi 顶点位于 seg+8 + vi*2*4。
        let p = seg + 8 + vi * 2 * 4;
        let vx = f32::from_le_bytes(self.buf[p..p + 4].try_into().unwrap());
        let vy = f32::from_le_bytes(self.buf[p + 4..p + 8].try_into().unwrap());
        (vx, vy)
    }
    /// 节点 i 的第 vi 个 mesh 顶点色的 alpha 分量（alpha 剥离后为原始值，不乘节点 alpha，走 _Alpha uniform）。
    fn mesh_color_alpha(&self, i: usize, vi: usize) -> f32 {
        let (seg, vc) = self.mesh_seg(i);
        // seg+8 起 verts[vc×2] + uvs[vc×2] 各 vc*2*4 = vc*2*4*2，colors 起。
        let colors_off = seg + 8 + vc * 2 * 4 * 2;
        // 每色 f32×4 = 16B；第 vi 顶点的 alpha 在 colors_off + vi*16 + 12。
        let a_off = colors_off + vi * 16 + 12;
        f32::from_le_bytes(self.buf[a_off..a_off + 4].try_into().unwrap())
    }
    fn clip_count(&self) -> u32 {
        if self.clip_table_len >= 4 {
            u32::from_le_bytes(
                self.buf[self.clip_table_off..self.clip_table_off + 4]
                    .try_into()
                    .unwrap(),
            )
        } else {
            0
        }
    }
    /// 读 clip 表 entries：Vec<(context_id, Rect, Option<[(f32,f32);4]>)>（§4.1 / §4.4）。
    /// layout: clip_count:u32 后跟 count × {context_id:u32, x,y,w,h:f32, radii: 4×(rx,ry):f32}（52B/entry）。
    /// radii 全零 → None（直角 clip）；非全零 → Some（圆角 SDF clip）。
    fn read_clips(&self) -> Vec<(u32, Rect, Option<[(f32, f32); 4]>)> {
        let count = self.clip_count() as usize;
        let mut p = self.clip_table_off + 4;
        (0..count)
            .map(|_| {
                let cid = u32::from_le_bytes(self.buf[p..p + 4].try_into().unwrap());
                p += 4;
                let x = f32::from_le_bytes(self.buf[p..p + 4].try_into().unwrap());
                p += 4;
                let y = f32::from_le_bytes(self.buf[p..p + 4].try_into().unwrap());
                p += 4;
                let w = f32::from_le_bytes(self.buf[p..p + 4].try_into().unwrap());
                p += 4;
                let h = f32::from_le_bytes(self.buf[p..p + 4].try_into().unwrap());
                p += 4;
                let mut radii = [(0.0f32, 0.0f32); 4];
                for corner in radii.iter_mut() {
                    let rx = f32::from_le_bytes(self.buf[p..p + 4].try_into().unwrap());
                    p += 4;
                    let ry = f32::from_le_bytes(self.buf[p..p + 4].try_into().unwrap());
                    p += 4;
                    *corner = (rx, ry);
                }
                let all_zero = radii.iter().all(|&(rx, ry)| rx == 0.0 && ry == 0.0);
                let radii_opt = if all_zero { None } else { Some(radii) };
                (cid, Rect { x, y, w, h }, radii_opt)
            })
            .collect()
    }
}

/// §4.4 / §4.1：clip 表 round-trip——context_id + 交集绝对 rect 序列化进 blob 末段。
/// 构造 FrameData 带 2 个 clip entry（含一个零面积 disjoint 交集），读回值正确；
/// 且 mask_context==0 永不入表（context 从 1 起）。
#[test]
fn clip_table_round_trip_with_entries() {
    let node = mesh_node(0, None, 0.0, 0.0, 1.0, 1.0);
    let frame = FrameData {
        nodes: vec![node],
        clips: vec![
            ClipEntry {
                context_id: 1,
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 100.0,
                },
                radii: None,
            },
            ClipEntry {
                context_id: 2,
                rect: Rect {
                    x: 50.0,
                    y: 50.0,
                    w: 0.0,
                    h: 0.0,
                },
                radii: None,
            },
        ],
    };
    let blob = build_blob(&frame);
    let view = TestView::parse(&blob);
    assert_eq!(view.clip_count(), 2, "clip_count == 2");
    let clips = view.read_clips();
    assert_eq!(clips.len(), 2);
    assert_eq!(clips[0].0, 1);
    assert_eq!(
        (clips[0].1.x, clips[0].1.y, clips[0].1.w, clips[0].1.h),
        (0.0, 0.0, 100.0, 100.0)
    );
    assert_eq!(clips[1].0, 2);
    // 零面积 disjoint 交集 round-trip（w/h=0）。
    assert_eq!(
        (clips[1].1.x, clips[1].1.y, clips[1].1.w, clips[1].1.h),
        (50.0, 50.0, 0.0, 0.0)
    );
    // radii=None → 序列化为全零 32B，读回仍 None（直角 clip）。
    assert!(clips[0].2.is_none(), "radii=None round-trip 为 None");
    assert!(clips[1].2.is_none(), "radii=None round-trip 为 None");
    // clip 表段长度 = 4(count) + 2×52(entry: ctx+rect 20B + radii 32B) = 108。
    assert_eq!(view.clip_table_len, 108, "clip_table_len = 4 + count×52");
    // v7：path_table 紧跟 clip_table 之后，是 blob 末段。
    //   本测试 mesh 无 image_path → path_table 仅 4B（path_count=0）。
    assert_eq!(
        view.path_table_off,
        view.clip_table_off + view.clip_table_len as usize,
        "path_table 紧跟 clip_table"
    );
    assert_eq!(
        view.path_table_len, 4,
        "无 image_path：path_table 仅 path_count=0，len=4"
    );
    assert_eq!(
        view.path_table_off + view.path_table_len as usize,
        blob.len(),
        "path_table 应是 blob 末段"
    );
}

/// 空 clip 表（无 overflow:hidden）：clip_count=0，clip_table_len=4（仅 count 占位）。
#[test]
fn empty_clip_table_round_trip() {
    let blob = build_blob(&frame(&[mesh_node(0, None, 0.0, 0.0, 1.0, 1.0)]));
    let view = TestView::parse(&blob);
    assert_eq!(view.clip_count(), 0);
    assert_eq!(
        view.clip_table_len, 4,
        "空 clip 表 len=4（仅 clip_count=0）"
    );
    assert_eq!(view.read_clips().len(), 0);
}

/// 圆角 clip entry round-trip：radii=Some([(rx,ry);4]) 序列化为 32B（4×(rx,ry)），
/// 读回值保真。验四角独立 (rx,ry) 对均正确透传（TL,TR,BR,BL 序）。
#[test]
fn clip_table_radii_round_trip() {
    let node = mesh_node(0, None, 0.0, 0.0, 1.0, 1.0);
    let radii = [
        (10.0, 12.0), // TL
        (20.0, 22.0), // TR
        (30.0, 32.0), // BR
        (40.0, 42.0), // BL
    ];
    let frame = FrameData {
        nodes: vec![node],
        clips: vec![ClipEntry {
            context_id: 1,
            rect: Rect {
                x: 5.0,
                y: 6.0,
                w: 100.0,
                h: 80.0,
            },
            radii: Some(radii),
        }],
    };
    let blob = build_blob(&frame);
    let view = TestView::parse(&blob);
    assert_eq!(view.clip_count(), 1);
    // entry 52B：clip_table_len = 4 + 1×52 = 56。
    assert_eq!(view.clip_table_len, 56, "圆角 clip entry 52B");
    let clips = view.read_clips();
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].0, 1);
    assert_eq!(
        (clips[0].1.x, clips[0].1.y, clips[0].1.w, clips[0].1.h),
        (5.0, 6.0, 100.0, 80.0)
    );
    let r = clips[0].2.expect("radii 应 Some");
    for i in 0..4 {
        assert!(
            (r[i].0 - radii[i].0).abs() < 1e-5 && (r[i].1 - radii[i].1).abs() < 1e-5,
            "corner[{}] round-trip: 期望 ({},{})，得 ({},{})",
            i,
            radii[i].0,
            radii[i].1,
            r[i].0,
            r[i].1
        );
    }
}

/// merged FrameData（transform=0、alpha=1、多 quad 拼接）经 build_blob，
/// re-base 减 0 = 顶点保持绝对；alpha×1 = 不变。blob 列结构零改（spec §9 硬契约）。
/// merged 由 merge_meshes 产：transform/alpha 已置 (0, 1)。blob 不烤 alpha（走 _Alpha uniform），
/// merged.alpha 走 _Alpha uniform。v - transform(=0) → 顶点保持绝对。
#[test]
fn merged_mesh_blob_keeps_absolute_verts_and_no_double_alpha() {
    // 构造一个 merged 节点：8 verts（2 quad 拼接）、transform=0、alpha=1。
    let merged = RenderNode {
        node_id: 1,
        parent_id: None,
        visible: true,
        alpha: 1.0,
        color_tint: [1.0; 4],
        world_matrix: transform::IDENTITY,
        blend: BlendMode::Normal,
        mask_context: MaskContext(0),
        sort_key: 0,
        change_level: ChangeLevel::Full,
        reuse_key: 0,
        effect: EffectBlock::default(),
        shadow_params: [0.0; 6],
        gradient: loomgui_core::render::gradient::GradientParams::default(),
        payload: NodePayload::Mesh {
            // 顶点已是绝对 design 坐标（merge 不 re-base）；re-base 减 transform(0) = 不变。
            verts: vec![
                [0.0, 0.0],
                [10.0, 0.0],
                [10.0, 10.0],
                [0.0, 10.0],
                [100.0, 0.0],
                [110.0, 0.0],
                [110.0, 10.0],
                [100.0, 10.0],
            ],
            uvs: vec![[0.0, 0.0]; 8],
            // 第二组 alpha 已烤 0.5（模拟 merge_batch 把第二节点 alpha=0.5 乘进 colors.a）。
            colors: vec![
                [1.0, 1.0, 1.0, 1.0],
                [1.0, 1.0, 1.0, 1.0],
                [1.0, 1.0, 1.0, 1.0],
                [1.0, 1.0, 1.0, 1.0],
                [1.0, 1.0, 1.0, 0.5],
                [1.0, 1.0, 1.0, 0.5],
                [1.0, 1.0, 1.0, 0.5],
                [1.0, 1.0, 1.0, 0.5],
            ],
            indices: vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7],
            image_path: Some("merged/atlas.png".into()), // v7：merged 带图 → path_idx>0
            program: 0,
            color_matrix: [0.0; 20],
        },
    };
    let frame = FrameData {
        nodes: vec![merged],
        clips: vec![],
    };
    let buf = build_blob(&frame);
    let view = TestView::parse(&buf);
    assert_eq!(view.node_count(), 1);
    assert_eq!(view.payload_kind(0), 1, "merged 仍是 Mesh payload_kind=1");
    // merged 顶点 8 个，re-base 减 0 = 绝对原值。
    let (vc, _ic) = view.mesh_vert_count(0);
    assert_eq!(vc, 8, "merged segment 8 顶点");
    // 第一顶点 = (0,0) 绝对（re-base 减 0）。
    let (vx, vy) = view.mesh_vert(0, 0);
    assert_eq!((vx, vy), (0.0, 0.0));
    // 第五顶点（第二 quad 首）= (100,0) 绝对，证明未 re-base 到本地。
    let (vx5, vy5) = view.mesh_vert(0, 4);
    assert_eq!((vx5, vy5), (100.0, 0.0));
    // 第二组 colors alpha=0.5，blob 再 ×alpha(1.0) = 不变。
    let ca = view.mesh_color_alpha(0, 4);
    assert!((ca - 0.5).abs() < 1e-6, "merged alpha=1 → blob 不二次烤");
    // 顺带验第一组（vi=0..3）alpha=1.0。
    for vi in 0..4 {
        let a = view.mesh_color_alpha(0, vi);
        assert!((a - 1.0).abs() < 1e-6, "第一组 colors.a=1.0");
    }
}
/// blob world_matrix round-trip——纯平移 + 剪切节点均写入 6 矩阵列，blob len > 100。
#[test]
fn blob_world_matrix_roundtrip() {
    let mk = |wm: transform::Affine2| RenderNode {
        node_id: 0,
        parent_id: None,
        visible: true,
        alpha: 1.0,
        color_tint: [1.0; 4],
        world_matrix: wm,
        blend: BlendMode::Normal,
        mask_context: MaskContext(0),
        sort_key: 0,
        change_level: ChangeLevel::Full,
        reuse_key: 0,
        effect: EffectBlock::default(),
        shadow_params: [0.0; 6],
        gradient: loomgui_core::render::gradient::GradientParams::default(),
        payload: NodePayload::Mesh {
            verts: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            uvs: vec![[0.0, 0.0]; 4],
            colors: vec![[1.0; 4]; 4],
            indices: vec![0, 1, 2, 0, 2, 3],
            image_path: None,
            program: 0, // v7：纯色 mesh
            color_matrix: [0.0; 20],
        },
    };
    // 纯平移节点
    let pure = mk(transform::from_translate(5.0, 7.0));
    // 剪切节点
    let skew = mk(transform::from_scale(2.0, 1.0).mul(transform::from_rotate(0.5)));
    let blob = build_blob(&FrameData {
        nodes: vec![pure, skew],
        clips: vec![],
    });
    // version=13（v13：23 列，加 grad_params）
    assert_eq!(
        u32::from_le_bytes(blob[4..8].try_into().unwrap()),
        13,
        "VERSION=13"
    );
    assert!(blob.len() > 100);
}

/// 纯色 mesh 节点经 build_blob → payload_kind==1 透传。
/// C# 侧 MirrorPool.cs `kind!=1&&!=2 continue` 跳过 kind≠1/2。
#[test]
fn blob_pure_mesh_kind_is_one() {
    let rn = mesh_node(0, None, 0.0, 0.0, 1.0, 1.0);
    let frame = FrameData {
        nodes: vec![rn],
        clips: vec![],
    };
    let blob = build_blob(&frame);
    assert!(!blob.is_empty(), "纯色 mesh 节点 blob 非空");
    assert_eq!(&blob[0..4], &MAGIC.to_le_bytes(), "magic");
    let view = TestView::parse(&blob);
    assert_eq!(view.node_count(), 1, "单节点占 1 位");
    assert_eq!(view.payload_kind(0), 1, "纯色 mesh payload_kind==1 透传");
    assert_eq!(view.program(0), 0, "纯色 mesh program=0");
    assert_eq!(
        u32::from_le_bytes(blob[4..8].try_into().unwrap()),
        13,
        "VERSION=13"
    );
}

/// color_matrix 列（[f32;20]，第 18 列 0-indexed）：program=3/4 节点填矩阵，其余全零占位。
#[test]
fn blob_color_matrix_column_round_trips() {
    let matrix = [
        0.299, 0.587, 0.114, 0.0, 0.0, 0.299, 0.587, 0.114, 0.0, 0.0, 0.299, 0.587, 0.114, 0.0,
        0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
    ];
    let nodes = vec![RenderNode {
        node_id: 1,
        parent_id: None,
        visible: true,
        alpha: 1.0,
        color_tint: [1.0; 4],
        world_matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        mask_context: MaskContext(0),
        sort_key: 0,
        blend: BlendMode::Normal,
        change_level: ChangeLevel::Full,
        reuse_key: 0,
        effect: EffectBlock::default(),
        shadow_params: [0.0; 6],
        gradient: loomgui_core::render::gradient::GradientParams::default(),
        payload: NodePayload::Mesh {
            verts: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            uvs: vec![[0.0, 0.0]; 4],
            colors: vec![[1.0; 4]; 4],
            indices: vec![0, 1, 2, 0, 2, 3],
            image_path: None, // v7：纯色
            program: 3,       // ColorFilter
            color_matrix: matrix,
        },
    }];
    let blob = build_blob(&FrameData {
        nodes,
        clips: vec![],
    });
    let view = TestView::parse(&blob);
    assert_eq!(view.version(), 13, "VERSION=13（v13：加 grad_params 列）");
    assert_eq!(view.program(0), 3, "program=3 round-trip");
    let m = view.color_matrix(0);
    for i in 0..20 {
        assert!(
            (m[i] - matrix[i]).abs() < 1e-5,
            "color_matrix[{}] round-trip",
            i
        );
    }
}

/// v8：change_level 列 round-trip（SKIP=0/HEADER=1/FULL=2）+
/// SKIP/HEADER 不写 mesh arena（mesh_len=0），FULL 写（mesh_len>0）。
#[test]
fn change_level_column_round_trips() {
    let mut skip = mesh_node(0, None, 0.0, 0.0, 5.0, 5.0);
    skip.change_level = ChangeLevel::Skip;
    let mut header = mesh_node(1, None, 0.0, 0.0, 5.0, 5.0);
    header.change_level = ChangeLevel::Header;
    let mut full = mesh_node(2, None, 0.0, 0.0, 5.0, 5.0);
    full.change_level = ChangeLevel::Full;
    let blob = build_blob(&frame(&[skip, header, full]));
    let view = TestView::parse(&blob);
    assert_eq!(view.version(), 13, "VERSION=13");
    assert_eq!(view.change_level(0), 0, "Skip=0");
    assert_eq!(view.change_level(1), 1, "Header=1");
    assert_eq!(view.change_level(2), 2, "Full=2");
    // SKIP/HEADER 不写 arena → mesh_len=0；FULL 写 arena → mesh_len>0。
    assert_eq!(view.mesh_len_col(0), 0, "Skip 不写 arena");
    assert_eq!(view.mesh_len_col(1), 0, "Header 不写 arena");
    assert!(view.mesh_len_col(2) > 0, "Full 写 arena");
}

/// v10：reuse_key 列（第 19 列，0-indexed；v10 删 text_off/text_len 后从第 21→19）round-trip。
#[test]
fn blob_v9_round_trips_reuse_key() {
    let rn = RenderNode {
        node_id: 7,
        parent_id: None,
        visible: true,
        alpha: 1.0,
        color_tint: [1.0; 4],
        world_matrix: transform::IDENTITY,
        blend: BlendMode::Normal,
        mask_context: MaskContext(0),
        sort_key: 0,
        change_level: ChangeLevel::Full,
        reuse_key: 42, // v9 新字段
        effect: EffectBlock::default(),
        shadow_params: [0.0; 6],
        gradient: loomgui_core::render::gradient::GradientParams::default(),
        payload: NodePayload::Mesh {
            verts: vec![[0.0, 0.0]; 4],
            uvs: vec![[0.0, 0.0]; 4],
            colors: vec![[1.0; 4]; 4],
            indices: vec![0, 1, 2, 0, 2, 3],
            image_path: None,
            program: 0,
            color_matrix: [0.0; 20],
        },
    };
    let blob = build_blob(&frame(&[rn]));
    let view = TestView::parse(&blob);
    assert_eq!(view.version(), 13, "blob VERSION=13");
    assert_eq!(view.reuse_key(0), 42, "reuse_key round-trip");
}

/// v11：effect_block 列（第 21 列 / index 20，128B = EffectBlock::SIZE）round-trip。
/// 非文字节点（EffectBlock::default()）写全 0；文字节点 effect 字段 round-trip 保真。
/// 照 color_matrix 列先例（per-node 定长 struct 列）。
#[test]
fn blob_writes_effect_block_column() {
    // 节点 0：默认 effect（全 0）；节点 1：outline_width=2.0 + glow_power=0.5。
    let mut effect = EffectBlock::default();
    effect.outline_width = 2.0;
    effect.glow_power = 0.5;
    let mut node_with_effect = mesh_node(0, None, 0.0, 0.0, 5.0, 5.0);
    node_with_effect.effect = effect;
    let plain_node = mesh_node(1, None, 0.0, 0.0, 5.0, 5.0);

    let blob = build_blob(&frame(&[plain_node, node_with_effect]));
    let view = TestView::parse(&blob);

    // 节点 0（默认 effect）：outline_width = 0.0（首个 f32）。
    assert_eq!(
        view.effect_block_f32(0, 0),
        0.0,
        "默认 effect：outline_width=0"
    );
    // 节点 1：outline_width = 2.0（effect_block 首个 f32）。
    assert_eq!(
        view.effect_block_f32(1, 0),
        2.0,
        "effect outline_width=2.0 round-trip"
    );
    // outline_width 后跟 outline_color[4]：默认全 0 → f32 索引 1..4 全 0。
    for j in 1..5 {
        assert_eq!(
            view.effect_block_f32(1, j),
            0.0,
            "outline_color 默认全 0（f32 idx {}）",
            j
        );
    }
    // glow_power 位于 f32 idx = 1(outline_width) + 4(outline_color) + 3*7(underlay[3]) = 26。
    // underlay 单 slot = offset_x + offset_y + softness + color[4] = 7 f32；3 slot = 21 f32。
    assert_eq!(
        view.effect_block_f32(1, 26),
        0.5,
        "effect glow_power=0.5 round-trip（f32 idx 26）"
    );
    // 节点 0 整块 effect_block 全 0（默认 effect）。
    for j in 0..(EffectBlock::SIZE / 4) {
        assert_eq!(
            view.effect_block_f32(0, j),
            0.0,
            "默认 effect 全 0（f32 idx {}）",
            j
        );
    }
}

/// v12：shadow_params 列（第 22 列 / index 21，[f32;6]=24B）round-trip。
/// 非 shadow 节点（[0.0;6]）写全零；shadow 节点 6 个 f32 round-trip 保真。
/// 照 effect_block 列先例（per-node 定长 struct 列）。
#[test]
fn blob_writes_shadow_params_column() {
    // 节点 0：默认 shadow（全零）；节点 1：SDF 参数 [halfSize.x, halfSize.y, radius, σ, inset, _pad]。
    let params = [12.5, 6.0, 3.0, 2.5, 1.0, 0.0];
    let mut node_with_shadow = mesh_node(0, None, 0.0, 0.0, 5.0, 5.0);
    node_with_shadow.shadow_params = params;
    let plain_node = mesh_node(1, None, 0.0, 0.0, 5.0, 5.0);

    let blob = build_blob(&frame(&[plain_node, node_with_shadow]));
    let view = TestView::parse(&blob);

    // 节点 0（默认 shadow_params）：全 6 个 f32 == 0.0。
    for j in 0..6 {
        assert_eq!(
            view.shadow_params_f32(0, j),
            0.0,
            "默认 shadow_params 全 0（f32 idx {}）",
            j
        );
    }
    // 节点 1：6 个 f32 round-trip 保真。
    for j in 0..6 {
        assert_eq!(
            view.shadow_params_f32(1, j),
            params[j],
            "shadow_params[{}] round-trip",
            j
        );
    }
}

/// v13：blob SOA 列数 = 23（加 grad_params 列）。读 header 的 col_offset 表长度断言。
/// header layout：magic(4)+version(4)+node_count(4) + N×col_offset(4) + 3 arena pair。
/// mesh_arena_off 字段位于 header offset `12 + N*4`。对 N=23 → offset 104。
#[test]
fn blob_column_count_is_23() {
    let blob = build_blob(&frame(&[mesh_node(0, None, 0.0, 0.0, 1.0, 1.0)]));
    assert_eq!(
        u32::from_le_bytes(blob[4..8].try_into().unwrap()),
        13,
        "VERSION=13（v13：加 grad_params 列）"
    );
    // mesh_arena_off 字段位置 = 12 + N*4。N=23 → offset 104（读出一个 >= header_len 的值）。
    // 反推列数 N = (mesh_arena_off_field_position - 12) / 4 = (104 - 12) / 4 = 23。
    let mesh_arena_off_field_at = 12 + 23 * 4;
    assert_eq!(
        mesh_arena_off_field_at, 104,
        "v13 header：N=23 列 → mesh_arena_off @ 104"
    );
    let mesh_arena_off = u32::from_le_bytes(
        blob[mesh_arena_off_field_at..mesh_arena_off_field_at + 4]
            .try_into()
            .unwrap(),
    ) as usize;
    // mesh_arena_off 必须 >= header_len（12 + 22*4 + 3*8 = 124），证明 22 列都正确布局。
    assert!(
        mesh_arena_off >= 12 + 22 * 4 + 24,
        "22 列布局后 mesh_arena_off({}) >= header_len(124)",
        mesh_arena_off
    );
}

/// v12：每列的字节长度 = node_count × stride[k]（22 列）。
/// 读到 col_off[k+1] - col_off[k] 或 (first arena offset) - col_off[21] 断言等于预期。
#[test]
fn blob_column_lengths_match_node_count_times_stride() {
    let blob = build_blob(&frame(&[
        mesh_node(0, None, 0.0, 0.0, 1.0, 1.0),
        mesh_node(1, None, 2.0, 2.0, 3.0, 3.0),
    ]));
    let node_count = u32::from_le_bytes(blob[8..12].try_into().unwrap()) as usize;
    assert_eq!(node_count, 2, "2 render nodes");

    let strides: [usize; 22] = [
        4, 4, 1, 4, 4, 4, 4, 4, 4, 4, 4, 4, 1, 4, 4, 4, 1, 80, 1, 4, 128, 24,
    ];
    // column offsets 从 header byte 12 开始
    let mut col_off = [0usize; 22];
    for k in 0..22 {
        col_off[k] =
            u32::from_le_bytes(blob[12 + k * 4..12 + k * 4 + 4].try_into().unwrap()) as usize;
    }
    let arena_off = u32::from_le_bytes(blob[100..104].try_into().unwrap()) as usize;

    for k in 0..22 {
        let start = col_off[k];
        let end = if k < 21 { col_off[k + 1] } else { arena_off };
        let actual_len = end - start;
        let expected = node_count * strides[k];
        assert_eq!(
            actual_len, expected,
            "col {} len: {} (expected {} = {} * {})",
            k, actual_len, expected, node_count, strides[k]
        );
    }
}
