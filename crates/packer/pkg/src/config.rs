//! `.ikat/config.json`：工作区接线 —— 会话根目录指向 UI 工作区与 Unity 工程的指针。
//!
//! 分离拓扑（会话根 ≠ ui 目录）下，会话根的 `.ikat/` 是接线枢纽：CLI 自拷贝、
//! `ui_root`、`unity_root` 同住一处，整个目录入库（团队 clone 即得配套 CLI）。
//! 指针优先相对根目录落盘（可入库、团队通用）；跨盘符等无法相对化时落绝对路径
//! （机器绑定）。`ui_root = "."` 是单目录形态（根即 ui 工作区）。
//!
//! `output_dir` 语义随基座：config 带 `unity_root` → 相对 unity 根解析（写
//! `Assets/Bundles` 即直达 Unity 工程）；无 → 相对 ui 工作区（本地输出）。

use crate::diag::BuildFailure;
use crate::workspace::WORKSPACE_FILE;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

pub const CONFIG_FILE: &str = ".ikat/config.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IkatConfig {
    pub ui_root: String,
    pub unity_root: Option<String>,
}

/// 工作区定位结果：会话根（`.ikat/` 所在）+ ui 工作区（`ikat.workspace.json` 所在）。
/// 单目录形态下两者相同。
#[derive(Debug)]
pub struct Located {
    pub root: PathBuf,
    pub ui: PathBuf,
    pub config: Option<IkatConfig>,
}

/// 从任意起点定位工作区，认三种摆放：
///
/// 1. 起点即 ui 工作区（含 `ikat.workspace.json`）——显式传 ui 路径，或单目录形态；
/// 2. 起点是会话根（`<起点>/.ikat/config.json`）——分离形态下从仓库根裸跑命令；
/// 3. 起点的上一级是会话根——在 `ui/` 目录里直接跑命令。
///
/// 都不命中 → 工具性失败（exit 2），报三步排查指引。
pub fn locate(start: &Path) -> Result<Located, BuildFailure> {
    let start = std::path::absolute(start)
        .map_err(|e| BuildFailure::config(format!("resolve {}: {e}", start.display())))?;

    // 形态 1：起点即 ui 工作区。config 就近找（本目录 .ikat = 单目录形态；
    // 上一级 .ikat = 分离形态）；找不到（无 config 的裸工作区）→ 本地输出形态。
    if start.join(WORKSPACE_FILE).is_file() {
        let (root, config) = match find_config(&start)? {
            Some((base, cfg)) => (base, Some(cfg)),
            None => (start.clone(), None),
        };
        return Ok(Located {
            root,
            ui: start,
            config,
        });
    }

    // 形态 2/3：起点是会话根或 ui 目录本体。
    match find_config(&start)? {
        Some((base, cfg)) => {
            let ui = resolve_pointer(&base, &cfg.ui_root);
            if !ui.join(WORKSPACE_FILE).is_file() {
                return Err(BuildFailure::config(format!(
                    "{}: ui_root `{}` 指向的目录没有 {}（ui 目录挪位或 config 失配）；\
                     修正 {}/{} 或重新 `ikat init`",
                    base.join(CONFIG_FILE).display(),
                    cfg.ui_root,
                    WORKSPACE_FILE,
                    base.display(),
                    CONFIG_FILE
                )));
            }
            Ok(Located {
                root: base,
                ui,
                config: Some(cfg),
            })
        }
        None => Err(BuildFailure::config(format!(
            "{} 不是 Ikat 工作区：该目录与其上一级都没有 {}，目录里也没有 {}。\
             在会话根（含 .ikat/ 的目录）或 ui 目录里运行，或先 `ikat init`。",
            start.display(),
            CONFIG_FILE,
            WORKSPACE_FILE
        ))),
    }
}

/// 就近找 config：先 `<dir>/.ikat/config.json`，再 `<dir>/../.ikat/config.json`。
/// 返回 config 所在的根目录（解析指针的基准）。文件存在但解析失败 → exit 2。
fn find_config(dir: &Path) -> Result<Option<(PathBuf, IkatConfig)>, BuildFailure> {
    let mut bases = [Some(dir), dir.parent()].into_iter().flatten();
    for base in bases.by_ref() {
        let path = base.join(CONFIG_FILE);
        if !path.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| BuildFailure::config(format!("read {}: {e}", path.display())))?;
        let cfg: IkatConfig = serde_json::from_str(&text).map_err(|e| {
            BuildFailure::config(format!(
                "parse {} (expect `ui_root` field): {e}",
                path.display()
            ))
        })?;
        return Ok(Some((base.to_path_buf(), cfg)));
    }
    Ok(None)
}

/// 解析产物输出基座：就近找 config，无 / 无 `unity_root` → `None`（本地模式，合法
/// 形态）；有 → 校验后返回目录。config 存在但 `unity_root` 指向的目录不存在 →
/// 工具性失败（exit 2），不静默 fallback 本地——用户显然期望产物进 Unity，静默
/// 改道会把问题藏到 Unity 里找不到产物时才发现。
pub fn resolve_output_base(ui_workspace: &Path) -> Result<Option<PathBuf>, BuildFailure> {
    let Some((base, cfg)) = find_config(ui_workspace)? else {
        return Ok(None);
    };
    let Some(unity_root) = cfg.unity_root else {
        return Ok(None);
    };
    let root = resolve_pointer(&base, &unity_root);
    if !root.is_dir() {
        return Err(BuildFailure::config(format!(
            "{}: unity_root `{}` 指向的目录不存在（Unity 工程挪位或本机无此工程）；\
             修正该文件或重开 GUI 打包器重建工作区",
            base.join(CONFIG_FILE).display(),
            unity_root
        )));
    }
    Ok(Some(root))
}

/// 指针解析：绝对路径原样，相对路径基于 config 所在根目录。
fn resolve_pointer(base: &Path, stored: &str) -> PathBuf {
    let p = Path::new(stored);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    }
}

/// 写 config（覆盖式——重复 init / GUI 重建即刷新）。指针优先相对化（相对根目录，
/// 正斜杠）；跨盘符等无法相对化时写绝对原样（机器绑定）。`ui == root` 时 ui_root
/// 落 `"."`（单目录形态的规范形态）。
pub fn write(root: &Path, ui: &Path, unity_root: Option<&Path>) -> Result<(), String> {
    let ui_stored = relativize(root, ui)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|| ui.to_string_lossy().into_owned());
    let unity_stored = unity_root.map(|u| {
        relativize(root, u)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|| u.to_string_lossy().into_owned())
    });
    let cfg = IkatConfig {
        ui_root: ui_stored,
        unity_root: unity_stored,
    };
    let dir = root.join(".ikat");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let path = root.join(CONFIG_FILE);
    let mut text = serde_json::to_string_pretty(&cfg).map_err(|e| format!("serialize: {e}"))?;
    text.push('\n'); // 尾换行：文本文件常规收尾，防重写伪 diff（同 save_workspace）
    std::fs::write(&path, text).map_err(|e| format!("write {}: {e}", path.display()))
}

/// 纯词法相对化（from 目录 → to 路径的相对表示）。基不同（一方绝对一方相对）或盘符
/// 不同（跨盘）→ None——相对化无意义，调用方存绝对原样。
fn relativize(from: &Path, to: &Path) -> Option<PathBuf> {
    if to.is_absolute() != from.is_absolute() {
        return None;
    }
    let mut fc = from.components().peekable();
    let mut tc = to.components().peekable();
    if let (Some(Component::Prefix(a)), Some(Component::Prefix(b))) = (fc.peek(), tc.peek()) {
        if a != b {
            return None;
        }
    }
    loop {
        match (fc.peek(), tc.peek()) {
            (Some(a), Some(b)) if a == b => {
                fc.next();
                tc.next();
            }
            _ => break,
        }
    }
    let mut out = PathBuf::new();
    for _ in 0..fc.count() {
        out.push("..");
    }
    for c in tc {
        out.push(c.as_os_str());
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 相对化契约：同盘上溯/下探、跨盘 None、同目录 "."。
    #[test]
    fn relativize_basics() {
        assert_eq!(
            relativize(Path::new("F:/proj"), Path::new("F:/proj/ui")).unwrap(),
            Path::new("ui")
        );
        assert_eq!(
            relativize(Path::new("F:/proj/ui"), Path::new("F:/proj/unity")).unwrap(),
            Path::new("../unity")
        );
        assert_eq!(
            relativize(Path::new("F:/a/b/c"), Path::new("F:/a/d")).unwrap(),
            Path::new("../../d")
        );
        assert_eq!(
            relativize(Path::new("F:/proj"), Path::new("F:/proj")).unwrap(),
            Path::new(".")
        );
        // 跨盘 None 是 Windows 盘符前缀语义；其他平台 `C:`/`D:` 只是普通目录名
        // （无 Prefix 组件，词法相对化仍可算），等价的「基不同 → None」改用
        // 绝对/相对混用验证。
        #[cfg(windows)]
        assert!(relativize(Path::new("C:/ui"), Path::new("D:/unity")).is_none());
        #[cfg(not(windows))]
        assert!(relativize(Path::new("/ui"), Path::new("rel")).is_none());
    }

    /// locate 三形态 + 失败路径。
    #[test]
    fn locate_three_layouts() {
        let tmp = std::env::temp_dir().join("ikat_config_locate_test");
        let _ = std::fs::remove_dir_all(&tmp);
        // repo 根：.ikat/config.json；ui 子目录：workspace.json。
        std::fs::create_dir_all(tmp.join("ui")).unwrap();
        std::fs::create_dir_all(tmp.join(".ikat")).unwrap();
        std::fs::write(tmp.join("ui").join(WORKSPACE_FILE), "{}").unwrap();
        write(&tmp, &tmp.join("ui"), None).unwrap();
        let cfg: IkatConfig =
            serde_json::from_str(&std::fs::read_to_string(tmp.join(CONFIG_FILE)).unwrap()).unwrap();
        assert_eq!(cfg.ui_root, "ui");
        assert_eq!(cfg.unity_root, None);

        // 形态 2：从根定位 → ui。
        let loc = locate(&tmp).unwrap();
        assert_eq!(loc.ui, tmp.join("ui"));
        assert_eq!(loc.root, tmp);
        // 形态 3：从 ui 目录定位 → 同一个 ui。
        let loc = locate(&tmp.join("ui")).unwrap();
        assert_eq!(loc.ui, tmp.join("ui"));
        // 形态 1：显式 ui 路径（等价形态 3，走 workspace.json 分支）。
        assert!(loc.config.is_some());

        // 单目录形态：根即 ui，ui_root = "."。
        let solo = tmp.join("solo");
        std::fs::create_dir_all(solo.join(".ikat")).unwrap();
        std::fs::write(solo.join(WORKSPACE_FILE), "{}").unwrap();
        write(&solo, &solo, None).unwrap();
        let loc = locate(&solo).unwrap();
        assert_eq!(loc.root, solo);
        assert_eq!(loc.ui, solo);

        // 都不命中 → exit 2（注意：tmp 内的子目录会合法命中 tmp 的 .ikat——上级
        // 发现是特性不是 bug，所以"非工作区"样本必须放在发现范围之外）。
        let nowhere = std::env::temp_dir().join("ikat_config_locate_nowhere");
        let _ = std::fs::remove_dir_all(&nowhere);
        std::fs::create_dir_all(&nowhere).unwrap();
        let err = locate(&nowhere).unwrap_err();
        assert_eq!(err.exit_code, 2);

        // config 指向的 ui 失联（无 workspace.json）→ exit 2（同样须在发现范围外）。
        let broken = std::env::temp_dir().join("ikat_config_locate_broken");
        let _ = std::fs::remove_dir_all(&broken);
        std::fs::create_dir_all(broken.join(".ikat")).unwrap();
        std::fs::write(broken.join(CONFIG_FILE), r#"{ "ui_root": "gone" }"#).unwrap();
        let err = locate(&broken).unwrap_err();
        assert_eq!(err.exit_code, 2);

        let _ = std::fs::remove_dir_all(&tmp);
        let _ = std::fs::remove_dir_all(&nowhere);
        let _ = std::fs::remove_dir_all(&broken);
    }

    /// 输出基座三态：无 config → None；带 unity_root → 解析目录；目录失效 → exit 2。
    #[test]
    fn output_base_three_states() {
        let tmp = std::env::temp_dir().join("ikat_config_base_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("ui")).unwrap();
        std::fs::create_dir_all(tmp.join("unity")).unwrap();
        let ui = tmp.join("ui");

        // 1. 无 config（裸 ui 工作区）→ 本地模式。
        std::fs::write(ui.join(WORKSPACE_FILE), "{}").unwrap();
        assert!(resolve_output_base(&ui).unwrap().is_none());

        // 2. 根上 config 带 unity_root → 相对根解析。
        write(&tmp, &ui, Some(&tmp.join("unity"))).unwrap();
        let base = resolve_output_base(&ui).unwrap().expect("base resolved");
        assert_eq!(
            base.canonicalize().unwrap(),
            tmp.join("unity").canonicalize().unwrap()
        );

        // 3. unity_root 指向不存在的目录 → exit 2，不静默 fallback。
        std::fs::write(
            tmp.join(CONFIG_FILE),
            r#"{ "ui_root": "ui", "unity_root": "../gone" }"#,
        )
        .unwrap();
        let err = resolve_output_base(&ui).unwrap_err();
        assert_eq!(err.exit_code, 2);

        // 4. config 存在但无 unity_root → 本地模式（合法形态）。
        std::fs::write(tmp.join(CONFIG_FILE), r#"{ "ui_root": "ui" }"#).unwrap();
        assert!(resolve_output_base(&ui).unwrap().is_none());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
