//! 交叉验证：html 引用的图都能在某 atlas 找到（单向；atlas 里未引用的图合法——运行时动态图标）。

use super::AtlasManifest;

/// 验证每个被引用 sprite_key 恰好归属一个 atlas。`atlases` = [(atlas_name, manifest)]。
pub fn assign_and_validate(
    referenced: &[String],
    atlases: &[(String, &AtlasManifest)],
) -> Result<(), String> {
    for key in referenced {
        let owners: Vec<&str> = atlases
            .iter()
            .filter(|(_, m)| m.sprites.contains_key(key))
            .map(|(n, _)| n.as_str())
            .collect();
        match owners.len() {
            0 => {
                return Err(format!(
                    "图 `{key}` 被引用但不在任何 atlas；把它所在目录加进某 atlas.dirs"
                ))
            }
            1 => {}
            _ => {
                return Err(format!(
                    "图 `{key}` 同时进了多个 atlas：{}；调整 atlas.dirs 使不重叠",
                    owners.join(", ")
                ))
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::{AtlasManifest, SpriteEntry};
    use std::collections::BTreeMap;

    fn manifest(keys: &[&str]) -> AtlasManifest {
        let mut sprites = BTreeMap::new();
        for k in keys {
            sprites.insert(
                k.to_string(),
                SpriteEntry {
                    page: 0,
                    uv: [0.0; 4],
                    orig: [1, 1],
                },
            );
        }
        AtlasManifest {
            pages: vec![],
            sprites,
        }
    }

    #[test]
    fn referenced_found_in_single_atlas_ok() {
        let ui = manifest(&["a.png", "b.png"]);
        assert!(assign_and_validate(&["a.png".into()], &[("ui".into(), &ui)]).is_ok());
    }

    #[test]
    fn unreferenced_atlas_image_is_fine() {
        let ui = manifest(&["a.png", "b.png"]);
        assert!(assign_and_validate(&["a.png".into()], &[("ui".into(), &ui)]).is_ok());
    }

    #[test]
    fn missing_reference_errors() {
        let ui = manifest(&["a.png"]);
        assert!(assign_and_validate(&["z.png".into()], &[("ui".into(), &ui)]).is_err());
    }

    #[test]
    fn conflict_errors() {
        let ui = manifest(&["a.png"]);
        let ch = manifest(&["a.png"]);
        assert!(assign_and_validate(
            &["a.png".into()],
            &[("ui".into(), &ui), ("char".into(), &ch)]
        )
        .is_err());
    }
}
