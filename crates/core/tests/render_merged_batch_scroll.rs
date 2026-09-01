//! merged 批增量的滚动/隐藏/平移回归（#109 验收期 bug 批）。
//!
//! 背景：merged 批旧实现矩阵恒 IDENTITY + 成员 ph（局部系、位移不变量）拼合定级，
//! 纯滚动/平移下 header_hash 与 payload_hash 双不变 → 整批 ChangeLevel::Skip →
//! Unity 镜像冻结在旧位置（拖动中布局全乱 / 世界锚点跟随跳变的根因）。修复后：
//! - 批持 anchor 平移矩阵 → 同质批（全员随 anchor 平移）滚动走 Header（只挪 GO）；
//! - 混合批（静态 anchor + 移动成员）整批 payload hash 必变 → Full（重传 mesh）。

use ikat_core::render::node::{ChangeLevel, NodePayload};
use ikat_core::scene::dynamic::{set_node_render_hidden, set_user_transform};
use ikat_core::scene::node::NodeId;
use ikat_core::stage::Stage;
use ikat_core::transform::NodeTransform;

fn font_bytes() -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/DejaVuSans.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("DejaVuSans.ttf fixture must exist")
}

/// root > scroller(overflow-y:scroll) > [dummy(独有 alpha，批分裂锚) + 两个同
/// DrawState div（相邻 sort → 合并成批，anchor=kid0）]。返回 (stage, scroller_id, kid_ids)。
fn make_stage() -> (Stage, u64, Vec<u64>) {
    let mut s = Stage::new((800.0, 600.0)).unwrap();
    s.register_font("dejavu", font_bytes(), true).unwrap();
    let root = s.create_root("div", "width:800px;height:600px").unwrap();
    let scroller = s
        .create_node(
            "div",
            "position:absolute;left:0;top:0;width:400px;height:100px;overflow-y:scroll",
        )
        .unwrap();
    s.append_child(root, scroller).unwrap();
    // dummy：id 最小但 alpha 独异（0.9），把 scroller/root 的透明 quad 挡在批外——
    // 否则批 anchor 落在不动的容器上，测不了「anchor 随成员移动」的 Header 路径。
    let dummy = s
        .create_node(
            "div",
            "position:absolute;left:0;top:0;width:200px;height:100px;background-color:#123456;opacity:0.9",
        )
        .unwrap();
    s.append_child(scroller, dummy).unwrap();
    let mut kids = Vec::new();
    for i in 0..2 {
        let k = s
            .create_node(
                "div",
                &format!(
                    "position:absolute;left:0;top:{}px;width:200px;height:100px;background-color:#123456",
                    100 + i * 100
                ),
            )
            .unwrap();
        s.append_child(scroller, k).unwrap();
        kids.push(k.0);
    }
    (s, scroller.0, kids)
}

/// 帧内找 merged 批行（≥2 quad 且 node_id ∈ 候选集——anchor 可能是 scroller 自身）。
fn merged_row<'a>(
    frame: &'a ikat_core::render::FrameData,
    candidates: &[u64],
) -> &'a ikat_core::render::node::RenderNode {
    frame
        .nodes
        .iter()
        .find(|rn| {
            candidates.contains(&rn.node_id)
                && matches!(
                    &rn.payload,
                    NodePayload::Mesh { verts, .. } if verts.len() >= 8
                )
        })
        .expect("merged batch row (>=2 quads) must exist")
}

fn set_scroll(s: &mut Stage, scroller: u64, y: f32) {
    let sc = s.scene.as_mut().unwrap();
    let entry = sc.scroll.get_mut(NodeId(scroller)).expect("scroll entry");
    entry.scroll_pos.1 = y;
}

#[test]
fn merged_batch_tracks_scroll_not_skip() {
    let (mut s, scroller, kids) = make_stage();
    let mut candidates = kids.clone();
    candidates.push(scroller);
    for _ in 0..3 {
        let _ = s.tick_and_render();
    }
    // 稳态：merged 批 Skip（增量命中——效率语义保留）。
    let steady = s.tick_and_render();
    assert_eq!(
        merged_row(&steady, &candidates).change_level,
        ChangeLevel::Skip,
        "稳态帧 merged 批应 Skip（增量复用）"
    );

    // 滚动 50px：merged 批必须非 Skip（混合批 anchor=scroller 不动、成员动 → Full；
    // 无论走 Header 还是 Full，冻结（Skip）都是回归）。
    set_scroll(&mut s, scroller, 50.0);
    let scrolled = s.tick_and_render();
    assert_ne!(
        merged_row(&scrolled, &candidates).change_level,
        ChangeLevel::Skip,
        "滚动帧 merged 批不得 Skip（旧实现整批冻结 = 拖动布局乱的根因）"
    );

    // 停稳帧恢复 Skip（增量效率不受修复影响）。
    let settled = s.tick_and_render();
    assert_eq!(
        merged_row(&settled, &candidates).change_level,
        ChangeLevel::Skip,
        "停稳帧 merged 批应回落 Skip"
    );
}

#[test]
fn homogeneous_batch_translation_is_header_with_matrix() {
    // 全员随 anchor 平移（世界锚点 Transform.Position 同形态）：批矩阵带平移 →
    // header_hash 变 → Header 级（不重传 mesh），矩阵 tx/ty 即平移量。make_stage 的
    // dummy 已把 scroller 静态 quad 挡在批外——批内只剩两个 kid（anchor=kid0 随平移动）。
    let (mut s, _scroller, kids) = make_stage();
    let candidates = kids.clone();
    for _ in 0..4 {
        let _ = s.tick_and_render();
    }
    {
        let sc = s.scene.as_mut().unwrap();
        for &k in &kids {
            set_user_transform(
                sc,
                NodeId(k),
                NodeTransform {
                    translate: [30.0, 70.0],
                    ..Default::default()
                },
            )
            .unwrap();
        }
    }
    let f = s.tick_and_render();
    let row = merged_row(&f, &candidates);
    assert_eq!(
        row.change_level,
        ChangeLevel::Header,
        "同质批平移应走 Header（矩阵变、payload 不变）"
    );
    assert_eq!(row.world_matrix[4], 30.0, "批矩阵 tx = 平移量");
    assert_eq!(
        row.world_matrix[5], 170.0,
        "批矩阵 ty = kid0 世界 y（布局 100 + 平移 70）"
    );
}

#[test]
fn render_hidden_splits_batch_and_emits_invisible_row() {
    let (mut s, _scroller, kids) = make_stage();
    for _ in 0..4 {
        let _ = s.tick_and_render();
    }
    {
        let sc = s.scene.as_mut().unwrap();
        set_node_render_hidden(sc, NodeId(kids[0]), true).unwrap();
    }
    let f = s.tick_and_render();
    assert!(
        f.nodes
            .iter()
            .any(|rn| !rn.visible && rn.change_level != ChangeLevel::Skip),
        "隐藏成员必须产出 visible=false 的非 Skip 行（隐藏 250 无效链路回归）"
    );

    // 恢复显示：无 visible=false 行。
    {
        let sc = s.scene.as_mut().unwrap();
        set_node_render_hidden(sc, NodeId(kids[0]), false).unwrap();
    }
    let f = s.tick_and_render();
    assert!(
        f.nodes.iter().all(|rn| rn.visible),
        "恢复显示后不得残留 visible=false 行"
    );
}

#[test]
fn set_node_render_hidden_same_value_is_noop_for_increment() {
    let (mut s, _scroller, kids) = make_stage();
    for _ in 0..4 {
        let _ = s.tick_and_render();
    }
    // 稳态后对未隐藏节点重复写 false（世界锚点每帧 SetNodeRenderVisible(true) 同款）：
    // 不得 bump render_input_version → 稳态帧保持全 Skip（无重建 churn）。
    let steady = s.tick_and_render();
    assert!(
        steady
            .nodes
            .iter()
            .all(|rn| rn.change_level == ChangeLevel::Skip),
        "前置：稳态帧应全 Skip"
    );
    for _ in 0..3 {
        let sc = s.scene.as_mut().unwrap();
        set_node_render_hidden(sc, NodeId(kids[0]), false).unwrap();
    }
    let f = s.tick_and_render();
    assert!(
        f.nodes
            .iter()
            .all(|rn| rn.change_level == ChangeLevel::Skip),
        "同值 render_hidden 写入不得触发重建（幂等短路）"
    );
}

#[test]
fn stage_document_root_is_not_hit_target() {
    // 文档根（create_root 建的宿主容器）不可命中：根铺满画布，可命中会让多 Stage
    // 输入路由（Pick 命中即独占）把指针下所有底层 Stage 饿死；点到空白处应返 None，
    // 命中只能落在页面内容（authored 子树）上。
    let mut s = Stage::new((800.0, 600.0)).unwrap();
    let root = s.create_root("div", "width:800px;height:600px").unwrap();
    let child = s
        .create_node(
            "div",
            "position:absolute;left:100px;top:100px;width:80px;height:60px",
        )
        .unwrap();
    s.append_child(root, child).unwrap();
    let _ = s.tick_and_render();
    let scene = s.scene.as_ref().unwrap();
    // 内容命中：子节点区域 → 子。
    assert_eq!(
        ikat_core::hit::hit_test(scene, (140.0, 130.0)),
        Some(NodeId(child.0)),
        "内容区域命中子节点"
    );
    // 空白区域：不得命中文档根（返 None）。
    assert_eq!(
        ikat_core::hit::hit_test(scene, (400.0, 300.0)),
        None,
        "空白区域命中必须是 None（文档根不可命中）"
    );
}

#[test]
fn class_rule_pointer_events_reaches_hit_test() {
    // 类规则驱动的 pointer-events:none（页面 <style> 的 .root{pointer-events:none}
    // 走 dynamic_rules → 运行时 rematch）必须落到 interaction.touchable（hit_test
    // 判据）。断链症状：规则只改 style 层，全画布根照样吞命中——多 Stage 输入路由
    // 把底层 Stage 饿死（mini-hud 实锤）。
    use ikat_core::scene::node::NodeId;
    use ikat_core::style::dynamic::{
        Combinator, Compound, Declaration, DynamicRule, ParsedSelector, ScopedRule, Specificity,
    };
    let mut s = Stage::new((800.0, 600.0)).unwrap();
    let root = s.create_root("div", "width:800px;height:600px").unwrap();
    let rule = DynamicRule {
        selector: ParsedSelector {
            raw: ".no-hit".into(),
            compound: vec![Compound {
                tag: None,
                classes: vec!["no-hit".into()],
                id: None,
                combinator: Combinator::Descendant,
                pseudo_hover: false,
                pseudo_active: false,
                pseudo_disabled: false,
                pseudo_focus: false,
                pseudo_nth_child: None,
                attrs: Vec::new(),
            }],
            specificity: Specificity(0, 1, 0),
        },
        declarations: vec![Declaration {
            prop: "pointer-events".into(),
            value: "none".into(),
        }],
    };
    // 全局规则（scope_root=INVALID 跨作用域命中，同 UIContext.StyleSheet 语义）。
    s.scene
        .as_mut()
        .unwrap()
        .dynamic_rules
        .entries
        .push(ScopedRule {
            rule,
            scope_root: NodeId::INVALID,
        });
    let child = s
        .create_node(
            "div",
            "position:absolute;left:0;top:0;width:800px;height:600px",
        )
        .unwrap();
    s.append_child(root, child).unwrap();
    {
        let sc = s.scene.as_mut().unwrap();
        ikat_core::scene::dynamic::add_class(sc, child, "no-hit").unwrap();
    }
    let _ = s.tick_and_render(); // rematch 跑一轮：pointer-events → style → interaction
    let scene = s.scene.as_ref().unwrap();
    assert_eq!(
        ikat_core::hit::hit_test(scene, (400.0, 300.0)),
        None,
        "类规则 pointer-events:none 必须让该节点退出命中（rematch 回写 interaction）"
    );
}
