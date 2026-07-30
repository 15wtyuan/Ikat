# 控件 role 化重构 + 围栏收紧 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把控件从「原生标签 + 框架注入 `.loom-*` 子节点」改成「`<div role="...">` + 作者自写结构」，围栏从 21 标签收紧到 14（砍 input/textarea/select/option/progress/ul/li），并清理契约文档漂移。

**Architecture:** role/aria/data-slot 进 pkg v28 + 运行时 RoleTable side table；resolve_semantic 改 role 驱动；删除 inject_control_children + 改写 sync_control_visuals 按 role/slot 查作者写的子节点；围栏加结构契约校验（必需子角色）+ CSS 契约改 role 驱动；删 PasswordField/SearchField；下线 7 标签 + 重写全部契约文档 + 补「文档↔schema」防漂移门。无并存期，一次性交付。

**Tech Stack:** Rust 2021（core/fence/packer/ffi），C#（Unity 投影层），bincode pkg 格式，xUnit headless 测试。

## Global Constraints

- **Rust edition 2021**，依赖钉版本（taffy 0.12 / ttf-parser 0.20 / slotmap 1.1 / csbindgen 1）。
- **pkg 格式一刀切升**：v27 → v28，`MIN_VERSION = MAX_VERSION = 28`，不留迁移器（`crates/core/src/asset/mod.rs:24-26`）。
- **NodeKind 是 `#[repr(u8)]` 按声明序**：删 PasswordField/SearchField 会 renumber，接受 renumber（pkg v28 拒旧包，判别值无跨版本义务），同步改 `from_u8` + exhaustive 断言 + repr_tests 硬编码值。
- **role 严格用 WAI-ARIA 标准名**：`combobox`（非 dropdown）/`switch`/`spinbutton`/`list`/`listitem`/`option`/`listbox`/`slider`/`progressbar`/`textbox`。
- **视觉部件用 `data-slot`**：`fill`/`thumb`（ARIA 不覆盖内部构造）。
- **围栏外输入打包期报错**，不静默降级（结构契约缺必需子角色 = error）。
- **代码注释写上线品质**：自包含、说 WHY、不引用内部编号。
- **本机是编码机**：Rust 改动后必须重编 + 拷 `.dll`（Unity 必须关着）；改 parse-time 逻辑必须重打 pkg。
- **用户只读中文**：问答/总结中文，commit message 英文，代码注释英文。
- **showcase 实际路径**：`showcase/showcase/*.html`（非 preview/）。
- **可用 subagent model**：仅 `Zhipu/glm-5.2` 与 `杜斌的GLM/glm-5.2`（不用 codemaker 相关）。

## Spec 对照

权威 spec：`docs/superpowers/specs/2026-07-30-control-role-refactor-and-fence-tightening-design.md`。本计划的每个 task 对应 spec 的一节，见各 task 标题的「spec §X」标注。

---

## Task 1: RoleTable side table + role 进 pkg v28

**spec §3.1（RoleTable）+ §3.2 末段（pkg 列）**

建立 role 存储的两层地基：pkg 层（TemplateNode 携带 role/aria/data-slot）+ 运行时层（Scene.roles side table）。后续所有 role 查找都依赖此 task。

**Files:**
- Modify: `crates/core/src/scene/node.rs`（加 `RoleInfo` + `RoleTable` struct，仿 `ControlTable` 约 :527-565；`Scene` 加 `roles` 字段）
- Modify: `crates/core/src/asset/mod.rs:24-26`（bump v28）+ `:98-110`（TemplateNode 加 role 字段）+ write/read_package 的 NodeBlock 布局（约 :240-310）
- Modify: `crates/packer/pkg/src/bridge.rs:74-90`（提取 role/aria/data-slot 写入 TemplateNode）
- Modify: `crates/core/src/scene/stage.rs`（instantiate 时从 TemplateNode 填 RoleTable；remove_node 联动清）
- Test: `crates/core/src/scene/node.rs`（RoleTable 单测）+ `crates/core/src/asset/mod.rs`（pkg v28 往返）

**Interfaces:**
- Produces: `RoleInfo { role: Option<String>, slots: HashMap<String,String> }`（aria 不进表，见决策点 1）；`RoleTable(HashMap<NodeId, RoleInfo>)` with `get/get_mut/insert/remove`；`Scene.roles: RoleTable`；`TemplateNode.role: Option<String>` + `data_slot: Option<String>`；pkg `PKG_FORMAT_VERSION=28`。

- [ ] **Step 1: 定义 RoleInfo + RoleTable（仿 ControlTable）**

在 `crates/core/src/scene/node.rs`，紧邻 `ControlTable` 定义（约 :565 后）加：

```rust
/// 节点的 role/data-slot 信息（打包期从 HTML 提取，运行时只读查表）。
/// role 驱动语义分派 + 控件结构定位（find_child_by_role / find_child_by_slot）。
/// 稀疏：只有带 role/data-slot 的节点进表。运行时态，不进 pkg（pkg 的
/// TemplateNode 携带 role/data_slot 字符串，instantiate 时填进此表）。
///
/// 注：aria-* 属性**不进 RoleInfo**——决策点 1 定为运行时从 ControlState 合成
/// （Task 4 的 attr_matches_node），避免打包期初始值与运行时实时值双源。aria-multiline
/// 等 resolve_semantic 消费的派发提示在 fence 阶段用完即弃。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RoleInfo {
    /// WAI-ARIA role（如 "combobox"/"slider"/"textbox"）。None = 普通 div，无控件语义。
    pub role: Option<String>,
    /// data-slot 值（如 "fill"/"thumb"）。ARIA 不覆盖控件内部视觉构造，用 HTML 标准的
    /// data-* 私有扩展机制表达「这是控件的哪个部件」。
    pub slots: std::collections::HashMap<String, String>,
}

impl RoleInfo {
    /// 是否有任何 role/slot 信息（无则不入表）。
    pub fn is_empty(&self) -> bool {
        self.role.is_none() && self.slots.is_empty()
    }
}

#[derive(Debug, Clone, Default)]
pub struct RoleTable(std::collections::HashMap<NodeId, RoleInfo>);

impl RoleTable {
    pub fn get(&self, id: NodeId) -> Option<&RoleInfo> { self.0.get(&id) }
    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut RoleInfo> { self.0.get_mut(&id) }
    pub fn insert(&mut self, id: NodeId, info: RoleInfo) {
        if !info.is_empty() { self.0.insert(id, info); }
    }
    pub fn remove(&mut self, id: NodeId) { self.0.remove(&id); }
    /// 取节点 role 字符串（find_child_by_role 用）。
    pub fn role_of(&self, id: NodeId) -> Option<&str> {
        self.0.get(&id).and_then(|i| i.role.as_deref())
    }
    /// 取节点某 data-slot 值（find_child_by_slot 用）。
    pub fn slot_of(&self, id: NodeId, slot: &str) -> Option<&str> {
        self.0.get(&id).and_then(|i| i.slots.get(slot).map(|s| s.as_str()))
    }
    pub fn clear(&mut self) { self.0.clear(); }
}
```

`Scene` 结构体（约 :527）加字段（紧邻 `controls`）：
```rust
/// 每节点 role/aria/data-slot（instantiate 从 TemplateNode 填）。运行时态，不进 pkg。
pub roles: RoleTable,
```

- [ ] **Step 2: 写 RoleTable 单测（先失败）**

`crates/core/src/scene/node.rs` 测试块加：

```rust
#[test]
fn role_table_get_insert_remove() {
    let mut t = RoleTable::default();
    let id = NodeId(7);
    assert!(t.get(id).is_none());
    assert_eq!(t.role_of(id), None);
    let info = RoleInfo { role: Some("slider".into()),
        slots: [("thumb".into(), "".into())].into_iter().collect() };
    t.insert(id, info.clone());
    assert_eq!(t.role_of(id), Some("slider"));
    assert_eq!(t.slot_of(id, "thumb"), Some(""));
    // 空 info 不入表
    t.insert(NodeId(8), RoleInfo::default());
    assert!(t.get(NodeId(8)).is_none());
    t.remove(id);
    assert!(t.get(id).is_none());
}
```

Run: `cargo test -p loomgui_core role_table_get_insert_remove`
Expected: FAIL（编译错——RoleTable 未定义，因 Step 1 还没写）→ 写完 Step 1 后 PASS。

实际顺序：Step 1 先写定义，Step 2 写测试立即应 PASS（定义已存在）。按 TDD 严格顺序则先写测试看编译失败，但此处测试与定义同文件，可一起提交。**核心要求：测试存在且通过。**

- [ ] **Step 3: TemplateNode 加 role 字段 + pkg v28**

`crates/core/src/asset/mod.rs`：

```rust
// :24-26
pub const PKG_FORMAT_VERSION: u32 = 28; // v28: TemplateNode role/aria/data-slot (role-driven controls)
pub(crate) const MIN_VERSION: u32 = 28;
pub(crate) const MAX_VERSION: u32 = 28;
```

`TemplateNode`（约 :98-110）加字段。先读该 struct 完整定义确认字段顺序，在末尾（control_init 后）加：

```rust
    /// WAI-ARIA role（"combobox"/"slider"/...）。None = 普通容器/叶子。role 驱动语义分派。
    pub role: Option<String>,
    /// data-* 中的 data-slot 值（"fill"/"thumb"）。控件视觉部件标识。
    pub data_slot: Option<String>,
```

注：aria 属性（aria-checked 等）的初始值——决策点 1 定为运行时从 ControlState 合成，故 **TemplateNode 不存 aria**（避免双源）。aria 仅在 HTML 层作为「作者标记 role=textbox 的多行」等派发提示，resolve_semantic 消费后不进 pkg。

- [ ] **Step 4: write_package / read_package 加 role/data_slot 列**

`crates/core/src/asset/mod.rs` 的 write_package / read_package（约 :240-310，手写逐字段序列化）。先读这两个函数完整体，在 control_init 写/读之后，加 role/data_slot 的写/读（同 id_attr/src 的 Option<String> 模式：写 `0xFF` sentinel 表 None，或写长度+bytes）。

读现有 id_attr 的写法，对 role/data_slot 用同样模式（保持一致）。

- [ ] **Step 5: pkg v28 往返测试**

`crates/core/src/asset/mod.rs` 测试块加（或改现有版本往返测试）：

```rust
#[test]
fn pkg_v28_roundtrip_with_role() {
    let mut nodes = vec![ TemplateNode::/*leaf with role*/ ];
    // 构造一个带 role="slider" + data_slot="thumb" 的 TemplateNode
    // write_package → bytes → read_package → 断言 role/data_slot 字段往返一致
    // 断言 PKG_FORMAT_VERSION == 28
}
#[test]
fn pkg_v27_rejected_as_too_old() {
    // 构造 magic=OK 但 version=27 的 bytes → read_package 返 Err(VersionTooOld)
}
```

Run: `cargo test -p loomgui_core pkg_v28`
Expected: PASS（含 v27 拒绝）。

- [ ] **Step 6: bridge 提取 role/data_slot**

`crates/packer/pkg/src/bridge.rs:74-90`（构造 TemplateNode 处）。先读该函数，在提取 classes/id_attr/src 之后，从 `IrElement.attributes` 提取：

```rust
let role = ir.attributes.iter().find(|(k, _)| k == "role").map(|(_, v)| v.clone());
let data_slot = ir.attributes.iter()
    .find(|(k, _)| k == "data-slot").map(|(_, v)| v.clone());
```

填入 TemplateNode 构造。

- [ ] **Step 7: instantiate 填 RoleTable + remove_node 联动**

`crates/core/src/scene/stage.rs`。先 grep `instantiate`/`create_node_from_template`/`remove_node` 找到：instantiate 遍历 TemplateNode 建树处（填 ControlTable 的同一段），加：

```rust
let info = RoleInfo {
    role: tn.role.clone(),
    slots: tn.data_slot.as_ref().map(|s| [("slot".to_string(), s.clone())].into_iter().collect()).unwrap_or_default(),
};
scene.roles.insert(node_id, info);
```

`remove_node`（清 ControlTable/AnimTable 处）加 `scene.roles.remove(id)`。

- [ ] **Step 8: 全量编译 + 测试 + commit**

```bash
cargo build --workspace
cargo test -p loomgui_core
cargo test -p loomgui_pkg   # bridge 改动
```

Expected: 全绿。注：现有 pkg 测试 fixture 是 v27 格式，bump v28 后读会失败——**所有 fixture 必须重打**（见 Task 10 重打步骤；本 task 先在 asset 测试里用内联构造的 v28 包，不动磁盘 fixture）。若 workspace 编译因 fixture v27 失败，先在 Task 10 前插入一个「重打 fixture」步骤，或本 task 末尾重打（`cargo run -p loomgui_pkg` 对每个 fixture workspace）。

```bash
git add -A
git commit -m "core: add RoleTable side table + TemplateNode role/data_slot (pkg v28)"
```

---

## Task 2: resolve_semantic 改 role 驱动 + 删 input[type] 机制

**spec §3.2**

语义分派从 `(tag, input_type)` 改为 `(tag, role, aria_attrs)`。无 role 的 div/span/button/img 走原 tag 映射；有 role 的 div 走 role→SemanticKind。

**Files:**
- Modify: `crates/fence/src/schema/tag.rs:103-130`（resolve_semantic 重写）
- Modify: `crates/fence/src/annotate.rs:17`（调用方，提取 role 而非 input_type）
- Modify: `crates/fence/src/schema/attr.rs`（`INPUT_STRUCTURAL` type 机制处理——type 属性不再结构性，但保留为可解析全局属性？决策：删 INPUT_STRUCTURAL，type 退为普通全局 attr）
- Test: `crates/fence/src/schema/tag.rs`（resolve_semantic 测试）+ `crates/fence/tests/`（annotate 集成）

**Interfaces:**
- Produces: `resolve_semantic(tag: &str, role: Option<&str>, aria_multiline: bool) -> Option<SemanticKind>`；role→SemanticKind 映射表（见 spec §2.2）。

- [ ] **Step 1: 写 resolve_semantic role 驱动的失败测试**

`crates/fence/src/schema/tag.rs` 测试块：

```rust
#[test]
fn resolve_semantic_role_driven() {
    // div + role → 控件 SemanticKind
    assert_eq!(resolve_semantic("div", Some("combobox"), false), Some(SemanticKind::Dropdown));
    assert_eq!(resolve_semantic("div", Some("slider"), false), Some(SemanticKind::Slider));
    assert_eq!(resolve_semantic("div", Some("spinbutton"), false), Some(SemanticKind::NumberField));
    assert_eq!(resolve_semantic("div", Some("switch"), false), Some(SemanticKind::Toggle));
    assert_eq!(resolve_semantic("div", Some("progressbar"), false), Some(SemanticKind::ProgressBar));
    assert_eq!(resolve_semantic("div", Some("list"), false), Some(SemanticKind::ListView));
    // textbox + aria-multiline
    assert_eq!(resolve_semantic("div", Some("textbox"), false), Some(SemanticKind::TextField));
    assert_eq!(resolve_semantic("div", Some("textbox"), true), Some(SemanticKind::TextArea));
    // 无 role 的基础标签
    assert_eq!(resolve_semantic("div", None, false), Some(SemanticKind::Container));
    assert_eq!(resolve_semantic("span", None, false), Some(SemanticKind::TextElement));
    assert_eq!(resolve_semantic("button", None, false), Some(SemanticKind::Button));
    assert_eq!(resolve_semantic("img", None, false), Some(SemanticKind::Image));
}
```

Run: `cargo test -p loomgui_fence resolve_semantic_role_driven`
Expected: FAIL（签名不匹配）。

- [ ] **Step 2: 重写 resolve_semantic**

```rust
/// role → SemanticKind（WAI-ARIA 标准 role 名）。
const ROLE_TO_SEMANTIC: &[(&str, SemanticKind)] = &[
    ("combobox", SemanticKind::Dropdown),
    ("option", SemanticKind::OptionItem),
    ("listbox", SemanticKind::Container), // listbox 是 combobox 的子容器，无独立 NodeKind
    ("slider", SemanticKind::Slider),
    ("spinbutton", SemanticKind::NumberField),
    ("switch", SemanticKind::Toggle),
    ("radio", SemanticKind::RadioButton),
    ("progressbar", SemanticKind::ProgressBar),
    ("list", SemanticKind::ListView),
    ("listitem", SemanticKind::ListItem),
];

pub fn resolve_semantic(tag: &str, role: Option<&str>, aria_multiline: bool) -> Option<SemanticKind> {
    // role 优先（div + role=控件）
    if let Some(r) = role {
        if let Some((_, kind)) = ROLE_TO_SEMANTIC.iter().find(|(k, _)| *k == r) {
            // textbox 特殊：aria-multiline 区分 TextArea/TextField
            return Some(if r == "textbox" {
                if aria_multiline { SemanticKind::TextArea } else { SemanticKind::TextField }
            } else { *kind });
        }
    }
    // 无 role 的基础标签
    match tag {
        "div" => Some(SemanticKind::Container),
        "span" => Some(SemanticKind::TextElement),
        "button" => Some(SemanticKind::Button),
        "img" => Some(SemanticKind::Image),
        "template" => Some(SemanticKind::Template),
        "slot" => Some(SemanticKind::Slot),
        _ => if tag.contains('-') { Some(SemanticKind::CustomElement) } else { None },
    }
}
```

注：`listbox`/`option` 是子角色，map 到 Container/OptionItem——具体 NodeKind 由 bridge 消费 SemanticKind 时定。`listbox` 无独立 NodeKind（它是 combobox 内的列表容器，普通 Container）。

- [ ] **Step 3: 改 annotate.rs 调用方**

`crates/fence/src/annotate.rs:17`。先读该函数：当前提取 `type` attr 传 input_type。改为提取 `role` + 检测 `aria-multiline="true"`：

```rust
let role = el.attributes.iter().find(|(k, _)| k == "role").map(|(_, v)| v.as_str());
let aria_multiline = el.attributes.iter()
    .any(|(k, v)| k == "aria-multiline" && v == "true");
let semantic = resolve_semantic(&el.tag, role, aria_multiline);
```

- [ ] **Step 4: 删 INPUT_STRUCTURAL + input[type] 机制**

`crates/fence/src/schema/attr.rs:35`：删 `INPUT_STRUCTURAL`（type 不再结构性分派）。`type` 退为普通全局 attr（is_global_attr 已覆盖）。确认无其它引用。

- [ ] **Step 5: 测试通过 + commit**

```bash
cargo test -p loomgui_fence
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings 2>&1 | tail -2
```

```bash
git add -A
git commit -m "fence: resolve_semantic role-driven (WAI-ARIA), drop input[type] dispatch"
```

---

## Task 3: 删除 inject_control_children + find_child_by_role/slot helper + sync_control_visuals 改写

**spec §3.3（注入拆除）**

本 task 是最大的一块。删除框架注入，改为按 role/data-slot 查作者写的子节点。提供 `find_child_by_role` / `find_child_by_slot` helper 替代 `find_child_by_class`。

**Files:**
- Modify: `crates/core/src/scene/control.rs:19-25`（删 FILL/TRACK/THUMB/CHECK/VALUE/POPUP 常量，或保留为 slot/role 字符串常量）
- Modify: `crates/core/src/scene/control.rs:167-222`（删 inject_control_children + make_child）
- Modify: `crates/core/src/scene/control.rs:230-242`（find_child_by_class → 改/加 find_child_by_role/slot）
- Modify: `crates/core/src/scene/control.rs:618-742`（sync_control_visuals 改写）
- Modify: `crates/core/src/scene/dynamic.rs:294`（删 inject_control_children 调用）
- Modify: 各 find_child_by_class 调用点（control.rs ~13 处 + hit.rs:63 + render/mod.rs:206）
- Test: `crates/core/src/scene/control.rs`（find_child_by_role/slot 单测 + sync 用 role 树 fixture）

**Interfaces:**
- Produces: `find_child_by_role(scene, parent, role) -> Option<NodeId>`；`find_child_by_role_recursive`（popup listbox 可能非直接子）；`find_child_by_slot(scene, parent, slot) -> Option<NodeId>`。
- Consumes: RoleTable（Task 1）。

- [ ] **Step 1: 加 find_child_by_role / find_child_by_slot helper + 失败测试**

在 `control.rs`，替代/补充 `find_child_by_class`（:230）：

```rust
/// 在 parent 直接子节点里按 role 找第一个匹配（基于 RoleTable）。
pub fn find_child_by_role(scene: &Scene, parent: NodeId, role: &str) -> Option<NodeId> {
    let children = scene.get(parent)?.children.clone();
    children.into_iter().find(|&cid| scene.roles.role_of(cid) == Some(role))
}
/// 在 parent 直接子节点里按 data-slot 找第一个匹配。
pub fn find_child_by_slot(scene: &Scene, parent: NodeId, slot: &str) -> Option<NodeId> {
    let children = scene.get(parent)?.children.clone();
    children.into_iter().find(|&cid| scene.roles.slot_of(cid, slot).is_some())
}
```

测试（control.rs 测试块）：构造一个带 role 子节点的 Scene，断言 find_child_by_role 命中。

- [ ] **Step 2: 改写常量——slot/role 字符串常量**

control.rs:19-25 的 FILL/TRACK/THUMB/CHECK/VALUE/POPUP 常量改为：

```rust
// role/slot 标识（WAI-ARIA role + data-slot），替代旧的 .loom-* class。
pub const ROLE_LISTBOX: &str = "listbox";
pub const ROLE_OPTION: &str = "option";
pub const SLOT_FILL: &str = "fill";
pub const SLOT_THUMB: &str = "thumb";
pub const SLOT_VALUE: &str = "value"; // dropdown 选中项显示区（data-slot=value）
```

注：Popup 不再用 class 标识——改用 `role=listbox` 定位 popup 容器。**TRACK/CHECK 删除**：旧 TRACK 是 slider 内 fill 的父容器，新结构 slider 直接含 fill+thumb 两个兄弟子节点（spec §2.2 slider 必需子角色只有 thumb，fill 是可选视觉填充），不再需要 track 层；旧 CHECK 是 toggle/radio 的勾节点，spec §2.2 明确不要求（作者用 `[aria-checked=true]` CSS 表达选中态）。

- [ ] **Step 3: 删除 inject_control_children + make_child + inject 调用**

删 control.rs:167-222（inject_control_children 整函数）+ make_child（:38-44）。
删 dynamic.rs:294 的调用（`inject_control_children(scene, id, kind)`）。

- [ ] **Step 4: 改写 sync_control_visuals 按 role/slot 查找**

control.rs:618-742。逻辑骨架保留（value/max→pct→width%、open→display、thumb 位移、value 文本），把：
- `find_child_by_class(id, FILL)` → `find_child_by_slot(id, SLOT_FILL)`
- `find_child_by_class(id, THUMB)` → `find_child_by_slot(id, SLOT_THUMB)`
- `find_child_by_class(id, TRACK)` → **删除**（新结构无 track 层，fill 直接是 slider 子节点）。
- `find_child_by_class(id, VALUE)` → `find_child_by_slot(id, SLOT_VALUE)`
- `find_child_by_class(id, POPUP)` → `find_child_by_role(id, ROLE_LISTBOX)`
- check（Toggle/Radio）：spec §2.2 不要求 check 子节点，删 check 相关 sync（作者用 [aria-checked] CSS 表达选中态）。

- [ ] **Step 5: 改各调用点**

control.rs 内：nth_option_text(:250)、reparent(:281)、dropdown_option_list(:329)、dropdown_option_at_pos(:380)、slider_pos_to_value(:1035)——改为按 role=option / data-slot 查找。

hit.rs:63（collect popup roots）：改为查 `ControlState::Dropdown{open}` 节点的 `role=listbox` 子。

render/mod.rs:206（collect_open_popup_roots）：同上，`find_child_by_role(id, ROLE_LISTBOX)`。

- [ ] **Step 6: reparent_options_into_popup 处理**

`stage.rs:768` reparent_options_into_popup：作者写 `combobox > listbox > option` 时 option 已在 listbox 内，reparent 多余。但若作者把 option 直接写在 combobox 下（结构契约应拦），决策：**保留 reparent 作兜底**（把 combobox 直接子 option 挪进 listbox），结构契约（Task 6）报缺 listbox 的 error。读该函数体，改为按 role=option 识别。

- [ ] **Step 7: 编译 + 测试 + commit**

```bash
cargo build -p loomgui_core
cargo test -p loomgui_core
```

注：此时 showcase 控件运行时会坏（作者还没写 role 结构），但打包不报错（inject 是运行时）。core 单测用 fixture 构造带 role 的树验证。中间态可接受（spec §2.5）。

```bash
git add -A
git commit -m "core: drop inject_control_children, sync/find by role+data-slot"
```

---

## Task 4: 运行时 attr_matches_node 从 ControlState 合成 aria 值

**spec §3.4（[aria-*] 属性选择器）+ 决策点 1**

运行时 rematch（`[aria-checked="true"]` 选择器）需要实时 aria 值。Node 不存属性字面值，故从 ControlState 合成。

**Files:**
- Modify: `crates/core/src/style/dynamic.rs:323-340`（attr_matches_node 改写）
- Test: `crates/core/src/style/dynamic.rs`（aria 匹配测试）

**Interfaces:**
- Produces: attr_matches_node 支持 `[aria-checked]`/`[aria-expanded]`/`[aria-valuenow]`/`[aria-multiline]`/`[role=]`，值从 ControlState/RoleTable 合成。

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn attr_matches_aria_checked_from_control_state() {
    // 构造 Toggle 节点，ControlState::Toggle{checked:true}
    // 断言 attr_matches_node(node_id, "aria-checked", "true") == true
    // 改 checked:false → 断言 == false
}
#[test]
fn attr_matches_aria_expanded_from_dropdown() {
    // Dropdown{open:true} → [aria-expanded="true"] 匹配
}
```

- [ ] **Step 2: 改写 attr_matches_node**

dynamic.rs:323。当前只认 `type`。改为：

```rust
fn attr_matches_node(scene: &Scene, id: NodeId, attr: &str, val: &str) -> bool {
    if let Some(rest) = attr.strip_prefix("aria-") {
        // aria-* 从 ControlState 合成实时值
        let live = synth_aria_value(scene, id, rest);
        return live.as_deref() == Some(val);
    }
    if attr == "role" {
        return scene.roles.role_of(id) == Some(val);
    }
    if attr == "data-slot" {
        return scene.roles.slot_of(id, val).is_some(); // data-slot 值即 slot 名
    }
    // 旧 type 机制已删（Task 2）
    false
}

/// 从 ControlState 合成 aria 属性值（运行时实时，随状态变）。
fn synth_aria_value(scene: &Scene, id: NodeId, aria: &str) -> Option<String> {
    let cs = scene.controls.get(id)?;
    Some(match (aria, cs) {
        ("checked", ControlState::Toggle { checked, .. } | ControlState::RadioButton { checked, .. }) =>
            checked.to_string(),
        ("expanded", ControlState::Dropdown { open, .. }) => open.to_string(),
        ("valuenow", ControlState::Progress { value, .. }
            | ControlState::Slider { value, .. }
            | ControlState::NumberField { edit, .. }) => /* format value */,
        ("multiline", _) => /* TextArea 节点返回 "true"——但 TextArea 无 ControlState 变体？
                              读 node.rs:480-525 确认 TextArea 怎么存，可能从 NodeKind 判 */,
        _ => return None,
    }.to_string())
}
```

注：TextArea 的 aria-multiline 是静态的（打包期定），可从 RoleTable 或 NodeKind 判。读 ControlState 定义确认 TextArea 变体结构后补全。

- [ ] **Step 3: 测试通过 + commit**

```bash
cargo test -p loomgui_core attr_matches
git add -A && git commit -m "core: synth aria-* attr values from ControlState for [aria-*] selectors"
```

---

## Task 5: 删 PasswordField/SearchField（Rust NodeKind + C# 投影层）

**spec §2.3 + §2.4**

删两个 NodeKind 变体 + 全引用清理。Rust + C# 同 commit（否则 PublicApi 编译门红）。接受 enum renumber（pkg v28 拒旧包）。

**Files:**
- Modify: `crates/core/src/scene/node.rs`（enum 定义 :110-111 + from_u8 :149-150 + exhaustive :207-208 + repr_tests :729-769）
- Modify: `crates/core/src/scene/control.rs:50`（transform_display_value PasswordField 掩码）+ `:137`（value_byte_to_display_byte）
- Modify: `crates/core/src/style/dynamic.rs:283-284,332-333`（selector/attr type 映射）
- Modify: `crates/core/src/render/mod.rs:1721-1722` + `dump.rs:36-37` + `examples/spec4b_dump.rs:148-149`
- Modify: `crates/fence/src/schema/tag.rs:111-112` + `bridge.rs:129-130,247` + `control_css_check.rs:40-41,231-232`
- Modify: `unity/package/Runtime/Public/LoomGUI.Nodes.cs:1523-1699`（删两类）+ `NodeKind.cs:34-35` + `NodeFactory.cs:58-59`
- Modify: `tests/dotnet/LoomGUI.HeadlessTests/`（ControlStateProjectionTests:65,80,86 + NodeKindTests:39,42 + TextFieldProjectionTests:114-139）
- Test: Rust 编译（非穷尽 match 报错驱动清理）+ dotnet PublicApi 编译门

- [ ] **Step 1: 删 Rust NodeKind 两变体**

node.rs enum 删 `PasswordField` / `SearchField` 两行。编译报非穷尽 match（control.rs render/dynamic/dump 等）——逐处清理。from_u8 + exhaustive 断言 + repr_tests 硬编码值同步改（renumber：原 PasswordField=18/SearchField=19 删后，Template 等后继变体前移）。

- [ ] **Step 2: 删 PasswordField 掩码逻辑**

control.rs:50 transform_display_value 删 PasswordField arm（`_ => value`）。
control.rs:137 value_byte_to_display_byte 删 PasswordField arm（identity）。**坑 173 随之消除**。

- [ ] **Step 3: 删 fence/bridge/control_css_check 引用**

tag.rs:111-112（resolve_semantic——Task 2 已重写，确认无残留 input type→Password/Search）。
bridge.rs:129-130,247（ControlInit 映射——确认 ControlInit 无 Password/Search variant；若有删之）。
control_css_check.rs:40-41（CONTROL_KINDS 删两项）,231-232（教学文案）。

- [ ] **Step 4: 删 render/dump/dynamic 引用**

逐处删 PasswordField/SearchField match arm。dynamic.rs:283-284（tag→"input" selector 合成）、:332-333（attr type 映射）——Task 2 已删 input[type]，确认此处一致。

- [ ] **Step 5: 删 C# 两类 + NodeKind + NodeFactory**

Nodes.cs:1523-1699 删 PasswordField + SearchField 两 class。
NodeKind.cs:34-35 删两 enum 值。
NodeFactory.cs:58-59 删两 switch arm。

- [ ] **Step 6: 删/改 C# 测试**

HeadlessTests：ControlStateProjectionTests:65,80,86（删 Password/Search 用例）、NodeKindTests:39,42、TextFieldProjectionTests:114-139（若有 Password 断言删）。

- [ ] **Step 7: 编译门验证 + commit**

```bash
cargo build --workspace
cargo test  # Rust 全绿
dotnet build tests/dotnet/LoomGUI.PublicApi/LoomGUI.PublicApi.csproj  # 编译门过
dotnet test tests/dotnet/LoomGUI.HeadlessTests/  # headless 绿
```

```bash
git add -A && git commit -m "core: remove PasswordField/SearchField (web-only, game self-implements)"
```

---

## Task 6: 围栏结构契约校验（必需子角色）+ CSS 契约改 role 驱动

**spec §3.4（严校验两件事）**

新增 Stage 6.8 结构契约校验（必需子角色缺失 = error）+ 改 control_css_check 为 role 驱动。

**Files:**
- Create: `crates/fence/src/control_structure_check.rs`（新阶段，必需子角色契约表 + 校验）
- Modify: `crates/fence/src/pipeline.rs:43`（注册 Stage 6.8，在 control_css 之后）
- Modify: `crates/fence/src/control_css_check.rs`（CONTROL_KINDS 调整 + has_injected_children/loom_children_hint 改 role/slot）
- Modify: `crates/fence/src/lib.rs`（导出新阶段 + DiagnosticCode）
- Test: `crates/fence/src/control_structure_check.rs` + `crates/fence/tests/`

**Interfaces:**
- Produces: `FenceMissingControlChild` DiagnosticCode；`check_control_structure(tree, file, line_map) -> Vec<Diagnostic>`；契约表（每控件 role → 必需子 role/slot 列表，见 spec §2.2）。

- [ ] **Step 1: 写结构契约失败测试**

```rust
#[test]
fn combobox_missing_listbox_errors() {
    // <div role="combobox"> 无 role=listbox 子 → FenceMissingControlChild error
}
#[test]
fn slider_missing_thumb_errors() {
    // <div role="slider"> 无 data-slot=thumb 子 → error
}
#[test]
fn listbox_without_option_errors() {
    // role=listbox 无 role=option 子 → error
}
#[test]
fn combobox_with_full_structure_ok() {
    // <div role="combobox"><div role="listbox"><div role="option">.. 全齐 → 无 error
}
```

- [ ] **Step 2: 实现结构契约表 + check_control_structure**

```rust
/// 每控件 role 的必需子角色/slot 契约（spec §2.2）。
const REQUIRED_CHILDREN: &[(&str, &[CheckSpec])] = &[
    ("combobox", &[CheckSpec::Role("listbox")]),
    ("listbox", &[CheckSpec::Role("option")]), // 至少一个 option
    ("slider", &[CheckSpec::Slot("thumb")]),
    ("progressbar", &[CheckSpec::Slot("fill")]),
    ("list", &[CheckSpec::Role("listitem")]),
];
```

遍历 IrTree，对有 role 的节点查契约，缺必需子 → push Diagnostic（教学文案：「role=combobox 需要一个 role=listbox 子节点，内含 role=option」）。

- [ ] **Step 3: 改 control_css_check role 驱动**

control_css_check.rs：CONTROL_KINDS 改为 role 列表；has_injected_children 改按 role 判断；loom_children_hint 文案改 role/slot 表述。匹配引擎 compound_matches_element（:96，已支持任意 attr）复用。

- [ ] **Step 4: 注册 Stage 6.8**

pipeline.rs:43，在 control_css（6.7）后加 control_structure（6.8）。

- [ ] **Step 5: 测试 + commit**

```bash
cargo test -p loomgui_fence
git add -A && git commit -m "fence: structural contract check (required child roles) + css check role-driven"
```

---

## Task 7: 标签下线（7 个）+ 死代码 + 假测试清理

**spec §2.1 + §4.2 + §4.3**

**前置约束（spec §8）：本 task 必须排在 showcase 改写（Task 8）之后。** 但死代码/假测试清理不依赖 showcase，可提前。为减少中间态，本 task 拆两步：先死代码/假测试（无依赖），后标签下线（排 Task 8 后）。

实际执行顺序：Step 1-3（死代码/假测试）可在 Task 8 前做；Step 4-5（标签下线）必须在 Task 8 后。**plan 按此标注顺序。**

**Files:**
- Modify: `crates/fence/src/schema/tag.rs`（TAGS 注册表删 input/textarea/select/option/progress/ul/li + resolve_semantic 删标签映射——Task 2 已改 resolve_semantic 为 role，确认 tag 映射删干净）
- Modify: `crates/fence/src/structural.rs:143`（删 validate_label_for）+ `:207-217`（ol 分支改 role=listitem）
- Modify: `crates/fence/src/schema/attr.rs:34-46`（删 LABEL_STRUCTURAL/A_STRUCTURAL）
- Modify: `crates/fence/src/inline_context_check.rs:243-251`（删/改 inline_in_text_block_ok 假测试）+ `:76,240`（删 p 豁免注释）
- Modify: `crates/fence/src/diagnostic.rs:22-25`（删撒谎注释）
- Modify: `crates/packer/pkg/src/bridge.rs:92`（validate_template_children tag=="li" → role=listitem）+ structural.rs:207（validate_template_root 同）
- Test: `crates/fence/tests/schema_contract.rs`（14 标签锁定）

- [ ] **Step 1: 删死代码（无依赖，可先做）**

删 structural.rs:143 validate_label_for + 其调用（:22）。
删 structural.rs:207-217 ol 分支（改 role=listitem 判断，ul/li 下线后 list 校验走 role）。
删 attr.rs:34-46 LABEL_STRUCTURAL/A_STRUCTURAL。

- [ ] **Step 2: 删/改假测试与撒谎注释**

inline_context_check.rs:243-251 inline_in_text_block_ok：用 `<p><a>` 恒通过——改为用合法标签（如 `<div><span>`）测真实的 inline-in-block 场景，或删（若该场景已无意义）。
inline_context_check.rs:76,240 + diagnostic.rs:22-25：删 `<p>` 豁免的虚假描述。

- [ ] **Step 3: bridge/structural 的 li 判断改 role**

bridge.rs:92 validate_template_children + structural.rs:207 validate_template_root：`tag=="li"` → 读 attrs 判 `role=listitem`（structural 在 annotate 前无 semantic，直接看 attr）。

- [ ] **Step 4: 【必须在 Task 8 后】下线 7 标签**

tag.rs TAGS 注册表删 input/textarea/select/option/progress/ul/li 7 个 TagSpec。
resolve_semantic（Task 2 已改 role 驱动）确认这 7 标签的 tag 映射已无（只走 role）。

- [ ] **Step 5: schema_contract 改 14 标签锁定 + 修 shell_tags_are_seven**

```rust
#[test]
fn all_6_runtime_tags_have_specs() { /* div span button img template slot */ }
#[test]
fn shell_tags_are_eight() { /* 含 script，断言 len()==8 */ }
#[test]
fn removed_tags_rejected() { /* input/select/.../ul/li 断言不在 */ }
```

- [ ] **Step 6: 测试 + commit**

```bash
cargo test -p loomgui_fence
git add -A && git commit -m "fence: retire 7 control/list tags, clean dead code (label/ol) + false tests"
```

---

## Task 8: showcase 全改 div+role

**spec §5**

7+ HTML 文件（character/components/stat-bar/form/inventory/mail/settings/shop）的所有控件标签（input×26/progress×8/li×6/option×7/select×2/textarea×2/ul×3）改 `<div role=...>` + 必需子结构。

**前置：Task 1-6 完成**（role 机制 + 结构契约校验就位，showcase 改写后能被校验拦错）。

**Files:**
- Modify: `showcase/showcase/character.html`、`form.html`、`inventory.html`、`mail.html`、`settings.html`、`shop.html`、`components/stat-bar.html`
- Modify: 对应 CSS（class 选择器不变，但补 `[aria-checked]`/`[aria-expanded]` 状态样式）

- [ ] **Step 1: form.html 改写（大头，input×11/select/textarea/option）**

每个 `<input type=text>` → `<div role="textbox" class=...>`
`<input type=number>` → `<div role="spinbutton" aria-valuenow=.. aria-valuemin=.. aria-valuemax=..>`
`<select><option>` → `<div role="combobox"><div role="listbox"><div role="option">..`
`<textarea>` → `<div role="textbox" aria-multiline="true">`

- [ ] **Step 2: settings.html（input×14/select/option）+ 其余文件**

逐文件改。slider/progress/toggle/radio 同理补 data-slot=thumb/fill、role=switch/radio + [aria-checked] CSS。

- [ ] **Step 3: ul/li → role=list/listitem**

character/inventory/mail 的 `<ul><li>` → `<div role="list"><div role="listitem">`。

- [ ] **Step 4: 打包验收**

```bash
cargo run -p loomgui_pkg -- build showcase
```
Expected: exit 0（结构契约 + CSS 契约校验通过 = 集成验收面）。

- [ ] **Step 5: commit**

```bash
git add showcase/ && git commit -m "showcase: rewrite all controls to div+role (WAI-ARIA)"
```

---

## Task 9: 文档全重写 + 防漂移门 + schema_contract 修

**spec §4.1 + §4.4**

**前置：Task 7 标签下线完成**（文档写终态 14 标签）。

**Files:**
- Modify: `AGENTS.md`、`CLAUDE.md`（31→14 标签 + 控件 role 化描述）
- Modify: `docs/design/fence.md`（主表 + 十余处散文）
- Modify: `docs/design/main-design.md`（:86-87 display 列表、:98-106 类型表、:117-130 映射、:172-180 对象树图删 TextBlock/Label/Link/Canvas/LineBreak、:480 block 列表）
- Modify: `docs/design/public-api.md`（:50-62 对象树、:62 Container 子类、:295/:310-311 删 Link/Label/Canvas、划线依据改 §2.4）
- Modify: `showcase/showcase/preview/README.md`（:30,:40 标签列表 + 特性承诺）
- Create: `crates/fence/tests/doc_schema_sync.rs`（文档↔schema 交叉门）
- Modify: `crates/fence/tests/schema_contract.rs`（已在 Task 7 Step 5 改，确认）

- [ ] **Step 1: AGENTS.md + CLAUDE.md 数字与描述**

「31 标签 = 8 shell + 23 runtime」→「14 标签 = 8 shell + 6 runtime」。控件段改 role 化（删 .loom-* 描述，加 role/data-slot 契约）。两文件保持同步（项目惯例）。

- [ ] **Step 2: fence.md 主表 + 散文**

主表（§2.2/§3.1）改 6 runtime 标签 + role→控件映射表。删 strong/em/label/a/canvas/p/ol 等已删标签的散文示例。控件 CSS 校验段（§6.7）改 role 驱动描述。

- [ ] **Step 3: main-design.md 五处**

删 display 列表里的已删标签；标签→类型表删 p/a/label/canvas；对象树图删 TextBlock/Label/Link/Canvas/LineBreak 幻影类型；映射列表改 role；block 默认标签列表删 header/nav/p/ol。

- [ ] **Step 4: public-api.md 对象树 + 划线依据**

按 spec §2.4 重写：对象树图、Container 子类列表、划线依据（content model → 子树归属）、删 Link/Label/Canvas 行为契约。

- [ ] **Step 5: preview/README.md**

删「23 runtime tags」整行旧列表 + 富文本/canvas/有序列表特性承诺。

- [ ] **Step 6: 写文档↔schema 防漂移门**

`crates/fence/tests/doc_schema_sync.rs`：从 fence.md 主表解析标签清单（正则提取表格行），与 `TAGS` 注册表 + `SHELL_TAGS` 比对，不一致 fail。

```rust
#[test]
fn fence_md_tags_match_schema() {
    let md = include_str!("../../../docs/design/fence.md");
    let parsed: Vec<String> = /* 正则提主表标签 */;
    let schema: Vec<String> = TAGS.iter().map(|t| t.name).chain(SHELL_TAGS.iter()).map(|s| s.to_string()).collect();
    assert_eq!(parsed, schema, "fence.md 标签表与 schema 注册表不一致（文档漂移）");
}
```

- [ ] **Step 7: commit**

```bash
git add -A && git commit -m "docs: rewrite all contracts to 14-tag role-driven terminal state + doc-schema sync gate"
```

---

## Task 10: 重编 dll + 重打全部 pkg + 全测试验收

**spec §6（测试策略）+ AGENTS.md 闭环**

**Files:**
- Modify: `unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll`（重编）
- Modify: `unity/package/Editor/Tools/loomgui_gui.exe`（重编，绑 fence）
- Modify: `unity/showcase-unity/Assets/Bundles/ui/showcase.pkg.bin`（重打）
- Modify: `crates/packer/pkg/tests/fixtures/*.pkg.bin`（重打所有 v27 fixture → v28）
- Modify: `tests/dotnet/LoomGUI.HeadlessTests/fixtures/*.pkg.bin`（重打）

- [ ] **Step 1: 重编 release dll（Unity 必须关着）**

```bash
# 确认 Unity 关
cargo build -p loomgui_ffi_c --release
cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
```

- [ ] **Step 2: 重打所有 pkg fixture**

每个 fixture workspace 重打（v27→v28）：
```bash
# Rust fixtures
for ws in crates/packer/pkg/tests/fixtures/*.workspace; do
  cargo run -p loomgui_pkg -- build "$ws"
done
# dotnet fixtures
for ws in tests/dotnet/LoomGUI.HeadlessTests/fixtures/*.workspace; do
  cargo run -p loomgui_pkg -- build "$ws"
done
```

- [ ] **Step 3: 重打 showcase pkg**

```bash
cargo run -p loomgui_pkg -- build showcase
```

- [ ] **Step 4: 重编 GUI exe（绑 fence）**

```bash
(cd crates/packer/gui/src-tauri && tauri build --no-bundle)
cp target/release/loomgui_gui.exe unity/package/Editor/Tools/loomgui_gui.exe
```
注：本环境 tauri-cli 是 npm 全局，用裸 `tauri build --no-bundle`（非 `cargo tauri`）。

- [ ] **Step 5: 同步 C# 绑定**

```bash
cargo run -p xtask -- sync-bindings
```

- [ ] **Step 6: 全测试验收**

```bash
cargo test                                    # 全 workspace
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
dotnet test tests/dotnet/LoomGUI.HeadlessTests/
dotnet build tests/dotnet/LoomGUI.PublicApi/LoomGUI.PublicApi.csproj
```
Expected: 全绿。

- [ ] **Step 7: commit**

```bash
git add -A && git commit -m "build: rebuild dll/exe + repack all pkg v28 fixtures; full suite green"
```

---

## 实施顺序总览（遵守 spec §8 约束）

```
Task 1 (RoleTable + pkg v28)
  → Task 2 (resolve_semantic role)
  → Task 3 (删 inject + sync 改写)
  → Task 4 (aria 合成)
  → Task 5 (删 Password/Search)
  → Task 6 (结构契约 + CSS 契约)
  → Task 8 (showcase 改写)        ← 必须在 Task 7 标签下线前
  → Task 7 (标签下线 + 死代码)     ← 标签下线排 showcase 后
  → Task 9 (文档 + 防漂移门)       ← 排标签下线后
  → Task 10 (重编 + 验收)
```

**注**：Task 7 的 Step 1-3（死代码/假测试，无依赖）可提前到 Task 8 前做；Step 4-5（标签下线）严格排 Task 8 后。dispatch 时按上述序号顺序执行即可（7 排 8 后）。
