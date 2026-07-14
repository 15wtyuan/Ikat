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
            for line in &report.log {
                eprintln!("{line}");
            }
            eprintln!(
                "OK: {} atlases, {} fonts",
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
