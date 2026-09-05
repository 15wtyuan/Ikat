//! 诊断（#82：列方向 flex-wrap 容器内子项高度逐帧动画，某一帧误判「超高换列」）：
//! 自包含最小复现——类规则给 row+wrap，inline 覆写 flex-direction:column（wrap 留存），
//! 面板 height transition 0→200px。逐帧 tick 扫描：列宽 != 320 或面板 x 偏移即异常帧。
//! 环境变量：DUR（transition 秒数，默认 0.4）。浏览器同结构不误换列（A/B 已证），
//! 异常出自 taffy 布局缓存路径。
//!
//! 用法：cargo run -p yio_core --example dump_layout_anim_jitter [--offline]

use yio_core::scene::dynamic::append_child;
use yio_core::stage::Stage;
use yio_pkg::build::{pack_components, Component};

fn main() {
    let dur: f32 = std::env::var("DUR")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.4);
    // HTML 环境变量：指定页面源文件（默认取 git 历史里的改版前 layout-anim 页——
    // 真复现锚点；最小化复现时换内联模板）。
    let default_html = r#"<style>
  .outer { display:flex; flex-direction:row; flex-wrap:wrap; align-items:center; gap:28px; padding:18px 22px; min-height:92px; }
  .inner { display:flex; flex-direction:row; flex-wrap:wrap; gap:10px; align-items:flex-start; min-height:92px; }
  .panel { width:320px; height:0px; transition:height DURs; overflow:hidden; background-color:#1a2f45; }
  .panel.open { height:200px; }
</style>
<div class="outer">
  <div class="inner" style="flex-direction:column">
    <button>go</button>
    <div class="panel"></div>
  </div>
</div>"#
    .replace("DUR", &dur.to_string());
    let html = std::env::var("HTML")
        .ok()
        .map(|p| std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {p}: {e}")))
        .unwrap_or(default_html);
    let pkg = pack_components(&[Component {
        name: "wraprepro".to_owned(),
        src: html,
        html_rel: "wraprepro.html".to_owned(),
    }])
    .expect("pack")
    .bytes;

    let mut s = Stage::new((1920.0, 1080.0)).expect("Stage::new");
    // CJK 字体（旧页按钮/标题是中文文本，measure 需同字体才同尺寸）。
    let root = env!("CARGO_MANIFEST_DIR");
    let font_default = std::fs::read(format!(
        "{root}/../../unity/showcase-unity/Assets/Bundles/fonts/LXGWWenKai.ttf.bytes"
    ))
    .expect("LXGWWenKai");
    let font_fallback = std::fs::read(format!(
        "{root}/../../unity/showcase-unity/Assets/Bundles/fonts/wqy-microhei.ttc.bytes"
    ))
    .expect("wqy");
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
    // 落定 cascade。
    s.advance_time(1.0 / 60.0);
    let _ = s.tick_and_render();

    // 定位：优先 id=fold-body（原页面），否则 class=panel（最小模板）。inner=其父。
    let (panel, inner) = {
        let scene = s.scene.as_ref().unwrap();
        let panel = scene
            .find_by_id_attr("fold-body")
            .or_else(|| {
                scene
                    .nodes
                    .iter()
                    .find(|(_, n)| n.classes.iter().any(|c| c == "panel"))
                    .map(|(_, n)| n.id)
            })
            .expect("panel");
        let inner = scene.get(panel).and_then(|n| n.parent).expect("inner");
        (panel, inner)
    };

    // 基线帧（应稳定 0px）。
    for _ in 0..2 {
        s.advance_time(1.0 / 60.0);
        let _ = s.tick_and_render();
    }

    // 触发：与 C# driver 同语义（Classes.Add("open")）。
    s.add_class(panel, "open").expect("add_class open");
    let frames = (dur * 60.0 * 1.3) as i32 + 6;
    println!("frame  t(ms)   panel(x,y,w,h)                 anim_h     inner(x,y,w,h)");
    let mut anomalies = 0usize;
    for i in 0..frames {
        s.advance_time(1.0 / 60.0);
        let _ = s.tick_and_render();
        let scene = s.scene.as_ref().unwrap();
        let p = scene.get(panel).unwrap();
        let c = scene.get(inner).unwrap();
        let anim_h = scene
            .anim
            .get(panel)
            .and_then(|a| a.height.as_ref())
            .map(|l| format!("{:.2}", l.value))
            .unwrap_or_else(|| "-".into());
        // 异常判定：列宽不再是 max(按钮,面板)=320（换列时 = 按钮+gap+面板），
        // 或面板 x 偏离列左缘（被排到按钮右边）。
        let bad = (c.layout_rect.w - 320.0).abs() > 0.5
            || (p.layout_rect.x - c.layout_rect.x).abs() > 0.5;
        if bad {
            anomalies += 1;
        }
        println!(
            "{:<4}   {:>5.0}   {:>7.2},{:<7.2}{:>7.2},{:<7.2}  {:>8}   {:>7.2},{:<7.2}{:>7.2},{:<7.2}{}",
            i,
            (i as f32 + 1.0) / 60.0 * 1000.0,
            p.layout_rect.x,
            p.layout_rect.y,
            p.layout_rect.w,
            p.layout_rect.h,
            anim_h,
            c.layout_rect.x,
            c.layout_rect.y,
            c.layout_rect.w,
            c.layout_rect.h,
            if bad { "   <-- ANOMALY" } else { "" },
        );
    }
    println!("anomaly frames: {anomalies}/{frames}");
}
