//! 共享路径辅助。路径推算以 xtask 的 CARGO_MANIFEST_DIR (= crates/xtask) 为基准。

use std::path::PathBuf;

/// 仓库根目录。
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}
