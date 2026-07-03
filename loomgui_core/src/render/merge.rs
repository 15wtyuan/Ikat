//! Mesh 合并：按 sort_key 扫描，连续同 DrawState 的 program=0 Mesh 节点
//! 拼成单个 merged Mesh payload → 1 draw call。
//!
//! 前置：`batch::reorder_for_batching` 已把同 DrawState 不相交元素排到 sort_key 相邻。
//! Text（program=1）/ 不同 DrawState 保持独立。

use crate::render::node::{NodePayload, RenderNode};

/// DrawState 键（image_path, program, mask_context, alpha_bits）。program=0 Mesh 才参与合并。
/// v1.4-a T6：texture 砍，改 image_path（同 path 的图可合批；None=纯色块可互合）。
/// v1.4-a T9：加 alpha（to_bits 比较）——alpha 是 per-renderer uniform，不同 alpha 不能合批。
fn mesh_key(rn: &RenderNode) -> Option<(Option<String>, u32, u32, u32)> {
    match &rn.payload {
        NodePayload::Mesh { image_path, program, .. }
            if *program == 0 && crate::transform::is_pure_translation(&rn.world_matrix) =>
        {
            Some((image_path.clone(), *program, rn.mask_context.0, rn.alpha.to_bits()))
        }
        _ => None,
    }
}

/// 按 sort_key 扫描，连续同 DrawState 的 Mesh 节点合并成单个 merged Mesh payload。
/// merged node_id = batch 内最小原始 node_id（锚）。
pub fn merge_meshes(nodes: Vec<RenderNode>) -> Vec<RenderNode> {
    // 1. 按 sort_key 排序（重排后序）。
    let mut order: Vec<usize> = (0..nodes.len()).collect();
    order.sort_by_key(|&i| nodes[i].sort_key);

    let mut out: Vec<RenderNode> = Vec::with_capacity(nodes.len());
    let mut i = 0;
    while i < order.len() {
        let idx = order[i];
        let key = mesh_key(&nodes[idx]);
        if key.is_none() {
            // Text：原样。
            out.push(nodes[idx].clone());
            i += 1;
            continue;
        }
        let key = key.unwrap();
        // 收集连续同 key 的 Mesh。
        // v1.4-a T6：key 含 Option<String>（非 Copy），用 ref 比较避免 move。
        let mut batch_idx: Vec<usize> = vec![idx];
        let mut j = i + 1;
        while j < order.len() && mesh_key(&nodes[order[j]]).as_ref() == Some(&key) {
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
        if let NodePayload::Mesh { verts: v, uvs: u, colors: c, indices: ix, .. } = &nodes[bi].payload {
            verts.extend_from_slice(v);
            uvs.extend_from_slice(u);
            // v1.4-a T9：alpha 不再烤进 colors（alpha 走 _Alpha uniform，单值 per-draw-call）。
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
        alpha: last.alpha, // v1.4-a T9：merged alpha=子 alpha（同 key 保证一致；走 _Alpha uniform）
        grayed: false,
        color_tint: [1.0; 4],
        world_matrix: crate::transform::IDENTITY,
        blend: last.blend,
        mask_context: last.mask_context,
        sort_key: last.sort_key,
        change_level: crate::render::node::ChangeLevel::Full,
        reuse_key: 0,
        payload: NodePayload::Mesh {
            verts, uvs, colors, indices,
            image_path: match &last.payload {
                NodePayload::Mesh { image_path, .. } => image_path.clone(),
                _ => None,
            },
            program: 0,
            color_matrix: [0.0; 20],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::node::{BlendMode, ChangeLevel, MaskContext};

    /// v1.4-a T6：texture 砍，mesh_node 改带 image_path（None=纯色，Some=图 path）。
    fn mesh_node(id: u32, path: Option<&str>, sort_key: u32, alpha: f32, rect_off: f32) -> RenderNode {
        RenderNode {
            node_id: id, parent_id: None, visible: true, alpha,
            grayed: false, color_tint: [1.0; 4],
            world_matrix: crate::transform::IDENTITY,
            blend: BlendMode::Normal, mask_context: MaskContext(0), sort_key,
            change_level: ChangeLevel::Full,
            reuse_key: 0,
            payload: NodePayload::Mesh {
                verts: vec![[rect_off, 0.0], [rect_off + 10.0, 0.0],
                            [rect_off + 10.0, 10.0], [rect_off, 10.0]],
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
        // v1.4-a T9：colors.a 不再烤 alpha（走 _Alpha uniform）；merged.alpha = 子 alpha。
        let nodes = vec![
            mesh_node(5, Some("a.png"), 0, 1.0, 0.0),
            mesh_node(3, Some("a.png"), 1, 1.0, 100.0), // 同 alpha
        ];
        let out = merge_meshes(nodes);
        assert_eq!(out.len(), 1, "2 同 DrawState → 1 merged");
        match &out[0].payload {
            NodePayload::Mesh { verts, indices, colors, image_path, .. } => {
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
        let out = merge_meshes(nodes);
        assert_eq!(out.len(), 1);
        if let NodePayload::Mesh { indices, .. } = &out[0].payload {
            // 第一组 [0,1,2,0,2,3]，第二组 +4 [4,5,6,4,6,7]，第三组 +8 [8,9,10,8,10,11]。
            assert_eq!(indices, &vec![0u32,1,2,0,2,3, 4,5,6,4,6,7, 8,9,10,8,10,11]);
        } else { panic!("expected Mesh"); }
    }

    #[test]
    fn different_drawstate_stay_separate() {
        // A(a.png) B(b.png) 同 mask_context 但 path 不同 → 不合并。
        let nodes = vec![mesh_node(1, Some("a.png"), 0, 1.0, 0.0), mesh_node(2, Some("b.png"), 1, 1.0, 100.0)];
        let out = merge_meshes(nodes);
        assert_eq!(out.len(), 2, "不同 image_path → 各自独立");
    }

    #[test]
    fn non_pure_translation_node_does_not_merge() {
        // 两同 DrawState Mesh，其一 world_matrix 非纯平移（旋转）→ 不合并
        use crate::transform;
        let mut a = mesh_node(1, Some("a.png"), 0, 1.0, 0.0);
        a.world_matrix = transform::from_rotate(0.5); // 非纯平移
        let b = mesh_node(2, Some("a.png"), 1, 1.0, 100.0); // 纯平移（IDENTITY）
        let out = merge_meshes(vec![a, b]);
        assert_eq!(out.len(), 2, "非纯平移节点 break merge");
    }

    #[test]
    fn text_node_stays_separate() {
        let mesh = mesh_node(1, Some("a.png"), 0, 1.0, 0.0);
        let mut text = mesh_node(2, Some("a.png"), 1, 1.0, 100.0);
        text.payload = NodePayload::Text {
            layout: crate::text::layout::TextLayout { text_width: 0.0, text_height: 0.0, lines: vec![] },
            font_size: 16.0, color: [1.0; 4], program: 1,
        };
        let out = merge_meshes(vec![mesh, text]);
        assert_eq!(out.len(), 2, "Text 不参与合并");
    }

    #[test]
    fn different_alpha_do_not_merge() {
        // alpha 剥离后：不同 alpha 不能合一个 draw call（uniform 单值）。
        let nodes = vec![
            mesh_node(1, Some("a.png"), 0, 1.0, 0.0),
            mesh_node(2, Some("a.png"), 1, 0.5, 100.0), // 不同 alpha
        ];
        let out = merge_meshes(nodes);
        assert_eq!(out.len(), 2, "不同 alpha → 不合并");
    }

    #[test]
    fn same_alpha_still_merge_no_bake() {
        // 同 alpha 仍合并；但 colors.a 不再烤 alpha（走 uniform）。
        let nodes = vec![
            mesh_node(1, Some("a.png"), 0, 0.5, 0.0),
            mesh_node(2, Some("a.png"), 1, 0.5, 100.0),
        ];
        let out = merge_meshes(nodes);
        assert_eq!(out.len(), 1, "同 alpha 合并");
        if let NodePayload::Mesh { colors, .. } = &out[0].payload {
            for c in colors {
                assert!((c[3] - 1.0).abs() < 1e-6, "colors.a 不烤 alpha（原始1.0）");
            }
        }
        assert!((out[0].alpha - 0.5).abs() < 1e-6, "merged.alpha=子 alpha（走 uniform）");
    }
}
