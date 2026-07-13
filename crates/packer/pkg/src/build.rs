//! Build orchestration: wire packages + atlases + fonts → output_dir.
//! Single entry point `build()` called by CLI (Task 9) and GUI (Task 18).

use serde::Serialize;

use crate::atlas::collect::collect_pngs;
use crate::atlas::pack::pack_atlas;
use crate::atlas::validate::assign_and_validate;
use crate::runtime::{RuntimeFont, RuntimeManifest, RUNTIME_FILE};
use crate::workspace::load_workspace;
use std::path::Path;

/// Build report: what was produced.
#[derive(Debug, Clone, Serialize)]
pub struct BuildReport {
    pub packages: Vec<String>,
    pub atlases: Vec<String>,
    pub fonts: Vec<String>,
    pub log: Vec<String>,
}

/// Run the full build pipeline for a workspace rooted at `workspace_root`.
///
/// Orchestration order:
/// 1. load workspace → resolve output_dir → mkdir ui/atlas/fonts
/// 2. per package: resolve html list → pack → write ui/<name>.pkg.bin, accumulate referenced_sprites
/// 3. per atlas: collect_pngs → pack_atlas → save pages + write atlas/<name>.atlas.json
/// 4. cross-validate (assign_and_validate)
/// 5. per font: copy → fonts/<basename>.bytes
/// 6. write loom.runtime.json → return BuildReport
pub fn build(workspace_root: &Path) -> Result<BuildReport, String> {
    let ws = load_workspace(workspace_root)?;
    if ws.output_dir.trim().is_empty() {
        return Err("output_dir 未配置：请在工作区「常规」页设置导出目录后再打包".into());
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
    let mut all_referenced: Vec<String> = Vec::new();

    // ── Packages ──────────────────────────────────────────────
    for pkg in &ws.packages {
        let html_list = resolve_html_list(workspace_root, pkg)?;
        let html_pairs: Vec<(String, std::path::PathBuf)> = html_list
            .iter()
            .map(|rel| (rel.clone(), workspace_root.join(rel)))
            .collect();

        report.log.push(format!(
            "packing {}: {} html files",
            pkg.name,
            html_pairs.len()
        ));
        let packed = crate::pack(workspace_root, &pkg.name, &html_pairs)?;

        let pkg_path = ui_dir.join(format!("{}.pkg.bin", pkg.name));
        std::fs::write(&pkg_path, &packed.pkg_bytes)
            .map_err(|e| format!("write {}: {e}", pkg_path.display()))?;
        report.packages.push(pkg.name.clone());
        report.log.push(format!("  wrote {}", pkg_path.display()));

        for key in &packed.referenced_sprites {
            if !all_referenced.contains(key) {
                all_referenced.push(key.clone());
            }
        }
    }

    // ── Atlases ───────────────────────────────────────────────
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

    // ── Cross-validate ────────────────────────────────────────
    {
        let refs: Vec<(String, &crate::atlas::AtlasManifest)> = atlas_manifests
            .iter()
            .map(|(n, m)| (n.clone(), m))
            .collect();
        assign_and_validate(&all_referenced, &refs)?;
        report.log.push("cross-validation passed".into());
    }

    // ── Fonts ─────────────────────────────────────────────────
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

    // ── Runtime manifest ─────────────────────────────────────
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

/// Resolve the html list for a package into workspace-root-relative paths.
///
/// - `pkg.html` non-empty (explicit): for each html filename, scan `pkg.dirs` in order;
///   first directory where the file exists wins. Returns relative paths like `ui/main.html`.
/// - `pkg.html` empty (auto-scan): scan each dir's top-level `.html` (non-recursive),
///   sorted, deduplicated.
fn resolve_html_list(
    workspace_root: &Path,
    pkg: &crate::workspace::PackageCfg,
) -> Result<Vec<String>, String> {
    if !pkg.html.is_empty() {
        let mut out = Vec::new();
        for html_name in &pkg.html {
            let mut found = false;
            for dir in &pkg.dirs {
                let candidate = workspace_root.join(dir).join(html_name);
                if candidate.exists() {
                    let rel = Path::new(dir).join(html_name);
                    let rel_str = rel.to_string_lossy().replace('\\', "/");
                    out.push(rel_str);
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(format!(
                    "html `{}` not found in any of {:?}",
                    html_name, pkg.dirs
                ));
            }
        }
        Ok(out)
    } else {
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for dir in &pkg.dirs {
            let abs_dir = workspace_root.join(dir);
            if !abs_dir.is_dir() {
                return Err(format!("package dir not found: {}", abs_dir.display()));
            }
            let entries = std::fs::read_dir(&abs_dir)
                .map_err(|e| format!("read_dir {}: {e}", abs_dir.display()))?;
            for entry in entries {
                let entry = entry.map_err(|e| format!("dir entry: {e}"))?;
                let path = entry.path();
                if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("html") {
                    if let Ok(rel) = path.strip_prefix(workspace_root) {
                        let key = rel.to_string_lossy().replace('\\', "/");
                        if seen.insert(key.clone()) {
                            out.push(key);
                        }
                    }
                }
            }
        }
        out.sort();
        Ok(out)
    }
}
