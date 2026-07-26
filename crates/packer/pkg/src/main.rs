//! loom-pkg CLI: read workspace config and build atlases + fonts + runtime manifest.
//! Usage: loom-pkg build <workspace-dir>
//!   Reads <workspace-dir>/loom.workspace.json, outputs to the configured output_dir.

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
            // 围栏一致性 warning（W1/W2）打到 stderr：合法但预览≠运行时的不一致，
            // 不阻断打包，但作者须看到以补全声明。修前 warning 被丢弃，CLI 用户名存实亡。
            for w in &report.warnings {
                eprintln!("{}", w.render());
            }
            for line in &report.log {
                eprintln!("{line}");
            }
            eprintln!(
                "OK: {} atlases, {} fonts{}",
                report.atlases.len(),
                report.fonts.len(),
                if report.warnings.is_empty() {
                    String::new()
                } else {
                    format!(", {} warning(s)", report.warnings.len())
                },
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("build failed: {e}");
            ExitCode::FAILURE
        }
    }
}
