use super::*;
use crate::asset::{PackageInput, TemplateNode};
use crate::scene::NodeKind;
use crate::style::resolved::ResolvedStyle;
use taffy::style::Dimension;

/// 辅助：建带 Image 子的 pkg（root Container + Image leaf，src=path）。
/// 图尺寸不再进入 pkg.bin，改为通过 Stage.image_sizes 直接灌入。
fn make_pkg_with_image(src: &str) -> Vec<u8> {
    let mut img_style = ResolvedStyle::default();
    // align_self=FLEX_START 防 column 容器 stretch 把 cross 轴宽拉满
    img_style.taffy_style.align_self = Some(taffy::style::AlignSelf::FLEX_START);
    let nodes = [
        TemplateNode {
            kind: NodeKind::Container,
            style: ResolvedStyle::default(),
            parent_idx: None,
            classes: vec![],
            id_attr: None,
            draggable: false,
            tabindex: None,
            content: None,
            src: None,
        },
        TemplateNode {
            kind: NodeKind::Image,
            style: img_style,
            parent_idx: Some(0),
            classes: vec![],
            id_attr: None,
            draggable: false,
            tabindex: None,
            content: None,
            src: Some(src.into()),
        },
    ];
    let rules = crate::style::dynamic::DynamicRuleTable::default();
    let input = PackageInput {
        components: vec![("comp1", &nodes, &rules)],
    };
    crate::asset::write_package(&input)
}

/// 端到端：40×20 图尺寸灌入 image_sizes → instantiate → solve → Image measure 用真实 40×20。
#[test]
fn set_image_sizes_and_measure_uses_real_dims() {
    let pkg_bytes = make_pkg_with_image("icons/wide.png");
    let mut s = Stage::new_for_test();
    s.create_root("div", "width:300px;height:300px").unwrap();
    s.load_package("bag", &pkg_bytes).unwrap();
    // 模拟 set_image_sizes：灌入 (w=40, h=20)
    s.image_sizes.insert("icons/wide.png".into(), (40, 20));
    assert_eq!(
        s.image_size("icons/wide.png"),
        Some((40, 20)),
        "image_sizes 表含 path → (40,20)"
    );

    let comp_root = s.instantiate("bag", "comp1").unwrap();
    s.append_child(s.scene.as_ref().unwrap().roots[0], comp_root)
        .unwrap();
    s.tick_and_render();

    // Image 是 comp_root 的首个子
    let scene = s.scene.as_ref().unwrap();
    let img_id = scene.get(comp_root).unwrap().children[0];
    let r = &scene.get(img_id).unwrap().layout_rect;
    // 无 CSS 尺寸 → 用尺寸表真实像素 40×20（三档第二档）
    assert!(
        (r.w - 40.0).abs() < 0.1,
        "measure 用真实 w=40（非 64 兜底），got {}",
        r.w
    );
    assert!(
        (r.h - 20.0).abs() < 0.1,
        "measure 用真实 h=20（非 64 兜底），got {}",
        r.h
    );
}

/// path 不在 image_sizes 表中 → image_size 返 None → measure fallback 64×64。
#[test]
fn missing_path_falls_back_to_64() {
    let pkg_bytes = make_pkg_with_image("icons/zero.png");
    let mut s = Stage::new_for_test();
    s.create_root("div", "width:300px;height:300px").unwrap();
    s.load_package("bag", &pkg_bytes).unwrap();
    // 不灌 "icons/zero.png" → image_size 返 None
    assert_eq!(
        s.image_size("icons/zero.png"),
        None,
        "未灌入 → None（fallback 64×64）"
    );

    let comp_root = s.instantiate("bag", "comp1").unwrap();
    s.append_child(s.scene.as_ref().unwrap().roots[0], comp_root)
        .unwrap();
    s.tick_and_render();

    let scene = s.scene.as_ref().unwrap();
    let img_id = scene.get(comp_root).unwrap().children[0];
    let r = &scene.get(img_id).unwrap().layout_rect;
    assert!(
        (r.w - 64.0).abs() < 0.1,
        "未灌入 → fallback w=64，got {}",
        r.w
    );
    assert!(
        (r.h - 64.0).abs() < 0.1,
        "未灌入 → fallback h=64，got {}",
        r.h
    );
}

/// CSS 尺寸赢过真实像素（三档第一档）。
/// 40×20 图 + CSS width:80px → w=80（CSS），height 等比 = 40（80×20/40，2:1 真实 aspect）。
#[test]
fn css_length_overrides_real_image_size() {
    let mut img_style = ResolvedStyle::default();
    img_style.taffy_style.size.width = Dimension::length(80.0);
    img_style.taffy_style.align_self = Some(taffy::style::AlignSelf::FLEX_START);
    let nodes = [
        TemplateNode {
            kind: NodeKind::Container,
            style: ResolvedStyle::default(),
            parent_idx: None,
            classes: vec![],
            id_attr: None,
            draggable: false,
            tabindex: None,
            content: None,
            src: None,
        },
        TemplateNode {
            kind: NodeKind::Image,
            style: img_style,
            parent_idx: Some(0),
            classes: vec![],
            id_attr: None,
            draggable: false,
            tabindex: None,
            content: None,
            src: Some("icons/wide.png".into()),
        },
    ];
    let rules = crate::style::dynamic::DynamicRuleTable::default();
    let input = PackageInput {
        components: vec![("comp1", &nodes, &rules)],
    };
    let pkg_bytes = crate::asset::write_package(&input);

    let mut s = Stage::new_for_test();
    s.create_root("div", "width:300px;height:300px").unwrap();
    s.load_package("bag", &pkg_bytes).unwrap();
    // 灌入真实尺寸 40×20
    s.image_sizes.insert("icons/wide.png".into(), (40, 20));
    let comp_root = s.instantiate("bag", "comp1").unwrap();
    s.append_child(s.scene.as_ref().unwrap().roots[0], comp_root)
        .unwrap();
    s.tick_and_render();

    let scene = s.scene.as_ref().unwrap();
    let img_id = scene.get(comp_root).unwrap().children[0];
    let r = &scene.get(img_id).unwrap().layout_rect;
    // CSS width:80px 赢（三档第一档）；height 等比用真实 2:1 aspect = 80×20/40 = 40
    assert!((r.w - 80.0).abs() < 0.1, "CSS width 赢：w=80，got {}", r.w);
    assert!(
        (r.h - 40.0).abs() < 0.1,
        "height 等比=40（80×20/40 真实 2:1），got {}",
        r.h
    );
}

/// 多包 `image_sizes` 共享同一表（与包解耦——都走运行时灌入）。
#[test]
fn multi_package_merged_sizes_via_direct_insert() {
    let pkg_a = make_pkg_with_image("icons/a.png");
    let pkg_b = make_pkg_with_image("icons/b.png");
    let mut s = Stage::new_for_test();
    s.load_package("a", &pkg_a).unwrap();
    s.load_package("b", &pkg_b).unwrap();
    // 模拟 set_image_sizes：分别灌入 a 和 b 的尺寸
    s.image_sizes.insert("icons/a.png".into(), (10, 20));
    s.image_sizes.insert("icons/b.png".into(), (30, 40));
    assert_eq!(s.image_size("icons/a.png"), Some((10, 20)), "path a 进表");
    assert_eq!(s.image_size("icons/b.png"), Some((30, 40)), "path b 进表");
}

/// 重复 load 同名包 + 重新灌入尺寸：新尺寸覆盖旧。
#[test]
fn reload_package_with_new_sizes() {
    let pkg_v1 = make_pkg_with_image("icons/x.png");
    let pkg_v2 = make_pkg_with_image("icons/x.png");
    let mut s = Stage::new_for_test();
    s.load_package("bag", &pkg_v1).unwrap();
    s.image_sizes.insert("icons/x.png".into(), (10, 10));
    assert_eq!(s.image_size("icons/x.png"), Some((10, 10)), "首次灌入");
    s.load_package("bag", &pkg_v2).unwrap();
    s.image_sizes.insert("icons/x.png".into(), (50, 50));
    assert_eq!(
        s.image_size("icons/x.png"),
        Some((50, 50)),
        "重灌覆盖（新尺寸）"
    );
}

/// set_image_sizes 批量覆盖式合并：同 path 后写赢；w/h=0 也存（image_size 的 filter 挡）。
#[test]
fn set_image_sizes_batch_merges() {
    let mut stage = Stage::new((200.0, 200.0)).unwrap();
    stage.set_image_sizes(&[
        ("icons/a.png".to_string(), 32, 32),
        ("icons/b.png".to_string(), 64, 48),
    ]);
    assert_eq!(stage.image_size("icons/a.png"), Some((32, 32)));
    assert_eq!(stage.image_size("icons/b.png"), Some((64, 48)));
    // 后写赢
    stage.set_image_sizes(&[("icons/a.png".to_string(), 100, 100)]);
    assert_eq!(stage.image_size("icons/a.png"), Some((100, 100)));
}

/// w/h=0 条目存入但 image_size 挡回 None（filter w/h=0 → fallback 64×64）。
#[test]
fn set_image_sizes_zero_dim_is_stored_but_filtered() {
    let mut stage = Stage::new((200.0, 200.0)).unwrap();
    stage.set_image_sizes(&[("icons/zero.png".to_string(), 0, 0)]);
    assert_eq!(
        stage.image_size("icons/zero.png"),
        None,
        "w/h=0 filter -> None"
    );
    // 但 HashMap 里有它
    assert!(stage.image_sizes.contains_key("icons/zero.png"));
}

/// reuse_key 是运行时字段（不进 pkg），driver 给 slot 节点设。
/// 0=无复用（默认），>0=按 reuse_key 复用 GO。
#[test]
fn set_reuse_key_sets_field() {
    let mut stage = Stage::new_for_test();
    let root = stage.create_root("div", "").unwrap();
    let child = stage.create_node("div", "").unwrap();
    stage.append_child(root, child).unwrap();
    assert_eq!(
        stage.scene.as_ref().unwrap().get(child).unwrap().reuse_key,
        0,
        "默认 0"
    );
    stage.set_reuse_key(child, 5);
    assert_eq!(
        stage.scene.as_ref().unwrap().get(child).unwrap().reuse_key,
        5
    );
}

/// set_reuse_key 对无效 node（已删/悬空）no-op，不 panic。
#[test]
fn set_reuse_key_invalid_node_noop() {
    let mut stage = Stage::new_for_test();
    let root = stage.create_root("div", "").unwrap();
    // NodeId(99999) 不存在 → no-op，不 panic。
    stage.set_reuse_key(crate::scene::node::NodeId(99999), 42);
    // create_root 成功即 no-op 未 panic。
    assert!(root.0 > 0);
}
