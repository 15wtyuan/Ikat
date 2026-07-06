//! 包格式（.pkg.bin，当前 version=13）：Rust-internal（packager 写、runtime 读，C# 不解析）。
//!
//! 多组件格式：一个 pkg.bin = 多个具名组件（ComponentTable 切分）。
//! 布局：Header(20B) + StringTable + ComponentTable + NodeBlock + PerComponentDynamicRules +
//!       AssetManifest。
//!   - Header 不含 root_w/root_h（root_size 归 Stage）+ 不含 atlas 引用（图集归 Unity）。
//!   - StringTable：组件名 / text content / img path / classes / id_attr 共用一张表（intern 去重）。
//!   - ComponentTable：每组件 {name_idx, root_node_idx, node_count, dynamic_rules_blob_len}。
//!   - NodeBlock：所有组件节点平铺，parent_idx 用 -1 表组件根（全局位置索引）。
//!   - PerComponentDynamicRules：每组件 dynamic_rules 的 bincode blob（紧跟 ComponentTable 段）。
//!   - AssetManifest：本包所有 img path + 图尺寸（打包期 PNG IHDR 静态数据，核心 measure/
//!     九宫格 UV 用；Unity 校验 res 齐全 + Sprite 查询也用）。
//! style 字段 = bincode(ResolvedStyle，已 bake)。img src 指向归一化 path 字符串（非 atlas sprite）。
//!
//! 核心不知图集（运行时纹理/UV 归 Unity）；AssetManifest 用 `Vec<AssetEntry { path, w, h }>`
//! ——核心知图尺寸（打包期 PNG IHDR 静态）但不知图集。

use crate::scene::NodeKind;
use crate::style::dynamic::DynamicRuleTable;
use crate::style::resolved::ResolvedStyle;

pub const PKG_MAGIC: u32 = 0x474B504C; // 磁盘字节(LE) "LPKG"（不与 frame blob "LOOM" 撞）
pub const PKG_FORMAT_VERSION: u32 = 13; // TemplateNode.data_controller（Controller 挂载点声明）
pub(crate) const MIN_VERSION: u32 = 13;
pub(crate) const MAX_VERSION: u32 = 13;
const NULL_IDX: u16 = 0xFFFF;

const KIND_CONTAINER: u8 = 0;
const KIND_BUTTON: u8 = 1;
const KIND_IMAGE: u8 = 2;
const KIND_TEXT: u8 = 3;

// ── 多组件包数据结构 ──────────────────────────────────────────────

/// AssetManifest 条目：归一化 path + 打包期 PNG IHDR 读出的真实像素尺寸。
///
/// `w`/`h` = PNG 原始像素（非图集/纹理运行时尺寸）。非 PNG 或读失败 → 0（核心 measure/九宫格
/// fallback 64×64）。核心 `Stage` 在 `load_package` 时建 `path → (w,h)` 查询表供 layout/render 用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetEntry {
    pub path: String,
    pub w: u32,
    pub h: u32,
}

/// 一个已加载的包（资源池条目）。`name` read 时填空串，由 `Stage::load_package(name, ..)` 覆盖。
#[derive(Debug, Clone)]
pub struct Package {
    pub name: String,
    pub components: std::collections::HashMap<String, ComponentTemplate>,
    /// 本包用到的所有 img path + 图尺寸（打包期 PNG IHDR 静态，核心 measure/九宫格用）。
    pub asset_manifest: Vec<AssetEntry>,
}

/// 一个组件的模板（instantiate 的克隆源）。
#[derive(Debug, Clone)]
pub struct ComponentTemplate {
    pub name: String,
    pub nodes: Vec<TemplateNode>,
    pub dynamic_rules: DynamicRuleTable,
}

/// 模板节点：序列化态（instantiate 时 build 成 live Node）。
/// 与 live Node 区别：无 NodeId（instantiate 时 slotmap 分配）、无 taffy_id（每帧 solve 重建）。
#[derive(Debug, Clone)]
pub struct TemplateNode {
    pub kind: NodeKind,
    pub style: ResolvedStyle,      // base_style（已 bake）
    pub parent_idx: Option<usize>, // 模板内位置索引（None=组件根）
    pub classes: Vec<String>,
    pub id_attr: Option<String>,
    pub draggable: bool,
    pub tabindex: Option<i32>,
    /// HTML `data-controller="name"` 属性值（Controller 挂载点声明）。
    /// instantiate 时填 live Node.data_controller；匹配器遇 `[data-page]` 回溯查此字段。
    pub data_controller: Option<String>,
}

/// write_package 的输入（打包器构造，已归一化：path 已相对、style 已 bake）。
pub struct PackageInput<'a> {
    pub components: Vec<(&'a str, &'a [TemplateNode], &'a DynamicRuleTable)>,
    /// 已去重归一化的 path + PNG IHDR 尺寸（打包器读 PNG header 填，核心 measure/九宫格用）。
    pub asset_manifest: &'a [AssetEntry],
}

#[derive(Debug)]
pub enum PkgError {
    BadMagic,
    TooOld(u32),
    TooNew(u32),
    Truncated(&'static str),
    OobString(u16),
    Bincode(bincode::Error),
    BadKind(u8),
    DupComponent(String),
}

impl std::fmt::Display for PkgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PkgError::BadMagic => write!(f, "bad magic (not a loom package)"),
            PkgError::TooOld(v) => {
                write!(f, "package formatVersion {v} too old (min {MIN_VERSION})")
            }
            PkgError::TooNew(v) => {
                write!(f, "package formatVersion {v} too new (max {MAX_VERSION})")
            }
            PkgError::Truncated(ctx) => write!(f, "truncated package: {ctx}"),
            PkgError::OobString(i) => write!(f, "string index {i} out of range"),
            PkgError::Bincode(e) => write!(f, "style bincode: {e}"),
            PkgError::BadKind(k) => write!(f, "bad node kind tag {k}"),
            PkgError::DupComponent(n) => {
                write!(f, "duplicate component name in package: {n}")
            }
        }
    }
}

impl std::error::Error for PkgError {}

impl From<bincode::Error> for PkgError {
    fn from(e: bincode::Error) -> Self {
        PkgError::Bincode(e)
    }
}

/// 从 StyleSheet 抽含 :hover/:active/:disabled/:focus 的规则 → DynamicRuleTable。
/// 纯静态规则不进（已在 base_style 烤好）。判定：parse_selector 后任一 compound 含伪类标志。
///
/// **parse-gated：**消费 parse 后的 StyleSheet（CSS 文本产物），runtime 无此输入。
#[cfg(feature = "parse")]
pub fn extract_dynamic_rules(sheet: &crate::parse::css::StyleSheet) -> DynamicRuleTable {
    use crate::parse::selector::parse_selector;
    use crate::style::dynamic::DynamicRule;
    let mut rules = Vec::new();
    for rule in &sheet.rules {
        let sel = match parse_selector(&rule.selector_text) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let has_pseudo = sel
            .compound
            .iter()
            .any(|c| c.pseudo_hover || c.pseudo_active || c.pseudo_disabled || c.pseudo_focus);
        if has_pseudo {
            rules.push(DynamicRule {
                selector: sel,
                declarations: rule.declarations.clone(),
            });
        }
    }
    DynamicRuleTable { rules }
}

// ── 归一化（path + CSS）──────────────────────────────────────────
//
// 这两个是纯函数，打包器扫 HTML 后调它们产归一化数据喂 write_package。
// 都 parse-gated：消费 parse 后的产物（scraper 解析 HTML / 读 css 文件），runtime 无此输入。

/// 归一化图片 src：去 res 目录前缀 + 统一正斜杠。
///
/// 输入例：`res/icons/skin.png` / `./res/icons/skin.png` / `res\icons\skin.png` → `icons/skin.png`。
/// `res_dir` 取自打包配置（默认 `res`，可配置）。
/// 不在 `res_dir/` 路径段下 → None（调用方 warning，不入 manifest）。
///
/// 边界检查：`res/` 必须是完整路径段，不误匹配 `pres/x`（前缀前字符须是 `/`、`.` 或串首）。
/// spec §3.4。
#[cfg(feature = "parse")]
pub fn normalize_path(src: &str, res_dir: &str) -> Option<String> {
    let unified = src.replace('\\', "/"); // Win 反斜杠 → 正斜杠
    let prefix = format!("{}/", res_dir);
    // 找所有 res/ 出现位置，取第一个满足"前字符是 / 或 . 或串首"的（合法路径段）。
    let mut start = 0usize;
    while let Some(rel) = unified[start..].find(&prefix) {
        let abs = start + rel; // prefix 在 unified 中的绝对起点
        let before_ok = abs == 0 || matches!(unified.as_bytes()[abs - 1], b'/' | b'.');
        if before_ok {
            let stripped = &unified[abs + prefix.len()..];
            if !stripped.is_empty() {
                return Some(stripped.to_string());
            }
            // res/ 后空（如 "res/"）→ None
            return None;
        }
        start = abs + 1; // 跳过本次误匹配，继续找下一个
    }
    None
}

/// 抽 HTML 的所有 CSS 合并成 stylesheet 串：
/// - `<style>` 内联（取 text content，可多处）
/// - `<link rel="stylesheet" href="...">` 引用的 .css 文件内容（base_dir.join(href) 读）
///
/// 行内 `style=""` 不抽——由 `resolve_styles` 直接 bake 进节点 style（spec §3.5）。
/// `base_dir` = HTML 文件所在目录，用于解析 `<link href>` 的相对路径。
///
/// **签名调整说明**：brief 伪写 `(tree: &DomTree, ...)`，但 `parse_html` 的围栏白名单
/// （`FENCE_TAGS = div/span/img/button`）会拒绝 `<style>`/`<link>`，无法从已解析 ElementTree
/// 取这俩节点。故直接吃原始 HTML 串、用 scraper 抽 `<style>`/`<link>`（与 dom.rs 同后端）。
/// 打包器在调 `parse_html` 之前先调本函数抽 CSS（同一份 HTML 串两用）。
#[cfg(feature = "parse")]
pub fn extract_component_css(html: &str, base_dir: &std::path::Path) -> String {
    use scraper::{Html, Selector};
    let document = Html::parse_document(html);
    let mut parts: Vec<String> = Vec::new();

    // <style> 内联：text content 整段收（scraper 的 text() 拼所有文本节点）。
    if let Ok(style_sel) = Selector::parse("style") {
        for el in document.select(&style_sel) {
            let text: String = el.text().collect();
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                parts.push(trimmed.to_string());
            }
        }
    }

    // <link rel="stylesheet" href="...">：读 href 文件内容。
    // rel 值大小写不敏感、允许空格分隔多值（如 "stylesheet preload"）——含 "stylesheet" 即抽。
    if let Ok(link_sel) = Selector::parse("link") {
        for el in document.select(&link_sel) {
            let rel_is_stylesheet = el
                .value()
                .attr("rel")
                .map(|r| {
                    r.split_whitespace()
                        .any(|t| t.eq_ignore_ascii_case("stylesheet"))
                })
                .unwrap_or(false);
            if !rel_is_stylesheet {
                continue;
            }
            let Some(href) = el.value().attr("href") else {
                continue;
            };
            let path = base_dir.join(href);
            if let Ok(content) = std::fs::read_to_string(&path) {
                let trimmed = content.trim();
                if !trimmed.is_empty() {
                    parts.push(trimmed.to_string());
                }
            }
            // 读失败（文件缺失/编码错）→ 静默跳过（打包期 warning 由调用方补）
        }
    }

    parts.join("\n")
}

/// 序列化 PackageInput → .pkg.bin bytes（多组件格式）。
///
/// 布局：Header(20B) + StringTable + ComponentTable + NodeBlock + PerComponentDynamicRules
///       + AssetManifest。所有字符串（组件名 / text / img path / classes / id_attr）
///       共用同一 StringTable（intern 去重）。`input` 须已归一化（path 相对、style bake）。
pub fn write_package(input: &PackageInput) -> Vec<u8> {
    // 1. intern 全部字符串（组件名 + 每节点 text/src/classes/id_attr + asset_manifest path）。
    //    所有 intern 必须在写 header(string_count) 之前完成。
    let mut strings: Vec<String> = Vec::new();
    let mut idx_of: std::collections::HashMap<String, u16> = std::collections::HashMap::new();

    let component_count = input.components.len();
    // 每组件：(name_idx, root_node_idx, node_count, dynamic_blob)
    // 全局 NodeBlock 由各组件节点顺次拼接，root_node_idx = 该组件首节点在全局的位置。
    let mut comp_records: Vec<(u16, u32, u32, Vec<u8>)> = Vec::with_capacity(component_count);
    // 每节点（全局）：(parent_idx:i32, kind_tag, style_blob, text_idx, src_idx, class_idx[], id_idx, flags, tabindex, dc_idx)
    let mut node_records: Vec<(i32, u8, Vec<u8>, u16, u16, Vec<u16>, u16, u8, i32, u16)> =
        Vec::new();
    let mut global_node_offset: u32 = 0;
    for (name, nodes, dynamic_rules) in &input.components {
        let name_idx = intern(name, &mut strings, &mut idx_of);
        let comp_base = global_node_offset;
        // spec 约定 nodes[0]=组件根（parent=None)。debug_assert：write 输入由打包器控制，
        // 违反即打包器 bug（非运行时 malformed 输入），故 debug_assert 足够（release 不付代价）。
        if !nodes.is_empty() {
            debug_assert!(
                nodes[0].parent_idx.is_none(),
                "component `{name}` nodes[0] must be root (parent_idx=None)"
            );
        }
        // intern 每节点字符串 + 收 (parent_idx 全局化, ...)。spec 约定 nodes[0]=组件根（parent=None）。
        for tn in nodes.iter() {
            // parent_idx 是组件内局部位置；转全局（-1 = 组件根）
            let parent_global: i32 = match tn.parent_idx {
                None => -1,
                Some(p) => (comp_base as usize + p) as i32,
            };
            let (kind_tag, text_idx, src_idx) = match &tn.kind {
                NodeKind::Container => (KIND_CONTAINER, NULL_IDX, NULL_IDX),
                NodeKind::Button => (KIND_BUTTON, NULL_IDX, NULL_IDX),
                NodeKind::Image { src } => {
                    (KIND_IMAGE, NULL_IDX, intern(src, &mut strings, &mut idx_of))
                }
                NodeKind::Text { content } => (
                    KIND_TEXT,
                    intern(content, &mut strings, &mut idx_of),
                    NULL_IDX,
                ),
            };
            let style_blob = bincode::serialize(&tn.style).expect("ResolvedStyle serializable");
            let class_idx: Vec<u16> = tn
                .classes
                .iter()
                .map(|c| intern(c, &mut strings, &mut idx_of))
                .collect();
            let id_idx = tn
                .id_attr
                .as_ref()
                .map(|id| intern(id, &mut strings, &mut idx_of))
                .unwrap_or(NULL_IDX);
            // data_controller：照 id_attr 同款 intern + NULL_IDX 哨兵（None=0xFFFF）。
            let dc_idx = tn
                .data_controller
                .as_ref()
                .map(|dc| intern(dc, &mut strings, &mut idx_of))
                .unwrap_or(NULL_IDX);
            let flags: u8 = if tn.draggable { 0x01 } else { 0x00 };
            let tabindex = tn.tabindex.unwrap_or(i32::MIN);
            node_records.push((
                parent_global,
                kind_tag,
                style_blob,
                text_idx,
                src_idx,
                class_idx,
                id_idx,
                flags,
                tabindex,
                dc_idx,
            ));
        }
        let node_count = nodes.len() as u32;
        let dynamic_blob =
            bincode::serialize(dynamic_rules).expect("DynamicRuleTable serializable");
        comp_records.push((name_idx, comp_base, node_count, dynamic_blob));
        global_node_offset += node_count;
    }
    // intern asset_manifest path（供 AssetManifest 段引用 idx）+ 收 w/h。
    let manifest_idx: Vec<(u16, u32, u32)> = input
        .asset_manifest
        .iter()
        .map(|e| (intern(&e.path, &mut strings, &mut idx_of), e.w, e.h))
        .collect();

    let mut out: Vec<u8> = Vec::new();
    // Header (20B): magic + version + flags + component_count + string_count
    out.extend_from_slice(&PKG_MAGIC.to_le_bytes());
    out.extend_from_slice(&PKG_FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // flags
    out.extend_from_slice(&(component_count as u32).to_le_bytes());
    out.extend_from_slice(&(strings.len() as u32).to_le_bytes());
    // StringTable
    for s in &strings {
        let bytes = s.as_bytes();
        out.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
        out.extend_from_slice(bytes);
    }
    // ComponentTable: 每组件 {name_idx(u16), root_node_idx(u32), node_count(u32), dynamic_rules_blob_len(u32)}
    for (name_idx, root_node_idx, node_count, dynamic_blob) in &comp_records {
        out.extend_from_slice(&name_idx.to_le_bytes());
        out.extend_from_slice(&root_node_idx.to_le_bytes());
        out.extend_from_slice(&node_count.to_le_bytes());
        out.extend_from_slice(&(dynamic_blob.len() as u32).to_le_bytes());
    }
    // NodeBlock: 每节点 {parent_idx(i32), kind_tag(u8), style_len(u32)+style_blob, text_idx(u16), src_idx(u16),
    //   class_count(u16)+class_idx[], id_idx(u16), flags(u8), tabindex(i32), dc_idx(u16)}
    for (
        parent_idx,
        kind_tag,
        style_blob,
        text_idx,
        src_idx,
        class_idx,
        id_idx,
        flags,
        tabindex,
        dc_idx,
    ) in &node_records
    {
        out.extend_from_slice(&parent_idx.to_le_bytes());
        out.push(*kind_tag);
        out.extend_from_slice(&(style_blob.len() as u32).to_le_bytes());
        out.extend_from_slice(style_blob);
        out.extend_from_slice(&text_idx.to_le_bytes());
        out.extend_from_slice(&src_idx.to_le_bytes());
        out.extend_from_slice(&(class_idx.len() as u16).to_le_bytes());
        for &cidx in class_idx {
            out.extend_from_slice(&cidx.to_le_bytes());
        }
        out.extend_from_slice(&id_idx.to_le_bytes());
        out.push(*flags);
        out.extend_from_slice(&tabindex.to_le_bytes());
        out.extend_from_slice(&dc_idx.to_le_bytes());
    }
    // PerComponentDynamicRules: 每组件 dynamic_blob（紧跟 ComponentTable 序）
    for (_, _, _, dynamic_blob) in &comp_records {
        out.extend_from_slice(dynamic_blob);
    }
    // AssetManifest: entry_count(u32) + count × {path_idx(u16), w(u32), h(u32)}（path + 图尺寸）
    out.extend_from_slice(&(manifest_idx.len() as u32).to_le_bytes());
    for &(pidx, w, h) in &manifest_idx {
        out.extend_from_slice(&pidx.to_le_bytes());
        out.extend_from_slice(&w.to_le_bytes());
        out.extend_from_slice(&h.to_le_bytes());
    }
    out
}

/// 反序列化 .pkg.bin → Package（多组件格式，含版本协商）。
/// `Package.name` read 时填空串（read 不知包名），由 `Stage::load_package(name, ..)` 覆盖。
pub fn read_package(bytes: &[u8]) -> Result<Package, PkgError> {
    let mut r = Reader::new(bytes);
    // Header (20B)
    let magic = r.u32("magic")?;
    if magic != PKG_MAGIC {
        return Err(PkgError::BadMagic);
    }
    let version = r.u32("version")?;
    if version < MIN_VERSION {
        return Err(PkgError::TooOld(version));
    }
    if version > MAX_VERSION {
        return Err(PkgError::TooNew(version));
    }
    let _flags = r.u32("flags")?;
    let component_count = r.u32("component_count")? as usize;
    let string_count = r.u32("string_count")? as usize;
    // StringTable
    let mut strings: Vec<String> = Vec::with_capacity(string_count);
    for _ in 0..string_count {
        let len = r.u16("str_len")? as usize;
        let s = r.utf8(len, "str_bytes")?;
        strings.push(s);
    }
    // ComponentTable: 每组件 {name_idx(u16), root_node_idx(u32), node_count(u32), dynamic_rules_blob_len(u32)}
    let mut comp_table: Vec<(u16, u32, u32, u32)> = Vec::with_capacity(component_count);
    for _ in 0..component_count {
        let name_idx = r.u16("comp_name_idx")?;
        let root_node_idx = r.u32("comp_root_node_idx")?;
        let node_count = r.u32("comp_node_count")?;
        let dynamic_len = r.u32("comp_dynamic_len")?;
        comp_table.push((name_idx, root_node_idx, node_count, dynamic_len));
    }
    // 总节点数 = 各组件 node_count 之和
    let total_nodes: u32 = comp_table.iter().map(|(_, _, n, _)| *n).sum();
    // NodeBlock → TemplateNode（平铺，parent_idx 存盘是全局位置；读后转回组件内局部）
    let mut all_nodes: Vec<TemplateNode> = Vec::with_capacity(total_nodes as usize);
    for _ in 0..total_nodes {
        let pidx = r.i32("parent_idx")?;
        let kind_tag = r.u8("kind")?;
        let style_len = r.u32("style_len")? as usize;
        let style: ResolvedStyle = bincode::deserialize(r.take(style_len, "style_blob")?)?;
        let text_idx = r.u16("text_idx")?;
        let src_idx = r.u16("src_idx")?;
        let class_count = r.u16("class_count")? as usize;
        let mut classes: Vec<String> = Vec::with_capacity(class_count);
        for _ in 0..class_count {
            let cidx = r.u16("class_idx")?;
            classes.push(string_at(&strings, cidx)?);
        }
        let id_idx = r.u16("id_idx")?;
        let id_attr = if id_idx == NULL_IDX {
            None
        } else {
            Some(string_at(&strings, id_idx)?)
        };
        let flags = r.u8("flags")?;
        let draggable = (flags & 0x01) != 0;
        let tab_raw = r.i32("tabindex")?;
        let tabindex = if tab_raw == i32::MIN {
            None
        } else {
            Some(tab_raw)
        };
        // data_controller：照 id_attr 同款 NULL_IDX 哨兵读（None=0xFFFF）。
        let dc_idx = r.u16("dc_idx")?;
        let data_controller = if dc_idx == NULL_IDX {
            None
        } else {
            Some(string_at(&strings, dc_idx)?)
        };
        // 存盘 parent_idx 是 NodeBlock 全局位置（-1=组件根）；先存全局，待切分组件时减 base 转局部
        let parent_global = if pidx < 0 { None } else { Some(pidx as usize) };
        let kind = match kind_tag {
            KIND_CONTAINER => NodeKind::Container,
            KIND_BUTTON => NodeKind::Button,
            KIND_IMAGE => NodeKind::Image {
                src: string_at(&strings, src_idx)?,
            },
            KIND_TEXT => NodeKind::Text {
                content: string_at(&strings, text_idx)?,
            },
            other => return Err(PkgError::BadKind(other)),
        };
        all_nodes.push(TemplateNode {
            kind,
            style,
            parent_idx: parent_global, // 临时存全局，下方切分时减 base
            classes,
            id_attr,
            draggable,
            tabindex,
            data_controller,
        });
    }
    // PerComponentDynamicRules: 每组件 dynamic_blob（按 ComponentTable 序）
    let mut components: std::collections::HashMap<String, ComponentTemplate> =
        std::collections::HashMap::with_capacity(component_count);
    for (name_idx, root_node_idx, node_count, dynamic_len) in &comp_table {
        let name = string_at(&strings, *name_idx)?;
        let start = *root_node_idx as usize;
        let end = start + *node_count as usize;
        // 防御 malformed ComponentTable：root_node_idx/node_count 越界 → Truncated（避免 slice panic）
        if start > all_nodes.len() || end > all_nodes.len() {
            return Err(PkgError::Truncated("comp_node_slice"));
        }
        let base = start;
        // 组件内 parent_idx：全局 - base（组件根 parent_idx=None 仍是 None）。
        // 防御 malformed：parent_global < base 表示父节点落到更早的组件 → Truncated（不允许跨组件父）
        let mut nodes = all_nodes[start..end].to_vec();
        for tn in nodes.iter_mut() {
            if let Some(p) = tn.parent_idx {
                if p < base {
                    return Err(PkgError::Truncated("cross_comp_parent"));
                }
                tn.parent_idx = Some(p - base);
            }
        }
        let dynamic_rules: DynamicRuleTable =
            bincode::deserialize(r.take(*dynamic_len as usize, "comp_dynamic_blob")?)?;
        // 防御 malformed：同名组件 → DupComponent（避免静默覆盖丢数据）
        if components.contains_key(&name) {
            return Err(PkgError::DupComponent(name));
        }
        components.insert(
            name.clone(),
            ComponentTemplate {
                name,
                nodes,
                dynamic_rules,
            },
        );
    }
    // AssetManifest: entry_count(u32) + count × {path_idx(u16), w(u32), h(u32)}（path + 图尺寸）
    let entry_count = r.u32("manifest_entry_count")? as usize;
    let mut asset_manifest: Vec<AssetEntry> = Vec::with_capacity(entry_count);
    for _ in 0..entry_count {
        let pidx = r.u16("manifest_path_idx")?;
        let w = r.u32("manifest_w")?;
        let h = r.u32("manifest_h")?;
        asset_manifest.push(AssetEntry {
            path: string_at(&strings, pidx)?,
            w,
            h,
        });
    }
    Ok(Package {
        name: String::new(),
        components,
        asset_manifest,
    })
}

fn string_at(strings: &[String], idx: u16) -> Result<String, PkgError> {
    if idx == NULL_IDX {
        return Ok(String::new());
    }
    strings
        .get(idx as usize)
        .cloned()
        .ok_or(PkgError::OobString(idx))
}

/// 把字符串 intern 进 stringTable（首次出现分配新索引，重复返回既有索引）。
fn intern(
    s: &str,
    strings: &mut Vec<String>,
    idx_of: &mut std::collections::HashMap<String, u16>,
) -> u16 {
    if let Some(&i) = idx_of.get(s) {
        return i;
    }
    let i = strings.len() as u16;
    strings.push(s.to_string());
    idx_of.insert(s.to_string(), i);
    i
}

/// 极简游标 reader：定长小端读取 + 截断保护。
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Reader { buf, pos: 0 }
    }
    fn need(&mut self, n: usize, ctx: &'static str) -> Result<&'a [u8], PkgError> {
        if self.pos + n > self.buf.len() {
            return Err(PkgError::Truncated(ctx));
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    fn u8(&mut self, ctx: &'static str) -> Result<u8, PkgError> {
        Ok(self.need(1, ctx)?[0])
    }
    fn u16(&mut self, ctx: &'static str) -> Result<u16, PkgError> {
        Ok(u16::from_le_bytes(self.need(2, ctx)?.try_into().unwrap()))
    }
    fn u32(&mut self, ctx: &'static str) -> Result<u32, PkgError> {
        Ok(u32::from_le_bytes(self.need(4, ctx)?.try_into().unwrap()))
    }
    fn i32(&mut self, ctx: &'static str) -> Result<i32, PkgError> {
        Ok(i32::from_le_bytes(self.need(4, ctx)?.try_into().unwrap()))
    }
    fn take(&mut self, n: usize, ctx: &'static str) -> Result<&'a [u8], PkgError> {
        self.need(n, ctx)
    }
    fn utf8(&mut self, n: usize, ctx: &'static str) -> Result<String, PkgError> {
        let s = self.need(n, ctx)?;
        std::str::from_utf8(s)
            .map(String::from)
            .map_err(|_| PkgError::Truncated(ctx))
    }
}

#[cfg(test)]
mod tests;
