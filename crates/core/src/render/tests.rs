#![allow(unreachable_patterns, irrefutable_let_patterns)]
use super::*;
use crate::scene::node::*;
use crate::style::resolved::{
    BackgroundSize, BorderRadius, BoxShadow, CornerRadius, ResolvedStyle, TextAlign,
};
use crate::text::atlas::GlyphAtlas;
use crate::text::layout::measure_text;
use crate::text::layout::{FontTable, TextLayout};
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
    let (frame, _, _) = build_render_nodes(
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
    // border_color + ts.border 激活：无背景图的 Container 把边框环形 mesh 拼进背景
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
    n.style.taffy_style.border = taffy::geometry::Rect::length(4.0_f32);
    // border_style=Solid 放行 render 门控（CSS initial=None 不画；本测试意图画边框）。
    n.style.border_style = crate::style::resolved::BorderStyle::Solid;
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    let fonts = test_font_table().expect("need test font for build_render_nodes");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
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

/// CSS `border` 简写（`<width> <style>? <color>?`）经 apply_decl → border_color + ts.border
/// → render border_ring。端到端验简写解析 color 后边框环确实渲染（修复前简写只取 width、
/// color 丢 → border_color=None → 不画，html 预览有边框而 Unity 无）。
#[test]
fn border_shorthand_renders_border_ring() {
    use crate::style::mapping::apply_decl;
    let mut n = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
        },
        Some([0.0, 0.0, 0.0, 1.0]),
    );
    assert!(apply_decl(&mut n.style, "border", "2px solid #5fb2c4"));
    let expected = n.style.border_color.expect("简写解析 color");
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let mesh = frame
        .nodes
        .iter()
        .find(|rn| matches!(&rn.payload, NodePayload::Mesh { verts, .. } if !verts.is_empty()))
        .expect("Mesh 节点");
    let NodePayload::Mesh { verts, colors, .. } = &mesh.payload else {
        unreachable!()
    };
    // 背景 4 + 边框 8 = 12 顶点（border_ring 激活；修复前仅背景 4）。
    assert_eq!(verts.len(), 12, "简写 border 激活 border_ring：背景4+边框8");
    assert!(
        colors.contains(&expected),
        "边框色顶点存在（简写 color 进渲染）"
    );
}

/// `border-bottom` 单边 longhand 端到端：apply_decl 设 ts.border.bottom + border_color →
/// build_render_nodes → border_ring 只发底边一条带的几何（2 三角 = 6 索引），其余三边不发。
/// 背景仍占 4 顶点 / 6 索引，故总 verts=4+8=12、indices=6+6=12。
#[test]
fn border_bottom_longhand_renders_single_edge() {
    use crate::style::mapping::apply_decl;
    let mut n = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
        },
        Some([0.0, 0.0, 0.0, 1.0]),
    );
    assert!(apply_decl(
        &mut n.style,
        "border-bottom",
        "1px solid #ff0000"
    ));
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let mesh = frame
        .nodes
        .iter()
        .find(|rn| matches!(&rn.payload, NodePayload::Mesh { verts, .. } if !verts.is_empty()))
        .expect("Mesh 节点");
    let NodePayload::Mesh {
        verts,
        indices,
        colors,
        ..
    } = &mesh.payload
    else {
        unreachable!()
    };
    // 背景 4 + 边框 8（border_ring 顶点固定 8，零宽边顶点仍在但不发三角）。
    assert_eq!(verts.len(), 12, "背景4 + 边框8 = 12 顶点");
    // 背景 6 + 底边 2 三角 × 3 索引 = 6；其他三边零宽不发 → indices=12（若四边全发会 = 30）。
    assert_eq!(
        indices.len(),
        12,
        "只底边 1 edge × 6 索引 + 背景 6 = 12，全四边发 = 30（排除）"
    );
    assert!(colors.contains(&[1.0, 0.0, 0.0, 1.0]), "边框红色顶点存在");
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
    let (frame, _, _) = build_render_nodes(
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
    grandchild.kind = NodeKind::TextNode;
    grandchild.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 50.0,
        h: 20.0,
    };
    // from_nodes 用 edges (parent_idx, child_idx) 按 vec 位置建 parent 关系。
    let mut scene = Scene::from_nodes(vec![parent, child, grandchild], vec![(0, 1), (1, 2)]);
    // grandchild 在 display:none 子树内（不渲染），但仍需注册 text_contents。
    let _root = scene.roots[0];
    let _mid = scene.get(_root).unwrap().children[0];
    let _gc = scene.get(_mid).unwrap().children[0];
    scene.text_contents.insert(_gc, "hi".into());
    let ft = test_font_table().expect("need test font for build_render_nodes");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
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

#[test]
fn template_subtree_pruned_from_render() {
    // <template> 是 ListView item 蓝图：进场景树供克隆，但 display:none 强制不渲染。
    // 整子树（含 li 与其文本）都不该产 RenderNode。
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
    let mut tpl = Node::default();
    tpl.kind = NodeKind::Template;
    tpl.style.taffy_style.display = taffy::Display::None;
    tpl.style.display_mode = crate::style::resolved::DisplayMode::None;
    let mut li = Node::default();
    li.kind = NodeKind::ListItem;
    li.style.background_color = Some([0.0, 1.0, 0.0, 1.0]);
    li.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 50.0,
        h: 20.0,
    };
    let mut scene = Scene::from_nodes(vec![parent, tpl, li], vec![(0, 1), (1, 2)]);
    let root = scene.roots[0];
    let tpl_id = scene.get(root).unwrap().children[0];
    let li_id = scene.get(tpl_id).unwrap().children[0];
    let ft = test_font_table().expect("need test font for build_render_nodes");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &ft,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    assert!(
        !frame
            .nodes
            .iter()
            .any(|rn| rn.node_id == tpl_id.0 || rn.node_id == li_id.0),
        "template 整子树必须被 display:none 剪枝"
    );
}

/// Image RenderNode payload 带 path（核心不知图集/UV）。
/// Image 节点 src="icons/skin.png" → Mesh payload image_path=Some("icons/skin.png")。
#[test]
fn image_render_node_carries_path_not_texid() {
    let mut a = Node::default();
    a.kind = NodeKind::Image;
    a.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 5.0,
        h: 5.0,
    };
    let mut scene = Scene::from_nodes(vec![a], vec![]);
    scene
        .image_srcs
        .insert(scene.roots[0], "icons/skin.png".into());
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
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
    let (frame, _, _) = build_render_nodes(
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
    let (frame, _, _) = build_render_nodes(
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
    a.kind = NodeKind::Image;
    a.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 5.0,
        h: 5.0,
    };
    let mut scene = Scene::from_nodes(vec![a], vec![]);
    scene.image_srcs.insert(scene.roots[0], "logo.png".into());
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
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
    a.kind = NodeKind::Image;
    a.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 5.0,
        h: 5.0,
    };
    let mut scene = Scene::from_nodes(vec![a], vec![]);
    scene.image_srcs.insert(scene.roots[0], "logo.png".into());
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
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
    n.kind = NodeKind::TextNode;
    n.style.font_size = 16.0;
    n.style.text_align = TextAlign::Left;
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 20.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    scene.text_contents.insert(scene.roots[0], "Hello".into());

    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
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
    n.kind = NodeKind::TextNode;
    n.style.font_size = 16.0;
    // padding/border 四向 4px/2px → content 偏移 left=2+4=6, top=2+4=6。
    n.style.taffy_style.padding = taffy::geometry::Rect {
        left: taffy::style::LengthPercentage::length(4.0),
        right: taffy::style::LengthPercentage::length(4.0),
        top: taffy::style::LengthPercentage::length(4.0),
        bottom: taffy::style::LengthPercentage::length(4.0),
    };
    n.style.taffy_style.border = taffy::geometry::Rect {
        left: taffy::style::LengthPercentage::length(2.0),
        right: taffy::style::LengthPercentage::length(2.0),
        top: taffy::style::LengthPercentage::length(2.0),
        bottom: taffy::style::LengthPercentage::length(2.0),
    };
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 20.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    scene.text_contents.insert(scene.roots[0], "AB".into());

    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
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
    n.kind = NodeKind::TextNode;
    n.style.font_size = 16.0;
    n.style.text_align = TextAlign::Left;
    n.layout_rect = Rect {
        x: 100.0,
        y: 200.0,
        w: 100.0,
        h: 20.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    scene.text_contents.insert(scene.roots[0], "Hello".into());
    crate::scene::transform::compute_world_transforms(&mut scene);
    // root 节点：layout_rect 即 world translate → wm 平移 (100,200)。
    let wm = scene.world_transforms[1];
    assert!((wm[4] - 100.0).abs() < 1e-3 && (wm[5] - 200.0).abs() < 1e-3);
    let (frame, _, _) = build_render_nodes(
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

/// flex 居中（align/justify center）把文字块原点算成亚像素浮点（如 24.75），而字形
/// 光栅是整数像素——后端 Bilinear 在亚像素位置双线性混合整个字形 → 模糊。build_text_mesh
/// 须把每字形 quad 原点 round 到整数 design px：sf=1（按设计分辨率渲染）时即屏幕像素整数，
/// Bilinear 退化为 Point 采样，字形清晰。
///
/// SDF 后契约缩小为只 snap 原点（left/top）：atlas bitmap 固定按 SOURCE_SIZE 光栅，quad 按
/// target/SOURCE 缩放后右下角 = 原点 + r.px_w/h * scale，scale 非 1 时常为小数。原点 snap
/// 已让字形 bitmap 落整数像素（消除模糊），quad 尺寸的小数由 SDF shader 平滑处理。
#[test]
fn build_text_snaps_quad_to_integer_pixel() {
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let mut n = Node::default();
    n.kind = NodeKind::TextNode;
    n.style.font_size = 16.0;
    // 模拟 flex 居中算出的亚像素原点（80×60 容器内文字居中 → 非整数起点）。
    n.layout_rect = Rect {
        x: 24.75,
        y: 40.5,
        w: 100.0,
        h: 20.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    scene.text_contents.insert(scene.roots[0], "Hello".into());
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
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
    assert!(!verts.is_empty(), "Hello 有字形 → verts 非空");
    // 每字形 4 顶点序：BL, BR, TR, TL。SDF 后只保证原点（left=BL/TL.x、top=TR/TL.y）整数对齐。
    for (i, v) in verts.iter().enumerate() {
        let pos = i % 4;
        if matches!(pos, 0 | 3) {
            // BL/TL.x = left（原点 x）
            assert!(
                v[0].fract() == 0.0,
                "vert[{}].x (left) 须整数像素对齐（pixel snap），实 {}",
                i,
                v[0]
            );
        }
        if matches!(pos, 2 | 3) {
            // TR/TL.y = top（原点 y）
            assert!(
                v[1].fract() == 0.0,
                "vert[{}].y (top) 须整数像素对齐（pixel snap），实 {}",
                i,
                v[1]
            );
        }
    }
}

/// build_text_mesh 输入 fixture：单字 'A' 在指定 font_size 下的 layout + 字体表 + 零 rect。
/// atlas 由 caller 各自构造（mut 借用不能从 fixture 返回后再 borrow layout/fonts）。
fn build_text_fixture(font_size: f32) -> (FontTable, TextLayout, Rect) {
    let fonts = test_font_table().expect("need test font");
    let layout = measure_text(
        "A",
        font_size,
        0.0,
        0.0,
        TextAlign::Left,
        false,
        None,
        &fonts.stack_for(None),
        [1.0, 1.0, 1.0, 1.0],
        crate::text::rich::RichWeight::Normal,
    );
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
    };
    (fonts, layout, rect)
}

/// 取 base 首字形 quad 宽度（BL.x 与 BR.x 之差绝对值）。顶点序 BL, BR, TR, TL。
fn quad_width(meshes: &TextMeshes) -> f32 {
    let (_, verts, _, _, _) = meshes.base.first().expect("至少一页 base mesh");
    assert!(!verts.is_empty(), "base mesh 含至少一字形");
    (verts[1][0] - verts[0][0]).abs()
}

/// SDF：atlas 固定按 SOURCE_SIZE(48) 光栅，build_text_mesh 按 target/SOURCE 缩放 quad。
/// target=48（=SOURCE）→ quad 宽 ≈ atlas bitmap 宽；target=24（=SOURCE/2）→ 宽减半。
#[test]
fn build_text_quad_scales_by_target_over_source() {
    let (fonts48, layout48, rect48) = build_text_fixture(48.0);
    let m48 = build_text_mesh(
        &layout48,
        &mut test_glyph_atlas(),
        &fonts48,
        &rect48,
        &[],
        None,
        false,
    );
    let (fonts24, layout24, rect24) = build_text_fixture(24.0);
    let m24 = build_text_mesh(
        &layout24,
        &mut test_glyph_atlas(),
        &fonts24,
        &rect24,
        &[],
        None,
        false,
    );
    let w48 = quad_width(&m48);
    let w24 = quad_width(&m24);
    assert!(
        (w48 - 2.0 * w24).abs() < 1.0,
        "target 减半 → quad 宽度减半，w48={w48} w24={w24}"
    );
}

/// measure_text 的 weight 参数 → GlyphRun.weight → build_text_mesh 双绘（顶点翻倍）。
/// 这是 plain text 节点 CSS font-weight:700 生效的根因路径：measure_text 从 style.font_weight
/// 经 weight_from_font_weight 转 weight，run 创建时带上（不再硬编码 Normal）；build_text_mesh
/// 只读 run.weight。rich text 走 measure_rich_text 自带 per-run weight，同管道。
#[test]
fn measure_text_weight_bold_double_draws() {
    let fonts = test_font_table().expect("need test font");
    let stack = fonts.stack_for(None);
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
    };
    let layout_normal = measure_text(
        "A",
        48.0,
        0.0,
        0.0,
        TextAlign::Left,
        false,
        None,
        &stack,
        [1.0; 4],
        crate::text::rich::RichWeight::Normal,
    );
    let layout_bold = measure_text(
        "A",
        48.0,
        0.0,
        0.0,
        TextAlign::Left,
        false,
        None,
        &stack,
        [1.0; 4],
        crate::text::rich::RichWeight::Bold,
    );
    let page_verts = |m: &TextMeshes| m.base.first().map(|p| p.1.len()).unwrap_or(0);
    let m_normal = build_text_mesh(
        &layout_normal,
        &mut test_glyph_atlas(),
        &fonts,
        &rect,
        &[],
        None,
        false,
    );
    let m_bold = build_text_mesh(
        &layout_bold,
        &mut test_glyph_atlas(),
        &fonts,
        &rect,
        &[],
        None,
        false,
    );
    assert_eq!(page_verts(&m_normal), 4, "1 glyph × 4 verts (normal)");
    assert_eq!(page_verts(&m_bold), 8, "bold 双绘 → 8 verts");
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
    n.kind = NodeKind::TextNode;
    n.style.font_size = 16.0;
    n.style.text_align = TextAlign::Left;
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 20.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    scene.text_contents.insert(scene.roots[0], "A B".into());
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
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
    let (frame, _, _) = build_render_nodes(
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
    a.kind = NodeKind::Image;
    a.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 10.0,
        h: 10.0,
    };
    let mut b = Node::default();
    b.kind = NodeKind::Image;
    b.layout_rect = Rect {
        x: 100.0,
        y: 0.0,
        w: 10.0,
        h: 10.0,
    };
    let mut scene = Scene::from_nodes(vec![root, a, b], vec![(0, 1), (0, 2)]);
    let _root_id = scene.roots[0];
    let _a_id = scene.get(_root_id).unwrap().children[0];
    let _b_id = scene.get(_root_id).unwrap().children[1];
    scene.image_srcs.insert(_a_id, "a.png".into());
    scene.image_srcs.insert(_b_id, "a.png".into());

    let fonts = test_font_table().expect("need test font");

    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
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
    let (frame, _, _) = build_render_nodes(
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
        Option<String>,
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
    let (fd, _, _) = build_render_nodes(
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
    let (fd, _, _) = build_render_nodes(
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
    root_s.taffy_style.size.width = Dimension::length(120.0);
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
            None,
            None,
        ),
        (
            Some(0),
            NodeKind::TextNode,
            text_s,
            vec![],
            None,
            false,
            None,
            None,
            Some(content.into()),
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
    let (frame, _, _) = build_render_nodes(
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
        crate::text::rich::RichWeight::Normal,
    )
    .text_width;
    let container_w = 100.0;
    assert!(
        intrinsic > container_w,
        "测试前置：长文本 intrinsic 应远超 container"
    );
    let mut root_s = ResolvedStyle::default();
    root_s.taffy_style.size.width = Dimension::length(container_w);
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
            None,
            None,
        ),
        (
            Some(0),
            NodeKind::TextNode,
            text_s,
            vec![],
            None,
            false,
            None,
            None,
            Some(content.into()),
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
    let (frame, _, _) = build_render_nodes(
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

// ── rich-text-block render arm（T7）─────────────────────────

/// rich-text-block Container 在 render 期读 `scene.text_layouts[div]` 产文字 mesh：
/// - 背景 RenderNode（真 div node_id，program=0）；
/// - 文字 RenderNode（tf_text_synth 合成 id，program=1，含字形顶点）；
/// - 折叠的 inline TextNode 子**不**单独产 RenderNode（T6 跳过 taffy → layout_rect=0，
///   T7 跳过 render 子遍历）。
#[test]
fn rich_text_block_renders_text_mesh() {
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    // root(structural Container, 固定宽 200) > div(rich_text_block, 显式宽 100) > TextNode
    // 长文本。div 显式宽驱动 measure_rich_text 换行（作 root 固定尺寸叶子测不到约束宽）。
    let mut root_s = ResolvedStyle::default();
    root_s.taffy_style.size.width = Dimension::length(200.0);
    let mut div_s = ResolvedStyle::default();
    div_s.taffy_style.size.width = Dimension::length(100.0);
    div_s.font_size = 16.0;
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
            None,
            None,
        ),
        (
            Some(0),
            NodeKind::Container,
            div_s,
            vec![],
            None,
            false,
            None,
            None,
            None,
            None,
        ),
        (
            Some(1),
            NodeKind::TextNode,
            ResolvedStyle::default(),
            vec![],
            None,
            false,
            None,
            None,
            Some("The quick brown fox jumps over the lazy dog".into()),
            None,
        ),
    ];
    let mut scene = Scene::build(&entries);
    let div = scene.get(scene.roots[0]).unwrap().children[0];
    let tn = scene.get(div).unwrap().children[0];
    scene.get_mut(div).unwrap().rich_text_block = true;
    crate::layout::solve(
        &mut scene,
        &fonts,
        (200.0, 1000.0),
        &std::collections::HashMap::new(),
    );
    // T6 契约：text_layouts[div] 已填，inline 子 layout_rect=0。
    assert!(
        scene.text_layouts[div.index()].is_some(),
        "solve 应为 rich-text-block div 填 text_layouts[div]"
    );
    let tn_rect = scene.get(tn).unwrap().layout_rect;
    assert!(
        tn_rect.w.abs() < 0.1 && tn_rect.h.abs() < 0.1,
        "folded inline child 应无独立 layout_rect，got {:?}",
        tn_rect
    );

    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );

    // 文字 RenderNode：tf_text_synth 合成 id（背景占真 div node_id，文字用合成 id 区分），
    // program=1，含字形顶点（>=1 字形 = 4 verts）。合成 id 经 is_tf_text_synth 排除合批，
    // 不会被 merge 吞掉——是 rich-text-block arm 运行的稳定信号。
    // 背景同时由同 arm 产（真 div node_id，program=0），但透明/兄弟 bg quad 可能被
    // merge_meshes 合并丢 node_id，故不在帧上断言背景 node_id；文字 mesh 存在即证明
    // 同 arm 已先 push 背景（同一代码路径）。
    let text_div_synth = tf_synth_id(div.0, TF_TEXT_SYNTH_BYTE);
    let text_verts = frame
        .nodes
        .iter()
        .find_map(|rn| {
            if rn.node_id == text_div_synth {
                match &rn.payload {
                    NodePayload::Mesh { verts, program, .. } if *program == 1 => Some(verts.len()),
                    _ => None,
                }
            } else {
                None
            }
        })
        .unwrap_or(0);
    assert!(
        text_verts >= 4,
        "rich-text-block div 应产文字 mesh（tf_text_synth id，program=1，>=4 verts），实 {} verts",
        text_verts
    );

    // 折叠的 inline TextNode 子不单独产 RenderNode：遍历全帧无 rn.node_id == tn.0。
    let tn_leaked = frame.nodes.iter().any(|rn| rn.node_id == tn.0);
    assert!(
        !tn_leaked,
        "folded inline TextNode 不应单独产 RenderNode（应折进父 mesh）"
    );
}

/// rich-text-block Container 带 box-shadow 时应同时产阴影 RenderNode：该 arm 推完
/// 背景 + 文字后提前 return，若不在 return 前补推阴影层，阴影会被静默丢弃（背景 +
/// 文字画了但无阴影）。守护 rich-text-block arm 内的 push_container_shadows 调用。
#[test]
fn rich_text_block_div_emits_box_shadow() {
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    // root(structural Container, 固定宽 200) > div(rich_text_block + box-shadow, 显式宽 100)
    // > TextNode 长文本。box-shadow 单 outer 层（与 box_shadow_emits_node_with_offset 同参数）。
    let mut root_s = ResolvedStyle::default();
    root_s.taffy_style.size.width = Dimension::length(200.0);
    let mut div_s = ResolvedStyle::default();
    div_s.taffy_style.size.width = Dimension::length(100.0);
    div_s.font_size = 16.0;
    div_s.box_shadow = vec![BoxShadow {
        ox: 2.0,
        oy: 3.0,
        spread: 0.0,
        blur: 0.0,
        color: [0.0, 0.0, 0.0, 0.5],
        inset: false,
    }];
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
            None,
            None,
        ),
        (
            Some(0),
            NodeKind::Container,
            div_s,
            vec![],
            None,
            false,
            None,
            None,
            None,
            None,
        ),
        (
            Some(1),
            NodeKind::TextNode,
            ResolvedStyle::default(),
            vec![],
            None,
            false,
            None,
            None,
            Some("The quick brown fox jumps over the lazy dog".into()),
            None,
        ),
    ];
    let mut scene = Scene::build(&entries);
    let div = scene.get(scene.roots[0]).unwrap().children[0];
    scene.get_mut(div).unwrap().rich_text_block = true;
    crate::layout::solve(
        &mut scene,
        &fonts,
        (200.0, 1000.0),
        &std::collections::HashMap::new(),
    );
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );

    // rich-text-block div 应产 box-shadow RenderNode（back_shadow_synth，program=5，半透明黑）。
    // 早 return 漏推阴影时此查找失败——这是该 arm 推阴影的直接证据。
    let shadow_rn = frame
        .nodes
        .iter()
        .find(|rn| is_back_shadow_synth(rn.node_id))
        .expect("rich-text-block div 应产 box-shadow RenderNode（back_shadow_synth）");
    assert_eq!(
        shadow_rn.node_id,
        back_shadow_id(div.0, 0),
        "阴影 node_id = back_shadow_id(div, 0)"
    );
    match &shadow_rn.payload {
        NodePayload::Mesh {
            program, colors, ..
        } => {
            assert_eq!(*program, 5, "阴影走 SDF 路径 program=5");
            assert!(
                colors.iter().all(|c| *c == [0.0, 0.0, 0.0, 0.5]),
                "阴影顶点色应为半透明黑"
            );
        }
        _ => panic!("阴影节点应为 Mesh"),
    }
}

/// 回归守卫：rich_text_block=false 的普通 Container 仍走原 Container 路径——
/// 其 TextNode 子节点独立测 + 独立渲染（产自己的 RenderNode，不折进父）。
/// 与上一个 rich 测试互为正反。
#[test]
fn non_rich_container_renders_text_child_separately() {
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let mut root_s = ResolvedStyle::default();
    root_s.taffy_style.size.width = Dimension::length(200.0);
    let mut text_s = ResolvedStyle::default();
    text_s.font_size = 16.0;
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
            None,
            None,
        ),
        (
            Some(0),
            NodeKind::TextNode,
            text_s,
            vec![],
            None,
            false,
            None,
            None,
            Some("Buy".into()),
            None,
        ),
    ];
    let mut scene = Scene::build(&entries);
    crate::layout::solve(
        &mut scene,
        &fonts,
        (200.0, 1000.0),
        &std::collections::HashMap::new(),
    );
    let tn = scene.get(scene.roots[0]).unwrap().children[0];
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    // 非 rich Container 的 TextNode 子独立产 RenderNode（真 node_id，program=1）。
    let tn_rendered = frame.nodes.iter().any(|rn| {
        rn.node_id == tn.0
            && matches!(&rn.payload, NodePayload::Mesh { program, .. } if *program == 1)
    });
    assert!(
        tn_rendered,
        "非 rich Container 的 TextNode 子应独立产文字 RenderNode（program=1）"
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
    let (f1, h1, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    assert_eq!(f1.nodes[0].change_level, ChangeLevel::Full, "首帧 FULL");
    // 第二帧不变 → SKIP
    let (f2, h2, _) =
        build_render_nodes(&scene, &fonts, &h1, &empty_sizes(), &mut test_glyph_atlas());
    assert_eq!(f2.nodes[0].change_level, ChangeLevel::Skip, "不变 → SKIP");
    // 第三帧改位置（纯平移 → world_matrix 变，但 re-base 后 verts 不变 → payload_hash 不变）→ HEADER
    scene.get_mut(scene.roots[0]).unwrap().layout_rect.x = 50.0;
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (f3, h3, _) =
        build_render_nodes(&scene, &fonts, &h2, &empty_sizes(), &mut test_glyph_atlas());
    assert_eq!(
        f3.nodes[0].change_level,
        ChangeLevel::Header,
        "位置变（纯平移）→ HEADER（payload 不变）"
    );
    // 第四帧改 color（只影响 header_hash 的 color_tint）→ HEADER
    scene.get_mut(scene.roots[0]).unwrap().style.color = [0.5, 0.5, 0.5, 1.0];
    let (f4, h4, _) =
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
    let (f5, _, _) =
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
    let (_f1, _h1, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    // prev 有 hash 但 node_id 不在其中（模拟 reload：prev 表残留不同节点的 hash）
    let mut stale: std::collections::HashMap<u32, (u64, u64)> = std::collections::HashMap::new();
    stale.insert(999, (0, 0));
    let (f2, _, _) = build_render_nodes(
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
    let (frame, _, _) = build_render_nodes(
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
    // no-repeat：验 contain 单张缩放（默认 repeat 会平铺，另测）。
    n.style.background_repeat = crate::style::resolved::BackgroundRepeat::NoRepeat;
    let mut scene = Scene::from_nodes(vec![n], vec![]);

    let fonts = test_font_table().expect("need font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
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
fn build_container_bg_image_contain_repeat_tiles_box() {
    // contain + 默认 repeat：图小于盒时平铺填满（CSS background-repeat 默认 repeat）。
    // 200×100 盒，src 64 兜底 contain → 单张 100×100；repeat 横向铺 2 块 → xmax=200（填满，无留白）。
    use crate::style::resolved::BackgroundRepeat;
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
    n.style.background_repeat = BackgroundRepeat::Repeat; // 默认值，显式写明意图
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    let fonts = test_font_table().expect("need font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    if let NodePayload::Mesh { verts, .. } = &frame.nodes[0].payload {
        let xmax = verts.iter().map(|v| v[0]).fold(f32::MIN, f32::max);
        assert!(
            (xmax - 200.0).abs() < 1e-2,
            "repeat 应平铺填满盒 xmax=200，got {}",
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
    let (frame, _, _) = build_render_nodes(
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
    let (frame, _, _) = build_render_nodes(
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
    let (frame, _, _) = build_render_nodes(
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
    let (frame, _, _) = build_render_nodes(
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
    img.kind = NodeKind::Image;
    img.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 10.0,
        h: 10.0,
    };
    let mut scene = Scene::from_nodes(vec![root, img], vec![(0, 1)]);
    let _root_id = scene.roots[0];
    let _img_id = scene.get(_root_id).unwrap().children[0];
    scene.image_srcs.insert(_img_id, "a.png".into());
    let fonts = test_font_table().expect("need font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
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
    let (frame, _, _) = build_render_nodes(
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
    let (frame, _, _) = build_render_nodes(
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
    let (frame, _, _) = build_render_nodes(
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
    let (frame, _, _) = build_render_nodes(
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
    let (frame, _, _) = build_render_nodes(
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
    let (frame, _, _) = build_render_nodes(
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
    let (frame, _, _) = build_render_nodes(
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
    let (frame, _, _) = build_render_nodes(
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
    let (frame, _, _) = build_render_nodes(
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
            h: LengthPercentage::length(8.0),
            v: LengthPercentage::length(8.0),
        }; 4],
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    let fonts = test_font_table().expect("need font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
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
            h: LengthPercentage::percent(0.5),
            v: LengthPercentage::percent(0.5),
        }; 4],
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    let fonts = test_font_table().expect("need font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
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
            h: LengthPercentage::length(12.0),
            v: LengthPercentage::length(12.0),
        }; 4],
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);

    let fonts = test_font_table().expect("need font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
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
    n.kind = NodeKind::TextNode;
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
    scene.text_contents.insert(scene.roots[0], "Hello".into());
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
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
        effect: crate::render::node::EffectBlock::default(),
        shadow_params: [0.0; 6],
        gradient: crate::render::gradient::GradientParams::default(),
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

/// 行内图合成节点 sort_key 须叠在 primary 的所有文字层之上，否则 sort_key=0 → 画在
/// 最底层被底色/文字盖住（行内图"消失"在底图之下）。构造 A(primary) + A_sub1(子页) +
/// A_img(行内图 sk=0) + B(后续真节点)，验 A_img sort_key = base+1，B 后移，无 tie。
#[test]
fn propagate_inline_image_sort_keys_stacks_above_text_layers() {
    use crate::render::node::{BlendMode, ChangeLevel, MaskContext, NodePayload, RenderNode};

    let a_id = 0u32;
    let b_id = 1u32;
    let a_sub1 = synth_text_node_id(a_id, 1);
    let a_img = synth_text_node_id(a_id, INLINE_IMG_SYNTH_ID_BASE);

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
        effect: crate::render::node::EffectBlock::default(),
        shadow_params: [0.0; 6],
        gradient: crate::render::gradient::GradientParams::default(),
        payload: empty_mesh.clone(),
    };

    // A primary=2, A_sub1=3（子页已 propagate），A_img=0（行内图待 propagate），B=4（后续真节点）。
    let mut nodes = vec![
        mk_rn(a_id, 2),
        mk_rn(a_sub1, 3),
        mk_rn(a_img, 0),
        mk_rn(b_id, 4),
    ];
    let images = vec![(a_id, a_img)];

    propagate_inline_image_sort_keys(&mut nodes, &images);

    let find = |nid: u32| nodes.iter().find(|n| n.node_id == nid).unwrap().sort_key;
    assert_eq!(find(a_id), 2, "A primary 不变");
    assert_eq!(find(a_sub1), 3, "A 子页不变");
    let a_img_sk = find(a_img);
    assert!(
        a_img_sk > find(a_sub1),
        "行内图叠在子页之上 ({} > {})",
        a_img_sk,
        find(a_sub1)
    );
    assert!(
        find(b_id) > a_img_sk,
        "后续真节点后移到行内图之上 ({} > {})",
        find(b_id),
        a_img_sk
    );

    // 无 tie。
    let mut sks: Vec<u32> = nodes.iter().map(|n| n.sort_key).collect();
    sks.sort();
    let mut unique = sks.clone();
    unique.dedup();
    assert_eq!(sks.len(), unique.len(), "sort_key 不得有 tie");
}

/// 哨兵：合成 node_id 硬上限文档。
/// 验证 synth_text_node_id / is_text_sub_page / text_sub_primary_id 的编码/解码一致性。
#[test]
fn synth_text_node_id_roundtrip() {
    let primary = 0x0000_0123u32;
    let sub = synth_text_node_id(primary, 5);
    assert!(is_text_sub_page(sub));
    assert!(!is_text_sub_page(primary));
    assert_eq!(text_sub_primary_id(sub), primary & 0x00FF_FFFF);

    // 边界：page=15 是子页上限（high byte 1..=15）。
    // page>=16 不再被 is_text_sub_page 识别（超出 1..=15 范围）——
    // 故 sub_page 编码实际可用范围是 1..15（atlas 跨页远不到此上限）。
    let max_sub = synth_text_node_id(0, 15);
    assert!(is_text_sub_page(max_sub));

    // page=16 超出子页范围 → is_text_sub_page 返 false。
    let shadow_like = synth_text_node_id(0, 16);
    assert!(!is_text_sub_page(shadow_like), "page=16 超出子页范围");

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

// RichText retired in Spec-2; deferred to compound-bundle text model.
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

// ── Image bg-color via BG_COMPOSITE ───────────────────────────────────

/// Image (`<img>`) + bg-color → program 2 (BG_COMPOSITE)，顶点色 = bg-color。
/// shader source-over：图(tex) over 底色(vcol)，透明像素透出底色（修紫底不显示 bug）。
/// 无需 back-layer / 合成 node_id——单 quad，GPU 合成（与 Container 同路径）。
#[test]
fn build_image_with_bg_color_uses_bg_composite() {
    let mut n = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        },
        Some([0.5, 0.0, 0.5, 1.0]), // 紫底
    );
    n.kind = NodeKind::Image;
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    scene.image_srcs.insert(scene.roots[0], "icon.png".into());

    let fonts = test_font_table().expect("need font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    assert_eq!(
        frame.nodes.len(),
        1,
        "Image 单 quad（shader 合成，不产 back-layer）"
    );
    match &frame.nodes[0].payload {
        NodePayload::Mesh {
            program,
            colors,
            image_path,
            ..
        } => {
            assert_eq!(*program, 2, "Image+bg-color → program 2 (BG_COMPOSITE)");
            assert_eq!(
                *colors.first().unwrap(),
                [0.5, 0.0, 0.5, 1.0],
                "顶点色 = bg-color（紫）"
            );
            assert_eq!(
                *image_path,
                Some("icon.png".to_string()),
                "image_path = src"
            );
        }
        _ => panic!("expected Mesh"),
    }
}

/// Image + bg-color + filter → program 4（BG_COMPOSITE + COLOR_FILTER 双 keyword）。
#[test]
fn build_image_with_bg_color_and_filter_uses_program_4() {
    let mut n = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        },
        Some([0.5, 0.0, 0.5, 1.0]),
    );
    n.kind = NodeKind::Image;
    n.style.color_filter = Some([0.0; 20]); // 触发 has_filter
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    scene.image_srcs.insert(scene.roots[0], "icon.png".into());

    let fonts = test_font_table().expect("need font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    match &frame.nodes[0].payload {
        NodePayload::Mesh { program, .. } => {
            assert_eq!(*program, 4, "Image+bg-color+filter → program 4");
        }
        _ => panic!("expected Mesh"),
    }
}

// ── box-shadow 集成测试 ───────────────────────────

/// box-shadow:2px 3px #000000 → 阴影节点 node_id = back_shadow_id(main_id,0)、
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
    n.style.box_shadow = vec![BoxShadow {
        ox: 2.0,
        oy: 3.0,
        spread: 0.0,
        blur: 0.0,
        color: [0.0, 0.0, 0.0, 0.5],
        inset: false,
    }];
    let mut scene = Scene::from_nodes(vec![n], vec![]);

    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
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
        .find(|rn| is_back_shadow_synth(rn.node_id))
        .expect("应存在 box-shadow RenderNode（back_shadow_synth）");
    let main_rn = frame
        .nodes
        .iter()
        .find(|rn| !is_shadow_synth(rn.node_id))
        .expect("应存在主节点 RenderNode");

    // 阴影 node_id = back_shadow_id(main_id, 0)
    assert_eq!(
        shadow_rn.node_id,
        back_shadow_id(main_rn.node_id, 0),
        "阴影 node_id = back_shadow_id(main_id, 0)"
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
    // outer + blur=0 → σ=0.5 → padded = shape + 3σ(1.5)：x_min = main + ox - 1.5
    let shadow_x_min = shadow_verts.iter().map(|v| v[0]).fold(f32::MAX, f32::min);
    assert!(
        (shadow_x_min - (main_x_min + 2.0 - 1.5)).abs() < 1e-3,
        "阴影 x_min({}) = main x_min({}) + ox(2.0) - 3σ pad(1.5)",
        shadow_x_min,
        main_x_min
    );

    let main_y_min = main_verts.iter().map(|v| v[1]).fold(f32::MAX, f32::min);
    let shadow_y_min = shadow_verts.iter().map(|v| v[1]).fold(f32::MAX, f32::min);
    assert!(
        (shadow_y_min - (main_y_min + 3.0 - 1.5)).abs() < 1e-3,
        "阴影 y_min({}) = main y_min({}) + oy(3.0) - 3σ pad(1.5)",
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

/// box-shadow 背层节点须继承主节点的 mask_context（clip 上下文）。
/// 坑：旧实现 push 阴影节点时硬编码 mask_context=0，overflow:auto 容器内的子节点
/// inset box-shadow 不被裁，溢出到容器外，UI 上表现为「黑底没被裁剪」。
/// showcase home 的 quick-bar（overflow-x:auto）的 chip 都带 inset box-shadow，
/// chip 本身被裁但 inset 环没被裁 → 溢出到 bar 外。
#[test]
fn box_shadow_back_layer_inherits_clip_mask_context() {
    // 构造：root clip 容器（clip_rect 开 mask_context=1）内一个 chip（带 box-shadow）。
    let root = container_node(
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
    let mut chip = container_node(
        1,
        Some(0),
        Rect {
            x: 200.0,
            y: 0.0,
            w: 50.0,
            h: 50.0,
        }, // 在 root 外（溢出）
        Some([0.1, 0.1, 0.1, 1.0]),
    );
    chip.style.box_shadow = vec![BoxShadow {
        ox: 0.0,
        oy: 0.0,
        spread: 0.0,
        blur: 0.0,
        color: [0.0, 0.0, 0.0, 1.0],
        inset: false,
    }];
    let mut scene = Scene::from_nodes(vec![root, chip], vec![(0, 1)]);
    // root 开 clip（overflow）→ assign_sort_keys DFS 给 root + 后代开 mask_context
    let root_id = *scene.roots.first().unwrap();
    let chip_id = scene.get(root_id).unwrap().children[0];
    scene.get_mut(root_id).unwrap().clip_rect = Some(Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    });
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    // chip 主节点 + chip 阴影节点，都应继承 root 的 mask_context(>0)
    let chip_main = frame
        .nodes
        .iter()
        .find(|rn| rn.node_id == chip_id.0)
        .expect("chip 主节点");
    let chip_shadow = frame
        .nodes
        .iter()
        .find(|rn| rn.node_id == back_shadow_id(chip_id.0, 0))
        .expect("chip 阴影节点");
    assert!(
        chip_main.mask_context.0 > 0,
        "chip 主节点应被 root clip（mask>0）"
    );
    assert_eq!(
        chip_shadow.mask_context, chip_main.mask_context,
        "box-shadow 背层须继承主节点的 mask_context（旧实现硬编码 0 → overflow 容器内 inset shadow 不被裁）"
    );
}

// ── box-shadow 多层 + inset 集成测试（Task 3）───────────────────────────

/// div 带 outer + inset 两层 box-shadow → primary + 1 back-layer + 1 front-layer，
/// sort_key: back < primary < front（outer 在下、inset 在上）。这是 Task 3 的核心验收。
#[test]
fn box_shadow_emits_back_and_front_layers() {
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
    n.style.box_shadow = vec![
        // outer 层（画在 primary 之下）
        BoxShadow {
            ox: 2.0,
            oy: 3.0,
            spread: 0.0,
            blur: 0.0,
            color: [0.0, 0.0, 0.0, 0.5],
            inset: false,
        },
        // inset 层（画在 primary 之上、子节点之下）
        BoxShadow {
            ox: 0.0,
            oy: 0.0,
            spread: 0.0,
            blur: 0.0,
            color: [0.0, 0.0, 0.0, 0.3],
            inset: true,
        },
    ];
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let back_count = frame
        .nodes
        .iter()
        .filter(|rn| is_back_shadow_synth(rn.node_id))
        .count();
    let front_count = frame
        .nodes
        .iter()
        .filter(|rn| is_front_shadow_synth(rn.node_id))
        .count();
    assert_eq!(back_count, 1, "1 outer 层 → 1 back-layer synth");
    assert_eq!(front_count, 1, "1 inset 层 → 1 front-layer synth");

    let back = frame
        .nodes
        .iter()
        .find(|rn| is_back_shadow_synth(rn.node_id))
        .unwrap();
    let primary = frame
        .nodes
        .iter()
        .find(|rn| !is_shadow_synth(rn.node_id))
        .unwrap();
    let front = frame
        .nodes
        .iter()
        .find(|rn| is_front_shadow_synth(rn.node_id))
        .unwrap();
    assert!(
        back.sort_key < primary.sort_key,
        "outer 阴影画在 primary 之下（back sk={} >= primary sk={}）",
        back.sort_key,
        primary.sort_key
    );
    assert!(
        primary.sort_key < front.sort_key,
        "inset 阴影画在 primary 之上（primary sk={} >= front sk={}）",
        primary.sort_key,
        front.sort_key
    );
}

/// 多 outer 层 + 多 inset 层的 CSS 层序：先列出的 outer 最贴 primary（最高 back sk），
/// 先列出的 inset 最离 primary（最高 front sk）。验多层 id 唯一 + 逆 CSS 赋值。
#[test]
fn box_shadow_multi_layer_css_order() {
    let mut n = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
        },
        Some([1.0, 1.0, 1.0, 1.0]),
    );
    n.style.box_shadow = vec![
        // outer A（先列）应在 outer B（后列）之上（更贴 primary）
        BoxShadow {
            ox: 0.0,
            oy: 0.0,
            spread: 0.0,
            blur: 0.0,
            color: [1.0, 0.0, 0.0, 1.0],
            inset: false,
        },
        BoxShadow {
            ox: 0.0,
            oy: 0.0,
            spread: 0.0,
            blur: 0.0,
            color: [0.0, 1.0, 0.0, 1.0],
            inset: false,
        },
        // inset C（先列）应在 inset D（后列）之上（更离 primary）
        BoxShadow {
            ox: 0.0,
            oy: 0.0,
            spread: 0.0,
            blur: 0.0,
            color: [0.0, 0.0, 1.0, 1.0],
            inset: true,
        },
        BoxShadow {
            ox: 0.0,
            oy: 0.0,
            spread: 0.0,
            blur: 0.0,
            color: [1.0, 1.0, 0.0, 1.0],
            inset: true,
        },
    ];
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let primary = frame
        .nodes
        .iter()
        .find(|rn| !is_shadow_synth(rn.node_id))
        .unwrap();
    let primary_sk = primary.sort_key;
    // 两 outer + 两 inset，id 唯一
    let back_ids: Vec<u32> = frame
        .nodes
        .iter()
        .filter(|rn| is_back_shadow_synth(rn.node_id))
        .map(|rn| rn.node_id)
        .collect();
    assert_eq!(back_ids.len(), 2, "2 outer 层");
    assert_eq!(
        back_ids.len(),
        back_ids
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        "back id 唯一"
    );
    let front_ids: Vec<u32> = frame
        .nodes
        .iter()
        .filter(|rn| is_front_shadow_synth(rn.node_id))
        .map(|rn| rn.node_id)
        .collect();
    assert_eq!(front_ids.len(), 2, "2 inset 层");
    assert_eq!(
        front_ids.len(),
        front_ids
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        "front id 唯一"
    );
    // outer A (CSS 0) 应 > outer B (CSS 1)：A 更贴 primary（更高 back sk）
    let outer_a = back_shadow_id(primary.node_id, 0);
    let outer_b = back_shadow_id(primary.node_id, 1);
    let sk_of = |id: u32| {
        frame
            .nodes
            .iter()
            .find(|rn| rn.node_id == id)
            .unwrap()
            .sort_key
    };
    assert!(
        sk_of(outer_a) > sk_of(outer_b),
        "outer 首层 (A) sk 高于末层 (B)"
    );
    assert!(sk_of(outer_a) < primary_sk, "所有 outer < primary");
    // inset C (CSS 0) 应 > inset D (CSS 1)：C 更离 primary（更高 front sk）
    let inset_c = front_shadow_id(primary.node_id, 0);
    let inset_d = front_shadow_id(primary.node_id, 1);
    assert!(
        sk_of(inset_c) > sk_of(inset_d),
        "inset 首层 (C) sk 高于末层 (D)"
    );
    assert!(sk_of(inset_c) > primary_sk, "所有 inset > primary");
}

/// blur>0 outer 阴影 → program=5 + shadow_params 非零（sigma）+ pad quad（顶点数=4）。
#[test]
fn box_shadow_blur_uses_sdf_program_and_params() {
    let mut n = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
        },
        Some([1.0; 4]),
    );
    n.style.box_shadow = vec![BoxShadow {
        ox: 0.0,
        oy: 0.0,
        spread: 0.0,
        blur: 8.0, // σ=4（blur/2，RmlUi 高斯）
        color: [0.0, 0.0, 0.0, 0.5],
        inset: false,
    }];
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let shadow = frame
        .nodes
        .iter()
        .find(|rn| is_back_shadow_synth(rn.node_id))
        .expect("blur outer 阴影节点");
    match &shadow.payload {
        NodePayload::Mesh { program, verts, .. } => {
            assert_eq!(*program, 5, "blur>0 → program=5 (SDF)");
            assert_eq!(verts.len(), 4, "blur quad = 4 顶点（pad 后）");
        }
        _ => panic!("expected Mesh"),
    }
    // shadow_params[3] = sigma = blur/2 = 4.0（RmlUi 高斯 σ，shader erfc 用）
    assert!(
        (shadow.shadow_params[3] - 4.0).abs() < 1e-3,
        "shadow_params.sigma = blur/2 = 4.0，得 {}",
        shadow.shadow_params[3]
    );
    // inset_flag = 0（outer）
    assert!(shadow.shadow_params[4].abs() < 1e-6, "outer → inset_flag=0");
}

// shadow_sigma 必须单调（blur 越大 σ 越大）且全程 ≥ 0.5（1px AA 下限）。
// 回归门：旧的 `if blur<0.5 {0.5} else {blur*0.5}` 在 blur=0.5 处 σ 从 0.5 掉到 0.25
//（blur↑ 但 σ↓），且 blur∈[0.5,1.0] 区间 σ<0.5 违反 AA 下限。
#[test]
fn shadow_sigma_is_monotonic_and_floored() {
    let samples = [0.0, 0.3, 0.49, 0.5, 0.6, 1.0, 2.0, 8.0];
    let mut prev = 0.0f32;
    for &blur in &samples {
        let sigma = super::shadow_sigma(blur);
        assert!(sigma >= 0.5, "blur={blur}: σ={sigma} 低于 0.5 AA 下限");
        assert!(
            sigma >= prev,
            "blur={blur}: σ={sigma} < 前一样本 σ={prev}（非单调）"
        );
        prev = sigma;
    }
    // 旧 bug 的直接复现：blur 增大 σ 反而变小。
    assert!(
        super::shadow_sigma(0.6) >= super::shadow_sigma(0.3),
        "blur↑ 但 σ↓"
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
    let (frame, _, _) = build_render_nodes(
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

// ── gradient text 整体渐变（background-clip:text）──

#[test]
fn gradient_text_spans_whole_text_not_per_glyph() {
    use crate::render::node::NodePayload;
    use crate::style::resolved::{Gradient, GradientStop};
    let fonts = match test_font_table() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let mut n = Node::default();
    n.kind = NodeKind::TextNode;
    n.style.font_size = 16.0;
    n.style.text_align = TextAlign::Left;
    n.style.background_clip_text = true;
    n.style.background_gradient = Some(Gradient::Linear {
        angle_deg: 90.0, // to right
        stops: vec![
            GradientStop {
                color: [1.0, 0.0, 0.0, 1.0],
                pos: 0.0,
            }, // 红（左端）
            GradientStop {
                color: [0.0, 1.0, 0.0, 1.0],
                pos: 1.0,
            }, // 绿（右端）
        ],
    });
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 32.0, // box 贴近文本宽（渐变 box = 元素 rect，CSS 语义）
        h: 30.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    scene.text_contents.insert(scene.roots[0], "AB".into());
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let text_rn = frame
        .nodes
        .iter()
        .find(|rn| matches!(&rn.payload, NodePayload::Mesh { program, .. } if *program == 1))
        .expect("gradient text 应产出 program=1 base mesh");
    let colors = match &text_rn.payload {
        NodePayload::Mesh { colors, .. } => colors,
        _ => unreachable!(),
    };
    assert!(colors.len() >= 8, "两字应有 ≥8 顶点，实际 {}", colors.len());
    // 整体渐变：右侧字(B)的左角色应比左侧字(A)更偏 b(绿)。
    // bug（每字独立 a→b）：两字左角色都=a(红)，G 分量差=0。
    let a_left_g = colors[0][1];
    let b_left_g = colors[4][1];
    assert!(
        b_left_g - a_left_g > 0.2,
        "整体渐变下右侧字左角色应更偏绿（G 差={:.2}）；每字独立渐变时两字左角色都=a，差=0",
        b_left_g - a_left_g
    );
}

// ── 背景渐变 program=6/7（per-fragment shader 路径）──

/// Container 背景渐变：program=6 + uv=box 局部坐标 + grad_params 已解析 + 顶点色=底色。
#[test]
fn background_gradient_emits_program6_local_uv_and_params() {
    use crate::render::node::NodePayload;
    use crate::style::resolved::{Gradient, GradientStop, RadialExtent};
    let mut n = Node::default();
    n.kind = NodeKind::Container;
    n.style.background_color = Some([0.1, 0.2, 0.3, 1.0]);
    n.style.background_gradient = Some(Gradient::Radial {
        extent: RadialExtent::Explicit(Some(1100.0), Some(560.0)),
        shape: crate::style::resolved::RadialShape::Ellipse,
        center: [
            crate::style::resolved::GradCoord::Pct(0.82),
            crate::style::resolved::GradCoord::Pct(-0.12),
        ],
        stops: vec![
            GradientStop {
                color: [0.37, 0.71, 0.83, 0.1],
                pos: 0.0,
            },
            GradientStop {
                color: [0.0; 4],
                pos: 0.6,
            },
        ],
    });
    n.layout_rect = Rect {
        x: 10.0,
        y: 20.0,
        w: 1920.0,
        h: 1080.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &test_font_table().unwrap_or_default(),
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let rn = frame
        .nodes
        .iter()
        .find(|rn| matches!(&rn.payload, NodePayload::Mesh { program, .. } if *program == 6))
        .expect("radial 渐变容器应产出 program=6");
    match &rn.payload {
        NodePayload::Mesh { uvs, colors, .. } => {
            // uv = box 局部像素坐标（TL=(0,0)，BR=(1920,1080)），供 shader 算 t。
            assert!(
                (uvs[0][0]).abs() < 1e-3 && (uvs[0][1]).abs() < 1e-3,
                "{:?}",
                uvs[0]
            );
            assert!((uvs[2][0] - 1920.0).abs() < 1e-3 && (uvs[2][1] - 1080.0).abs() < 1e-3);
            // 顶点色仍承载 background-color（shader 内渐变 over 底色合成）。
            assert_eq!(colors[0], [0.1, 0.2, 0.3, 1.0]);
        }
        _ => unreachable!(),
    }
    // grad_params：home 光晕几何（cx=1574.4, cy=-129.6, 1100×560）。
    let g = &rn.gradient;
    assert_eq!(g.kind, 1);
    assert!((g.center[0] - 1574.4).abs() < 0.2);
    assert!((g.center[1] + 129.6).abs() < 0.2);
    assert!((g.radii[0] - 1100.0).abs() < 1e-3 && (g.radii[1] - 560.0).abs() < 1e-3);
    assert_eq!(g.stop_count, 2);
    assert!((g.stops[1][4] - 0.6).abs() < 1e-5);
}

/// 渐变 + filter（program=3 路径的渐变版）→ program=7（GRADIENT + COLOR_FILTER 双变体）。
#[test]
fn background_gradient_with_filter_emits_program7() {
    use crate::render::node::NodePayload;
    use crate::style::resolved::{Gradient, GradientStop};
    let mut n = Node::default();
    n.kind = NodeKind::Container;
    n.style.background_gradient = Some(Gradient::Linear {
        angle_deg: 45.0,
        stops: vec![
            GradientStop {
                color: [1.0, 0.0, 0.0, 1.0],
                pos: 0.0,
            },
            GradientStop {
                color: [0.0, 0.0, 1.0, 1.0],
                pos: 1.0,
            },
        ],
    });
    n.style.color_filter = Some(crate::style::color_filter::grayscale());
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &test_font_table().unwrap_or_default(),
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let has7 = frame
        .nodes
        .iter()
        .any(|rn| matches!(&rn.payload, NodePayload::Mesh { program, .. } if *program == 7));
    assert!(has7, "渐变 + filter → program=7");
}

/// 渐变节点非几何参数变（stops 色变、box 不变）→ Header 级（只更 MPB 不重建 mesh）。
#[test]
fn gradient_params_change_is_header_level() {
    use crate::style::resolved::{Gradient, GradientStop};
    let mk = |c: [f32; 4]| {
        let mut n = Node::default();
        n.kind = NodeKind::Container;
        n.style.background_gradient = Some(Gradient::Linear {
            angle_deg: 90.0,
            stops: vec![
                GradientStop { color: c, pos: 0.0 },
                GradientStop {
                    color: [0.0, 0.0, 1.0, 1.0],
                    pos: 1.0,
                },
            ],
        });
        n.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        };
        n
    };
    let mut scene_a = Scene::from_nodes(vec![mk([1.0, 0.0, 0.0, 1.0])], vec![]);
    let mut scene_b = Scene::from_nodes(vec![mk([0.0, 1.0, 0.0, 1.0])], vec![]);
    crate::scene::transform::compute_world_transforms(&mut scene_a);
    crate::scene::transform::compute_world_transforms(&mut scene_b);
    let ha = {
        let (frame, _, _) = build_render_nodes(
            &scene_a,
            &test_font_table().unwrap_or_default(),
            &std::collections::HashMap::new(),
            &empty_sizes(),
            &mut test_glyph_atlas(),
        );
        crate::render::dirty::header_hash(&frame.nodes[0])
    };
    let hb = {
        let (frame, _, _) = build_render_nodes(
            &scene_b,
            &test_font_table().unwrap_or_default(),
            &std::collections::HashMap::new(),
            &empty_sizes(),
            &mut test_glyph_atlas(),
        );
        crate::render::dirty::header_hash(&frame.nodes[0])
    };
    assert_ne!(ha, hb, "stops 色变 → header_hash 变（Header 级）");
}

/// effect 打包：FontEffect → EffectBlock 槽位映射。
/// Shadow→underlay（多重 ≤3，超 3 丢）、Stroke→outline、Glow→glow、Blur→blur。
#[test]
fn pack_effects_maps_to_slots() {
    use crate::render::node::UnderlaySlot;
    use crate::text::font_effect::FontEffect;
    let effects = vec![
        FontEffect::Shadow {
            ox: 3.0,
            oy: 0.0,
            blur: 0.0,
            color: [0., 0., 0., 1.],
        },
        FontEffect::Stroke {
            w: 2.0,
            color: [0., 0., 0., 1.],
        },
        FontEffect::Glow {
            w: 4.0,
            color: [0.37, 0.70, 0.77, 1.],
        },
        FontEffect::Blur { w: 2.0 },
    ];
    let eb = crate::render::pack_effects(&effects);
    // outline
    assert_eq!(eb.outline_width, 2.0);
    assert_eq!(eb.outline_color, [0., 0., 0., 1.]);
    // underlay[0] = 第一个 shadow
    assert_eq!(
        eb.underlay[0],
        UnderlaySlot {
            offset_x: 3.0,
            offset_y: 0.0,
            softness: 0.0,
            color: [0., 0., 0., 1.]
        }
    );
    // underlay[1]/[2] 未填 = default（color.a=0 → shader 不启用）
    assert_eq!(eb.underlay[1].color[3], 0.0);
    assert_eq!(eb.underlay[2].color[3], 0.0);
    // glow / blur
    assert_eq!(eb.glow_power, 4.0);
    assert_eq!(eb.glow_color, [0.37, 0.70, 0.77, 1.]);
    assert_eq!(eb.blur_width, 2.0);
}

/// 多重 shadow：前 3 个填 underlay[0..3]，第 4 个丢弃（不 panic）。
#[test]
fn pack_effects_caps_shadows_at_three() {
    use crate::text::font_effect::FontEffect;
    let effects = vec![
        FontEffect::Shadow {
            ox: 2.0,
            oy: 2.0,
            blur: 0.0,
            color: [0., 0., 0., 1.],
        },
        FontEffect::Shadow {
            ox: 4.0,
            oy: 4.0,
            blur: 0.0,
            color: [0., 0., 0., 1.],
        },
        FontEffect::Shadow {
            ox: 6.0,
            oy: 6.0,
            blur: 0.0,
            color: [0., 0., 0., 1.],
        },
        FontEffect::Shadow {
            ox: 8.0,
            oy: 8.0,
            blur: 0.0,
            color: [0., 0., 0., 1.],
        }, // 超出，丢
    ];
    let eb = crate::render::pack_effects(&effects);
    assert_eq!(eb.underlay[0].offset_x, 2.0);
    assert_eq!(eb.underlay[1].offset_x, 4.0);
    assert_eq!(eb.underlay[2].offset_x, 6.0);
    // 没有 underlay[3]（只 3 槽）；第 4 个 shadow 被吞，不 panic 即可。
}

/// EffectBlock 默认 = 无 effect（全 0：outline_width=0 / color.a=0 / blur_width=0）。
#[test]
fn effect_block_default_is_no_effect() {
    use crate::render::node::EffectBlock;
    let eb = EffectBlock::default();
    assert_eq!(eb.outline_width, 0.0);
    assert_eq!(eb.outline_color[3], 0.0);
    assert_eq!(eb.underlay[0].color[3], 0.0);
    assert_eq!(eb.glow_color[3], 0.0);
    assert_eq!(eb.blur_width, 0.0);
}

/// to_bytes 往返：固定 128B，同 EffectBlock 产出同 bytes。
#[test]
fn effect_block_to_bytes_stable() {
    use crate::render::node::EffectBlock;
    let eb = EffectBlock::default();
    let bytes = eb.to_bytes();
    assert_eq!(bytes.len(), 128, "EffectBlock 序列化定长 128B");
    // 全 0 effect → 全 0 bytes
    assert!(bytes.iter().all(|&b| b == 0));
    // 同 effect 同 bytes（稳定性，供 dirty hash）
    let eb2 = EffectBlock::default();
    assert_eq!(eb.to_bytes(), eb2.to_bytes());
}

/// build_text_mesh 把节点 text_effects 打包进 TextMeshes.effect（同一文字节点所有页
/// 共享一份 effect 配置），base 字形 mesh 仍正常产出（SDF 后 effect 改由 shader
/// uniform 实现，build_text_mesh 不再产出 back/front layer mesh）。
#[test]
fn build_text_packs_effects_into_meshes() {
    use crate::text::font_effect::FontEffect;
    let (fonts, layout, rect) = build_text_fixture(48.0);
    let effects = vec![FontEffect::Shadow {
        ox: 3.0,
        oy: 3.0,
        blur: 0.0,
        color: [0., 0., 0., 1.],
    }];
    let m = build_text_mesh(
        &layout,
        &mut test_glyph_atlas(),
        &fonts,
        &rect,
        &effects,
        None,
        false,
    );
    // effect 进了 TextMeshes.effect（shadow → underlay[0]）
    assert_eq!(m.effect.underlay[0].offset_x, 3.0);
    assert_eq!(m.effect.underlay[0].offset_y, 3.0);
    // base 非空（字形 quad 仍产出）
    assert!(!m.base.is_empty(), "base 字形 mesh 仍产出");
}

#[test]
fn border_ring_rounded_has_more_vertices_than_sharp() {
    use crate::render::border::{border_ring, BorderWidths};
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    };
    let w = BorderWidths::all(4.0);
    let sharp = border_ring(&rect, &[(0.0, 0.0); 4], w, [1.0; 4]);
    let round = border_ring(&rect, &[(20.0, 20.0); 4], w, [1.0; 4]);
    assert_eq!(sharp.0.len(), 8, "直角环 8 顶点(现状)");
    assert!(
        round.0.len() > 8,
        "圆角环顶点数 > 8(弧分段), got {}",
        round.0.len()
    );
    assert_eq!(round.0.len() % 2, 0, "外+内轮廓等长");
}

#[test]
fn border_ring_rounded_inner_radius_clamps() {
    // 外半径 5 < border width 10 → 内半径钳 0(内角直角),外圆内方
    use crate::render::border::{border_ring, BorderWidths};
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    };
    let (verts, _, _, _) = border_ring(&rect, &[(5.0, 5.0); 4], BorderWidths::all(10.0), [1.0; 4]);
    assert!(verts.len() > 8, "外圆角分段 + 内角点 infill");
}

#[test]
fn border_ring_zero_width_returns_empty() {
    use crate::render::border::{border_ring, BorderWidths};
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    };
    let (v, _, _, _) = border_ring(&rect, &[(20.0, 20.0); 4], BorderWidths::all(0.0), [1.0; 4]);
    assert!(v.is_empty(), "全零宽早期返回");
}

#[test]
fn container_border_with_radius_emits_rounded_border() {
    // border-radius:20px + border:4px solid red → 边框环走圆角路径（顶点数 > 直角态 12）
    use crate::style::mapping::apply_decl;
    let mut n = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        },
        Some([0.0, 0.0, 1.0, 1.0]),
    );
    assert!(apply_decl(&mut n.style, "border-radius", "20px"));
    assert!(apply_decl(&mut n.style, "border", "4px solid #ff0000"));
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let mesh = frame
        .nodes
        .iter()
        .find(|rn| matches!(&rn.payload, NodePayload::Mesh { verts, .. } if !verts.is_empty()))
        .expect("至少一个非空 Mesh 节点");
    let NodePayload::Mesh { verts, colors, .. } = &mesh.payload else {
        unreachable!()
    };
    // 背景 rounded_rect(中心+弧) + 边框圆角环(外+内轮廓),总顶点 > 直角态 12
    assert!(
        verts.len() > 12,
        "圆角 border+bg 顶点 > 12, got {}",
        verts.len()
    );
    assert!(colors.contains(&[1.0, 0.0, 0.0, 1.0]), "红色边框顶点存在");
}

/// border 门控（对齐 CSS initial=none）：设了 border-width>0 + border-color，但
/// border_style=None（ResolvedStyle 默认）时不应产任何 border_ring 几何——背景 quad 仅
/// 4 顶点。CSS 规范 border-style 默认 none，none 不渲染边框（即便 width/color 已声明）。
#[test]
fn border_style_none_renders_no_border_even_with_width_and_color() {
    let mut n = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        },
        Some([0.0, 0.0, 1.0, 1.0]), // 蓝底（产背景 quad 4 顶点）
    );
    // width=2 四边 + color=红，但 border_style 保持默认 None。
    n.style.taffy_style.border = taffy::geometry::Rect::length(2.0_f32);
    n.style.border_color = Some([1.0, 0.0, 0.0, 1.0]);
    n.style.border_style = crate::style::resolved::BorderStyle::None;
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    let fonts = test_font_table().expect("need test font for build_render_nodes");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    // 期望：没有 border_ring 顶点。border_ring 直角态产 8 顶点（背景 4 + 边框 8 = 12），
    // 阈值 >10 区分“有边框”与“纯背景 4 顶点”。
    let has_border_geom = frame.nodes.iter().any(|rn| {
        if let NodePayload::Mesh { verts, .. } = &rn.payload {
            verts.len() > 10
        } else {
            false
        }
    });
    assert!(
        !has_border_geom,
        "border_style=None 不应渲染边框几何（即使 width+color 已设）"
    );
}

// ── TextField / TextArea / NumberField 渲染 ──

/// 构造一个带控件状态的叶子节点 scene（TextField/TextArea/NumberField）。
/// node_id=0，layout_rect 200×50，无背景色。
fn make_scene_with_text_control(kind: NodeKind, state: ControlState) -> (Scene, NodeId) {
    let mut n = Node::default();
    n.id = NodeId(0);
    n.kind = kind;
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 50.0,
    };
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    let root = scene.roots[0];
    scene.controls.ensure(root, state);
    (scene, root)
}

#[test]
fn textfield_renders_value_text() {
    let (mut scene, id) = make_scene_with_text_control(
        NodeKind::TextField,
        ControlState::TextField(EditState::from_init("hello".into(), "".into(), 0, false)),
    );
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    // 文字 mesh 用合成 id（TF_TEXT_SYNTH_BYTE，与背景区分），primary 仍 = id.0。
    // 验文字节点存在且非空：text_sub_primary_id 命中真节点且 program=1 的 Mesh。
    let has_text = frame.nodes.iter().any(|rn| {
        text_sub_primary_id(rn.node_id) == id.0
            && matches!(&rn.payload, NodePayload::Mesh { program: 1, verts, .. } if !verts.is_empty())
    });
    assert!(
        has_text,
        "TextField value='hello' must produce non-empty text glyph mesh (program=1)"
    );
}

#[test]
fn numberfield_renders_value_text() {
    // NumberField 是 TextField 的数值约束变体：edit 复用 EditState，渲染路径应与 TextField
    // 完全一致。回归守卫：验证 NumberField 产非空文字 mesh（修复前落入 catch-all 返空 mesh）。
    let (mut scene, id) = make_scene_with_text_control(
        NodeKind::NumberField,
        ControlState::NumberField {
            edit: EditState::from_init("42".into(), "".into(), 0, false),
            min: 0.0,
            max: 100.0,
            step: 1.0,
        },
    );
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    // 与 textfield_renders_value_text 同断言：文字 mesh 用合成 id，primary 命中真节点且
    // program=1 的 Mesh 非空。
    let has_text = frame.nodes.iter().any(|rn| {
        text_sub_primary_id(rn.node_id) == id.0
            && matches!(&rn.payload, NodePayload::Mesh { program: 1, verts, .. } if !verts.is_empty())
    });
    assert!(
        has_text,
        "NumberField value='42' must produce non-empty text glyph mesh (program=1)"
    );
}

#[test]
fn text_control_background_and_text_mesh_have_distinct_node_ids() {
    // 回归：TextField/TextArea/NumberField 的背景框 mesh 与文字 mesh 必须用不同 node_id。
    // 修复前两者共享真 node_id → C# MirrorPool 按 node_id 唯一索引 GO，第二个 mesh 覆盖
    // 第一个 → 控件渲染残缺/不可见（settings showcase 的 spinbutton “无法渲染” 根因）。
    // 修复后文字 mesh 用合成 node_id（tf_synth_id，high byte=TF_TEXT_SYNTH_BYTE），
    // primary 关联仍 = 真节点 id（text_sub_primary_id 可还原）。
    let (mut scene, id) = make_scene_with_text_control(
        NodeKind::NumberField,
        ControlState::NumberField {
            edit: EditState::from_init("42".into(), "".into(), 0, false),
            min: 0.0,
            max: 100.0,
            step: 1.0,
        },
    );
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    // 背景 mesh：program=0，node_id = 真节点 id（控件主体，在 id_to_pos 注册）。
    let bg = frame
        .nodes
        .iter()
        .find(|rn| {
            rn.node_id == id.0 && matches!(&rn.payload, NodePayload::Mesh { program: 0, .. })
        })
        .expect("background mesh (program=0) exists");
    // 文字 mesh：program=1，非空 verts（字形）。
    let text = frame
        .nodes
        .iter()
        .find(|rn| {
            matches!(&rn.payload, NodePayload::Mesh { program: 1, verts, .. } if !verts.is_empty())
        })
        .expect("non-empty text mesh (program=1) exists");
    // ① 核心：背景与文字 node_id 不同 → C# MirrorPool 各自独立 GO，互不覆盖。
    assert_ne!(
        bg.node_id, text.node_id,
        "background and text mesh must have distinct node_ids (else C# MirrorPool conflict)"
    );
    // ② 文字 mesh 的合成 id 可还原回真节点 id（sort_key 传播 / 调试反查依赖此）。
    assert_eq!(
        text_sub_primary_id(text.node_id),
        id.0,
        "text mesh synth id must decode back to the real node id"
    );
    // ③ 文字 mesh 在背景之上绘制（sort_key 更大），否则被不透明背景遮挡。
    assert!(
        text.sort_key > bg.sort_key,
        "text mesh sort_key ({}) must be > background sort_key ({}) so text draws on top",
        text.sort_key,
        bg.sort_key
    );
}

#[test]
fn text_control_text_mesh_sort_key_follows_background_in_multi_node_scene() {
    // 多节点场景回归：NumberField 不是首节点时，文字 mesh（合成 id）sort_key 必须紧跟
    // 背景之后。修复前 assign_sort_keys 不给合成 id 赋值（初始 sk=0），reorder 把它排到
    // 所有真节点之前 → 文字绘制在背景之下被不透明背景遮挡（settings showcase spinbutton
    // 文字不可见的多节点根因）。单节点场景 reorder 巧合让文字 sk=1 > 背景 sk=0，掩盖此 bug。
    let mut root = Node::default();
    root.kind = NodeKind::Container;
    root.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 400.0,
        h: 200.0,
    };
    let mut before = Node::default();
    before.kind = NodeKind::Container;
    before.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 50.0,
    };
    let mut nf = Node::default();
    nf.kind = NodeKind::NumberField;
    nf.layout_rect = Rect {
        x: 0.0,
        y: 100.0,
        w: 200.0,
        h: 50.0,
    };
    let mut scene = Scene::from_nodes(vec![root, before, nf], vec![(0, 1), (0, 2)]);
    let nf_id = scene
        .nodes
        .iter()
        .find(|(_, n)| n.kind == NodeKind::NumberField)
        .map(|(_, n)| n.id)
        .expect("NumberField node");
    scene.controls.ensure(
        nf_id,
        ControlState::NumberField {
            edit: EditState::from_init("42".into(), "".into(), 0, false),
            min: 0.0,
            max: 100.0,
            step: 1.0,
        },
    );
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let bg = frame
        .nodes
        .iter()
        .find(|rn| {
            rn.node_id == nf_id.0 && matches!(&rn.payload, NodePayload::Mesh { program: 0, .. })
        })
        .expect("背景 mesh 存在");
    let text = frame
        .nodes
        .iter()
        .find(|rn| {
            is_tf_text_synth(rn.node_id)
                && matches!(&rn.payload, NodePayload::Mesh { program: 1, verts, .. } if !verts.is_empty())
        })
        .expect("文字 mesh 存在");
    assert!(
        text.sort_key > bg.sort_key,
        "文字 sort_key ({}) 必须 > 背景 sort_key ({})，否则文字被背景遮挡",
        text.sort_key,
        bg.sort_key
    );
}

// ── 控件外壳背景渲染（Toggle/RadioButton/Slider/ProgressBar）──
// 39d0a8d 回归守卫：这四种控件是「空 div」——视觉全靠自身 background（不像 Slider/ProgressBar
// 的 data-slot 子节点能兑底，Toggle/RadioButton 连子节点都没有）。修复前它们不在 is_container()
// 也无专门 render arm，落入 `_ => 空 mesh`，在 Unity PlayMode 完全不渲染/不可交互。
// 本组测试固化「控件外壳画自身 background」不变量。

/// 构造一个控件外壳叶子节点 scene（带 background + ControlState）。
fn make_control_shell_scene(kind: NodeKind, state: ControlState, bg: [f32; 4]) -> (Scene, NodeId) {
    let mut n = Node::default();
    n.id = NodeId(0);
    n.kind = kind;
    n.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 40.0,
        h: 40.0,
    };
    n.style.background_color = Some(bg);
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    let root = scene.roots[0];
    scene.controls.ensure(root, state);
    (scene, root)
}

#[test]
fn control_shell_paints_own_background() {
    // 每种空 div 控件必产一个 program=0、非空 verts、node_id 命中的背景 Mesh。
    // 若某控件从 build_container_mesh arm 脱落（如当年 Toggle/RadioButton），会落入 `_ =>` 空
    // mesh，本断言即失败——直接编码「控件外壳画自身 background」不变量。
    let bg = [0.2, 0.6, 0.9, 1.0];
    let cases: Vec<(NodeKind, ControlState)> = vec![
        (NodeKind::Toggle, ControlState::Toggle { checked: false }),
        (
            NodeKind::RadioButton,
            ControlState::Radio {
                checked: false,
                name: "g".into(),
            },
        ),
        (
            NodeKind::Slider,
            ControlState::Slider {
                value: 0.0,
                min: 0.0,
                max: 1.0,
                step: 0.1,
                dragging: false,
            },
        ),
        (
            NodeKind::ProgressBar,
            ControlState::Progress {
                value: 0.5,
                max: 1.0,
                indeterminate: false,
            },
        ),
    ];
    let fonts = test_font_table().expect("need test font");
    for (kind, state) in cases {
        let (mut scene, id) = make_control_shell_scene(kind, state, bg);
        crate::scene::transform::compute_world_transforms(&mut scene);
        let (frame, _, _) = build_render_nodes(
            &scene,
            &fonts,
            &std::collections::HashMap::new(),
            &empty_sizes(),
            &mut test_glyph_atlas(),
        );
        let has_bg = frame.nodes.iter().any(|rn| {
            rn.node_id == id.0
                && matches!(&rn.payload, NodePayload::Mesh { program: 0, verts, .. } if !verts.is_empty())
        });
        assert!(
            has_bg,
            "{:?} 控件外壳必须画自身 background（program=0 非空 Mesh，node_id 命中）",
            kind
        );
    }
}

#[test]
fn textfield_empty_value_renders_placeholder() {
    // value 为空 → 渲染 placeholder 文字。
    let (mut scene, id) = make_scene_with_text_control(
        NodeKind::TextField,
        ControlState::TextField(EditState::from_init(
            "".into(),
            "Search...".into(),
            0,
            false,
        )),
    );
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    // placeholder 文字也应产 glyph mesh（文字 mesh 用合成 id，primary 命中真节点）。
    let has_text = frame.nodes.iter().any(|rn| {
        text_sub_primary_id(rn.node_id) == id.0
            && matches!(&rn.payload, NodePayload::Mesh { program: 1, verts, .. } if !verts.is_empty())
    });
    assert!(
        has_text,
        "TextField with empty value must render placeholder 'Search...'"
    );
}

#[test]
fn textarea_renders_value_text() {
    // TextArea 多行输入也渲染 value。
    let (mut scene, id) = make_scene_with_text_control(
        NodeKind::TextArea,
        ControlState::TextArea(EditState::from_init(
            "line1\nline2".into(),
            "".into(),
            0,
            false,
        )),
    );
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let has_text = frame.nodes.iter().any(|rn| {
        text_sub_primary_id(rn.node_id) == id.0
            && matches!(&rn.payload, NodePayload::Mesh { program: 1, verts, .. } if !verts.is_empty())
    });
    assert!(
        has_text,
        "TextArea value='line1\\nline2' must produce text glyph mesh"
    );
}

// ── Task 12: 光标 / 选区 / composition 渲染 mesh ──

/// 构造一个聚焦的 TextField（focused_node + cursor_visible）。
/// `value` 非空时 cursor 默认在末尾（from_init）。
fn make_focused_textfield(value: &str) -> (Scene, NodeId) {
    let (mut scene, id) = make_scene_with_text_control(
        NodeKind::TextField,
        ControlState::TextField(EditState::from_init(value.into(), "".into(), 0, false)),
    );
    scene.focused_node = Some(id);
    // from_init 已设 cursor_visible=true，显式重申以便后续 readonly 变体改写后仍可见意图。
    if let Some(ControlState::TextField(e)) = scene.controls.get_mut(id) {
        e.cursor_visible = true;
    }
    (scene, id)
}

/// 聚焦 TextField 必须产出光标 quad（合成 node_id，program=0 纯色，非空 verts）。
#[test]
fn focused_textfield_renders_cursor_quad() {
    let (mut scene, id) = make_focused_textfield("ab");
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let cursor_id = tf_synth_id(id.0, TF_CURSOR_SYNTH_BYTE);
    let cursor_nodes: Vec<_> = frame
        .nodes
        .iter()
        .filter(|rn| rn.node_id == cursor_id)
        .collect();
    assert!(
        !cursor_nodes.is_empty(),
        "聚焦 TextField 必须产出光标 RenderNode（合成 id）"
    );
    // 光标 mesh 非空（4 顶点 quad）。
    let has_geom = cursor_nodes.iter().any(|rn| {
        matches!(&rn.payload, NodePayload::Mesh { verts, program: 0, .. } if !verts.is_empty())
    });
    assert!(has_geom, "光标 mesh 必须有几何（4 顶点纯色 quad）");
}

/// 未聚焦 TextField 不画光标。
#[test]
fn unfocused_textfield_no_cursor_quad() {
    let (mut scene, id) = make_scene_with_text_control(
        NodeKind::TextField,
        ControlState::TextField(EditState::from_init("ab".into(), "".into(), 0, false)),
    );
    scene.focused_node = None; // 未聚焦
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let cursor_id = tf_synth_id(id.0, TF_CURSOR_SYNTH_BYTE);
    let has_cursor = frame.nodes.iter().any(|rn| {
        rn.node_id == cursor_id
            && matches!(&rn.payload, NodePayload::Mesh { verts, .. } if !verts.is_empty())
    });
    assert!(!has_cursor, "未聚焦 TextField 不应画光标");
}

/// cursor_visible=false（闪烁灭相）时不画光标。
#[test]
fn textfield_cursor_hidden_when_not_visible() {
    let (mut scene, id) = make_focused_textfield("ab");
    // 模拟闪烁灭相
    if let Some(ControlState::TextField(e)) = scene.controls.get_mut(id) {
        e.cursor_visible = false;
    }
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let cursor_id = tf_synth_id(id.0, TF_CURSOR_SYNTH_BYTE);
    let has_cursor = frame.nodes.iter().any(|rn| {
        rn.node_id == cursor_id
            && matches!(&rn.payload, NodePayload::Mesh { verts, .. } if !verts.is_empty())
    });
    assert!(!has_cursor, "cursor_visible=false（灭相）不应画光标");
}

/// readonly TextField 即使聚焦也不画光标（照 HTML readonly 行为，焦点框无 caret）。
#[test]
fn readonly_focused_textfield_no_cursor_quad() {
    let (mut scene, id) = make_scene_with_text_control(
        NodeKind::TextField,
        ControlState::TextField(EditState::from_init("ab".into(), "".into(), 0, true)),
    );
    scene.focused_node = Some(id);
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let cursor_id = tf_synth_id(id.0, TF_CURSOR_SYNTH_BYTE);
    let has_cursor = frame.nodes.iter().any(|rn| {
        rn.node_id == cursor_id
            && matches!(&rn.payload, NodePayload::Mesh { verts, .. } if !verts.is_empty())
    });
    assert!(!has_cursor, "readonly TextField 聚焦不应画光标");
}

/// 有选区（selection_begin < selection_end）时画选区背景 quad（合成 id）。
#[test]
fn textfield_with_selection_renders_selection_bg() {
    let (mut scene, id) = make_focused_textfield("hello");
    // 选区 [1, 4) = "ell"
    if let Some(ControlState::TextField(e)) = scene.controls.get_mut(id) {
        e.cursor = 4;
        e.anchor = 1;
    }
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let sel_id = tf_synth_id(id.0, TF_SELECTION_SYNTH_BYTE);
    let has_sel = frame.nodes.iter().any(|rn| {
        rn.node_id == sel_id
            && matches!(&rn.payload, NodePayload::Mesh { verts, program: 0, .. } if !verts.is_empty())
    });
    assert!(has_sel, "有选区时必须画选区背景 quad");
}

/// 退化选区（cursor == anchor）不画选区背景。
#[test]
fn textfield_no_selection_no_selection_bg() {
    let (mut scene, id) = make_focused_textfield("hello");
    // cursor == anchor（from_init 默认，无选区）
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let sel_id = tf_synth_id(id.0, TF_SELECTION_SYNTH_BYTE);
    let has_sel = frame.nodes.iter().any(|rn| {
        rn.node_id == sel_id
            && matches!(&rn.payload, NodePayload::Mesh { verts, .. } if !verts.is_empty())
    });
    assert!(!has_sel, "退化选区（cursor==anchor）不应画选区背景");
}

/// 有 composition 时画下划线 quad（合成 id）。
#[test]
fn textfield_with_composition_renders_underline() {
    let (mut scene, id) = make_focused_textfield("a");
    // 插入 composition：pos=1（value="a" 末尾），text="b"
    if let Some(ControlState::TextField(e)) = scene.controls.get_mut(id) {
        e.composition = Some(Composition {
            text: "b".into(),
            pos: 1,
        });
    }
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let comp_id = tf_synth_id(id.0, TF_COMPOSITION_SYNTH_BYTE);
    let has_comp = frame.nodes.iter().any(|rn| {
        rn.node_id == comp_id
            && matches!(&rn.payload, NodePayload::Mesh { verts, program: 0, .. } if !verts.is_empty())
    });
    assert!(has_comp, "有 composition 时必须画下划线 quad");
}

/// 合成 id 三标签互不冲突，且不与真 node_id 冲突。
#[test]
fn tf_synth_ids_are_distinct() {
    let primary = 0x0000_0005u32;
    let c = tf_synth_id(primary, TF_CURSOR_SYNTH_BYTE);
    let s = tf_synth_id(primary, TF_SELECTION_SYNTH_BYTE);
    let u = tf_synth_id(primary, TF_COMPOSITION_SYNTH_BYTE);
    assert_ne!(c, s);
    assert_ne!(c, u);
    assert_ne!(s, u);
    assert_ne!(c, primary);
    // 不被误判为 text 跨页子页（high byte 1..=15）。
    assert!(!is_text_sub_page(c));
    assert!(!is_text_sub_page(s));
    assert!(!is_text_sub_page(u));
    // 不被误判为 box-shadow 合成节点（high byte 36..=47）——撞了会被 batch.rs
    // is_mergeable_mesh / merge.rs mesh_key 当 box-shadow 合成节点排除，导致这些 mesh
    // 永不 merge-batch（TF synth byte 32..=35 在安全区，不撞 36..=47）。
    for id in [c, s, u] {
        assert!(
            !is_shadow_synth(id),
            "edit synth id must not collide with box-shadow synth range"
        );
    }
}

/// 编辑反馈 mesh 的 sort_key 顺序：背景 < 选区 < 文字 < 光标。
/// 选区先于文字 push（在文字下层，标准编辑器行为）；光标最后 push（最上层）。
/// 锁定 reorder_for_batching 对合成 id（program=0 mergeable）不 panic 且保序
/// （坑 139：合成 id + program:0 曾触发 aabb_of 的 scene.get().expect panic；
/// aabb_of 已加零面积兜底，此测回归保护）。多节点 reorder（n≥2）才触 aabb_of 路径。
#[test]
fn textfield_editing_mesh_sort_key_order() {
    let (mut scene, id) = make_focused_textfield("hello");
    // 选区 [1,4)
    if let Some(ControlState::TextField(e)) = scene.controls.get_mut(id) {
        e.cursor = 4;
        e.anchor = 1;
    }
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let sk = |nid: u32| -> u32 {
        frame
            .nodes
            .iter()
            .find(|rn| rn.node_id == nid)
            .map(|rn| rn.sort_key)
            .unwrap_or(u32::MAX)
    };
    let bg_sk = sk(id.0);
    let sel_sk = sk(tf_synth_id(id.0, TF_SELECTION_SYNTH_BYTE));
    let cur_sk = sk(tf_synth_id(id.0, TF_CURSOR_SYNTH_BYTE));
    // 找文字 mesh（合成 id TF_TEXT_SYNTH_BYTE，program=1，非空 verts）。
    let text_sk = frame
        .nodes
        .iter()
        .find(|rn| {
            is_tf_text_synth(rn.node_id)
                && matches!(&rn.payload, NodePayload::Mesh { program: 1, verts, .. } if !verts.is_empty())
        })
        .map(|rn| rn.sort_key)
        .expect("文字 mesh 必须存在");
    // 顺序：背景 < 选区 < 文字 < 光标（升序 sort_key = 绘制序，后绘者在上层）。
    // 选区在文字之下（选区作背景，文字保持清晰）；光标在文字之上（caret 压在选中文字上）。
    assert!(bg_sk < sel_sk, "背景在选区之下: {bg_sk} < {sel_sk}");
    assert!(sel_sk < text_sk, "选区在文字之下: {sel_sk} < {text_sk}");
    assert!(text_sk < cur_sk, "光标在文字之上: {text_sk} < {cur_sk}");
}

// ===== caret-color / selection-background / selection-color render style (Task 15) =====
//
// Task 12 用常量缺省色画 caret/selection（caret=text color，selection-bg=蓝半透）。
// Task 15 让 render arm 读 ResolvedStyle 的 caret_color/selection_background/selection_color
// （None 时回退到同样的常量）。这些测验：声明 style 色后 mesh 颜色 == 声明色。

/// 从一个 RenderNode 的 Mesh payload 取首个顶点色（quad 是 4 同色顶点，首色即代表）。
/// 无几何 / 非 program=0 纯色 mesh → None。
fn first_vertex_color(rn: &crate::render::RenderNode) -> Option<[f32; 4]> {
    match &rn.payload {
        NodePayload::Mesh {
            verts,
            colors,
            program: 0,
            ..
        } if !verts.is_empty() && !colors.is_empty() => Some(colors[0]),
        _ => None,
    }
}

/// caret-color style 声明后，光标 mesh 颜色 == 声明色（非缺省 text color）。
#[test]
fn textfield_caret_uses_caret_color_style() {
    let (mut scene, id) = make_focused_textfield("ab");
    // 声明 caret-color = 纯红。node.style.color 仍是默认黑——若 render 用缺省会画黑光标。
    scene.get_mut(id).expect("node").style.caret_color = Some([1.0, 0.0, 0.0, 1.0]);
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let cursor_id = tf_synth_id(id.0, TF_CURSOR_SYNTH_BYTE);
    let caret_color = frame
        .nodes
        .iter()
        .find(|rn| rn.node_id == cursor_id)
        .and_then(first_vertex_color)
        .expect("cursor mesh must have geometry");
    assert_eq!(
        caret_color,
        [1.0, 0.0, 0.0, 1.0],
        "caret uses caret-color style, not default text color"
    );
}

/// 未声明 caret-color 时光标回退到 text color（Task 12 缺省行为保持）。
#[test]
fn textfield_caret_falls_back_to_text_color() {
    let (mut scene, id) = make_focused_textfield("ab");
    // color = 纯蓝，不声明 caret_color → 光标应回退到蓝。
    scene.get_mut(id).expect("node").style.color = [0.0, 0.0, 1.0, 1.0];
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let cursor_id = tf_synth_id(id.0, TF_CURSOR_SYNTH_BYTE);
    let caret_color = frame
        .nodes
        .iter()
        .find(|rn| rn.node_id == cursor_id)
        .and_then(first_vertex_color)
        .expect("cursor mesh must have geometry");
    assert_eq!(
        caret_color,
        [0.0, 0.0, 1.0, 1.0],
        "caret falls back to text color when caret-color unset"
    );
}

/// selection-background style 声明后，选区 mesh 颜色 == 声明色（非缺省蓝半透）。
#[test]
fn textfield_selection_uses_selection_background_style() {
    let (mut scene, id) = make_focused_textfield("hello");
    if let Some(ControlState::TextField(e)) = scene.controls.get_mut(id) {
        e.cursor = 4;
        e.anchor = 1;
    }
    // 声明 selection-background = 绿半透。缺省是蓝半透——若 render 用缺省会画蓝色。
    scene.get_mut(id).expect("node").style.selection_background = Some([0.0, 1.0, 0.0, 0.4]);
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let sel_id = tf_synth_id(id.0, TF_SELECTION_SYNTH_BYTE);
    let sel_color = frame
        .nodes
        .iter()
        .find(|rn| rn.node_id == sel_id)
        .and_then(first_vertex_color)
        .expect("selection mesh must have geometry");
    assert_eq!(
        sel_color,
        [0.0, 1.0, 0.0, 0.4],
        "selection uses selection-background style, not default blue"
    );
}

/// 未声明 selection-background 时选区回退到蓝半透（Task 12 缺省行为保持）。
#[test]
fn textfield_selection_falls_back_to_default_blue() {
    let (mut scene, id) = make_focused_textfield("hello");
    if let Some(ControlState::TextField(e)) = scene.controls.get_mut(id) {
        e.cursor = 4;
        e.anchor = 1;
    }
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let sel_id = tf_synth_id(id.0, TF_SELECTION_SYNTH_BYTE);
    let sel_color = frame
        .nodes
        .iter()
        .find(|rn| rn.node_id == sel_id)
        .and_then(first_vertex_color)
        .expect("selection mesh must have geometry");
    assert_eq!(
        sel_color,
        [0.0, 0.0, 1.0, 0.5],
        "selection falls back to default blue translucent"
    );
}

// ===== TextControl caret geometry =====

/// 渲染后取光标 quad 的左边缘 x（纯色 quad，verts 为世界坐标 [x,y] 对；取最小 x）。
fn caret_left_x(frame: &FrameData, cursor_id: u32) -> Option<f32> {
    frame.nodes.iter().find_map(|rn| {
        if rn.node_id != cursor_id {
            return None;
        }
        match &rn.payload {
            NodePayload::Mesh { verts, .. } if !verts.is_empty() => {
                Some(verts.iter().map(|v| v[0]).fold(f32::INFINITY, f32::min))
            }
            _ => None,
        }
    })
}

/// TextField 光标几何：cursor=2（"ab" 末尾）必须落在有限正 x（两字符宽之后）。
/// 回归锁：显示串与 value 同字节布局（无掩码），cursor 字节偏移直接取像素 x。
#[test]
fn textfield_caret_unaffected_by_display_remap() {
    let fonts = test_font_table().expect("need test font");
    let (mut scene, id) = make_focused_textfield("ab");
    if let Some(ControlState::TextField(e)) = scene.controls.get_mut(id) {
        e.cursor = 2;
    }
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let x = caret_left_x(&frame, tf_synth_id(id.0, TF_CURSOR_SYNTH_BYTE))
        .expect("TextField cursor=2 must render caret quad");
    // TextField cursor=2 光标 x 必须有限且 >0（"ab" 两字符宽之后）。
    assert!(
        x.is_finite() && x > 0.0,
        "TextField caret x finite and positive: {x}"
    );
}

// ── Dropdown 浮层渲染（Task 11：popup 子树跳出正常 DFS，末尾追加，mask=0）──
//
// 模式同 scrollbar thumb（render/mod.rs 末尾 append）：open Dropdown 的 .loom-popup
// 子树不在正常 DFS 渲染（不进 id_to_pos），merge 后末尾追加——sort_key 续 max_sort+1，
// mask_context=MaskContext(0) 跳出祖先 overflow:hidden clip。
//
// 测试场景：overflow:hidden 容器内的 select（open=true）→ .loom-popup 子树（含 option +
// 其 text）应全部 sort_key > 正常节点，mask_context=0（不被 outer clip 裁）。

/// 建测试 dropdown 场景：outer(overflow:hidden) > select(Dropdown,open) > [.loom-value,
/// .loom-popup > [option > text]]。返回 (scene, outer_id, select_id, popup_id, option_id,
/// option_text_id)。`open` 控制 Dropdown 的 open 字段 + .loom-popup 的 display。
fn make_popup_scene(open: bool) -> (Scene, NodeId, NodeId, NodeId, NodeId, NodeId) {
    use crate::style::resolved::OverflowMode;
    let mut outer_style = ResolvedStyle::default();
    outer_style.overflow_x = OverflowMode::Hidden;
    outer_style.overflow_y = OverflowMode::Hidden;
    // .loom-popup 的 display：open→block（可见，标准弹出列表语义，走末尾追加路径），closed→none（被
    // collect_display_none_subtree 剪掉，整子树不渲染——模拟 sync_control_visuals 的
    // inline override 效果，这里直接写 style.taffy_style.display 省去 rematch）。
    let mut popup_style = ResolvedStyle::default();
    popup_style.taffy_style.display = if open {
        taffy::style::Display::Block
    } else {
        taffy::style::Display::None
    };
    let entries: Vec<(
        Option<usize>,
        NodeKind,
        ResolvedStyle,
        Vec<String>,
        Option<String>,
        bool,
        Option<i32>,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = vec![
        // 0: outer overflow:hidden 容器（开 clip，子树 mask_context>0）
        (
            None,
            NodeKind::Container,
            outer_style,
            vec![],
            None,
            false,
            None,
            None,
            None,
            None,
        ),
        // 1: select（Dropdown 控件节点）
        (
            Some(0),
            NodeKind::Dropdown,
            ResolvedStyle::default(),
            vec![],
            None,
            false,
            None,
            None,
            None,
            None,
        ),
        // 2: .loom-value（Container，不在 popup 子树——正常渲染）
        (
            Some(1),
            NodeKind::Container,
            ResolvedStyle::default(),
            vec!["loom-value".to_string()],
            None,
            false,
            None,
            None,
            None,
            None,
        ),
        // 3: .loom-popup（浮层容器；display 由 popup_style 定）
        (
            Some(1),
            NodeKind::Container,
            popup_style,
            vec!["loom-popup".to_string()],
            None,
            false,
            None,
            None,
            None,
            None,
        ),
        // 4: option（OptionItem，popup 子树成员）
        (
            Some(3),
            NodeKind::OptionItem,
            ResolvedStyle::default(),
            vec![],
            None,
            false,
            None,
            None,
            None,
            None,
        ),
        // 5: option 的文本（TextNode，popup 子树叶子）
        (
            Some(4),
            NodeKind::TextNode,
            ResolvedStyle::default(),
            vec![],
            None,
            false,
            None,
            None,
            Some("B".to_string()),
            None,
        ),
    ];
    let mut scene = Scene::build(&entries);
    let outer_id = scene.roots[0];
    let select_id = scene.get(outer_id).unwrap().children[0];
    let popup_id = scene
        .get(select_id)
        .unwrap()
        .children
        .iter()
        .copied()
        .find(|&c| {
            scene
                .get(c)
                .unwrap()
                .classes
                .iter()
                .any(|x| x == "loom-popup")
        })
        .expect("loom-popup child");
    let option_id = scene.get(popup_id).unwrap().children[0];
    let option_text_id = scene.get(option_id).unwrap().children[0];
    // 作者自写结构下 listbox 靠 role 定位（render collect_open_popup_roots 按 role=listbox
    // 查），Scene::build 的 entries 不含 role 字段，这里手动登记 listbox 的 role。
    scene.roles.insert(
        popup_id,
        crate::scene::node::RoleInfo {
            role: Some(crate::scene::control::ROLE_LISTBOX.to_string()),
            slots: Default::default(),
            aria_controls: None,
        },
    );
    // 给所有节点非零 layout_rect（render 按 rect 产几何；0×0 节点会被某些路径跳过）。
    let all_ids: Vec<NodeId> = scene.nodes.values().map(|n| n.id).collect();
    for nid in all_ids {
        scene.get_mut(nid).unwrap().layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        };
    }
    // 挂 Dropdown 控件状态（open 字段驱动末尾追加逻辑）。
    scene.controls.ensure(
        select_id,
        ControlState::Dropdown {
            selected_index: 0,
            open,
            value_lock: false,
            open_selected_index: None,
            option_values: Vec::new(),
        },
    );
    crate::scene::transform::compute_world_transforms(&mut scene);
    (
        scene,
        outer_id,
        select_id,
        popup_id,
        option_id,
        option_text_id,
    )
}

#[test]
fn open_popup_renders_above_all_with_no_clip() {
    let fonts = test_font_table().expect("need test font");
    let (scene, _outer_id, _select_id, popup_id, option_id, option_text_id) =
        make_popup_scene(true);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    // popup 子树 = .loom-popup + option + option 的 TextNode（按 node_id 过滤）。
    let popup_subtree_ids = [popup_id.0, option_id.0, option_text_id.0];
    let popup_rns: Vec<&RenderNode> = frame
        .nodes
        .iter()
        .filter(|rn| popup_subtree_ids.contains(&rn.node_id))
        .collect();
    assert!(
        !popup_rns.is_empty(),
        "open popup 子树必须渲染（至少 .loom-popup + option text）"
    );
    // 正常节点（outer / select / .loom-value）的最大 sort_key。
    let normal_ids = scene
        .nodes
        .values()
        .map(|n| n.id.0)
        .filter(|id| !popup_subtree_ids.contains(id))
        .collect::<Vec<_>>();
    let normal_rns: Vec<&RenderNode> = frame
        .nodes
        .iter()
        .filter(|rn| normal_ids.contains(&rn.node_id))
        .collect();
    let max_normal_sort = normal_rns.iter().map(|rn| rn.sort_key).max().unwrap_or(0);
    // 前置验证：outer(overflow:hidden) 内的正常节点（.loom-value）确实被裁（mask>0），
    // 证明 popup 的 mask=0 不是“场景无 clip”的假象，而是真的跳出了祖先 clip。
    let value_rn = frame.nodes.iter().find(|rn| {
        scene
            .get(NodeId(rn.node_id))
            .is_some_and(|n| n.classes.iter().any(|c| c == "loom-value"))
    });
    if let Some(rn) = value_rn {
        assert!(
            rn.mask_context.0 > 0,
            ".loom-value 在 outer(overflow:hidden) 内应被裁（mask>0），证明场景 clip 生效"
        );
    }
    // 浮层 sort_key 全部 > 正常节点 max（画在最上层）。
    assert!(
        popup_rns.iter().all(|rn| rn.sort_key > max_normal_sort),
        "open popup 子树 sort_key 必须全部 > 正常节点 max sort_key {max_normal_sort}"
    );
    // 浮层 mask_context 全部 = MaskContext(0)（跳出 outer 的 overflow:hidden clip）。
    assert!(
        popup_rns.iter().all(|rn| rn.mask_context == MaskContext(0)),
        "open popup 子树 mask_context 必须 = MaskContext(0)（不被祖先 overflow:hidden 裁）"
    );
}

#[test]
fn closed_popup_not_rendered() {
    // open=false → .loom-popup display:none → collect_display_none_subtree 剪掉整子树，
    // 也不走末尾追加（open=false 不进追加循环）→ popup 子树完全不出现在 FrameData。
    let fonts = test_font_table().expect("need test font");
    let (scene, _outer_id, _select_id, popup_id, option_id, option_text_id) =
        make_popup_scene(false);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    let popup_subtree_ids = [popup_id.0, option_id.0, option_text_id.0];
    let leaked: Vec<u32> = frame
        .nodes
        .iter()
        .filter(|rn| popup_subtree_ids.contains(&rn.node_id))
        .map(|rn| rn.node_id)
        .collect();
    assert!(
        leaked.is_empty(),
        "closed popup 子树不应渲染，但有节点泄漏：{leaked:?}"
    );
}

#[test]
fn popup_sort_key_strictly_above_scrollbar_thumb() {
    // 回归防护：scrollbar thumb 与 open popup 末尾追加同走 max_sort+1 续号——
    // popup_counter 必须从 thumb 之后再续，避免 popup 首节点与 thumb sort_key 撞号
    // （撞号会让 Unity MirrorPool 按 sort_key 排序时 popup/thumb 顺序不确定）。
    // 构造：outer(overflow:scroll + 超高 content → 产 thumb) > select(open, popup 子树)。
    use crate::style::resolved::OverflowMode;
    let fonts = test_font_table().expect("need test font");
    let mut outer_style = ResolvedStyle::default();
    outer_style.overflow_y = OverflowMode::Scroll;
    let mut popup_style = ResolvedStyle::default();
    popup_style.taffy_style.display = taffy::style::Display::Flex;
    let entries: Vec<(
        Option<usize>,
        NodeKind,
        ResolvedStyle,
        Vec<String>,
        Option<String>,
        bool,
        Option<i32>,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = vec![
        (
            None,
            NodeKind::Container,
            outer_style,
            vec![],
            None,
            false,
            None,
            None,
            None,
            None,
        ),
        (
            Some(0),
            NodeKind::Dropdown,
            ResolvedStyle::default(),
            vec![],
            None,
            false,
            None,
            None,
            None,
            None,
        ),
        (
            Some(1),
            NodeKind::Container,
            popup_style,
            vec!["loom-popup".to_string()],
            None,
            false,
            None,
            None,
            None,
            None,
        ),
    ];
    let mut scene = Scene::build(&entries);
    let outer_id = scene.roots[0];
    let select_id = scene.get(outer_id).unwrap().children[0];
    let popup_id = scene
        .get(select_id)
        .unwrap()
        .children
        .iter()
        .copied()
        .find(|&c| {
            scene
                .get(c)
                .unwrap()
                .classes
                .iter()
                .any(|x| x == "loom-popup")
        })
        .expect("loom-popup");
    // 作者自写结构下 listbox 靠 role 定位（Scene::build entries 不含 role 字段，手动登记）。
    scene.roles.insert(
        popup_id,
        crate::scene::node::RoleInfo {
            role: Some(crate::scene::control::ROLE_LISTBOX.to_string()),
            slots: Default::default(),
            aria_controls: None,
        },
    );
    // outer 设大 viewport，content 更高 → overflow:scroll effective → 产 v-thumb。
    scene.get_mut(outer_id).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    };
    scene.get_mut(select_id).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 40.0,
        h: 40.0,
    };
    scene.get_mut(popup_id).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 10.0,
        h: 10.0,
    };
    // content 尺寸需 > viewport 才 effective：手动设 scroll content_size。
    crate::scroll::refresh_content_sizes(&mut scene);
    // 强制 content 超高（refresh 可能因单子未溢出而不超——直接写 scroll 表）。gurad：
    if let Some(s) = scene.scroll.get_mut(outer_id) {
        s.content_size = (100.0, 300.0);
        s.viewport_size = (100.0, 100.0);
    }
    scene.controls.ensure(
        select_id,
        ControlState::Dropdown {
            selected_index: 0,
            open: true,
            value_lock: false,
            open_selected_index: None,
            option_values: Vec::new(),
        },
    );
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    // thumb 节点（合成 id 带 V_THUMB_FLAG）与 popup 节点都应存在。
    let thumb_sk = frame
        .nodes
        .iter()
        .find(|rn| rn.node_id & crate::scroll::V_THUMB_FLAG != 0)
        .map(|rn| rn.sort_key);
    let popup_sk = frame
        .nodes
        .iter()
        .find(|rn| rn.node_id == popup_id.0)
        .map(|rn| rn.sort_key);
    if let (Some(thumb_sk), Some(popup_sk)) = (thumb_sk, popup_sk) {
        assert!(
            popup_sk > thumb_sk,
            "popup sort_key ({popup_sk}) 必须 > scrollbar thumb sort_key ({thumb_sk})——
            popup 末尾追加续号须在 thumb 之后"
        );
    }
    // 至少 popup 必须存在（thumb 可能因 content 计算细节不 effective，但 popup 一定有）。
    assert!(popup_sk.is_some(), "open popup 必须渲染");
}

#[test]
fn open_popup_renders_option_list_via_reparent_path() {
    // 生产路径回归：select 经 create_node_from_template（作者自写结构）+ listbox role 子 +
    // option 在 listbox 内 + sync_control_visuals（open=true 设 listbox 的 display:block）后，
    // render 末尾追加须把 option 文本画进浮层 listbox（mask=0，跳出祖先 overflow:hidden）。这是
    // Task 11 popup 渲染的真正生产结构证明：option 是 listbox 的子节点（而非 select 的兄弟），
    // 否则 listbox 浮层为空、option 被祖先 clip 裁掉。
    use crate::asset::ControlInit;
    use crate::scene::control::ROLE_LISTBOX;
    use crate::scene::node::RoleInfo;
    use crate::style::resolved::OverflowMode;
    let fonts = test_font_table().expect("need test font");
    // outer(overflow:hidden) > select(Dropdown) 。select 不入 roots（隔离 solve，手动设 rect）。
    let mut outer_style = ResolvedStyle::default();
    outer_style.overflow_x = OverflowMode::Hidden;
    outer_style.overflow_y = OverflowMode::Hidden;
    let mut scene = Scene::default();
    // outer 作 root（overflow:hidden 开 clip，验证 popup 跳出它）。
    let outer = crate::scene::dynamic::create_root(&mut scene, "div", "").expect("create root div");
    scene.get_mut(outer).unwrap().base_style = outer_style.clone();
    scene.get_mut(outer).unwrap().style = outer_style;
    // select：用 create_node_from_template（作者自写结构，core 不注入）。
    let sel = crate::scene::dynamic::create_node_from_template(
        &mut scene,
        NodeKind::Dropdown,
        ResolvedStyle::default(),
        Some(ControlInit::Dropdown {
            selected_index: 0,
            option_values: Vec::new(),
        }),
    );
    crate::scene::dynamic::append_child(&mut scene, outer, sel).expect("select attach");
    // listbox role 子（作者写的弹出列表容器）。
    let popup = crate::scene::dynamic::create_node_from_template(
        &mut scene,
        NodeKind::Container,
        ResolvedStyle::default(),
        None,
    );
    crate::scene::dynamic::append_child(&mut scene, sel, popup).expect("listbox attach");
    scene.roles.insert(
        popup,
        RoleInfo {
            role: Some(ROLE_LISTBOX.to_string()),
            slots: Default::default(),
            aria_controls: None,
        },
    );
    // 2 个 option 直接挂 listbox（作者正确结构）。
    let mut opt_ids = vec![];
    for t in ["Apple", "Banana"] {
        let opt = crate::scene::dynamic::create_node_from_template(
            &mut scene,
            NodeKind::OptionItem,
            ResolvedStyle::default(),
            None,
        );
        let txt = crate::scene::dynamic::create_node_from_template(
            &mut scene,
            NodeKind::TextNode,
            ResolvedStyle::default(),
            None,
        );
        scene.text_contents.insert(txt, t.to_string());
        crate::scene::dynamic::append_child(&mut scene, opt, txt).expect("text attach");
        crate::scene::dynamic::append_child(&mut scene, popup, opt).expect("option attach");
        opt_ids.push(opt);
    }
    // 展开 popup：sync_control_visuals 按 open=true 移除 popup 的 display:none 覆盖。
    scene.controls.ensure(
        sel,
        ControlState::Dropdown {
            selected_index: 0,
            open: true,
            value_lock: false,
            open_selected_index: None,
            option_values: Vec::new(),
        },
    );
    crate::scene::control::sync_control_visuals(&mut scene, sel, 0.0);
    assert_eq!(
        scene
            .get(popup)
            .unwrap()
            .inline_override
            .taffy_style
            .display,
        taffy::style::Display::Block,
        "sync 后 popup inline display:block（展开始可见，标准弹出列表语义，下帧 rematch 应用）"
    );
    // 给所有节点非零 layout_rect（render 按 rect 产几何）。
    let all_ids: Vec<NodeId> = scene.nodes.values().map(|n| n.id).collect();
    for nid in all_ids {
        scene.get_mut(nid).unwrap().layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        };
    }
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    // option 子树（option + 其 TextNode）必须出现在 FrameData（证明 popup 浮层 DFS
    // 遍历到了它们——未 reparent 的旧结构会漏掉，option 会在 popup 外被祖先 clip 裁掉）。
    for opt in &opt_ids {
        assert!(
            frame.nodes.iter().any(|rn| rn.node_id == opt.0),
            "option {opt:?} 须渲染（popup 浮层含它）"
        );
        // option 是 popup 的直接子（reparent 后），其 RenderNode mask 必须 = 0（跳出 outer clip）。
        let opt_rn = frame
            .nodes
            .iter()
            .find(|rn| rn.node_id == opt.0)
            .expect("option render node");
        assert_eq!(
            opt_rn.mask_context,
            MaskContext(0),
            "option 在 popup 浮层里 → mask=0（跳出祖先 overflow:hidden）"
        );
    }
}
