//! img src (relative to html file) -> sprite_key (image path relative to workspace root, forward-slash).
//! Browser-native semantics: `<img src="home.png">` resolves relative to the html file's directory.
//! sprite_key is globally unique (root-relative path), preventing collisions from same-named files in different directories.

use std::path::{Component, Path, PathBuf};

/// Resolve an html `<img src>` into an image path relative to the workspace root (forward-slash).
/// - `workspace_root`: absolute path to workspace root.
/// - `html_file`: absolute path to the html file (src resolves relative to its parent directory).
/// - `src`: raw `<img src>` value (relative to html file).
///
/// Lexically normalizes `.`/`..`, then strips the workspace root prefix.
/// If the resolved path escapes the workspace root -> Err.
/// Pure lexical math; does not touch disk (the image may not exist yet).
pub fn resolve_img_src(
    workspace_root: &Path,
    html_file: &Path,
    src: &str,
) -> Result<String, String> {
    let base = html_file
        .parent()
        .ok_or_else(|| format!("html file has no parent: {}", html_file.display()))?;
    let joined = base.join(src);
    // Single-pass lexical normalization: handle CurDir/ParentDir inline.
    let mut normalized = PathBuf::new();
    for comp in joined.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    let rel = normalized.strip_prefix(workspace_root).map_err(|_| {
        format!(
            "img src `{src}` resolves to {} which escapes workspace root {}",
            normalized.display(),
            workspace_root.display()
        )
    })?;
    Ok(rel.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn root() -> PathBuf {
        PathBuf::from(if cfg!(windows) { r"C:\ws" } else { "/ws" })
    }

    #[test]
    fn sibling_image() {
        let html = root().join("ui").join("showcase").join("main.html");
        let key = resolve_img_src(&root(), &html, "home.png").unwrap();
        assert_eq!(key, "ui/showcase/home.png");
    }

    #[test]
    fn subdir_image() {
        let html = root().join("ui").join("showcase").join("main.html");
        let key = resolve_img_src(&root(), &html, "images/x.png").unwrap();
        assert_eq!(key, "ui/showcase/images/x.png");
    }

    #[test]
    fn parent_traversal_into_assets() {
        let html = root().join("ui").join("showcase").join("main.html");
        let key = resolve_img_src(&root(), &html, "../../assets/icon.png").unwrap();
        assert_eq!(key, "assets/icon.png");
    }

    #[test]
    fn escape_root_errors() {
        let html = root().join("main.html");
        let err = resolve_img_src(&root(), &html, "../outside.png");
        assert!(err.is_err(), "escaping workspace root should error");
    }
}
