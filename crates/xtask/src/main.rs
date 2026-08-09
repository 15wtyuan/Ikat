//! xtask: 构建编排工具。
//! 用法: cargo run -p xtask -- <subcommand>
//! 当前子命令: sync-bindings

mod bindings;
mod paths;
mod release_check;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: cargo run -p xtask -- <subcommand>");
        eprintln!("  sync-bindings  Generate C# bindings and distribute to engine backends");
        eprintln!("  release-check  Pre-release package sanity check");
        std::process::exit(1);
    }
    match args[0].as_str() {
        "sync-bindings" => {
            if let Err(e) = bindings::sync_bindings() {
                eprintln!("sync-bindings failed: {e}");
                std::process::exit(1);
            }
        }
        "release-check" => {
            if let Err(e) = release_check::run_release_check() {
                eprintln!("release-check failed: {e}");
                std::process::exit(1);
            }
        }
        other => {
            eprintln!("unknown subcommand: {other}");
            std::process::exit(1);
        }
    }
}
