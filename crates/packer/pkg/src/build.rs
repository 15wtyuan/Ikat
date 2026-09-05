//! Build orchestration: atlases + fonts + packages (HTML -> .pkg.bin) + runtime manifest -> output_dir.
//! Single entry point build() called by CLI and GUI.
//!
//! HTML -> .pkg.bin 编排：fence parse_template + bridge + write_package。
//! referenced_sprites 回接 atlas 交叉验证（assign_and_validate，缺失 sprite 非静默）。

use serde::{Deserialize, Serialize};

use crate::atlas::collect::collect_pngs;
use crate::atlas::pack::pack_atlas;
use crate::bridge::translate_keyframes;
use crate::diag::{code, BuildFailure, PackDiagnostic, Severity};
use crate::expand::{bridge_with_components, ComponentRegistry};
use crate::runtime::{RuntimeFont, RuntimeManifest, RUNTIME_FILE};
use crate::workspace::{load_workspace, PackageCfg};
use std::path::Path;
use yio_core::asset::{write_package_with_scopes, ComponentScopeInput, PackageInput, TemplateNode};
use yio_core::scene::KeyframesRule;
use yio_core::style::dynamic::DynamicRuleTable;
use yio_core::style::resolved::BorderStyle;

/// Build report: what was produced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildReport {
    /// Package names (one per PackageCfg, written to ui/<name>.pkg.bin).
    pub packages: Vec<String>,
    pub atlases: Vec<String>,
    pub fonts: Vec<String>,
    pub log: Vec<String>,
    /// 围栏内一致性 warning（如 border-width 无 border-style、bg-image 无 size）：
    /// 合法但预览 ≠ 运行时的不一致。不阻断打包，供 CLI 打印 / GUI 呈现提醒作者
    /// 补全声明。空 = 无 warning。
    pub warnings: Vec<PackDiagnostic>,
}

/// 一个待打包的组件：名字 + HTML 源码 + 该 HTML 相对 workspace_root 的路径（正斜杠）。
/// `html_rel` 仅用于把 img src 归一化为 sprite_key（见 `normalize_sprite_key`），
/// 不参与 fence/bridge——后者只关心 `name` + `src`。
pub struct Component {
    pub name: String,
    pub src: String,
    pub html_rel: String,
}

/// `pack_components` 的产出：pkg.bin 字节 + 引用到的 sprite_key 集合 + 围栏诊断（warning）。
/// 用具名结构体而非裸 3-tuple——`Result<(Vec<u8>, Vec<String>, Vec<PackDiagnostic>), BuildFailure>`
/// 会触发 clippy type_complexity，且调用点 `let (bytes, refs, _w) = ...` 的位置语义不自说明。
/// 具名后调用点 `let PackResult { bytes, referenced_sprites, warnings } = ...` 清晰可读。
#[derive(Debug, Clone)]
pub struct PackResult {
    /// pkg.bin 字节（write_package 产出，可直接写 ui/<name>.pkg.bin）。
    pub bytes: Vec<u8>,
    /// 所有组件 img src / background-image 并集，已归一化为 workspace_root 相对路径
    /// （sprite_key 口径），供 atlas 交叉验证。
    pub referenced_sprites: Vec<String>,
    /// 围栏一致性 warning（如 border-width 无 style、bg-image 无 size）：合法但预览≠
    /// 运行时的不一致，不阻断打包，供 CLI/GUI 呈现。
    pub warnings: Vec<PackDiagnostic>,
}

/// 打包一个 package：components = [Component]。返 [`PackResult`]。
/// build() 读文件组装 Component 调本函数；本函数接字符串便于单测。
///
/// 流程：每组件 `parse_template` → `bridge` → 累积；末尾 `write_package` 出 pkg.bin。
/// fence Error 级 diagnostic 收集后跨组件累积（collect-all：全部组件解析完才失败，
/// 诊断一次给全）；Warning 级不阻断打包，收集进返回值 `warnings` 供 CLI/GUI 呈现。
/// bridge 多根 → Err（不静默产森林）。
pub fn pack_components(components: &[Component]) -> Result<PackResult, BuildFailure> {
    pack_components_inner(components, &ComponentRegistry::empty(), &|_| None)
}

/// 带 Custom Element 注册表的打包：hyphen 标签在打包期展开（slot 投影 + 展开域锚定
/// 规则），见 expand.rs / component-system spec。注册表通常来自 build() 的
/// components/ 目录扫描（scan_component_registry）；空注册表时退化为旧行为
///（页面级 `<slot>` 仍报错）。
pub fn pack_components_with_registry(
    components: &[Component],
    registry: &ComponentRegistry,
) -> Result<PackResult, BuildFailure> {
    pack_components_inner(components, registry, &|_| None)
}

/// 带 CSS 加载器的打包入口：页面 HTML 里的 `<link rel="stylesheet">` 由调用方按
/// workspace 相对路径提供 CSS 内容（fence 不做 io）。生产路径（analyze）传工作区根
/// 读取器；字符串单测走上面对应入口（无外部样式表）。
pub fn pack_components_with_css(
    components: &[Component],
    registry: &ComponentRegistry,
    load_css: &dyn Fn(&str) -> Option<String>,
) -> Result<PackResult, BuildFailure> {
    pack_components_inner(components, registry, load_css)
}

/// pack_components_inner 的累积条目：一个已 bridge 的页面组件（+ 组件展开域锚定规则）。
struct BuiltComponent {
    name: String,
    html_rel: String,
    nodes: Vec<TemplateNode>,
    rules: DynamicRuleTable,
    keyframes: Vec<KeyframesRule>,
    scopes: Vec<(usize, DynamicRuleTable)>,
}

fn pack_components_inner(
    components: &[Component],
    registry: &ComponentRegistry,
    load_css: &dyn Fn(&str) -> Option<String>,
) -> Result<PackResult, BuildFailure> {
    let mut built: Vec<BuiltComponent> = Vec::new();
    let mut refs: Vec<String> = Vec::new();
    // collect-all：诊断（error + warning）跨组件全量收集，收完才判失败——AI 一轮修全。
    // 失败时 warning 也随 BuildFailure.diagnostics 一并给出。
    let mut diagnostics: Vec<PackDiagnostic> = Vec::new();
    // 本包页面类名分桶——组件 `<style>` 死规则检查（跨文件证据版）的墙外证据。
    let mut scope_buckets = yio_fence::component_scope_check::ScopeClassBuckets::new();
    for comp in components {
        let Component {
            name,
            src,
            html_rel,
        } = comp;
        let parsed = yio_fence::parse_template_with_css(src, html_rel, load_css);
        scope_buckets.add_page_tree(&parsed.tree);
        // 该组件全部围栏诊断先进收集（Error+Warning）。Error 存在则跳过 bridge
        //（坏树不值得展开），但循环继续——后续组件的诊断也要给作者。
        let has_error = parsed
            .diagnostics
            .iter()
            .any(|d| d.severity == yio_fence::diagnostic::Severity::Error);
        diagnostics.extend(
            parsed
                .diagnostics
                .iter()
                .map(|d| PackDiagnostic::from_fence(d, name, html_rel)),
        );
        if has_error {
            continue;
        }
        // 组件展开 bridge（registry 为空时除页面级 <slot> 报错外与旧 bridge 等价）：
        // 展开 CustomElement host + slot 投影 + 收集展开域锚定规则与组件动画。
        // bridge 错误首错即断（展开期无 collect-all 机制），已收集的诊断随失败带出。
        // 错误本身必须落进 diagnostics（合成 Error）：analyze 只消费 diagnostics，
        // message-only 的失败在那里会被丢弃——包静默消失而 build 仍报 OK。
        let out = match bridge_with_components(&parsed, html_rel, registry) {
            Ok(out) => out,
            Err(e) => {
                diagnostics.push(PackDiagnostic::synthetic_error(
                    code::PACK_ERROR,
                    name,
                    html_rel,
                    e,
                ));
                return Err(BuildFailure::validation(
                    format!("bridge error in component {name}"),
                    diagnostics,
                ));
            }
        };
        // Image.src 归一化已在 walker emit 时按各文件 html_rel 完成（页面文件 + 展开的
        // 组件文件各归各的——组件 src 相对组件文件，不能再按页面 html_rel 二次归一）。
        // 组件 @keyframes 合并：宿主（页面文件）优先，展开引入的组件动画按名去重。
        // 同名且内容一致 → 静默去重（同一组件多实例、或不同组件定义了等价动画——
        // 无分歧不出声，告警只是噪音）；同名且内容不同 → warning（真碰撞：宿主/
        // 先到者胜出，组件侧被忽略；如需同时生效请改名）。
        let mut keyframes = translate_keyframes(&parsed.keyframes);
        for kf in out.extra_keyframes {
            match keyframes.iter().find(|k| k.name == kf.name) {
                None => keyframes.push(kf),
                Some(existing) if *existing == kf => {}
                Some(_) => diagnostics.push(PackDiagnostic::synthetic_warning(
                    code::COMPONENT_KEYFRAMES_COLLISION,
                    name,
                    html_rel,
                    format!(
                        "组件动画 `@keyframes {}` 与宿主（或先展开组件）同名且内容不同——\
                         宿主优先，组件侧被忽略；如需同时生效请改名",
                        kf.name
                    ),
                )),
            }
        }
        // border 与 background-image/gradient 共存 warning（render 层互斥门：有图/渐变
        // 时彩色边框环静默不画，fence.md 边框节限制②）。作者端无感知时难分「自己写错
        // 还是框架不支持」——打包期点破。九宫格 slice 与 clip:text 会抑制渐变（此时
        // 边框照画），按 render 同款条件排除，不误报。
        for tn in &out.nodes {
            let s = &tn.style;
            let border_renderable = s.border_style != BorderStyle::None
                && s.border_color.is_some()
                && [
                    &s.taffy_style.border.top,
                    &s.taffy_style.border.right,
                    &s.taffy_style.border.bottom,
                    &s.taffy_style.border.left,
                ]
                .iter()
                .any(|e| {
                    let cl = e.into_raw();
                    cl.tag() == taffy::style::CompactLength::LENGTH_TAG && cl.value() > 0.0
                });
            let has_image = s.background_image.is_some();
            let has_slice = s.border_image_slice.is_some();
            let use_gradient = !has_image
                && !has_slice
                && !s.background_clip_text
                && s.background_gradient.is_some();
            if border_renderable && (has_image || use_gradient) {
                let where_ = match &tn.id_attr {
                    Some(id) => format!("#{id}"),
                    None => format!("{:?}", tn.kind),
                };
                diagnostics.push(PackDiagnostic::synthetic_warning(
                    code::BORDER_BG_EXCLUSIVE,
                    name,
                    html_rel,
                    format!(
                        "节点 {where_} 的彩色边框与 background-{} 共存——互斥渲染，边框不会画出\
                         （改纯色背景或去掉边框）",
                        if has_image { "image" } else { "gradient" },
                    ),
                ));
            }
        }
        // 页面文件 <style> class 规则的 background-image / background url() 归一——runtime
        // rematch 拿 declarations 的原始 url 值调 apply_decl，未归一会让 SpriteResolver miss
        // （标本馆 .bg-cover/.bg-contain 白块：bg-image 在 class 规则里）。inline
        // bg-image 与展开组件文件的规则已在 walker emit 时按各自 html_rel 归一（expand.rs），
        // refs 收进 out.bg_refs。
        let mut dynamic_rules = DynamicRuleTable {
            rules: parsed.dynamic_rules,
        };
        normalize_bg_rules(&mut dynamic_rules, html_rel, &mut refs);
        refs.extend(out.bg_refs);
        built.push(BuiltComponent {
            name: name.clone(),
            html_rel: html_rel.clone(),
            nodes: out.nodes,
            rules: dynamic_rules,
            keyframes,
            scopes: out.scopes,
        });
        // img src 相对 HTML 文件；归一化为 sprite_key（相对 workspace_root，正斜杠），
        // 否则与 atlas collect 的 sprite_key 前缀不匹配 → 交叉验证挂。
        for img_src in &parsed.referenced_sprites {
            refs.push(normalize_sprite_key(html_rel, img_src));
        }
    }
    // 同名组件：write_package 不查（返回 Vec<u8> 无 Result），read_package 运行时才
    // DupComponent 拒绝——产物是静默坏包。构建期 fail fast，给最早反馈。
    let mut seen = std::collections::HashSet::new();
    for b in &built {
        if !seen.insert(b.name.as_str()) {
            diagnostics.push(PackDiagnostic::synthetic_error(
                code::DUPLICATE_COMPONENT_NAME,
                &b.name,
                &b.html_rel,
                format!("duplicate component name `{}` in package", b.name),
            ));
        }
    }
    // 组件 `<style>` 纯类规则墙外死代码检查（跨文件证据版）：本包页面树作证据、
    // workspace 注册表全量组件作被检面。warning 级，随包报告给作者。
    for (comp_name, def) in registry.iter() {
        let input = yio_fence::component_scope_check::ComponentScopeInput {
            name: comp_name,
            html_rel: &def.html_rel,
            tree: &def.parsed.tree,
            rules: &def.parsed.dynamic_rules,
        };
        let mut fence_diags = Vec::new();
        yio_fence::component_scope_check::warn_component_rules_out_of_scope(
            &[input],
            &scope_buckets,
            &mut fence_diags,
        );
        diagnostics.extend(
            fence_diags
                .iter()
                .map(|d| PackDiagnostic::from_fence(d, comp_name, &def.html_rel)),
        );
    }
    // collect-all 收尾：任一 error → 整体失败（exit 1），诊断全量随失败带出。
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        let errors = diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count();
        return Err(BuildFailure::validation(
            format!("{errors} error(s) in package"),
            diagnostics,
        ));
    }
    let comp_refs: Vec<(&str, &[TemplateNode], &DynamicRuleTable, &[KeyframesRule])> = built
        .iter()
        .map(|b| {
            (
                b.name.as_str(),
                b.nodes.as_slice(),
                &b.rules,
                b.keyframes.as_slice(),
            )
        })
        .collect();
    let scopes: Vec<ComponentScopeInput> = built
        .iter()
        .flat_map(|b| {
            b.scopes.iter().map(|(anchor, rules)| ComponentScopeInput {
                component: b.name.as_str(),
                anchor_idx: *anchor,
                rules,
            })
        })
        .collect();
    let bytes = write_package_with_scopes(
        &PackageInput {
            components: comp_refs,
        },
        &scopes,
    );
    Ok(PackResult {
        bytes,
        referenced_sprites: refs,
        // 到这里 diagnostics 只剩 warning（error 已在上面整体失败返回）。
        warnings: diagnostics,
    })
}

/// 把 PackageCfg 解析成 HTML 文件相对路径列表（相对工作区根，正斜杠）。
/// `html` 非空 = 显式态（锁定文件，原样返回）；空 = 自动态（扫 `dirs` 顶层 `*.html`，排序保稳定）。
/// 自动态仅扫顶层（非递归）：避免误纳子目录的设计系统/模板片段。
/// workspace_cmd（list/show/new）与 build/analyze 共用。
pub fn resolve_html_list(workspace_root: &Path, pkg: &PackageCfg) -> Result<Vec<String>, String> {
    if !pkg.html.is_empty() {
        return Ok(pkg.html.clone());
    }
    let mut out = Vec::new();
    for dir in &pkg.dirs {
        let full = workspace_root.join(dir);
        if !full.is_dir() {
            return Err(format!(
                "package `{}` dir not found: {}",
                pkg.name,
                full.display()
            ));
        }
        let mut entries: Vec<String> = std::fs::read_dir(&full)
            .map_err(|e| format!("read dir {}: {e}", full.display()))?
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) == Some("html") {
                    p.file_name()?.to_str().map(|n| format!("{dir}/{n}"))
                } else {
                    None
                }
            })
            .collect();
        entries.sort();
        out.extend(entries);
        // components/ 子目录同收：组件文件按 stem 进 pkg 组件映射，运行时
        // `Instantiate("my-widget")` / `GetTemplate` 可克隆（fgui 组件一等公民同构；
        // 消费侧 api-reference「the registered stem, no components/ prefix」既有承诺
        // 的兑现）。文件内自定义标签照常经注册表展开（嵌套组件克隆路径）。页面与
        // 组件撞名 → 打包期 duplicate component name 错误（fail loud）。
        let comp_dir = format!("{dir}/components");
        let comp_full = workspace_root.join(&comp_dir);
        if comp_full.is_dir() {
            let mut comp_entries: Vec<String> = std::fs::read_dir(&comp_full)
                .map_err(|e| format!("read dir {}: {e}", comp_full.display()))?
                .filter_map(|e| e.ok())
                .filter_map(|e| {
                    let p = e.path();
                    if p.extension().and_then(|x| x.to_str()) == Some("html") {
                        p.file_name()?.to_str().map(|n| format!("{comp_dir}/{n}"))
                    } else {
                        None
                    }
                })
                .collect();
            comp_entries.sort();
            out.extend(comp_entries);
        }
    }
    Ok(out)
}

/// 取路径的文件名主干（去扩展名）——组件名来自 html 文件名。
/// `"ui/showcase/home.html"` → `"home"`。无扩展名或无法解析时原样返回。
fn stem(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_string()
}

/// 归一 background-image 路径为 sprite_key 并登记进 refs（atlas 交叉验证）。
/// inline（base_style 已提取的路径，walker emit 调）与 class 规则（url() 串里
/// parse_url 出的路径，normalize_bg_rules 调）共用。pub(crate)：expand.rs walker 也调。
pub(crate) fn normalize_bg_ref(html_rel: &str, path: &str, refs: &mut Vec<String>) -> String {
    let norm = normalize_sprite_key(html_rel, path);
    refs.push(norm.clone());
    norm
}

/// 动态规则表里 background-image / background url() 声明值归一为 sprite_key（class
/// 规则 bg-image 走 dynamic_rules，runtime rematch 用原始值重放，未归一
/// SpriteResolver miss → 白块）。linear-gradient 值无 url()，跳过（与 url 互斥）。
/// 页面 <style>（build.rs）与展开组件 scope 规则（expand.rs）共用。
pub(crate) fn normalize_bg_rules(
    table: &mut DynamicRuleTable,
    html_rel: &str,
    refs: &mut Vec<String>,
) {
    for r in table.rules.iter_mut() {
        for d in r.declarations.iter_mut() {
            if (d.prop == "background-image" || d.prop == "background")
                && !d.value.trim().starts_with("linear-gradient(")
            {
                if let Some(path) = yio_core::style::mapping::parse_url(&d.value) {
                    let norm = normalize_bg_ref(html_rel, &path, refs);
                    d.value = format!("url(\"{norm}\")");
                }
            }
        }
    }
}

/// 把 img src（相对 HTML 文件）归一化为 sprite_key（相对 workspace_root，正斜杠）。
/// `html_rel` = HTML 相对 workspace_root（如 `"showcase/home.html"`）；`src` = img src 原值。
/// 例：`("showcase/home.html", "../res/icons/x.png")` → `"res/icons/x.png"`。
///
/// 为什么手写归约而不是用 `PathBuf::canonicalize`：canonicalize 要求路径在磁盘上存在
/// 且返绝对路径；这里只做纯字符串词法归约（HTML src 可能指向尚未收集的图）。
/// `Component`-based 归约跨平台（Windows `\` 与 `/` 都正确迭代），输出统一正斜杠
/// 与 `atlas/collect.rs` 的 sprite_key 口径一致（`replace('\\', "/")`）。
pub(crate) fn normalize_sprite_key(html_rel: &str, src: &str) -> String {
    let base = Path::new(html_rel)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let joined = base.join(src);
    let mut stack: Vec<&str> = Vec::new();
    for comp in joined.components() {
        use std::path::Component;
        match comp {
            Component::Normal(s) => stack.push(s.to_str().unwrap_or("")),
            Component::CurDir => {}
            Component::ParentDir => {
                stack.pop();
            }
            Component::RootDir | Component::Prefix(_) => {} // 绝对路径不归一化（围栏外）
        }
    }
    stack.join("/")
}

/// analyze 的产出：全部计算结果，**零写入**。`build()` 据此写盘产出 BuildReport；
/// `check()` 据此直接出诊断报告——校验语义单代码路径，check/build 天然不漂移。
pub struct AnalyzeOutcome {
    pub workspace: crate::workspace::Workspace,
    /// (atlas 名, 打包结果)。pages 尚未编码写盘；manifest 供交叉验证消费。
    pub atlases: Vec<(String, crate::atlas::pack::PackedAtlas)>,
    /// (package 名, pkg.bin 字节 + sprite 引用 + warnings)。
    pub packages: Vec<(String, PackResult)>,
    /// registry 侧（components/ 组件文件）的一致性 warning。
    pub warnings: Vec<PackDiagnostic>,
}

/// 分析工作区：load → 图集收集打包（只算不写）→ 字体存在性 → 组件注册表 →
/// 逐包解析打包 → 覆盖交叉验证。内容错误（围栏/字体缺失/图集溢出/覆盖缺失冲突）
/// collect-all：收集全部后统一失败（exit 1，诊断全量）；工具性错误（io/workspace
/// 解析）即时失败（exit 2）。
pub fn analyze(workspace_root: &Path) -> Result<AnalyzeOutcome, BuildFailure> {
    let ws = load_workspace(workspace_root)?;
    let mut diags: Vec<PackDiagnostic> = Vec::new();

    // 分辨率适配配置（design/match_mode，透传 runtime.json 给集成层）：
    // 值域错误在这里拦——静默落 letterbox 会让作者以为适配生效。
    if let Some(d) = ws.design {
        if !d.w.is_finite() || !d.h.is_finite() || d.w <= 0.0 || d.h <= 0.0 {
            diags.push(PackDiagnostic::synthetic_error(
                code::CONFIG_INVALID,
                "design",
                "yio.workspace.json",
                format!("design 分辨率须为正有限值，got {}x{}", d.w, d.h),
            ));
        }
    }
    if let Some(m) = &ws.match_mode {
        if !matches!(m.as_str(), "letterbox" | "fit-width" | "fit-height") {
            diags.push(PackDiagnostic::synthetic_error(
                code::CONFIG_INVALID,
                "match_mode",
                "yio.workspace.json",
                format!("match_mode 须为 letterbox | fit-width | fit-height，got {m}"),
            ));
        }
    }

    // 溢出是内容错误（作者须调 max_size / standalone）：收集成诊断，继续后续图集。
    let mut atlases: Vec<(String, crate::atlas::pack::PackedAtlas)> = Vec::new();
    for atlas in &ws.atlases {
        let images = collect_pngs(workspace_root, atlas)?;
        match pack_atlas(atlas, &images) {
            Ok(packed) => atlases.push((atlas.name.clone(), packed)),
            Err(e) => diags.push(PackDiagnostic::synthetic_error(
                code::ATLAS_IMAGE_OVERFLOW,
                "",
                &atlas.name,
                e,
            )),
        }
    }

    for font in &ws.fonts {
        if !workspace_root.join(&font.file).exists() {
            diags.push(PackDiagnostic::synthetic_error(
                code::FONT_FILE_MISSING,
                &font.family,
                &font.file,
                format!(
                    "字体文件不存在：{}（fonts[].file 指向的文件不在工作区内）",
                    font.file
                ),
            ));
        }
    }

    // Custom Element 注册表：components/ 目录扫描，hyphen 标签打包期展开。
    // 单组件文件错误与 warning 在注册表内 collect-all；包内围栏诊断在
    // pack_components_with_registry 内 collect-all；跨包错误也进收集池（包不产出）。
    let (registry, comp_warnings) =
        crate::expand::scan_component_registry(workspace_root, &ws.packages)?;
    let mut packages: Vec<(String, PackResult)> = Vec::new();
    let mut all_refs: Vec<String> = Vec::new();
    for pkg in &ws.packages {
        let html_files = resolve_html_list(workspace_root, pkg)?;
        let comps: Vec<Component> = html_files
            .iter()
            .map(|rel| {
                let path = workspace_root.join(rel);
                let src = std::fs::read_to_string(&path)
                    .map_err(|e| format!("read {}: {e}", path.display()))?;
                Ok(Component {
                    name: stem(rel),
                    src,
                    html_rel: rel.clone(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        // 页面 <link rel="stylesheet"> 的外部 CSS 按 workspace 相对路径读取。
        let load_css = |css_rel: &str| std::fs::read_to_string(workspace_root.join(css_rel)).ok();
        match pack_components_with_css(&comps, &registry, &load_css) {
            Ok(pr) => {
                all_refs.extend(pr.referenced_sprites.iter().cloned());
                packages.push((pkg.name.clone(), pr));
            }
            Err(f) => {
                // 防线：只带 message、不带 Error 级诊断的失败若直接 extend 会整体蒸发
                // ——该包从产物里静默消失而 build/check 仍报成功。任何新错误源漏配
                // 诊断时这里兜底合成，失败永远可见。
                if !f.diagnostics.iter().any(|d| d.severity == Severity::Error) {
                    diags.push(PackDiagnostic::synthetic_error(
                        code::PACK_ERROR,
                        &pkg.name,
                        "",
                        f.message,
                    ));
                }
                diags.extend(f.diagnostics);
            }
        }
    }
    // 展开用到的组件的 sprite 引用并入交叉验证（未用组件是设计期存货，缺图不阻断）。
    all_refs.extend(registry.used_refs());

    // 单向：html 引用的图必须在某 atlas；atlas 未引用的图合法（运行时动态图标）。
    // collect 版：每个违规 key 一条诊断。
    let atlas_refs: Vec<(String, &crate::atlas::AtlasManifest)> = atlases
        .iter()
        .map(|(n, p)| (n.clone(), &p.manifest))
        .collect();
    diags.extend(crate::atlas::validate::assign_and_validate(
        &all_refs,
        &atlas_refs,
    ));

    // 预览兜底：`data-fill` 是「本列表运行时才填充」的标记（yio-preview skill
    // 约定）；缺按页模拟脚本 = 人类预览看到空列表。不靠 AI 记得写，靠 check 提醒。
    for pkg in &ws.packages {
        for rel in resolve_html_list(workspace_root, pkg).unwrap_or_default() {
            let Ok(src) = std::fs::read_to_string(workspace_root.join(&rel)) else {
                continue;
            };
            if !src.contains("data-fill") {
                continue;
            }
            let Some(dir) = Path::new(&rel).parent() else {
                continue;
            };
            let stem = Path::new(&rel)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            let has_sim = workspace_root
                .join(dir)
                .join(format!("preview/pages/{stem}.js"))
                .is_file();
            if !has_sim {
                diags.push(PackDiagnostic::synthetic_warning(
                    code::PREVIEW_DATA_FILL_WITHOUT_SIM,
                    &pkg.name,
                    &rel,
                    format!(
                        "页面带 data-fill（运行时填充）但缺 {}/preview/pages/{stem}.js \
                         ——人类预览（yio preview）将看到空列表；按 yio-preview skill 补演示数据脚本",
                        dir.display()
                    ),
                ));
            }
        }
    }

    if diags.iter().any(|d| d.severity == Severity::Error) {
        let errors = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count();
        return Err(BuildFailure::validation(
            format!("{errors} error(s) in workspace"),
            diags,
        ));
    }
    // 顶层 diags（此处只剩 Warning 级——Error 已提前整体失败）并进 outcome，
    // 否则成功路径静默丢弃（修前无症状只因顶层诊断恰好全是 Error 级）。
    let mut warnings = comp_warnings;
    warnings.extend(diags);
    Ok(AnalyzeOutcome {
        workspace: ws,
        atlases,
        packages,
        warnings,
    })
}

/// Run the full build pipeline for a workspace rooted at workspace_root.
///
/// 失败值 [`BuildFailure`]：exit 1 = 内容错误（围栏/资源/结构，diagnostics collect-all）；
/// exit 2 = 工具性失败（配置/io）。io 与 workspace 读取类错误经 `From<String>` 统一归 2。
///
/// Steps:
/// 1. output_dir 快速失败检查 → analyze（零写入，见 [`analyze`]）
/// 2. 写入段：mkdirs / clean stale / atlas pages+manifests / fonts / ui/*.pkg.bin / runtime
pub fn build(workspace_root: &Path) -> Result<BuildReport, BuildFailure> {
    // output_dir 检查先于 analyze 的重计算（check 无此检查——它零写入，不关心落点）。
    let ws_pre = load_workspace(workspace_root)?;
    if ws_pre.output_dir.trim().is_empty() {
        return Err(BuildFailure::config(
            "output_dir not configured: set it in the workspace General page before building",
        ));
    }
    let outcome = analyze(workspace_root)?;
    let ws = outcome.workspace;
    // 输出基座链：就近找到的 .yio/config.json 带 unity_root → output_dir 相对 Unity
    // 工程根解析（直达 Assets）；无 → 相对 ui 工作区（本地输出）。路径失效在 resolve
    // 内报 exit 2。
    let output_dir = crate::config::resolve_output_base(workspace_root)?
        .unwrap_or_else(|| workspace_root.to_path_buf())
        .join(&ws.output_dir);

    let ui_dir = output_dir.join("ui");
    let atlas_dir = output_dir.join("atlas");
    let fonts_dir = output_dir.join("fonts");
    std::fs::create_dir_all(&ui_dir)
        .map_err(|e| format!("create ui dir {}: {e}", ui_dir.display()))?;
    std::fs::create_dir_all(&atlas_dir)
        .map_err(|e| format!("create atlas dir {}: {e}", atlas_dir.display()))?;
    std::fs::create_dir_all(&fonts_dir)
        .map_err(|e| format!("create fonts dir {}: {e}", fonts_dir.display()))?;

    // 清理上次构建的残留产物（删包重打场景）：删 ui/atlas/fonts 下的生成文件
    //（.pkg.bin / .atlas.json / .png / .bytes），保留 Unity 的 .meta（删了会重生成 GUID、断引用）。
    // 不清理 yio.runtime.json（本函数末尾覆盖写）。
    clean_stale_outputs(&ui_dir, &["pkg.bin"])?;
    clean_stale_outputs(&atlas_dir, &["atlas.json", "png"])?;
    clean_stale_outputs(&fonts_dir, &["bytes"])?;

    let mut report = BuildReport {
        packages: Vec::new(),
        atlases: Vec::new(),
        fonts: Vec::new(),
        log: Vec::new(),
        warnings: outcome.warnings,
    };

    for (name, packed) in &outcome.atlases {
        report.log.push(format!("writing atlas {name}"));
        for (i, page_img) in packed.pages.iter().enumerate() {
            let page_name = crate::atlas::pack::page_file_name(name, i);
            let page_path = atlas_dir.join(&page_name);
            page_img
                .save(&page_path)
                .map_err(|e| format!("save atlas page {}: {e}", page_path.display()))?;
        }
        let manifest_path = atlas_dir.join(format!("{name}.atlas.json"));
        let mut manifest_text = serde_json::to_string_pretty(&packed.manifest)
            .map_err(|e| format!("serialize atlas manifest {name}: {e}"))?;
        manifest_text.push('\n'); // 尾换行：防重写伪 diff（同 save_workspace）
        std::fs::write(&manifest_path, manifest_text)
            .map_err(|e| format!("write {}: {e}", manifest_path.display()))?;
        report.atlases.push(name.clone());
        report
            .log
            .push(format!("  wrote {} page(s) + manifest", packed.pages.len()));
    }

    for font in &ws.fonts {
        let src = workspace_root.join(&font.file);
        let basename = Path::new(&font.file)
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| format!("invalid font file path: {}", font.file))?;
        let dst = fonts_dir.join(format!("{}.bytes", basename));
        std::fs::copy(&src, &dst)
            .map_err(|e| format!("copy font {} -> {}: {e}", src.display(), dst.display()))?;
        report.fonts.push(basename.to_string());
        report.log.push(format!("copied font {}", dst.display()));
    }

    for (name, pr) in &outcome.packages {
        let pkg_path = ui_dir.join(format!("{name}.pkg.bin"));
        std::fs::write(&pkg_path, &pr.bytes)
            .map_err(|e| format!("write {}: {e}", pkg_path.display()))?;
        report.packages.push(name.clone());
        // warning 聚合进报告（跨 package），供 CLI/GUI 统一呈现。不阻断打包。
        report.warnings.extend(pr.warnings.iter().cloned());
        report.log.push(format!(
            "wrote {} ({} bytes)",
            pkg_path.display(),
            pr.bytes.len()
        ));
    }

    // runtime.packages = report.packages（依赖上一段先填完——排序契约）。
    let runtime = RuntimeManifest {
        version: 1,
        packages: report.packages.clone(),
        atlases: report.atlases.clone(),
        design: ws.design,
        match_mode: ws.match_mode.clone(),
        fonts: ws
            .fonts
            .iter()
            .map(|f| {
                let basename = Path::new(&f.file)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .ok_or_else(|| {
                        format!("invalid font file path in runtime manifest: {}", f.file)
                    })?;
                Ok(RuntimeFont {
                    family: f.family.clone(),
                    file: format!("{}.bytes", basename),
                    default: f.default,
                    fallback: f.fallback,
                })
            })
            .collect::<Result<Vec<_>, String>>()?,
    };
    let runtime_path = output_dir.join(RUNTIME_FILE);
    let mut runtime_text = serde_json::to_string_pretty(&runtime)
        .map_err(|e| format!("serialize runtime manifest: {e}"))?;
    runtime_text.push('\n'); // 尾换行：防重写伪 diff（同 save_workspace）
    std::fs::write(&runtime_path, runtime_text)
        .map_err(|e| format!("write {}: {e}", runtime_path.display()))?;
    report.log.push(format!("wrote {}", runtime_path.display()));

    Ok(report)
}

/// 清理输出目录里上次构建的残留产物：删扩展名匹配的文件，跳过 .meta（Unity GUID，
/// 删了重生成会断引用）和非匹配文件。删包重打场景必需——否则 workspace 里删掉的
/// package/atlas/font 的产物会一直残留在 output_dir，运行时读到旧产物。
fn clean_stale_outputs(dir: &Path, exts: &[&str]) -> Result<(), String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("read dir {}: {e}", dir.display())),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none() {
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        // .meta 是 Unity 资产序列化文件（同名的 .pkg.bin.meta / .png.meta），绝不能删。
        if ext == "meta" {
            continue;
        }
        // 多段扩展名（pkg.bin / atlas.json）用文件名后缀匹配，单段（png / bytes）用 ext 匹配。
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let stale = exts.iter().any(|e| name.ends_with(&format!(".{e}")));
        if stale {
            std::fs::remove_file(&path)
                .map_err(|e| format!("remove stale {}: {e}", path.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod package_tests {
    use super::*;
    use yio_core::scene::NodeKind;

    /// 写一个最小多页 + 组件工作区到临时目录，供 analyze 级 e2e 测试用。
    fn write_temp_ws(tag: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
        let tmp = std::env::temp_dir().join(format!("yio_ws_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        for (rel, content) in files {
            let path = tmp.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, content).unwrap();
        }
        tmp
    }

    const TIP_PANEL_LINES: &str = r#"<div class="tip">
  <slot name="line0"></slot>
  <slot name="line1"></slot>
</div>"#;

    const WS_JSON: &str =
        r#"{"version":1,"output_dir":"../out","packages":[{"name":"game","dirs":["ui"]}]}"#;

    /// showcase ::part 摆台契约锁：读真 showcase.pkg.bin（组件/页面标记的集成面），
    /// instantiate component-lab 后 .lw-hot 实例 title 金、默认实例保持组件默认色。
    /// 防摆台回归（行内 style 压过 ::part 一类：part 目标节点禁行内 color 声明）。
    #[test]
    fn showcase_bundle_part_demo_contract() {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../unity/showcase-unity/Assets/Bundles/ui/showcase.pkg.bin"
        ))
        .unwrap();
        let mut stage = yio_core::stage::Stage::new((1080.0, 1920.0)).unwrap();
        stage.create_root("div", "").unwrap();
        stage.load_package("showcase", &bytes).unwrap();
        let _ = stage.instantiate("showcase", "component-lab").unwrap();
        yio_core::style::dynamic::rematch_pseudo_classes(
            stage.scene.as_mut().unwrap(),
            (1080.0, 1920.0),
            [0.0; 4],
        );
        let scene = stage.scene.as_ref().unwrap();
        fn dfs(
            scene: &yio_core::scene::Scene,
            id: yio_core::scene::NodeId,
        ) -> Option<yio_core::scene::NodeId> {
            if scene.roles.get(id).and_then(|i| i.attr("part")) == Some("title") {
                return Some(id);
            }
            for &c in &scene.get(id).unwrap().children {
                if let Some(f) = dfs(scene, c) {
                    return Some(f);
                }
            }
            None
        }
        let hot_host = scene
            .nodes
            .values()
            .find(|n| {
                n.kind == yio_core::scene::NodeKind::CustomElement
                    && n.classes.iter().any(|c| c == "lw-hot")
            })
            .map(|n| n.id)
            .unwrap();
        let hot = dfs(scene, hot_host).unwrap();
        let plain_host = scene
            .nodes
            .values()
            .find(|n| n.kind == yio_core::scene::NodeKind::CustomElement && n.classes.is_empty())
            .map(|n| n.id)
            .unwrap();
        let plain = dfs(scene, plain_host).unwrap();
        let gold: [f32; 4] = [0.9529412, 0.6117647, 0.07058824, 1.0];
        let dflt: [f32; 4] = [0.8784314, 0.9019608, 0.9254902, 1.0];
        assert_eq!(
            scene.get(hot).unwrap().style.color,
            gold,
            "lw-hot 的 title 应金"
        );
        assert_eq!(
            scene.get(plain).unwrap().style.color,
            dflt,
            "默认实例保持组件默认色"
        );
    }

    #[test]
    fn part_rule_matches_through_real_pack_instantiate_pipeline() {
        let tmp = std::env::temp_dir().join(format!("yio-part-e2e-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("ui/components")).unwrap();
        std::fs::write(
            tmp.join("ui/main.html"),
            "<style>.hot::part(title) { color:#f39c12; }</style>
<div><my-part-card class=\"hot\"></my-part-card></div>",
        )
        .unwrap();
        std::fs::write(
            tmp.join("ui/components/my-part-card.html"),
            "<div class=\"c-root\"><span class=\"c-title\" part=\"title\">T</span></div>",
        )
        .unwrap();
        let ws = crate::workspace::Workspace {
            version: 1,
            output_dir: String::new(),
            design: None,
            match_mode: None,
            packages: vec![crate::workspace::PackageCfg {
                name: "ui".into(),
                dirs: vec!["ui".into()],
                html: vec![],
            }],
            atlases: vec![],
            fonts: vec![],
        };
        let (registry, _) = crate::expand::scan_component_registry(&tmp, &ws.packages).unwrap();
        let htmls = resolve_html_list(&tmp, &ws.packages[0]).unwrap();
        let comps: Vec<Component> = htmls
            .iter()
            .map(|rel| {
                let src_html = std::fs::read_to_string(tmp.join(rel)).unwrap();
                Component {
                    name: stem(rel),
                    src: src_html,
                    html_rel: rel.clone(),
                }
            })
            .collect();
        let pr = pack_components_with_css(&comps, &registry, &|_| None).unwrap();

        // core 真管线：load → instantiate → rematch。
        let mut stage = yio_core::stage::Stage::new((200.0, 200.0)).unwrap();
        stage.create_root("div", "").unwrap();
        stage.load_package("ui", &pr.bytes).unwrap();
        let _root = stage.instantiate("ui", "main").unwrap();
        yio_core::style::dynamic::rematch_pseudo_classes(
            stage.scene.as_mut().unwrap(),
            (1080.0, 1920.0),
            [0.0; 4],
        );
        let scene = stage.scene.as_ref().unwrap();
        // 目标：组件内部带 part="title" 的 span；host 带 class hot。
        let mut host_found = false;
        let mut target: Option<yio_core::scene::NodeId> = None;
        for n in scene.nodes.values() {
            if n.kind == yio_core::scene::NodeKind::CustomElement
                && n.classes.iter().any(|c| c == "hot")
            {
                host_found = true;
            }
            if scene
                .roles
                .get(n.id)
                .and_then(|i| i.attr("part"))
                .is_some_and(|v| v == "title")
            {
                target = Some(n.id);
            }
        }
        assert!(host_found, "host（class=hot 的 CustomElement）在树中");
        let target = target.expect("part=title 目标节点在树中（attrs 进 RoleInfo）");
        let got = scene.get(target).unwrap().style.color;
        assert_eq!(
            got,
            [0.9529412, 0.6117647, 0.07058824, 1.0],
            "::part(title) 规则命中（#f39c12）——真管线穿墙"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn components_dir_entries_instantiable_by_stem() {
        let tmp = std::env::temp_dir().join(format!("yio-comp-entry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("ui/components")).unwrap();
        std::fs::write(
            tmp.join("ui/main.html"),
            "<div><span id=\"a\">page</span></div>",
        )
        .unwrap();
        std::fs::write(
            tmp.join("ui/components/my-card.html"),
            "<div class=\"card\"><slot name=\"icon\"><img src=\"../a.png\" style=\"width:8px;height:8px\"></slot></div>",
        )
        .unwrap();
        std::fs::write(
            tmp.join("ui/components/card-holder.html"),
            "<div><my-card></my-card></div>",
        )
        .unwrap();
        let ws = crate::workspace::Workspace {
            version: 1,
            output_dir: String::new(),
            design: None,
            match_mode: None,
            packages: vec![crate::workspace::PackageCfg {
                name: "ui".into(),
                dirs: vec!["ui".into()],
                html: vec![],
            }],
            atlases: vec![],
            fonts: vec![],
        };
        let (registry, _) = crate::expand::scan_component_registry(&tmp, &ws.packages).unwrap();
        let htmls = resolve_html_list(&tmp, &ws.packages[0]).unwrap();
        let stems: Vec<String> = htmls.iter().map(|r| stem(r)).collect();
        let stems: Vec<&str> = stems.iter().map(|s| s.as_str()).collect();
        // 直收：页面 + 两个组件文件都进条目
        assert!(stems.contains(&"main"), "stems = {stems:?}");
        assert!(
            stems.contains(&"my-card"),
            "components/ 直收，stems = {stems:?}"
        );
        assert!(stems.contains(&"card-holder"), "stems = {stems:?}");

        // 打包 → 读回 pkg 组件映射：三 stem 都可实例化；holder 含展开的 host 子树。
        let comps: Vec<Component> = htmls
            .iter()
            .map(|rel| {
                let src_html = std::fs::read_to_string(tmp.join(rel)).unwrap();
                Component {
                    name: stem(rel),
                    src: src_html,
                    html_rel: rel.clone(),
                }
            })
            .collect();
        let pr = pack_components_with_css(&comps, &registry, &|_| None).unwrap();
        let pkg = yio_core::asset::read_package(&pr.bytes).unwrap();
        assert!(pkg.components.contains_key("main"));
        assert!(
            pkg.components.contains_key("my-card"),
            "slot fallback 走组件语义"
        );
        assert!(pkg.components.contains_key("card-holder"));
        let holder = &pkg.components["card-holder"];
        assert!(
            holder
                .nodes
                .iter()
                .any(|n| n.kind == yio_core::scene::NodeKind::CustomElement),
            "holder 条目内 <my-card> 经注册表展开为 host 节点"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn analyze_fails_loudly_on_dangling_slot_projection() {
        let tmp = write_temp_ws(
            "dangling_slot",
            &[
                ("yio.workspace.json", WS_JSON),
                ("ui/components/tip-panel.html", TIP_PANEL_LINES),
                (
                    "ui/battle.html",
                    r#"<div style="display:flex"><tip-panel><span slot="line0">攻 13</span></tip-panel></div>"#,
                ),
                (
                    "ui/map.html",
                    // desc 槽已被重构删掉，map 页仍投影 → 悬空 slot 名。
                    r#"<div style="display:flex"><tip-panel><span slot="desc">旧投影</span></tip-panel></div>"#,
                ),
            ],
        );
        let err = match analyze(&tmp) {
            Err(e) => e,
            Ok(_) => panic!("悬空 slot 必须失败（修前包静默消失、build 报 OK）"),
        };
        assert_eq!(err.exit_code, 1, "内容错误 exit 1：{err}");
        let has = err
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error && d.message.contains("desc"));
        assert!(
            has,
            "错误须以 Error 级诊断暴露（修前 message 被吞、包静默消失）：{err:?}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 回归：投影 light 子 id 与组件模板 id 撞车 → analyze 失败可见。
    #[test]
    fn analyze_fails_loudly_on_projection_id_collision() {
        let tmp = write_temp_ws(
            "id_collision",
            &[
                ("yio.workspace.json", WS_JSON),
                (
                    "ui/components/tip-panel.html",
                    r#"<div class="tip"><slot name="line0"></slot><div id="tip-row-0"></div></div>"#,
                ),
                (
                    "ui/battle.html",
                    r#"<div style="display:flex"><tip-panel><span slot="line0" id="tip-row-0">撞</span></tip-panel></div>"#,
                ),
            ],
        );
        let err = match analyze(&tmp) {
            Err(e) => e,
            Ok(_) => panic!("展开域 id 撞车必须失败"),
        };
        assert_eq!(err.exit_code, 1);
        let has = err
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error && d.message.contains("tip-row-0"));
        assert!(has, "撞车错误须以 Error 级诊断暴露：{err:?}");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// pack 层回归：bridge 错误（无效 slot）落进 BuildFailure.diagnostics 而非只在 message。
    #[test]
    fn pack_bridge_error_carries_error_diagnostic() {
        use crate::expand::ComponentRegistry;
        let (reg, _) = ComponentRegistry::from_sources(&[(
            "tip-panel".to_string(),
            TIP_PANEL_LINES.to_string(),
            "ui/components/tip-panel.html".to_string(),
        )])
        .unwrap();
        let err = pack_components_with_registry(
            &[Component {
                name: "page".to_string(),
                src: r#"<div style="display:flex"><tip-panel><span slot="nope">x</span></tip-panel></div>"#
                    .to_string(),
                html_rel: "page.html".to_string(),
            }],
            &reg,
        )
        .expect_err("无效 slot 应失败");
        let diag = err
            .diagnostics
            .iter()
            .find(|d| d.severity == Severity::Error && d.code == "PackError")
            .expect("bridge 错误须是 Error 级 PackError 诊断");
        assert!(
            diag.message.contains("nope"),
            "诊断 message 须含 slot 名：{}",
            diag.message
        );
        assert_eq!(diag.file, "page.html", "诊断指向出错页面文件");
    }

    #[test]
    fn clean_stale_outputs_removes_products_keeps_meta() {
        // 模拟删包重打：ui 目录里有旧产物 + .meta + 非产物文件。clean 应删产物、留 .meta。
        let tmp = std::env::temp_dir().join(format!("yio_clean_test_{}", std::process::id()));
        let ui = tmp.join("ui");
        std::fs::create_dir_all(&ui).unwrap();
        std::fs::write(ui.join("showcase.pkg.bin"), b"old").unwrap();
        std::fs::write(ui.join("showcase.pkg.bin.meta"), b"guid").unwrap(); // 必须留
        std::fs::write(ui.join("deleted.pkg.bin"), b"stale").unwrap(); // 删包重打应清掉
        std::fs::write(ui.join("readme.txt"), b"keep").unwrap(); // 非产物留着

        clean_stale_outputs(&ui, &["pkg.bin"]).unwrap();

        assert!(!ui.join("showcase.pkg.bin").exists(), "产物删除");
        assert!(
            ui.join("showcase.pkg.bin.meta").exists(),
            ".meta 必须保留（删了断 Unity GUID）"
        );
        assert!(!ui.join("deleted.pkg.bin").exists(), "残留产物清掉");
        assert!(ui.join("readme.txt").exists(), "非产物文件不动");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn pack_components_roundtrip_single() {
        // html_rel 放 workspace_root 顶层 → src 原样进 refs（base 为空）。
        // 根 div 设 display:flex：子是 flex item（不走 rich-text inline/block 分类），
        // 避免 FenceMixedInlineBlock 拒载（div 块子 + img 语义 inline 子 = mixed）。
        let comps = vec![Component {
            name: "home".to_string(),
            src:
                r#"<div class="root" style="display:flex"><div>hi</div><img src="icons/a.png" style="display:block"></div>"#
                    .to_string(),
            html_rel: "home.html".to_string(),
        }];
        let PackResult {
            bytes,
            referenced_sprites: refs,
            ..
        } = pack_components(&comps).unwrap();
        let pkg = yio_core::asset::read_package(&bytes).unwrap();
        let comp = pkg.components.get("home").expect("home component");
        assert_eq!(comp.nodes[0].kind, NodeKind::Container); // div
        assert!(
            refs.iter().any(|r| r == "icons/a.png"),
            "referenced_sprites missing: {refs:?}"
        );
    }

    #[test]
    fn pack_components_normalizes_image_src_to_sprite_key() {
        // HTML 嵌套子目录（spec4b/spec4b.html），img src ../res/icons/x.png → pkg Image.src
        // 必须归一成 res/icons/x.png（atlas key 口径）。否则 runtime SpriteResolver 拿原 src
        // ../res/.. 查 atlas miss。回归 bug：bridge 存原 src、refs 归一但 pkg src 字段漏。
        let comps = vec![Component {
            name: "spec4b".to_string(),
            src: r#"<div class="root"><img src="../res/icons/x.png" style="display:block"></div>"#
                .to_string(),
            html_rel: "spec4b/spec4b.html".to_string(),
        }];
        let PackResult {
            bytes,
            referenced_sprites: refs,
            ..
        } = pack_components(&comps).unwrap();
        let pkg = yio_core::asset::read_package(&bytes).unwrap();
        let comp = pkg.components.get("spec4b").expect("spec4b component");
        let img = comp
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Image)
            .expect("Image node");
        assert_eq!(
            img.src.as_deref(),
            Some("res/icons/x.png"),
            "pkg Image.src must be normalized to atlas key (got {:?})",
            img.src
        );
        assert!(
            refs.iter().any(|r| r == "res/icons/x.png"),
            "refs should also be normalized: {refs:?}"
        );
    }

    #[test]
    fn pack_components_multi_component() {
        let comps = vec![
            Component {
                name: "nav".to_string(),
                src: r#"<div><button style="display:block">l</button></div>"#.to_string(),
                html_rel: "nav.html".to_string(),
            },
            Component {
                name: "page".to_string(),
                src: r#"<div class="page">body</div>"#.to_string(),
                html_rel: "page.html".to_string(),
            },
        ];
        let PackResult { bytes, .. } = pack_components(&comps).unwrap();
        let pkg = yio_core::asset::read_package(&bytes).unwrap();
        assert!(pkg.components.contains_key("nav"));
        assert!(pkg.components.contains_key("page"));
    }

    #[test]
    fn pack_components_propagates_bridge_error() {
        // 多根 → bridge 报错（不静默产森林）；错误带组件名定位来源。
        let comps = vec![Component {
            name: "bad".to_string(),
            src: r#"<div>a</div><div>b</div>"#.to_string(),
            html_rel: "bad.html".to_string(),
        }];
        let err = pack_components(&comps).expect_err("multi-root should error");
        assert!(
            err.message.contains("component bad"),
            "bridge error should name the component: {err}"
        );
    }

    #[test]
    fn pack_components_rejects_duplicate_names() {
        // 同名组件：write_package 不查（返 Vec<u8> 无 Result），read_package 运行时才
        // DupComponent 拒绝——产物是静默坏包。pack_components 构建期须 fail fast。
        let comps = vec![
            Component {
                name: "dup".to_string(),
                src: r#"<div>a</div>"#.to_string(),
                html_rel: "dup1.html".to_string(),
            },
            Component {
                name: "dup".to_string(),
                src: r#"<div>b</div>"#.to_string(),
                html_rel: "dup2.html".to_string(),
            },
        ];
        let err = pack_components(&comps).expect_err("dup names should error");
        assert_eq!(err.exit_code, 1, "重名是内容错误（exit 1）");
        let diag = err
            .diagnostics
            .iter()
            .find(|d| d.code == "DuplicateComponentName")
            .expect("重名须以合成诊断码暴露");
        assert_eq!(diag.severity, crate::diag::Severity::Error);
        assert_eq!(diag.file, "dup2.html", "诊断指向第二次出现的文件");
        assert!(
            diag.message.contains("duplicate component name") && diag.message.contains("dup"),
            "诊断 message 应描述性: {}",
            diag.message
        );
    }

    /// collect-all 回归：两个组件各含围栏 Error → 两条 error 诊断都在（修前首错即断，
    /// 只报第一个组件）。AI 修一轮全改是围栏诊断的契约。
    #[test]
    fn pack_components_collects_errors_across_components() {
        let comps = vec![
            Component {
                name: "bad-a".to_string(),
                // 围栏外标签（14 标签集之外）→ FenceUnknownTag error。
                src: r#"<p>not in fence</p>"#.to_string(),
                html_rel: "bad-a.html".to_string(),
            },
            Component {
                name: "bad-b".to_string(),
                // 未识别 role → error（role 白名单校验）。
                src: r#"<div role="nope"></div>"#.to_string(),
                html_rel: "bad-b.html".to_string(),
            },
        ];
        let err = pack_components(&comps).expect_err("two error components should fail");
        assert_eq!(err.exit_code, 1);
        let errors: Vec<&crate::diag::PackDiagnostic> = err
            .diagnostics
            .iter()
            .filter(|d| d.severity == crate::diag::Severity::Error)
            .collect();
        assert_eq!(errors.len(), 2, "两个组件的 error 都要报: {err:?}");
        assert!(
            errors.iter().any(|d| d.file == "bad-a.html"),
            "bad-a 的诊断在场"
        );
        assert!(
            errors.iter().any(|d| d.file == "bad-b.html"),
            "bad-b 的诊断在场（修前首错即断会漏掉它）"
        );
    }

    /// collect-all 回归：失败时 warning 也随诊断带出（AI 修 error 一轮顺带处理 warning）。
    #[test]
    fn build_failure_carries_warnings_too() {
        let comps = vec![
            Component {
                name: "bad".to_string(),
                src: r#"<p>not in fence</p>"#.to_string(),
                html_rel: "bad.html".to_string(),
            },
            Component {
                name: "warn".to_string(),
                // W1：border-width 有、border-style 缺省。
                src: r#"<div style="border-width:2px;border-color:#ff0000"></div>"#.to_string(),
                html_rel: "warn.html".to_string(),
            },
        ];
        let err = pack_components(&comps).expect_err("error component should fail");
        assert_eq!(err.exit_code, 1);
        assert!(
            err.diagnostics
                .iter()
                .any(|d| d.severity == crate::diag::Severity::Warning
                    && d.code == "FenceBorderWithoutStyle"),
            "失败诊断须携带 warning（修 error 一轮顺带可见）: {:?}",
            err.diagnostics
        );
    }

    #[test]
    fn pack_components_warning_does_not_block_packaging() {
        // 回归锁：围栏内一致性 warning（border-width 无 style）合法但不阻断打包。
        // build.rs 曾把任何 diagnostic 当 fatal → warning 命中时 pkg 打不出来，违反设计意图。
        // 构造只产该 warning（无 Error）的组件，断言 pack_components 返 Ok。
        let comps = vec![Component {
            name: "warn".to_string(),
            src: r#"<div style="border-width:2px;border-color:#ff0000"></div>"#.to_string(),
            html_rel: "warn.html".to_string(),
        }];
        // 双重断言：先证明确实产了 W1 warning（否则测试无效——HTML 没命中 warning），
        // 再证明 pack_components 仍返 Ok（warning 被放行）。
        let parsed = yio_fence::parse_template(&comps[0].src, "warn.html");
        assert!(
            parsed.diagnostics.iter().any(|d| {
                d.code == yio_fence::diagnostic::DiagnosticCode::FenceBorderWithoutStyle
                    && d.severity == yio_fence::diagnostic::Severity::Warning
            }),
            "测试前置：HTML 应触发 W1 warning，否则此测试无效: {:?}",
            parsed.diagnostics
        );
        // 具名结构体：修后 warning 经 PackResult.warnings 暴露（修前是二元组，warning 被丢弃）。
        let PackResult { bytes, .. } = pack_components(&comps)
            .expect("warning 不应阻断打包：pack_components 应返 Ok，但实际被当 fatal");
        // 确认产物可读（不是静默坏包）。
        let pkg = yio_core::asset::read_package(&bytes).unwrap();
        assert!(
            pkg.components.contains_key("warn"),
            "warning 组件应正常写入 pkg"
        );
    }

    #[test]
    fn pack_components_exposes_warnings_in_return_value() {
        // 回归锁：一致性 warning 不阻断打包，但必须对 CLI/GUI 可见。
        // 修前 pack_components 只查 Error 级 diagnostic，warning 留在局部 parsed.diagnostics
        // 里随循环结束丢弃 → 作者感知不到「预览 ≠ 运行时」的不一致。修后 warning 经
        // PackResult.warnings 暴露 → build() 进 BuildReport.warnings → CLI 打印。
        let comps = vec![
            Component {
                name: "warn".to_string(),
                // W1：border-width 有、border-style 缺省。
                src: r#"<div style="border-width:2px;border-color:#ff0000"></div>"#.to_string(),
                html_rel: "warn.html".to_string(),
            },
            Component {
                name: "bg".to_string(),
                // W2：background-image 有、background-size 缺省。
                src: r#"<div style="background-image:url(a.png)"></div>"#.to_string(),
                html_rel: "bg.html".to_string(),
            },
        ];
        // 修后 warning 经返回值 PackResult.warnings 暴露（修前被丢弃）。
        let PackResult { warnings, .. } = pack_components(&comps).expect("warning 不阻断打包");
        // W1 来自 warn 组件，须带组件名 + 文件位置 + 短码。
        let w1 = warnings
            .iter()
            .find(|w| w.code == "FenceBorderWithoutStyle")
            .expect("W1 warning 应暴露在返回值里（修前被丢弃）");
        assert_eq!(w1.component, "warn");
        assert_eq!(w1.file, "warn.html");
        assert!(w1.line >= 1, "warning 须带行号定位");
        assert!(w1.column >= 1, "warning 须带列号定位");
        assert!(
            !w1.message.is_empty(),
            "message 非空（含问题说明 + 修复引导）"
        );
        // W2 来自 bg 组件，证明跨组件收集（非只留首个组件的）。
        let w2 = warnings
            .iter()
            .find(|w| w.code == "FenceBgImageWithoutSize")
            .expect("W2 warning 应暴露在返回值里");
        assert_eq!(w2.component, "bg");
        assert_eq!(w2.file, "bg.html");
    }

    #[test]
    fn normalize_sprite_key_resolves_dotdot_against_html_dir() {
        // HTML 在 showcase/home.html（workspace_root 相对），img src ../res/icons/x.png
        // → sprite_key res/icons/x.png（atlas sprite_key 是 workspace_root 相对路径）。
        // 这是 showcase 的核心用例：HTML 嵌套在子目录，src 用 ../ 逃到 workspace_root。
        assert_eq!(
            normalize_sprite_key("showcase/home.html", "../res/icons/x.png"),
            "res/icons/x.png"
        );
        // 无 ../ 的 src：相对 HTML 所在目录解析（浏览器语义）→ showcase/res/icons/y.png。
        // 不是直接相对 workspace_root；与相对 URL 标准一致。
        assert_eq!(
            normalize_sprite_key("showcase/home.html", "res/icons/y.png"),
            "showcase/res/icons/y.png"
        );
        // HTML 位于 workspace_root 顶层：parent 为空，src 原样（去掉 leading "./"）。
        assert_eq!(normalize_sprite_key("home.html", "res/z.png"), "res/z.png");
    }
}
