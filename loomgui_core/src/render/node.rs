//! Render 层契约：RenderNode。
//!
//! `build_render_nodes` 遍历 solve 后的 `Scene`，为每个 `Node` 产一个 `RenderNode`；
//! payload 按节点 kind 决定（Container/Button → Mesh quad 背景色；Image → Mesh quad +
//! image_path；Text → measure_text 产 TextLayout）。sort_key / mask_context 由
//! `batch::assign_sort_keys` 后处理。stage 层负责把 `Vec<RenderNode>` diff 成 draw list / JSON。
//!
//! 核心不知图集。Mesh payload 带 `image_path: Option<String>`（Image 节点 /
//! bg-image 容器填 path，纯色容器 None）。path 推给 Unity，Unity 按 path 查 Sprite Atlas
//! 拿 Sprite（含 UV+Texture）。UV 始终全图 (0,0)-(1,1)（Unity Sprite 自带真实 UV；核心无子区概念）。

use serde::Serialize;

/// mask 上下文（rect clip 层级）。
///
/// `MaskContext(0)` = 无 clip；`>0` = clip 层级 id（用出现序作 id）。
/// 由 `batch::assign_sort_keys` 在 BatchingRoot（clip_rect 的 Container）上开新层级，
/// 子树继承。
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Hash)]
pub struct MaskContext(pub u32);

/// 混合模式。仅 Normal。
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub enum BlendMode {
    Normal,
}

/// 帧级变更级别（与 payload_kind「是什么」正交，表示「这帧变了什么」）。
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[repr(u8)]
pub enum ChangeLevel {
    Skip = 0,   // 表头+几何均未变：C# 保留 GO，不碰
    Header = 1, // 只表头变：C# 只改 GO transform/材质，不重建 mesh
    Full = 2,   // 几何变：C# 重建 mesh
}

/// 节点渲染载荷。
///
/// - `Mesh`：quad 几何（背景色块 / 图片 / 文本字形）。`image_path`=None 表示纯色（无贴图），
///   `Some(path)` 为 Image 节点 / bg-image 容器的归一化图片 path，或文本的合成 atlas path
///   `loomgui://font-atlas/p<n>`（核心不知图集，path 推给 Unity 查 Sprite/atlas；
///   atlas 是 Stage 级共享实例，path 只以 page 为键，不含 font_id）。
///   UV 对于 Image/bg 始终 (0,0)-(1,1)（Unity Sprite 自带真实 UV；核心无子区）；
///   对于 text 为 atlas 内的字形子区 UV。`program`=0 = 纯色/无图 Image shader，
///   1 = Text shader，2 = Container+bg-image 合成，3 = filter 无 bg-image，4 = filter+bg-image。
#[derive(Debug, Clone, Serialize)]
pub enum NodePayload {
    Mesh {
        verts: Vec<[f32; 2]>,
        uvs: Vec<[f32; 2]>,
        colors: Vec<[f32; 4]>,
        indices: Vec<u32>,
        image_path: Option<String>, // None=纯色，Some=图片 path 或合成 atlas path
        program: u32,
        color_matrix: [f32; 20], // ColorFilter 矩阵；program≠3/4 全零
    },
}

/// 单个 underlay（shadow）槽：偏移采样 distance + softness（shadow blur 近似）+ 色。
/// color.a=0 → 该槽未启用（shader 据此跳过，effect 无 flags，参数隐含启用）。
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct UnderlaySlot {
    pub offset_x: f32,
    pub offset_y: f32,
    pub softness: f32,
    pub color: [f32; 4],
}

/// 文字效果参数块（per-text-node）。定长，序列化进 FFI SOA 的 effect_block 列（照
/// color_matrix 先例）。无 flags：effect 启用由参数隐含（outline_width>0 /
/// underlay.color.a>0 / glow_color.a>0 / blur_width>0）。Default 全 0 = 无 effect。
///
/// 槽位对标 TextMeshPro（_Outline*/_Underlay*/_Glow*）；多重 shadow 扩展为 underlay[3]
/// （TMP underlay 单槽）。blur 是 LoomGUI 私有近似（TMP 无整字高斯 blur）。
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct EffectBlock {
    pub outline_width: f32,
    pub outline_color: [f32; 4],
    pub underlay: [UnderlaySlot; 3],
    pub glow_power: f32,
    pub glow_color: [f32; 4],
    pub blur_width: f32,
}

impl EffectBlock {
    /// 序列化定长（32 × f32 = 128 字节，小端）。字段顺序固定，FFI blob 写出与 C# 解析、
    /// dirty hash 共用此方法（DRY）。字段顺序 = outline / underlay[3] / glow / blur。
    pub const SIZE: usize = 128;

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        let mut o = 0usize;
        macro_rules! wf {
            ($v:expr) => {
                buf[o..o + 4].copy_from_slice(&($v).to_le_bytes());
                o += 4;
            };
        }
        wf!(self.outline_width);
        for &c in &self.outline_color {
            wf!(c);
        }
        for slot in &self.underlay {
            wf!(slot.offset_x);
            wf!(slot.offset_y);
            wf!(slot.softness);
            for &c in &slot.color {
                wf!(c);
            }
        }
        wf!(self.glow_power);
        for &c in &self.glow_color {
            wf!(c);
        }
        wf!(self.blur_width);
        debug_assert_eq!(o, Self::SIZE, "EffectBlock 字段顺序/数量与 SIZE 不符");
        buf
    }
}

/// 渲染节点（draw list 的最小单元）。
///
/// 字段映射 Node → 渲染语义：
/// - `node_id` / `parent_id`：与 scene.nodes 索引对齐（build 直填 n.id.0）。
/// - `alpha` ← `style.opacity`；`color_tint` ← `style.color`。
/// - `transform.x/y` ← `layout_rect.x/y`（父坐标系）。
/// - `mask_context` / `sort_key`：batch::assign_sort_keys 后填。
#[derive(Debug, Clone, Serialize)]
pub struct RenderNode {
    pub node_id: u32,
    pub parent_id: Option<u32>,
    pub visible: bool,
    pub alpha: f32,
    pub color_tint: [f32; 4],
    pub world_matrix: crate::transform::Affine2,
    pub blend: BlendMode,
    pub mask_context: MaskContext,
    pub sort_key: u32,
    pub change_level: ChangeLevel,
    pub reuse_key: u32,
    pub payload: NodePayload,
}

#[cfg(test)]
mod serde_smoke_tests {
    use super::*;
    #[test]
    fn render_node_serializes_to_json() {
        // 契约：RenderNode 必须能 serde_json::to_string。
        let rn = RenderNode {
            node_id: 0,
            parent_id: None,
            visible: true,
            alpha: 1.0,
            color_tint: [1.0; 4],
            world_matrix: crate::transform::IDENTITY,
            blend: BlendMode::Normal,
            mask_context: MaskContext(2),
            sort_key: 5,
            change_level: ChangeLevel::Full,
            reuse_key: 0,
            payload: NodePayload::Mesh {
                verts: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                colors: vec![[1.0; 4]; 4],
                indices: vec![0, 1, 2, 0, 2, 3],
                image_path: Some("icons/skin.png".into()),
                program: 0,
                color_matrix: [0.0; 20],
            },
        };
        let s = serde_json::to_string(&rn).expect("RenderNode must serialize");
        assert!(s.contains("\"sort_key\":5"));
        assert!(s.contains("\"mask_context\":2"));
        assert!(s.contains("\"image_path\""));
        assert!(s.contains("icons/skin.png"));
        assert!(s.contains("\"Mesh\""));
    }
}
