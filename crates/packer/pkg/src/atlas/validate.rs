//! 交叉验证：html 引用的图都能在某 atlas 找到（单向；atlas 里未引用的图合法——运行时动态图标）。

use super::AtlasManifest;
use crate::diag::{code, PackDiagnostic};

/// 验证每个被引用 sprite_key 恰好归属一个 atlas。`atlases` = [(atlas_name, manifest)]。
///
/// collect-all：每个违规 key 一条诊断，全量收集后返回（空 = 通过）——check/build
/// 共享同一份结果，AI 一轮修全。file 用 sprite_key（即资源相对路径，可直接定位）。
pub fn assign_and_validate(
    referenced: &[String],
    atlases: &[(String, &AtlasManifest)],
) -> Vec<PackDiagnostic> {
    let mut diags = Vec::new();
    for key in referenced {
        let owners: Vec<&str> = atlases
            .iter()
            .filter(|(_, m)| m.sprites.contains_key(key))
            .map(|(n, _)| n.as_str())
            .collect();
        match owners.len() {
            0 => diags.push(PackDiagnostic::synthetic_error(
                code::SPRITE_MISSING_FROM_ATLAS,
                "",
                key,
                format!("图 `{key}` 被引用但不在任何 atlas；把它所在目录加进某 atlas.dirs"),
            )),
            1 => {}
            _ => diags.push(PackDiagnostic::synthetic_error(
                code::SPRITE_ATLAS_CONFLICT,
                "",
                key,
                format!(
                    "图 `{key}` 同时进了多个 atlas：{}；调整 atlas.dirs 使不重叠",
                    owners.join(", ")
                ),
            )),
        }
    }
    diags
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::{AtlasManifest, SpriteEntry};
    use crate::diag::Severity;
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
        assert!(assign_and_validate(&["a.png".into()], &[("ui".into(), &ui)]).is_empty());
    }

    #[test]
    fn unreferenced_atlas_image_is_fine() {
        let ui = manifest(&["a.png", "b.png"]);
        assert!(assign_and_validate(&["a.png".into()], &[("ui".into(), &ui)]).is_empty());
    }

    #[test]
    fn missing_reference_errors() {
        let ui = manifest(&["a.png"]);
        let diags = assign_and_validate(&["z.png".into()], &[("ui".into(), &ui)]);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, "SpriteMissingFromAtlas");
        assert_eq!(diags[0].severity, Severity::Error);
        assert_eq!(diags[0].file, "z.png");
    }

    #[test]
    fn conflict_errors() {
        let ui = manifest(&["a.png"]);
        let ch = manifest(&["a.png"]);
        let diags = assign_and_validate(
            &["a.png".into()],
            &[("ui".into(), &ui), ("char".into(), &ch)],
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, "SpriteAtlasConflict");
    }

    /// collect-all：缺失 + 冲突混合时两条都在（修前首错即断）。
    #[test]
    fn collects_all_violations() {
        let ui = manifest(&["a.png", "shared.png"]);
        let ch = manifest(&["shared.png"]);
        let diags = assign_and_validate(
            &["z.png".into(), "shared.png".into()],
            &[("ui".into(), &ui), ("char".into(), &ch)],
        );
        assert_eq!(diags.len(), 2, "缺失和冲突都要报: {diags:?}");
        assert!(diags.iter().any(|d| d.code == "SpriteMissingFromAtlas"));
        assert!(diags.iter().any(|d| d.code == "SpriteAtlasConflict"));
    }
}
