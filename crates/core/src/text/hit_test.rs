//! Rich-text-block 子节点命中测试。
//!
//! [`hit_test`]（`crate::hit`）把一个 design 坐标点解析到容器级 `NodeId`。当命中目标是
//! rich-text-block 容器（其 inline 子树被 solve 折叠进单段 inline flow、render 画进父 mesh，
//! 子节点无独立 box），容器级命中无法区分「点落在外层文字 / 某个 span / 行内图」——而后端
//! 须 firing span 级点击事件。本模块在容器级命中之后做二次细化：给定 rich-text-block 容器
//! 内的 block-local 点，返回它命中的源 inline 节点（span / TextNode / Image）。
//!
//! 几何来源 = solve 期存进 `scene.text_layouts[block]` 的 `TextLayout.run_rects`：
//! 每个 `RichRunRect` 是一条 input run 在某行的命中矩形，坐标在布局的
//! pen/content-area 空间（content-area 左上原点）。incoming `local_pt` 是 block-local
//! （border-box 左上原点）——须减去 padding+border 内边距转成 content 坐标，与 render
//! `bake_content_offset` 把同样的内边距烤进 glyph pen 的变换互逆。

use crate::render::resolve_lp;
use crate::scene::node::{NodeId, Scene};

/// 把 rich-text-block 容器内的 block-local 点细化到源 inline 节点。
///
/// `block` 须是 `rich_text_block=true` 且 solve 已为其填 `scene.text_layouts[block]` 的容器。
/// `local_pt` 是相对该容器 border-box 左上的点（block-local，与 [`crate::hit::hit_test`]
/// world_to_local 后的本地坐标同空间）。返回首个包含（point-in-rect）该点的 `run_rect.source`；
/// 无 layout / 无 rect 命中 → `None`。
///
/// **坐标系换算**：`run_rects` 在 pen/content-area 空间（content-area 左上原点，render
/// `bake_content_offset` 在画时把 padding+border 内边距烤进 glyph pen；存进 `text_layouts`
/// 的是烤前原值）。`local_pt` 是 border-box 原点，故先减左 padding+border、顶 padding+border
/// 得 content 坐标，再判命中——与 render 放置文字的变换严格互逆。
///
/// **命中顺序**：按 `run_rects` 存储序首个命中即返。measure 按行从上到下、行内按 pen 推进序
/// 推 rect，故 first-match 给出视觉上最上/最左的 run；同行同 source 的多段（跨行拆条）任意
/// 一条命中都返同一 source，粒度正确。后端如需更细（如区分同一 span 跨行的两段）可后续扩展。
pub fn hit_test_rich(scene: &Scene, block: NodeId, local_pt: (f32, f32)) -> Option<NodeId> {
    let layout = scene.text_layouts.get(block.index())?.as_ref()?;
    let node = scene.get(block)?;
    let s = &node.style;
    // 与 render `bake_content_offset` 同一组内边距提取（render/mod.rs TextNode / rich-text
    // arm 的 off_left/off_top）。`resolve_lp` 是 render 暴露的 taffy LengthPercentage 解码
    // 单一真相源——此处复用保证两路换算永远一致（改一处即两处同步）。
    let off_x = resolve_lp(s.taffy_style.border.left) + resolve_lp(s.taffy_style.padding.left);
    let off_y = resolve_lp(s.taffy_style.border.top) + resolve_lp(s.taffy_style.padding.top);
    let content_pt = (local_pt.0 - off_x, local_pt.1 - off_y);
    for r in &layout.run_rects {
        if content_pt.0 >= r.x
            && content_pt.0 <= r.x + r.w
            && content_pt.1 >= r.y
            && content_pt.1 <= r.y + r.h
        {
            return Some(r.source);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::solve;
    use crate::scene::node::{NodeKind, Scene};
    use crate::style::resolved::ResolvedStyle;
    use crate::text::layout::FontTable;
    use std::collections::HashMap;

    /// 测试字体：仓库内 DejaVuSans.ttf（跨平台一致），缺则跳过。
    fn font_table() -> Option<FontTable> {
        let path = format!(
            "{}/tests/fixtures/DejaVuSans.ttf",
            env!("CARGO_MANIFEST_DIR")
        );
        let bytes = std::fs::read(&path).ok()?;
        let mut ft = FontTable::new();
        ft.register("default", bytes, true).ok()?;
        Some(ft)
    }

    /// `<div rich>text <span>x</span></div>` 场景：div 显式宽 100，font_size 16。
    /// 返回 (scene, div_id, textnode_id, span_id)。`padding` 额外设进 div.style.taffy_style。
    /// `solve` 后 `scene.text_layouts[div]` 含 run_rects（tn0 一条、span 一条）。
    fn rich_scene(padding: f32) -> (Scene, NodeId, NodeId, NodeId) {
        let mut root_s = ResolvedStyle::default();
        root_s.taffy_style.size.width = taffy::style::Dimension::length(200.0);
        let mut div_s = ResolvedStyle::default();
        div_s.taffy_style.size.width = taffy::style::Dimension::length(100.0);
        div_s.font_size = 16.0;
        if padding > 0.0 {
            div_s.taffy_style.padding = taffy::geometry::Rect {
                left: taffy::style::LengthPercentage::length(padding),
                right: taffy::style::LengthPercentage::length(padding),
                top: taffy::style::LengthPercentage::length(padding),
                bottom: taffy::style::LengthPercentage::length(padding),
            };
        }
        let entries = [
            (
                None,
                NodeKind::Container,
                root_s,
                Vec::new(),
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
                Vec::new(),
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
                Vec::new(),
                None,
                false,
                None,
                None,
                Some("text ".into()),
                None,
            ),
            (
                Some(1),
                NodeKind::TextElement,
                ResolvedStyle::default(),
                Vec::new(),
                None,
                false,
                None,
                None,
                None,
                None,
            ),
            (
                Some(3),
                NodeKind::TextNode,
                ResolvedStyle::default(),
                Vec::new(),
                None,
                false,
                None,
                None,
                Some("x".into()),
                None,
            ),
        ];
        let mut scene = Scene::build(&entries);
        let div = scene.get(scene.roots[0]).unwrap().children[0];
        let tn0 = scene.get(div).unwrap().children[0];
        let span = scene.get(div).unwrap().children[1];
        scene.get_mut(div).unwrap().rich_text_block = true;
        let fonts = font_table().expect("need DejaVuSans.ttf fixture");
        solve(
            &mut scene,
            &fonts,
            (200.0, 1000.0),
            [0.0; 4],
            &HashMap::new(),
        );
        (scene, div, tn0, span)
    }

    /// run_rects 里取首个 `source == target` 的 rect 中心点（content-area 坐标）。
    fn rect_center_for(scene: &Scene, div: NodeId, target: NodeId) -> (f32, f32) {
        let layout = scene.text_layouts[div.index()]
            .as_ref()
            .expect("solve 应填 text_layouts[div]");
        let r = layout
            .run_rects
            .iter()
            .find(|r| r.source == target)
            .unwrap_or_else(|| panic!("应有 source={:?} 的 run_rect", target));
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    #[test]
    fn hit_test_rich_resolves_to_span_source() {
        let (scene, div, _tn0, span) = rich_scene(0.0);
        // span rect 中心（content 坐标）→ 无 padding 时 == block-local。
        let (cx, cy) = rect_center_for(&scene, div, span);
        assert_eq!(
            hit_test_rich(&scene, div, (cx, cy)),
            Some(span),
            "点在 span rect 内 → Some(span_id)"
        );
    }

    #[test]
    fn hit_test_rich_resolves_to_textnode_source() {
        let (scene, div, tn0, _span) = rich_scene(0.0);
        let (cx, cy) = rect_center_for(&scene, div, tn0);
        assert_eq!(
            hit_test_rich(&scene, div, (cx, cy)),
            Some(tn0),
            "点在外层 text 区 → Some(textnode_id)"
        );
    }

    #[test]
    fn hit_test_rich_returns_none_outside() {
        let (scene, div, _tn0, _span) = rich_scene(0.0);
        // 远离所有 rect（div 宽 100，content 远小于此；200,200 必在 box 外）。
        assert_eq!(
            hit_test_rich(&scene, div, (200.0, 200.0)),
            None,
            "点在所有 run_rect 外 → None"
        );
    }

    #[test]
    fn hit_test_rich_returns_none_when_no_layout() {
        // 未 solve 的 scene：text_layouts[div] 仍 None → None。
        let (mut scene, div, _tn0, _span) = rich_scene(0.0);
        scene.text_layouts[div.index()] = None;
        assert_eq!(hit_test_rich(&scene, div, (5.0, 5.0)), None);
    }

    /// 坐标换算（关键点）：div 有 padding=10 → run_rects 仍在 pen/content 坐标
    /// （content 左上原点），block-local 点须减 (10,10) 才命中。验证 padding 区（block-local
    /// < 10）不误命中、content 区（block-local = content + 10）正确命中。
    #[test]
    fn hit_test_rich_subtracts_padding_border_offset() {
        let pad = 10.0;
        let (scene, div, tn0, span) = rich_scene(pad);
        // span rect center in CONTENT coords；block-local 须加 pad（无 border）。
        let (cx, cy) = rect_center_for(&scene, div, span);
        assert_eq!(
            hit_test_rich(&scene, div, (cx + pad, cy + pad)),
            Some(span),
            "block-local = content + padding → 命中（换算正确）"
        );
        // tn0 同理验证一次（左段 text 也须加 pad）。
        let (tx, ty) = rect_center_for(&scene, div, tn0);
        assert_eq!(
            hit_test_rich(&scene, div, (tx + pad, ty + pad)),
            Some(tn0),
            "text 段同样须 content+padding 才命中"
        );
        // padding 带内的 block-local 点（< pad）→ content 坐标为负 → 必在所有 rect 外
        // （首行 rect y>=0，x>=0）。这是换算被应用（非恒等）的铁证：若 hit_test 忽略 padding，
        // (pad/2, pad/2) 会被当作 content (pad/2, pad/2) 命中首行 rect（首行覆盖该区）。
        assert_eq!(
            hit_test_rich(&scene, div, (pad / 2.0, pad / 2.0)),
            None,
            "padding 带内 block-local → content 负 → 不命中（证明换算非恒等）"
        );
    }
}
