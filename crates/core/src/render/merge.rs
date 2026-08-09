//! Mesh 合并：按 sort_key 扫描，连续同 DrawState 的 program=0 Mesh 节点
//! 拼成单个 merged Mesh payload → 1 draw call。
//!
//! 前置：`batch::reorder_for_batching` 已把同 DrawState 不相交元素排到 sort_key 相邻。
//! Text（program=1）/ 不同 DrawState 保持独立。

use crate::render::node::{NodePayload, RenderNode};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Hash a [f32; 20] color matrix to a u64 for mesh_key comparison.
/// Floats are hashed via their IEEE 754 bit patterns so that NaN == NaN and -0 == 0
/// don't accidentally split batches.
fn hash_color_matrix(m: &[f32; 20]) -> u64 {
    let mut h = DefaultHasher::new();
    for &v in m.iter() {
        v.to_bits().hash(&mut h);
    }
    h.finish()
}

/// DrawState 键（image_path, program, mask_context, alpha_bits, color_matrix_hash）。
/// program=0/1 Mesh 才参与合并。含 color_matrix 哈希——不同 color filter（如 grayscale vs sepia）
/// 不能合批，否则 filter 数据在 merge_batch 中被清零丢失。
///
/// 控件 node_id（control_ids）强制返回 None：merge 会把被合并者的 node_id 吞成 anchor，
/// 控件必须保留独立 node_id 供 Unity 后端建交互实体（hit test / 状态 / 镜像 GameObject）。
fn mesh_key(
    control_ids: &std::collections::HashSet<u32>,
    rn: &RenderNode,
) -> Option<(Option<String>, u32, u32, u32, u64)> {
    if control_ids.contains(&rn.node_id) {
        return None; // 控件保留独立 node_id，不参与合并
    }
    if rn.node_id & crate::render::BACK_LAYER_FLAG != 0 {
        return None; // back-layer 合成节点（如 box-shadow）不合批
    }
    if crate::render::is_tf_edit_synth(rn.node_id) {
        return None; // TextField 编辑反馈 mesh（光标/选区/composition）须独立保留
    }
    if crate::render::is_tf_text_synth(rn.node_id) {
        return None; // 文本控件文字主体 mesh（合成 id）须独立保留：背景与文字已拆为两个
                     // node_id（见 TF_TEXT_SYNTH_BYTE），合批会把文字并入别的 GO，
                     // 破坏 C# MirrorPool 按 node_id 的 dirty/change_level 跟踪。
    }
    match &rn.payload {
        NodePayload::Mesh {
            image_path,
            program,
            color_matrix,
            ..
        } if (*program == 0 || *program == 1)
            && crate::transform::is_pure_translation(&rn.world_matrix) =>
        {
            Some((
                image_path.clone(),
                *program,
                rn.mask_context.0,
                rn.alpha.to_bits(),
                hash_color_matrix(color_matrix),
            ))
        }
        _ => None,
    }
}

/// 按 sort_key 扫描，连续同 DrawState 的 Mesh 节点合并成单个 merged Mesh payload。
/// merged node_id = batch 内最小原始 node_id（锚）。控件 node_id（control_ids）排除合并。
pub fn merge_meshes(
    control_ids: &std::collections::HashSet<u32>,
    nodes: Vec<RenderNode>,
) -> Vec<RenderNode> {
    // 1. 按 sort_key 排序（重排后序）。
    let mut order: Vec<usize> = (0..nodes.len()).collect();
    order.sort_by_key(|&i| nodes[i].sort_key);

    let mut out: Vec<RenderNode> = Vec::with_capacity(nodes.len());
    let mut i = 0;
    while i < order.len() {
        let idx = order[i];
        let key = mesh_key(control_ids, &nodes[idx]);
        if key.is_none() {
            // Text / 控件：原样保留独立 node_id。
            out.push(nodes[idx].clone());
            i += 1;
            continue;
        }
        let key = key.unwrap();
        // 收集连续同 key 的 Mesh。
        // key 含 Option<String>（非 Copy），用 ref 比较避免 move。
        let mut batch_idx: Vec<usize> = vec![idx];
        let mut j = i + 1;
        while j < order.len() && mesh_key(control_ids, &nodes[order[j]]).as_ref() == Some(&key) {
            batch_idx.push(order[j]);
            j += 1;
        }
        if batch_idx.len() == 1 {
            out.push(nodes[idx].clone());
        } else {
            out.push(merge_batch(&nodes, &batch_idx));
        }
        i = j;
    }
    out
}

/// 把一组同 DrawState Mesh 节点拼成单个 merged Mesh payload。
fn merge_batch(nodes: &[RenderNode], batch: &[usize]) -> RenderNode {
    // 锚 node_id = batch 内最小原始 node_id。
    let anchor = batch.iter().map(|&i| nodes[i].node_id).min().unwrap();
    let last = &nodes[*batch.last().unwrap()]; // 取 texture/program/mask_context/sort_key 模板
    let mut verts: Vec<[f32; 2]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut base: u32 = 0;
    for &bi in batch {
        let NodePayload::Mesh {
            verts: v,
            uvs: u,
            colors: c,
            indices: ix,
            ..
        } = &nodes[bi].payload;
        {
            verts.extend_from_slice(v);
            uvs.extend_from_slice(u);
            // alpha 不烤进 colors（alpha 走 _Alpha uniform，单值 per-draw-call）。
            colors.extend_from_slice(c);
            for &ixv in ix {
                indices.push(ixv + base);
            }
            base += v.len() as u32;
        }
    }
    RenderNode {
        node_id: anchor,
        parent_id: None,
        visible: true,
        alpha: last.alpha, // merged alpha=子 alpha（同 key 保证一致；走 _Alpha uniform）
        color_tint: [1.0; 4],
        world_matrix: crate::transform::IDENTITY,
        blend: last.blend,
        mask_context: last.mask_context,
        sort_key: last.sort_key,
        change_level: crate::render::node::ChangeLevel::Full,
        reuse_key: 0,
        effect: crate::render::node::EffectBlock::default(),
        shadow_params: [0.0; 6],
        payload: NodePayload::Mesh {
            verts,
            uvs,
            colors,
            indices,
            image_path: match &last.payload {
                NodePayload::Mesh { image_path, .. } => image_path.clone(),
            },
            // 继承模板 program：text（program=1）合批后必须仍 program=1，否则 Unity 用
            // image shader 渲染 atlas R8 → 字形 quad 填满顶点色成实心方块。program=0 通用 mesh 同理继承。
            program: match &last.payload {
                NodePayload::Mesh { program, .. } => *program,
            },
            color_matrix: match &last.payload {
                NodePayload::Mesh { color_matrix, .. } => *color_matrix,
            },
        },
    }
}

#[cfg(test)]
#[allow(unreachable_patterns, irrefutable_let_patterns)]
mod tests {
    use super::*;
    use crate::render::node::{BlendMode, ChangeLevel, MaskContext};

    /// mesh_node 带 image_path（None=纯色，Some=图 path）。
    fn mesh_node(
        id: u32,
        path: Option<&str>,
        sort_key: u32,
        alpha: f32,
        rect_off: f32,
    ) -> RenderNode {
        RenderNode {
            node_id: id,
            parent_id: None,
            visible: true,
            alpha,
            color_tint: [1.0; 4],
            world_matrix: crate::transform::IDENTITY,
            blend: BlendMode::Normal,
            mask_context: MaskContext(0),
            sort_key,
            change_level: ChangeLevel::Full,
            reuse_key: 0,
            effect: crate::render::node::EffectBlock::default(),
            shadow_params: [0.0; 6],
            payload: NodePayload::Mesh {
                verts: vec![
                    [rect_off, 0.0],
                    [rect_off + 10.0, 0.0],
                    [rect_off + 10.0, 10.0],
                    [rect_off, 10.0],
                ],
                uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                colors: vec![[1.0, 1.0, 1.0, 1.0]; 4],
                indices: vec![0, 1, 2, 0, 2, 3],
                image_path: path.map(|s| s.to_string()),
                program: 0,
                color_matrix: [0.0; 20],
            },
        }
    }

    #[test]
    fn two_same_drawstate_merge_into_one() {
        // A(a.png,sk0) B(a.png,sk1) 同 alpha=1.0 → 1 merged：8 verts / 12 indices。
        // colors.a 不烤 alpha（走 _Alpha uniform）；merged.alpha = 子 alpha。
        let nodes = vec![
            mesh_node(5, Some("a.png"), 0, 1.0, 0.0),
            mesh_node(3, Some("a.png"), 1, 1.0, 100.0), // 同 alpha
        ];
        let out = merge_meshes(&Default::default(), nodes);
        assert_eq!(out.len(), 1, "2 同 DrawState → 1 merged");
        match &out[0].payload {
            NodePayload::Mesh {
                verts,
                indices,
                colors,
                image_path,
                ..
            } => {
                assert_eq!(verts.len(), 8, "2×4 verts");
                assert_eq!(indices.len(), 12, "2×6 indices");
                assert_eq!(*image_path, Some("a.png".to_string()));
                // colors.a 不烤 alpha（原始 1.0）。
                for c in colors {
                    assert!((c[3] - 1.0).abs() < 1e-6, "colors.a 不烤 alpha（原始1.0）");
                }
            }
            _ => panic!("expected Mesh"),
        }
        // 锚 node_id = min(5,3) = 3。
        assert_eq!(out[0].node_id, 3, "锚 = batch 内最小 node_id");
        // merged world_matrix=IDENTITY / alpha=1.0（子 alpha）。
        assert!(crate::transform::is_identity(&out[0].world_matrix));
        assert!((out[0].alpha - 1.0).abs() < 1e-6);
    }

    #[test]
    fn index_offset_correct_for_three_nodes() {
        // 3 节点同 DrawState → merged indices 第二组 +4、第三组 +8。
        let nodes = vec![
            mesh_node(1, Some("a.png"), 0, 1.0, 0.0),
            mesh_node(2, Some("a.png"), 1, 1.0, 50.0),
            mesh_node(3, Some("a.png"), 2, 1.0, 100.0),
        ];
        let out = merge_meshes(&Default::default(), nodes);
        assert_eq!(out.len(), 1);
        if let NodePayload::Mesh { indices, .. } = &out[0].payload {
            // 第一组 [0,1,2,0,2,3]，第二组 +4 [4,5,6,4,6,7]，第三组 +8 [8,9,10,8,10,11]。
            assert_eq!(
                indices,
                &vec![0u32, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7, 8, 9, 10, 8, 10, 11]
            );
        } else {
            panic!("expected Mesh");
        }
    }

    #[test]
    fn different_drawstate_stay_separate() {
        // A(a.png) B(b.png) 同 mask_context 但 path 不同 → 不合并。
        let nodes = vec![
            mesh_node(1, Some("a.png"), 0, 1.0, 0.0),
            mesh_node(2, Some("b.png"), 1, 1.0, 100.0),
        ];
        let out = merge_meshes(&Default::default(), nodes);
        assert_eq!(out.len(), 2, "不同 image_path → 各自独立");
    }

    #[test]
    fn non_pure_translation_node_does_not_merge() {
        // 两同 DrawState Mesh，其一 world_matrix 非纯平移（旋转）→ 不合并
        use crate::transform;
        let mut a = mesh_node(1, Some("a.png"), 0, 1.0, 0.0);
        a.world_matrix = transform::from_rotate(0.5); // 非纯平移
        let b = mesh_node(2, Some("a.png"), 1, 1.0, 100.0); // 纯平移（IDENTITY）
        let out = merge_meshes(&Default::default(), vec![a, b]);
        assert_eq!(out.len(), 2, "非纯平移节点 break merge");
    }

    #[test]
    fn two_same_atlas_text_nodes_merge() {
        // v1.6：text 现产 Mesh(program=1)，同 atlas path 允合批。
        // 两 text 节点同 program=1 同 image_path → merge 成 1 个 8-vert Mesh。
        let mut t1 = mesh_node(1, Some("loomgui://font-atlas/p0"), 0, 1.0, 0.0);
        t1.payload = NodePayload::Mesh {
            verts: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            colors: vec![[1.0; 4]; 4],
            indices: vec![0, 1, 2, 0, 2, 3],
            image_path: Some("loomgui://font-atlas/p0".into()),
            program: 1,
            color_matrix: [0.0; 20],
        };
        let mut t2 = mesh_node(2, Some("loomgui://font-atlas/p0"), 1, 1.0, 100.0);
        t2.payload = NodePayload::Mesh {
            verts: vec![[100.0, 0.0], [110.0, 0.0], [110.0, 10.0], [100.0, 10.0]],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            colors: vec![[1.0; 4]; 4],
            indices: vec![0, 1, 2, 0, 2, 3],
            image_path: Some("loomgui://font-atlas/p0".into()),
            program: 1,
            color_matrix: [0.0; 20],
        };
        let out = merge_meshes(&Default::default(), vec![t1, t2]);
        assert_eq!(out.len(), 1, "两同 atlas text 节点 → 1 merged");
        match &out[0].payload {
            NodePayload::Mesh { verts, .. } => {
                assert_eq!(verts.len(), 8, "2×4 verts");
            }
            _ => panic!("expected Mesh"),
        }
    }

    /// merged text mesh 必须保留 program=1（不能硬写 0）。
    /// mesh_key 含 program → 同 batch 的 text 全是 program=1，merged 应继承。
    /// 若 merged program=0：Unity 用 image shader 渲染 atlas R8 纹理 → 整个字形 quad
    /// 填满顶点色 = 实心方块（字看不清）。core 侧根因。
    #[test]
    fn merged_text_mesh_preserves_program_one() {
        let mut t1 = mesh_node(1, Some("loomgui://font-atlas/p0"), 0, 1.0, 0.0);
        t1.payload = NodePayload::Mesh {
            verts: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            colors: vec![[1.0; 4]; 4],
            indices: vec![0, 1, 2, 0, 2, 3],
            image_path: Some("loomgui://font-atlas/p0".into()),
            program: 1,
            color_matrix: [0.0; 20],
        };
        let mut t2 = mesh_node(2, Some("loomgui://font-atlas/p0"), 1, 1.0, 100.0);
        t2.payload = NodePayload::Mesh {
            verts: vec![[100.0, 0.0], [110.0, 0.0], [110.0, 10.0], [100.0, 10.0]],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            colors: vec![[1.0; 4]; 4],
            indices: vec![0, 1, 2, 0, 2, 3],
            image_path: Some("loomgui://font-atlas/p0".into()),
            program: 1,
            color_matrix: [0.0; 20],
        };
        let out = merge_meshes(&Default::default(), vec![t1, t2]);
        assert_eq!(out.len(), 1, "两同 atlas text 节点 → 1 merged");
        match &out[0].payload {
            NodePayload::Mesh {
                program,
                image_path,
                ..
            } => {
                assert_eq!(*program, 1, "merged text 必须保留 program=1（text shader）");
                assert_eq!(
                    *image_path,
                    Some("loomgui://font-atlas/p0".to_string()),
                    "merged text 保留 atlas path"
                );
            }
            _ => panic!("expected Mesh"),
        }
    }

    #[test]
    fn different_alpha_do_not_merge() {
        // alpha 剥离后：不同 alpha 不能合一个 draw call（uniform 单值）。
        let nodes = vec![
            mesh_node(1, Some("a.png"), 0, 1.0, 0.0),
            mesh_node(2, Some("a.png"), 1, 0.5, 100.0), // 不同 alpha
        ];
        let out = merge_meshes(&Default::default(), nodes);
        assert_eq!(out.len(), 2, "不同 alpha → 不合并");
    }

    #[test]
    fn same_alpha_still_merge_no_bake() {
        // 同 alpha 仍合并；但 colors.a 不烤 alpha（走 uniform）。
        let nodes = vec![
            mesh_node(1, Some("a.png"), 0, 0.5, 0.0),
            mesh_node(2, Some("a.png"), 1, 0.5, 100.0),
        ];
        let out = merge_meshes(&Default::default(), nodes);
        assert_eq!(out.len(), 1, "同 alpha 合并");
        if let NodePayload::Mesh { colors, .. } = &out[0].payload {
            for c in colors {
                assert!((c[3] - 1.0).abs() < 1e-6, "colors.a 不烤 alpha（原始1.0）");
            }
        }
        assert!(
            (out[0].alpha - 0.5).abs() < 1e-6,
            "merged.alpha=子 alpha（走 uniform）"
        );
    }

    /// Bug: mesh_key previously omitted color_matrix, so nodes with different
    /// color filters (grayscale vs sepia) got the same key and were merged,
    /// silently dropping the filter data (merge_batch hardcoded [0;20]).
    #[test]
    fn different_color_matrix_yields_different_mesh_key() {
        let grayscale = [
            0.299, 0.587, 0.114, 0.0, 0.0, 0.299, 0.587, 0.114, 0.0, 0.0, 0.299, 0.587, 0.114, 0.0,
            0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
        ];
        let sepia = [
            0.393, 0.769, 0.189, 0.0, 0.0, 0.349, 0.686, 0.168, 0.0, 0.0, 0.272, 0.534, 0.131, 0.0,
            0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
        ];

        let make_node = |id: u32, cm: [f32; 20]| RenderNode {
            node_id: id,
            parent_id: None,
            visible: true,
            alpha: 1.0,
            color_tint: [1.0; 4],
            world_matrix: crate::transform::IDENTITY,
            blend: BlendMode::Normal,
            mask_context: MaskContext(0),
            sort_key: id,
            change_level: ChangeLevel::Full,
            reuse_key: 0,
            effect: crate::render::node::EffectBlock::default(),
            shadow_params: [0.0; 6],
            payload: NodePayload::Mesh {
                verts: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
                uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                colors: vec![[1.0; 4]; 4],
                indices: vec![0, 1, 2, 0, 2, 3],
                image_path: Some("a.png".into()),
                program: 0,
                color_matrix: cm,
            },
        };

        let n1 = make_node(1, grayscale);
        let n2 = make_node(2, sepia);
        let k1 = mesh_key(&Default::default(), &n1);
        let k2 = mesh_key(&Default::default(), &n2);
        assert_ne!(k1, k2, "不同 color_matrix → 不同 mesh_key，不合并");
    }

    /// Regression: same color_matrix (even non-zero) → same mesh_key → still mergeable.
    /// The merged batch must preserve the inherited color_matrix, not zero it out.
    #[test]
    fn same_color_matrix_still_merges_and_preserves_filter() {
        let grayscale = [
            0.299, 0.587, 0.114, 0.0, 0.0, 0.299, 0.587, 0.114, 0.0, 0.0, 0.299, 0.587, 0.114, 0.0,
            0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
        ];

        let make_node = |id: u32, sk: u32| RenderNode {
            node_id: id,
            parent_id: None,
            visible: true,
            alpha: 1.0,
            color_tint: [1.0; 4],
            world_matrix: crate::transform::IDENTITY,
            blend: BlendMode::Normal,
            mask_context: MaskContext(0),
            sort_key: sk,
            change_level: ChangeLevel::Full,
            reuse_key: 0,
            effect: crate::render::node::EffectBlock::default(),
            shadow_params: [0.0; 6],
            payload: NodePayload::Mesh {
                verts: vec![
                    [sk as f32 * 50.0, 0.0],
                    [sk as f32 * 50.0 + 10.0, 0.0],
                    [sk as f32 * 50.0 + 10.0, 10.0],
                    [sk as f32 * 50.0, 10.0],
                ],
                uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                colors: vec![[1.0; 4]; 4],
                indices: vec![0, 1, 2, 0, 2, 3],
                image_path: Some("a.png".into()),
                program: 0,
                color_matrix: grayscale,
            },
        };

        let nodes = vec![make_node(1, 0), make_node(2, 1)];
        let out = merge_meshes(&Default::default(), nodes);
        assert_eq!(out.len(), 1, "同 color_matrix 仍可合并");

        // merged batch must inherit the non-zero color_matrix, not zero it out.
        match &out[0].payload {
            NodePayload::Mesh {
                color_matrix,
                verts,
                ..
            } => {
                assert_eq!(
                    color_matrix, &grayscale,
                    "merged 保留继承的 color_matrix，不清零"
                );
                assert_eq!(verts.len(), 8, "2×4 verts 合并");
            }
            _ => panic!("expected Mesh"),
        }
    }

    #[test]
    fn control_node_id_excluded_from_merge() {
        // 控件节点（control_ids）必须保留独立 node_id：Unity 后端按 node_id 建交互实体
        // （hit test / 状态 / 镜像 GameObject）。即便与邻居同 DrawState 相邻，也排除合并——
        // 否则 merge 把控件 node_id 吞成 anchor，Unity 丢失控件实体（不渲染、不可交互）。
        // 复现：pivot 后 Toggle/RadioButton 是空 div，自身 background mesh 走 program=0，
        // 与相邻纯色背景同 DrawState，被合并后 node_id 消失。
        let nodes = vec![
            mesh_node(10, None, 0, 1.0, 0.0),   // 控件
            mesh_node(11, None, 1, 1.0, 100.0), // 同 DrawState 邻居
        ];
        let control_ids: std::collections::HashSet<u32> = [10u32].iter().copied().collect();
        let out = merge_meshes(&control_ids, nodes);
        assert_eq!(out.len(), 2, "控件排除合并 → 控件与邻居各自独立");
        assert!(
            out.iter().any(|rn| rn.node_id == 10),
            "控件 node_id 10 保留（未被吞成 anchor）"
        );
        assert!(
            out.iter().any(|rn| rn.node_id == 11),
            "邻居 node_id 11 保留"
        );
    }
}
