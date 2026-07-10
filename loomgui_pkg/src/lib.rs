//! 打包器库：系统目录多 HTML → .pkg.bin（无 atlas）。
//! 每个 HTML 独立 parse → resolve_styles → build_scene → 抽 TemplateNode；
//! img src / background-image url 归一化进 asset_manifest；CSS bake 进 style_blob。
//! 图集归 Unity，打包器不产 atlas。
//!
//! **图尺寸**：打包器对每个 manifest path 读 PNG IHDR（前 8 字节 magic + 13 字节 IHDR，
//! width/height big-endian u32 at offset 16/20）填 `AssetEntry { path, w, h }`。非 PNG 或读失败 →
//! w/h=0（核心 measure fallback 64×64）。PNG header 解析 ~30 行即可，无需完整 PNG 解码。

// type_complexity：打包器 build_scene 抽 TemplateNode 的返回类型天然是多层 Vec/HashMap 嵌套，
// 拆 alias 跨函数引用反而更难读。
#![allow(clippy::type_complexity)]

pub mod resolve;
pub mod workspace;

use loomgui_core::asset::{
    extract_component_css, normalize_path, AssetEntry, ControllerEntry, PackageInput, TemplateNode,
};
use loomgui_core::scene::NodeId;
use scraper::{Html, Selector as ScraperSelector};
use std::path::Path;

/// 打包产物：.pkg.bin bytes + asset_manifest（归一化 path + 图尺寸，供 Unity 校验 + 核心 measure/九宫格）。
/// 图集归 Unity，打包器不产 atlas。manifest 是 `Vec<AssetEntry>`（path + PNG IHDR 尺寸）。
#[derive(Debug)]
pub struct PackedPackage {
    pub pkg_bytes: Vec<u8>,
    pub asset_manifest: Vec<AssetEntry>,
}

/// 把单 scene 转 Vec<TemplateNode>（按 slotmap 插入序 = DFS 先序），同时把 img src +
/// background-image url 归一化收进 manifest（去重）。None（src 不在 res 下）→ warning 不入 manifest。
///
/// parent_idx = 父节点在 Vec 中的位置（None=组件根）。slotmap values() 对无删除的全新 map
/// 按槽位序迭代 = 插入序 = build_scene 的 DFS 先序，故 parent 总在 child 前出现，位置索引稳定。
///
/// **manifest 收 `AssetEntry { path, w:0, h:0 }`**（此处只收 path，w/h 由 `pack` 后置
/// 读 PNG IHDR 填——scene_to_template 不知 res 目录绝对路径，只有归一化 path）。
///
/// 同时扫 `Node.data_controller` 收 `ControllerEntry`：mount_node_idx = 节点在产物 Vec 中的
/// 位置（同 parent_idx 约定），initial_selected_index 从 `controller_pages` 查（打包期扫
/// `data-page` 属性得，key = controller name）。
fn scene_to_template(
    scene: &loomgui_core::scene::Scene,
    res_dir: &str,
    manifest: &mut Vec<AssetEntry>,
    seen: &mut std::collections::HashSet<String>,
    controller_pages: &std::collections::HashMap<String, i32>,
) -> (Vec<TemplateNode>, Vec<ControllerEntry>) {
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
        // img src 归一化进 manifest（去重）。归一化后回写节点 src，让 write_package 的
        // StringTable 收归一化 path（非原 src）。
        let mut kind = n.kind.clone();
        if let loomgui_core::scene::NodeKind::Image { src } = &mut kind {
            if !src.is_empty() {
                match normalize_path(src, res_dir) {
                    Some(norm) => {
                        if seen.insert(norm.clone()) {
                            manifest.push(AssetEntry {
                                path: norm.clone(),
                                w: 0,
                                h: 0,
                            });
                        }
                        *src = norm;
                    }
                    None => {
                        eprintln!(
                            "warn: img src `{src}` 不在 res 目录 `{res_dir}` 下，跳过 manifest"
                        );
                    }
                }
            }
        }
        // background-image url 同样归一化进 manifest（去重；与 img src 同 url 只入一次）。
        let mut style = n.style.clone();
        if let Some(url) = &style.background_image {
            if !url.is_empty() {
                match normalize_path(url, res_dir) {
                    Some(norm) => {
                        if seen.insert(norm.clone()) {
                            manifest.push(AssetEntry {
                                path: norm.clone(),
                                w: 0,
                                h: 0,
                            });
                        }
                        style.background_image = Some(norm);
                    }
                    None => {
                        eprintln!("warn: background-image url `{url}` 不在 res 目录 `{res_dir}` 下，跳过 manifest");
                    }
                }
            }
        }
        // RichText 行内图 src 归一化进 manifest（去重）。行内图嵌在 NodeKind::RichText 的
        // runs → RichKind::Image，顶层只 match NodeKind::Image 不会下钻——不归一化则 src 保持
        // "res/icons/x.png"（SpriteResolver 顶层子目录="res" 无 folder→atlas 映射 → 默认图集
        // miss → 白方块），且不入 manifest（Unity 不打包进 atlas）。归一化后回写 run.src。
        if let loomgui_core::scene::NodeKind::RichText { runs } = &mut kind {
            for r in runs.iter_mut() {
                if let loomgui_core::text::rich::RichKind::Image { src, .. } = &mut r.kind {
                    if !src.is_empty() {
                        match normalize_path(src, res_dir) {
                            Some(norm) => {
                                if seen.insert(norm.clone()) {
                                    manifest.push(AssetEntry {
                                        path: norm.clone(),
                                        w: 0,
                                        h: 0,
                                    });
                                }
                                *src = norm;
                            }
                            None => {
                                eprintln!(
                                    "warn: rich img src `{src}` 不在 res 目录 `{res_dir}` 下，跳过 manifest"
                                );
                            }
                        }
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
        // mount_node_idx = 节点在产物 Vec 中的位置（同 parent_idx 约定，组件内局部下标）。
        // initial_selected_index 从 controller_pages 查（打包期扫 data-page 属性得），缺省 0。
        if let Some(name) = &n.data_controller {
            let initial = controller_pages.get(name).copied().unwrap_or(0);
            controllers.push(ControllerEntry {
                name: name.clone(),
                mount_node_idx: i as u32,
                initial_selected_index: initial,
            });
        }
    }
    (nodes, controllers)
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

/// 读 PNG IHDR chunk 取真实像素 width/height。
///
/// PNG 布局：8 字节 magic (`\x89PNG\r\n\x1a\n`) + IHDR chunk（4 字节长度 + 4 字节 "IHDR" +
/// 13 字节数据：width(4 BE) + height(4 BE) + bit_depth(1) + color_type(1) + ...）。
/// width 在 offset 16，height 在 offset 20（big-endian u32）。
///
/// 非 PNG（magic 不符）/ 文件过短 / 读失败 → `(0, 0)`（核心 measure fallback 64×64）。
/// PNG header 解析 ~30 行，无需完整 PNG 解码。
fn read_png_size(path: &std::path::Path) -> (u32, u32) {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("warn: read png `{}` failed: {e}", path.display());
            return (0, 0);
        }
    };
    // PNG magic: \x89 P N G \r \n \x1a \n
    const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if bytes.len() < 24 || bytes[..8] != PNG_MAGIC {
        // 非 PNG（或过短）→ 0/0（核心 fallback 64×64）
        return (0, 0);
    }
    // IHDR chunk：offset 8 = length(4 BE) + "IHDR"(4) + data(width(4 BE) + height(4 BE) + ...)
    // width at offset 16, height at offset 20（big-endian u32）。
    let chunk_type = &bytes[12..16];
    if chunk_type != b"IHDR" {
        eprintln!("warn: `{}` first chunk not IHDR", path.display());
        return (0, 0);
    }
    let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    (w, h)
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

/// 把系统目录下多个 HTML 打成 .pkg.bin（多组件格式，无 atlas）。
///
/// - `source_dir`：包源目录（html + res 所在）。
/// - `pkg_name`：包名（当前未进 pkg.bin header，供 CLI 日志用；未来版本号/元数据可扩展）。
/// - `html_files`：要打包的 HTML 文件名列表（相对 sourceDir，含 .html 扩展名）。
/// - `res_root`：资源根目录路径（如 `Assets/LoomUI/res`；res 目录名从中推导，归一化 path 去此前缀）。
///
/// 每 HTML 独立：抽 CSS → 剥 style/link → parse_html → parse_css → resolve_styles →
/// build_scene → scene_to_template（归一化 src 进 manifest）→ 收 (组件名, nodes, dynamic_rules)。
/// 组件名 = 文件名去 .html。最后 write_package 产 pkg_bytes。
pub fn pack(
    source_dir: &Path,
    _pkg_name: &str,
    html_files: &[String],
    res_root: &Path,
) -> Result<PackedPackage, String> {
    let res_dir = res_root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("res");
    // owned 生命周期：nodes/dynamic/controllers 需在 write_package 借用时存活，故先全部收集进 owned Vec。
    let mut owned: Vec<(
        String,
        Vec<TemplateNode>,
        loomgui_core::style::dynamic::DynamicRuleTable,
        Vec<ControllerEntry>,
    )> = Vec::with_capacity(html_files.len());
    let mut manifest: Vec<AssetEntry> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for hf in html_files {
        let html_path = source_dir.join(hf);
        let html = std::fs::read_to_string(&html_path)
            .map_err(|e| format!("read {}: {e}", html_path.display()))?;
        // 1. 抽 CSS（<style> + <link>）—— parse_html 前调（围栏白名单挡 style/link）。
        let css = extract_component_css(&html, source_dir);
        // 2. 剥 style/link 后再 parse_html（否则围栏报错）。
        let stripped = strip_style_and_link(&html);
        let tree = loomgui_core::parse::dom::parse_html(&stripped)
            .map_err(|e| format!("parse_html {hf}: {e}"))?;
        let sheet = loomgui_core::parse::css::parse_css(&css)
            .map_err(|e| format!("parse_css {hf}: {e}"))?;
        let dynamic = loomgui_core::asset::extract_dynamic_rules(&sheet);
        let styles = loomgui_core::style::cascade::resolve_styles(&tree, &sheet);
        let (tree, styles) = desugar_block_divs(tree, styles)
            .map_err(|e| format!("desugar_block_divs {hf}: {e}"))?;
        let scene = loomgui_core::scene::build_scene(&tree, &styles);
        // 扫 data-controller + data-page → controller name → initial page 映射。
        let controller_pages = collect_controller_pages(&tree);
        let (nodes, controllers) =
            scene_to_template(&scene, res_dir, &mut manifest, &mut seen, &controller_pages);
        let comp_name = hf.strip_suffix(".html").unwrap_or(hf).to_string();
        owned.push((comp_name, nodes, dynamic, controllers));
    }

    // 对每个 manifest path 读 PNG IHDR 填真实尺寸 w/h。
    // path 是相对 res_dir 的归一化路径（如 "icons/skin.png"），绝对路径 = res_root/path。
    // 非 PNG / 读失败 → 0/0（核心 measure fallback 64×64）。
    for entry in &mut manifest {
        let abs = res_root.join(&entry.path);
        let (w, h) = read_png_size(&abs);
        entry.w = w;
        entry.h = h;
    }

    // 组 PackageInput（借用 owned）→ write_package。
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
        asset_manifest: &manifest,
    };
    let pkg_bytes = loomgui_core::asset::write_package(&input);

    Ok(PackedPackage {
        pkg_bytes,
        asset_manifest: manifest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn scene_to_template_normalizes_img_src_and_collects_manifest() {
        // 手搓 scene：root + img 子（src="res/icons/skin.png"）
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
                    src: "res/icons/skin.png".into(),
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
        let mut manifest: Vec<AssetEntry> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let controller_pages = std::collections::HashMap::new();
        let (nodes, _controllers) =
            scene_to_template(&scene, "res", &mut manifest, &mut seen, &controller_pages);
        // scene_to_template 只收 path（w/h=0），w/h 由 pack 后置读 PNG IHDR 填
        assert_eq!(
            manifest,
            vec![AssetEntry {
                path: "icons/skin.png".into(),
                w: 0,
                h: 0
            }],
            "归一化 path 进 manifest"
        );
        // 节点 src 也被归一化
        match &nodes[1].kind {
            NodeKind::Image { src } => assert_eq!(src, "icons/skin.png", "节点 src 归一化"),
            other => panic!("expected Image, got {other:?}"),
        }
    }

    /// RichText 行内图 src 也要归一化进 manifest + 回写 run.src（白方块 bug：嵌在
    /// NodeKind::RichText 的 Image run 顶层 match 不下钻）。
    #[test]
    fn scene_to_template_normalizes_rich_inline_image_src() {
        use loomgui_core::scene::{NodeKind, Scene};
        use loomgui_core::style::resolved::ResolvedStyle;
        use loomgui_core::text::rich::{RichDeco, RichKind, RichRun, RichStyle, RichWeight};
        let runs = vec![RichRun {
            kind: RichKind::Image {
                src: "res/icons/zap.png".into(),
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
        let mut manifest: Vec<AssetEntry> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let controller_pages = std::collections::HashMap::new();
        let (nodes, _controllers) =
            scene_to_template(&scene, "res", &mut manifest, &mut seen, &controller_pages);
        assert_eq!(
            manifest,
            vec![AssetEntry {
                path: "icons/zap.png".into(),
                w: 0,
                h: 0
            }],
            "行内图归一化 path 进 manifest"
        );
        match &nodes[0].kind {
            NodeKind::RichText { runs } => match &runs[0].kind {
                RichKind::Image { src, .. } => {
                    assert_eq!(src, "icons/zap.png", "run.src 归一化")
                }
                _ => panic!("expected Image run"),
            },
            other => panic!("expected RichText, got {other:?}"),
        }
    }

    #[test]
    fn scene_to_template_dedups_same_src_across_nodes() {
        // 两 img 同 src → manifest 只入一次
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
                    src: "res/a.png".into(),
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
                    src: "res/a.png".into(),
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
        let mut manifest: Vec<AssetEntry> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let controller_pages = std::collections::HashMap::new();
        let _ = scene_to_template(&scene, "res", &mut manifest, &mut seen, &controller_pages);
        assert_eq!(manifest.len(), 1, "同 src 去重只入一次");
    }

    #[test]
    fn scene_to_template_skips_src_outside_res_with_warning() {
        // src 不在 res 下 → None → 不入 manifest（不 Err）
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
                    src: "other/foo.png".into(),
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
        let mut manifest: Vec<AssetEntry> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let controller_pages = std::collections::HashMap::new();
        let (nodes, _controllers) =
            scene_to_template(&scene, "res", &mut manifest, &mut seen, &controller_pages);
        assert!(manifest.is_empty(), "res 外 src 不入 manifest");
        // 节点 src 保持原样（未归一化）
        match &nodes[1].kind {
            NodeKind::Image { src } => assert_eq!(src, "other/foo.png", "未归一化的 src 保持原样"),
            other => panic!("expected Image, got {other:?}"),
        }
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
        let mut manifest: Vec<AssetEntry> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let controller_pages = std::collections::HashMap::new();
        let (nodes, _controllers) =
            scene_to_template(&scene, "res", &mut manifest, &mut seen, &controller_pages);
        assert_eq!(nodes[0].parent_idx, None, "root parent=None");
        assert_eq!(nodes[1].parent_idx, Some(0), "child parent_idx=Some(0)");
    }
}
