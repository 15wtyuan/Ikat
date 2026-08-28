//! #82 换列误触发守卫：列方向 flex-wrap 容器内子项高度动画，任何一帧不得误换列。
//!
//! taffy 0.12 缺陷（#82 根因，2026-08-27 调研批定案）：wrap 容器在 sizing 测量轮里
//! 拿到的 Definite 主轴可用空间被当作换行约束（`collect_flex_lines` 不区分「容器主
//! 尺寸显式 definite」与「祖先测量轮传来的 Definite」），换列测量结果经节点缓存复用
//! 污染终态布局——同输入不同帧出不同终态（showcase layout-anim 页实拍：37 帧里 1 帧
//! 面板被排到按钮右侧、列宽 320→386 闪一帧；浏览器同结构 0 异常）。
//!
//! **taffy 0.14 升级（2026-08-28）未修掉本缺陷**——0.14 的 `collect_flex_lines` 门控
//! （主尺寸不 definite 则单行）对本路径空洞放行：误换列轮里 `known_dimensions=None`，
//! 而 definiteness 归一化是 `is_definite || known.is_none()`（known 缺席视为 definite）；
//! 且容器无 max-height 时换列约束直接取 available 的 Definite——该值恰等于内容总高
//! （30.34+10+194.6779=235.0179 > 235.01788，超 2 ulp）触发换列。auto 主尺寸 wrap
//! 容器按 CSS 不应对 available 换列（Chrome 同结构 0 异常）。危害已收窄：面板位置
//! 正确（0.12 是面板跳到按钮右侧），仅容器宽 386 污染一帧（后续兄弟右移 66px 一帧）。
//!
//! 本测试锁定的是**最终布局不得闪烁**：内容宽度全程恒定、面板始终贴列左缘。
//! 在 0.12 与 0.14 上均红（#[ignore] 挂账），上游修复或围栏规避后转绿入库防回归。
//!
//! 复现条件刻意保持与现场一致、不做任何「干净化」：
//! - 走完整管线（fence → pkg → Stage → 逐帧 tick）——独立 taffy mini-repro 不复现，
//!   flicker 依赖真实管线的完整测量轮组合；
//! - 用 showcase 同款 CJK 字体（LXGWWenKai 主 + wqy-microhei 备）——按钮高度落在
//!   特定值是触发边界的一部分（font-size:14px 触发、默认字号不触发，#82 二分实录）；
//! - 三要素：column+wrap 容器（类规则 row+wrap、inline 只覆写 direction）+ min-height
//!   （.stage 类 92px）+ 滚动祖先（.body overflow-y:auto 触发 min-content 测量）。
//!
//! 字体从 unity/showcase-unity 读取（仓库入库文件，CI 可用；#38 仓库瘦身若挪字体
//! 路径须同步此处与 dump_layout_anim_jitter example）。

use ikat_core::scene::dynamic::append_child;
use ikat_core::stage::Stage;
use ikat_pkg::build::{pack_components, Component};

/// 与 dump_layout_anim_jitter example / 改版前 layout-anim 页第 1 节同构的最小模板。
/// 2026-08-28 实测：0.12 上 37 帧第 19 帧（t=333ms，anim height 194.68）误换列，
/// 与真实页逐位一致——这是锚点，勿改动模板与字体组合（改了触发边界会漂）。
const REPRO_HTML: &str = r#"<style>
  .root {display:flex;flex-direction:column; width:1920px; height:1080px; }
  .body {display:flex;flex-direction:column; flex-grow:1; padding:32px 56px; gap:22px; overflow-y:auto; }
  .section {display:flex;flex-direction:column; padding:22px; gap:12px; }
  .stage {display:flex; flex-direction:row; gap:28px; align-items:center; flex-wrap:wrap; padding:18px 22px; min-height:92px; }
  .panel-body { width:320px; height:0px; transition:height .4s; overflow:hidden; }
  .panel-body.open { height:200px; }
</style>
<div class="root">
  <div class="body">
    <div class="section">
      <div class="stage">
        <div class="stage" style="padding:0;flex-direction:column;align-items:flex-start;gap:10px">
          <button id="btn-fold" style="font-size:14px;padding:6px 14px">展开</button>
          <div class="panel-body" id="fold-body"></div>
        </div>
      </div>
    </div>
  </div>
</div>"#;

const DUR: f32 = 0.4;

#[test]
#[ignore = "复现锚点在 taffy 0.12 与 0.14 上都红（#82）：0.14 残留缺陷——auto 主尺寸 wrap 容器对 available Definite 换列（collect_flex_lines 无 max-size 回退路径 + known=None 时 known_main_size_is_definite 空洞为 true），待上游修复/围栏规避后转绿"]
fn column_wrap_height_anim_no_flicker() {
    let pkg = pack_components(&[Component {
        name: "wraprepro".to_owned(),
        src: REPRO_HTML.to_owned(),
        html_rel: "wraprepro.html".to_owned(),
    }])
    .expect("pack")
    .bytes;

    let mut s = Stage::new((1920.0, 1080.0)).expect("Stage::new");
    let root = env!("CARGO_MANIFEST_DIR");
    let font_default = std::fs::read(format!(
        "{root}/../../unity/showcase-unity/Assets/Bundles/fonts/LXGWWenKai.ttf.bytes"
    ))
    .expect("LXGWWenKai（入库字体，见模块注释——#38 挪路径须同步）");
    let font_fallback = std::fs::read(format!(
        "{root}/../../unity/showcase-unity/Assets/Bundles/fonts/wqy-microhei.ttc.bytes"
    ))
    .expect("wqy-microhei");
    s.register_font("LXGWWenKai", font_default, true).unwrap();
    s.register_font("wqy-microhei", font_fallback, false)
        .unwrap();
    s.set_fallback_families(&["wqy-microhei".to_string()]);
    s.load_package("wraprepro", &pkg).expect("load_package");
    let root_id = s.create_root("div", "").expect("create_root");
    let inst = s
        .instantiate("wraprepro", "wraprepro")
        .expect("instantiate");
    {
        let scene = s.scene.as_mut().unwrap();
        append_child(scene, root_id, inst).expect("append_child");
    }
    // 落定 cascade + 基线帧（面板 0px 应稳定）。
    for _ in 0..3 {
        s.advance_time(1.0 / 60.0);
        let _ = s.tick_and_render();
    }

    let (panel, inner) = {
        let scene = s.scene.as_ref().unwrap();
        let panel = scene
            .find_by_id_attr("fold-body")
            .expect("panel #fold-body");
        let inner = scene.get(panel).and_then(|n| n.parent).expect("inner");
        (panel, inner)
    };

    // 触发：与 C# driver 同语义（Classes.Add("open")）。
    s.add_class(panel, "open").expect("add_class open");
    let frames = (DUR * 60.0 * 1.3) as i32 + 6;
    let mut anomalies: Vec<(i32, [f32; 4], [f32; 4])> = Vec::new();
    let mut last_height = 0.0f32;
    for i in 0..frames {
        s.advance_time(1.0 / 60.0);
        let _ = s.tick_and_render();
        let scene = s.scene.as_ref().unwrap();
        let p = scene.get(panel).unwrap().layout_rect;
        let c = scene.get(inner).unwrap().layout_rect;
        last_height = p.h;
        // 正确语义：列容器宽度 = max(按钮, 面板) = 320 全程恒定；面板 x 贴列左缘。
        // 误换列时宽度跳到 按钮+gap+面板（~386）且面板 x 偏到按钮右侧——闪一帧即违规。
        let bad = (c.w - 320.0).abs() > 0.5 || (p.x - c.x).abs() > 0.5;
        if bad {
            anomalies.push((i, [p.x, p.y, p.w, p.h], [c.x, c.y, c.w, c.h]));
        }
    }
    assert!(
        anomalies.is_empty(),
        "height 动画期间 wrap 容器出现换列闪烁帧（frame, panel_rect, inner_rect）：{anomalies:?}"
    );
    // transition 终点精确落位（动画引擎侧独立守卫，顺带钉住）。
    assert!(
        (last_height - 200.0).abs() <= 0.01,
        "transition 终点应为 200px，实测 {last_height}"
    );
}
