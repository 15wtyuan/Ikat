//! 包格式（.pkg.bin，当前 version=16）：Rust-internal（packager 写、runtime 读，C# 不解析）。
//!
//! 多组件格式：一个 pkg.bin = 多个具名组件（ComponentTable 切分）。
//! 布局：Header(20B) + StringTable + ComponentTable + RichRunsArena + NodeBlock +
//!       PerComponent(DynamicRules+Controllers)。
//!   - Header 不含 root_w/root_h（root_size 归 Stage）+ 不含 atlas 引用（图集归 Unity）。
//!   - StringTable：组件名 / text content / img path / classes / id_attr 共用一张表（intern 去重）。
//!   - ComponentTable：每组件 {name_idx, root_node_idx, node_count, dynamic_rules_blob_len}。
//!   - RichRunsArena：所有 RichText 节点的 runs blob 拼接（每段前缀 u32 len）。NodeBlock
//!     的 rich_off 字段指向此 arena 的字节偏移（NULL_RICH_OFF=0xFFFF_FFFF=无 runs）。
//!   - NodeBlock：所有组件节点平铺，parent_idx 用 -1 表组件根（全局位置索引）。
//!   - PerComponentDynamicRules：每组件 dynamic_rules 的 bincode blob（紧跟 ComponentTable 段）。
//! style 字段 = bincode(ResolvedStyle，已 bake)。img src 指向归一化 path 字符串（非 atlas sprite）。
//!
//! 核心不知图集（运行时纹理/UV 归 Unity）。图尺寸由 Stage.set_image_sizes 在运行时灌入
//! （来自 atlas.json），不再进 pkg.bin。

use crate::scene::NodeKind;
use crate::style::dynamic::DynamicRuleTable;
use crate::style::resolved::ResolvedStyle;

pub const PKG_MAGIC: u32 = 0x474B504C; // 磁盘字节(LE) "LPKG"（不与 frame blob "LOOM" 撞）
pub const PKG_FORMAT_VERSION: u32 = 16; // v16：删单值 border width 字段（render 改读 ts.border 四边，bincode 布局变）
pub(crate) const MIN_VERSION: u32 = 16;
pub(crate) const MAX_VERSION: u32 = 16;
const NULL_IDX: u16 = 0xFFFF;
/// NodeBlock 中 rich_off 字段的"无 runs"哨兵。非 RichText 节点写此值。
const NULL_RICH_OFF: u32 = 0xFFFF_FFFF;

const KIND_CONTAINER: u8 = 0;
const KIND_BUTTON: u8 = 1;
const KIND_IMAGE: u8 = 2;
const KIND_TEXT: u8 = 3;
const KIND_RICHTEXT: u8 = 4;

// ── 多组件包数据结构 ──────────────────────────────────────────────

/// 一个已加载的包（资源池条目）。`name` read 时填空串，由 `Stage::load_package(name, ..)` 覆盖。
#[derive(Debug, Clone)]
pub struct Package {
    pub name: String,
    pub components: std::collections::HashMap<String, ComponentTemplate>,
}

/// 一个组件的模板（instantiate 的克隆源）。
#[derive(Debug, Clone)]
pub struct ComponentTemplate {
    pub name: String,
    pub nodes: Vec<TemplateNode>,
    pub dynamic_rules: DynamicRuleTable,
    /// Controller 元数据（打包期扫 data-controller + data-page 得）。
    /// instantiate 时 mount_node_idx 经 id_map 重映射成活 NodeId，建 registry 条目。
    pub controllers: Vec<ControllerEntry>,
}

/// 打包期 Controller 元数据。mount_node_idx 是组件内局部下标（同 TemplateNode.parent_idx），
/// instantiate 时经 id_map 重映射成活 NodeId。initial_selected_index 来自 mount 元素的
/// `data-page` 属性（缺省 0）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerEntry {
    pub name: String,
    pub mount_node_idx: u32,
    pub initial_selected_index: i32,
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
    pub components: Vec<(
        &'a str,
        &'a [TemplateNode],
        &'a DynamicRuleTable,
        &'a [ControllerEntry],
    )>,
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

/// 序列化 PackageInput → .pkg.bin bytes（多组件格式）。
///
/// 布局：Header(20B) + StringTable + ComponentTable + NodeBlock + PerComponent(DynamicRules+Controllers)。
/// 所有字符串（组件名 / text / img path / classes / id_attr / controller name）
/// 共用同一 StringTable（intern 去重）。`input` 须已归一化（path 相对、style bake）。
pub fn write_package(input: &PackageInput) -> Vec<u8> {
    // 1. intern 全部字符串（组件名 + 每节点 text/src/classes/id_attr）。
    //    所有 intern 必须在写 header(string_count) 之前完成。
    let mut strings: Vec<String> = Vec::new();
    let mut idx_of: std::collections::HashMap<String, u16> = std::collections::HashMap::new();

    let component_count = input.components.len();
    // 每组件：(name_idx, root_node_idx, node_count, dynamic_blob)
    // 全局 NodeBlock 由各组件节点顺次拼接，root_node_idx = 该组件首节点在全局的位置。
    let mut comp_records: Vec<(u16, u32, u32, Vec<u8>)> = Vec::with_capacity(component_count);
    // 每组件：Vec<(name_idx, mount_node_idx, initial_selected_index)>（ControllerSection 用）
    let mut ctrl_records: Vec<Vec<(u16, u32, i32)>> = Vec::with_capacity(component_count);
    // 每节点（全局）：(parent_idx:i32, kind_tag, style_blob, text_idx, src_idx, class_idx[], id_idx, flags, tabindex, dc_idx, rich_off)
    let mut node_records: Vec<(i32, u8, Vec<u8>, u16, u16, Vec<u16>, u16, u8, i32, u16, u32)> =
        Vec::new();
    // RichText runs arena：所有 RichText 节点的 runs bincode blob 拼接，每段前缀 u32 len。
    // NodeBlock 的 rich_off 字段指向此 arena 的字节偏移。非 RichText 节点用 NULL_RICH_OFF 哨兵。
    let mut rich_runs_arena: Vec<u8> = Vec::new();
    let mut global_node_offset: u32 = 0;
    for (name, nodes, dynamic_rules, controllers) in &input.components {
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
            let (kind_tag, text_idx, src_idx, rich_off) = match &tn.kind {
                NodeKind::Container => (KIND_CONTAINER, NULL_IDX, NULL_IDX, NULL_RICH_OFF),
                NodeKind::Button => (KIND_BUTTON, NULL_IDX, NULL_IDX, NULL_RICH_OFF),
                NodeKind::Image { src } => (
                    KIND_IMAGE,
                    NULL_IDX,
                    intern(src, &mut strings, &mut idx_of),
                    NULL_RICH_OFF,
                ),
                NodeKind::Text { content } => (
                    KIND_TEXT,
                    intern(content, &mut strings, &mut idx_of),
                    NULL_IDX,
                    NULL_RICH_OFF,
                ),
                NodeKind::RichText { runs } => {
                    // runs 整段 bincode 序列化进 rich_runs arena；列存 offset 供 NodeBlock 引用。
                    let bytes = bincode::serialize(runs).expect("serialize RichText runs");
                    let off = rich_runs_arena.len() as u32;
                    rich_runs_arena.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                    rich_runs_arena.extend_from_slice(&bytes);
                    (KIND_RICHTEXT, NULL_IDX, NULL_IDX, off)
                }
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
                rich_off,
            ));
        }
        let node_count = nodes.len() as u32;
        let dynamic_blob =
            bincode::serialize(dynamic_rules).expect("DynamicRuleTable serializable");
        // ControllerSection：intern name + 收 (name_idx, mount_node_idx, initial_selected_index)。
        // name 经 StringTable 去重（与组件名/text/classes 共表）。
        let mut ctrls: Vec<(u16, u32, i32)> = Vec::with_capacity(controllers.len());
        for c in controllers.iter() {
            let name_idx = intern(&c.name, &mut strings, &mut idx_of);
            ctrls.push((name_idx, c.mount_node_idx, c.initial_selected_index));
        }
        comp_records.push((name_idx, comp_base, node_count, dynamic_blob));
        ctrl_records.push(ctrls);
        global_node_offset += node_count;
    }

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
    // RichRunsArena: arena_len(u32) + arena_bytes。放在 NodeBlock 前——read 需在解 NodeBlock
    // 时按 rich_off 查 arena 取 runs blob（需先有 arena 切片）。
    out.extend_from_slice(&(rich_runs_arena.len() as u32).to_le_bytes());
    out.extend_from_slice(&rich_runs_arena);
    // NodeBlock: 每节点 {parent_idx(i32), kind_tag(u8), style_len(u32)+style_blob, text_idx(u16), src_idx(u16),
    //   class_count(u16)+class_idx[], id_idx(u16), flags(u8), tabindex(i32), dc_idx(u16), rich_off(u32)}
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
        rich_off,
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
        out.extend_from_slice(&rich_off.to_le_bytes());
    }
    // PerComponentDynamicRules + PerComponentControllers：每组件 dynamic_blob 紧跟其
    // ControllerSection（同 ComponentTable 顺序）。read 按同序逐组件读。
    for ((_, _, _, dynamic_blob), ctrls) in comp_records.iter().zip(ctrl_records.iter()) {
        out.extend_from_slice(dynamic_blob);
        // ControllerSection: controller_count(u32) + count × {name_idx(u16), mount_node_idx(u32), initial_selected_index(i32)}
        out.extend_from_slice(&(ctrls.len() as u32).to_le_bytes());
        for &(name_idx, mount_idx, initial) in ctrls {
            out.extend_from_slice(&name_idx.to_le_bytes());
            out.extend_from_slice(&mount_idx.to_le_bytes());
            out.extend_from_slice(&initial.to_le_bytes());
        }
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
    // RichRunsArena: arena_len(u32) + arena_bytes。需在 NodeBlock 解析前读出——
    // KIND_RICHTEXT 节点按 rich_off 查 arena 取 runs blob。
    let rich_arena_len = r.u32("rich_arena_len")? as usize;
    let rich_runs_arena: &[u8] = r.take(rich_arena_len, "rich_runs_arena")?;
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
        // rich_off：NULL_RICH_OFF=0xFFFF_FFFF 表非 RichText 节点；否则指向 rich_runs_arena 偏移。
        let rich_off = r.u32("rich_off")?;
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
            KIND_RICHTEXT => {
                // rich_runs_arena 布局：每段 [len_u32][bytes...]。rich_off 指向段首。
                if rich_off == NULL_RICH_OFF {
                    return Err(PkgError::Truncated("rich_off_null_for_richtext"));
                }
                let off = rich_off as usize;
                if off + 4 > rich_runs_arena.len() {
                    return Err(PkgError::Truncated("rich_off_oob"));
                }
                let len =
                    u32::from_le_bytes(rich_runs_arena[off..off + 4].try_into().unwrap()) as usize;
                let blob_start = off + 4;
                let blob_end = blob_start + len;
                if blob_end > rich_runs_arena.len() {
                    return Err(PkgError::Truncated("rich_blob_oob"));
                }
                let runs: Vec<crate::text::rich::RichRun> =
                    bincode::deserialize(&rich_runs_arena[blob_start..blob_end])?;
                NodeKind::RichText { runs }
            }
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
        // ControllerSection (per-component): controller_count(u32) + count × {name_idx(u16), mount_node_idx(u32), initial_selected_index(i32)}
        // name_idx 引用 StringTable（intern 去重，同组件名/text/classes 共表）。
        let ctrl_count = r.u32("controller_count")? as usize;
        let mut controllers = Vec::with_capacity(ctrl_count);
        for _ in 0..ctrl_count {
            let name_idx = r.u16("controller_name_idx")?;
            let mount_node_idx = r.u32("controller_mount_idx")?;
            let initial_selected_index = r.i32("controller_initial_idx")?;
            let name = string_at(&strings, name_idx)?;
            controllers.push(ControllerEntry {
                name,
                mount_node_idx,
                initial_selected_index,
            });
        }
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
                controllers,
            },
        );
    }
    Ok(Package {
        name: String::new(),
        components,
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
    // u16 索引 + NULL_IDX(0xFFFF) 哨兵：真实索引只能 0..65534。下一个串的索引
    // 若 = NULL_IDX 会读回空串、若回绕到 0 会撞首串——均静默 corrupt。打包期直接 panic。
    if strings.len() >= NULL_IDX as usize {
        panic!(
            "string table overflow: StringTable holds {} distinct strings (u16 index, \
             NULL_IDX=0xFFFF reserved); component/text/src/class/id/manifest share this table",
            strings.len()
        );
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
