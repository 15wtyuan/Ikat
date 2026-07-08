use super::*;
use crate::asset::{PackageInput, TemplateNode};
use crate::parse::dom::{ElementId, ElementTree};
use crate::scene::NodeKind;
use crate::style::resolved::ResolvedStyle;

/// 测试辅助：从 inline HTML+CSS 抽出 ComponentTemplate 数据（nodes + dynamic_rules），
/// 模仿 loomgui_pkg 打包器的提取逻辑（gather_rec 同构：tag→kind、resolve_styles 烘焙 style、
/// classes/id/draggable/tabindex 从 ElementData 取）。供黄金等价测试把 inline 场景序列化成包。
///
/// 约定：整棵 inline 树打包成一个名为 "scene" 的组件（nodes[0]=根，parent_idx=None）。
fn gather_template_nodes(
    tree: &ElementTree,
    styles: &[ResolvedStyle],
    el_id: ElementId,
    parent_idx: Option<usize>,
    out: &mut Vec<TemplateNode>,
) {
    let el = &tree.nodes[el_id.0];
    let style = &styles[el_id.0];
    let mut kind = crate::scene::dynamic::kind_from_tag(&el.tag)
        .unwrap_or_else(|_| unreachable!("parse 层白名单已挡围栏外 tag"));
    match &mut kind {
        NodeKind::Image { src } => {
            *src = el.attrs.get("src").cloned().unwrap_or_default();
        }
        NodeKind::Text { content } => {
            *content = el.text.clone().unwrap_or_default();
        }
        _ => {}
    }
    let draggable = el
        .attrs
        .get("draggable")
        .map(|v| v == "true")
        .unwrap_or(false);
    let tabindex = el.attrs.get("tabindex").and_then(|v| v.parse::<i32>().ok());
    let data_controller = el.attrs.get("data-controller").cloned();
    let my_idx = out.len();
    out.push(TemplateNode {
        kind: kind.clone(),
        style: style.clone(),
        parent_idx,
        classes: el.classes.clone(),
        id_attr: el.id.clone(),
        draggable,
        tabindex,
        data_controller,
    });
    // Container/Button 的裸文本 → Text 子（同 gather_rec，继承字体/颜色字段）
    if matches!(kind, NodeKind::Container | NodeKind::Button) {
        if let Some(text) = &el.text {
            let mut ts = ResolvedStyle::default();
            ts.color = style.color;
            ts.font_size = style.font_size;
            ts.font_family = style.font_family.clone();
            ts.font_weight = style.font_weight;
            ts.line_height = style.line_height;
            ts.letter_spacing = style.letter_spacing;
            ts.text_align = style.text_align;
            ts.white_space_nowrap = style.white_space_nowrap;
            out.push(TemplateNode {
                kind: NodeKind::Text {
                    content: text.clone(),
                },
                style: ts,
                parent_idx: Some(my_idx),
                classes: Vec::new(),
                id_attr: None,
                draggable: false,
                tabindex: None,
                data_controller: None,
            });
        }
    }
    for c in &el.children {
        gather_template_nodes(tree, styles, *c, Some(my_idx), out);
    }
}

/// 测试辅助：把 inline HTML+CSS 打成一个名为 "scene" 的单组件包 bytes。
/// 返回 (pkg_bytes, asset_manifest)。dynamic_rules 从 CSS 抽（含 :hover 等伪类的规则）。
fn pkg_bytes_from_inline(html: &str, css: &str) -> (Vec<u8>, Vec<String>) {
    let tree = crate::parse::dom::parse_html(html).unwrap();
    let sheet = crate::parse::css::parse_css(css).unwrap();
    let styles = crate::style::cascade::resolve_styles(&tree, &sheet);
    let dynamic = crate::asset::extract_dynamic_rules(&sheet);
    let mut nodes: Vec<TemplateNode> = Vec::new();
    // 单根树（inline 测试都是单根）；多根场景测试不在此 helper 范围
    for root in &tree.roots {
        gather_template_nodes(&tree, &styles, *root, None, &mut nodes);
    }
    // asset_manifest：扫所有 Image 节点的 src（已归一化路径——测试用 src 直接作 path）。
    // 图尺寸测试 helper 无 PNG 文件 → w/h=0（核心 measure fallback 64×64）。
    // 真实尺寸由 loomgui_pkg 打包器读 PNG IHDR 填（见 pkg 测试）。
    let manifest: Vec<crate::asset::AssetEntry> = nodes
        .iter()
        .filter_map(|tn| match &tn.kind {
            NodeKind::Image { src } if !src.is_empty() => Some(crate::asset::AssetEntry {
                path: src.clone(),
                w: 0,
                h: 0,
            }),
            _ => None,
        })
        .collect();
    let manifest_paths: Vec<String> = manifest.iter().map(|e| e.path.clone()).collect();
    let input = PackageInput {
        components: vec![("scene", nodes.as_slice(), &dynamic, &[])],
        asset_manifest: &manifest,
    };
    (crate::asset::write_package(&input), manifest_paths)
}

/// 黄金等价（最强门）：inline 渲染 == 包渲染。
///
/// load_package 进资源池不建 scene，包路径走
/// `load_package → instantiate("scene") → append_child → render`，与 inline 路径
/// （load_inline_for_test → render）渲染输出逐字等价对比。证明 instantiate 克隆子树 +
/// 挂载后几何/样式与 inline 同构（零回归）。
#[test]
fn package_load_renders_identical_to_inline() {
    let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
    let html = r#"<div class="c"><span>hi</span><img src="logo.png"></div>"#;
    let css = ".c{width:200px;height:100px;overflow:hidden;background-color:#ff0000;}";

    // inline 路径（test-only helper，保留 parse→scene 管线验证）
    let mut s_inline = Stage::new((200.0, 100.0)).unwrap();
    s_inline
        .register_font("DejaVu", std::fs::read(font_path).unwrap(), true)
        .unwrap();
    s_inline.load_inline_for_test(html, css).unwrap();
    let inline_json = s_inline.render_json();

    // 包路径：load_package → instantiate("scene") → 挂为 scene 根 → render。
    // inline 路径把 .c div 作 scene 根；包路径 instantiate 返回孤立根，直接 push 进
    // scene.roots（同 create_root 语义），不套额外 stage_root——保证两路径节点树同构。
    let (pkg_bytes, _) = pkg_bytes_from_inline(html, css);
    let mut s_pkg = Stage::new((200.0, 100.0)).unwrap();
    s_pkg
        .register_font("DejaVu", std::fs::read(font_path).unwrap(), true)
        .unwrap();
    s_pkg.load_package("bag", &pkg_bytes).unwrap();
    // ensure_scene（首次建空骨架）+ instantiate 返回孤立根 → push 进 scene.roots 作场景根
    s_pkg.ensure_scene();
    let comp_root = s_pkg.instantiate("bag", "scene").unwrap();
    s_pkg.scene.as_mut().unwrap().roots.push(comp_root);
    let pkg_json = s_pkg.render_json();

    assert_eq!(
        inline_json, pkg_json,
        "包路径渲染输出必须 == inline（instantiate 克隆子树等价）"
    );
}

/// load_package → instantiate → :hover 重匹配验证。
/// 按钮 + :hover 规则打成包，instantiate 后 Move 到按钮 → RollOver + 伪类重匹配变蓝。
#[cfg(feature = "parse")]
#[test]
fn set_input_hover_emits_rollover_and_rematch() {
    let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
    let html = r#"<div class="root"><button class="btn">OK</button></div>"#;
    let css = r#".btn { width: 100px; height: 50px; background-color: #cccccc; } .btn:hover { background-color: #0000ff; }"#;
    let (pkg_bytes, _) = pkg_bytes_from_inline(html, css);

    let mut s = Stage::new((200.0, 100.0)).unwrap();
    s.register_font("DejaVu", std::fs::read(font_path).unwrap(), true)
        .unwrap();
    s.load_package("bag", &pkg_bytes).unwrap();
    // 包路径 instantiate 返回孤立根（div.root）→ push 进 scene.roots 作场景根（同 inline 语义）
    s.ensure_scene();
    let comp_root = s.instantiate("bag", "scene").unwrap();
    s.scene.as_mut().unwrap().roots.push(comp_root);
    // comp_root = div.root；btn = root 的首个 button 子（gather_rec 把 <button>OK</button> 建成 Button + auto Text 子）
    // warmup tick：compute_world_transforms 在 process/scroll 后跑，hit_test 读上帧
    // world_transforms（1 帧延迟语义）。首帧 world_transforms 空 → 首帧 hit_test 全 None。
    s.tick_and_render();
    // btn = comp_root 的首个 button 子（gather_rec 把 <button>OK</button> 建成 Button + auto Text 子）
    let btn_id = {
        let sc = s.scene.as_ref().unwrap();
        *sc.get(comp_root)
            .unwrap()
            .children
            .iter()
            .find(|&&c| matches!(sc.get(c).unwrap().kind, NodeKind::Button))
            .unwrap()
    };
    // Move 到按钮 (50,25)（按钮在 (0,0,100,50)）
    s.set_input(&[crate::input::PointerEvent {
        kind: crate::input::PointerKind::Move,
        x: 50.0,
        y: 25.0,
        button: 0,
        pad: [0, 0],
        touch_id: -1,
    }]);
    s.tick_and_render();
    let events = s.last_events();
    assert!(
        events
            .iter()
            .any(|e| e.event_type == crate::input::EVT_ROLL_OVER),
        "Move 到按钮 → RollOver"
    );
    assert!(s.is_pointer_on_ui(), "命中按钮 → is_pointer_on_ui=true");
    // hover 后 rematch：btn style.background_color 应变蓝（dynamic 规则 .btn:hover）
    let scene = s.scene.as_ref().unwrap();
    let btn = scene.get(btn_id).unwrap();
    assert_eq!(
        btn.style.background_color,
        Some([0.0, 0.0, 1.0, 1.0]),
        ":hover 伪类重匹配 → 蓝"
    );
}

/// load_package → instantiate → disabled 抑制 click。
/// 按钮打成包，instantiate 后 set_node_disabled(true) → Down+Up 不产 Click。
#[cfg(feature = "parse")]
#[test]
fn set_node_disabled_inhibits_click() {
    let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
    let html = r#"<div class="root"><button class="btn">OK</button></div>"#;
    let css = r#".btn { width: 100px; height: 50px; }"#;
    let (pkg_bytes, _) = pkg_bytes_from_inline(html, css);

    let mut s = Stage::new((200.0, 100.0)).unwrap();
    s.register_font("DejaVu", std::fs::read(font_path).unwrap(), true)
        .unwrap();
    s.load_package("bag", &pkg_bytes).unwrap();
    s.ensure_scene();
    let comp_root = s.instantiate("bag", "scene").unwrap();
    s.scene.as_mut().unwrap().roots.push(comp_root);
    // btn = comp_root 的首个 Button 子
    let btn_id = {
        let sc = s.scene.as_ref().unwrap();
        *sc.get(comp_root)
            .unwrap()
            .children
            .iter()
            .find(|&&c| matches!(sc.get(c).unwrap().kind, NodeKind::Button))
            .unwrap()
    };
    s.set_node_disabled(btn_id, true);
    // warmup tick（同 hover 测：hit_test 1 帧延迟，首帧 world_transforms 空）
    s.tick_and_render();
    // 命中前置：Move 到按钮 → is_pointer_on_ui=true（证明按钮被几何命中，disabled 才有抑制对象）
    s.set_input(&[crate::input::PointerEvent {
        kind: crate::input::PointerKind::Move,
        x: 50.0,
        y: 25.0,
        button: 0,
        pad: [0, 0],
        touch_id: -1,
    }]);
    s.tick_and_render();
    assert!(
        s.is_pointer_on_ui(),
        "Move 到按钮 → 命中 UI（命中前置：证明按钮被几何命中）"
    );
    // Down + Up 在按钮上——disabled 不产 Click
    s.set_input(&[
        crate::input::PointerEvent {
            kind: crate::input::PointerKind::Down,
            x: 50.0,
            y: 25.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        },
        crate::input::PointerEvent {
            kind: crate::input::PointerKind::Up,
            x: 50.0,
            y: 25.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        },
    ]);
    s.tick_and_render();
    let events = s.last_events();
    assert!(
        !events
            .iter()
            .any(|e| e.event_type == crate::input::EVT_CLICK),
        "disabled → 不产 Click"
    );
}

#[test]
fn is_pointer_on_ui_false_when_miss() {
    // 空 scene / 命中根外 → false。手搓 Stage（不走 parse）
    let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
    let mut s = Stage::new((200.0, 100.0)).unwrap();
    s.register_font("DejaVu", std::fs::read(font_path).unwrap(), true)
        .unwrap();
    // 手搓空 scene（SlotMap::with_key()）
    s.scene = Some(crate::scene::node::Scene {
        roots: vec![],
        nodes: slotmap::SlotMap::with_key(),
        dynamic_rules: Default::default(),
        focused_node: None,
        world_transforms: Vec::new(),
        anim: Default::default(),
        scroll: Default::default(),
        text_layouts: Vec::new(),
        rich_fragments: Vec::new(),
        node_sort_keys: Vec::new(),
        controllers: Default::default(),
        pending_controller_events: Vec::new(),
        pending_transitions: Vec::new(),
    });
    s.set_input(&[crate::input::PointerEvent {
        kind: crate::input::PointerKind::Move,
        x: 50.0,
        y: 50.0,
        button: 0,
        pad: [0, 0],
        touch_id: -1,
    }]);
    s.tick_and_render();
    assert!(!s.is_pointer_on_ui(), "空 scene → false");
}

/// load 时 scroll 表清空（防 reload 后容器 NodeId 悬空，同 tween clear）。
/// 塞 scroll_pos 后 reload → scroll 表为空（get 返 None）；重新 ensure 后归零。
#[cfg(feature = "parse")]
#[test]
fn load_clears_scroll_state() {
    let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
    let html = r#"<div class="c"></div>"#;
    let css = ".c{width:200px;height:100px;overflow:scroll;}";
    let mut s = Stage::new((200.0, 100.0)).unwrap();
    s.register_font("DejaVu", std::fs::read(font_path).unwrap(), true)
        .unwrap();
    s.load_inline_for_test(html, css).unwrap();
    let root_id = s.scene.as_ref().unwrap().roots[0];
    // 手动塞 scroll_pos，模拟上一会话残留
    s.scene.as_mut().unwrap().scroll.ensure(root_id).scroll_pos = (50.0, 50.0);
    // reload → scroll 表应被清
    s.load_inline_for_test(html, css).unwrap();
    assert!(
        s.scene.as_ref().unwrap().scroll.get(root_id).is_none(),
        "reload 后 scroll 表清空，NodeId 槽不存在"
    );
}

/// tween 经 Stage 公共 API 注册 → advance_time stash dt → tick update 写 anim + 产 complete。
/// 注：.b 是 CSS class 不是 id 属性，find_node_by_id("b") 返 None。div.b 是唯一根节点。
#[test]
fn stage_tween_advances_opacity_and_emits_complete() {
    let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
    let html = r#"<div class="b"></div>"#;
    let css = ".b{width:100px;height:50px;}";
    let mut s = Stage::new((200.0, 100.0)).unwrap();
    s.register_font("DejaVu", std::fs::read(font_path).unwrap(), true)
        .unwrap();
    s.load_inline_for_test(html, css).unwrap();
    let rid = s.scene.as_ref().unwrap().roots[0];
    // opacity 0→1，1s Linear，tag=99
    s.tween(
        rid,
        crate::tween::TweenProp::Opacity,
        [0.0, 0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0, 0.0],
        crate::tween::Ease::Linear,
        0.0,
        1.0,
        99,
    );
    s.advance_time(0.5);
    s.tick_and_render();
    let op = s
        .scene
        .as_ref()
        .unwrap()
        .anim
        .0
        .get(&rid)
        .and_then(|a| a.opacity);
    assert!((op.unwrap() - 0.5).abs() < 1e-4, "半程 opacity=0.5");
    assert!(
        s.last_events()
            .iter()
            .all(|e| e.event_type != crate::input::EVT_TWEEN_COMPLETE),
        "未结束"
    );
    s.advance_time(0.5);
    s.tick_and_render();
    assert!(
        s.last_events()
            .iter()
            .any(|e| e.event_type == crate::input::EVT_TWEEN_COMPLETE && e.touch_id == 99),
        "结束 → complete(tag=99)"
    );
}

/// 直接 tick_and_render()（不 advance_time）→ pending_dt=0。
/// 用 delay=1.0 注册 tween：elapsed(0) < delay(1) → update 跳过 apply，opacity 保持 None。
/// 验证 tween 集成对「不 advance_time」的现有 stage 调用模式无副作用。
#[test]
fn stage_tick_without_advance_time_is_zero_regression() {
    let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
    let mut s = Stage::new((200.0, 100.0)).unwrap();
    s.register_font("DejaVu", std::fs::read(font_path).unwrap(), true)
        .unwrap();
    s.load_inline_for_test(r#"<div class="b"></div>"#, ".b{width:100px;height:50px;}")
        .unwrap();
    let rid = s.scene.as_ref().unwrap().roots[0];
    // delay=1.0：dt=0 时 elapsed=0 < delay → 不 apply（若用 delay=0，update 会写 start 值）
    s.tween(
        rid,
        crate::tween::TweenProp::Opacity,
        [0.0, 0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0, 0.0],
        crate::tween::Ease::Linear,
        1.0,
        1.0,
        0,
    );
    s.tick_and_render(); // 无 advance_time → dt=0 → elapsed < delay → 不推进
    assert!(
        !s.scene.as_ref().unwrap().anim.0.contains_key(&rid),
        "dt=0 不写 override（HashMap 无条目）"
    );
}

/// tween 写读对称回归：tween 写 scene.anim（用 id.index()）→ render 读 anim.opacity
/// （AnimTable::get 用 node.index()）→ frame.nodes[该节点].alpha 吃到 override。
/// 堵「tween 写入正确但 render 读取失败」盲区——确保写读 key 一致，
/// 越界 index 会让 anim override 在渲染层丢失 → alpha 退回 CSS 默认 1.0。
#[test]
fn tween_anim_override_visible_in_render_output() {
    let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
    let mut s = Stage::new((200.0, 100.0)).unwrap();
    s.register_font("DejaVu", std::fs::read(font_path).unwrap(), true)
        .unwrap();
    s.load_inline_for_test(r#"<div class="b"></div>"#, ".b{width:100px;height:50px;}")
        .unwrap();
    let rid = s.scene.as_ref().unwrap().roots[0];
    // tween opacity 0→0.5，delay=0、duration=1.0、Linear。
    s.tween(
        rid,
        crate::tween::TweenProp::Opacity,
        [0.0, 0.0, 0.0, 0.0],
        [0.5, 0.0, 0.0, 0.0],
        crate::tween::Ease::Linear,
        0.0,
        1.0,
        0,
    );
    // 推进整段 duration → tt=1.0 → Linear 插值末值 0.5。
    s.advance_time(1.0);
    let frame = s.tick_and_render();
    // 唯一根节点 → frame.nodes[pos=0]。断言 render 输出吃到 anim override（alpha=0.5），
    // 不是只断言 anim 表内值——确保读写对称贯穿到渲染层。
    assert!(
        (frame.nodes[0].alpha - 0.5).abs() < 1e-5,
        "tween anim.opacity override 应在 render 输出可见：alpha={}（期望 0.5）",
        frame.nodes[0].alpha
    );
}

/// 拖拽滚动容器 → 同 tick world_transforms 已含 scroll_pos（零延迟）。
/// process 写 scroll_pos（drag_follow）→ compute_world_transforms 在 process 后读 scroll_pos
/// → world matrix 含 T(-scroll_pos) offset。
#[cfg(feature = "parse")]
#[test]
fn drag_follow_visible_same_frame_in_world_transforms() {
    use crate::transform::Affine2Ext;
    let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
    let html = r#"<div class="scroll"><div class="content"></div></div>"#;
    let css = r#".scroll{width:200px;height:200px;overflow:scroll;} .content{width:50px;height:400px;flex-shrink:0;}"#;
    let mut s = Stage::new((200.0, 200.0)).unwrap();
    s.register_font("DejaVu", std::fs::read(font_path).unwrap(), true)
        .unwrap();
    s.load_inline_for_test(html, css).unwrap();
    // 首 tick 建立 layout + content_size/overlap
    s.tick_and_render();
    // feed 拖拽输入（mouse touch_id=-1，dy=20 > SCROLL_THRESHOLD_MOUSE=8）
    s.advance_time(0.016);
    s.set_input(&[
        crate::input::PointerEvent {
            kind: crate::input::PointerKind::Down,
            x: 25.0,
            y: 25.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        },
        crate::input::PointerEvent {
            kind: crate::input::PointerKind::Move,
            x: 25.0,
            y: 45.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        },
    ]);
    s.tick_and_render();
    // 子节点 world.apply 反映 scroll_pos（非 0）
    let scene = s.scene.as_ref().unwrap();
    // content 子 = root 的首个子；drag 下拖 → scroll_pos.y 减（越界打折负值）→ world y 反映（≠0）
    let content_id = scene.get(scene.roots[0]).unwrap().children[0];
    let (_x, y) = scene.world_transforms[content_id.index()].apply_point(0.0, 0.0);
    assert!(y != 0.0, "拖拽同帧进 world matrix：y={}", y);
}

/// 支柱1：rematch 提到 solve/compute 前 → :active{scale} 当帧 world 即含缩放。
/// 若 compute 在 rematch 前 → 当帧 world 无 scale（仍是 identity）——本测钉住正确顺序。
/// 走包路径（load_package → instantiate），因为 dynamic_rules 由打包器提取进 scene。
#[test]
fn active_scale_visible_same_frame() {
    let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
    let html = r#"<div id="b" class="btn">x</div>"#;
    let css = ".btn{width:100px;height:100px;} .btn:active{transform:scale(0.5);}";
    let (pkg_bytes, _) = pkg_bytes_from_inline(html, css);

    let mut stage = Stage::new((200.0, 200.0)).expect("stage");
    stage
        .register_font("DejaVu", std::fs::read(font_path).unwrap(), true)
        .unwrap();
    stage.load_package("bag", &pkg_bytes).unwrap();
    stage.ensure_scene();
    let comp_root = stage.instantiate("bag", "scene").unwrap();
    stage.scene.as_mut().unwrap().roots.push(comp_root);
    // warmup tick：hit_test 读上帧 world_transforms（1 帧延迟，首帧空）
    stage.tick_and_render();
    // 找到 btn NodeId（comp_root 就是 .btn div）
    let bid = {
        let scene = stage.scene.as_ref().unwrap();
        scene
            .nodes
            .values()
            .find(|n| n.id_attr.as_deref() == Some("b"))
            .unwrap()
            .id
    };
    // Feed Down 事件触发 active 状态（PointerState::recompute_active 设置 active=true）
    // 注意：warmup tick 已算 world_transforms，hit_test 可命中。
    stage.set_input(&[crate::input::PointerEvent {
        kind: crate::input::PointerKind::Down,
        x: 50.0,
        y: 50.0,
        button: 0,
        pad: [0, 0],
        touch_id: -1,
    }]);
    // 本帧 tick：process 设 active → rematch 应在 compute 前生效 → world m_a=0.5，非 identity(1.0)
    stage.tick_and_render();
    let scene = stage.scene.as_ref().unwrap();
    let wm = scene.world_transforms[bid.index()];
    assert!(
        (wm[0] - 0.5).abs() < 1e-3,
        "active scale 当帧进 world：m_a=0.5，实={}",
        wm[0]
    );
}

/// compute_world_transforms 在 render 前每帧跑（每帧一次，非末尾+首帧 guard）。
/// tick 后 world_transforms 应非空——证明 compute 在 render 前执行过。
#[cfg(feature = "parse")]
#[test]
fn tick_computes_world_transforms_before_render() {
    let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
    let mut s = Stage::new((200.0, 200.0)).unwrap();
    s.register_font("DejaVu", std::fs::read(font_path).unwrap(), true)
        .unwrap();
    s.load_inline_for_test(r#"<div class="c"></div>"#, ".c{width:100px;height:50px;}")
        .unwrap();
    s.tick_and_render();
    // tick 后 world_transforms 应非空（compute 在 render 前跑过）
    assert!(
        !s.scene.as_ref().unwrap().world_transforms.is_empty(),
        "compute_world_transforms 在 render 前跑过"
    );
}

/// hit_test 在 world_transforms 空/未对齐时不 panic（bounds guard 拦截）。
/// 结构变更帧新增节点本帧 world_transforms 未算 → 未命中（1 帧延迟语义），不越界 panic。
#[test]
fn hit_test_bounds_guard_no_panic_on_empty_worlds() {
    let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
    let mut s = Stage::new((200.0, 200.0)).unwrap();
    s.register_font("DejaVu", std::fs::read(font_path).unwrap(), true)
        .unwrap();
    // 手搓 scene：1 个 touchable 节点（root，覆盖点 50,50）但 world_transforms 空。
    // hit_subtree 走到 bounds guard（id.index() >= world_transforms.len()）→ 返 None，不 panic。
    use crate::scene::node::{Node, NodeKind, Rect, Scene};
    use crate::style::resolved::ResolvedStyle;
    let mut root = Node::default();
    root.kind = NodeKind::Container;
    root.style = ResolvedStyle::default();
    root.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 200.0,
    };
    root.touchable = true;
    let scene = Scene::from_nodes(vec![root], vec![]);
    s.scene = Some(scene);
    // world_transforms 空（未 compute）→ hit_test bounds guard 应拦截，不 panic，返 None
    let hit = crate::hit::hit_test(s.scene.as_ref().unwrap(), (50.0, 50.0));
    assert_eq!(
        hit, None,
        "world_transforms 空 → bounds guard 返 None（未命中，1 帧延迟语义）"
    );
}

/// remove_node 后 tick_and_render 不 panic（容量化并行数组防越界）。
/// 删中间节点产生 slotmap 间隙 → 高 idx live 节点 id.index() > nodes.len()。
/// 若 world_transforms/taffy_ids/text_layouts 按存活数(len)分配 → 越界 panic。
/// 按 capacity+1 分配 → 间隙安全。此测验证整条管线（solve+compute+render）不崩。
#[cfg(feature = "parse")]
#[test]
fn remove_node_then_tick_does_not_panic_on_slot_gap() {
    let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
    // 4 节点：root + 3 子（a, b, c），删 b（中间）→ a/c 仍 live，c 在高 idx。
    let html = r#"<div class="root"><div class="a"></div><div class="b"></div><div class="c"></div></div>"#;
    let css = ".root{width:200px;height:200px;} .a,.b,.c{width:50px;height:50px;}";
    let mut s = Stage::new((200.0, 200.0)).unwrap();
    s.register_font("DejaVu", std::fs::read(font_path).unwrap(), true)
        .unwrap();
    s.load_inline_for_test(html, css).unwrap();
    s.tick_and_render(); // 首帧：建 world_transforms 基线
                         // 取 b 的 NodeId（root 的第 2 个 div 子——注意 root 的 Text 子不在这里，3 个 div 子直接挂 root）
    let scene = s.scene.as_ref().unwrap();
    let root_id = scene.roots[0];
    let div_kids: Vec<_> = scene
        .get(root_id)
        .unwrap()
        .children
        .iter()
        .filter(|&&c| {
            matches!(
                scene.get(c).unwrap().kind,
                crate::scene::node::NodeKind::Container
            )
        })
        .copied()
        .collect();
    assert_eq!(div_kids.len(), 3, "3 个 div 子");
    let b_id = div_kids[1];
    // 删 b（中间子）→ slotmap 间隙
    s.remove_node(b_id);
    // tick + render：solve/compute_world_transforms/build_render_nodes 全跑，不应越界 panic
    s.tick_and_render();
    // b 已失效（NodeId 失效），a/c 仍 live
    let scene = s.scene.as_ref().unwrap();
    assert!(scene.get(b_id).is_none(), "b 删除后 NodeId 失效");
    assert!(scene.get(div_kids[0]).is_some(), "a 仍 live");
    assert!(
        scene.get(div_kids[2]).is_some(),
        "c 仍 live（高 idx，间隙后仍可索引）"
    );
    // 再 tick 一帧确认稳定（world_transforms 已按新容量重算）
    s.tick_and_render();
}

/// transition 请求 → Stage tick drain 后提交 tween。
/// hover btn（base_style 声明 transition: background-color 0.3s linear）→
/// warmup tick 置 cascaded_once=true → Move 到 btn → tick → rematch 检测 bg 变化推请求 →
/// drain kill 旧 tween（无）+ 提交新 tween（BgColor 0→1, 0.3s）。
/// 断言：Stage::tweens 有 1 个 active（非 killed）tween for (btn, BgColor)。
#[cfg(feature = "parse")]
#[test]
fn transition_request_becomes_tween() {
    let (mut s, btn_id) = transition_stage();
    // warmup tick：置 cascaded_once=true（首次 cascade 即时生效不动画）+ 建 world_transforms
    // 基线（hit_test 1 帧延迟语义，首帧 world 空 → 首帧 hit_test 全 None）。
    s.tick_and_render();
    // Move 到按钮 (50,25)（按钮在 (0,0,100,50)）→ hover 状态置位 → tick 后 rematch 检测
    // bg 0→1 变化（cascaded_once=true）→ 推 transition 请求 → drain 提交 tween。
    s.set_input(&[crate::input::PointerEvent {
        kind: crate::input::PointerKind::Move,
        x: 50.0,
        y: 25.0,
        button: 0,
        pad: [0, 0],
        touch_id: -1,
    }]);
    s.tick_and_render();
    // 断言：Stage::tweens 有 1 个 active tween for (btn, BgColor)
    let active: Vec<_> = s
        .tweens
        .tweens
        .iter()
        .filter(|t| {
            !t.killed && t.node == btn_id && matches!(t.prop, crate::tween::TweenProp::BgColor)
        })
        .collect();
    assert_eq!(
        active.len(),
        1,
        "hover transition → 1 active BgColor tween for btn"
    );
    // tween 末值 = hover 白 [1,1,1,1]（#ffffff）
    let t = active[0];
    assert!(
        (t.end[0] - 1.0).abs() < 1e-5 && (t.end[3] - 1.0).abs() < 1e-5,
        "tween end = #ffffff"
    );
    // tag = TRANSITION_TAG（0xFFFF_FFFE，区分 driver 提交的 tag）
    assert_eq!(t.tag, 0xFFFF_FFFE, "transition tween tag = TRANSITION_TAG");
}

/// transition 中段换向：kill 旧 tween + 从 mid-flight override 提交新 tween（无 snap）。
/// hover → tween A（bg 0→1, 0.3s linear）。advance 0.1s → A mid-flight（bg≈0.33）。
/// un-hover（Move 走）→ rematch 检测 bg 1→0 变化推请求（start=anim override≈0.33, end=0）→
/// drain kill A（override 保留 0.33）+ 提交 B（start=0.33, end=0, 0.3s）。
/// 断言：B 的 start ≈ 0.33（mid-flight 连续，无 snap 回 0）；anim.bg_color 在 resubmit 当帧 ≈ 0.33。
#[cfg(feature = "parse")]
#[test]
fn mid_transition_rechange_kills_and_continues() {
    let (mut s, btn_id) = transition_stage();
    // warmup tick：置 cascaded_once=true + 建 world 基线
    s.tick_and_render();
    // 1) hover → tick → tween A 提交（bg 0→1, 0.3s linear）
    s.set_input(&[crate::input::PointerEvent {
        kind: crate::input::PointerKind::Move,
        x: 50.0,
        y: 25.0,
        button: 0,
        pad: [0, 0],
        touch_id: -1,
    }]);
    s.advance_time(0.0); // hover 当帧 dt=0（tween 已注册但未推进）
    s.tick_and_render();
    // 2) advance 0.1s → tick → A 推进 0.1s（mid-flight，norm=0.333, bg≈0.333）
    s.advance_time(0.1);
    s.tick_and_render();
    let mid_bg = s
        .scene
        .as_ref()
        .unwrap()
        .anim
        .0
        .get(&btn_id)
        .and_then(|a| a.bg_color)
        .map(|c| c[0])
        .unwrap_or(0.0);
    assert!(
        (mid_bg - 0.3333).abs() < 0.02,
        "A mid-flight bg≈0.333，实得 {:.4}",
        mid_bg
    );
    // 3) un-hover（Move 走到按钮外）→ tick → rematch 检测 bg 变化推请求 →
    //    drain kill A（override 保留 ≈0.333）+ 提交 B（start≈0.333, end=0）
    s.set_input(&[crate::input::PointerEvent {
        kind: crate::input::PointerKind::Move,
        x: 150.0, // 按钮外
        y: 75.0,
        button: 0,
        pad: [0, 0],
        touch_id: -1,
    }]);
    s.advance_time(0.0); // resubmit 当帧 dt=0（仅 drain，不推进 B）
    s.tick_and_render();
    // 断言：B 已提交（1 active tween for (btn, BgColor)），且 B 的 start ≈ mid-flight 值
    let active: Vec<_> = s
        .tweens
        .tweens
        .iter()
        .filter(|t| {
            !t.killed && t.node == btn_id && matches!(t.prop, crate::tween::TweenProp::BgColor)
        })
        .collect();
    assert_eq!(active.len(), 1, "drain 后仅 1 active tween（B；A 被 kill）");
    let b = active[0];
    assert!(
        (b.start[0] - mid_bg).abs() < 0.02,
        "B start = mid-flight override ({:.4})，无 snap 回 0；实得 {:.4}",
        mid_bg,
        b.start[0]
    );
    assert!(
        (b.end[0] - 0.0).abs() < 1e-5,
        "B end = base bg（0），实得 {:.4}",
        b.end[0]
    );
    // anim.bg_color 当帧 ≈ mid_bg（kill 保留 override，新 tween dt=0 未推进 → 仍 mid-flight 值）
    let resubmit_bg = s
        .scene
        .as_ref()
        .unwrap()
        .anim
        .0
        .get(&btn_id)
        .and_then(|a| a.bg_color)
        .map(|c| c[0])
        .unwrap_or(0.0);
    assert!(
        (resubmit_bg - mid_bg).abs() < 0.02,
        "resubmit 当帧 anim.bg_color ≈ mid-flight ({:.4})，无 snap；实得 {:.4}",
        mid_bg,
        resubmit_bg
    );
    // 4) A 被 kill（killed=true）。drain 在 tweens.update 之后跑（update 在 tick ①，drain 在 ⑥.5），
    //    故 A 的 killed 条目本轮 retain 未清（retain 在 update 末尾跑）——下轮 update 才清出。
    //    验：A 标 killed（不再 active），B 是唯一 active tween（已在 active.len()==1 断言）。
    assert!(
        s.tweens.tweens.iter().any(|t| t.node == btn_id && t.killed),
        "A 标 killed（retain 清出延至下轮 update，本帧仍驻留但非 active）"
    );
    // 下轮 update（advance+tick）→ A 被 retain 清出 → 仅 B active
    s.advance_time(0.0);
    s.tick_and_render();
    assert!(
        !s.tweens.tweens.iter().any(|t| t.node == btn_id && t.killed),
        "下轮 update 后 A 被 retain 清出（无 killed 残留）"
    );
    let active_after: Vec<_> = s
        .tweens
        .tweens
        .iter()
        .filter(|t| {
            !t.killed && t.node == btn_id && matches!(t.prop, crate::tween::TweenProp::BgColor)
        })
        .collect();
    assert_eq!(
        active_after.len(),
        1,
        "下轮 update 后仅 B active（A 已清出）"
    );
}

/// 共享脚手架：建 transition 测试用 Stage（包路径 load+instantiate，含 :hover 动态规则）。
/// btn：100×50 在 (0,0)，base bg=#000000，:hover bg=#ffffff，transition: background-color 0.3s linear。
/// 返回 (Stage, btn_id)。caller 须先 warmup tick（置 cascaded_once + 建 world 基线）。
#[cfg(feature = "parse")]
fn transition_stage() -> (Stage, crate::scene::node::NodeId) {
    let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
    let html = r#"<div class="root"><button class="btn">OK</button></div>"#;
    let css = r#".btn { width: 100px; height: 50px; background-color: #000000; }
                .btn:hover { background-color: #ffffff; }
                .btn { transition: background-color 0.3s linear; }"#;
    let (pkg_bytes, _) = pkg_bytes_from_inline(html, css);
    let mut s = Stage::new((200.0, 100.0)).unwrap();
    s.register_font("DejaVu", std::fs::read(font_path).unwrap(), true)
        .unwrap();
    s.load_package("bag", &pkg_bytes).unwrap();
    s.ensure_scene();
    let comp_root = s.instantiate("bag", "scene").unwrap();
    s.scene.as_mut().unwrap().roots.push(comp_root);
    // btn = comp_root 的首个 Button 子（gather_rec 把 <button>OK</button> 建成 Button + auto Text 子）
    let btn_id = {
        let sc = s.scene.as_ref().unwrap();
        *sc.get(comp_root)
            .unwrap()
            .children
            .iter()
            .find(|&&c| matches!(sc.get(c).unwrap().kind, NodeKind::Button))
            .unwrap()
    };
    (s, btn_id)
}

#[test]
#[cfg(feature = "parse")]
fn stage_register_font_sets_default_for_measure() {
    let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
    let mut s = Stage::new((200.0, 200.0)).expect("stage");
    s.register_font("DejaVu", std::fs::read(font_path).unwrap(), true)
        .unwrap();
    let tree = crate::parse::dom::parse_html("<div>Hello</div>").unwrap();
    let sheet = crate::parse::css::parse_css("").unwrap();
    let styles = crate::style::cascade::resolve_styles(&tree, &sheet);
    s.scene = Some(crate::scene::node::build_scene(&tree, &styles));
    s.advance_time(0.016);
    let _frame = s.tick_and_render(); // must not panic on "no default font"
}

/// tick_and_render 每帧写回 rich_fragments 时清空本帧无 fragments 的 slot。
/// 若只写不清（bug），上一帧有链接、本帧 set_rich_text 删了链接的节点会保留 stale fragments，
/// rich_link_at 读到已删 link_id。
#[test]
#[cfg(feature = "parse")]
fn tick_and_render_clears_stale_rich_fragments() {
    use crate::text::rich::RichFragment;
    let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
    // 最小有效场景：display:block div → load_inline_for_test 不跑 desugar 故仍是 Container。
    // 测试聚焦 writeback 清空逻辑，不依赖 build 产 fragments 的完整链路。
    let html = r#"<div style="display:block;width:100px;height:20px">text</div>"#;
    let css = "";
    let mut s = Stage::new((200.0, 100.0)).unwrap();
    s.register_font("DejaVu", std::fs::read(font_path).unwrap(), true)
        .unwrap();
    s.load_inline_for_test(html, css).unwrap();
    let root = s.scene.as_ref().unwrap().roots[0];
    // 首帧 tick 建 taffy / world_transforms 基线
    s.tick_and_render();
    // 注入 stale fragments——模拟上一帧 build 产了 fragments 且已写回 rich_fragments
    {
        let sc = s.scene.as_mut().unwrap();
        let idx = root.index();
        sc.rich_fragments = vec![None; sc.nodes.capacity() + 1];
        sc.rich_fragments[idx] = Some(vec![RichFragment {
            x: 10.0,
            y: 5.0,
            w: 50.0,
            h: 20.0,
            link_id: 99,
        }]);
    }
    // 本帧该节点不产 fragments（Container 无 link run）→ writeback 应清空 slot
    s.tick_and_render();
    let sc = s.scene.as_ref().unwrap();
    assert!(
        sc.rich_fragments[root.index()].is_none(),
        "tick_and_render 应清空本帧无 fragments 的 slot（stale fragments 残留）"
    );
}
