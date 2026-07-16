//! Build orchestration: atlases + fonts + packages (HTML -> .pkg.bin) + runtime manifest -> output_dir.
//! Single entry point build() called by CLI and GUI.
//!
//! R3: HTML -> .pkg.bin 编排已重建（fence parse_template + bridge + write_package）。
//! referenced_sprites 回接 atlas 交叉验证（assign_and_validate，缺失 sprite 非静默）。

use serde::Serialize;

use crate::atlas::collect::collect_pngs;
use crate::atlas::pack::pack_atlas;
use crate::bridge::bridge;
use crate::runtime::{RuntimeFont, RuntimeManifest, RUNTIME_FILE};
use crate::workspace::{load_workspace, PackageCfg};
use loomgui_core::asset::{write_package, ControllerEntry, PackageInput, TemplateNode};
use loomgui_core::style::dynamic::DynamicRuleTable;
use std::path::Path;

/// Build report: what was produced.
#[derive(Debug, Clone, Serialize)]
pub struct BuildReport {
    /// Package names (one per PackageCfg, written to ui/<name>.pkg.bin).
    pub packages: Vec<String>,
    pub atlases: Vec<String>,
    pub fonts: Vec<String>,
    pub log: Vec<String>,
}

/// 打包一个 package：components = [(组件名, html 源码)]。返 (pkg.bin bytes, referenced_sprites)。
/// build() 读文件组 (name, src) 调本函数；本函数接字符串便于单测。
///
/// 流程：每组件 `parse_template` → `bridge` → 累积；末尾 `write_package` 出 pkg.bin。
/// fence diagnostics 非空 → Err（不静默降级）；bridge 多根 → Err（不静默产森林）。
/// referenced_sprites = 所有组件 img src / background-image 并集（供 atlas 交叉验证）。
pub fn pack_components(components: &[(String, String)]) -> Result<(Vec<u8>, Vec<String>), String> {
    let mut built: Vec<(
        String,
        Vec<TemplateNode>,
        DynamicRuleTable,
        Vec<ControllerEntry>,
    )> = Vec::new();
    let mut refs: Vec<String> = Vec::new();
    for (name, src) in components {
        let parsed = loomgui_fence::parse_template(src, name);
        if !parsed.diagnostics.is_empty() {
            return Err(format!(
                "fence diagnostics in {name}: {:?}",
                parsed.diagnostics
            ));
        }
        // bridge 错误带组件名：多组件包里，否则 "多根" 之类错误无法定位是哪个组件。
        let (nodes, controllers) =
            bridge(&parsed).map_err(|e| format!("bridge error in component {name}: {e}"))?;
        built.push((
            name.clone(),
            nodes,
            DynamicRuleTable {
                rules: parsed.dynamic_rules,
            },
            controllers,
        ));
        refs.extend(parsed.referenced_sprites);
    }
    // 同名组件：write_package 不查（返回 Vec<u8> 无 Result），read_package 运行时才
    // DupComponent 拒绝——产物是静默坏包。构建期 fail fast，给最早反馈。
    let mut seen = std::collections::HashSet::new();
    for (name, _, _, _) in &built {
        if !seen.insert(name.as_str()) {
            return Err(format!("duplicate component name `{name}` in package"));
        }
    }
    let comp_refs: Vec<(&str, &[TemplateNode], &DynamicRuleTable, &[ControllerEntry])> = built
        .iter()
        .map(|(n, nodes, dr, c)| (n.as_str(), nodes.as_slice(), dr, c.as_slice()))
        .collect();
    let bytes = write_package(&PackageInput {
        components: comp_refs,
    });
    Ok((bytes, refs))
}

/// 把 PackageCfg 解析成 HTML 文件相对路径列表（相对工作区根，正斜杠）。
/// `html` 非空 = 显式态（锁定文件，原样返回）；空 = 自动态（扫 `dirs` 顶层 `*.html`，排序保稳定）。
/// 自动态仅扫顶层（非递归）：避免误纳子目录的设计系统/模板片段。
fn resolve_html_list(workspace_root: &Path, pkg: &PackageCfg) -> Result<Vec<String>, String> {
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

/// Run the full build pipeline for a workspace rooted at workspace_root.
///
/// Steps:
/// 1. load workspace, resolve output_dir, create atlas/fonts/ui dirs
/// 2. per atlas: collect_pngs -> pack_atlas -> save pages + atlas.json
/// 3. per font: copy -> fonts/<basename>.bytes
/// 4. per package: resolve_html_list -> pack_components -> write ui/<name>.pkg.bin
/// 5. cross-validate HTML referenced_sprites against atlases (non-silent on missing)
/// 6. write loom.runtime.json (packages field filled from step 4) -> return BuildReport
pub fn build(workspace_root: &Path) -> Result<BuildReport, String> {
    let ws = load_workspace(workspace_root)?;
    if ws.output_dir.trim().is_empty() {
        return Err(
            "output_dir not configured: set it in the workspace General page before building"
                .into(),
        );
    }
    let output_dir = workspace_root.join(&ws.output_dir);

    let ui_dir = output_dir.join("ui");
    let atlas_dir = output_dir.join("atlas");
    let fonts_dir = output_dir.join("fonts");
    std::fs::create_dir_all(&ui_dir)
        .map_err(|e| format!("create ui dir {}: {e}", ui_dir.display()))?;
    std::fs::create_dir_all(&atlas_dir)
        .map_err(|e| format!("create atlas dir {}: {e}", atlas_dir.display()))?;
    std::fs::create_dir_all(&fonts_dir)
        .map_err(|e| format!("create fonts dir {}: {e}", fonts_dir.display()))?;

    let mut report = BuildReport {
        packages: Vec::new(),
        atlases: Vec::new(),
        fonts: Vec::new(),
        log: Vec::new(),
    };

    // ---------- Atlases ----------
    let mut atlas_manifests: Vec<(String, crate::atlas::AtlasManifest)> = Vec::new();
    for atlas in &ws.atlases {
        report.log.push(format!(
            "collecting atlas {} from {:?}",
            atlas.name, atlas.dirs
        ));
        let images = collect_pngs(workspace_root, atlas)?;
        report
            .log
            .push(format!("  {} pngs collected", images.len()));

        let packed = pack_atlas(atlas, &images)?;

        // Save each page PNG.
        for (i, page_img) in packed.pages.iter().enumerate() {
            let page_name = crate::atlas::pack::page_file_name(&atlas.name, i);
            let page_path = atlas_dir.join(&page_name);
            page_img
                .save(&page_path)
                .map_err(|e| format!("save atlas page {}: {e}", page_path.display()))?;
        }

        // Write atlas manifest JSON (pretty).
        let manifest_path = atlas_dir.join(format!("{}.atlas.json", atlas.name));
        let manifest_text = serde_json::to_string_pretty(&packed.manifest)
            .map_err(|e| format!("serialize atlas manifest {}: {e}", atlas.name))?;
        std::fs::write(&manifest_path, manifest_text)
            .map_err(|e| format!("write {}: {e}", manifest_path.display()))?;

        report.atlases.push(atlas.name.clone());
        report
            .log
            .push(format!("  wrote {} page(s) + manifest", packed.pages.len()));
        atlas_manifests.push((atlas.name.clone(), packed.manifest));
    }

    // ---------- Fonts ----------
    for font in &ws.fonts {
        let src = workspace_root.join(&font.file);
        if !src.exists() {
            return Err(format!("font file not found: {}", src.display()));
        }
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

    // ---------- Packages (HTML -> .pkg.bin) ----------
    // R3 rebuild: 重建 d8fe705 删掉的 HTML→pkg.bin 编排。fence + bridge 现已存在。
    // 必须在 Runtime manifest 之前：runtime.packages = report.packages.clone()，
    // 故先填 report.packages 再序列化 runtime（brief 原排序会让 runtime.packages 恒空）。
    let mut all_refs: Vec<String> = Vec::new();
    for pkg in &ws.packages {
        let html_files = resolve_html_list(workspace_root, pkg)?;
        let comps: Vec<(String, String)> = html_files
            .iter()
            .map(|rel| {
                let path = workspace_root.join(rel);
                let src = std::fs::read_to_string(&path)
                    .map_err(|e| format!("read {}: {e}", path.display()))?;
                Ok((stem(rel), src))
            })
            .collect::<Result<Vec<_>, String>>()?;
        report.log.push(format!(
            "packaging {} ({} component html)",
            pkg.name,
            comps.len()
        ));
        let (bytes, refs) = pack_components(&comps)?;
        let pkg_path = ui_dir.join(format!("{}.pkg.bin", pkg.name));
        std::fs::write(&pkg_path, &bytes)
            .map_err(|e| format!("write {}: {e}", pkg_path.display()))?;
        report.packages.push(pkg.name.clone());
        report.log.push(format!(
            "  wrote {} ({} bytes)",
            pkg_path.display(),
            bytes.len()
        ));
        all_refs.extend(refs);
    }

    // ---------- Cross-validate: HTML refs must all be in some atlas ----------
    // 单向：html 引用的图必须在某 atlas；atlas 未引用的图合法（运行时动态图标）。
    // 复活 atlas/validate.rs 的死代码——缺失 sprite 非静默（build 失败）。
    if !all_refs.is_empty() {
        let atlas_refs: Vec<(String, &crate::atlas::AtlasManifest)> = atlas_manifests
            .iter()
            .map(|(n, m)| (n.clone(), m))
            .collect();
        crate::atlas::validate::assign_and_validate(&all_refs, &atlas_refs)?;
    }

    // ---------- Runtime manifest ----------
    let runtime = RuntimeManifest {
        version: 1,
        packages: report.packages.clone(),
        atlases: report.atlases.clone(),
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
    let runtime_text = serde_json::to_string_pretty(&runtime)
        .map_err(|e| format!("serialize runtime manifest: {e}"))?;
    std::fs::write(&runtime_path, runtime_text)
        .map_err(|e| format!("write {}: {e}", runtime_path.display()))?;
    report.log.push(format!("wrote {}", runtime_path.display()));

    Ok(report)
}

#[cfg(test)]
mod package_tests {
    use super::*;
    use loomgui_core::scene::NodeKind;

    #[test]
    fn pack_components_roundtrip_single() {
        let comps = vec![(
            "home".to_string(),
            r#"<div class="root"><p>hi</p><img src="icons/a.png"></div>"#.to_string(),
        )];
        let (bytes, refs) = pack_components(&comps).unwrap();
        let pkg = loomgui_core::asset::read_package(&bytes).unwrap();
        let comp = pkg.components.get("home").expect("home component");
        assert_eq!(comp.nodes[0].kind, NodeKind::Container); // div
        assert!(
            refs.iter().any(|r| r == "icons/a.png"),
            "referenced_sprites missing: {refs:?}"
        );
    }

    #[test]
    fn pack_components_multi_component() {
        let comps = vec![
            (
                "nav".to_string(),
                r#"<nav><a href="x">l</a></nav>"#.to_string(),
            ),
            (
                "page".to_string(),
                r#"<div class="page">body</div>"#.to_string(),
            ),
        ];
        let (bytes, _) = pack_components(&comps).unwrap();
        let pkg = loomgui_core::asset::read_package(&bytes).unwrap();
        assert!(pkg.components.contains_key("nav"));
        assert!(pkg.components.contains_key("page"));
    }

    #[test]
    fn pack_components_propagates_bridge_error() {
        // 多根 → bridge 报错（不静默产森林）；错误带组件名定位来源。
        let comps = vec![("bad".to_string(), r#"<div>a</div><div>b</div>"#.to_string())];
        let err = pack_components(&comps).expect_err("multi-root should error");
        assert!(
            err.contains("component bad"),
            "bridge error should name the component: {err}"
        );
    }

    #[test]
    fn pack_components_rejects_duplicate_names() {
        // 同名组件：write_package 不查（返 Vec<u8> 无 Result），read_package 运行时才
        // DupComponent 拒绝——产物是静默坏包。pack_components 构建期须 fail fast。
        let comps = vec![
            ("dup".to_string(), r#"<div>a</div>"#.to_string()),
            ("dup".to_string(), r#"<div>b</div>"#.to_string()),
        ];
        let err = pack_components(&comps).expect_err("dup names should error");
        assert!(
            err.contains("duplicate component name") && err.contains("dup"),
            "dup-name error should be descriptive: {err}"
        );
    }
}
