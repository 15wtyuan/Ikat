//! xtask: 构建编排工具。
//! 用法: cargo run -p xtask -- <subcommand> [--dry-run]

mod bindings;
mod git;
mod gui;
mod paths;
mod release;
mod release_check;
mod reout;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        usage();
        std::process::exit(1);
    }
    // `--dry-run`（任意位置）统一剥出，其余按位置参解析。
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let positional: Vec<&String> = args.iter().filter(|a| *a != "--dry-run").collect();
    match (
        positional[0].as_str(),
        positional.get(1).map(|s| s.as_str()),
    ) {
        ("sync-bindings", _) => {
            if let Err(e) = bindings::sync_bindings() {
                eprintln!("sync-bindings failed: {e}");
                std::process::exit(1);
            }
        }
        ("release-check", _) => {
            if let Err(e) = release_check::run_release_check() {
                eprintln!("release-check failed: {e}");
                std::process::exit(1);
            }
        }
        ("release", Some(ver)) => {
            if let Err(e) = release::run_release(ver, dry_run) {
                eprintln!("release {ver} failed: {e}");
                std::process::exit(1);
            }
        }
        ("reout", None) => {
            if let Err(e) = reout::run_reout(dry_run) {
                eprintln!("reout failed: {e}");
                std::process::exit(1);
            }
        }
        _ => {
            usage();
            std::process::exit(1);
        }
    }
}

fn usage() {
    eprintln!("usage: cargo run -p xtask -- <subcommand> [--dry-run]");
    eprintln!(
        "  sync-bindings              Generate C# bindings and distribute to engine backends"
    );
    eprintln!(
        "  release-check              Pre-release sanity gates (version alignment / changelog /"
    );
    eprintln!("                             dll existence / asmdef / artifact staleness)");
    eprintln!(
        "  release <ver> [--dry-run]  Full release orchestration: bump + changelog fold + clean-"
    );
    eprintln!(
        "                             worktree artifact build + verify + commit + tag + push"
    );
    eprintln!(
        "  reout [--dry-run]          Daily artifact re-out after Rust changes (dll/exe/bindings/bundle/fixtures)"
    );
    eprintln!("                             showcase bundle) — reports dirty paths, never commits");
}
