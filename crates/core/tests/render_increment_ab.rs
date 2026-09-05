//! A2 render build 增量的 A/B 对拍安全网（#109）。
//!
//! 增量 render 的正确性风险是「输入枚举不完备 = 陈旧帧」——goldens 抓不住列语义、
//! 单测验不了跨帧。此处同一场景 + 同一 mutation 脚本跑两个 Stage：一个开增量
//! （指纹命中复用上帧产物），一个关增量（每帧全量重建），**逐帧 render_json 必须
//! 逐字节全等**。任何漏枚举的输入通道（改了输出但没进指纹）都会在此现形。
//!
//! 脚本覆盖的通道即指纹的输入清单验收面：稳态 / set_text / inline 改色 / transform
//! 平移 / 祖先 opacity 级联 / display:none 结构翻转 / tween 动画帧 / 字体重注册。

use yio_core::stage::Stage;

fn font_bytes() -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/DejaVuSans.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("DejaVuSans.ttf fixture must exist")
}

/// 场景：root > (header 容器带背景 + 文本) + (body 容器带背景 + 两段文本) + 阴影节点。
fn make_stage() -> Stage {
    let mut s = Stage::new((800.0, 600.0)).unwrap();
    s.register_font("dejavu", font_bytes(), true).unwrap();
    let root = s.create_root("div", "width:800px;height:600px").unwrap();
    let header = s
        .create_node("div", "width:800px;height:80px;background-color:#224466")
        .unwrap();
    s.append_child(root, header).unwrap();
    let title = s
        .create_node("span", "font-size:24px;color:#ffffff")
        .unwrap();
    s.set_text(title, "Inventory").unwrap();
    s.append_child(header, title).unwrap();
    let body = s
        .create_node("div", "width:800px;height:520px;background-color:#112233")
        .unwrap();
    s.append_child(root, body).unwrap();
    let line1 = s.create_node("span", "font-size:16px").unwrap();
    s.set_text(line1, "hello world hello world").unwrap();
    s.append_child(body, line1).unwrap();
    let line2 = s
        .create_node("span", "font-size:16px;color:#ffcc00")
        .unwrap();
    s.set_text(line2, "second line").unwrap();
    s.append_child(body, line2).unwrap();
    let shadow = s
        .create_node(
            "div",
            "width:200px;height:40px;box-shadow:0 4px 8px #00000080",
        )
        .unwrap();
    s.append_child(body, shadow).unwrap();
    s
}

/// 双 Stage 对拍：`script(f, stage)` 在第 f 帧前施加 mutation（两 Stage 同脚本）。
/// 每帧后 render_json 全等断言。
fn ab_compare(frames: usize, script: impl Fn(usize, &mut Stage)) {
    let mut inc = make_stage();
    let mut full = make_stage();
    full.incremental_render = false;
    for f in 0..frames {
        script(f, &mut inc);
        script(f, &mut full);
        let a = inc.render_json();
        let b = full.render_json();
        assert_eq!(
            a, b,
            "frame {f}: 增量与全量输出必须逐字节全等（陈旧 = 指纹漏通道）"
        );
    }
}

/// 稳态 + 各突变通道逐帧对拍（单脚本串行覆盖，任何一帧红即指认通道）。
#[test]
fn incremental_matches_full_rebuild_across_mutation_script() {
    ab_compare(10, |f, s| match f {
        0 | 1 => {} // 0: 首建全量；1: 稳态（应全 Skip）
        2 => {
            // set_text：文本几何变
            let body = s.scene.as_ref().unwrap().roots[0];
            let line1 = s.scene.as_ref().unwrap().get(body).unwrap().children[0];
            let line1 = s.scene.as_ref().unwrap().get(line1).unwrap().children[0];
            s.set_text(line1, "mutated text content").unwrap();
        }
        3 => {}
        4 => {
            // inline 改色（rematch 值比较路径 → render_input_version）
            let root = s.scene.as_ref().unwrap().roots[0];
            let header = s.scene.as_ref().unwrap().get(root).unwrap().children[0];
            s.set_inline_override(header, "background-color:#aa0000")
                .unwrap();
        }
        5 => {}
        6 => {
            // transform 平移（wm 变 → 指纹含矩阵全量）
            let root = s.scene.as_ref().unwrap().roots[0];
            let body = s.scene.as_ref().unwrap().get(root).unwrap().children[1];
            let mut t = s.scene.as_ref().unwrap().get(body).unwrap().user_transform;
            t.translate[1] += 40.0;
            yio_core::scene::dynamic::set_user_transform(s.scene.as_mut().unwrap(), body, t)
                .unwrap();
        }
        7 => {}
        8 => {
            // 祖先 opacity 级联（累积 alpha 进指纹）
            let root = s.scene.as_ref().unwrap().roots[0];
            s.set_inline_override(root, "opacity:0.5").unwrap();
        }
        9 => {
            // display:none 结构翻转（present-set 签名 → 缓存整表清空兜底）
            let root = s.scene.as_ref().unwrap().roots[0];
            let body = s.scene.as_ref().unwrap().get(root).unwrap().children[1];
            s.set_inline_override(body, "display:none").unwrap();
        }
        _ => unreachable!(),
    });
}

/// tween 动画帧对拍：bg_color / opacity 补间逐帧推进（anim hash 通道）。
#[test]
fn incremental_matches_full_rebuild_during_tween() {
    ab_compare(6, |f, s| {
        if f == 0 {
            let root = s.scene.as_ref().unwrap().roots[0];
            let header = s.scene.as_ref().unwrap().get(root).unwrap().children[0];
            s.tween(
                header,
                yio_core::tween::TweenSpec {
                    prop: yio_core::tween::TweenProp::BgColor,
                    start: [0.13, 0.27, 0.4, 1.0, 0.0, 0.0, 0.0, 0.0],
                    end: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
                    ease: yio_core::tween::Ease::Linear,
                    delay: 0.0,
                    duration: 0.1,
                    tag: 0,
                    repeat: 0,
                    yoyo: false,
                    shadow: None,
                },
            );
        }
        s.advance_time(0.016);
    });
}

/// 字体重注册（宿主代数钩）：同 stage 上重注册字体 → 全文本失效重测 → 输出与
/// 全量重建路径一致（res_gen 全局失效通道）。
#[test]
fn incremental_matches_full_after_host_registry_change() {
    let mut inc = make_stage();
    let mut full = make_stage();
    full.incremental_render = false;
    // 预热两帧（建立缓存与 hash 基线）。
    let _ = inc.render_json();
    let _ = full.render_json();
    let _ = inc.render_json();
    let _ = full.render_json();
    inc.register_font("dejavu", font_bytes(), true).unwrap();
    full.register_font("dejavu", font_bytes(), true).unwrap();
    assert_eq!(inc.render_json(), full.render_json());
}

/// 稳态帧全命中断言：无控件的场景，第 3 帧重建数 = 0（全部指纹命中）。
#[test]
fn steady_state_frame_is_all_cache_hits() {
    let mut s = make_stage();
    let _ = s.render_json(); // 首建
    let _ = s.render_json(); // 第二帧：hash 基线对齐
    s.render_cache.hits = 0;
    s.render_cache.misses = 0;
    let _ = s.render_json(); // 稳态帧
    assert_eq!(s.render_cache.misses, 0, "稳态帧零重建");
    assert!(s.render_cache.hits > 0, "稳态帧全部命中");
}

/// render_hidden（世界锚点出屏自动隐藏的 core 面）：开 → 全部渲染行 visible=0，
/// 关 → 恢复；布局/命中不受影响（与 display:none 正交）。A/B 双路同步一致。
#[test]
fn render_hidden_toggles_visibility_without_layout_change() {
    let mut inc = make_stage();
    let mut full = make_stage();
    full.incremental_render = false;
    let _ = inc.render_json();
    let _ = full.render_json();
    let base_layout = {
        let s = inc.scene.as_ref().unwrap();
        let root = s.roots[0];
        s.get(root).unwrap().layout_rect
    };

    let body = {
        let s = inc.scene.as_ref().unwrap();
        let root = s.roots[0];
        s.get(root).unwrap().children[1]
    };
    inc.set_node_render_hidden(body, true).unwrap();
    full.set_node_render_hidden(body, true).unwrap();

    let j1 = inc.render_json();
    let j2 = full.render_json();
    assert_eq!(j1, j2, "隐藏帧 A/B 全等");
    assert!(j1.contains("\"visible\": false"), "渲染行 visible=0 进输出");
    // 布局不动（display:none 会塌掉高度——render_hidden 不得）。
    let after_layout = {
        let s = inc.scene.as_ref().unwrap();
        let root = s.roots[0];
        s.get(root).unwrap().layout_rect
    };
    assert_eq!(base_layout, after_layout, "render_hidden 不影响布局");

    inc.set_node_render_hidden(body, false).unwrap();
    full.set_node_render_hidden(body, false).unwrap();
    assert_eq!(inc.render_json(), full.render_json(), "恢复帧 A/B 全等");
}

/// render_hidden 继承（CSS visibility:hidden 语义）：隐藏祖先 → 整子树全部渲染行
/// visible=0（世界锚点隐藏的是整棵锚定子树——血条 = 容器 + fill + 文字，只藏容器
/// 背景会留「子节点裸奔」半隐态）。藏 root = 全帧无任何可见行；恢复即整树复明。
#[test]
fn render_hidden_is_inherited_down_the_subtree() {
    let mut inc = make_stage();
    let mut full = make_stage();
    full.incremental_render = false;
    let _ = inc.render_json();
    let _ = full.render_json();

    let root = {
        let s = inc.scene.as_ref().unwrap();
        s.roots[0]
    };
    inc.set_node_render_hidden(root, true).unwrap();
    full.set_node_render_hidden(root, true).unwrap();
    let j1 = inc.render_json();
    let j2 = full.render_json();
    assert_eq!(j1, j2, "整树隐藏帧 A/B 全等（继承传播进指纹）");
    assert!(
        !j1.contains("\"visible\": true"),
        "祖先隐藏必须传播整子树（增量路不得留陈旧可见行）"
    );

    inc.set_node_render_hidden(root, false).unwrap();
    full.set_node_render_hidden(root, false).unwrap();
    let j3 = inc.render_json();
    assert!(j3.contains("\"visible\": true"), "恢复后子树重新可见");
}

/// world-space 挂载（#109 C8）：挂载子树行顶点/矩阵 re-base 到挂载根局部系 + blob
/// mount_id 标注。A/B 双路全等（增量缓存行存 re-base 后形态）；挂载翻转改变输出
///（≠挂载前），解除回屏幕空间 = 逐字节回到挂载前（往返闭合）。
#[test]
fn world_mount_rebases_subtree_rows_round_trip() {
    let mut inc = make_stage();
    let mut full = make_stage();
    full.incremental_render = false;
    let _ = inc.render_json();
    let _ = full.render_json();
    let j_before = inc.render_json();

    let body = {
        let s = inc.scene.as_ref().unwrap();
        let root = s.roots[0];
        s.get(root).unwrap().children[1]
    };
    inc.set_node_mount(body, 7).unwrap();
    full.set_node_mount(body, 7).unwrap();

    let j1 = inc.render_json();
    let j2 = full.render_json();
    assert_eq!(j1, j2, "挂载帧 A/B 全等（re-base 进指纹与缓存形态）");
    assert_ne!(j1, j_before, "挂载必须改写行坐标（re-base 生效）");
    assert!(
        j1.contains("\"mount_root_id\": 7"),
        "挂载子树行带 mount_root_id 标注"
    );

    // 同根换槽（重绑另一 3D 容器）：归属/原点不变、唯槽位变——指纹必须失效旧行
    //（漏了会回放旧槽行，后端路由错容器）。
    inc.set_node_mount(body, 9).unwrap();
    full.set_node_mount(body, 9).unwrap();
    let j3 = inc.render_json();
    assert_eq!(j3, full.render_json(), "换槽帧 A/B 全等");
    assert!(
        j3.contains("\"mount_root_id\": 9") && !j3.contains("\"mount_root_id\": 7"),
        "换槽后行全部携带新槽位（旧槽行不得回放）"
    );

    // 挂载根移动（inline override 高度变化 → 根 layout 位变 → 原点变）后 A/B 仍全等。
    inc.set_inline_override(body, "height:460px").unwrap();
    full.set_inline_override(body, "height:460px").unwrap();
    assert_eq!(inc.render_json(), full.render_json(), "根移动帧 A/B 全等");
    inc.unset_inline_override(body, "height").unwrap();
    full.unset_inline_override(body, "height").unwrap();

    inc.set_node_mount(body, 0).unwrap();
    full.set_node_mount(body, 0).unwrap();
    let j_after = inc.render_json();
    // change_level/reuse_key 是帧相对定级（挂载周期后行重建 → Full/新 key），几何往返
    // 比较须剥离这两个字段行，其余逐行全等。
    let strip = |j: &str| -> String {
        j.lines()
            .filter(|l| !l.contains("\"change_level\"") && !l.contains("\"reuse_key\""))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(
        strip(&j_after),
        strip(&j_before),
        "解除挂载 = 回到屏幕空间形态（几何往返闭合）"
    );
}

/// merged 批非锚成员变更必须 Full（A/B 对拍结构性盲区：双 Stage 同用 ph_reuse，同错
/// 则同错、逐字节仍全等——须断言 change_level 本身）。merged 行 payload = 批内多行
/// concat，锚自身单行 hash 不代表批内容：改批内第二个成员的宽度，锚指纹不变，唯有
/// 成员 ph 拼合的批 hash 能捕捉 → 该帧必须出现 Full 行（错误实现会全 Skip → 旧 mesh）。
#[test]
fn merged_batch_member_change_upgrades_to_full() {
    let mut s = Stage::new((800.0, 600.0)).unwrap();
    s.register_font("dejavu", font_bytes(), true).unwrap();
    let root = s.create_root("div", "width:800px;height:600px").unwrap();
    // 两个同 DrawState 相邻 solid div → merge 成单行（锚 = 较小 node id = 第一个）。
    let a = s
        .create_node("div", "width:100px;height:50px;background-color:#334455")
        .unwrap();
    s.append_child(root, a).unwrap();
    let b = s
        .create_node("div", "width:100px;height:50px;background-color:#335544")
        .unwrap();
    s.append_child(root, b).unwrap();

    let _ = s.render_json(); // 首建（全 Full）
    let j_steady = s.render_json(); // 稳态：全 Skip
    assert!(
        !j_steady.contains("\"change_level\": \"Full\""),
        "稳态帧应全 Skip（前置自检）"
    );

    // 改非锚成员 b 的宽度：merged 批内容变 → 该行必须 Full。
    s.set_inline_override(b, "width:160px").unwrap();
    let j = s.render_json();
    assert!(
        j.contains("\"change_level\": \"Full\""),
        "merged 批非锚成员变更必须升 Full（Skip = 成员几何丢失上屏）"
    );
}
