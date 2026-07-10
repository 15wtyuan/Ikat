//! 打包器库：系统目录多 HTML → .pkg.bin（无 atlas）。
//! 每个 HTML 独立 parse → resolve_styles → build_scene → 抽 TemplateNode；
//! img src / background-image url 相对 html 文件解析成 workspace-root-relative sprite_key；
//! CSS bake 进 style_blob。图集归 Unity，打包器不产 atlas。

// type_complexity：打包器 build_scene 抽 TemplateNode 的返回类型天然是多层 Vec/HashMap 嵌套，
// 拆 alias 跨函数引用反而更难读。
#![allow(clippy::type_complexity)]

pub mod atlas;
pub mod build;
pub mod resolve;
pub mod runtime;
pub mod workspace;

use loomgui_core::asset::{extract_component_css, ControllerEntry, PackageInput, TemplateNode};
use loomgui_core::scene::NodeId;
use scraper::{Html, Selector as ScraperSelector};
use std::path::Path;

/// 打包产物：.pkg.bin bytes + referenced_sprites（本包所有 img/bg/rich-img 引用到的
/// sprite_key，去重，workspace-root-relative。图集归 Unity，打包器不产 atlas。）
#[derive(Debug)]
pub struct PackedPackage {
    pub pkg_bytes: Vec<u8>,
    /// 本包所有 img/bg/rich-img 引用到的 sprite_key（去重），供 build 交叉验证。
    pub referenced_sprites: Vec<String>,
}

/// 把单 scene 转 Vec<TemplateNode>（按 slotmap 插入序 = DFS 先序），同时把 img src +
/// background-image url + rich 行内图 src 经 `resolve_img_src` 解析成 workspace-root-relative
/// sprite_key，去重收进 `referenced`，并回写 src。
///
/// 解析失败（越出 workspace root）→ Err（引用根外的图是错误，不静默跳过）。
///
/// parent_idx = 父节点在 Vec 中的位置（None=组件根）。slotmap values() 对无删除的全新 map
/// 按槽位序迭代 = 插入序 = build_scene 的 DFS 先序，故 parent 总在 child 前出现，位置索引稳定。
///
/// 同时扫 `Node.data_controller` 收 `ControllerEntry`：mount_node_idx = 节点在产物 Vec 中的
/// 位置（同 parent_idx 约定），initial_selected_index 从 `controller_pages` 查（打包期扫
/// `data-page` 属性得，key = controller name）。
fn scene_to_template(
    scene: &loomgui_core::scene::Scene,
    workspace_root: &Path,
    html_file: &Path,
    referenced: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
    controller_pages: &std::collections::HashMap<String, i32>,
) -> Result<(Vec<TemplateNode>, Vec<ControllerEntry>), String> {
    // NodeId → 在产物 Vec 中的位置（slotmap 插入序）。
    let pos_of: std::collections::HashMap<NodeId, usize> = scene
        .nodes
        .values()
        .enumerate()
        .map(|(i, n)| (n.id, i))
        .collect();

    let mut nodes: Vec<TemplateNode> = Vec::with_capacity(scene.nodes.len());
    let mut controllers: Vec<ControllerEntry> = Vec::new();
    for (i, n) in scene.nodes.values().enumerate() {
        // img src 解析成 sprite_key（去重）。回写节点 src。
        let mut kind = n.kind.clone();
        if let loomgui_core::scene::NodeKind::Image { src } = &mut kind {
            if !src.is_empty() {
                let key = crate::resolve::resolve_img_src(workspace_root, html_file, src)?;
                if seen.insert(key.clone()) {
                    referenced.push(key.clone());
                }
                *src = key;
            }
        }
        // background-image url 同样解析成 sprite_key（去重；与 img src 同 url 只入一次）。
        let mut style = n.style.clone();
        if let Some(url) = &style.background_image {
            if !url.is_empty() {
                let key = crate::resolve::resolve_img_src(workspace_root, html_file, url)?;
                if seen.insert(key.clone()) {
                    referenced.push(key.clone());
                }
                style.background_image = Some(key);
            }
        }
        // RichText 行内图 src 解析成 sprite_key（去重）。
        if let loomgui_core::scene::NodeKind::RichText { runs } = &mut kind {
            for r in runs.iter_mut() {
                if let loomgui_core::text::rich::RichKind::Image { src, .. } = &mut r.kind {
                    if !src.is_empty() {
                        let key = crate::resolve::resolve_img_src(workspace_root, html_file, src)?;
                        if seen.insert(key.clone()) {
                            referenced.push(key.clone());
                        }
                        *src = key;
                    }
                }
            }
        }
        nodes.push(TemplateNode {
            kind,
            style,
            parent_idx: n.parent.map(|p| pos_of[&p]),
            classes: n.classes.clone(),
            id_attr: n.id_attr.clone(),
            draggable: n.draggable,
            tabindex: n.tabindex,
            data_controller: n.data_controller.clone(),
        });
        // data-controller="name" 的节点 → ControllerEntry。
        if let Some(name) = &n.data_controller {
            let initial = controller_pages.get(name).copied().unwrap_or(0);
            controllers.push(ControllerEntry {
                name: name.clone(),
                mount_node_idx: i as u32,
                initial_selected_index: initial,
            });
        }
    }
    Ok((nodes, controllers))
}

/// 扫 ElementTree 收 controller name → initial_selected_index 映射。
/// 仅 data-controller 元素同时带 data-page 属性时记录其解析值（i32，缺省/非数字 → 0）。
/// data-controller 无 data-page → 不进 map（scene_to_template 查不到时默认 0）。
fn collect_controller_pages(
    tree: &loomgui_core::parse::dom::ElementTree,
) -> std::collections::HashMap<String, i32> {
    let mut map = std::collections::HashMap::new();
    for el in &tree.nodes {
        if let Some(name) = el.attrs.get("data-controller") {
            let initial = el
                .attrs
                .get("data-page")
                .and_then(|v| v.parse::<i32>().ok())
                .unwrap_or(0);
            map.insert(name.clone(), initial);
        }
    }
    map
}

/// 从 HTML 串剥掉所有 `<style>...</style>` 和 `<link ...>` 元素（含内容），返回干净 HTML。
/// parse_html 的围栏白名单（div/span/img/button）拒绝 `<style>`/`<link>`，故打包器在
/// 调 parse_html 前先抽 CSS（extract_component_css）再剥这俩 tag。用 scraper 重构文档树
/// 后序列化回 HTML 字符串（与 parse_html 同后端，保证语义一致）。
fn strip_style_and_link(html: &str) -> String {
    let document = Html::parse_document(html);
    let mut out = String::with_capacity(html.len());
    // 遍历 body 子树，跳过 style/link 节点，重新拼 HTML。
    let body_sel = ScraperSelector::parse("body").unwrap();
    if let Some(body) = document.select(&body_sel).next() {
        serialize_children(&body, &mut out);
    } else {
        // 无 body（scraper 对无 html/head 包裹的片段会合成）→ 退回原串（让 parse_html 报错）。
        return html.to_string();
    }
    out
}

/// 递归序列化元素的子节点（跳过 style/link），拼回 HTML 字符串。
fn serialize_children(el: &scraper::ElementRef, out: &mut String) {
    for child in el.children() {
        match child.value() {
            scraper::node::Node::Text(t) => {
                // t.text 是 scraper 解码后的文本（源码 `&lt;i&gt;` → `<i>`）。裸写回会让二次
                // parse_html 把解码出的 `<` 当标签起点 → 误判为元素子节点（行内混排假报错）。
                // 须重新转义 `&` `<` `>`，保证 round-trip 语义一致。
                escape_text_into(&t.text, out);
            }
            scraper::node::Node::Element(e) => {
                // 跳过 style/link（CSS 已由 extract_component_css 抽走）。
                if e.name() == "style" || e.name() == "link" {
                    continue;
                }
                if let Some(eref) = scraper::ElementRef::wrap(child) {
                    out.push('<');
                    out.push_str(e.name());
                    for (k, v) in e.attrs() {
                        out.push(' ');
                        out.push_str(k);
                        out.push_str("=\"");
                        escape_attr_into(v, out);
                        out.push('"');
                    }
                    out.push('>');
                    serialize_children(&eref, out);
                    out.push_str("</");
                    out.push_str(e.name());
                    out.push('>');
                }
            }
            _ => {}
        }
    }
}

/// 文本节点转义：`&` 先（避二次转义）、`<`、`>`。scraper 已把实体解码成裸字符，
/// 序列化回 HTML 必须重新转义，否则二次解析把 `<` 当标签起点（行内混排假报错）。
fn escape_text_into(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
}

/// 属性值转义：`&` 先、`"`（双引号包裹内须转义）、`<`/`>`（一并转义求稳）。
fn escape_attr_into(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
}

/// v1.7 desugar：inline `display:block` div 的 raw_rich → parse_rich_markup → runs。
///
/// 在 `resolve_styles` 后、`build_scene` 前调。对每个 raw_rich 非空的元素：
/// 1. 守护栏（spec §4.2）：block div 拒 flex 属性（justify-content/align-items/gap）——
///    block div 是富文本叶而非 flex 容器，写这些属性无意义且 AI 不可预测。
/// 2. base 样式从 ResolvedStyle 转 RichBaseStyle（color/font_size/weight/style/deco）。
/// 3. parse_rich_markup(raw, base, 0) → runs，存进 ElementData.rich_runs。
/// 4. build_scene 据 rich_runs 产 NodeKind::RichText。
///
/// `(tree, styles)` 同长同序不变量保持：desugar 只填 ElementData.rich_runs，
/// 不增删节点、不改 styles 顺序（block div 的 taffy_style 仍 Flex，layout 照常）。
pub fn desugar_block_divs(
    mut tree: loomgui_core::parse::dom::ElementTree,
    styles: Vec<loomgui_core::style::resolved::ResolvedStyle>,
) -> Result<
    (
        loomgui_core::parse::dom::ElementTree,
        Vec<loomgui_core::style::resolved::ResolvedStyle>,
    ),
    String,
> {
    use loomgui_core::style::resolved::DisplayMode;
    use loomgui_core::text::rich::{
        parse_rich_markup, RichBaseStyle, RichDeco, RichStyle, RichWeight,
    };
    for (idx, data) in tree.nodes.iter_mut().enumerate() {
        let Some(raw) = data.raw_rich.as_ref() else {
            continue;
        };
        let s = &styles[idx];
        // raw_rich 只在 inline display:block div 上设；display_mode 必 Block。
        // 守护栏：block div 拒 flex 属性（写了报错，不静默吞——AI 可预测性）。
        if s.display_mode == DisplayMode::Block {
            check_no_flex_props(s).map_err(|e| format!("元素 <{}>: {e}", data.tag))?;
        }
        // base 样式：从 ResolvedStyle 转 RichBaseStyle。weight/deco MVP 用默认
        // （bold 走 <b>、underline 走 <u>，base 不带——避免 base 已粗时 <b> 重复加粗语义混乱）。
        let base = RichBaseStyle {
            color: s.color,
            font_size: s.font_size,
            weight: RichWeight::Normal,
            style: RichStyle::Normal,
            deco: RichDeco::default(),
        };
        let runs = parse_rich_markup(raw, base, 0)?;
        data.rich_runs = Some(runs);
    }
    Ok((tree, styles))
}

/// block div 的 flex 属性护栏（spec §4.2）：justify-content/align-items/gap 非默认 → Err。
/// block div 是富文本叶，写 flex 属性无意义（inline flow 不走 taffy flex 排列）。
fn check_no_flex_props(s: &loomgui_core::style::resolved::ResolvedStyle) -> Result<(), String> {
    let ts = &s.taffy_style;
    let default = loomgui_core::style::resolved::ResolvedStyle::default().taffy_style;
    if ts.justify_content.is_some() {
        return Err(
            "display:block 不支持 justify-content（block div 是富文本叶，非 flex 容器）".into(),
        );
    }
    if ts.align_items.is_some() {
        return Err(
            "display:block 不支持 align-items（block div 是富文本叶，非 flex 容器）".into(),
        );
    }
    if ts.gap != default.gap {
        return Err("display:block 不支持 gap（block div 是富文本叶，非 flex 容器）".into());
    }
    Ok(())
}

/// 把工作区根下多个 HTML 打成 .pkg.bin（多组件格式，无 atlas）。
///
/// - `workspace_root`：工作区根目录（绝对路径）。img src 相对 html 文件解析成
///   workspace-root-relative sprite_key。
/// - `_name`：包名（当前未进 pkg.bin header，供 CLI 日志用；未来版本号/元数据可扩展）。
/// - `html_files`：[(workspace-root-relative html 路径, html 绝对路径)]。
///   组件名 = 相对路径的 file_stem。
///
/// 每 HTML 独立：抽 CSS → 剥 style/link → parse_html → parse_css → resolve_styles →
/// build_scene → scene_to_template（解析 img src 成 sprite_key）→ 收 (组件名, nodes, dynamic_rules)。
/// 最后 write_package 产 pkg_bytes。
pub fn pack(
    workspace_root: &Path,
    _name: &str,
    html_files: &[(String, std::path::PathBuf)],
) -> Result<PackedPackage, String> {
    // owned 生命周期：nodes/dynamic/controllers 需在 write_package 借用时存活，故先全部收集进 owned Vec。
    let mut owned: Vec<(
        String,
        Vec<TemplateNode>,
        loomgui_core::style::dynamic::DynamicRuleTable,
        Vec<ControllerEntry>,
    )> = Vec::with_capacity(html_files.len());
    let mut referenced: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (rel_path, abs_path) in html_files {
        let html = std::fs::read_to_string(abs_path)
            .map_err(|e| format!("read {}: {e}", abs_path.display()))?;
        // 1. 抽 CSS（<style> + <link>）—— parse_html 前调（围栏白名单挡 style/link）。
        // extract_component_css 的 base_dir 用 html 文件所在目录（解析 <link href> 相对路径）。
        let html_dir = abs_path.parent().unwrap_or(workspace_root);
        let css = extract_component_css(&html, html_dir);
        // 2. 剥 style/link 后再 parse_html（否则围栏报错）。
        let stripped = strip_style_and_link(&html);
        let tree = loomgui_core::parse::dom::parse_html(&stripped)
            .map_err(|e| format!("parse_html {rel_path}: {e}"))?;
        let sheet = loomgui_core::parse::css::parse_css(&css)
            .map_err(|e| format!("parse_css {rel_path}: {e}"))?;
        let dynamic = loomgui_core::asset::extract_dynamic_rules(&sheet);
        let styles = loomgui_core::style::cascade::resolve_styles(&tree, &sheet);
        let (tree, styles) = desugar_block_divs(tree, styles)
            .map_err(|e| format!("desugar_block_divs {rel_path}: {e}"))?;
        let scene = loomgui_core::scene::build_scene(&tree, &styles);
        // 扫 data-controller + data-page → controller name → initial page 映射。
        let controller_pages = collect_controller_pages(&tree);
        let (nodes, controllers) = scene_to_template(
            &scene,
            workspace_root,
            abs_path,
            &mut referenced,
            &mut seen,
            &controller_pages,
        )?;
        let comp_name = std::path::Path::new(rel_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(rel_path)
            .to_string();
        owned.push((comp_name, nodes, dynamic, controllers));
    }

    // 组 PackageInput（借用 owned）→ write_package。
    // asset_manifest 传空切片：Task 10 将删除 PackageInput.asset_manifest 字段。
    let comp_refs: Vec<(
        &str,
        &[TemplateNode],
        &loomgui_core::style::dynamic::DynamicRuleTable,
        &[ControllerEntry],
    )> = owned
        .iter()
        .map(|(name, nodes, dyn_rules, ctrls)| {
            (name.as_str(), nodes.as_slice(), dyn_rules, ctrls.as_slice())
        })
        .collect();
    let input = PackageInput {
        components: comp_refs,
        asset_manifest: &[],
    };
    let pkg_bytes = loomgui_core::asset::write_package(&input);

    Ok(PackedPackage {
        pkg_bytes,
        referenced_sprites: referenced,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// 构造测试用的 workspace_root 和 html 文件路径。
    /// 约定：workspace_root = /ws（Unix）或 C:\ws（Windows），html 在 ui/showcase/ 下。
    fn ws_root() -> PathBuf {
        PathBuf::from(if cfg!(windows) { r"C:\ws" } else { "/ws" })
    }

    fn html_path() -> PathBuf {
        ws_root().join("ui").join("showcase").join("main.html")
    }

    #[test]
    fn strip_style_and_link_removes_style_and_link_elements() {
        let html = r#"<div class="a"><style>.a { color: red; }</style><span>hi</span><link rel="stylesheet" href="x.css"></div>"#;
        let stripped = strip_style_and_link(html);
        assert!(!stripped.contains("style"), "style 元素已剥: {stripped}");
        assert!(!stripped.contains("link"), "link 元素已剥: {stripped}");
        assert!(stripped.contains("hi"), "正文保留: {stripped}");
        assert!(stripped.contains("<div"), "div 保留: {stripped}");
        assert!(stripped.contains("<span"), "span 保留: {stripped}");
    }

    #[test]
    fn strip_style_and_link_preserves_img_src() {
        let html = r#"<div><img src="res/x.png"></div>"#;
        let stripped = strip_style_and_link(html);
        assert!(stripped.contains("res/x.png"), "img src 保留");
    }

    #[test]
    fn strip_style_and_link_escapes_entities_in_text() {
        // 回归：scraper 把 `&lt;i&gt;` 解码成文本 "<i>"。序列化回 HTML 时若裸写，
        // 二次 parse_html 会把 "<i>" 当标签 → span 误判含元素子节点 → 行内混排假报错。
        // 文本节点须重新转义 `<` `>` `&`，保证 round-trip 语义一致。
        let html = r#"<div><span>a &lt;i&gt; c</span></div>"#;
        let stripped = strip_style_and_link(html);
        assert!(stripped.contains("&lt;i&gt;"), "文本实体保留: {stripped}");
        assert!(!stripped.contains("<i>"), "未裸写解码出的标签: {stripped}");
        assert!(
            loomgui_core::parse::dom::parse_html(&stripped).is_ok(),
            "二次 parse_html 成功（实体文本不被误判为元素）: {stripped}"
        );
    }

    #[test]
    fn scene_to_template_resolves_img_src_to_sprite_key() {
        // 手搓 scene：root + img 子（src="home.png"，相对 html 文件的目录 ui/showcase/）
        // resolved → "ui/showcase/home.png"
        use loomgui_core::scene::{NodeKind, Scene};
        use loomgui_core::style::resolved::ResolvedStyle;
        let entries: Vec<(
            Option<usize>,
            NodeKind,
            ResolvedStyle,
            Vec<String>,
            Option<String>,
            bool,
            Option<i32>,
            Option<String>,
        )> = vec![
            (
                None,
                NodeKind::Container,
                ResolvedStyle::default(),
                vec![],
                None,
                false,
                None,
                None,
            ),
            (
                Some(0),
                NodeKind::Image {
                    src: "home.png".into(),
                },
                ResolvedStyle::default(),
                vec![],
                None,
                false,
                None,
                None,
            ),
        ];
        let scene = Scene::build(&entries);
        let mut referenced: Vec<String> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let controller_pages = std::collections::HashMap::new();
        let (nodes, _controllers) = scene_to_template(
            &scene,
            &ws_root(),
            &html_path(),
            &mut referenced,
            &mut seen,
            &controller_pages,
        )
        .expect("scene_to_template ok");
        assert_eq!(
            referenced,
            vec!["ui/showcase/home.png".to_string()],
            "sprite_key 进 referenced"
        );
        // 节点 src 被回写为 sprite_key
        match &nodes[1].kind {
            NodeKind::Image { src } => {
                assert_eq!(src, "ui/showcase/home.png", "节点 src 回写为 sprite_key")
            }
            other => panic!("expected Image, got {other:?}"),
        }
    }

    /// RichText 行内图 src 也要解析成 sprite_key + referenced 收集 + 回写 run.src。
    #[test]
    fn scene_to_template_resolves_rich_inline_image_src() {
        use loomgui_core::scene::{NodeKind, Scene};
        use loomgui_core::style::resolved::ResolvedStyle;
        use loomgui_core::text::rich::{RichDeco, RichKind, RichRun, RichStyle, RichWeight};
        let runs = vec![RichRun {
            kind: RichKind::Image {
                src: "zap.png".into(),
                w: 22.0,
                h: 22.0,
                valign: loomgui_core::text::rich::RichVAlign::Middle,
            },
            color: [1.0; 4],
            font_id: 0,
            size_px: 20,
            weight: RichWeight::Normal,
            style: RichStyle::Normal,
            deco: RichDeco::default(),
            link_id: None,
        }];
        let entries: Vec<(
            Option<usize>,
            NodeKind,
            ResolvedStyle,
            Vec<String>,
            Option<String>,
            bool,
            Option<i32>,
            Option<String>,
        )> = vec![(
            None,
            NodeKind::RichText { runs },
            ResolvedStyle::default(),
            vec![],
            None,
            false,
            None,
            None,
        )];
        let scene = Scene::build(&entries);
        let mut referenced: Vec<String> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let controller_pages = std::collections::HashMap::new();
        let (nodes, _controllers) = scene_to_template(
            &scene,
            &ws_root(),
            &html_path(),
            &mut referenced,
            &mut seen,
            &controller_pages,
        )
        .expect("scene_to_template ok");
        assert_eq!(
            referenced,
            vec!["ui/showcase/zap.png".to_string()],
            "行内图 sprite_key 进 referenced"
        );
        match &nodes[0].kind {
            NodeKind::RichText { runs } => match &runs[0].kind {
                RichKind::Image { src, .. } => {
                    assert_eq!(src, "ui/showcase/zap.png", "run.src 回写为 sprite_key")
                }
                _ => panic!("expected Image run"),
            },
            other => panic!("expected RichText, got {other:?}"),
        }
    }

    #[test]
    fn scene_to_template_dedups_same_src_across_nodes() {
        // 两 img 同 src → referenced 只入一次
        use loomgui_core::scene::{NodeKind, Scene};
        use loomgui_core::style::resolved::ResolvedStyle;
        let entries: Vec<(
            Option<usize>,
            NodeKind,
            ResolvedStyle,
            Vec<String>,
            Option<String>,
            bool,
            Option<i32>,
            Option<String>,
        )> = vec![
            (
                None,
                NodeKind::Container,
                ResolvedStyle::default(),
                vec![],
                None,
                false,
                None,
                None,
            ),
            (
                Some(0),
                NodeKind::Image {
                    src: "a.png".into(),
                },
                ResolvedStyle::default(),
                vec![],
                None,
                false,
                None,
                None,
            ),
            (
                Some(0),
                NodeKind::Image {
                    src: "a.png".into(),
                },
                ResolvedStyle::default(),
                vec![],
                None,
                false,
                None,
                None,
            ),
        ];
        let scene = Scene::build(&entries);
        let mut referenced: Vec<String> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let controller_pages = std::collections::HashMap::new();
        let _ = scene_to_template(
            &scene,
            &ws_root(),
            &html_path(),
            &mut referenced,
            &mut seen,
            &controller_pages,
        )
        .expect("scene_to_template ok");
        assert_eq!(referenced.len(), 1, "同 src 去重只入一次");
    }

    #[test]
    fn scene_to_template_errors_on_src_outside_workspace_root() {
        // src 越出 workspace root → Err（不再 warn 静默跳过）
        use loomgui_core::scene::{NodeKind, Scene};
        use loomgui_core::style::resolved::ResolvedStyle;
        let entries: Vec<(
            Option<usize>,
            NodeKind,
            ResolvedStyle,
            Vec<String>,
            Option<String>,
            bool,
            Option<i32>,
            Option<String>,
        )> = vec![
            (
                None,
                NodeKind::Container,
                ResolvedStyle::default(),
                vec![],
                None,
                false,
                None,
                None,
            ),
            (
                Some(0),
                NodeKind::Image {
                    src: "../outside.png".into(),
                },
                ResolvedStyle::default(),
                vec![],
                None,
                false,
                None,
                None,
            ),
        ];
        let scene = Scene::build(&entries);
        let mut referenced: Vec<String> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let controller_pages = std::collections::HashMap::new();
        // html 在 ws root 下，../outside.png 越出根
        let html_at_root = ws_root().join("main.html");
        let result = scene_to_template(
            &scene,
            &ws_root(),
            &html_at_root,
            &mut referenced,
            &mut seen,
            &controller_pages,
        );
        assert!(result.is_err(), "越出 workspace root 应 Err");
        assert!(
            result.unwrap_err().contains("escapes workspace root"),
            "错误信息提及 escapes"
        );
    }

    #[test]
    fn scene_to_template_parent_idx_maps_to_position() {
        // root(parent=None) + child(parent=root) → child parent_idx=Some(0)
        use loomgui_core::scene::{NodeKind, Scene};
        use loomgui_core::style::resolved::ResolvedStyle;
        let entries: Vec<(
            Option<usize>,
            NodeKind,
            ResolvedStyle,
            Vec<String>,
            Option<String>,
            bool,
            Option<i32>,
            Option<String>,
        )> = vec![
            (
                None,
                NodeKind::Container,
                ResolvedStyle::default(),
                vec![],
                None,
                false,
                None,
                None,
            ),
            (
                Some(0),
                NodeKind::Text {
                    content: "hi".into(),
                },
                ResolvedStyle::default(),
                vec![],
                None,
                false,
                None,
                None,
            ),
        ];
        let scene = Scene::build(&entries);
        let mut referenced: Vec<String> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let controller_pages = std::collections::HashMap::new();
        let (nodes, _controllers) = scene_to_template(
            &scene,
            &ws_root(),
            &html_path(),
            &mut referenced,
            &mut seen,
            &controller_pages,
        )
        .expect("scene_to_template ok");
        assert_eq!(nodes[0].parent_idx, None, "root parent=None");
        assert_eq!(nodes[1].parent_idx, Some(0), "child parent_idx=Some(0)");
    }
}
