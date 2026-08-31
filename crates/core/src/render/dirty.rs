//! dirty hash：header_hash（表头） + payload_hash（几何），供 Stage 跨帧分轴比较定
//! ChangeLevel。碰撞最坏 1 帧延迟，不破正确性。
//! 另有 render_input_fp（A2 增量 build 的输入侧指纹）+ RenderBuildCache——把变更检测
//! 从「重建后验尸」前移到「重建前验输入」，输入未变的节点整段复用上帧产物。

use crate::render::node::{NodePayload, RenderNode};
use crate::scene::node::{Node, NodeKind, Scene};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// 控件壳节点（merge 排除清单同款）：EditState / 视觉同步（fill 宽度、caret、光标闪烁）
/// 每帧可变且写点分散——v1 不枚举其输入，直接永不命中缓存（控件稀疏，代价可接受）。
fn never_cache_kind(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Toggle
            | NodeKind::RadioButton
            | NodeKind::Slider
            | NodeKind::ProgressBar
            | NodeKind::Dropdown
            | NodeKind::OptionItem
            | NodeKind::TextField
            | NodeKind::TextArea
            | NodeKind::NumberField
    )
}

/// A2 增量 build 的单节点输入指纹：命中 → 跳过 render_one_node，整段复用上帧产物。
///
/// 输入枚举（漏一项 = 陈旧帧，A/B 对拍测试兜底）：
/// - `render_input_version`：style 实际改写（rematch 值比较）/ set_src。
/// - `layout_rect` 宽高（0.25px 量化，同 text_fingerprint 桶纪律）：几何随尺寸变。
///   x/y 不进——纯平移节点 rect.x/y = wm 平移分量（wm 已含），非纯平移恒 (0,0)。
/// - `world_transforms` 全量 + 累积 alpha：v1 正确性优先——动的节点重建（不劣于全量
///   路径），静态场景全命中（增量 build 的目标态：稳态帧零几何重建）。
/// - `text_layout_versions`：TextLayout 重算而 style 未变的通道（约束宽变导致重测）。
/// - `anim` 烘进 mesh 的通道（bg_color/text_color/box_shadow）；opacity/transform 经
///   alpha/wm 覆盖，width/height/flex_grow 经 layout_rect 覆盖，不重复进键。
/// - `res_gen`（宿主资源代数）：image_sizes / 字体注册表变更全局失效。
/// - `rich_text_block` 位：容器臂选择（折 inline flow vs 常规子树）。
/// - `image_srcs`：Image intrinsic 几何源（set_src 也 bump version，双保险）。
/// - `visible`（累积渲染隐藏，祖先任一 render_hidden 即真）：visibility 继承语义下
///   后代行的 visible 位随祖先翻转，必须进键防缓存陈旧。
pub fn render_input_fp(
    n: &Node,
    scene: &Scene,
    alpha: f32,
    visible: bool,
    res_gen: u64,
    frame_no: u64,
) -> u64 {
    let mut h = DefaultHasher::new();
    if never_cache_kind(n.kind) {
        // 控件壳：每帧唯一值（frame_no 单调）→ 永不命中。
        0xC0FF_EE00u64.hash(&mut h);
        frame_no.hash(&mut h);
        return h.finish();
    }
    n.render_input_version.hash(&mut h);
    visible.hash(&mut h);
    ((n.layout_rect.w * 4.0).round() as i64).hash(&mut h);
    ((n.layout_rect.h * 4.0).round() as i64).hash(&mut h);
    n.rich_text_block.hash(&mut h);
    for &v in scene
        .world_transforms
        .get(n.id.index())
        .copied()
        .unwrap_or(crate::transform::IDENTITY)
        .iter()
    {
        v.to_le_bytes().hash(&mut h);
    }
    alpha.to_le_bytes().hash(&mut h);
    scene
        .text_layout_versions
        .get(n.id.index())
        .copied()
        .unwrap_or(0)
        .hash(&mut h);
    if let Some(anim) = scene.anim.get(n.id) {
        // Some/None 判别必须进键：Some([0;4])（动画到透明黑）≠ None（退回 CSS 色）。
        match anim.bg_color {
            Some(c) => {
                1u8.hash(&mut h);
                for &x in c.iter() {
                    x.to_le_bytes().hash(&mut h);
                }
            }
            None => 0u8.hash(&mut h),
        }
        match anim.text_color {
            Some(c) => {
                1u8.hash(&mut h);
                for &x in c.iter() {
                    x.to_le_bytes().hash(&mut h);
                }
            }
            None => 0u8.hash(&mut h),
        }
        match &anim.box_shadow {
            Some(shadows) => {
                1u8.hash(&mut h);
                shadows.len().hash(&mut h);
                for s in shadows {
                    for &f in [s.ox, s.oy, s.spread, s.blur].iter() {
                        f.to_le_bytes().hash(&mut h);
                    }
                    for &c in s.color.iter() {
                        c.to_le_bytes().hash(&mut h);
                    }
                    s.inset.hash(&mut h);
                }
            }
            None => 0u8.hash(&mut h),
        }
    }
    res_gen.hash(&mut h);
    if let Some(src) = scene.image_srcs.get(&n.id) {
        src.hash(&mut h);
    }
    h.finish()
}

/// 跨帧 render build 缓存（Stage 持有）。present-set 签名不等（增删/换父/display 翻转/
/// popup 开合/fold 变化——任何改变「哪些节点进渲染」的事件）→ 整表清空兜底。
#[derive(Default)]
pub struct RenderBuildCache {
    pub entries: std::collections::HashMap<crate::scene::node::NodeId, CachedNodeBuild>,
    pub structure_sig: u64,
    /// 诊断/测试观测：指纹命中数与重建数（含结构兜底清空后的重建）。
    pub hits: u64,
    pub misses: u64,
}

/// 单 scene 节点的上帧产物（含全部合成层：text 子页 / 外阴影 / 行内图）。
pub struct CachedNodeBuild {
    pub input_fp: u64,
    /// 该节点产出的全部 RenderNode（顺序 = 产出序）。replay 整段 clone。
    pub nodes: Vec<RenderNode>,
    /// primary（真 node_id）在 nodes 内的下标（id_to_pos 注册用）。
    pub primary_idx: usize,
    /// 外阴影合成对（primary_id, synth_id）——replay 续接 sort 传播。
    pub back_pairs: Vec<(u64, u64)>,
    /// 富文本行内图合成对。
    pub inline_pairs: Vec<(u64, u64)>,
    /// 与 nodes 对齐的 payload_hash（存入时算好；命中行 hash 定级 pass 免重算几何 hash）。
    pub phs: Vec<u64>,
}

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
