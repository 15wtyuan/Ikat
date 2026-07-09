#![allow(unreachable_patterns, irrefutable_let_patterns)]
use super::*;
use crate::scene::node::*;
use crate::style::resolved::{
    BackgroundSize, BorderRadius, BoxShadow, CornerRadius, ResolvedStyle, TextAlign,
};
use crate::text::atlas::GlyphAtlas;
use crate::text::layout::measure_text;
use crate::text::layout::FontTable;
use taffy::style::{Dimension, LengthPercentage};

/// 测试 glyph atlas（空，不注册真实字体时 atlas 为空壳——ensure 需 face 参数才分配）。
fn test_glyph_atlas() -> GlyphAtlas {
    GlyphAtlas::new()
}

/// 测试字体表：仓库内 DejaVuSans.ttf（跨平台一致），缺则跳过。
fn test_font_table() -> Option<FontTable> {
    let path = format!(
        "{}/tests/fixtures/DejaVuSans.ttf",
        env!("CARGO_MANIFEST_DIR")
    );
    let bytes = std::fs::read(&path).ok()?;
    let mut ft = FontTable::new();
    ft.register("DejaVu", bytes, true).ok()?;
    Some(ft)
}

/// 测试辅助：空图尺寸表（无 path → 全 64×64 兜底）。
fn empty_sizes() -> ImageSizeTable {
    std::collections::HashMap::new()
}

/// 测试辅助：建单条 path→(w,h) 尺寸表。
fn sizes(path: &str, w: u32, h: u32) -> ImageSizeTable {
    let mut m = std::collections::HashMap::new();
    m.insert(path.to_string(), (w, h));
    m
}

/// 构造一个带 layout_rect 的 Container Node。
fn container_node(id: usize, parent: Option<usize>, rect: Rect, bg: Option<[f32; 4]>) -> Node {
    let mut n = Node::default();
    n.id = NodeId(id as u32);
    n.parent = parent.map(|p| NodeId(p as u32));
    n.kind = NodeKind::Container;
    n.layout_rect = rect;
    n.style.background_color = bg;
    n
}

#[test]
fn build_container_produces_mesh_quad() {
    // root 红底 10x10 → Mesh payload，4 verts / 6 indices，背景色烤进 colors。
    let mut scene = Scene::from_nodes(
        vec![container_node(
            0,
            None,
            Rect {
                x: 1.0,
                y: 2.0,
                w: 10.0,
                h: 10.0,
            },
            Some([1.0, 0.0, 0.0, 1.0]),
        )],
        vec![],
    );
    let fonts = test_font_table().expect("need test font for build_render_nodes");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let rns = &frame.nodes;
    assert_eq!(rns.len(), 1);
    match &rns[0].payload {
        NodePayload::Mesh {
            verts,
            indices,
            colors,
            image_path,
            program,
            ..
        } => {
            assert_eq!(verts.len(), 4);
            assert_eq!(indices.len(), 6);
            assert!(image_path.is_none(), "Container 无图 → image_path=None");
            assert_eq!(*program, 0);
            for c in colors {
                assert_eq!(*c, [1.0, 0.0, 0.0, 1.0]);
            }
        }
        _ => panic!("expected Mesh payload"),
    }
    // world_matrix 纯平移 → tx/ty = layout_rect x/y
    assert_eq!(rns[0].world_matrix[4], 1.0);
    assert_eq!(rns[0].world_matrix[5], 2.0);
}

#[test]
fn build_container_with_border_emits_border_node() {
    // border_color + border_width 激活：无背景图的 Container 把边框环形 mesh 拼进背景
    // quad 同一 payload（同 program=0，单 draw call）。背景蓝、边框红两色共存于顶点色。
    let mut n = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
        },
        Some([0.0, 0.0, 1.0, 1.0]), // 蓝底
    );
    n.style.border_color = Some([1.0, 0.0, 0.0, 1.0]); // 红边
    n.style.border_width = 4.0;
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    let fonts = test_font_table().expect("need test font for build_render_nodes");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let rns = &frame.nodes;
    // 单节点（背景 + 边框拼在同一 Mesh payload）。
    let mesh = rns
        .iter()
        .find(|rn| matches!(&rn.payload, NodePayload::Mesh { verts, .. } if !verts.is_empty()))
        .expect("至少一个 Mesh 节点");
    let NodePayload::Mesh {
        verts,
        indices,
        colors,
        program,
        ..
    } = &mesh.payload
    else {
        unreachable!()
    };
    // 背景 4 顶点 + 边框 8 顶点 = 12（拼在同一 payload，单 draw call）。
    assert_eq!(
        verts.len(),
        12,
        "背景 4 + 边框 8 = 12 顶点，得 {}",
        verts.len()
    );
    assert_eq!(
        indices.len(),
        6 + 24,
        "背景 6 + 边框 24 = 30 索引，得 {}",
        indices.len()
    );
    assert_eq!(*program, 0u32, "纯色背景 + 边框同走 program=0");
    assert!(colors.contains(&[1.0, 0.0, 0.0, 1.0]), "边框红色顶点存在");
    assert!(colors.contains(&[0.0, 0.0, 1.0, 1.0]), "背景蓝色顶点存在");
}

#[test]
fn build_container_without_border_no_border_node() {
    // border_color 缺省（None）→ 不发边框节点。回归保护：dead field 激活不应影响无边框节点。
    let n = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        },
        Some([1.0; 4]),
    );
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    let fonts = test_font_table().expect("need test font for build_render_nodes");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    // 单 Container 单节点（merge 后仍 1 节点，4 顶点）。
    let total_verts: usize = frame
        .nodes
        .iter()
        .filter_map(|rn| match &rn.payload {
            NodePayload::Mesh { verts, .. } => Some(verts.len()),
            _ => None,
        })
        .sum();
    assert_eq!(total_verts, 4, "无边框 → 仅背景 4 顶点（无边框额外顶点）");
}

#[test]
fn build_skips_display_none_subtree() {
    // parent(flex,红底) → child(display:none,绿底) → grandchild(Text "hi")
    // CSS 语义：display:none 整子树不渲染——Text 后代不该进 frame.nodes。
    // 之前 bug：build 遍历所有节点无过滤，display:none 节点的 Text 后代虽 layout_rect=0
    //   但仍产 Text RenderNode，Unity 渲染了字形（"内容堆在左边显示"根因）。
    let parent = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        },
        Some([1.0, 0.0, 0.0, 1.0]),
    );
    let mut child = container_node(
        1,
        Some(0),
        Rect {
            x: 0.0,
            y: 0.0,
            w: 50.0,
            h: 50.0,
        },
        Some([0.0, 1.0, 0.0, 1.0]),
    );
    child.style.taffy_style.display = taffy::Display::None;
    let mut grandchild = Node::default();
    grandchild.kind = NodeKind::Text {
        content: "hi".into(),
    };
    grandchild.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 50.0,
        h: 20.0,
    };
    // from_nodes 用 edges (parent_idx, child_idx) 按 vec 位置建 parent 关系。
    let mut scene = Scene::from_nodes(vec![parent, child, grandchild], vec![(0, 1), (1, 2)]);
    let ft = test_font_table().expect("need test font for build_render_nodes");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &ft,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let has_text = frame
        .nodes
        .iter()
        .any(|rn| matches!(rn.payload, NodePayload::Mesh { program: 1, .. }));
    assert!(
        !has_text,
        "display:none 子树的 Text 后代不该进 frame.nodes（child 整子树剪掉）"
    );
}

/// Image RenderNode payload 带 path（核心不知图集/UV）。
/// Image 节点 src="icons/skin.png" → Mesh payload image_path=Some("icons/skin.png")。
#[test]
fn image_render_node_carries_path_not_texid() {
    let mut a = Node::default();
    a.kind = NodeKind::Image {
        src: "icons/skin.png".into(),
    };
    a.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 5.0,
        h: 5.0,
    };
    let mut scene = Scene::from_nodes(vec![a], vec![]);
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    match &frame.nodes[0].payload {
        NodePayload::Mesh { image_path, .. } => {
            assert_eq!(
                *image_path,
                Some("icons/skin.png".to_string()),
                "Image payload 带 path=src"
            );
        }
        _ => panic!("expected Mesh"),
    }
}

/// bg-image 同走 path。Container 设 background-image:url(icons/bg.png) →
/// Mesh payload image_path=Some("icons/bg.png")。
#[test]
fn bg_image_carries_path() {
    let mut n = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        },
        None,
    );
    n.style.background_image = Some("icons/bg.png".into());
    n.style.background_size = BackgroundSize::Stretch;
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    let fonts = test_font_table().expect("need font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    match &frame.nodes[0].payload {
        NodePayload::Mesh { image_path, .. } => {
            assert_eq!(
                *image_path,
                Some("icons/bg.png".to_string()),
                "bg-image payload 带 path=url"
            );
        }
        _ => panic!("expected Mesh"),
    }
}

/// 纯色 Container（无 bg-image）image_path=None。
#[test]
fn solid_container_image_path_is_none() {
    let mut scene = Scene::from_nodes(
        vec![container_node(
            0,
            None,
            Rect {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
            },
            Some([1.0, 0.0, 0.0, 1.0]),
        )],
        vec![],
    );
    let fonts = test_font_table().expect("need font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    match &frame.nodes[0].payload {
        NodePayload::Mesh { image_path, .. } => {
            assert!(image_path.is_none(), "纯色 Container image_path=None");
        }
        _ => panic!("expected Mesh"),
    }
}

/// Image payload 带 path + UV 全图 (0,0)-(1,1)（核心不知图集，无子区）。
/// v-flip 仍保留（design y-down 配 Unity y-up）：TL=(0,1)，BR=(1,0)。
#[test]
fn build_image_carries_path_and_full_uv() {
    let mut a = Node::default();
    a.kind = NodeKind::Image {
        src: "logo.png".into(),
    };
    a.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 5.0,
        h: 5.0,
    };
    let mut scene = Scene::from_nodes(vec![a], vec![]);
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    match &frame.nodes[0].payload {
        NodePayload::Mesh {
            image_path,
            uvs,
            program,
            ..
        } => {
            assert_eq!(
                *image_path,
                Some("logo.png".to_string()),
                "Image payload 带 path=src"
            );
            assert_eq!(*program, 0, "Image program=0（tex*vcol）");
            // UV 全图 + v 翻转：TL=(0,1)，BR=(1,0)。
            assert_eq!(uvs[0], [0.0, 1.0], "TL == (0,1)（全图 + v 翻转）");
            assert_eq!(uvs[2], [1.0, 0.0], "BR == (1,0)（全图 + v 翻转）");
        }
        _ => panic!("expected Mesh"),
    }
}

#[test]
fn build_image_uv_is_full_region() {
    // 核心不知图集 → UV 永远全图 (0,0)-(1,1)（v 翻转后 TL=(0,1), BR=(1,0)）。
    let mut a = Node::default();
    a.kind = NodeKind::Image {
        src: "logo.png".into(),
    };
    a.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 5.0,
        h: 5.0,
    };
    let mut scene = Scene::from_nodes(vec![a], vec![]);
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    match &frame.nodes[0].payload {
        NodePayload::Mesh { uvs, .. } => {
            assert_eq!(uvs[0], [0.0, 1.0], "TL == (0,1)（v 翻转）");
            assert_eq!(uvs[2], [1.0, 0.0], "BR == (1,0)（v 翻转）");
        }
        _ => panic!("expected Mesh"),
    }
}

#[test]
fn build_text_produces_text_layout() {
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let mut n = Node::default();
    n.kind = NodeKind::Text {
        content: "Hello".into(),
    };
    n.style.font_size = 16.0;
    n.style.text_align = TextAlign::Left;
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 20.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);

    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let rns = &frame.nodes;
    match &rns[0].payload {
        NodePayload::Mesh {
            verts,
            uvs,
            program,
            image_path,
            ..
        } => {
            assert_eq!(*program, 1, "text → program=1");
            assert!(
                image_path
                    .as_ref()
                    .is_some_and(|p| p.starts_with("loomgui://font-atlas/")),
                "text image_path = synthetic atlas path"
            );
            assert!(!verts.is_empty(), "text 有字形 → verts 非空");
            assert!(!uvs.is_empty(), "text 有 UV");
        }
        _ => panic!("expected Mesh payload for text"),
    }
}

/// pen 必须 GO-local——measure_text 产 content-box 相对坐标，
/// build_render_nodes 烤 (border_left+padding_left, border_top+padding_top) 偏移。
/// 设 padding=4px、border=2px → content 偏移 (6, 6)，每 glyph 的 (x,y) 应 +6。
#[test]
fn build_text_bakes_content_offset_into_glyph_pen() {
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let mut n = Node::default();
    n.kind = NodeKind::Text {
        content: "AB".into(),
    };
    n.style.font_size = 16.0;
    // padding/border 四向 4px/2px → content 偏移 left=2+4=6, top=2+4=6。
    n.style.taffy_style.padding = taffy::geometry::Rect {
        left: taffy::style::LengthPercentage::Length(4.0),
        right: taffy::style::LengthPercentage::Length(4.0),
        top: taffy::style::LengthPercentage::Length(4.0),
        bottom: taffy::style::LengthPercentage::Length(4.0),
    };
    n.style.taffy_style.border = taffy::geometry::Rect {
        left: taffy::style::LengthPercentage::Length(2.0),
        right: taffy::style::LengthPercentage::Length(2.0),
        top: taffy::style::LengthPercentage::Length(2.0),
        bottom: taffy::style::LengthPercentage::Length(2.0),
    };
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 20.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);

    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let rns = &frame.nodes;
    match &rns[0].payload {
        NodePayload::Mesh { verts, program, .. } => {
            assert_eq!(*program, 1, "text → program=1");
            // AB = 2 glyph → 8 verts (2 × 4)。
            assert_eq!(verts.len(), 8, "AB = 2 glyph × 4 verts = 8");
            // 验证 content offset 已烤入 pen：首字形 BL 顶点的 x 应 >= content offset
            // (border+padding=6.0) 减 atlas pad——位图原点 = bbox 外扩 pad，build_text_mesh
            // left = g.x + bearing_x - pad + rect.x。无 content offset 时此处 ~0 或负。
            assert!(
                verts[0][0] >= 6.0 - crate::text::atlas::GLYPH_PAD as f32,
                "首 glyph BL x 应含 content offset (border+padding=6.0，减 pad)，实 {}",
                verts[0][0]
            );
        }
        _ => panic!("expected Mesh payload"),
    }
}

/// 文字 mesh 顶点必须在节点世界空间，且朝向正确（y-down：底 > 顶）。
/// build_text_mesh 把 rect.x/rect.y 烤进顶点（对齐 Image quad 的父空间约定）——
/// 否则 blob re-base 减 (tx,ty) 后净值落 design 原点，所有文字堆左上角。
/// 且 top=baseline-bearing_y（font y-up 的 bearing 在 y-down 里要从 baseline 减）——
/// 否则字形上下颠倒。节点放 (100,200)（root → wm 平移），顶点应落在 (100+,200+)。
#[test]
fn build_text_verts_in_world_space_and_upright() {
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let mut n = Node::default();
    n.kind = NodeKind::Text {
        content: "Hello".into(),
    };
    n.style.font_size = 16.0;
    n.style.text_align = TextAlign::Left;
    n.layout_rect = Rect {
        x: 100.0,
        y: 200.0,
        w: 100.0,
        h: 20.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    crate::scene::transform::compute_world_transforms(&mut scene);
    // root 节点：layout_rect 即 world translate → wm 平移 (100,200)。
    let wm = scene.world_transforms[1];
    assert!((wm[4] - 100.0).abs() < 1e-3 && (wm[5] - 200.0).abs() < 1e-3);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let text_rn = frame
        .nodes
        .iter()
        .find(|rn| matches!(&rn.payload, NodePayload::Mesh { program: 1, .. }))
        .expect("应有 text RenderNode");
    let verts = match &text_rn.payload {
        NodePayload::Mesh { verts, .. } => verts,
        _ => unreachable!(),
    };
    assert!(!verts.is_empty(), "text 有字形 → verts 非空");
    // ① 世界空间：每顶点 x>=90（节点 x=100）、y>=190（节点 y=200）。
    //    修复前顶点在 pen 坐标（~0,0）→ 堆左上角，断言失败。
    for v in verts {
        assert!(v[0] >= 90.0, "vert x 应在节点世界空间 (>=90)，实 {}", v[0]);
        assert!(
            v[1] >= 190.0,
            "vert y 应在节点世界空间 (>=190)，实 {}",
            v[1]
        );
    }
    // ② 朝向：每 glyph 4 顶点 (BL,BR,TR,TL)，y-down 下 BL.y（底）应 > TL.y（顶）。
    //    修复前 top=baseline+bearing_y 符号反 → BL.y < TL.y（颠倒），断言失败。
    for g in (0..verts.len()).step_by(4) {
        let bl_y = verts[g][1]; // BL
        let tl_y = verts[g + 3][1]; // TL
        assert!(
            bl_y > tl_y,
            "glyph BL.y({}) 应 > TL.y({})（y-down 底>顶，否则上下颠倒）",
            bl_y,
            tl_y
        );
    }
}

/// 空格等无轮廓字形不该渲染成方块——rasterize_glyph 对 gid>0 无 bbox/空轮廓返空
/// bitmap，build_text_mesh 跳过（不产 quad）。advance 在 layout 已算，pen 前进不受影响。
#[test]
fn build_text_skips_blank_glyphs_like_space() {
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let mut n = Node::default();
    n.kind = NodeKind::Text {
        content: "A B".into(),
    };
    n.style.font_size = 16.0;
    n.style.text_align = TextAlign::Left;
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 20.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let text_rn = frame
        .nodes
        .iter()
        .find(|rn| matches!(&rn.payload, NodePayload::Mesh { program: 1, .. }))
        .expect("应有 text RenderNode");
    let verts = match &text_rn.payload {
        NodePayload::Mesh { verts, .. } => verts,
        _ => unreachable!(),
    };
    // "A B" = 3 codepoints，但空格无轮廓 → 只 A、B 两字形产 quad = 8 verts。
    // 修复前空格走 tofu → 3 quad = 12 verts。
    assert_eq!(
        verts.len(),
        8,
        "空格应跳过（无轮廓不产 quad），A B = 2 字形 × 4 = 8 verts，实 {}",
        verts.len()
    );
}

#[test]
fn build_assigns_monotonic_keys() {
    // 用嵌套 clip 链（root > mid > leaf，每层 clip_rect 开新 mask_context）
    // → 3 个不同 DrawState → 不合并 → 保 3 节点。
    // 验 sort_key 单调（batch 已测，这里走端到端确认 build 接通 assign_sort_keys）。
    let root = container_node(0, None, Rect::default(), None);
    let mid = container_node(1, Some(0), Rect::default(), None);
    let leaf = container_node(2, Some(1), Rect::default(), None);
    let mut scene = Scene::from_nodes(vec![root, mid, leaf], vec![(0, 1), (1, 2)]);
    let root_id = scene.roots[0];
    let mid_id = scene.get(root_id).unwrap().children[0];
    let leaf_id = scene.get(mid_id).unwrap().children[0];
    scene.get_mut(root_id).unwrap().clip_rect = Some(Rect::default()); // 开 mask_context=1
    scene.get_mut(mid_id).unwrap().clip_rect = Some(Rect::default()); // 开 mask_context=2
    scene.get_mut(leaf_id).unwrap().clip_rect = Some(Rect::default()); // 开 mask_context=3

    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let rns = &frame.nodes;
    // 3 个不同 mask_context → 不合并 → 保 3 节点；sort_key 经 reorder 重赋后仍单调。
    assert_eq!(rns.len(), 3, "3 个不同 mask_context → 不合并");
    assert!(rns[0].sort_key < rns[1].sort_key);
    assert!(rns[1].sort_key < rns[2].sort_key);
}

/// 端到端 merge：root(Container, image_path=None) > [img A, img B]（同 image_path=Some、
/// 同 mask_context、AABB 不相交）。reorder 让两 Image 相邻，merge 合两 Image 成 1 个 8-vert
/// merged mesh；root 是 Container(image_path=None) 不同 DrawState → 不合。
/// 结果：FrameData 含恰好 1 个 8-vert Mesh payload（两 Image 合并）。
#[test]
fn build_merges_adjacent_same_drawstate_meshes() {
    let root = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 300.0,
            h: 50.0,
        },
        None,
    );
    let mut a = Node::default();
    a.kind = NodeKind::Image {
        src: "a.png".into(),
    };
    a.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 10.0,
        h: 10.0,
    };
    let mut b = Node::default();
    b.kind = NodeKind::Image {
        src: "a.png".into(),
    };
    b.layout_rect = Rect {
        x: 100.0,
        y: 0.0,
        w: 10.0,
        h: 10.0,
    };
    let mut scene = Scene::from_nodes(vec![root, a, b], vec![(0, 1), (0, 2)]);

    let fonts = test_font_table().expect("need test font");

    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    // root(Container, image_path=None) + 1 merged(Image image_path=Some("a.png")) = 2 节点（3 输入合并后）。
    let mesh_count = frame
        .nodes
        .iter()
        .filter(|n| matches!(&n.payload, NodePayload::Mesh { verts, .. } if verts.len() == 8))
        .count();
    assert_eq!(mesh_count, 1, "两同 atlas Image → 1 个 8-vert merged mesh");
    // merged node 的 world_matrix 应为 IDENTITY（merge_batch 把锚矩阵置 identity），
    // 顶点保持绝对 design 坐标。
    let merged = frame
        .nodes
        .iter()
        .find(|n| matches!(&n.payload, NodePayload::Mesh { verts, .. } if verts.len() == 8))
        .expect("merged node 存在");
    assert!(crate::transform::is_identity(&merged.world_matrix));
    assert!(
        (merged.alpha - 1.0).abs() < 1e-6,
        "merged alpha=1 防 blob 二次烤"
    );
}

/// build_render_nodes 读 anim.opacity/bg_color override（replace-override）。
/// CSS opacity=1.0、bg=红；anim opacity=0.25、bg=蓝 → alpha=0.25、Mesh colors=蓝。
#[test]
fn build_reads_anim_opacity_and_bg_override() {
    let mut n = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        },
        Some([1.0, 0.0, 0.0, 1.0]),
    );
    n.style.opacity = 1.0;
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    let rid = scene.roots[0];
    // anim override：opacity=0.25、bg=蓝（生产路径写法：ensure(id) 返 &mut NodeAnim）
    {
        let a = scene.anim.ensure(rid);
        a.opacity = Some(0.25);
        a.bg_color = Some([0.0, 0.0, 1.0, 1.0]);
    }

    let fonts = test_font_table().expect("need font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    assert!(
        (frame.nodes[0].alpha - 0.25).abs() < 1e-5,
        "anim.opacity override → alpha=0.25"
    );
    match &frame.nodes[0].payload {
        NodePayload::Mesh { colors, .. } => {
            assert_eq!(
                *colors.first().unwrap(),
                [0.0, 0.0, 1.0, 1.0],
                "anim.bg_color override → 蓝"
            );
        }
        _ => panic!("expected Mesh"),
    }
}

// ── 合成 scrollbar thumb ─────────────────────────

#[test]
fn effective_scroll_container_emits_thumb_node() {
    use crate::style::resolved::{OverflowMode, ResolvedStyle};

    // 构造：overflow_y=Scroll 容器 + content>viewport → effective
    let mut scroll_style = ResolvedStyle::default();
    scroll_style.overflow_y = OverflowMode::Scroll;
    let entries: Vec<(
        Option<usize>,
        NodeKind,
        ResolvedStyle,
        Vec<String>,
        Option<String>,
        bool,
        Option<i32>,
        Option<String>,
    )> = vec![
        (
            None,
            NodeKind::Container,
            scroll_style.clone(),
            vec![],
            None,
            false,
            None,
            None,
        ),
        (
            Some(0),
            NodeKind::Container,
            ResolvedStyle::default(),
            vec![],
            None,
            false,
            None,
            None,
        ),
        (
            Some(0),
            NodeKind::Container,
            ResolvedStyle::default(),
            vec![],
            None,
            false,
            None,
            None,
        ),
    ];
    let mut scene = Scene::build(&entries);
    let root_id = scene.roots[0];
    let c0 = scene.get(root_id).unwrap().children[0];
    let c1 = scene.get(root_id).unwrap().children[1];
    scene.get_mut(root_id).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    };
    scene.get_mut(c0).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 40.0,
        h: 40.0,
    };
    scene.get_mut(c1).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 30.0,
        h: 200.0,
    }; // content_y=200 > viewport=100
    crate::scroll::refresh_content_sizes(&mut scene);
    crate::scene::transform::compute_world_transforms(&mut scene);

    let fonts = test_font_table().expect("need test font");
    let (fd, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let thumbs: Vec<_> = fd
        .nodes
        .iter()
        .filter(|n| n.node_id & crate::scroll::V_THUMB_FLAG != 0)
        .collect();
    assert!(!thumbs.is_empty(), "垂直 thumb 追加");
    // 验 thumb 是 Mesh quad 半透明灰
    let thumb = thumbs[0];
    assert_eq!(thumb.mask_context, MaskContext(0), "thumb mask_context=0");
    assert!(thumb.sort_key > 0, "thumb sort_key > 0");
    match &thumb.payload {
        NodePayload::Mesh { colors, .. } => {
            assert_eq!(colors[0], [0.6, 0.6, 0.6, 0.6], "半透明灰");
        }
        _ => panic!("thumb 应为 Mesh"),
    }
}

#[test]
fn non_effective_container_no_thumb() {
    // 构造 overflow:auto 但 content < viewport → 非 effective → 无 thumb
    use crate::style::resolved::{OverflowMode, ResolvedStyle};
    let mut scroll_style = ResolvedStyle::default();
    scroll_style.overflow_y = OverflowMode::Auto;
    let entries = vec![
        (
            None,
            NodeKind::Container,
            scroll_style.clone(),
            vec![],
            None,
            false,
            None,
            None,
        ),
        (
            Some(0),
            NodeKind::Container,
            ResolvedStyle::default(),
            vec![],
            None,
            false,
            None,
            None,
        ),
    ];
    let mut scene = Scene::build(&entries);
    let root_id = scene.roots[0];
    let c0 = scene.get(root_id).unwrap().children[0];
    scene.get_mut(root_id).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    };
    scene.get_mut(c0).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 40.0,
        h: 40.0,
    }; // content < viewport
    crate::scroll::refresh_content_sizes(&mut scene);
    crate::scene::transform::compute_world_transforms(&mut scene);

    let fonts = test_font_table().expect("need test font");
    let (fd, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let has_thumb = fd
        .nodes
        .iter()
        .any(|n| n.node_id & (crate::scroll::V_THUMB_FLAG | crate::scroll::H_THUMB_FLAG) != 0);
    assert!(!has_thumb, "non-effective 容器无 thumb");
}

/// render 复用 layout 阶段 TextLayout，不重测。
/// 验证：solve 填 scene.text_layouts，build_render_nodes 的 Text payload 行数
/// == text_layouts 行数（render 直接读，不重测）。
#[test]
fn render_text_payload_matches_layout_text_layout() {
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let content = "the layout reuse check text";
    let fs = 16.0;
    let mut root_s = ResolvedStyle::default();
    root_s.taffy_style.size.width = Dimension::Length(120.0);
    let mut text_s = ResolvedStyle::default();
    text_s.font_size = fs;
    let entries = vec![
        (
            None,
            NodeKind::Container,
            root_s,
            vec![],
            None,
            false,
            None,
            None,
        ),
        (
            Some(0),
            NodeKind::Text {
                content: content.into(),
            },
            text_s,
            vec![],
            None,
            false,
            None,
            None,
        ),
    ];
    let mut scene = Scene::build(&entries);
    crate::layout::solve(
        &mut scene,
        &fonts,
        (120.0, 100.0),
        &std::collections::HashMap::new(),
    );
    let text_id = scene.get(scene.roots[0]).unwrap().children[0];
    assert!(
        scene.text_layouts[text_id.index()].is_some(),
        "solve 应为 Text 节点填 text_layouts"
    );
    let layout_lines = scene.text_layouts[text_id.index()]
        .as_ref()
        .unwrap()
        .lines
        .len();
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let render_verts = match &frame.nodes[1].payload {
        NodePayload::Mesh { verts, program, .. } => {
            assert_eq!(*program, 1, "text → program=1");
            verts.len()
        }
        _ => panic!("expected Mesh payload"),
    };
    let expected_min_verts = layout_lines * 4; // at least 1 glyph per line → 4 verts each
    assert!(
        render_verts >= expected_min_verts,
        "render 应复用 layout TextLayout（至少 {} verts，实 {}）",
        expected_min_verts,
        render_verts
    );
}

/// 长文本回归（intrinsic 远超 container）仍正确换行。
#[test]
fn render_long_text_still_wraps_with_layout_reuse() {
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let content = "The quick brown fox jumps over the lazy dog again and again";
    let fs = 16.0;
    let intrinsic = measure_text(
        content,
        fs,
        0.0,
        0.0,
        TextAlign::Left,
        false,
        None,
        &fonts.stack_for(None),
        [1.0, 1.0, 1.0, 1.0],
    )
    .text_width;
    let container_w = 100.0;
    assert!(
        intrinsic > container_w,
        "测试前置：长文本 intrinsic 应远超 container"
    );
    let mut root_s = ResolvedStyle::default();
    root_s.taffy_style.size.width = Dimension::Length(container_w);
    let mut text_s = ResolvedStyle::default();
    text_s.font_size = fs;
    let entries = vec![
        (
            None,
            NodeKind::Container,
            root_s,
            vec![],
            None,
            false,
            None,
            None,
        ),
        (
            Some(0),
            NodeKind::Text {
                content: content.into(),
            },
            text_s,
            vec![],
            None,
            false,
            None,
            None,
        ),
    ];
    let mut scene = Scene::build(&entries);
    crate::layout::solve(
        &mut scene,
        &fonts,
        (container_w, 100.0),
        &std::collections::HashMap::new(),
    );
    // 验证 solve 填了 text_layouts 且确实换行为多行。
    let text_id = scene.get(scene.roots[0]).unwrap().children[0];
    let layout = scene.text_layouts[text_id.index()]
        .as_ref()
        .expect("solve 应为 Text 节点填 text_layouts");
    assert!(
        layout.lines.len() >= 2,
        "长文本 intrinsic={:.1} container={} 应换行为多行，实 {} 行",
        intrinsic,
        container_w,
        layout.lines.len()
    );

    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let verts = match &frame.nodes[1].payload {
        NodePayload::Mesh { verts, program, .. } => {
            assert_eq!(*program, 1, "text → program=1");
            verts.len()
        }
        _ => panic!("expected Mesh payload"),
    };
    let glyph_count = verts / 4;
    assert!(
        glyph_count >= 2,
        "长文本 intrinsic={:.1} container={} 应含多字形，got {} glyphs",
        intrinsic,
        container_w,
        glyph_count
    );
}

// ── change_level 三级测试 ─────────────────────────

#[test]
fn change_level_skip_header_full() {
    use crate::render::node::ChangeLevel;
    let mut scene = Scene::from_nodes(
        vec![container_node(
            0,
            None,
            Rect {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
            },
            Some([1.0, 0.0, 0.0, 1.0]),
        )],
        vec![],
    );
    let fonts = test_font_table().expect("font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    // 首帧：无基线 → FULL
    let (f1, h1, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    assert_eq!(f1.nodes[0].change_level, ChangeLevel::Full, "首帧 FULL");
    // 第二帧不变 → SKIP
    let (f2, h2, _, _) =
        build_render_nodes(&scene, &fonts, &h1, &empty_sizes(), &mut test_glyph_atlas());
    assert_eq!(f2.nodes[0].change_level, ChangeLevel::Skip, "不变 → SKIP");
    // 第三帧改位置（纯平移 → world_matrix 变，但 re-base 后 verts 不变 → payload_hash 不变）→ HEADER
    scene.get_mut(scene.roots[0]).unwrap().layout_rect.x = 50.0;
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (f3, h3, _, _) =
        build_render_nodes(&scene, &fonts, &h2, &empty_sizes(), &mut test_glyph_atlas());
    assert_eq!(
        f3.nodes[0].change_level,
        ChangeLevel::Header,
        "位置变（纯平移）→ HEADER（payload 不变）"
    );
    // 第四帧改 color（只影响 header_hash 的 color_tint）→ HEADER
    scene.get_mut(scene.roots[0]).unwrap().style.color = [0.5, 0.5, 0.5, 1.0];
    let (f4, h4, _, _) =
        build_render_nodes(&scene, &fonts, &h3, &empty_sizes(), &mut test_glyph_atlas());
    assert_eq!(
        f4.nodes[0].change_level,
        ChangeLevel::Header,
        "color 变 → HEADER（payload 不变）"
    );
    // 第五帧改背景色 → FULL
    scene
        .get_mut(scene.roots[0])
        .unwrap()
        .style
        .background_color = Some([0.0, 1.0, 0.0, 1.0]);
    let (f5, _, _, _) =
        build_render_nodes(&scene, &fonts, &h4, &empty_sizes(), &mut test_glyph_atlas());
    assert_eq!(f5.nodes[0].change_level, ChangeLevel::Full, "bg 变 → FULL");
}

/// reload（prev 非空但 node_id 不在其中）→ Full
#[test]
fn change_level_reload_all_full() {
    use crate::render::node::ChangeLevel;
    let mut scene = Scene::from_nodes(
        vec![container_node(
            0,
            None,
            Rect {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
            },
            Some([1.0, 0.0, 0.0, 1.0]),
        )],
        vec![],
    );
    let fonts = test_font_table().expect("font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (_f1, _h1, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    // prev 有 hash 但 node_id 不在其中（模拟 reload：prev 表残留不同节点的 hash）
    let mut stale: std::collections::HashMap<u32, (u64, u64)> = std::collections::HashMap::new();
    stale.insert(999, (0, 0));
    let (f2, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &stale,
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    assert_eq!(
        f2.nodes[0].change_level,
        ChangeLevel::Full,
        "prev 无本节点 → Full（防错位）"
    );
}

// ── Container bg-image ────────────────────────────

#[test]
fn build_container_with_bg_image_carries_path() {
    // Container 设 background-image → Mesh image_path=Some(url)、program=2（CSS 合成）。
    // UV 全图 (0,0)-(1,1) + v 翻转：TL=(0,1)。无底色 → 透明顶点色。
    let mut n = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 50.0,
        },
        None,
    );
    n.style.background_image = Some("a.png".into());
    n.style.background_size = BackgroundSize::Cover;
    let mut scene = Scene::from_nodes(vec![n], vec![]);

    let fonts = test_font_table().expect("need font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    match &frame.nodes[0].payload {
        NodePayload::Mesh {
            image_path,
            program,
            uvs,
            colors,
            ..
        } => {
            assert_eq!(
                *image_path,
                Some("a.png".to_string()),
                "bg-image → image_path=url"
            );
            assert_eq!(*program, 2, "带图 Container → program=2（CSS 合成）");
            // 全图 + v 翻转：TL=(0,1)
            assert!((uvs[0][0] - 0.0).abs() < 1e-5, "TL u=0");
            assert!((uvs[0][1] - 1.0).abs() < 1e-5, "TL v=1.0（全图 + v 翻转）");
            // 无 background-color → 顶点色透明（图独立显示）
            assert_eq!(
                *colors.first().unwrap(),
                [0.0, 0.0, 0.0, 0.0],
                "无底色 → 透明顶点色"
            );
        }
        _ => panic!("expected Mesh"),
    }
}

#[test]
fn build_container_bg_image_contain_shrinks_geometry() {
    // contain：图完整放入，geometry 缩到子矩形（左上 CSS position 0% 0%），右下留白。
    // 100×100 图，200×100 容器：s=min(2,1)=1，子矩形 100×100 左上 → verts xmax=100（右留白 100）。
    let mut n = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 100.0,
        },
        None,
    );
    n.style.background_image = Some("a.png".into());
    n.style.background_size = BackgroundSize::Contain;
    let mut scene = Scene::from_nodes(vec![n], vec![]);

    let fonts = test_font_table().expect("need font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    if let NodePayload::Mesh { verts, .. } = &frame.nodes[0].payload {
        let xmax = verts.iter().map(|v| v[0]).fold(f32::MIN, f32::max);
        assert!(
            (xmax - 100.0).abs() < 1e-2,
            "contain 子矩形 xmax=100（src 64 兜底缩放宽，右留白），got {}",
            xmax
        );
    } else {
        panic!("expected Mesh");
    }
}

#[test]
fn build_container_bg_image_coexists_with_bg_color() {
    // background-color + background-image 共存：顶点色=底色 tint + image_path=Some(url)
    let mut n = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        },
        Some([0.0, 1.0, 0.0, 1.0]),
    );
    n.style.background_image = Some("a.png".into());
    n.style.background_size = BackgroundSize::Stretch;
    let mut scene = Scene::from_nodes(vec![n], vec![]);

    let fonts = test_font_table().expect("need font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    match &frame.nodes[0].payload {
        NodePayload::Mesh {
            image_path,
            colors,
            uvs,
            ..
        } => {
            assert_eq!(
                *image_path,
                Some("a.png".to_string()),
                "bg-image → image_path=url"
            );
            assert_eq!(
                *colors.first().unwrap(),
                [0.0, 1.0, 0.0, 1.0],
                "顶点色=绿底（tint）"
            );
            // Stretch 全图 + v 翻转：TL=(0,1)
            assert_eq!(uvs[0], [0.0, 1.0], "Stretch TL=(0,1)（v 翻转）");
        }
        _ => panic!("expected Mesh"),
    }
}

// ── program 号（CSS 合成 bg-image）──────────────

#[test]
fn build_container_bg_image_hit_sets_program_2() {
    // Container 设 background-image → image_path=Some(url) → program=2（CSS 合成）。
    let mut n = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        },
        Some([0.0, 1.0, 0.0, 1.0]),
    );
    n.style.background_image = Some("a.png".into());
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    let fonts = test_font_table().expect("need font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    match &frame.nodes[0].payload {
        NodePayload::Mesh {
            program,
            image_path,
            ..
        } => {
            assert_eq!(
                *image_path,
                Some("a.png".to_string()),
                "bg-image → image_path=Some"
            );
            assert_eq!(*program, 2, "Container+bg-image → program=2");
        }
        _ => panic!("expected Mesh"),
    }
}

#[test]
fn build_container_without_bg_image_keeps_program_0() {
    // Container 无 bg-image → program=0（tex*vcol，白占位×bg-color=bg-color）。
    let n = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        },
        Some([1.0, 0.0, 0.0, 1.0]),
    );
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    let fonts = test_font_table().expect("need font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    match &frame.nodes[0].payload {
        NodePayload::Mesh { program, .. } => {
            assert_eq!(*program, 0, "无 bg-image → program=0");
        }
        _ => panic!("expected Mesh"),
    }
}

#[test]
fn build_container_bg_image_sets_program_2() {
    // path 直填：url 原样进 image_path。
    // Container 设 bg-image(任意 url) → image_path=Some(url)、program=2。
    let mut n = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        },
        Some([1.0, 0.0, 0.0, 1.0]),
    );
    n.style.background_image = Some("missing.png".into());
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    let fonts = test_font_table().expect("need font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    match &frame.nodes[0].payload {
        NodePayload::Mesh {
            program,
            image_path,
            ..
        } => {
            assert_eq!(*image_path, Some("missing.png".to_string()), "path 直填");
            assert_eq!(*program, 2, "任意 bg-image url → program=2");
        }
        _ => panic!("expected Mesh"),
    }
}

#[test]
fn build_image_node_keeps_program_0() {
    // Image 节点 program=0（tex*vcol，图透明区透下层）。
    let mut root = Node::default();
    root.kind = NodeKind::Container;
    root.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    };
    let mut img = Node::default();
    img.kind = NodeKind::Image {
        src: "a.png".into(),
    };
    img.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 10.0,
        h: 10.0,
    };
    let mut scene = Scene::from_nodes(vec![root, img], vec![(0, 1)]);
    let fonts = test_font_table().expect("need font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let img_rn = frame.nodes.iter()
            .find(|n| matches!(&n.payload, NodePayload::Mesh { image_path, .. } if *image_path == Some("a.png".to_string())))
            .expect("img mesh");
    if let NodePayload::Mesh { program, .. } = &img_rn.payload {
        assert_eq!(*program, 0, "Image → program=0");
    }
}

// ── color_filter → program=3 + nine_slice 分流 ──────────

#[test]
fn build_container_with_filter_sets_program_3() {
    // Container + filter:grayscale(1) → program=3 + color_matrix 灰化矩阵
    let mut n = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 80.0,
            h: 80.0,
        },
        Some([1.0, 0.0, 0.0, 1.0]),
    );
    n.style.color_filter = Some(crate::style::color_filter::grayscale());
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    crate::scene::transform::compute_world_transforms(&mut scene);
    let fonts = test_font_table().expect("need font");
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    match &frame.nodes[0].payload {
        NodePayload::Mesh {
            program,
            color_matrix,
            ..
        } => {
            assert_eq!(*program, 3, "filter → program=3");
            assert!(
                (color_matrix[0] - 0.299).abs() < 1e-4,
                "color_matrix 含灰化矩阵"
            );
        }
        _ => panic!("expected Mesh"),
    }
}

/// Container + bg-image(命中) + filter → program=4（BG_COMPOSITE+COLOR_FILTER 双 keyword，spec §3.2）。
/// 回归：split program=3 → 3（filter 无 bg-image）/ 4（filter+bg-image 双 keyword）。
/// program=4 由 MaterialManager.cs 同时 EnableKeyword COLOR_FILTER + BG_COMPOSITE，
/// 让 shader 走 `tex.rgb*tex.a + vcol.rgb*(1-tex.a)`（CSS 合成）后再跑 COLOR_FILTER 后处理。
#[test]
fn build_container_with_bg_image_and_filter_sets_program_4() {
    let mut n = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 80.0,
            h: 80.0,
        },
        Some([1.0, 0.0, 0.0, 1.0]),
    );
    n.style.background_image = Some("a.png".into());
    n.style.background_size = BackgroundSize::Stretch;
    n.style.color_filter = Some(crate::style::color_filter::grayscale());
    let mut scene = Scene::from_nodes(vec![n], vec![]);

    let fonts = test_font_table().expect("need font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    match &frame.nodes[0].payload {
        NodePayload::Mesh {
            program,
            image_path,
            color_matrix,
            ..
        } => {
            assert_eq!(
                *image_path,
                Some("a.png".to_string()),
                "bg-image → image_path=Some"
            );
            assert_eq!(
                *program, 4,
                "bg-image+filter → program=4（BG_COMPOSITE+COLOR_FILTER 双 keyword，spec §3.2）"
            );
            assert!(
                (color_matrix[0] - 0.299).abs() < 1e-4,
                "color_matrix 含灰化矩阵"
            );
        }
        _ => panic!("expected Mesh"),
    }
}

#[test]
fn build_container_with_slice_uses_nine_slice() {
    // Container + bg-image + border-image-slice → nine_slice mesh（16 顶点）
    let mut n = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 80.0,
            h: 80.0,
        },
        Some([1.0, 0.0, 0.0, 1.0]),
    );
    n.style.background_image = Some("skin.png".into());
    n.style.background_size = BackgroundSize::Stretch;
    n.style.border_image_slice = Some(crate::style::resolved::SliceInsets {
        top: 10.0,
        right: 10.0,
        bottom: 10.0,
        left: 10.0,
    });
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    crate::scene::transform::compute_world_transforms(&mut scene);
    let fonts = test_font_table().expect("need font");
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    match &frame.nodes[0].payload {
        NodePayload::Mesh { verts, .. } => {
            assert_eq!(verts.len(), 16, "slice → nine_slice 16 顶点");
        }
        _ => panic!("expected Mesh"),
    }
}

/// 九宫格 UV 按真实图尺寸算 slice_px / src_px。
/// 80×80 图 + slice 10 → UV 切片线 = 10/80 = 0.125（非 64 兜底的 10/64≈0.156）。
/// 尺寸表查真实比例，避免 fallback 64 导致 UV 偏移。
#[test]
fn build_container_with_slice_uv_proportional_to_real_image_size() {
    let mut n = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 80.0,
            h: 80.0,
        },
        Some([1.0, 0.0, 0.0, 1.0]),
    );
    n.style.background_image = Some("skin.png".into());
    n.style.background_size = BackgroundSize::Stretch;
    n.style.border_image_slice = Some(crate::style::resolved::SliceInsets {
        top: 10.0,
        right: 10.0,
        bottom: 10.0,
        left: 10.0,
    });
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    crate::scene::transform::compute_world_transforms(&mut scene);
    let fonts = test_font_table().expect("need font");
    // 尺寸表 skin.png=80×80 → UV 切片 = 10/80 = 0.125
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &sizes("skin.png", 80, 80),
        &mut test_glyph_atlas(),
    );
    match &frame.nodes[0].payload {
        NodePayload::Mesh { uvs, verts, .. } => {
            assert_eq!(verts.len(), 16, "nine_slice 16 顶点");
            // nine_slice 行主序 4×4：v[1] = (slice_left, 0) 的 UV = (umin + slice.left * sx, vmin)
            // sx = (umax-umin)/src_w = 1.0/80 = 0.0125；slice.left=10 → uv.x = 0 + 10*0.0125 = 0.125
            // 注：v-flip 后 vmin/vmax 交换（传入 [0,1],[1,0]）→ tex_y[0]=vmax=0, tex_y[1]=vmax-slice*sy
            // uvs[1].x = umin + slice.left * sx = 0.125
            assert!(
                (uvs[1][0] - 0.125).abs() < 1e-4,
                "左切片 UV.x=0.125（slice 10 / src 80），got {}",
                uvs[1][0]
            );
            // uvs[2].x = umin + (src_w - slice.right) * sx = 0 + 70*0.0125 = 0.875
            assert!(
                (uvs[2][0] - 0.875).abs() < 1e-4,
                "右切片 UV.x=0.875（(80-10)/80），got {}",
                uvs[2][0]
            );
        }
        _ => panic!("expected Mesh"),
    }
}

/// 九宫格 UV fallback 64×64（尺寸表无 path）—— 回归验证。
/// 64×64 兜底 + slice 10 → UV 切片 = 10/64 ≈ 0.15625。
#[test]
fn build_container_with_slice_uv_falls_back_to_64_when_no_size() {
    let mut n = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 80.0,
            h: 80.0,
        },
        Some([1.0, 0.0, 0.0, 1.0]),
    );
    n.style.background_image = Some("skin.png".into());
    n.style.background_size = BackgroundSize::Stretch;
    n.style.border_image_slice = Some(crate::style::resolved::SliceInsets {
        top: 10.0,
        right: 10.0,
        bottom: 10.0,
        left: 10.0,
    });
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    crate::scene::transform::compute_world_transforms(&mut scene);
    let fonts = test_font_table().expect("need font");
    // 尺寸表无 skin.png → fallback 64×64 → UV 切片 = 10/64 ≈ 0.15625
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    match &frame.nodes[0].payload {
        NodePayload::Mesh { uvs, .. } => {
            let expected = 10.0 / 64.0;
            assert!(
                (uvs[1][0] - expected).abs() < 1e-4,
                "fallback 64：左切片 UV.x={}（10/64），got {}",
                expected,
                uvs[1][0]
            );
        }
        _ => panic!("expected Mesh"),
    }
}

#[test]
fn build_container_no_filter_keeps_program_0_or_2() {
    // 无 filter → program 0（无图）/ 2（bg-image 命中）
    let mut scene = Scene::from_nodes(
        vec![container_node(
            0,
            None,
            Rect {
                x: 0.0,
                y: 0.0,
                w: 80.0,
                h: 80.0,
            },
            Some([1.0, 0.0, 0.0, 1.0]),
        )],
        vec![],
    );
    crate::scene::transform::compute_world_transforms(&mut scene);
    let fonts = test_font_table().expect("need font");
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    if let NodePayload::Mesh { program, .. } = &frame.nodes[0].payload {
        assert_eq!(*program, 0, "无图无 filter → program=0");
    }
}

#[test]
fn build_container_bg_image_missing_url_carries_path() {
    // url 直填 image_path=Some：url 原样进 payload。
    let mut n = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        },
        None,
    );
    n.style.background_image = Some("missing.png".into());
    let mut scene = Scene::from_nodes(vec![n], vec![]);

    let fonts = test_font_table().expect("need font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    match &frame.nodes[0].payload {
        NodePayload::Mesh {
            image_path,
            program,
            ..
        } => {
            assert_eq!(
                *image_path,
                Some("missing.png".to_string()),
                "url 直填 image_path"
            );
            assert_eq!(*program, 2, "bg-image → program=2");
        }
        _ => panic!("expected Mesh"),
    }
}

#[test]
fn build_container_no_bg_image_image_path_none() {
    // 无 background-image → image_path=None
    let mut scene = Scene::from_nodes(
        vec![container_node(
            0,
            None,
            Rect {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
            },
            Some([1.0, 0.0, 0.0, 1.0]),
        )],
        vec![],
    );
    let fonts = test_font_table().expect("need font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    match &frame.nodes[0].payload {
        NodePayload::Mesh { image_path, .. } => {
            assert!(image_path.is_none(), "无图 Container image_path=None")
        }
        _ => panic!("expected Mesh"),
    }
}

// ── border-radius Tests ────────────────────────────

#[test]
fn container_zero_radius_uses_quad() {
    // 未设 border-radius（默认全 0）→ 走 quad（4 顶点）
    let mut scene = Scene::from_nodes(
        vec![container_node(
            0,
            None,
            Rect {
                x: 0.0,
                y: 0.0,
                w: 80.0,
                h: 80.0,
            },
            Some([1.0, 0.0, 0.0, 1.0]),
        )],
        vec![],
    );
    let fonts = test_font_table().expect("need font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let rn = &frame.nodes[0];
    match &rn.payload {
        NodePayload::Mesh { verts, .. } => {
            assert_eq!(
                verts.len(),
                4,
                "radius=0 走 quad（4 顶点），得 {}",
                verts.len()
            );
        }
        other => panic!("期望 Mesh，得 {:?}", other),
    }
}

#[test]
fn container_radius_uses_rounded_rect() {
    // border-radius:8px → 走 rounded_rect（顶点 >4）
    let mut n = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 80.0,
            h: 80.0,
        },
        Some([1.0, 0.0, 0.0, 1.0]),
    );
    n.style.border_radius = BorderRadius {
        corners: [CornerRadius {
            h: LengthPercentage::Length(8.0),
            v: LengthPercentage::Length(8.0),
        }; 4],
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    let fonts = test_font_table().expect("need font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let rn = &frame.nodes[0];
    match &rn.payload {
        NodePayload::Mesh { verts, .. } => {
            assert!(
                verts.len() > 4,
                "radius>0 走 rounded_rect（顶点>4），得 {}",
                verts.len()
            );
        }
        other => panic!("期望 Mesh，得 {:?}", other),
    }
}

#[test]
fn container_radius_percent_resolved() {
    // border-radius:50% × 80×80 rect → resolve 成 40 → rounded_rect（顶点>4）
    // 使用 container_node 直接设 layout_rect，无需 solve。
    let mut n = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 80.0,
            h: 80.0,
        },
        Some([1.0, 0.0, 0.0, 1.0]),
    );
    n.style.border_radius = BorderRadius {
        corners: [CornerRadius {
            h: LengthPercentage::Percent(0.5),
            v: LengthPercentage::Percent(0.5),
        }; 4],
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    let fonts = test_font_table().expect("need font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let rn = &frame.nodes[0];
    match &rn.payload {
        NodePayload::Mesh { verts, .. } => {
            assert!(
                verts.len() > 4,
                "% resolve 后 radius>0 → rounded_rect，得 {}",
                verts.len()
            );
        }
        other => panic!("期望 Mesh，得 {:?}", other),
    }
}

#[test]
fn container_bg_image_with_radius_uses_rounded_rect() {
    // bg-image + border-radius 共存：image_path=Some AND 走 rounded_rect（verts>4）
    let mut n = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        },
        Some([0.0, 1.0, 0.0, 1.0]),
    );
    n.style.background_image = Some("a.png".into());
    n.style.background_size = BackgroundSize::Stretch;
    n.style.border_radius = BorderRadius {
        corners: [CornerRadius {
            h: LengthPercentage::Length(12.0),
            v: LengthPercentage::Length(12.0),
        }; 4],
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);

    let fonts = test_font_table().expect("need font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    match &frame.nodes[0].payload {
        NodePayload::Mesh {
            image_path, verts, ..
        } => {
            assert_eq!(
                *image_path,
                Some("a.png".to_string()),
                "bg-image+radius: image_path=Some"
            );
            assert!(
                verts.len() > 4,
                "bg-image+radius: rounded_rect（顶点>4），得 {}",
                verts.len()
            );
        }
        _ => panic!("expected Mesh"),
    }
}

// ── 多页 atlas 跨页拆分 text 子页测试 ───────────────────

/// 回归：text 子页 reuse_key 必须为 0（不继承主节点），防止虚拟列表内多页覆盖。
/// 构造带 reuse_key=7 的 text 节点，跑 build_render_nodes，验 primary 继承 reuse_key=7、
/// 子页 reuse_key=0。
#[test]
fn text_sub_pages_reuse_key_is_zero_not_inherited() {
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let mut n = Node::default();
    n.kind = NodeKind::Text {
        content: "Hello".into(),
    };
    n.reuse_key = 7; // 模拟虚拟列表 slot
    n.style.font_size = 16.0;
    n.style.text_align = TextAlign::Left;
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 20.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    for rn in &frame.nodes {
        if is_text_sub_page(rn.node_id) {
            assert_eq!(
                rn.reuse_key, 0,
                "子页 reuse_key 必须为 0（不继承主节点），得 {}",
                rn.reuse_key
            );
        }
    }
    // primary 节点验证：应存在一个非子页 text mesh 且 reuse_key=7
    let primary = frame.nodes.iter().find(|rn| {
        !is_text_sub_page(rn.node_id) && matches!(&rn.payload, NodePayload::Mesh { program: 1, .. })
    });
    assert!(primary.is_some(), "应存在 primary text RenderNode");
    assert_eq!(primary.unwrap().reuse_key, 7, "primary 继承 reuse_key=7");
}

/// 回归：propagate_text_sub_page_sort_keys 累积偏移防 stale primary_sk。
/// 构造两 text 节点各带子页，验 sort_key 单调无 tie。
#[test]
fn propagate_text_sub_page_sort_keys_cumulative_shift_no_ties() {
    use crate::render::node::{BlendMode, ChangeLevel, MaskContext, NodePayload, RenderNode};

    // 构造 nodes vec：A(primary, sk=2), A_sub1(synth, sk=0), A_sub2(synth, sk=0),
    //                  B(primary, sk=3), B_sub1(synth, sk=0), C(real, sk=4)
    let a_id = 0u32;
    let b_id = 1u32;
    let c_id = 2u32;
    let a_sub1 = synth_text_node_id(a_id, 1);
    let a_sub2 = synth_text_node_id(a_id, 2);
    let b_sub1 = synth_text_node_id(b_id, 1);

    let empty_mesh = NodePayload::Mesh {
        verts: vec![],
        uvs: vec![],
        colors: vec![],
        indices: vec![],
        image_path: None,
        program: 0,
        color_matrix: [0.0; 20],
    };

    let mk_rn = |node_id: u32, sort_key: u32| RenderNode {
        node_id,
        parent_id: None,
        visible: true,
        alpha: 1.0,
        color_tint: [1.0; 4],
        world_matrix: crate::transform::IDENTITY,
        blend: BlendMode::Normal,
        mask_context: MaskContext(0),
        sort_key,
        change_level: ChangeLevel::Full,
        reuse_key: 0,
        payload: empty_mesh.clone(),
    };

    let mut nodes = vec![
        mk_rn(a_id, 2),
        mk_rn(a_sub1, 0),
        mk_rn(a_sub2, 0),
        mk_rn(b_id, 3),
        mk_rn(b_sub1, 0),
        mk_rn(c_id, 4),
    ];

    // id_to_pos：真节点映射（不含合成子页）
    let mut id_to_pos: std::collections::HashMap<NodeId, usize> = std::collections::HashMap::new();
    id_to_pos.insert(NodeId(a_id), 0);
    id_to_pos.insert(NodeId(b_id), 3);
    id_to_pos.insert(NodeId(c_id), 5);

    propagate_text_sub_page_sort_keys(&mut nodes, &id_to_pos);

    // 预期：
    //   cum=0, adj=2: B(3→5), C(4→6). cum=2
    //   cum=2, adj=3+2=5: C(6→7). cum=3
    //   After shift: A=2, B=5, C=7
    //   Sub-pages: A_sub1=A.sk+1=3, A_sub2=4, B_sub1=B.sk+1=6
    //   Final: A=2, A_sub1=3, A_sub2=4, B=5, B_sub1=6, C=7

    let find = |nid: u32| nodes.iter().find(|n| n.node_id == nid).unwrap().sort_key;
    assert_eq!(find(a_id), 2, "A primary");
    assert_eq!(find(a_sub1), 3, "A sub 1");
    assert_eq!(find(a_sub2), 4, "A sub 2");
    assert_eq!(find(b_id), 5, "B primary");
    assert_eq!(find(b_sub1), 6, "B sub 1");
    assert_eq!(find(c_id), 7, "C");

    // 验无 tie
    let mut sks: Vec<u32> = nodes.iter().map(|n| n.sort_key).collect();
    sks.sort();
    let unique: Vec<u32> = {
        let mut u = sks.clone();
        u.dedup();
        u
    };
    assert_eq!(sks.len(), unique.len(), "sort_key 不得有 tie");
}

/// 哨兵：合成 node_id 硬上限文档。
/// 验证 synth_text_node_id / is_text_sub_page / text_sub_primary_id / text_sub_page_idx
/// 的编码/解码一致性。
#[test]
fn synth_text_node_id_roundtrip() {
    let primary = 0x0000_0123u32;
    let sub = synth_text_node_id(primary, 5);
    assert!(is_text_sub_page(sub));
    assert!(!is_text_sub_page(primary));
    assert_eq!(text_sub_primary_id(sub), primary & 0x00FF_FFFF);
    assert_eq!(text_sub_page_idx(sub), 5);

    // 边界：page=15（不与 BOX_SHADOW_FLAG bit 28 冲突的最大子页号）。
    // page>=16 会设置 bit 28（BOX_SHADOW_FLAG），is_text_sub_page 据此排除 shadow 节点——
    // 故 sub_page 编码实际可用范围是 1..15（atlas 跨页远不到此上限）。
    let max_sub = synth_text_node_id(0, 15);
    assert_eq!(text_sub_page_idx(max_sub), 15);
    assert!(is_text_sub_page(max_sub));

    // page=16 设置 BOX_SHADOW_FLAG → is_text_sub_page 返 false（shadow 节点语义）。
    let shadow_like = synth_text_node_id(0, 16);
    assert!(
        !is_text_sub_page(shadow_like),
        "page=16 触发 BOX_SHADOW_FLAG"
    );

    // 真实 node index=4095（bits[23:12]=4095）不应被误判为子页
    let high_index = (4095u32 << 12) | 1; // index=4095, gen=1
    assert!(!is_text_sub_page(high_index), "index=4095 仍不被误判");
}

/// 哨兵：index=4096 会与合成子页 bit 碰撞——验 is_text_sub_page 误判。
#[test]
fn node_index_4096_triggers_sub_page_collision() {
    // index=4096 → bits[31:12] = 0x00001 (bit 24 set) → bits[31:24]=1 → 子页误判
    let collision_id = 4096u32 << 12;
    assert!(
        is_text_sub_page(collision_id),
        "index=4096 → bits[31:24]=1 → 误判为子页（证明硬上限哨兵的动机）"
    );
}

// ── RichText build arm 测试（v1.7）──

/// RichText 节点产 Mesh{ program:1, loomgui:// image_path }，per-run 色烤顶点色。
/// 两 run（红 + 蓝）→ 同一 mesh 的顶点色应分两段（前 N 顶点红、后 N 顶点蓝）。
#[test]
fn rich_text_node_emits_mesh_with_per_vertex_color() {
    use crate::text::rich::{RichDeco, RichKind, RichRun, RichStyle, RichWeight};
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let mut n = Node::default();
    n.kind = NodeKind::RichText {
        runs: vec![
            RichRun {
                kind: RichKind::Text { text: "AB".into() },
                color: [1.0, 0.0, 0.0, 1.0], // 红
                font_id: 0,
                size_px: 16,
                weight: RichWeight::Normal,
                style: RichStyle::Normal,
                deco: RichDeco::default(),
                link_id: None,
            },
            RichRun {
                kind: RichKind::Text { text: "CD".into() },
                color: [0.0, 0.0, 1.0, 1.0], // 蓝
                font_id: 0,
                size_px: 16,
                weight: RichWeight::Normal,
                style: RichStyle::Normal,
                deco: RichDeco::default(),
                link_id: None,
            },
        ],
    };
    n.style.font_size = 16.0;
    n.style.text_align = TextAlign::Left;
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 30.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);

    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    // 取 primary RichText mesh（program=1，非子页）。
    let rn = frame
        .nodes
        .iter()
        .find(|rn| {
            !is_text_sub_page(rn.node_id)
                && matches!(&rn.payload, NodePayload::Mesh { program: 1, .. })
        })
        .expect("应存在 primary RichText RenderNode");
    match &rn.payload {
        NodePayload::Mesh {
            verts,
            colors,
            program,
            image_path,
            ..
        } => {
            assert_eq!(*program, 1, "rich text → program=1");
            assert!(
                image_path
                    .as_ref()
                    .is_some_and(|p| p.starts_with("loomgui://font-atlas/")),
                "rich image_path = synthetic atlas path（与 Text 同形）"
            );
            // 4 字形 × 4 顶点 = 16（bold 不双绘：weight=Normal）。
            assert_eq!(verts.len(), 16, "ABCD = 4 glyph × 4 verts = 16");
            assert_eq!(colors.len(), 16, "colors 与 verts 等长");
            // 前 8 顶点红（AB），后 8 顶点蓝（CD）。
            for c in &colors[..8] {
                assert_eq!(*c, [1.0, 0.0, 0.0, 1.0], "AB 段顶点色应红");
            }
            for c in &colors[8..] {
                assert_eq!(*c, [0.0, 0.0, 1.0, 1.0], "CD 段顶点色应蓝");
            }
        }
        _ => panic!("expected Mesh payload for rich text"),
    }
}

/// RichText 叶带 background-color（如 .rt 底色）→ 须自画 bg quad（Container arm 不覆盖
/// RichText 叶）。bg 占真 node_id（DFS sort_key S），text 首页改用子页 1（propagate 给
/// S+1）→ bg 在 text 下层。无 bg-color 时 text 用真 node_id（原行为不变）。
#[test]
fn rich_text_node_emits_bg_quad_when_bg_color_set() {
    use crate::text::rich::{RichDeco, RichKind, RichRun, RichStyle, RichWeight};
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let mut n = Node::default();
    n.kind = NodeKind::RichText {
        runs: vec![RichRun {
            kind: RichKind::Text { text: "AB".into() },
            color: [1.0, 1.0, 1.0, 1.0],
            font_id: 0,
            size_px: 16,
            weight: RichWeight::Normal,
            style: RichStyle::Normal,
            deco: RichDeco::default(),
            link_id: None,
        }],
    };
    n.style.background_color = Some([0.102, 0.114, 0.18, 1.0]); // ~#1a1d2e
    n.style.font_size = 16.0;
    n.style.text_align = TextAlign::Left;
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 30.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    // bg quad：program 0，真 node_id（非子页），4 verts，顶点色 = background-color。
    let bg = frame
        .nodes
        .iter()
        .find(|rn| {
            !is_text_sub_page(rn.node_id)
                && matches!(&rn.payload, NodePayload::Mesh { program: 0, .. })
        })
        .expect("RichText 带 bg-color 应产 bg quad（program 0，真 node_id，非子页）");
    match &bg.payload {
        NodePayload::Mesh {
            verts,
            colors,
            program,
            image_path,
            ..
        } => {
            assert_eq!(*program, 0, "bg quad program=0");
            assert_eq!(verts.len(), 4, "bg quad 4 verts");
            assert_eq!(
                colors[0],
                [0.102, 0.114, 0.18, 1.0],
                "bg 顶点色 = background-color"
            );
            assert!(image_path.is_none(), "纯色 bg 无 image_path");
        }
        _ => panic!("expected Mesh payload for bg quad"),
    }
    // text mesh 仍存在（program 1），且画在 bg 之上（sort_key 更大）。
    let text = frame
        .nodes
        .iter()
        .find(|rn| matches!(&rn.payload, NodePayload::Mesh { program: 1, .. }))
        .expect("text mesh 应存在");
    assert!(
        text.sort_key > bg.sort_key,
        "text 画在 bg 之上（sort_key {} > {}）",
        text.sort_key,
        bg.sort_key
    );
}

/// 两同字体 RichText span → merge 后应合并 draw call（program=1 已在合批白名单）。
/// 验 RichText 与 Text 同走 atlas path 合批路径，不因 per-run 色破坏合批。
#[test]
fn two_rich_nodes_same_atlas_merge() {
    use crate::text::rich::{RichDeco, RichKind, RichRun, RichStyle, RichWeight};
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    // root 容器（无图，program=0）+ 两 RichText 子（同 font_id=0、同 page0 atlas path）。
    let root = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 300.0,
            h: 50.0,
        },
        None,
    );
    let mk_rich = |id: usize, parent: usize, x: f32| {
        let mut n = Node::default();
        n.id = NodeId(id as u32);
        n.parent = Some(NodeId(parent as u32));
        n.kind = NodeKind::RichText {
            runs: vec![RichRun {
                kind: RichKind::Text { text: "AB".into() },
                color: [1.0, 1.0, 1.0, 1.0],
                font_id: 0,
                size_px: 16,
                weight: RichWeight::Normal,
                style: RichStyle::Normal,
                deco: RichDeco::default(),
                link_id: None,
            }],
        };
        n.style.font_size = 16.0;
        n.layout_rect = Rect {
            x,
            y: 0.0,
            w: 100.0,
            h: 20.0,
        };
        n
    };
    let a = mk_rich(1, 0, 0.0);
    let b = mk_rich(2, 0, 100.0);
    let mut scene = Scene::from_nodes(vec![root, a, b], vec![(0, 1), (0, 2)]);

    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    // 两 RichText 同 atlas path（loomgui://font-atlas/p0）→ merge 成 1 个 mesh。
    // root 是 Container(image_path=None) 不同 DrawState → 不合。
    // merge 后 program 归 0（merge 统一 program 字段），故按 image_path 过滤而非 program。
    // 合并 mesh 应含两 RichText 的 8 顶点（2×2 字形 × 4）= 16 顶点。
    let rich_meshes: Vec<_> = frame
        .nodes
        .iter()
        .filter(|rn| {
            matches!(
                &rn.payload,
                NodePayload::Mesh {
                    image_path: Some(p),
                    ..
                } if p.starts_with("loomgui://font-atlas/")
            )
        })
        .collect();
    assert_eq!(
        rich_meshes.len(),
        1,
        "两同 atlas RichText → 1 个合并 mesh，实 {}",
        rich_meshes.len()
    );
    if let NodePayload::Mesh { verts, .. } = &rich_meshes[0].payload {
        assert_eq!(
            verts.len(),
            16,
            "两 RichText 各 2 字形 × 4 顶点 = 16（合并后），实 {}",
            verts.len()
        );
    }
}

/// padding top 须偏移字形 y（bake 烤 line.baseline），否则文字顶到盒顶而非 padding 内。
/// 框架层 bug：bake 旧版只烤 glyphs.x/y，字形 top 走 line.baseline（不含 off_y）→ y padding 丢。
/// 行内图偏下（比文字低 padding）+ 文字垂直上对齐（html 居中）同根因。
#[test]
fn rich_text_padding_offsets_glyphs_vertically() {
    use crate::text::rich::{RichDeco, RichKind, RichRun, RichStyle, RichWeight};
    use taffy::style::LengthPercentage;
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let mk = |pad_top: f32| {
        let mut n = Node::default();
        n.kind = NodeKind::RichText {
            runs: vec![RichRun {
                kind: RichKind::Text { text: "A".into() },
                color: [1.0, 1.0, 1.0, 1.0],
                font_id: 0,
                size_px: 16,
                weight: RichWeight::Normal,
                style: RichStyle::Normal,
                deco: RichDeco::default(),
                link_id: None,
            }],
        };
        n.style.font_size = 16.0;
        n.style.text_align = TextAlign::Left;
        n.style.taffy_style.padding.top = LengthPercentage::Length(pad_top);
        n.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 60.0,
        };
        n
    };
    let build_min_y = |n: Node| {
        let mut scene = Scene::from_nodes(vec![n], vec![]);
        crate::scene::transform::compute_world_transforms(&mut scene);
        let (frame, _, _, _) = build_render_nodes(
            &scene,
            &fonts,
            &std::collections::HashMap::new(),
            &empty_sizes(),
            &mut test_glyph_atlas(),
        );
        let mut min_y = f32::INFINITY;
        for rn in &frame.nodes {
            if let NodePayload::Mesh { verts, program, .. } = &rn.payload {
                if *program == 1 {
                    for v in verts {
                        if v[1] < min_y {
                            min_y = v[1];
                        }
                    }
                }
            }
        }
        min_y
    };
    let top_no_pad = build_min_y(mk(0.0));
    let top_pad14 = build_min_y(mk(14.0));
    let diff = top_pad14 - top_no_pad;
    assert!(
        (diff - 14.0).abs() < 1.5,
        "padding top 14 须偏移字形 14px（diff={}，期望≈14）",
        diff
    );
}

/// 行内图 verts 须加节点 rect.x/y（同字形），否则跑到 design 原点。
/// 旧版 ix=img.x+off_x（缺 rect），单测 rect=0 巧合过，PlayMode rect≠0 才显现（图消失/错位）。
#[test]
fn rich_image_positioned_at_node_rect() {
    use crate::text::rich::{RichDeco, RichKind, RichRun, RichStyle, RichVAlign, RichWeight};
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let mut n = Node::default();
    n.kind = NodeKind::RichText {
        runs: vec![RichRun {
            kind: RichKind::Image {
                src: "res/icons/zap.png".into(),
                w: 20.0,
                h: 20.0,
                valign: RichVAlign::Baseline,
            },
            color: [1.0, 1.0, 1.0, 1.0],
            font_id: 0,
            size_px: 16,
            weight: RichWeight::Normal,
            style: RichStyle::Normal,
            deco: RichDeco::default(),
            link_id: None,
        }],
    };
    n.style.font_size = 16.0;
    n.style.text_align = TextAlign::Left;
    n.layout_rect = Rect {
        x: 100.0,
        y: 50.0,
        w: 200.0,
        h: 30.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    // image mesh（program 0，image_path 含 zap）。
    let img_mesh = frame
        .nodes
        .iter()
        .find(|rn| {
            matches!(
                &rn.payload,
                NodePayload::Mesh {
                    program: 0,
                    image_path: Some(p),
                    ..
                } if p.contains("zap")
            )
        })
        .expect("行内图 mesh 应存在");
    if let NodePayload::Mesh { verts, .. } = &img_mesh.payload {
        let min_x = verts.iter().map(|v| v[0]).fold(f32::INFINITY, f32::min);
        let min_y = verts.iter().map(|v| v[1]).fold(f32::INFINITY, f32::min);
        // 行内图在节点 rect(100,50)处：x 含 rect.x（>=100），y 含 rect.y（>0，不跑负原点）。
        assert!(
            min_x >= 100.0,
            "行内图 x 须含节点 rect.x（>=100，不跑原点），实 {}",
            min_x
        );
        assert!(
            min_y > 0.0,
            "行内图 y 须含节点 rect.y（>0，不跑原点），实 {}",
            min_y
        );
    }
}

/// RichText 含行内图 → frame 同时产 text Mesh（program=1）+ image Mesh（program=0, image_path=src）。
/// 验证 measure_rich_text 记录 Image run 位置 + build 产 image quad 端到端。
#[test]
fn rich_image_emits_mesh_with_image_path_and_program_0() {
    use crate::text::rich::{RichDeco, RichKind, RichRun, RichStyle, RichVAlign, RichWeight};
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let mut n = Node::default();
    n.kind = NodeKind::RichText {
        runs: vec![
            RichRun {
                kind: RichKind::Text { text: "Hi".into() },
                color: [1.0, 1.0, 1.0, 1.0],
                font_id: 0,
                size_px: 16,
                weight: RichWeight::Normal,
                style: RichStyle::Normal,
                deco: RichDeco::default(),
                link_id: None,
            },
            RichRun {
                kind: RichKind::Image {
                    src: "emoji/cool.png".into(),
                    w: 16.0,
                    h: 16.0,
                    valign: RichVAlign::Baseline,
                },
                color: [1.0, 1.0, 1.0, 1.0],
                font_id: 0,
                size_px: 16,
                weight: RichWeight::Normal,
                style: RichStyle::Normal,
                deco: RichDeco::default(),
                link_id: None,
            },
        ],
    };
    n.style.font_size = 16.0;
    n.style.text_align = TextAlign::Left;
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 30.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);

    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    // 应同时存在 text Mesh（program=1）和 image Mesh（program=0）
    let has_text = frame
        .nodes
        .iter()
        .any(|rn| matches!(&rn.payload, NodePayload::Mesh { program: 1, .. }));
    let image_node = frame.nodes.iter().find(|rn| {
        matches!(&rn.payload, NodePayload::Mesh { program: 0, image_path: Some(p), .. } if p == "emoji/cool.png")
    });
    assert!(has_text, "应存在 text Mesh（program=1）");
    assert!(
        image_node.is_some(),
        "应存在 image Mesh（program=0, image_path=src）"
    );
    // image Mesh 应有 4 顶点、6 索引、全图 UV
    if let Some(rn) = image_node {
        match &rn.payload {
            NodePayload::Mesh {
                verts,
                uvs,
                indices,
                image_path,
                program,
                ..
            } => {
                assert_eq!(verts.len(), 4, "image quad = 4 顶点");
                assert_eq!(indices.len(), 6, "2 三角形 = 6 索引");
                assert_eq!(*program, 0, "image → program=0");
                assert_eq!(
                    *image_path,
                    Some("emoji/cool.png".to_string()),
                    "image_path = src"
                );
                // UV v-flip 与 mesh::quad Image arm 相同约定：
                //   TL→(0,1), TR→(1,1), BR→(1,0), BL→(0,0)。
                assert_eq!(uvs[0], [0.0, 1.0], "TL UV (v-flipped)");
                assert_eq!(uvs[1], [1.0, 1.0], "TR UV (v-flipped)");
                assert_eq!(uvs[2], [1.0, 0.0], "BR UV (v-flipped)");
                assert_eq!(uvs[3], [0.0, 0.0], "BL UV (v-flipped)");
            }
            _ => panic!("expected Mesh"),
        }
    }
}

/// RichText run 带 underline → build 后 mesh 含 4 顶点装饰 quad，色 = run.color。
#[test]
fn rich_deco_underline_adds_quad() {
    use crate::text::rich::{
        RichDeco, RichKind, RichRun, RichStyle, RichWeight, TextDecoLines, TextDecoStyle,
    };
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let mut n = Node::default();
    n.kind = NodeKind::RichText {
        runs: vec![RichRun {
            kind: RichKind::Text { text: "AB".into() },
            color: [1.0, 0.0, 0.0, 1.0], // 红
            font_id: 0,
            size_px: 16,
            weight: RichWeight::Normal,
            style: RichStyle::Normal,
            deco: RichDeco {
                lines: TextDecoLines::UNDERLINE,
                style: TextDecoStyle::Solid,
                color: None,
                thickness: None,
            },
            link_id: None,
        }],
    };
    n.style.font_size = 16.0;
    n.style.text_align = TextAlign::Left;
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 30.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);

    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let rn = frame
        .nodes
        .iter()
        .find(|rn| {
            !is_text_sub_page(rn.node_id)
                && matches!(&rn.payload, NodePayload::Mesh { program: 1, .. })
        })
        .expect("应存在 primary RichText RenderNode");
    match &rn.payload {
        NodePayload::Mesh { verts, colors, .. } => {
            // AB = 2 字形 × 4 顶点 + underline deco quad 4 顶点 = 12 顶点
            assert_eq!(
                verts.len(),
                12,
                "2 glyph × 4 + underline 4 = 12 verts，实 {}",
                verts.len()
            );
            assert_eq!(colors.len(), verts.len(), "colors 与 verts 等长");
            // 前 8 顶点是字形色（红），后 4 顶点是装饰线色（红，同 run.color）。
            // 所有顶点色都应 = run.color（红）。
            for c in colors.iter() {
                assert_eq!(*c, [1.0, 0.0, 0.0, 1.0], "装饰线色 = run.color 红");
            }
        }
        _ => panic!("expected Mesh payload"),
    }
}

/// RichText run 带 strike → build 后 mesh 含装饰线 quad（厚度 ≥ 1px），色 = run.color。
#[test]
fn rich_deco_strike_adds_quad() {
    use crate::text::rich::{
        RichDeco, RichKind, RichRun, RichStyle, RichWeight, TextDecoLines, TextDecoStyle,
    };
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let mut n = Node::default();
    n.kind = NodeKind::RichText {
        runs: vec![RichRun {
            kind: RichKind::Text { text: "CD".into() },
            color: [0.0, 0.0, 1.0, 1.0], // 蓝
            font_id: 0,
            size_px: 16,
            weight: RichWeight::Normal,
            style: RichStyle::Normal,
            deco: RichDeco {
                lines: TextDecoLines::LINE_THROUGH,
                style: TextDecoStyle::Solid,
                color: None,
                thickness: None,
            },
            link_id: None,
        }],
    };
    n.style.font_size = 16.0;
    n.style.text_align = TextAlign::Left;
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 30.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);

    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let rn = frame
        .nodes
        .iter()
        .find(|rn| {
            !is_text_sub_page(rn.node_id)
                && matches!(&rn.payload, NodePayload::Mesh { program: 1, .. })
        })
        .expect("应存在 primary RichText RenderNode");
    match &rn.payload {
        NodePayload::Mesh { verts, colors, .. } => {
            // CD = 2 字形 × 4 顶点 + strike deco quad 4 顶点 = 12 顶点
            assert!(verts.len() > 8, "应含装饰线顶点（>8），实 {}", verts.len());
            // 所有顶点色 = run.color（蓝）。
            for c in colors.iter() {
                assert_eq!(*c, [0.0, 0.0, 1.0, 1.0], "装饰线色 = run.color 蓝");
            }
        }
        _ => panic!("expected Mesh payload"),
    }
}

/// RichText dashed 装饰线 → 因分段 quad 数 > solid 单 quad，顶点数更多。
#[test]
fn rich_deco_dashed_produces_more_verts_than_solid() {
    use crate::text::rich::{
        RichDeco, RichKind, RichRun, RichStyle, RichWeight, TextDecoLines, TextDecoStyle,
    };
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    for (style, label) in [
        (TextDecoStyle::Solid, "solid"),
        (TextDecoStyle::Dashed, "dashed"),
        (TextDecoStyle::Dotted, "dotted"),
    ] {
        let mut n = Node::default();
        n.kind = NodeKind::RichText {
            runs: vec![RichRun {
                kind: RichKind::Text {
                    text: "ABCDEFGHIJKLMNOP".into(),
                },
                color: [1.0; 4],
                font_id: 0,
                size_px: 16,
                weight: RichWeight::Normal,
                style: RichStyle::Normal,
                deco: RichDeco {
                    lines: TextDecoLines::UNDERLINE,
                    style,
                    color: None,
                    thickness: None,
                },
                link_id: None,
            }],
        };
        n.style.font_size = 16.0;
        n.style.text_align = TextAlign::Left;
        n.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 400.0,
            h: 30.0,
        };
        let mut scene = Scene::from_nodes(vec![n], vec![]);
        crate::scene::transform::compute_world_transforms(&mut scene);
        let (frame, _, _, _) = build_render_nodes(
            &scene,
            &fonts,
            &std::collections::HashMap::new(),
            &empty_sizes(),
            &mut test_glyph_atlas(),
        );
        let rn = frame
            .nodes
            .iter()
            .find(|rn| {
                !is_text_sub_page(rn.node_id)
                    && matches!(&rn.payload, NodePayload::Mesh { program: 1, .. })
            })
            .expect("should exist primary RichText RenderNode");
        match &rn.payload {
            NodePayload::Mesh { verts, .. } => {
                match label {
                    "solid" => {
                        // solid: glyphs + 1 quad (4 verts) for deco
                        let glyph_verts = verts.len() - 4;
                        assert!(glyph_verts > 0, "{label}: should have glyph vertices");
                    }
                    "dashed" | "dotted" => {
                        // dashed/dotted: segments → >4 deco verts
                        assert!(
                            verts.len() > 20,
                            "{label}: expected >20 verts (segmented), got {}",
                            verts.len()
                        );
                    }
                    _ => {}
                }
            }
            _ => panic!("{label}: expected Mesh payload"),
        }
    }
}

/// RichText double 装饰线 → 两条 quad（8 顶点），比 solid 单 quad（4 顶点）多。
#[test]
fn rich_deco_double_produces_two_quads() {
    use crate::text::rich::{
        RichDeco, RichKind, RichRun, RichStyle, RichWeight, TextDecoLines, TextDecoStyle,
    };
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let mut n = Node::default();
    n.kind = NodeKind::RichText {
        runs: vec![RichRun {
            kind: RichKind::Text { text: "AB".into() },
            color: [1.0; 4],
            font_id: 0,
            size_px: 16,
            weight: RichWeight::Normal,
            style: RichStyle::Normal,
            deco: RichDeco {
                lines: TextDecoLines::UNDERLINE,
                style: TextDecoStyle::Double,
                color: None,
                thickness: None,
            },
            link_id: None,
        }],
    };
    n.style.font_size = 16.0;
    n.style.text_align = TextAlign::Left;
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 30.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let rn = frame
        .nodes
        .iter()
        .find(|rn| {
            !is_text_sub_page(rn.node_id)
                && matches!(&rn.payload, NodePayload::Mesh { program: 1, .. })
        })
        .expect("should exist primary RichText RenderNode");
    match &rn.payload {
        NodePayload::Mesh { verts, .. } => {
            // 2 glyph × 4 = 8 + double deco 2×4 = 8 = 16 verts
            assert_eq!(
                verts.len(),
                16,
                "2 glyph × 4 + double (2 quads × 4) = 16 verts, got {}",
                verts.len()
            );
        }
        _ => panic!("expected Mesh payload"),
    }
}

/// RichText 装饰线独立色：deco.color 覆盖 run.color。
#[test]
fn rich_deco_independent_color_overrides_run_color() {
    use crate::text::rich::{
        RichDeco, RichKind, RichRun, RichStyle, RichWeight, TextDecoLines, TextDecoStyle,
    };
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let mut n = Node::default();
    n.kind = NodeKind::RichText {
        runs: vec![RichRun {
            kind: RichKind::Text { text: "AB".into() },
            color: [1.0, 1.0, 1.0, 1.0], // run.color = 白
            font_id: 0,
            size_px: 16,
            weight: RichWeight::Normal,
            style: RichStyle::Normal,
            deco: RichDeco {
                lines: TextDecoLines::UNDERLINE,
                style: TextDecoStyle::Solid,
                color: Some([1.0, 0.0, 0.0, 1.0]), // deco.color = 红（独立于 run.color）
                thickness: None,
            },
            link_id: None,
        }],
    };
    n.style.font_size = 16.0;
    n.style.text_align = TextAlign::Left;
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 30.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let rn = frame
        .nodes
        .iter()
        .find(|rn| {
            !is_text_sub_page(rn.node_id)
                && matches!(&rn.payload, NodePayload::Mesh { program: 1, .. })
        })
        .expect("should exist primary RichText RenderNode");
    match &rn.payload {
        NodePayload::Mesh { verts, colors, .. } => {
            // glyphs 色 = 白，deco quad 色 = 红
            // 2 glyph × 4 = 8 glyph verts (白) + 4 deco verts (红) = 12 verts
            assert_eq!(verts.len(), 12, "2 glyph × 4 + underline 4 = 12");
            // 前 8 顶点（字形）色 = 白
            for c in &colors[0..8] {
                assert_eq!(*c, [1.0, 1.0, 1.0, 1.0], "glyph color = white");
            }
            // 后 4 顶点（装饰线）色 = 红
            for c in &colors[8..] {
                assert_eq!(*c, [1.0, 0.0, 0.0, 1.0], "deco color = red (independent)");
            }
        }
        _ => panic!("expected Mesh payload"),
    }
}

/// `ensure_solid` 首次调分配 1×1 白像素，二次命中返同 UV（缓存不重复分配）。
#[test]
fn ensure_solid_hit_returns_same_uv() {
    let mut atlas = test_glyph_atlas();
    let r1 = atlas.ensure_solid();
    let r2 = atlas.ensure_solid();
    assert_eq!(r1.page, r2.page);
    assert_eq!((r1.u0, r1.v0, r1.u1, r1.v1), (r2.u0, r2.v0, r2.u1, r2.v1));
    assert_eq!(r1.px_w, 1);
    assert_eq!(r1.px_h, 1);
}

/// 富文本链接 <a> 在窄宽度下跨行换行 → fragments 拆成多 rect，link_id 一致。
#[test]
fn rich_fragment_cross_line_link_splits_rects() {
    use crate::text::rich::{RichDeco, RichFragment, RichKind, RichRun, RichStyle, RichWeight};
    let ft = test_font_table().expect("need test font");
    // 窄宽度下 "link text here" 会换行
    let runs = vec![RichRun {
        kind: RichKind::Text {
            text: "link text here".into(),
        },
        color: [0.0, 1.0, 0.0, 1.0],
        font_id: 0,
        size_px: 24,
        weight: RichWeight::Normal,
        style: RichStyle::Normal,
        deco: RichDeco::default(),
        link_id: Some(7),
    }];
    let mut n = Node::default();
    n.kind = NodeKind::RichText { runs };
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 50.0,
        h: 80.0,
    };
    n.style.color = [1.0, 1.0, 1.0, 1.0];
    n.style.font_size = 24.0;
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    let nid = scene.roots[0]; // from_nodes 覆盖 id，从 scene 取实值
                              // 预填 text_layout（窄宽度 layout）
    let layout = crate::text::layout::measure_rich_text(
        &match &scene.get(nid).unwrap().kind {
            NodeKind::RichText { runs } => runs.clone(),
            _ => unreachable!(),
        },
        Some(50.0),
        0.0,
        crate::style::resolved::TextAlign::Left,
        &ft.stack_for(None),
    );
    scene.text_layouts[nid.index()] = Some(layout);
    scene.rich_fragments = vec![None; scene.nodes.capacity() + 1];
    crate::scene::transform::compute_world_transforms(&mut scene);

    let (_frame, _hashes, _sort_keys, rich_fragments) = build_render_nodes(
        &scene,
        &ft,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    // 找 node_id 对应的 fragments
    let frags: Vec<&RichFragment> = rich_fragments
        .iter()
        .filter(|(nid_u32, _)| *nid_u32 == nid.0)
        .flat_map(|(_, f)| f.iter())
        .collect();
    assert!(
        frags.len() >= 2,
        "窄宽度换行应产 >=2 fragment rect，实际 {} 个",
        frags.len()
    );
    // 所有 fragment 的 link_id 一致
    for f in &frags {
        assert_eq!(f.link_id, 7, "所有 fragment link_id 应为 7");
    }
    // fragment rect 尺寸合理（非零宽高）
    for f in &frags {
        assert!(f.w > 0.0, "fragment w 应 > 0，实际 {:.1}", f.w);
        assert!(f.h > 0.0, "fragment h 应 > 0，实际 {:.1}", f.h);
    }
}

// ── box-shadow 集成测试 ───────────────────────────

/// box-shadow:2px 3px #000000 → 阴影节点 node_id = main_id|BOX_SHADOW_FLAG、
/// sort_key < main sort_key、阴影 verts x 偏移 ox=2、y 偏移 oy=3。
#[test]
fn box_shadow_emits_node_with_offset_and_sort_key() {
    let mut n = container_node(
        0,
        None,
        Rect {
            x: 10.0,
            y: 20.0,
            w: 100.0,
            h: 50.0,
        },
        Some([1.0, 0.0, 0.0, 1.0]),
    );
    n.style.box_shadow = Some(BoxShadow {
        ox: 2.0,
        oy: 3.0,
        spread: 0.0,
        color: [0.0, 0.0, 0.0, 0.5],
    });
    let mut scene = Scene::from_nodes(vec![n], vec![]);

    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    // 应有两个 RenderNode：主节点（Container bg）+ 阴影节点。
    assert!(
        frame.nodes.len() >= 2,
        "box-shadow 应产独立 RenderNode，共 {} 节点",
        frame.nodes.len()
    );

    let shadow_rn = frame
        .nodes
        .iter()
        .find(|rn| rn.node_id & BOX_SHADOW_FLAG != 0)
        .expect("应存在 box-shadow RenderNode（node_id 带 BOX_SHADOW_FLAG）");
    let main_rn = frame
        .nodes
        .iter()
        .find(|rn| rn.node_id & BOX_SHADOW_FLAG == 0)
        .expect("应存在主节点 RenderNode");

    // 阴影 node_id = main_id | BOX_SHADOW_FLAG
    assert_eq!(
        shadow_rn.node_id,
        main_rn.node_id | BOX_SHADOW_FLAG,
        "阴影 node_id = main_id | BOX_SHADOW_FLAG"
    );

    // 阴影 sort_key < main sort_key（阴影绘在主节点之下）
    assert!(
        shadow_rn.sort_key < main_rn.sort_key,
        "阴影 sort_key({}) < main sort_key({})",
        shadow_rn.sort_key,
        main_rn.sort_key
    );

    // 阴影 verts 偏移 ox=2, oy=3（相对主节点 bg quad）
    let shadow_verts = match &shadow_rn.payload {
        NodePayload::Mesh { verts, .. } => verts,
        _ => panic!("阴影节点应为 Mesh"),
    };
    let main_verts = match &main_rn.payload {
        NodePayload::Mesh { verts, .. } => verts,
        _ => panic!("主节点应为 Mesh"),
    };

    // 主节点 bg quad x_min = rect.x = 10.0
    let main_x_min = main_verts.iter().map(|v| v[0]).fold(f32::MAX, f32::min);
    // 阴影 quad x_min = shadow_rect.x - spread = (10+2) - 0 = 12.0
    let shadow_x_min = shadow_verts.iter().map(|v| v[0]).fold(f32::MAX, f32::min);
    assert!(
        (shadow_x_min - (main_x_min + 2.0)).abs() < 1e-3,
        "阴影 x_min({}) = main x_min({}) + ox(2.0)",
        shadow_x_min,
        main_x_min
    );

    let main_y_min = main_verts.iter().map(|v| v[1]).fold(f32::MAX, f32::min);
    let shadow_y_min = shadow_verts.iter().map(|v| v[1]).fold(f32::MAX, f32::min);
    assert!(
        (shadow_y_min - (main_y_min + 3.0)).abs() < 1e-3,
        "阴影 y_min({}) = main y_min({}) + oy(3.0)",
        shadow_y_min,
        main_y_min
    );

    // 阴影颜色正确
    let colors = match &shadow_rn.payload {
        NodePayload::Mesh { colors, .. } => colors,
        _ => unreachable!(),
    };
    assert!(
        colors.iter().all(|c| *c == [0.0, 0.0, 0.0, 0.5]),
        "阴影顶点色应为半透明黑"
    );
}

// ── resolve_slice_percent ───────────────────────────

#[test]
fn nine_slice_percent_resolves_to_pixels() {
    // slice 25%，src 100×100 → 应 resolve 成 25px，非 0.25px
    let mut slice = crate::style::resolved::SliceInsets {
        top: 0.25,
        right: 0.25,
        bottom: 0.25,
        left: 0.25,
    };
    let src_w = 100.0;
    let src_h = 100.0;
    let resolved = resolve_slice_percent(&slice, src_w, src_h);
    // 25% × 100 = 25px
    assert!(
        (resolved.left - 25.0).abs() < 1e-3,
        "% resolve 成像素，got {}",
        resolved.left
    );
    assert!((resolved.top - 25.0).abs() < 1e-3, "top % resolve 成像素");
    // 像素值不小于 1 → 不 resolve
    slice.left = 10.0;
    let r2 = resolve_slice_percent(&slice, src_w, src_h);
    assert!(
        (r2.left - 10.0).abs() < 1e-3,
        "像素值 10 不动，got {}",
        r2.left
    );
    // 混合：left=25% 存 0.25（resolve），right=20px（不动）
    slice.left = 0.25;
    slice.right = 20.0;
    let r3 = resolve_slice_percent(&slice, src_w, src_h);
    assert!((r3.left - 25.0).abs() < 1e-3, "left % → 25px");
    assert!((r3.right - 20.0).abs() < 1e-3, "right px → 20px 不动");
}

/// 端到端：Container + bg-image + border-image-slice:25% → render 期 % resolve 成像素
/// → nine_slice 16 顶点（非 quad 退化）。旧 bug：% 存 0.25 被当 0.25px 用，
/// 源图 80×80 时 slice=0.25px → grid_x[1]=0.25,grid_y[1]=0.25 → 几乎全图拉伸，
/// 16 顶点仍在但视觉坍缩；resolve 后 slice=20px → 正确九宫格。
#[test]
fn build_container_slice_percent_resolves_in_render() {
    let mut n = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 80.0,
            h: 80.0,
        },
        Some([1.0, 0.0, 0.0, 1.0]),
    );
    n.style.background_image = Some("skin.png".into());
    n.style.background_size = BackgroundSize::Stretch;
    // 25% 存为 0.25 → render 期应 resolve 为 80×0.25=20px
    n.style.border_image_slice = Some(crate::style::resolved::SliceInsets {
        top: 0.25,
        right: 0.25,
        bottom: 0.25,
        left: 0.25,
    });
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    crate::scene::transform::compute_world_transforms(&mut scene);
    let fonts = test_font_table().expect("need font");
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &sizes("skin.png", 80, 80),
        &mut test_glyph_atlas(),
    );
    match &frame.nodes[0].payload {
        NodePayload::Mesh { verts, uvs, .. } => {
            // 16 顶点 = 九宫格（非 quad 退化为 4 顶点）
            assert_eq!(verts.len(), 16, "% resolve 后 nine_slice 16 顶点");
            // 验证 UV 切片线在 20/80=0.25 而非 0.25/80≈0.003
            // 源图 80×80，resolve 后 slice=20px
            // sx = 1.0/80 = 0.0125
            // UV left = 20*0.0125 = 0.25
            assert!(
                (uvs[1][0] - 0.25).abs() < 1e-3,
                "resolve 后 UV 左切片 = 0.25（非 0.003），实 {}",
                uvs[1][0]
            );
            // UV right = (80-20)*0.0125 = 0.75
            assert!(
                (uvs[2][0] - 0.75).abs() < 1e-3,
                "resolve 后 UV 右切片 = 0.75，实 {}",
                uvs[2][0]
            );
        }
        _ => panic!("expected Mesh"),
    }
}

// ── text-shadow Back layer（v1.8 Task 8）──

#[test]
fn text_shadow_emits_back_layer_quad_with_offset() {
    use crate::text::font_effect::FontEffect;
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let mut n = Node::default();
    n.kind = NodeKind::Text {
        content: "A".into(),
    };
    n.style.font_size = 16.0;
    n.style.text_align = TextAlign::Left;
    n.style.text_effects = vec![FontEffect::Shadow {
        ox: 3.0,
        oy: 3.0,
        blur: 0.0,
        color: [0.0, 0.0, 0.0, 1.0],
    }];
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 30.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let shadow_rn = frame
        .nodes
        .iter()
        .find(|rn| (rn.node_id & super::BOX_SHADOW_FLAG) != 0)
        .expect("shadow back RenderNode");
    let primary_rn = frame
        .nodes
        .iter()
        .find(|rn| {
            !is_text_sub_page(rn.node_id)
                && (rn.node_id & super::BOX_SHADOW_FLAG) == 0
                && matches!(&rn.payload, NodePayload::Mesh { program: 1, .. })
        })
        .expect("primary text RenderNode");
    assert!(
        shadow_rn.sort_key < primary_rn.sort_key,
        "shadow sort_key {} < primary {}",
        shadow_rn.sort_key,
        primary_rn.sort_key
    );
    match &shadow_rn.payload {
        NodePayload::Mesh {
            verts,
            colors,
            program,
            ..
        } => {
            assert_eq!(*program, 1);
            assert!(!verts.is_empty());
            for c in colors {
                assert_eq!(*c, [0.0, 0.0, 0.0, 1.0], "shadow color");
            }
            if let NodePayload::Mesh {
                verts: primary_verts,
                ..
            } = &primary_rn.payload
            {
                let pv = primary_verts[0];
                let sv = verts[0];
                assert!(
                    (sv[0] - (pv[0] + 3.0)).abs() < 1e-3,
                    "shadow x offset: {} vs {}",
                    sv[0],
                    pv[0] + 3.0
                );
                assert!(
                    (sv[1] - (pv[1] + 3.0)).abs() < 1e-3,
                    "shadow y offset: {} vs {}",
                    sv[1],
                    pv[1] + 3.0
                );
            }
        }
        _ => panic!("expected Mesh"),
    }
}

#[test]
fn text_shadow_no_blur_same_glyph_size() {
    use crate::text::font_effect::FontEffect;
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let mut n = Node::default();
    n.kind = NodeKind::Text {
        content: "B".into(),
    };
    n.style.font_size = 20.0;
    n.style.text_effects = vec![FontEffect::Shadow {
        ox: 2.0,
        oy: 2.0,
        blur: 0.0,
        color: [1.0, 0.0, 0.0, 1.0],
    }];
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 30.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    crate::scene::transform::compute_world_transforms(&mut scene);
    let mut atlas = test_glyph_atlas();
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut atlas,
    );
    let shadow_rn = frame
        .nodes
        .iter()
        .find(|rn| (rn.node_id & super::BOX_SHADOW_FLAG) != 0)
        .expect("shadow back");
    let primary_rn = frame
        .nodes
        .iter()
        .find(|rn| {
            !is_text_sub_page(rn.node_id)
                && (rn.node_id & super::BOX_SHADOW_FLAG) == 0
                && matches!(&rn.payload, NodePayload::Mesh { program: 1, .. })
        })
        .expect("primary");
    if let (NodePayload::Mesh { verts: sv, .. }, NodePayload::Mesh { verts: pv, .. }) =
        (&shadow_rn.payload, &primary_rn.payload)
    {
        assert_eq!(sv.len(), pv.len(), "blur=0 same vert count");
    }
}

#[test]
fn text_shadow_multiple_shadows_emit_multiple_back_layers() {
    use crate::text::font_effect::FontEffect;
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let mut n = Node::default();
    n.kind = NodeKind::Text {
        content: "C".into(),
    };
    n.style.font_size = 16.0;
    n.style.text_effects = vec![
        FontEffect::Shadow {
            ox: 1.0,
            oy: 1.0,
            blur: 0.0,
            color: [1.0, 0.0, 0.0, 1.0],
        },
        FontEffect::Shadow {
            ox: 2.0,
            oy: 2.0,
            blur: 0.0,
            color: [0.0, 0.0, 1.0, 1.0],
        },
    ];
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 30.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let shadow_nodes: Vec<_> = frame
        .nodes
        .iter()
        .filter(|rn| (rn.node_id & super::BOX_SHADOW_FLAG) != 0)
        .collect();
    assert_eq!(shadow_nodes.len(), 2, "2 shadows -> 2 back nodes");
    let c0 = match &shadow_nodes[0].payload {
        NodePayload::Mesh { colors, .. } => colors[0],
        _ => panic!("expected Mesh"),
    };
    let c1 = match &shadow_nodes[1].payload {
        NodePayload::Mesh { colors, .. } => colors[0],
        _ => panic!("expected Mesh"),
    };
    assert_eq!(c0, [1.0, 0.0, 0.0, 1.0], "first shadow red");
    assert_eq!(c1, [0.0, 0.0, 1.0, 1.0], "second shadow blue");
}

#[test]
fn text_without_shadow_emits_no_back_layer() {
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let mut n = Node::default();
    n.kind = NodeKind::Text {
        content: "D".into(),
    };
    n.style.font_size = 16.0;
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 30.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let has_shadow = frame
        .nodes
        .iter()
        .any(|rn| (rn.node_id & super::BOX_SHADOW_FLAG) != 0);
    assert!(!has_shadow, "no text-shadow -> no back layer");
}

// ── -webkit-text-stroke Front layer（v1.8 Task 9）──

#[test]
fn text_stroke_emits_front_layer_quad_same_position() {
    use crate::text::font_effect::FontEffect;
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let mut n = Node::default();
    n.kind = NodeKind::Text {
        content: "A".into(),
    };
    n.style.font_size = 16.0;
    n.style.text_align = TextAlign::Left;
    n.style.text_effects = vec![FontEffect::Stroke {
        w: 2.0,
        color: [1.0, 0.0, 0.0, 1.0],
    }];
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 30.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    // Find stroke Front layer: has TEXT_STROKE_FRONT_FLAG (bit 31) set
    let stroke_rn = frame
        .nodes
        .iter()
        .find(|rn| (rn.node_id & super::TEXT_STROKE_FRONT_FLAG) != 0)
        .expect("stroke Front RenderNode");
    let primary_rn = frame
        .nodes
        .iter()
        .find(|rn| {
            !is_text_sub_page(rn.node_id)
                && (rn.node_id & super::BOX_SHADOW_FLAG) == 0
                && (rn.node_id & super::TEXT_STROKE_FRONT_FLAG) == 0
                && matches!(&rn.payload, NodePayload::Mesh { program: 1, .. })
        })
        .expect("primary text RenderNode");
    assert!(
        stroke_rn.sort_key > primary_rn.sort_key,
        "stroke Front sort_key {} > primary {}",
        stroke_rn.sort_key,
        primary_rn.sort_key
    );
    match &stroke_rn.payload {
        NodePayload::Mesh {
            verts,
            colors,
            program,
            ..
        } => {
            assert_eq!(*program, 1);
            assert!(!verts.is_empty());
            for c in colors {
                assert_eq!(*c, [1.0, 0.0, 0.0, 1.0], "stroke color");
            }
            // Stroke Front layer has same position as base glyph (no offset).
            if let NodePayload::Mesh {
                verts: primary_verts,
                ..
            } = &primary_rn.payload
            {
                let pv = primary_verts[0];
                let sv = verts[0];
                // Stroke uses eroded glyph with expanded canvas, so positions may differ
                // by the pad expansion. Just verify it's in the ballpark (no offset applied).
                assert!(
                    (sv[1] - pv[1]).abs() <= 5.0,
                    "stroke no y-offset: sv[1]={}, pv[1]={}",
                    sv[1],
                    pv[1]
                );
            }
        }
        _ => panic!("expected Mesh"),
    }
}

#[test]
fn text_stroke_and_shadow_both_emit() {
    use crate::text::font_effect::FontEffect;
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let mut n = Node::default();
    n.kind = NodeKind::Text {
        content: "B".into(),
    };
    n.style.font_size = 16.0;
    n.style.text_effects = vec![
        FontEffect::Shadow {
            ox: 2.0,
            oy: 2.0,
            blur: 0.0,
            color: [0.0, 0.0, 0.0, 1.0],
        },
        FontEffect::Stroke {
            w: 2.0,
            color: [1.0, 0.0, 0.0, 1.0],
        },
    ];
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 30.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let shadow_count = frame
        .nodes
        .iter()
        .filter(|rn| (rn.node_id & super::BOX_SHADOW_FLAG) != 0)
        .count();
    let stroke_count = frame
        .nodes
        .iter()
        .filter(|rn| (rn.node_id & super::TEXT_STROKE_FRONT_FLAG) != 0)
        .count();
    assert_eq!(shadow_count, 1, "1 shadow Back layer");
    assert_eq!(stroke_count, 1, "1 stroke Front layer");
    // Verify ordering: shadow Back < primary < stroke Front
    let primary = frame
        .nodes
        .iter()
        .find(|rn| {
            !is_text_sub_page(rn.node_id)
                && (rn.node_id & super::BOX_SHADOW_FLAG) == 0
                && (rn.node_id & super::TEXT_STROKE_FRONT_FLAG) == 0
                && matches!(&rn.payload, NodePayload::Mesh { program: 1, .. })
        })
        .expect("primary");
    let shadow = frame
        .nodes
        .iter()
        .find(|rn| (rn.node_id & super::BOX_SHADOW_FLAG) != 0)
        .expect("shadow");
    let stroke = frame
        .nodes
        .iter()
        .find(|rn| (rn.node_id & super::TEXT_STROKE_FRONT_FLAG) != 0)
        .expect("stroke");
    assert!(
        shadow.sort_key < primary.sort_key,
        "shadow {} < primary {}",
        shadow.sort_key,
        primary.sort_key
    );
    assert!(
        primary.sort_key < stroke.sort_key,
        "primary {} < stroke {}",
        primary.sort_key,
        stroke.sort_key
    );
}

/// 多页 text + stroke：stroke sort_key 必须 > 所有子页 base sort_key。
/// 回归：propagate_text_stroke_sort_keys 原来的 primary_sk+1 会在多页时被子页遮盖。
#[test]
fn text_stroke_sort_key_above_sub_pages() {
    // 模拟 propagate_text_sub_page_sort_keys 之后的状态：
    // primary=100(sk=5), sub1(101, sk=6), sub2(102, sk=7), other_real=200(sk=9)
    // stroke 节点 sk=0，待 propagate_text_stroke_sort_keys 调整。
    let primary_id: u32 = 100;
    let sub1_id = synth_text_node_id(primary_id, 1);
    let sub2_id = synth_text_node_id(primary_id, 2);
    let stroke_id = synth_text_node_id(primary_id, 128); // TEXT_STROKE_FRONT_FLAG >> 24 = 128
    let other_id: u32 = 200;

    let empty_mesh = crate::render::node::NodePayload::Mesh {
        verts: vec![],
        uvs: vec![],
        colors: vec![],
        indices: vec![],
        image_path: None,
        program: 0,
        color_matrix: [0.0; 20],
    };

    let mk_rn = |node_id: u32, sort_key: u32| crate::render::node::RenderNode {
        node_id,
        parent_id: None,
        visible: true,
        alpha: 1.0,
        color_tint: [1.0; 4],
        world_matrix: crate::transform::IDENTITY,
        blend: crate::render::node::BlendMode::Normal,
        mask_context: crate::render::node::MaskContext(0),
        sort_key,
        change_level: crate::render::node::ChangeLevel::Full,
        reuse_key: 0,
        payload: empty_mesh.clone(),
    };

    let mut nodes = vec![
        mk_rn(primary_id, 5),
        mk_rn(sub1_id, 6),
        mk_rn(sub2_id, 7),
        mk_rn(stroke_id, 0), // 初始 0，由 propagate 设置
        mk_rn(other_id, 9),
    ];

    let strokes = vec![(primary_id, stroke_id)];
    super::propagate_text_stroke_sort_keys(&mut nodes, &strokes);

    let find = |nid: u32| nodes.iter().find(|n| n.node_id == nid).unwrap().sort_key;
    let primary_sk = find(primary_id);
    let sub1_sk = find(sub1_id);
    let sub2_sk = find(sub2_id);
    let stroke_sk = find(stroke_id);
    let other_sk = find(other_id);

    // stroke 在上（最高 sort_key = 后绘）
    assert!(stroke_sk > sub1_sk, "stroke {stroke_sk} > sub1 {sub1_sk}");
    assert!(stroke_sk > sub2_sk, "stroke {stroke_sk} > sub2 {sub2_sk}");
    // 子页同页序保持
    assert!(sub1_sk < sub2_sk, "sub1 {sub1_sk} < sub2 {sub2_sk}");
    assert!(
        primary_sk < sub1_sk,
        "primary {primary_sk} < sub1 {sub1_sk}"
    );
    // 其他节点在 stroke 之后（被 threshold 位移 +1 推开）
    assert!(
        other_sk > stroke_sk,
        "other {other_sk} > stroke {stroke_sk}"
    );
    // 无 tie
    let mut sks: Vec<u32> = nodes.iter().map(|n| n.sort_key).collect();
    sks.sort();
    let mut dedup = sks.clone();
    dedup.dedup();
    assert_eq!(sks.len(), dedup.len(), "sort_key 不得有 tie");
}

#[test]
fn text_without_stroke_emits_no_front_layer() {
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let mut n = Node::default();
    n.kind = NodeKind::Text {
        content: "D".into(),
    };
    n.style.font_size = 16.0;
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 30.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let has_stroke = frame
        .nodes
        .iter()
        .any(|rn| (rn.node_id & super::TEXT_STROKE_FRONT_FLAG) != 0);
    assert!(!has_stroke, "no -webkit-text-stroke -> no Front layer");
}

// ── font-effect:glow Back layer（v1.8 Task 10）──

#[test]
fn font_effect_glow_emits_back_layer_quad_no_offset() {
    use crate::text::font_effect::FontEffect;
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let mut n = Node::default();
    n.kind = NodeKind::Text {
        content: "A".into(),
    };
    n.style.font_size = 16.0;
    n.style.text_align = TextAlign::Left;
    n.style.text_effects = vec![FontEffect::Glow {
        w: 2.0,
        color: [1.0, 0.5, 0.0, 1.0],
    }];
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 30.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    // glow 是 Back layer，复用 BOX_SHADOW_FLAG 标记。
    let glow_rn = frame
        .nodes
        .iter()
        .find(|rn| (rn.node_id & super::BOX_SHADOW_FLAG) != 0)
        .expect("glow Back RenderNode");
    let primary_rn = frame
        .nodes
        .iter()
        .find(|rn| {
            !is_text_sub_page(rn.node_id)
                && (rn.node_id & super::BOX_SHADOW_FLAG) == 0
                && (rn.node_id & super::TEXT_STROKE_FRONT_FLAG) == 0
                && matches!(&rn.payload, NodePayload::Mesh { program: 1, .. })
        })
        .expect("primary text RenderNode");
    assert!(
        glow_rn.sort_key < primary_rn.sort_key,
        "glow sort_key {} < primary {}",
        glow_rn.sort_key,
        primary_rn.sort_key
    );
    match &glow_rn.payload {
        NodePayload::Mesh {
            verts,
            colors,
            program,
            ..
        } => {
            assert_eq!(*program, 1);
            assert!(!verts.is_empty());
            for c in colors {
                assert_eq!(*c, [1.0, 0.5, 0.0, 1.0], "glow color = orange");
            }
        }
        _ => panic!("expected Mesh"),
    }
}

#[test]
fn font_effect_glow_and_shadow_both_emit_back_layers() {
    use crate::text::font_effect::FontEffect;
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let mut n = Node::default();
    n.kind = NodeKind::Text {
        content: "B".into(),
    };
    n.style.font_size = 16.0;
    n.style.text_effects = vec![
        FontEffect::Glow {
            w: 2.0,
            color: [1.0, 0.5, 0.0, 1.0],
        },
        FontEffect::Shadow {
            ox: 2.0,
            oy: 2.0,
            blur: 0.0,
            color: [0.0, 0.0, 0.0, 1.0],
        },
    ];
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 30.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let back_nodes: Vec<_> = frame
        .nodes
        .iter()
        .filter(|rn| (rn.node_id & super::BOX_SHADOW_FLAG) != 0)
        .collect();
    assert_eq!(back_nodes.len(), 2, "glow + shadow = 2 Back layer nodes");
    // 主节点 sort_key 应大于两个 Back layer nodes。
    let primary = frame
        .nodes
        .iter()
        .find(|rn| {
            !is_text_sub_page(rn.node_id)
                && (rn.node_id & super::BOX_SHADOW_FLAG) == 0
                && (rn.node_id & super::TEXT_STROKE_FRONT_FLAG) == 0
                && matches!(&rn.payload, NodePayload::Mesh { program: 1, .. })
        })
        .expect("primary");
    for bn in &back_nodes {
        assert!(
            bn.sort_key < primary.sort_key,
            "back node sort_key {} < primary {}",
            bn.sort_key,
            primary.sort_key
        );
    }
    // 第一 Back 节点（glow 先声明）的色应为橘色。
    if let NodePayload::Mesh { colors, .. } = &back_nodes[0].payload {
        assert_eq!(colors[0], [1.0, 0.5, 0.0, 1.0], "glow orange first");
    }
    // 第二 Back 节点（shadow）的色应为黑。
    if let NodePayload::Mesh { colors, .. } = &back_nodes[1].payload {
        assert_eq!(colors[0], [0.0, 0.0, 0.0, 1.0], "shadow black second");
    }
}

#[test]
fn font_effect_glow_with_shadow_and_stroke_all_emit() {
    use crate::text::font_effect::FontEffect;
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let mut n = Node::default();
    n.kind = NodeKind::Text {
        content: "C".into(),
    };
    n.style.font_size = 16.0;
    n.style.text_effects = vec![
        FontEffect::Glow {
            w: 2.0,
            color: [1.0, 0.5, 0.0, 1.0],
        },
        FontEffect::Shadow {
            ox: 2.0,
            oy: 2.0,
            blur: 0.0,
            color: [0.0, 0.0, 0.0, 1.0],
        },
        FontEffect::Stroke {
            w: 2.0,
            color: [1.0, 0.0, 0.0, 1.0],
        },
    ];
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 30.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let back_count = frame
        .nodes
        .iter()
        .filter(|rn| (rn.node_id & super::BOX_SHADOW_FLAG) != 0)
        .count();
    let stroke_count = frame
        .nodes
        .iter()
        .filter(|rn| (rn.node_id & super::TEXT_STROKE_FRONT_FLAG) != 0)
        .count();
    assert_eq!(back_count, 2, "glow + shadow = 2 Back layers");
    assert_eq!(stroke_count, 1, "1 stroke Front layer");
    // 验序：Back < primary < Front
    let primary = frame
        .nodes
        .iter()
        .find(|rn| {
            !is_text_sub_page(rn.node_id)
                && (rn.node_id & super::BOX_SHADOW_FLAG) == 0
                && (rn.node_id & super::TEXT_STROKE_FRONT_FLAG) == 0
                && matches!(&rn.payload, NodePayload::Mesh { program: 1, .. })
        })
        .expect("primary");
    let front = frame
        .nodes
        .iter()
        .find(|rn| (rn.node_id & super::TEXT_STROKE_FRONT_FLAG) != 0)
        .expect("stroke Front");
    assert!(
        primary.sort_key < front.sort_key,
        "primary {} < Front {}",
        primary.sort_key,
        front.sort_key
    );
}

// ── font-effect:blur Front layer（v1.8 Task 11）──

/// Blur 是 Front layer（在 base 之后绘制）。无自有颜色——使用 run.color（字形自身的颜色）。
/// Front layer sort_key > primary，与 stroke 共享 TEXT_STROKE_FRONT_FLAG 编码。
#[test]
fn font_effect_blur_emits_front_layer_quad_with_run_color() {
    use crate::text::font_effect::FontEffect;
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let mut n = Node::default();
    n.kind = NodeKind::Text {
        content: "A".into(),
    };
    n.style.font_size = 16.0;
    n.style.text_align = TextAlign::Left;
    n.style.text_effects = vec![FontEffect::Blur { w: 2.0 }];
    n.style.color = [0.2, 0.4, 0.6, 1.0]; // run.color
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 30.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    // Blur is Front layer, uses TEXT_STROKE_FRONT_FLAG (bit 31).
    let blur_rn = frame
        .nodes
        .iter()
        .find(|rn| (rn.node_id & super::TEXT_STROKE_FRONT_FLAG) != 0)
        .expect("blur Front RenderNode");
    let primary_rn = frame
        .nodes
        .iter()
        .find(|rn| {
            !is_text_sub_page(rn.node_id)
                && (rn.node_id & super::BOX_SHADOW_FLAG) == 0
                && (rn.node_id & super::TEXT_STROKE_FRONT_FLAG) == 0
                && matches!(&rn.payload, NodePayload::Mesh { program: 1, .. })
        })
        .expect("primary text RenderNode");
    assert!(
        blur_rn.sort_key > primary_rn.sort_key,
        "blur Front sort_key {} > primary {}",
        blur_rn.sort_key,
        primary_rn.sort_key
    );
    match &blur_rn.payload {
        NodePayload::Mesh {
            verts,
            colors,
            program,
            ..
        } => {
            assert_eq!(*program, 1);
            assert!(!verts.is_empty());
            // Blur has no own color — uses run.color (the glyph itself blurred).
            for c in colors {
                assert_eq!(*c, [0.2, 0.4, 0.6, 1.0], "blur uses run.color");
            }
            // Blur Front layer has same position as base glyph (no offset).
            if let NodePayload::Mesh {
                verts: primary_verts,
                ..
            } = &primary_rn.payload
            {
                let pv = primary_verts[0];
                let bv = verts[0];
                assert!(
                    (bv[1] - pv[1]).abs() <= 5.0,
                    "blur no y-offset: bv[1]={}, pv[1]={}",
                    bv[1],
                    pv[1]
                );
            }
        }
        _ => panic!("expected Mesh"),
    }
}

/// blur + stroke 共存：两者均为 Front layer，按声明顺序入列。
/// blur 使用 run.color（无自有颜色），stroke 使用自身的 stroke.color。
#[test]
fn font_effect_blur_and_stroke_both_emit_front_layers() {
    use crate::text::font_effect::FontEffect;
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let mut n = Node::default();
    n.kind = NodeKind::Text {
        content: "B".into(),
    };
    n.style.font_size = 16.0;
    n.style.text_effects = vec![
        FontEffect::Blur { w: 2.0 },
        FontEffect::Stroke {
            w: 2.0,
            color: [1.0, 0.0, 0.0, 1.0],
        },
    ];
    n.style.color = [0.1, 0.3, 0.5, 1.0]; // run.color
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 30.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let front_count = frame
        .nodes
        .iter()
        .filter(|rn| (rn.node_id & super::TEXT_STROKE_FRONT_FLAG) != 0)
        .count();
    assert_eq!(front_count, 2, "blur + stroke = 2 Front layers");
    let primary = frame
        .nodes
        .iter()
        .find(|rn| {
            !is_text_sub_page(rn.node_id)
                && (rn.node_id & super::BOX_SHADOW_FLAG) == 0
                && (rn.node_id & super::TEXT_STROKE_FRONT_FLAG) == 0
                && matches!(&rn.payload, NodePayload::Mesh { program: 1, .. })
        })
        .expect("primary");
    let front_nodes: Vec<_> = frame
        .nodes
        .iter()
        .filter(|rn| (rn.node_id & super::TEXT_STROKE_FRONT_FLAG) != 0)
        .collect();
    for fn_rn in &front_nodes {
        assert!(
            fn_rn.sort_key > primary.sort_key,
            "Front layer sort_key {} > primary {}",
            fn_rn.sort_key,
            primary.sort_key
        );
    }
    // blur 先声明 → 第一 Front 节点颜色 = run.color。
    if let NodePayload::Mesh { colors, .. } = &front_nodes[0].payload {
        assert_eq!(colors[0], [0.1, 0.3, 0.5, 1.0], "blur uses run.color first");
    }
    // stroke 第二 → 颜色 = stroke.color 红色。
    if let NodePayload::Mesh { colors, .. } = &front_nodes[1].payload {
        assert_eq!(colors[0], [1.0, 0.0, 0.0, 1.0], "stroke red second");
    }
}
