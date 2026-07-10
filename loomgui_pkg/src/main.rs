//! 极简 CLI（不引 clap）：loomgui_pkg <workspaceRoot> <pkgName> [--html <h1,h2,...>] [-o <out.pkg.bin>]。
//! 不传 --html → 扫 workspaceRoot 顶层所有 .html（不递归，排除 res 目录）。
//! -o 默认 <workspaceRoot>/<pkgName>.pkg.bin。
//! 产物只写 pkg.bin（图集归 Unity）。

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!(
            "usage: {} <workspaceRoot> <pkgName> [--html <h1,h2,...>] [-o <out.pkg.bin>]",
            args.first().map(String::as_str).unwrap_or("loomgui_pkg")
        );
        return ExitCode::from(2);
    }
    let workspace_root = PathBuf::from(&args[1]);
    let pkg_name = &args[2];
    let mut html_list: Option<Vec<String>> = None;
    let mut out_path: Option<String> = None;
    let mut i = 3;
    while i < args.len() {
        match args[i].as_str() {
            "--html" => {
                let v = args.get(i + 1).cloned().unwrap_or_default();
                html_list = Some(
                    v.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect(),
                );
                i += 2;
            }
            "-o" => {
                out_path = args.get(i + 1).cloned();
                i += 2;
            }
            other => {
                eprintln!("unknown arg: {other}");
                return ExitCode::from(2);
            }
        }
    }

    // 不传 --html → 扫 workspaceRoot 顶层所有 .html（不递归）。
    let html_files: Vec<String> = match html_list {
        Some(list) => list,
        None => match scan_top_level_html(&workspace_root) {
            Ok(list) if !list.is_empty() => list,
            Ok(_) => {
                eprintln!("no .html files found in {}", workspace_root.display());
                return ExitCode::FAILURE;
            }
            Err(e) => {
                eprintln!("scan {}: {e}", workspace_root.display());
                return ExitCode::FAILURE;
            }
        },
    };

    let out = out_path.unwrap_or_else(|| {
        workspace_root
            .join(format!("{pkg_name}.pkg.bin"))
            .to_string_lossy()
            .into_owned()
    });

    // 构造 [(relative_path, absolute_path)] 列表
    let html_pairs: Vec<(String, PathBuf)> = html_files
        .iter()
        .map(|name| {
            let abs = workspace_root.join(name);
            (name.clone(), abs)
        })
        .collect();

    match loomgui_pkg::pack(&workspace_root, pkg_name, &html_pairs) {
        Ok(p) => {
            if let Err(e) = fs::write(&out, &p.pkg_bytes) {
                eprintln!("write {out}: {e}");
                return ExitCode::FAILURE;
            }
            eprintln!(
                "wrote {out} ({} bytes, {} components, {} referenced sprites)",
                p.pkg_bytes.len(),
                html_files.len(),
                p.referenced_sprites.len()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("pack: {e}");
            ExitCode::FAILURE
        }
    }
}

/// 扫 workspaceRoot 顶层 .html 文件（不递归子目录）。
/// 返回相对 workspaceRoot 的文件名列表（如 ["a.html", "b.html"]），按字母序。
fn scan_top_level_html(workspace_root: &Path) -> std::io::Result<Vec<String>> {
    let mut list: Vec<String> = Vec::new();
    for entry in fs::read_dir(workspace_root)? {
        let entry = entry?;
        let path = entry.path();
        // 只收文件（跳过子目录），不递归。
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.extension().and_then(|e| e.to_str()) == Some("html") {
            list.push(name);
        }
    }
    list.sort();
    Ok(list)
}
