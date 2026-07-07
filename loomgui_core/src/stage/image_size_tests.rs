use super::*;
use crate::asset::{AssetEntry, PackageInput, TemplateNode};
use crate::scene::NodeKind;
use crate::style::resolved::ResolvedStyle;
use taffy::style::Dimension;

/// 辅助：建带 Image 子的 pkg（root Container + Image leaf，src=path）。
/// AssetEntry 带真实 w/h（模拟打包器读 PNG IHDR 填）。
fn make_pkg_with_image_size(src: &str, w: u32, h: u32) -> Vec<u8> {
    let mut img_style = ResolvedStyle::default();
    // align_self=FlexStart 防 column 容器 stretch 把 cross 轴宽拉满
    img_style.taffy_style.align_self = Some(taffy::style::AlignSelf::FlexStart);
    let nodes = [
        TemplateNode {
            kind: NodeKind::Container,
            style: ResolvedStyle::default(),
            parent_idx: None,
            classes: vec![],
            id_attr: None,
            draggable: false,
            tabindex: None,
            data_controller: None,
        },
        TemplateNode {
            kind: NodeKind::Image { src: src.into() },
            style: img_style,
            parent_idx: Some(0),
            classes: vec![],
            id_attr: None,
            draggable: false,
            tabindex: None,
            data_controller: None,
        },
    ];
    let rules = crate::style::dynamic::DynamicRuleTable::default();
    let manifest = [AssetEntry {
        path: src.into(),
        w,
        h,
    }];
    let input = PackageInput {
        components: vec![("comp1", &nodes, &rules, &[])],
        asset_manifest: &manifest,
    };
    crate::asset::write_package(&input)
}

/// 端到端：40×20 图打包进 pkg → load_package 建尺寸表 → instantiate → solve
/// → Image measure 用真实 40×20（非 64×64 兜底）。
#[test]
fn load_package_builds_size_table_and_measure_uses_real_dims() {
    let pkg_bytes = make_pkg_with_image_size("icons/wide.png", 40, 20);
    let mut s = Stage::new_for_test();
    s.create_root("div", "width:300px;height:300px").unwrap();
    s.load_package("bag", &pkg_bytes).unwrap();
    // load_package 后 Stage.image_sizes 含 path → (w,h)
    assert_eq!(
        s.image_size("icons/wide.png"),
        Some((40, 20)),
        "load_package 建尺寸表：path→(40,20)"
    );

    let comp_root = s.instantiate("bag", "comp1").unwrap();
    s.append_child(s.scene.as_ref().unwrap().roots[0], comp_root)
        .unwrap();
    s.tick_and_render();

    // Image 是 comp_root 的首个子（gather: root[0] + img[1]，img parent_idx=0）
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

/// pkg 的 AssetEntry w/h=0（非 PNG / 读失败）→ 尺寸表无有效条目 → measure fallback 64×64。
#[test]
fn load_package_zero_dims_falls_back_to_64() {
    let pkg_bytes = make_pkg_with_image_size("icons/zero.png", 0, 0);
    let mut s = Stage::new_for_test();
    s.create_root("div", "width:300px;height:300px").unwrap();
    s.load_package("bag", &pkg_bytes).unwrap();
    // w/h=0 → image_size 返 None（filter 掉 0/0）
    assert_eq!(
        s.image_size("icons/zero.png"),
        None,
        "w/h=0 → None（fallback 64×64）"
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
        "w/h=0 → fallback w=64，got {}",
        r.w
    );
    assert!(
        (r.h - 64.0).abs() < 0.1,
        "w/h=0 → fallback h=64，got {}",
        r.h
    );
}

/// CSS 尺寸赢过真实像素（三档第一档）。
/// 40×20 图 + CSS width:80px → w=80（CSS），height 等比 = 40（80×20/40，2:1 真实 aspect）。
#[test]
fn css_length_overrides_real_image_size() {
    let mut img_style = ResolvedStyle::default();
    img_style.taffy_style.size.width = Dimension::Length(80.0);
    img_style.taffy_style.align_self = Some(taffy::style::AlignSelf::FlexStart);
    let nodes = [
        TemplateNode {
            kind: NodeKind::Container,
            style: ResolvedStyle::default(),
            parent_idx: None,
            classes: vec![],
            id_attr: None,
            draggable: false,
            tabindex: None,
            data_controller: None,
        },
        TemplateNode {
            kind: NodeKind::Image {
                src: "icons/wide.png".into(),
            },
            style: img_style,
            parent_idx: Some(0),
            classes: vec![],
            id_attr: None,
            draggable: false,
            tabindex: None,
            data_controller: None,
        },
    ];
    let rules = crate::style::dynamic::DynamicRuleTable::default();
    let manifest = [AssetEntry {
        path: "icons/wide.png".into(),
        w: 40,
        h: 20,
    }];
    let input = PackageInput {
        components: vec![("comp1", &nodes, &rules, &[])],
        asset_manifest: &manifest,
    };
    let pkg_bytes = crate::asset::write_package(&input);

    let mut s = Stage::new_for_test();
    s.create_root("div", "width:300px;height:300px").unwrap();
    s.load_package("bag", &pkg_bytes).unwrap();
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

/// 多包 load_package 合并尺寸表（path 全局唯一）。
#[test]
fn multi_package_merges_size_tables() {
    let pkg_a = make_pkg_with_image_size("icons/a.png", 10, 20);
    let pkg_b = make_pkg_with_image_size("icons/b.png", 30, 40);
    let mut s = Stage::new_for_test();
    s.load_package("a", &pkg_a).unwrap();
    s.load_package("b", &pkg_b).unwrap();
    assert_eq!(
        s.image_size("icons/a.png"),
        Some((10, 20)),
        "包 a 的 path 进表"
    );
    assert_eq!(
        s.image_size("icons/b.png"),
        Some((30, 40)),
        "包 b 的 path 进表（多包合并）"
    );
}

/// 重复 load 同名包 → 新包尺寸覆盖前包（path 全局唯一，后写赢）。
#[test]
fn reload_package_overwrites_size_entry() {
    let pkg_v1 = make_pkg_with_image_size("icons/x.png", 10, 10);
    let pkg_v2 = make_pkg_with_image_size("icons/x.png", 50, 50);
    let mut s = Stage::new_for_test();
    s.load_package("bag", &pkg_v1).unwrap();
    assert_eq!(s.image_size("icons/x.png"), Some((10, 10)), "首次 load");
    s.load_package("bag", &pkg_v2).unwrap();
    assert_eq!(
        s.image_size("icons/x.png"),
        Some((50, 50)),
        "重 load 覆盖（新尺寸）"
    );
}

/// 重 load 同名包：旧包独有 path（新包没有）应从尺寸表清除（避免悬空残留）。
/// 覆盖 load_package 替换同名包时先清前次包 manifest 条目的逻辑。
#[test]
fn reload_package_clears_obsolete_size_entries() {
    let pkg_v1 = make_pkg_with_image_size("icons/old.png", 10, 20);
    let pkg_v2 = make_pkg_with_image_size("icons/new.png", 30, 40);
    let mut s = Stage::new_for_test();
    s.load_package("bag", &pkg_v1).unwrap();
    assert_eq!(
        s.image_size("icons/old.png"),
        Some((10, 20)),
        "v1 的 path 进表"
    );
    // v2 manifest 只有 new.png（无 old.png）→ 替换后 old.png 应清除
    s.load_package("bag", &pkg_v2).unwrap();
    assert_eq!(
        s.image_size("icons/old.png"),
        None,
        "v2 没有 old.png → 旧 path 清除（不悬空残留）"
    );
    assert_eq!(
        s.image_size("icons/new.png"),
        Some((30, 40)),
        "v2 的 path 进表"
    );
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
