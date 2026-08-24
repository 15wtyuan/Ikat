//! dirty hash：header_hash（表头） + payload_hash（几何），供 Stage 跨帧分轴比较定
//! ChangeLevel。碰撞最坏 1 帧延迟，不破正确性。

use crate::render::node::{NodePayload, RenderNode};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// 几何轴 hash：payload 全量（verts/uvs/colors/indices/image_path/program/color_matrix
/// 或 font_size/color/全量 glyph）。不含 world_matrix/alpha/sort/mask（那是 header_hash）。
/// 全量——不采样。过去用采样 hash 造成几何变更漏检（缺帧/跳变），现改全量覆盖
/// payload 所有字段，杜绝此类漏字段缺陷。
pub fn payload_hash(rn: &RenderNode) -> u64 {
    let mut h = DefaultHasher::new();
    match &rn.payload {
        NodePayload::Mesh {
            verts,
            uvs,
            colors,
            indices,
            image_path,
            program,
            color_matrix,
        } => {
            1u8.hash(&mut h); // 判别
            image_path.hash(&mut h);
            program.hash(&mut h);
            for &v in color_matrix.iter() {
                v.to_le_bytes().hash(&mut h);
            }
            // re-base verts to local before hashing。
            // 纯平移节点 bake 了绝对世界坐标进 verts→减法得 local；
            // 非纯平移节点已 box-local（Rect{x:0,y:0}）→不减。
            // 这样位置变只改 world_matrix（进 header_hash），不改 payload_hash→Header。
            let pure = crate::transform::is_pure_translation(&rn.world_matrix);
            let (tx, ty) = if pure {
                (rn.world_matrix[4], rn.world_matrix[5])
            } else {
                (0.0, 0.0)
            };
            for v in verts {
                (v[0] - tx).to_le_bytes().hash(&mut h);
                (v[1] - ty).to_le_bytes().hash(&mut h);
            }
            for u in uvs {
                u[0].to_le_bytes().hash(&mut h);
                u[1].to_le_bytes().hash(&mut h);
            }
            for c in colors {
                for &x in c.iter() {
                    x.to_le_bytes().hash(&mut h);
                }
            }
            for &ix in indices {
                ix.hash(&mut h);
            }
        }
    }
    h.finish()
}

/// 表头轴 hash：world_matrix + visible + alpha + sort_key + mask_context + color_tint + blend +
/// reuse_key + parent_id + effect + shadow_params。廉价属性——变了 C# 只需改 GO transform / 材质
/// （SetPropertyBlock _Alpha / SDF effect / shadow uniforms），不碰 mesh。
/// reuse_key 进 header_hash——同 NodeId 换 reuse_key 时需触发 Header 级变更刷新 GO
/// 绑定（理论上 driver 不该这么用，但 hash 该覆盖所有身份字段，避免漏）。
/// effect 进 header_hash——SDF effect 参数（outline/underlay/glow/blur）变只更 MPB uniform，
/// 不重建几何（effect 是渲染层属性，非 mesh 几何）。
/// shadow_params 进 header_hash——box-shadow SDF 参数（halfSize/radius/sigma/inset）变只更
/// MPB uniform（_ShadowHalfSize 等），不重建几何（照 effect 同路径，渲染层属性非 mesh 几何）。
/// gradient 进 header_hash——渐变参数（角度/stops/radial 几何）变只更 MPB uniform，不重建
/// mesh（uv 局部坐标在 box 尺寸变时由 payload_hash 兜住）。
pub fn header_hash(rn: &RenderNode) -> u64 {
    let mut h = DefaultHasher::new();
    for &v in rn.world_matrix.iter() {
        v.to_le_bytes().hash(&mut h);
    }
    rn.visible.hash(&mut h);
    rn.alpha.to_le_bytes().hash(&mut h);
    rn.sort_key.hash(&mut h);
    rn.mask_context.0.hash(&mut h);
    for &v in rn.color_tint.iter() {
        v.to_le_bytes().hash(&mut h);
    }
    (match rn.blend {
        crate::render::node::BlendMode::Normal => 0u8,
    })
    .hash(&mut h);
    rn.reuse_key.hash(&mut h);
    rn.parent_id.hash(&mut h);
    rn.effect.to_bytes().hash(&mut h); // SDF effect 参数：变 → Header 级（只更 MPB uniform）
                                       // box-shadow SDF 参数：变 → Header 级（只更 MPB uniform，照 effect 路径，不重建 mesh）。
    for &v in rn.shadow_params.iter() {
        v.to_le_bytes().hash(&mut h);
    }
    // 渐变参数：变 → Header 级（只更 MPB uniform，照 shadow_params 路径）。
    rn.gradient.to_bytes().hash(&mut h);
    h.finish()
}

#[cfg(test)]
#[allow(unreachable_patterns, irrefutable_let_patterns)]
mod tests {
    use super::*;
    use crate::render::node::{BlendMode, ChangeLevel, MaskContext, NodePayload, RenderNode};
    use crate::transform::IDENTITY;

    /// mesh_rn：构造带 image_path 的 Mesh RenderNode（None=纯色，Some=图 path）。
    fn mesh_rn(path: Option<&str>, alpha: f32, color0: [f32; 4]) -> RenderNode {
        RenderNode {
            node_id: 0,
            parent_id: None,
            visible: true,
            alpha,
            color_tint: [1.0; 4],
            world_matrix: IDENTITY,
            blend: BlendMode::Normal,
            mask_context: MaskContext(0),
            sort_key: 0,
            change_level: ChangeLevel::Full,
            reuse_key: 0,
            effect: crate::render::node::EffectBlock::default(),
            shadow_params: [0.0; 6],
            gradient: crate::render::gradient::GradientParams::default(),
            payload: NodePayload::Mesh {
                verts: vec![[0.0, 0.0]; 4],
                uvs: vec![[0.0, 0.0]; 4],
                colors: vec![color0; 4],
                indices: vec![0, 1, 2, 0, 2, 3],
                image_path: path.map(|s| s.to_string()),
                program: 0,
                color_matrix: [0.0; 20],
            },
        }
    }

    #[test]
    fn header_hash_world_matrix_change() {
        let a = mesh_rn(Some("a.png"), 1.0, [1.0; 4]);
        let mut b = mesh_rn(Some("a.png"), 1.0, [1.0; 4]);
        b.world_matrix = [1.0, 0.0, 0.0, 1.0, 5.0, 0.0]; // tx=5
        assert_ne!(
            header_hash(&a),
            header_hash(&b),
            "world 变 → header_hash 变"
        );
    }

    #[test]
    fn header_hash_ignores_payload() {
        // 几何变、表头不变 → header_hash 相等（payload 归 payload_hash）。
        let a = mesh_rn(Some("a.png"), 1.0, [1.0; 4]);
        let mut b = mesh_rn(Some("a.png"), 1.0, [1.0; 4]);
        if let NodePayload::Mesh { verts, .. } = &mut b.payload {
            verts[0] = [9.0, 9.0];
        }
        assert_eq!(header_hash(&a), header_hash(&b), "几何变不影响 header_hash");
    }

    #[test]
    fn header_hash_alpha_change() {
        let a = mesh_rn(Some("a.png"), 1.0, [1.0; 4]);
        let b = mesh_rn(Some("a.png"), 0.5, [1.0; 4]); // alpha 0.5
        assert_ne!(
            header_hash(&a),
            header_hash(&b),
            "alpha 变 → header_hash 变（HEADER）"
        );
    }

    #[test]
    fn payload_hash_ignores_alpha() {
        // alpha 归 header，payload_hash 不含 alpha（否则 alpha 变会误落 FULL）。
        let a = mesh_rn(Some("a.png"), 1.0, [1.0; 4]);
        let b = mesh_rn(Some("a.png"), 0.5, [1.0; 4]);
        assert_eq!(
            payload_hash(&a),
            payload_hash(&b),
            "payload_hash 不含 alpha"
        );
    }

    // reuse_key 进 header_hash 回归测试（身份字段进表头 hash，
    // 同 NodeId 换 reuse_key 时 header_hash 应变化触发 Header 级变更）。

    #[test]
    fn header_hash_includes_reuse_key() {
        // reuse_key 变 → header_hash 变（身份字段进表头 hash）。
        let a = mesh_rn(Some("a.png"), 1.0, [1.0; 4]);
        let mut b = mesh_rn(Some("a.png"), 1.0, [1.0; 4]);
        b.reuse_key = 7;
        assert_ne!(
            header_hash(&a),
            header_hash(&b),
            "reuse_key 变 → header_hash 变"
        );
    }

    #[test]
    fn header_hash_includes_parent_id() {
        // parent_id 变 → header_hash 变。同 node_id 换父时须触发 Header 变更，
        // C# MirrorPool 才能 re-parent GameObject（否则 ChangeLevel::Skip 不动）。
        let a = mesh_rn(Some("a.png"), 1.0, [1.0; 4]);
        let mut b = mesh_rn(Some("a.png"), 1.0, [1.0; 4]);
        b.parent_id = Some(42);
        assert_ne!(
            header_hash(&a),
            header_hash(&b),
            "parent_id 变 → header_hash 变"
        );
    }

    // effect 进 header_hash（SDF effect 参数变 = Header 级，只更 MPB uniform，
    // 不重建 mesh）。payload_hash 不采样 effect（effect 非几何）。

    #[test]
    fn header_hash_includes_effect() {
        let a = mesh_rn(Some("a.png"), 1.0, [1.0; 4]);
        let mut b = mesh_rn(Some("a.png"), 1.0, [1.0; 4]);
        b.effect.outline_width = 2.0;
        assert_ne!(
            header_hash(&a),
            header_hash(&b),
            "effect 变 → header_hash 变（HEADER 级，只更 MPB）"
        );
    }

    #[test]
    fn payload_hash_ignores_effect() {
        let a = mesh_rn(Some("a.png"), 1.0, [1.0; 4]);
        let mut b = mesh_rn(Some("a.png"), 1.0, [1.0; 4]);
        b.effect.outline_width = 2.0;
        assert_eq!(
            payload_hash(&a),
            payload_hash(&b),
            "effect 不进 payload_hash（非几何，effect 归 header 轴）"
        );
    }

    // shadow_params 进 header_hash（box-shadow SDF 参数变 = Header 级，只更 MPB uniform，
    // 不重建 mesh）。payload_hash 不采样 shadow_params（shadow 非几何）。

    #[test]
    fn header_hash_includes_shadow_params() {
        let a = mesh_rn(Some("a.png"), 1.0, [1.0; 4]);
        let mut b = mesh_rn(Some("a.png"), 1.0, [1.0; 4]);
        b.shadow_params[2] = 5.0; // box-shadow radius 变
        assert_ne!(
            header_hash(&a),
            header_hash(&b),
            "shadow_params 变 → header_hash 变（HEADER 级，只更 MPB）"
        );
    }

    #[test]
    fn payload_hash_ignores_shadow_params() {
        let a = mesh_rn(Some("a.png"), 1.0, [1.0; 4]);
        let mut b = mesh_rn(Some("a.png"), 1.0, [1.0; 4]);
        b.shadow_params[2] = 5.0;
        assert_eq!(
            payload_hash(&a),
            payload_hash(&b),
            "shadow_params 不进 payload_hash（非几何，归 header 轴）"
        );
    }
}
