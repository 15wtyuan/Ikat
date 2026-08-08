//! 诊断 Bug #2（home 渐隐入场只命中几个 + 时序错）：
//! 实例化 showcase home，dump 每张 nav-card 的 resolved `style.animation` 声明：
//!   - 每张 card 几条 AnimationSpec（1=级联正确取胜者；2=级联把 base + nth-child 都并进来了）
//!   - 每条 delay（应为 0.05s 的整数倍 stagger）
//!   - card 在父级元素子里的 nth-child 位置（验 :nth-child(N) 匹配的 N 是否对得上）

use loomgui_core::scene::dynamic::append_child;
use loomgui_core::scene::node::NodeKind;
use loomgui_core::stage::Stage;

fn main() {
    let root = env!("CARGO_MANIFEST_DIR");
    let pkg_path = format!(
        "{}/../../unity/showcase-unity/Assets/Bundles/ui/showcase.pkg.bin",
        root
    );
    let font_default = format!(
        "{}/../../unity/showcase-unity/Assets/Bundles/fonts/LXGWWenKai.ttf.bytes",
        root
    );
    let font_fallback = format!(
        "{}/../../unity/showcase-unity/Assets/Bundles/fonts/wqy-microhei.ttc.bytes",
        root
    );

    let pkg = std::fs::read(&pkg_path).expect("read pkg");
    let mut s = Stage::new((1920.0, 1080.0)).expect("Stage::new");
    s.register_font("LXGWWenKai", std::fs::read(&font_default).unwrap(), true)
        .unwrap();
    s.register_font(
        "wqy-microhei",
        std::fs::read(&font_fallback).unwrap(),
        false,
    )
    .unwrap();
    s.set_fallback_families(&["wqy-microhei".to_string()]);
    s.set_image_sizes(&icon_sizes());
    s.load_package("showcase", &pkg).expect("load_package");
    let root_id = s.create_root("div", "").expect("create_root");
    let inst = s.instantiate("showcase", "home").expect("instantiate home");
    {
        let scene = s.scene.as_mut().unwrap();
        append_child(scene, root_id, inst).expect("append_child");
    }
    // 跑一帧让 cascade + sync_animation_players 落定（rematch 把 animation 写进 style）。
    let _frame = s.tick_and_render();
    let scene = s.scene.as_ref().unwrap();

    println!("========== home nav-card 入场动画 resolved 声明 ==========");
    println!(
        "{:<8} {:<14} {:<10} {:<14} {:<10}",
        "nthChild", "id_attr", "#specs", "name/dur/delay/fill", "iter"
    );

    for n in scene.nodes.values() {
        let is_card = n.classes.iter().any(|c| c == "nav-card");
        if !is_card {
            continue;
        }
        // 计算 nth-child 位置：父级元素子里它是第几个（1-based，只数元素子，与 CSS 匹配器一致）。
        let nth = n
            .parent
            .and_then(|p| scene.get(p))
            .map(|p| {
                p.children
                    .iter()
                    .filter(|&&c| scene.get(c).is_some_and(|cn| cn.kind != NodeKind::TextNode))
                    .position(|c| *c == n.id)
                    .map(|i| i + 1)
                    .unwrap_or(0)
            })
            .unwrap_or(0);

        let anims = &n.style.animation;
        let id_attr = n.id_attr.clone().unwrap_or_default();
        if anims.is_empty() {
            println!(
                "{:<8} {:<14} {:<10} (no animation declared)",
                nth, id_attr, 0
            );
            continue;
        }
        for (i, a) in anims.iter().enumerate() {
            let prefix = if i == 0 {
                format!("{:<8} {:<14} {:<10}", nth, id_attr, anims.len())
            } else {
                format!("{:<8} {:<14} {:<10}", "", "", "")
            };
            println!(
                "{} {:<22} iter={:?}",
                prefix,
                format!(
                    "{} dur={:.2}s delay={:.3}s fill={:?}",
                    a.name, a.duration, a.delay, a.fill_mode
                ),
                a.iteration_count
            );
        }
    }

    // 顺带 dump NodeAnim（运行时动画状态）确认 player 是否建起来。
    println!();
    println!("========== scene.anim（运行时 NodeAnim）==========");
    for n in scene.nodes.values() {
        if !n.classes.iter().any(|c| c == "nav-card") {
            continue;
        }
        let id_attr = n.id_attr.clone().unwrap_or_default();
        match scene.anim.get(n.id) {
            Some(_) => println!("  nav-card {:<14} → HAS NodeAnim", id_attr),
            None => println!("  nav-card {:<14} → no NodeAnim", id_attr),
        }
    }
}

fn icon_sizes() -> Vec<(String, u32, u32)> {
    let names = [
        "logo",
        "settings",
        "inventory",
        "mail",
        "shop",
        "character",
        "form",
        "lab",
        "item-chest",
        "weapon",
        "item-gem",
        "skill-eye",
        "item-scroll",
        "item-staff",
    ];
    names
        .iter()
        .map(|n| (format!("res/icons/{}.png", n), 128, 128))
        .collect()
}
