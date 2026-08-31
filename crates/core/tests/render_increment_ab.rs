//! A2 render build 增量的 A/B 对拍安全网（#109）。
//!
//! 增量 render 的正确性风险是「输入枚举不完备 = 陈旧帧」——goldens 抓不住列语义、
//! 单测验不了跨帧。此处同一场景 + 同一 mutation 脚本跑两个 Stage：一个开增量
//! （指纹命中复用上帧产物），一个关增量（每帧全量重建），**逐帧 render_json 必须
//! 逐字节全等**。任何漏枚举的输入通道（改了输出但没进指纹）都会在此现形。
//!
//! 脚本覆盖的通道即指纹的输入清单验收面：稳态 / set_text / inline 改色 / transform
//! 平移 / 祖先 opacity 级联 / display:none 结构翻转 / tween 动画帧 / 字体重注册。

use ikat_core::stage::Stage;

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
            ikat_core::scene::dynamic::set_user_transform(s.scene.as_mut().unwrap(), body, t)
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
                ikat_core::tween::TweenSpec {
                    prop: ikat_core::tween::TweenProp::BgColor,
                    start: [0.13, 0.27, 0.4, 1.0, 0.0, 0.0, 0.0, 0.0],
                    end: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
                    ease: ikat_core::tween::Ease::Linear,
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
