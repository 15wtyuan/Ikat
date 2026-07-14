//! Build orchestration: atlases + fonts + runtime manifest -> output_dir.
//! Single entry point build() called by CLI and GUI.
//!
//! R1.1 note: the HTML -> .pkg.bin compilation path was removed. It will be
//! rebuilt in R3 via the fence crate. Until then, the packer handles only
//! atlases, fonts, and the runtime manifest. The packages field in
//! BuildReport and RuntimeManifest is kept (always empty) for forward
//! compatibility with the GUI frontend.

use serde::Serialize;

use crate::atlas::collect::collect_pngs;
use crate::atlas::pack::pack_atlas;
use crate::runtime::{RuntimeFont, RuntimeManifest, RUNTIME_FILE};
use crate::workspace::load_workspace;
use std::path::Path;

/// Build report: what was produced.
#[derive(Debug, Clone, Serialize)]
pub struct BuildReport {
    /// Package names (always empty until R3 rebuilds the HTML path).
    pub packages: Vec<String>,
    pub atlases: Vec<String>,
    pub fonts: Vec<String>,
    pub log: Vec<String>,
}

/// Run the full build pipeline for a workspace rooted at workspace_root.
///
/// Steps:
/// 1. load workspace, resolve output_dir, create atlas/fonts/ui dirs
/// 2. per atlas: collect_pngs -> pack_atlas -> save pages + atlas.json
/// 3. per font: copy -> fonts/<basename>.bytes
/// 4. write loom.runtime.json -> return BuildReport
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

    let _ = &atlas_manifests; // kept for future cross-validation (R3)

    Ok(report)
}
