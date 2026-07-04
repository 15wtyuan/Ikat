//! dirty hash：header_hash（表头） + payload_hash（几何），供 Stage 跨帧分轴比较定
//! ChangeLevel。碰撞最坏 1 帧延迟，不破正确性。

use crate::render::node::{NodePayload, RenderNode};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// 几何轴 hash：payload 全量（verts/uvs/colors/indices/image_path/program/color_matrix
/// 或 font_size/color/全量 glyph）。不含 world_matrix/alpha/sort/mask（那是 header_hash）。
/// 全量——不采样，根治漏字段类 bug（坑 56/75/76）。
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
            // re-base verts to local before hashing（同 blob.rs:104-111）。
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
        NodePayload::Text {
            layout,
            font_size,
            color,
            program,
        } => {
            2u8.hash(&mut h);
            font_size.to_le_bytes().hash(&mut h);
            program.hash(&mut h);
            for &v in color.iter() {
                v.to_le_bytes().hash(&mut h);
            }
            for line in &layout.lines {
                // P4：line 全量字段（y/height/baseline/width）——line-height 变化改 height，
                // 不 hash 会漏（违反"全量不采样"承诺，坑 105 同类）。
                line.y.to_le_bytes().hash(&mut h);
                line.height.to_le_bytes().hash(&mut h);
                line.baseline.to_le_bytes().hash(&mut h);
                line.width.to_le_bytes().hash(&mut h);
                for run in &line.runs {
                    run.font_size.to_le_bytes().hash(&mut h);
                    for g in &run.glyphs {
                        g.codepoint.hash(&mut h);
                        g.x.to_le_bytes().hash(&mut h);
                        g.y.to_le_bytes().hash(&mut h);
                    }
                }
            }
        }
    }
    h.finish()
}

/// 表头轴 hash：world_matrix + visible + alpha + sort_key + mask_context + color_tint + blend + reuse_key。
/// 廉价属性——变了 C# 只需改 GO transform / 材质（SetPropertyBlock _Alpha），不碰 mesh。
/// P2：reuse_key 进 header_hash——同 NodeId 换 reuse_key 时（理论上 driver 不该这么用，
/// 但 hash 该覆盖所有身份字段，避免漏）。
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
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::node::{BlendMode, ChangeLevel, MaskContext, NodePayload, RenderNode};
    use crate::text::layout::{Glyph, GlyphRun, Line, TextLayout};
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

    // -----------------------------------------------------------------------
    // payload_hash 测试（支柱2：几何全量，不采样）
    // -----------------------------------------------------------------------

    fn text_rn_content(font_size: f32, color: [f32; 4], cps: &[u32]) -> RenderNode {
        let glyphs: Vec<Glyph> = cps
            .iter()
            .enumerate()
            .map(|(i, &cp)| Glyph {
                glyph_id: 1,
                codepoint: cp,
                x: i as f32 * 10.0,
                y: 0.0,
                bearing_x: 0.0,
                bearing_y: 0.0,
            })
            .collect();
        let layout = TextLayout {
            text_width: 100.0,
            text_height: 20.0,
            lines: vec![Line {
                y: 0.0,
                height: 20.0,
                baseline: 16.0,
                width: 100.0,
                runs: vec![GlyphRun { font_size, glyphs }],
            }],
        };
        RenderNode {
            node_id: 0,
            parent_id: None,
            visible: true,
            alpha: 1.0,
            color_tint: [1.0; 4],
            world_matrix: IDENTITY,
            blend: BlendMode::Normal,
            mask_context: MaskContext(0),
            sort_key: 0,
            change_level: ChangeLevel::Full,
            reuse_key: 0,
            payload: NodePayload::Text {
                layout,
                font_size,
                color,
                program: 1,
            },
        }
    }

    // -----------------------------------------------------------------------
    // header_hash 测试（支柱2：表头轴，与 payload_hash 正交）
    // -----------------------------------------------------------------------

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

    #[test]
    fn payload_hash_full_text_no_collision() {
        // "hello"→"helps"：首字 h/5 字/首字坐标同——全量 hash 必变（采样才会漏判）。
        let a = text_rn_content(16.0, [1.0; 4], &[104, 101, 108, 108, 111]); // hello
        let b = text_rn_content(16.0, [1.0; 4], &[104, 101, 108, 112, 115]); // helps
        assert_ne!(
            payload_hash(&a),
            payload_hash(&b),
            "全量 codepoint → hash 变"
        );
    }

    // -----------------------------------------------------------------------
    // P2/P4 补字段回归测试（reuse_key 进 header_hash；line 全量进 payload_hash）
    // -----------------------------------------------------------------------

    #[test]
    fn header_hash_includes_reuse_key() {
        // P2：reuse_key 变 → header_hash 变（身份字段进表头 hash）。
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
    fn payload_hash_text_includes_line_height() {
        // P4：line.height 变（line-height 改）→ payload_hash 变。全量不采样，line 字段必入 hash。
        let a = text_rn_content(16.0, [1.0; 4], &[104, 101, 108]); // hel
        let mut b = text_rn_content(16.0, [1.0; 4], &[104, 101, 108]);
        if let NodePayload::Text { layout, .. } = &mut b.payload {
            layout.lines[0].height = 30.0; // 改 line-height
        }
        assert_ne!(
            payload_hash(&a),
            payload_hash(&b),
            "line.height 变 → payload_hash 变"
        );
    }

    #[test]
    fn payload_hash_text_includes_line_baseline_y() {
        // P4：line.baseline/y 变 → payload_hash 变（行位置变，quad 跟着变）。
        let a = text_rn_content(16.0, [1.0; 4], &[104, 101, 108]);
        let mut b = text_rn_content(16.0, [1.0; 4], &[104, 101, 108]);
        if let NodePayload::Text { layout, .. } = &mut b.payload {
            layout.lines[0].baseline = 20.0;
            layout.lines[0].y = 4.0;
        }
        assert_ne!(
            payload_hash(&a),
            payload_hash(&b),
            "line.baseline/y 变 → payload_hash 变"
        );
    }
}
