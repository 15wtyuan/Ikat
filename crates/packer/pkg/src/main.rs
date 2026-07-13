//! loom-pkg CLI：零参 build 读工作区配置一键打包。
//! 用法：loom-pkg build <workspace-dir>
//!   读 <workspace-dir>/loom.workspace.json，全量产出到配置的 output_dir。

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 || args[1] != "build" {
        eprintln!(
            "usage: {} build <workspace-dir>",
            args.first().map(String::as_str).unwrap_or("loom-pkg")
        );
        return ExitCode::from(2);
    }
    let root = PathBuf::from(&args[2]);
    match loomgui_pkg::build::build(&root) {
        Ok(report) => {
            for line in &report.log {
                eprintln!("{line}");
            }
            eprintln!(
                "OK: {} packages, {} atlases, {} fonts",
                report.packages.len(),
                report.atlases.len(),
                report.fonts.len()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("build failed: {e}");
            ExitCode::FAILURE
        }
    }
}
