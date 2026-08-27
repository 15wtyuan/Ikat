//! 收集 + 解码 atlas 源 PNG。递归扫 atlas.dirs，key = 相对工作区根路径。

use crate::workspace::AtlasCfg;
use std::path::Path;

/// 一张已解码的源图。
pub struct SourceImage {
    /// sprite_key = 图相对工作区根路径（正斜杠）。
    pub key: String,
    pub rgba: image::RgbaImage,
    pub w: u32,
    pub h: u32,
}

/// 递归扫 atlas.dirs 下所有 .png，解码 RGBA8。key 按字母序、去重。
pub fn collect_pngs(workspace_root: &Path, atlas: &AtlasCfg) -> Result<Vec<SourceImage>, String> {
    let mut keys: Vec<String> = Vec::new();
    for dir in &atlas.dirs {
        let abs_dir = workspace_root.join(dir);
        collect_dir(workspace_root, &abs_dir, &mut keys)?;
    }
    keys.sort();
    keys.dedup();

    let mut out = Vec::with_capacity(keys.len());
    for key in keys {
        let abs = workspace_root.join(&key);
        let img = image::open(&abs)
            .map_err(|e| format!("decode {}: {e}", abs.display()))?
            .to_rgba8();
        let (w, h) = (img.width(), img.height());
        out.push(SourceImage {
            key,
            rgba: img,
            w,
            h,
        });
    }
    Ok(out)
}

/// 递归收一个目录下的 .png，push 相对根 key。
fn collect_dir(workspace_root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
    if !dir.is_dir() {
        return Ok(()); // 目录不存在 → 空收集（上层报错留给交叉验证）
    }
    for entry in std::fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("dir entry: {e}"))?;
        let path = entry.path();
        if path.is_dir() {
            collect_dir(workspace_root, &path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("png") {
            let rel = path
                .strip_prefix(workspace_root)
                .map_err(|_| format!("png {} 不在工作区根下", path.display()))?;
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_recursive_sorted() {
        let tmp = std::env::temp_dir().join("ikat_collect_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("assets/icons")).unwrap();
        std::fs::create_dir_all(tmp.join("assets/sub")).unwrap();
        // 写两张 2x2 PNG。
        let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]));
        img.save(tmp.join("assets/icons/b.png")).unwrap();
        img.save(tmp.join("assets/sub/a.png")).unwrap();

        let cfg = AtlasCfg {
            name: "ui".into(),
            standalone: false,
            dirs: vec!["assets".into()],
            max_size: 2048,
            padding: 4,
        };
        let imgs = collect_pngs(&tmp, &cfg).unwrap();
        assert_eq!(imgs.len(), 2);
        assert_eq!(imgs[0].key, "assets/icons/b.png", "递归 + 字母序");
        assert_eq!(imgs[1].key, "assets/sub/a.png");
        assert_eq!(imgs[0].w, 2);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
