//! Custom Element 打包期展开（component-system spec §2）：
//!
//! - **注册表 = Package 注册表**（main-design §7.4「Package 注册表承担 customElements.define()
//!   的角色」）：每个 package dir 下 `components/` 子目录的 `*.html` 即组件，文件名 = 标签名
//!   （必须含连字符）。未注册 hyphen 标签 / 无效 slot / 页面裸 `<slot>` / 展开环 → 打包错误。
//! - **展开形状**：host 节点 kind=CustomElement（保留 `custom_tag` 字面量 + `component_scope`
//!   位），组件模板根成为 host 第一个子。host 在页面作用域；内部归展开域（硬墙）。
//! - **slot 投影**：组件模板里的 `<slot name=x>`（无 name = 默认 slot）在拼接位被移除，
//!   替换为 host 的 `slot="x"` light 子（文档序）；无分配子时保留 fallback 原位拼接。
//!   产物中不再有 NodeKind::Slot 节点——slot 是编译期糖。
//! - **作用域规则**：每展开实例一条 (host idx, 组件动态规则) 锚定记录，随 pkg v35 的
//!   PerComponentScopes 段走，instantiate 时按 scope_root=host 包装（Shadow DOM 隔离）。
//! - **投影内容归组件作用域**（spec §2.5 硬墙取舍）：light 子的 CSS/查找/id 都进展开域；
//!   同一展开域内 light 子 id 与组件模板 id 撞车 → 打包错误。

use crate::bridge::{
    attr, extract_classes, extract_control_init, map_semantic, translate_keyframes,
    validate_template_children,
};
use crate::diag::BuildFailure;
use loomgui_core::asset::TemplateNode;
use loomgui_core::scene::{KeyframesRule, NodeKind};
use loomgui_core::style::dynamic::DynamicRuleTable;
use loomgui_fence::ir::{IrNodeKind, IrTree};
use loomgui_fence::schema::tag::SemanticKind;
use loomgui_fence::ParsedTemplate;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// 一个注册组件：fence 已解析的模板 + 规则/动画 + 归一化 sprite 引用。
pub struct ComponentDef {
    pub parsed: ParsedTemplate,
    pub dynamic_rules: DynamicRuleTable,
    pub keyframes: Vec<KeyframesRule>,
    /// img 引用（已按组件文件 html_rel 归一为 sprite_key）——仅被页面用到时进交叉验证。
    pub refs: Vec<String>,
    pub html_rel: String,
}

/// Custom Element 注册表（workspace 级：所有 package 的 components/ 目录并集）。
/// `used` 走 RefCell：展开器持 `&ComponentRegistry`（展开中 immutably 借着组件定义），
/// 用量登记在 walk 途中发生—— interior mutability 免去字段级借用拆分。
pub struct ComponentRegistry {
    defs: HashMap<String, ComponentDef>,
    used: std::cell::RefCell<HashSet<String>>,
}

impl ComponentRegistry {
    /// 空注册表（等价旧行为；页面级 `<slot>` 仍报错——slot 语义只在展开上下文成立）。
    pub fn empty() -> Self {
        ComponentRegistry {
            defs: HashMap::new(),
            used: std::cell::RefCell::new(HashSet::new()),
        }
    }

    /// 从 (名, HTML 源, html_rel) 构建注册表。单测/字符串入口。
    ///
    /// collect-all：单个组件的注册错误（fence Error / 命名 / 单根 / 子树校验）收集成
    /// 诊断、跳过该组件、继续注册后续组件；循环结束若有 error 级诊断则整体失败——
    /// 此时不会走到页面展开，未注册组件不产生连锁噪音。
    pub fn from_sources(
        sources: &[(String, String, String)],
    ) -> Result<(Self, Vec<crate::diag::PackDiagnostic>), BuildFailure> {
        Self::from_sources_with_css(sources, &|_| None)
    }

    /// [`from_sources`] 带外部样式表加载器：组件文件里的 `<link rel="stylesheet">`
    /// 由调用方按 workspace 相对路径提供内容（fence 不做 io）。
    pub fn from_sources_with_css(
        sources: &[(String, String, String)],
        load_css: &dyn Fn(&str) -> Option<String>,
    ) -> Result<(Self, Vec<crate::diag::PackDiagnostic>), BuildFailure> {
        let mut reg = ComponentRegistry::empty();
        let mut diagnostics: Vec<crate::diag::PackDiagnostic> = Vec::new();
        for (name, src, html_rel) in sources {
            reg.register(name.clone(), src, html_rel, load_css, &mut diagnostics);
        }
        if diagnostics
            .iter()
            .any(|d| d.severity == crate::diag::Severity::Error)
        {
            return Err(BuildFailure::validation(
                "component registry has errors",
                diagnostics,
            ));
        }
        Ok((reg, diagnostics))
    }

    /// 注册一个组件：任何错误 push 进 diagnostics 后返回（组件不注册），不中断循环。
    fn register(
        &mut self,
        file_stem: String,
        src: &str,
        html_rel: &str,
        load_css: &dyn Fn(&str) -> Option<String>,
        diagnostics: &mut Vec<crate::diag::PackDiagnostic>,
    ) {
        use crate::diag::{code, PackDiagnostic};
        if !file_stem.contains('-') {
            diagnostics.push(PackDiagnostic::synthetic_error(
                code::COMPONENT_NAME_REQUIRES_HYPHEN,
                &file_stem,
                html_rel,
                format!(
                    "component file `{html_rel}`: name `{file_stem}` must contain a hyphen \
                     (custom element naming)"
                ),
            ));
            return;
        }
        if self.defs.contains_key(&file_stem) {
            diagnostics.push(PackDiagnostic::synthetic_error(
                code::DUPLICATE_COMPONENT_NAME,
                &file_stem,
                html_rel,
                format!("duplicate component name `{file_stem}` in registry"),
            ));
            return;
        }
        let parsed = loomgui_fence::parse_template_with_css(src, html_rel, load_css);
        let has_error = parsed
            .diagnostics
            .iter()
            .any(|d| d.severity == loomgui_fence::diagnostic::Severity::Error);
        diagnostics.extend(
            parsed
                .diagnostics
                .iter()
                .map(|d| PackDiagnostic::from_fence(d, &file_stem, html_rel)),
        );
        if has_error {
            return; // 围栏 Error：组件不注册（循环继续，后续组件诊断照常收集）
        }
        // 单根契约（与页面 bridge 同一条规则）：组件模板必须恰好一个根元素。
        if parsed.tree.roots.len() != 1 {
            diagnostics.push(PackDiagnostic::synthetic_error(
                code::COMPONENT_MULTIPLE_ROOTS,
                &file_stem,
                html_rel,
                format!(
                    "component `{file_stem}` must have exactly one root element (got {})",
                    parsed.tree.roots.len()
                ),
            ));
            return;
        }
        if let Err(e) = validate_template_children(&parsed.tree) {
            diagnostics.push(PackDiagnostic::synthetic_error(
                code::PACK_ERROR,
                &file_stem,
                html_rel,
                format!("component `{file_stem}`: {e}"),
            ));
            return;
        }
        let refs = parsed
            .referenced_sprites
            .iter()
            .map(|s| crate::build::normalize_sprite_key(html_rel, s))
            .collect();
        self.defs.insert(
            file_stem,
            ComponentDef {
                dynamic_rules: DynamicRuleTable {
                    rules: parsed.dynamic_rules.clone(),
                },
                keyframes: translate_keyframes(&parsed.keyframes),
                parsed,
                refs,
                html_rel: html_rel.to_string(),
            },
        );
    }

    fn lookup(&self, tag: &str) -> Option<&ComponentDef> {
        self.defs.get(tag)
    }

    /// 全部注册组件（名 → 定义）。跨文件检查（组件死规则警告）需要以注册表
    /// 全量为被检面——页面树证据由调用方逐页聚合。
    pub fn iter(&self) -> impl Iterator<Item = (&String, &ComponentDef)> {
        self.defs.iter()
    }

    fn mark_used(&self, tag: &str) {
        self.used.borrow_mut().insert(tag.to_string());
    }

    /// 被页面实际用到的组件的 sprite 引用（未用组件是设计期存货，其缺图不阻断构建）。
    pub fn used_refs(&self) -> Vec<String> {
        let used = self.used.borrow();
        let mut out = Vec::new();
        for tag in used.iter() {
            if let Some(def) = self.defs.get(tag) {
                out.extend(def.refs.iter().cloned());
            }
        }
        out
    }
}

/// 扫 workspace 所有 package 的 components/ 目录建注册表。目录不存在 = 跳过（可选设施）。
/// 组件文件读取 io 错误归工具性失败（exit 2）；注册内容错误在 from_sources 内 collect-all。
pub fn scan_component_registry(
    workspace_root: &Path,
    packages: &[crate::workspace::PackageCfg],
) -> Result<(ComponentRegistry, Vec<crate::diag::PackDiagnostic>), BuildFailure> {
    let mut sources: Vec<(String, String, String)> = Vec::new();
    for pkg in packages {
        for dir in &pkg.dirs {
            let comp_dir = workspace_root.join(dir).join("components");
            let Ok(entries) = std::fs::read_dir(&comp_dir) else {
                continue; // 无 components/ 目录 = 无组件，合法
            };
            let mut files: Vec<String> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("html"))
                .filter_map(|p| {
                    let name = p.file_stem()?.to_str()?.to_string();
                    let file = p.file_name()?.to_str()?.to_string();
                    Some(format!("{name}\x00{dir}/components/{file}"))
                })
                .collect();
            files.sort();
            for f in files {
                let (stem, rel) = f.split_once('\x00').expect("joined with \\x00");
                let path = workspace_root.join(rel);
                let src = std::fs::read_to_string(&path)
                    .map_err(|e| format!("read component {}: {e}", path.display()))?;
                sources.push((stem.to_string(), src, rel.to_string()));
            }
        }
    }
    // 组件文件里的 <link rel="stylesheet"> 同页面待遇：按 workspace 相对路径读取。
    let load_css = |css_rel: &str| std::fs::read_to_string(workspace_root.join(css_rel)).ok();
    ComponentRegistry::from_sources_with_css(&sources, &load_css)
}

/// bridge_with_components 的产出。
pub struct ExpansionOutput {
    pub nodes: Vec<TemplateNode>,
    /// 每展开实例一条 (host 模板 idx, 组件动态规则)——进 pkg v35 PerComponentScopes。
    pub scopes: Vec<(usize, DynamicRuleTable)>,
    /// 展开引入的组件 @keyframes（名字冲突由调用方按「宿主优先」裁决 + 警告）。
    pub extra_keyframes: Vec<KeyframesRule>,
    /// 归一化 background-image 路径时收集的 sprite_key（页面 + 各组件文件），
    /// 供 atlas 交叉验证。src 归一 refs 在 pack_components 从 referenced_sprites 收，
    /// bg-image 不走 extract_sprites——这里补齐同口径。
    pub bg_refs: Vec<String>,
}

/// slot 分配表：slot 名（None = 默认 slot）→ host light 子的 IrNodeId 列表（文档序）。
type SlotAssignment = HashMap<Option<String>, Vec<loomgui_fence::ir::IrNodeId>>;

struct Walker<'a> {
    registry: &'a ComponentRegistry,
    /// 展开栈（tag 名）——环检测：a 用 b 用 a 直接报错。
    stack: Vec<String>,
    scopes: Vec<(usize, DynamicRuleTable)>,
    extra_keyframes: Vec<KeyframesRule>,
    /// 活跃展开域的 id 集合（每帧 = 组件模板 id + 拼接进来的 light 子 id）。
    id_frames: Vec<HashSet<String>>,
    nodes: Vec<TemplateNode>,
    /// 归一 bg-image 时收集的 sprite_key（atlas 交叉验证）。
    bg_refs: Vec<String>,
}

/// 带组件注册表的 bridge：页面树 + 展开（host/投影/锚定规则）。
/// 页面级 `<slot>` 在此报错（slot 语义只在组件模板内成立，spec §2.4）。
pub fn bridge_with_components(
    parsed: &ParsedTemplate,
    html_rel: &str,
    registry: &ComponentRegistry,
) -> Result<ExpansionOutput, String> {
    if parsed.tree.roots.len() != 1 {
        return Err(format!(
            "组件 HTML 必须单一根元素（当前 {} 个顶层）",
            parsed.tree.roots.len()
        ));
    }
    validate_template_children(&parsed.tree)?;
    let mut w = Walker {
        registry,
        stack: Vec::new(),
        scopes: Vec::new(),
        extra_keyframes: Vec::new(),
        id_frames: Vec::new(),
        nodes: Vec::new(),
        bg_refs: Vec::new(),
    };
    w.walk_page(parsed, html_rel)?;
    if w.nodes.is_empty() {
        return Err("组件无可实例化节点，产物为空".into());
    }
    Ok(ExpansionOutput {
        nodes: w.nodes,
        scopes: w.scopes,
        extra_keyframes: w.extra_keyframes,
        bg_refs: w.bg_refs,
    })
}

impl<'a> Walker<'a> {
    /// 走页面文件（或任意非展开上下文文件）。
    fn walk_page(&mut self, parsed: &ParsedTemplate, html_rel: &str) -> Result<(), String> {
        let root = parsed.tree.roots[0];
        self.walk_node(parsed, html_rel, root, None, false)
    }

    /// 走一个节点（页面文件或被投影的 light 子）。`in_projection` = 当前处于展开域内
    ///（拼接进来的 light 子，id 须入当前帧检查）。
    fn walk_node(
        &mut self,
        parsed: &ParsedTemplate,
        html_rel: &str,
        nid: loomgui_fence::ir::IrNodeId,
        parent_tpl: Option<usize>,
        in_projection: bool,
    ) -> Result<(), String> {
        let node = &parsed.tree.nodes[nid.0];
        match &node.kind {
            IrNodeKind::Comment(_) | IrNodeKind::Doctype { .. } => Ok(()),
            IrNodeKind::Text(s) => {
                self.emit_text(parsed, nid, s.clone(), parent_tpl);
                Ok(())
            }
            IrNodeKind::Element(el) => {
                if el.semantic == Some(SemanticKind::CustomElement) {
                    return self.expand_host(parsed, html_rel, nid, parent_tpl);
                }
                if el.semantic == Some(SemanticKind::Slot) {
                    // 页面/light 子里的 <slot> 一律非法：slot 只能出现在组件模板里，
                    // 且由投影路径消费（walk_component），不会走到这里。
                    return Err(format!(
                        "<slot> 只能出现在组件模板（components/ 目录）里；页面/light 子不支持 \
                        （{} 的 <{}>）",
                        html_rel, el.tag
                    ));
                }
                let tpl_idx =
                    self.emit_element(parsed, html_rel, nid, parent_tpl, in_projection)?;
                for &c in &node.children {
                    self.walk_node(parsed, html_rel, c, Some(tpl_idx), in_projection)?;
                }
                Ok(())
            }
        }
    }

    /// 走组件模板（展开上下文）：Slot 元素在拼接位被消费（不产节点），
    /// 其余元素正常产出。`assignment` 是 host light 子分配表（消费式 remove）。
    fn walk_component(
        &mut self,
        comp: &ParsedTemplate,
        comp_rel: &str,
        host_idx: usize,
        host_parsed: &ParsedTemplate,
        host_rel: &str,
        assignment: &mut SlotAssignment,
    ) -> Result<(), String> {
        // 组件模板根挂 host 下（host 第一个子；spec §2.3 展开形状）。
        let root = comp.tree.roots[0];
        self.walk_component_node(
            comp,
            comp_rel,
            host_parsed,
            host_rel,
            root,
            Some(host_idx),
            assignment,
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn walk_component_node(
        &mut self,
        comp: &ParsedTemplate,
        comp_rel: &str,
        host_parsed: &ParsedTemplate,
        host_rel: &str,
        nid: loomgui_fence::ir::IrNodeId,
        parent_tpl: Option<usize>,
        assignment: &mut SlotAssignment,
        in_slot_fallback: bool,
    ) -> Result<(), String> {
        let node = &comp.tree.nodes[nid.0];
        match &node.kind {
            IrNodeKind::Comment(_) | IrNodeKind::Doctype { .. } => Ok(()),
            IrNodeKind::Text(s) => {
                self.emit_text(comp, nid, s.clone(), parent_tpl);
                Ok(())
            }
            IrNodeKind::Element(el) => {
                if el.semantic == Some(SemanticKind::Slot) {
                    if in_slot_fallback {
                        return Err(
                            "组件模板不支持 <slot> 嵌套（slot 的 fallback 里再放 slot）".into()
                        );
                    }
                    let key = attr(el, "name");
                    let kids = assignment.remove(&key).unwrap_or_default();
                    if !kids.is_empty() {
                        // 投影：light 子（页面文件）拼到 slot 位，归组件展开域。
                        for &c in &kids {
                            self.walk_node(host_parsed, host_rel, c, parent_tpl, true)?;
                        }
                    } else {
                        // fallback：slot 自身子（组件文件）原位拼接。
                        for &c in &node.children {
                            self.walk_component_node(
                                comp,
                                comp_rel,
                                host_parsed,
                                host_rel,
                                c,
                                parent_tpl,
                                assignment,
                                true,
                            )?;
                        }
                    }
                    return Ok(());
                }
                if el.semantic == Some(SemanticKind::CustomElement) {
                    // 嵌套组件：组件文件里再用 hyphen 标签——以当前组件文件为 light 上下文
                    // 递归展开（环检测在 expand_host 内做）。
                    return self.expand_host(comp, comp_rel, nid, parent_tpl);
                }
                let tpl_idx = self.emit_element(comp, comp_rel, nid, parent_tpl, true)?;
                for &c in &node.children {
                    self.walk_component_node(
                        comp,
                        comp_rel,
                        host_parsed,
                        host_rel,
                        c,
                        Some(tpl_idx),
                        assignment,
                        false,
                    )?;
                }
                Ok(())
            }
        }
    }

    /// 展开 host：注册表查组件 → 发射 host 节点 → 计算分配 → 锚定规则/动画/id 帧 →
    /// 走组件模板（投影感知）。
    fn expand_host(
        &mut self,
        page: &ParsedTemplate,
        page_rel: &str,
        nid: loomgui_fence::ir::IrNodeId,
        parent_tpl: Option<usize>,
    ) -> Result<(), String> {
        let node = &page.tree.nodes[nid.0];
        let IrNodeKind::Element(el) = &node.kind else {
            return Ok(());
        };
        let tag = el.tag.clone();
        let Some(def) = self.registry.lookup(&tag) else {
            return Err(format!(
                "UnregisteredCustomElement: `<{tag}>` 未注册（components/ 目录无 {tag}.html；\
                 main-design §7.4 未注册元素打包期报错）"
            ));
        };
        if self.stack.contains(&tag) {
            return Err(format!(
                "组件展开环：{} -> {tag}（组件递归引用，打包期拒绝）",
                self.stack.join(" -> ")
            ));
        }

        // host 节点：kind=CustomElement + custom_tag + component_scope；样式/id/class 取
        // 页面文件（host 在页面作用域——页面对 host 的选择器照常生效）。
        let ir_idx = nid.0;
        let host_idx = self.nodes.len();
        self.nodes.push(TemplateNode {
            kind: NodeKind::CustomElement,
            style: page.styles.get(ir_idx).cloned().unwrap_or_default(),
            parent_idx: parent_tpl,
            classes: extract_classes(el),
            id_attr: attr(el, "id"),
            draggable: false,
            tabindex: attr(el, "tabindex").and_then(|s| s.parse::<i32>().ok()),
            content: None,
            src: None,
            control_init: None,
            role: attr(el, "role"),
            data_slot: attr(el, "data-slot"),
            aria_controls: attr(el, "aria-controls"),
            rich_text_block: false, // host 是容器壳，不是 rich-text-block 根
            custom_tag: Some(tag.clone()),
            component_scope: true,
        });

        // slot 分配：host light 子按 slot 属性分桶（None = 默认桶；裸文本也进默认桶）。
        // 空白文本节点跳过（不进默认 slot——作者排版缩进不是内容）。
        let mut assignment: SlotAssignment = HashMap::new();
        for &c in &node.children {
            if let IrNodeKind::Text(s) = &page.tree.nodes[c.0].kind {
                if s.trim().is_empty() {
                    continue;
                }
            }
            let cname = match &page.tree.nodes[c.0].kind {
                IrNodeKind::Element(cel) => attr(cel, "slot"),
                _ => None,
            };
            assignment.entry(cname).or_default().push(c);
        }

        // 扫组件模板的 slot 位（名字 → 位置），并校验分配合法性。
        let slot_positions = scan_slots(&def.parsed.tree)?;
        for name in assignment.keys() {
            if !slot_positions.contains_key(name) {
                return Err(match name {
                    Some(n) => format!(
                        "无效 slot：`<{}>` 的 light 子 slot=\"{n}\" 在组件 {tag} 模板里无对应 \
                         <slot name=\"{n}\">（main-design §7.4 无效 slot 打包期报错）",
                        tag
                    ),
                    None => format!(
                        "无效 slot：`<{}>` 有无 slot 属性的 light 子，但组件 {tag} 模板无默认 \
                         <slot>（main-design §7.4 无效 slot 打包期报错）",
                        tag
                    ),
                });
            }
        }

        // 展开域锚定规则 + 组件动画合并（名字冲突由调用方裁决）。
        // 组件 <style> 规则里的 bg-image url 按**组件文件** page_rel 归一（坑 203：
        // class 规则 bg-image 走 dynamic_rules，runtime rematch 用原始值重放）。
        let mut comp_rules = def.dynamic_rules.clone();
        crate::build::normalize_bg_rules(&mut comp_rules, page_rel, &mut self.bg_refs);
        self.scopes.push((host_idx, comp_rules));
        self.extra_keyframes.extend(def.keyframes.iter().cloned());

        // id 帧：组件模板元素与投影 light 子都在 emit 时入帧（双向撞车任一顺序可检；
        // 不预填模板 id——预填会让模板自身元素的 insert 撞自己，凡带 id 的组件必误报）。
        self.id_frames.push(HashSet::new());

        self.stack.push(tag.clone());
        // def（registry 不可变借）在本调用后不再使用——NLL 下借用在 mark_used 前结束，
        // 后续 registry 可变借不冲突。
        self.walk_component(
            &def.parsed,
            &def.html_rel,
            host_idx,
            page,
            page_rel,
            &mut assignment,
        )?;
        self.stack.pop();
        self.id_frames.pop();
        // 用量登记（used_refs 只统计被实际用到的组件，未用组件的缺图不阻断构建）。
        self.registry.mark_used(&tag);
        Ok(())
    }

    /// 发射普通元素节点（页面/组件文件通用）。返回模板 idx。
    fn emit_element(
        &mut self,
        parsed: &ParsedTemplate,
        html_rel: &str,
        nid: loomgui_fence::ir::IrNodeId,
        parent_tpl: Option<usize>,
        in_scope_frame: bool,
    ) -> Result<usize, String> {
        let ir_idx = nid.0;
        let IrNodeKind::Element(el) = &parsed.tree.nodes[ir_idx].kind else {
            unreachable!("emit_element on non-element");
        };
        let kind = map_semantic(el)?;
        // id 入活跃展开帧（light 子 × 组件模板撞车检查，spec §2.5）。
        if in_scope_frame {
            if let Some(id) = attr(el, "id") {
                if let Some(frame) = self.id_frames.last_mut() {
                    if !frame.insert(id.clone()) {
                        return Err(format!(
                            "展开域 id 撞车：`{id}`（light 子与组件模板同 id；投影内容归组件\
                             作用域，同域 id 须唯一）"
                        ));
                    }
                }
            }
        }
        let tpl_idx = self.nodes.len();
        let src = if kind == NodeKind::Image {
            attr(el, "src").map(|s| crate::build::normalize_sprite_key(html_rel, &s))
        } else {
            None
        };
        let mut style = parsed.styles.get(ir_idx).cloned().unwrap_or_default();
        // inline bg-image 按**本文件** html_rel 归一（与 src 同位）——base_style 烘的是
        // 文件相对路径（../res/...），runtime SpriteResolver key 是 workspace 相对
        // （res/...），未归一会 miss → 白纹理（坑 203）。
        if let Some(bg) = style.background_image.take() {
            style.background_image = Some(crate::build::normalize_bg_ref(
                html_rel,
                &bg,
                &mut self.bg_refs,
            ));
        }
        self.nodes.push(TemplateNode {
            kind,
            style,
            parent_idx: parent_tpl,
            classes: extract_classes(el),
            id_attr: attr(el, "id"),
            draggable: false,
            tabindex: attr(el, "tabindex").and_then(|s| s.parse::<i32>().ok()),
            content: None,
            src,
            control_init: extract_control_init(kind, el, ir_idx, &parsed.tree),
            role: attr(el, "role"),
            data_slot: attr(el, "data-slot"),
            aria_controls: attr(el, "aria-controls"),
            rich_text_block: parsed.rich_text_blocks.contains(&ir_idx),
            custom_tag: None,
            component_scope: false,
        });
        Ok(tpl_idx)
    }

    /// 发射文本节点。
    fn emit_text(
        &mut self,
        parsed: &ParsedTemplate,
        nid: loomgui_fence::ir::IrNodeId,
        text: String,
        parent_tpl: Option<usize>,
    ) {
        let ir_idx = nid.0;
        self.nodes.push(TemplateNode {
            kind: NodeKind::TextNode,
            style: parsed.styles.get(ir_idx).cloned().unwrap_or_default(),
            parent_idx: parent_tpl,
            classes: vec![],
            id_attr: None,
            draggable: false,
            tabindex: None,
            content: Some(text),
            src: None,
            control_init: None,
            role: None,
            data_slot: None,
            aria_controls: None,
            rich_text_block: false,
            custom_tag: None,
            component_scope: false,
        });
    }
}

/// 扫组件模板里的 slot 元素：名字（None = 默认 slot）→ 任意一个 IrNodeId（校验存在性用，
/// 拼接由 walk 驱动）。不支持嵌套 slot（fallback 里的 slot 由 walk 阶段报错）。
fn scan_slots(
    tree: &IrTree,
) -> Result<HashMap<Option<String>, loomgui_fence::ir::IrNodeId>, String> {
    let mut out = HashMap::new();
    fn rec(
        tree: &IrTree,
        nid: loomgui_fence::ir::IrNodeId,
        out: &mut HashMap<Option<String>, loomgui_fence::ir::IrNodeId>,
    ) -> Result<(), String> {
        let node = &tree.nodes[nid.0];
        if let IrNodeKind::Element(el) = &node.kind {
            if el.semantic == Some(SemanticKind::Slot) {
                let name = attr(el, "name");
                if let Some(prev) = out.insert(name, nid) {
                    let _ = prev;
                    // 同名双 slot：DOM 允许但等价歧义；围栏哲学取确定报错。
                    return Err(
                        "组件模板有重复 slot（同名或双默认 slot，spec §2.4 确定性要求）".into(),
                    );
                }
            }
        }
        for &c in &node.children {
            rec(tree, c, out)?;
        }
        Ok(())
    }
    for &r in &tree.roots {
        rec(tree, r, &mut out)?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::{pack_components_with_registry, Component, PackResult};

    const CARD_HTML: &str = r#"<div class="gic">
    <slot name="title"><span class="gic-title">默认标题</span></slot>
    <slot name="action"></slot>
</div>"#;

    fn registry_with(card_src: &str) -> ComponentRegistry {
        let (reg, _w) = ComponentRegistry::from_sources(&[(
            "game-item-card".to_string(),
            card_src.to_string(),
            "components/game-item-card.html".to_string(),
        )])
        .expect("registry build ok");
        reg
    }

    fn pack_page(registry: &ComponentRegistry, page: &str) -> PackResult {
        pack_components_with_registry(
            &[Component {
                name: "page".to_string(),
                src: page.to_string(),
                html_rel: "page.html".to_string(),
            }],
            registry,
        )
        .expect("pack ok")
    }

    fn pack_page_err(registry: &ComponentRegistry, page: &str) -> crate::diag::BuildFailure {
        pack_components_with_registry(
            &[Component {
                name: "page".to_string(),
                src: page.to_string(),
                html_rel: "page.html".to_string(),
            }],
            registry,
        )
        .expect_err("pack should fail")
    }

    /// 组件 `<style>` 纯类规则墙外死代码（跨文件证据版）：规则写进组件、元素在
    /// 页面 host 外 → FenceComponentRuleOutOfScope warning。
    #[test]
    fn component_dead_rule_warns_via_page_evidence() {
        let reg = registry_with(
            "<style>.tip-stem { width: 10px }</style>\
             <div class=\"card\"><slot></slot></div>",
        );
        let pr = pack_page(&reg, "<div class=\"tip-stem\"></div>");
        let dead: Vec<_> = pr
            .warnings
            .iter()
            .filter(|w| w.code == "FenceComponentRuleOutOfScope")
            .collect();
        assert_eq!(dead.len(), 1, "恰好一条死规则警告: {:?}", pr.warnings);
        assert!(dead[0].message.contains("tip-stem"));
        assert_eq!(dead[0].file, "components/game-item-card.html");
    }

    /// 运行时挂类（类名全库不出现，is-hover 类惯例）→ 静默，不误报。
    #[test]
    fn component_runtime_class_rule_silent() {
        let reg =
            registry_with("<style>.is-hover { opacity: 0.5 }</style><div class=\"card\"></div>");
        let pr = pack_page(&reg, "<div class=\"unrelated\"></div>");
        assert!(
            pr.warnings
                .iter()
                .all(|w| w.code != "FenceComponentRuleOutOfScope"),
            "无静态墙外证据不断死: {:?}",
            pr.warnings
        );
    }

    /// 展开 happy path：host（custom_tag + component_scope）+ 组件子树 + 锚定规则 +
    /// slot 投影（light 子拼进 slot 位，fallback 丢弃）。
    #[test]
    fn expansion_happy_path() {
        let reg = registry_with(CARD_HTML);
        let pr = pack_page(
            &reg,
            r#"<div style="display:flex"><game-item-card id="c1"><button slot="action" style="display:block">装备</button></game-item-card></div>"#,
        );
        let pkg = loomgui_core::asset::read_package(&pr.bytes).unwrap();
        let comp = pkg.components.get("page").unwrap();
        // host = root 的第一个子
        let host = &comp.nodes[1];
        assert_eq!(host.kind, NodeKind::CustomElement);
        assert_eq!(host.custom_tag.as_deref(), Some("game-item-card"));
        assert!(host.component_scope);
        assert_eq!(host.id_attr.as_deref(), Some("c1"));
        // 组件根 div.gic = host 第一个子；孙辈 = title fallback span（title slot 无分配）
        let gic = &comp.nodes[2];
        assert_eq!(gic.kind, NodeKind::Container);
        assert!(gic.classes.iter().any(|c| c == "gic"));
        // fallback span（title 无 light 子 → fallback 保留）+ 投影 button（action slot）。
        // 空白文本子（模板换行缩进）不计——与普通 bridge 同口径保留，但断言只数元素子。
        let kids: Vec<&TemplateNode> = comp
            .nodes
            .iter()
            .filter(|n| n.parent_idx == Some(2) && n.kind != NodeKind::TextNode)
            .collect();
        assert_eq!(kids.len(), 2, "gic 元素子 = fallback span + 投影 button");
        assert!(
            kids.iter().all(|n| n.kind != NodeKind::Slot),
            "无 Slot 节点残留"
        );
        // 锚定规则：一条，锚 host idx=1
        assert_eq!(comp.component_scopes.len(), 1);
        assert_eq!(comp.component_scopes[0].0, 1);
    }

    /// named slot 无分配 → fallback 保留；有分配 → fallback 丢弃。
    #[test]
    fn slot_fallback_semantics() {
        let reg = registry_with(CARD_HTML);
        // title 无 light 子 → fallback span 保留
        let pr = pack_page(
            &reg,
            r#"<div style="display:flex"><game-item-card></game-item-card></div>"#,
        );
        let pkg = loomgui_core::asset::read_package(&pr.bytes).unwrap();
        let comp = pkg.components.get("page").unwrap();
        let has_fallback = comp
            .nodes
            .iter()
            .any(|n| n.classes.iter().any(|c| c == "gic-title"));
        assert!(has_fallback, "无分配 → fallback 保留");

        // title 有 light 子 → fallback 丢弃
        let pr2 = pack_page(
            &reg,
            r#"<div style="display:flex"><game-item-card><span slot="title">我的卡</span></game-item-card></div>"#,
        );
        let pkg2 = loomgui_core::asset::read_package(&pr2.bytes).unwrap();
        let comp2 = pkg2.components.get("page").unwrap();
        assert!(
            !comp2
                .nodes
                .iter()
                .any(|n| n.classes.iter().any(|c| c == "gic-title")),
            "有分配 → fallback 丢弃"
        );
    }

    /// 默认 slot：无 slot 属性的 light 子 + 裸文本 → 默认 slot（无 name 的 slot）。
    #[test]
    fn default_slot_receives_unslotted_children() {
        let (reg, _) = ComponentRegistry::from_sources(&[(
            "plain-box".to_string(),
            r#"<div class="pb"><slot>占位</slot></div>"#.to_string(),
            "components/plain-box.html".to_string(),
        )])
        .unwrap();
        let pr = pack_page(
            &reg,
            r#"<div style="display:flex"><plain-box><span>内容A</span></plain-box></div>"#,
        );
        let pkg = loomgui_core::asset::read_package(&pr.bytes).unwrap();
        let comp = pkg.components.get("page").unwrap();
        assert!(
            comp.nodes.iter().any(|n| n.kind == NodeKind::TextElement),
            "light 子 span 拼进默认 slot"
        );
        assert!(
            !comp
                .nodes
                .iter()
                .any(|n| n.kind == NodeKind::TextNode && n.content.as_deref() == Some("占位")),
            "默认 slot 有分配 → fallback 文本丢弃"
        );
    }

    /// 未注册 hyphen → 打包错误（UnregisteredCustomElement，main-design §7.4）。
    #[test]
    fn unregistered_custom_element_errors() {
        let reg = ComponentRegistry::empty();
        let err = pack_page_err(
            &reg,
            r#"<div style="display:flex"><ghost-widget></ghost-widget></div>"#,
        );
        assert!(
            err.message.contains("UnregisteredCustomElement")
                && err.message.contains("ghost-widget"),
            "error should name the tag: {err}"
        );
    }

    /// light 子 slot 属性指向不存在的 slot 名 → 错误。
    #[test]
    fn invalid_slot_name_errors() {
        let reg = registry_with(CARD_HTML);
        let err = pack_page_err(
            &reg,
            r#"<div style="display:flex"><game-item-card><span slot="nope">x</span></game-item-card></div>"#,
        );
        assert!(
            err.message.contains("无效 slot") && err.message.contains("nope"),
            "got: {err}"
        );
    }

    /// 无 slot 属性 light 子 + 组件无默认 slot → 错误。
    #[test]
    fn unslotted_child_without_default_slot_errors() {
        let reg = registry_with(CARD_HTML); // 只有 name=title / name=action，无默认 slot
        let err = pack_page_err(
            &reg,
            r#"<div style="display:flex"><game-item-card><span>游离子</span></game-item-card></div>"#,
        );
        assert!(err.message.contains("默认"), "got: {err}");
    }

    /// 页面级（非展开上下文）<slot> → 错误。
    #[test]
    fn page_level_slot_errors() {
        let reg = ComponentRegistry::empty();
        let err = pack_page_err(
            &reg,
            r#"<div style="display:flex"><slot name="x"></slot></div>"#,
        );
        assert!(
            err.message.contains("slot") && err.message.contains("组件模板"),
            "got: {err}"
        );
    }

    /// 嵌套组件：组件文件里再用 hyphen 标签 → 递归展开，双层锚定规则。
    #[test]
    fn nested_components_expand() {
        let (reg, _) = ComponentRegistry::from_sources(&[
            (
                "game-item-card".to_string(),
                CARD_HTML.to_string(),
                "components/game-item-card.html".to_string(),
            ),
            (
                "card-row".to_string(),
                r#"<div class="row"><game-item-card></game-item-card><game-item-card></game-item-card></div>"#
                    .to_string(),
                "components/card-row.html".to_string(),
            ),
        ])
        .unwrap();
        let pr = pack_page(
            &reg,
            r#"<div style="display:flex"><card-row></card-row></div>"#,
        );
        let pkg = loomgui_core::asset::read_package(&pr.bytes).unwrap();
        let comp = pkg.components.get("page").unwrap();
        let hosts = comp
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::CustomElement)
            .count();
        assert_eq!(hosts, 3, "card-row host + 2 game-item-card hosts");
        // 三个展开域锚定：card-row（页面树 idx 1）+ 两个内层 card（各自 host idx）
        assert_eq!(comp.component_scopes.len(), 3);
        // 内层 host 的 component_scope 也置位
        assert_eq!(comp.nodes.iter().filter(|n| n.component_scope).count(), 3);
    }

    /// 展开环（a 用 b 用 a）→ 错误。
    #[test]
    fn expansion_cycle_errors() {
        let (reg, _) = ComponentRegistry::from_sources(&[
            (
                "comp-a".to_string(),
                r#"<div><comp-b></comp-b></div>"#.to_string(),
                "components/comp-a.html".to_string(),
            ),
            (
                "comp-b".to_string(),
                r#"<div><comp-a></comp-a></div>"#.to_string(),
                "components/comp-b.html".to_string(),
            ),
        ])
        .unwrap();
        let err = pack_page_err(&reg, r#"<div style="display:flex"><comp-a></comp-a></div>"#);
        assert!(err.message.contains("环"), "got: {err}");
    }

    /// 同展开域 id 撞车（light 子 id = 组件模板 id）→ 错误。
    #[test]
    fn scope_id_collision_errors() {
        let (reg, _) = ComponentRegistry::from_sources(&[(
            "game-item-card".to_string(),
            r#"<div class="gic" style="display:flex"><span id="shared">常驻</span><slot name="title"></slot></div>"#
                .to_string(),
            "components/game-item-card.html".to_string(),
        )])
        .unwrap();
        let err = pack_page_err(
            &reg,
            r#"<div style="display:flex"><game-item-card><span slot="title" id="shared">撞</span></game-item-card></div>"#,
        );
        assert!(
            err.message.contains("撞车") && err.message.contains("shared"),
            "got: {err}"
        );
    }

    /// 组件文件名无连字符 → 注册表构建错误（合成诊断码 + 描述性 message）。
    #[test]
    fn component_name_requires_hyphen() {
        let err = match ComponentRegistry::from_sources(&[(
            "widget".to_string(),
            r#"<div></div>"#.to_string(),
            "components/widget.html".to_string(),
        )]) {
            Err(e) => e,
            Ok(_) => panic!("no-hyphen name should error"),
        };
        let diag = err
            .diagnostics
            .iter()
            .find(|d| d.code == "ComponentNameRequiresHyphen")
            .expect("无连字符须以合成诊断码暴露");
        assert!(diag.message.contains("hyphen"), "got: {}", diag.message);
        assert_eq!(diag.file, "components/widget.html");
    }

    /// 组件名同名冲突 → 错误（合成诊断码 + 描述性 message）。
    #[test]
    fn duplicate_component_names_error() {
        let err = match ComponentRegistry::from_sources(&[
            (
                "game-item-card".to_string(),
                r#"<div></div>"#.to_string(),
                "a/components/game-item-card.html".to_string(),
            ),
            (
                "game-item-card".to_string(),
                r#"<div></div>"#.to_string(),
                "b/components/game-item-card.html".to_string(),
            ),
        ]) {
            Err(e) => e,
            Ok(_) => panic!("dup should error"),
        };
        let diag = err
            .diagnostics
            .iter()
            .find(|d| d.code == "DuplicateComponentName")
            .expect("重名须以合成诊断码暴露");
        assert!(diag.message.contains("duplicate"), "got: {}", diag.message);
    }

    /// collect-all 回归：两个组件文件各含围栏 Error → 两条 error 诊断都在（修前首错
    /// 即断只报第一个）。注册表阶段失败不走到页面展开，无未注册连锁噪音。
    #[test]
    fn registry_collects_errors_across_components() {
        let err = match ComponentRegistry::from_sources(&[
            (
                "bad-a".to_string(),
                r#"<p>not in fence</p>"#.to_string(),
                "components/bad-a.html".to_string(),
            ),
            (
                "bad-b".to_string(),
                r#"<div role="nope"></div>"#.to_string(),
                "components/bad-b.html".to_string(),
            ),
        ]) {
            Err(e) => e,
            Ok(_) => panic!("two error components should fail"),
        };
        assert_eq!(err.exit_code, 1);
        let errors: Vec<&crate::diag::PackDiagnostic> = err
            .diagnostics
            .iter()
            .filter(|d| d.severity == crate::diag::Severity::Error)
            .collect();
        assert_eq!(errors.len(), 2, "两个组件文件的 error 都要报: {err:?}");
        assert!(errors.iter().any(|d| d.file == "components/bad-a.html"));
        assert!(
            errors.iter().any(|d| d.file == "components/bad-b.html"),
            "修前首错即断会漏掉 bad-b"
        );
    }

    /// 同一组件多实例的 keyframes 重复展开：内容一致 → 静默去重，不逐实例告警。
    #[test]
    fn keyframes_identical_duplicates_silent() {
        let (reg, _) = ComponentRegistry::from_sources(&[(
            "anim-card".to_string(),
            r#"<style>@keyframes fade { from { opacity: 0 } to { opacity: 1 } }</style>
<div class="ac"><slot></slot></div>"#
                .to_string(),
            "components/anim-card.html".to_string(),
        )])
        .unwrap();
        let pr = pack_page(
            &reg,
            r#"<div style="display:flex"><anim-card><span>a</span></anim-card><anim-card><span>b</span></anim-card></div>"#,
        );
        assert!(
            pr.warnings
                .iter()
                .all(|w| w.code != "ComponentKeyframesNameCollision"),
            "内容一致的重复 keyframes 不应告警: {:?}",
            pr.warnings
        );
        let pkg = loomgui_core::asset::read_package(&pr.bytes).unwrap();
        let comp = pkg.components.get("page").unwrap();
        assert_eq!(
            comp.keyframes.iter().filter(|k| k.name == "fade").count(),
            1,
            "去重后只保留一份"
        );
    }

    /// 组件 keyframes 合并：不同名并入宿主；同名宿主胜 + warning。
    #[test]
    fn keyframes_merge_with_host_priority() {
        let (reg, _) = ComponentRegistry::from_sources(&[(
            "anim-card".to_string(),
            r#"<style>@keyframes fade { from { opacity: 0 } to { opacity: 1 } }
@keyframes pulse { from { opacity: 1 } to { opacity: 0.5 } }</style>
<div class="ac"><slot></slot></div>"#
                .to_string(),
            "components/anim-card.html".to_string(),
        )])
        .unwrap();
        let pr = pack_page(
            &reg,
            r#"<style>@keyframes fade { from { opacity: 1 } to { opacity: 0 } }</style>
<div style="display:flex"><anim-card><span>hi</span></anim-card></div>"#,
        );
        let pkg = loomgui_core::asset::read_package(&pr.bytes).unwrap();
        let comp = pkg.components.get("page").unwrap();
        let names: Vec<&str> = comp.keyframes.iter().map(|k| k.name.as_str()).collect();
        assert!(names.contains(&"fade") && names.contains(&"pulse"));
        assert_eq!(
            names.iter().filter(|n| **n == "fade").count(),
            1,
            "同名去重（宿主优先）"
        );
        // fade 是宿主侧（to opacity 0）——宿主优先校验
        let fade = comp.keyframes.iter().find(|k| k.name == "fade").unwrap();
        let to_opacity = fade
            .stops
            .iter()
            .find(|s| matches!(s.selector, loomgui_core::scene::KeyframeStopSelector::To))
            .and_then(|s| s.props.opacity);
        assert_eq!(to_opacity, Some(0.0), "host fade wins (to opacity 0)");
        // 同名碰撞 warning
        assert!(
            pr.warnings
                .iter()
                .any(|w| w.code == "ComponentKeyframesNameCollision"),
            "collision warning surfaced: {:?}",
            pr.warnings
        );
    }

    /// E2E：展开产物 instantiate 后 host 三标记 + 组件内部规则按域隔离 + 查找边界。
    #[test]
    fn expansion_end_to_end_instantiate() {
        let (reg, _) = ComponentRegistry::from_sources(&[(
            "game-item-card".to_string(),
            r#"<style>.gic-body { background-color: #0000ff }</style>
<div class="gic"><slot name="title"></slot><div class="gic-body">内容</div></div>"#
                .to_string(),
            "components/game-item-card.html".to_string(),
        )])
        .unwrap();
        let pr = pack_page(
            &reg,
            r#"<div style="display:flex"><game-item-card id="card"><span slot="title">标题</span></game-item-card></div>"#,
        );
        let mut stage = loomgui_core::stage::Stage::new((1920.0, 1080.0)).unwrap();
        stage.create_root("div", "").unwrap();
        stage.load_package("bag", &pr.bytes).unwrap();
        let root = stage.instantiate("bag", "page").unwrap();
        let scene = stage.scene.as_ref().unwrap();
        let host = scene.get(root).unwrap().children[0];
        assert_eq!(
            scene.get(host).unwrap().custom_tag.as_deref(),
            Some("game-item-card")
        );
        // 页面级查找不穿透 host 内部，host 自身可命中
        assert_eq!(scene.find_node_by_id_in_subtree(root, "card"), Some(host));
        assert_eq!(scene.find_node_by_id_in_subtree(root, "title"), None);
        // 组件内部规则按域包装
        let entries = &scene.dynamic_rules.entries;
        assert!(entries.iter().any(|e| e.scope_root == host));
    }
}
