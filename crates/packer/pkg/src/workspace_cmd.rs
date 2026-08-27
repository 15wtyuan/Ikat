//! workspace 管理命令：查询（list / show）与变更（new / font add / atlas add）。
//!
//! AI 的主编辑路径——改 ikat.workspace.json 一律走这些命令而非手编（实体量级成百
//! 上千，手编是事故制造机）。查询分两级：list 摘要（一行一实体，护 AI 上下文）；
//! show 单包明细（纯配置 + 文件系统扫描，不跑 analyze——重校验是 check 的事）。
//! 写操作整文件重写、无锁（与 GUI 表单双写容忍：低频、最后写者赢）。

use crate::build::resolve_html_list;
use crate::diag::BuildFailure;
use crate::runtime::DesignDim;
use crate::workspace::{load_workspace, save_workspace, AtlasCfg, PackageCfg};
use serde::Serialize;
use std::path::Path;

/// `list pkg` 的单包摘要。
#[derive(Debug, Clone, Serialize)]
pub struct PkgSummary {
    pub name: String,
    /// 页面 HTML 文件数（resolve_html_list 口径）。
    pub pages: usize,
    /// components/ 下的自定义组件数。
    pub components: usize,
}

/// `list atlas` 的单图集摘要。
#[derive(Debug, Clone, Serialize)]
pub struct AtlasSummary {
    pub name: String,
    pub dirs: Vec<String>,
    pub standalone: bool,
    pub max_size: u32,
    pub padding: u32,
    /// dirs 下递归扫到的 PNG 数（配置 + 文件系统现状对账）。
    pub sprites: usize,
}

/// `list font` 的单字体摘要。
#[derive(Debug, Clone, Serialize)]
pub struct FontSummary {
    pub family: String,
    pub file: String,
    pub default: bool,
    pub fallback: bool,
}

/// `show <pkg>` 的单包明细。
#[derive(Debug, Clone, Serialize)]
pub struct PkgDetail {
    pub name: String,
    pub dirs: Vec<String>,
    /// 页面 HTML 文件相对路径（工作区根、正斜杠）。
    pub pages: Vec<String>,
    /// 注册的自定义组件 tag（components/*.html 的文件名主干）。
    pub components: Vec<String>,
}

pub fn list_pkgs(root: &Path) -> Result<Vec<PkgSummary>, BuildFailure> {
    let ws = load_workspace(root)?;
    let mut out = Vec::new();
    for pkg in &ws.packages {
        let pages = resolve_html_list(root, pkg)?;
        out.push(PkgSummary {
            name: pkg.name.clone(),
            pages: pages.len(),
            components: count_component_files(root, pkg)?,
        });
    }
    Ok(out)
}

pub fn list_atlases(root: &Path) -> Result<Vec<AtlasSummary>, BuildFailure> {
    let ws = load_workspace(root)?;
    Ok(ws
        .atlases
        .iter()
        .map(|a| AtlasSummary {
            name: a.name.clone(),
            dirs: a.dirs.clone(),
            standalone: a.standalone,
            max_size: a.max_size,
            padding: a.padding,
            sprites: a
                .dirs
                .iter()
                .map(|d| count_pngs_recursive(&root.join(d)))
                .sum(),
        })
        .collect())
}

pub fn list_fonts(root: &Path) -> Result<Vec<FontSummary>, BuildFailure> {
    let ws = load_workspace(root)?;
    Ok(ws
        .fonts
        .iter()
        .map(|f| FontSummary {
            family: f.family.clone(),
            file: f.file.clone(),
            default: f.default,
            fallback: f.fallback,
        })
        .collect())
}

pub fn show_pkg(root: &Path, name: &str) -> Result<PkgDetail, BuildFailure> {
    let ws = load_workspace(root)?;
    let pkg = ws
        .packages
        .iter()
        .find(|p| p.name == name)
        .ok_or_else(|| BuildFailure::config(format!("package `{name}` not found")))?;
    let pages = resolve_html_list(root, pkg)?;
    Ok(PkgDetail {
        name: pkg.name.clone(),
        dirs: pkg.dirs.clone(),
        pages,
        components: component_files(root, pkg)?,
    })
}

/// `new <name>`：建 `ui/<name>/main.html`（最小围栏合法页）+ 注册 packages[]。
/// 保证 new 后 check 必绿（cargo new "hello world" 同款承诺）。
pub fn new_pkg(root: &Path, name: &str) -> Result<PkgSummary, BuildFailure> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.trim() != name
    {
        return Err(BuildFailure::config(format!(
            "invalid package name `{name}`: must be a plain directory-name-safe token"
        )));
    }
    let mut ws = load_workspace(root)?;
    if ws.packages.iter().any(|p| p.name == name) {
        return Err(BuildFailure::validation(
            format!("package `{name}` already registered (duplicate package name)"),
            vec![],
        ));
    }
    let dir_rel = format!("ui/{name}");
    let dir = root.join(&dir_rel);
    // 目录已有 HTML → 拒绝（避免覆盖作者已有页面；空目录/不存在目录正常创建）。
    let existing_htmls = resolve_html_list(
        root,
        &PackageCfg {
            name: name.to_string(),
            dirs: vec![dir_rel.clone()],
            html: vec![],
        },
    )
    .unwrap_or_default();
    if !existing_htmls.is_empty() {
        return Err(BuildFailure::validation(
            format!(
                "directory `{dir_rel}` already contains html ({first}...); \
                 pick another name or register it manually",
                first = existing_htmls[0]
            ),
            vec![],
        ));
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let page = format!(
        "<!DOCTYPE html>\n<html>\n<head><title>{name}</title></head>\n<body>\n\
         <div style=\"width:100%;height:100%;display:flex;align-items:center;justify-content:center\">\n  \
         <div>Hello, {name}!</div>\n</div>\n</body>\n</html>\n"
    );
    std::fs::write(dir.join("main.html"), page).map_err(|e| format!("write main.html: {e}"))?;
    ws.packages.push(PackageCfg {
        name: name.to_string(),
        dirs: vec![dir_rel],
        html: vec![],
    });
    save_workspace(root, &ws)?;
    Ok(PkgSummary {
        name: name.to_string(),
        pages: 1,
        components: 0,
    })
}

/// `font add <file>`：拷文件进 `fonts/` + 注册 fonts[]。family 冲突拒绝。
pub fn add_font(
    root: &Path,
    src: &Path,
    family: &str,
    default: bool,
    fallback: bool,
) -> Result<FontSummary, BuildFailure> {
    if family.trim().is_empty() {
        return Err(BuildFailure::config(
            "font family must not be empty (--family)",
        ));
    }
    if !src.is_file() {
        return Err(BuildFailure::config(format!(
            "font file not found: {}",
            src.display()
        )));
    }
    let mut ws = load_workspace(root)?;
    if ws.fonts.iter().any(|f| f.family == family) {
        return Err(BuildFailure::validation(
            format!("font family `{family}` already registered"),
            vec![],
        ));
    }
    let basename = src
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| BuildFailure::config(format!("invalid font path: {}", src.display())))?;
    let fonts_dir = root.join("fonts");
    std::fs::create_dir_all(&fonts_dir).map_err(|e| format!("create fonts dir: {e}"))?;
    let dst = fonts_dir.join(basename);
    // 源文件已在 fonts/ 落位（先手动放进 fonts/ 再注册）时跳过拷贝：同源同目标的
    // fs::copy 在 Windows 报共享冲突，报出来像文件被第三方锁住，极具误导性。
    if !same_file(src, &dst) {
        std::fs::copy(src, &dst).map_err(|e| format!("copy font {}: {e}", src.display()))?;
    }
    let file = format!("fonts/{basename}");
    ws.fonts.push(crate::workspace::FontCfg {
        family: family.to_string(),
        file: file.clone(),
        default,
        fallback,
    });
    save_workspace(root, &ws)?;
    Ok(FontSummary {
        family: family.to_string(),
        file,
        default,
        fallback,
    })
}

/// `ikat design` 回显实体（写命令成功回显实体 JSON 契约）。
#[derive(serde::Serialize)]
pub struct DesignEcho {
    pub design: Option<DesignDim>,
    pub match_mode: Option<String>,
}

/// `design [WxH] [--match letterbox|fit-width|fit-height] [--clear]`：设/清设计分辨率
/// 与适配模式（workspace.design/match_mode——分辨率适配配置正主，`ikat build` 透传
/// runtime.json，引擎集成层消费）。size/mode 只在显式给时动；`--clear` 全清。
pub fn set_design(
    root: &Path,
    size: Option<(f32, f32)>,
    mode: Option<String>,
    clear: bool,
) -> Result<DesignEcho, BuildFailure> {
    if let Some((w, h)) = size {
        if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 {
            return Err(BuildFailure::config(
                "design size must be positive finite (e.g. 1920x1080)",
            ));
        }
    }
    if let Some(m) = &mode {
        if !matches!(m.as_str(), "letterbox" | "fit-width" | "fit-height") {
            return Err(BuildFailure::config(
                "match_mode must be letterbox | fit-width | fit-height",
            ));
        }
    }
    let mut ws = load_workspace(root)?;
    if clear {
        ws.design = None;
        ws.match_mode = None;
    } else {
        if let Some((w, h)) = size {
            ws.design = Some(DesignDim { w, h });
        }
        if let Some(m) = mode {
            ws.match_mode = Some(m);
        }
    }
    save_workspace(root, &ws)?;
    Ok(DesignEcho {
        design: ws.design,
        match_mode: ws.match_mode,
    })
}

/// `atlas add <dir>`：注册 atlases[] 一条。dir 已被其他图集扫描 → 拒绝（会造覆盖冲突）。
#[allow(clippy::too_many_arguments)]
pub fn add_atlas(
    root: &Path,
    dir_rel: &str,
    name: Option<String>,
    max_size: u32,
    padding: u32,
    standalone: bool,
) -> Result<AtlasSummary, BuildFailure> {
    let dir_rel = dir_rel.replace('\\', "/");
    let dir = root.join(&dir_rel);
    if !dir.is_dir() {
        return Err(BuildFailure::config(format!(
            "atlas dir not found: {} (create it first or check the path)",
            dir.display()
        )));
    }
    let name = match name {
        Some(n) => n,
        None => dir_rel
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("atlas")
            .to_string(),
    };
    let mut ws = load_workspace(root)?;
    if let Some(owner) = ws.atlases.iter().find(|a| a.dirs.contains(&dir_rel)) {
        return Err(BuildFailure::validation(
            format!(
                "dir `{dir_rel}` already scanned by atlas `{}` (overlapping dirs cause \
                 SpriteAtlasConflict)",
                owner.name
            ),
            vec![],
        ));
    }
    if ws.atlases.iter().any(|a| a.name == name) {
        return Err(BuildFailure::validation(
            format!("atlas name `{name}` already registered"),
            vec![],
        ));
    }
    let summary = AtlasSummary {
        name: name.clone(),
        dirs: vec![dir_rel.clone()],
        standalone,
        max_size,
        padding,
        sprites: count_pngs_recursive(&dir),
    };
    ws.atlases.push(AtlasCfg {
        name,
        standalone,
        dirs: vec![dir_rel],
        max_size,
        padding,
    });
    save_workspace(root, &ws)?;
    Ok(summary)
}

fn count_component_files(root: &Path, pkg: &PackageCfg) -> Result<usize, BuildFailure> {
    Ok(component_files(root, pkg)?.len())
}

/// 两路径是否指向同一文件。canonicalize 优先（解析真实盘上拼写与归一 . / ..），
/// 任一端不存在时退词法绝对路径比较。
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => std::path::absolute(a).ok() == std::path::absolute(b).ok(),
    }
}

/// components/ 目录下的组件 tag 列表（每 package dir 一个 components/ 子目录）。
fn component_files(root: &Path, pkg: &PackageCfg) -> Result<Vec<String>, BuildFailure> {
    let mut out = Vec::new();
    for dir in &pkg.dirs {
        let comp_dir = root.join(dir).join("components");
        let Ok(entries) = std::fs::read_dir(&comp_dir) else {
            continue; // 无 components/ = 无自定义组件，合法
        };
        let mut tags: Vec<String> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("html"))
            .filter_map(|p| p.file_stem()?.to_str().map(String::from))
            .collect();
        tags.sort();
        out.extend(tags);
    }
    Ok(out)
}

/// 递归数 PNG（只看扩展名——快；尺寸/解码是打包时的事）。
fn count_pngs_recursive(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut n = 0;
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            n += count_pngs_recursive(&p);
        } else if p.extension().and_then(|e| e.to_str()) == Some("png") {
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ws(tmp: &std::path::Path) {
        std::fs::create_dir_all(tmp.join("ui/showcase")).unwrap();
        std::fs::write(tmp.join("ui/showcase/home.html"), r#"<div>hi</div>"#).unwrap();
        std::fs::write(
            tmp.join("ikat.workspace.json"),
            r#"{"version":1,"output_dir":"output","packages":[{"name":"showcase","dirs":["ui/showcase"],"html":[]}],"atlases":[],"fonts":[]}"#,
        )
        .unwrap();
    }

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let tmp = std::env::temp_dir().join(format!("ikat_wcmd_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        tmp
    }

    #[test]
    fn set_design_writes_and_partial_update_keeps_other_field() {
        let tmp = tmpdir("design");
        make_ws(&tmp);
        // 全量设
        let echo = set_design(
            &tmp,
            Some((1920.0, 1080.0)),
            Some("fit-width".into()),
            false,
        )
        .unwrap();
        assert_eq!(
            echo.design,
            Some(DesignDim {
                w: 1920.0,
                h: 1080.0
            })
        );
        assert_eq!(echo.match_mode.as_deref(), Some("fit-width"));
        // 只动 mode：design 保留（部分更新语义）
        let echo = set_design(&tmp, None, Some("letterbox".into()), false).unwrap();
        assert_eq!(
            echo.design,
            Some(DesignDim {
                w: 1920.0,
                h: 1080.0
            })
        );
        assert_eq!(echo.match_mode.as_deref(), Some("letterbox"));
        // 落盘核对
        let ws = load_workspace(&tmp).unwrap();
        assert_eq!(
            ws.design,
            Some(DesignDim {
                w: 1920.0,
                h: 1080.0
            })
        );
        assert_eq!(ws.match_mode.as_deref(), Some("letterbox"));
        // clear 全清
        let echo = set_design(&tmp, None, None, true).unwrap();
        assert_eq!(echo.design, None);
        assert_eq!(echo.match_mode, None);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn set_design_rejects_invalid_values() {
        let tmp = tmpdir("design_bad");
        make_ws(&tmp);
        assert!(set_design(&tmp, Some((0.0, 1080.0)), None, false).is_err());
        assert!(set_design(&tmp, None, Some("expand".into()), false).is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn list_pkg_reports_pages_and_components() {
        let tmp = tmpdir("list");
        make_ws(&tmp);
        std::fs::create_dir_all(tmp.join("ui/showcase/components")).unwrap();
        std::fs::write(
            tmp.join("ui/showcase/components/my-card.html"),
            "<div>x</div>",
        )
        .unwrap();
        let pkgs = list_pkgs(&tmp).unwrap();
        assert_eq!(pkgs.len(), 1);
        assert_eq!(pkgs[0].name, "showcase");
        assert_eq!(pkgs[0].pages, 1);
        assert_eq!(pkgs[0].components, 1);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn new_pkg_creates_page_and_registers() {
        let tmp = tmpdir("new");
        make_ws(&tmp);
        let s = new_pkg(&tmp, "battle").unwrap();
        assert_eq!(s.pages, 1);
        assert!(tmp.join("ui/battle/main.html").exists());
        // 注册进 workspace。
        let ws = load_workspace(&tmp).unwrap();
        assert!(ws.packages.iter().any(|p| p.name == "battle"));
        // 重名拒绝（exit 1）。
        let err = new_pkg(&tmp, "battle").unwrap_err();
        assert_eq!(err.exit_code, 1);
        // new 出的包 check 必绿（最小合法页承诺）。
        let outcome = crate::build::analyze(&tmp).expect("init+new => check clean");
        assert!(outcome.packages.iter().any(|(n, _)| n == "battle"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn add_font_copies_and_registers() {
        let tmp = tmpdir("font");
        make_ws(&tmp);
        let src = tmp.join("NotoStub.ttf");
        std::fs::write(&src, b"stub font").unwrap();
        let s = add_font(&tmp, &src, "NotoSansSC", true, false).unwrap();
        assert_eq!(s.file, "fonts/NotoStub.ttf");
        assert!(tmp.join("fonts/NotoStub.ttf").exists());
        let ws = load_workspace(&tmp).unwrap();
        assert_eq!(ws.fonts.len(), 1);
        assert!(ws.fonts[0].default);
        // family 冲突拒绝。
        let err = add_font(&tmp, &src, "NotoSansSC", false, false).unwrap_err();
        assert_eq!(err.exit_code, 1);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn add_font_already_in_fonts_dir_registers_without_copy() {
        let tmp = tmpdir("font_inplace");
        make_ws(&tmp);
        std::fs::create_dir_all(tmp.join("fonts")).unwrap();
        std::fs::write(tmp.join("fonts/WenKai.ttf"), b"stub font").unwrap();
        // 源就是 fonts/ 里的目标文件：跳过拷贝直接注册（同源同目标 copy 在
        // Windows 报共享冲突，形似文件被锁）。
        let s = add_font(&tmp, &tmp.join("fonts/WenKai.ttf"), "WenKai", true, false).unwrap();
        assert_eq!(s.file, "fonts/WenKai.ttf");
        let ws = load_workspace(&tmp).unwrap();
        assert_eq!(ws.fonts.len(), 1);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn add_atlas_registers_and_blocks_overlap() {
        let tmp = tmpdir("atlas");
        make_ws(&tmp);
        std::fs::create_dir_all(tmp.join("assets/icons")).unwrap();
        let s = add_atlas(&tmp, "assets/icons", None, 2048, 4, false).unwrap();
        assert_eq!(s.name, "icons", "默认名取目录末段");
        // dir 重叠拒绝（会导致 SpriteAtlasConflict）。
        let err = add_atlas(
            &tmp,
            "assets/icons",
            Some("other".to_string()),
            2048,
            4,
            false,
        )
        .unwrap_err();
        assert_eq!(err.exit_code, 1);
        assert!(err.message.contains("already scanned"));
        // 不存在的目录 = 用法/配置错。
        let err = add_atlas(&tmp, "no/such/dir", None, 2048, 4, false).unwrap_err();
        assert_eq!(err.exit_code, 2);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn show_pkg_lists_pages_and_components() {
        let tmp = tmpdir("show");
        make_ws(&tmp);
        std::fs::create_dir_all(tmp.join("ui/showcase/components")).unwrap();
        std::fs::write(
            tmp.join("ui/showcase/components/hero-card.html"),
            "<div>x</div>",
        )
        .unwrap();
        let d = show_pkg(&tmp, "showcase").unwrap();
        assert_eq!(d.pages, vec!["ui/showcase/home.html".to_string()]);
        assert_eq!(d.components, vec!["hero-card".to_string()]);
        // 未知包 = config 错。
        let err = show_pkg(&tmp, "ghost").unwrap_err();
        assert_eq!(err.exit_code, 2);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
