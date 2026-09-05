//! `.yio/config.json`：工作区接线 —— 会话根目录指向 UI 工作区与 Unity 工程的指针。
//!
//! 分离拓扑（会话根 ≠ ui 目录）下，会话根的 `.yio/` 是接线枢纽：CLI 自拷贝、
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

pub const CONFIG_FILE: &str = ".yio/config.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct YioConfig {
    pub ui_root: String,
    pub unity_root: Option<String>,
}

/// 工作区定位结果：会话根（`.yio/` 所在）+ ui 工作区（`yio.workspace.json` 所在）。
/// 单目录形态下两者相同。
#[derive(Debug)]
pub struct Located {
    pub root: PathBuf,
    pub ui: PathBuf,
    pub config: Option<YioConfig>,
}

/// 从任意起点定位工作区，认三种摆放：
///
/// 1. 起点即 ui 工作区（含 `yio.workspace.json`）——显式传 ui 路径，或单目录形态；
/// 2. 起点是会话根（`<起点>/.yio/config.json`）——分离形态下从仓库根裸跑命令；
/// 3. 起点的上一级是会话根——在 `ui/` 目录里直接跑命令。
///
/// 都不命中 → 工具性失败（exit 2），报三步排查指引。
pub fn locate(start: &Path) -> Result<Located, BuildFailure> {
    let start = std::path::absolute(start)
        .map_err(|e| BuildFailure::config(format!("resolve {}: {e}", start.display())))?;

    // 形态 1：起点即 ui 工作区。config 就近找（本目录 .yio = 单目录形态；
    // 上一级 .yio = 分离形态）；找不到（无 config 的裸工作区）→ 本地输出形态。
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
                     修正 {}/{} 或重新 `yio init`",
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
            "{} 不是 Yio 工作区：该目录与其上一级都没有 {}，目录里也没有 {}。\
             在会话根（含 .yio/ 的目录）或 ui 目录里运行，或先 `yio init`。",
            start.display(),
            CONFIG_FILE,
            WORKSPACE_FILE
        ))),
    }
}

/// 就近找 config：先 `<dir>/.yio/config.json`，再 `<dir>/../.yio/config.json`。
/// 返回 config 所在的根目录（解析指针的基准）。文件存在但解析失败 → exit 2。
fn find_config(dir: &Path) -> Result<Option<(PathBuf, YioConfig)>, BuildFailure> {
    let mut bases = [Some(dir), dir.parent()].into_iter().flatten();
    for base in bases.by_ref() {
        let path = base.join(CONFIG_FILE);
        if !path.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| BuildFailure::config(format!("read {}: {e}", path.display())))?;
        let cfg: YioConfig = serde_json::from_str(&text).map_err(|e| {
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

/// Unity 侧运行时包名（`packages-lock.json` 里做版本漂移比对的条目）。
pub const UNITY_PACKAGE: &str = "com.yio.unity";

/// lock 文件里单个包条目的最小投影（其余字段忽略）。
#[derive(Deserialize)]
struct LockEntry {
    version: String,
}

#[derive(Deserialize)]
struct PackagesLock {
    dependencies: std::collections::HashMap<String, LockEntry>,
}

/// UPM 包 manifest 的最小投影。
#[derive(Deserialize)]
struct PackageManifest {
    version: String,
}

/// 工作区 CLI 与 Unity 包版本漂移告警（check 的 warning 级信号）：顺 config 的
/// `unity_root` 读 `<unity>/Packages/packages-lock.json` 里 [`UNITY_PACKAGE`] 的
/// 版本，与本 CLI 版本比对，双向漂移都报。lock 记的是 `file:` 本地引用时（仓库
/// dogfood / 本地装包形态），版本号在被引目录的 package.json 里，顺路径解析。
/// 跳过条件全是环境态而非漂移：无 config / 无 `unity_root`（本地模式）/ unity
/// 目录失联 / 无 lock / lock 无条目 / `file:` 目标解析失败——那是安装期状态，
/// 不是版本漂移，不告警。
pub fn version_drift_warning(ui_workspace: &Path) -> Option<String> {
    let (base, cfg) = find_config(ui_workspace).ok()??;
    let unity_stored = cfg.unity_root?;
    let root = resolve_pointer(&base, &unity_stored);
    if !root.is_dir() {
        return None;
    }
    let lock_text =
        std::fs::read_to_string(root.join("Packages").join("packages-lock.json")).ok()?;
    let lock: PackagesLock = serde_json::from_str(&lock_text).ok()?;
    let entry = lock.dependencies.get(UNITY_PACKAGE)?;
    let pkg_version = extract_package_version(&entry.version, &root)?;
    let cli_version = crate::scaffold::YIO_VERSION;
    Some(match compare_versions(&pkg_version, cli_version) {
        // 数值相等（含 `0.0.17.0` vs `0.0.17` 这类零填充形态）= 无漂移。
        Some(std::cmp::Ordering::Equal) => return None,
        Some(std::cmp::Ordering::Greater) => format!(
            "Unity package {UNITY_PACKAGE} {pkg_version} is newer than this yio CLI \
             {cli_version} — the workspace CLI is behind the Unity side; copy the matching \
             yio(.exe) over .yio/ and run `yio scaffold`"
        ),
        Some(std::cmp::Ordering::Less) => format!(
            "Unity package {UNITY_PACKAGE} {pkg_version} is older than this yio CLI \
             {cli_version} — a newer CLI can write pkg.bin the older Unity plugin cannot \
             read (format_version only grows); update the Unity package to match (reinstall \
             from the newer tag)"
        ),
        None => format!(
            "Unity package {UNITY_PACKAGE} version {pkg_version} differs from this yio CLI \
             {cli_version}; align both sides (copy the matching exe / update the Unity package)"
        ),
    })
}

/// 从 lock 条目的 `version` 字段取可比对的语义版本。lock 记录三种形态：
/// - 裸 semver（registry / 本地缓存安装）——原样；
/// - `file:<相对 Packages 的路径>`（本地装包）——顺路径读目标 package.json；
/// - git URL（`https://…#v0.0.17`）——取 `#` 片段剥 `v` 前缀；片段是裸 commit
///   hash（无 tag 锁定）时离线无从定版 → None（不猜）。
fn extract_package_version(raw: &str, unity_root: &Path) -> Option<String> {
    if let Some(rel) = raw.strip_prefix("file:") {
        let manifest = unity_root.join("Packages").join(rel).join("package.json");
        let m: PackageManifest =
            serde_json::from_str(&std::fs::read_to_string(manifest).ok()?).ok()?;
        return Some(m.version);
    }
    if raw.contains("://") {
        let fragment = raw.rsplit('#').next()?;
        let tag = fragment.strip_prefix('v').unwrap_or(fragment);
        // 裸 commit hash 长度 40 的十六进制，无 tag 语义。
        if tag.len() == 40 && tag.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        return Some(tag.to_string());
    }
    Some(raw.to_string())
}

/// x.y.z 数值逐段比较（段数不同补零，`0.0.17` vs `0.0.17.1` 仍可序）。出现
/// 非数值段（预发布等）→ None：只报「不同」，不猜方向。
fn compare_versions(a: &str, b: &str) -> Option<std::cmp::Ordering> {
    let parse =
        |v: &str| -> Option<Vec<u64>> { v.split('.').map(|s| s.parse::<u64>().ok()).collect() };
    let (a, b) = (parse(a)?, parse(b)?);
    let n = a.len().max(b.len());
    let pad = |mut v: Vec<u64>| {
        v.resize(n, 0);
        v
    };
    Some(pad(a).cmp(&pad(b)))
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
    let cfg = YioConfig {
        ui_root: ui_stored,
        unity_root: unity_stored,
    };
    let dir = root.join(".yio");
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
        let tmp = std::env::temp_dir().join("yio_config_locate_test");
        let _ = std::fs::remove_dir_all(&tmp);
        // repo 根：.yio/config.json；ui 子目录：workspace.json。
        std::fs::create_dir_all(tmp.join("ui")).unwrap();
        std::fs::create_dir_all(tmp.join(".yio")).unwrap();
        std::fs::write(tmp.join("ui").join(WORKSPACE_FILE), "{}").unwrap();
        write(&tmp, &tmp.join("ui"), None).unwrap();
        let cfg: YioConfig =
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
        std::fs::create_dir_all(solo.join(".yio")).unwrap();
        std::fs::write(solo.join(WORKSPACE_FILE), "{}").unwrap();
        write(&solo, &solo, None).unwrap();
        let loc = locate(&solo).unwrap();
        assert_eq!(loc.root, solo);
        assert_eq!(loc.ui, solo);

        // 都不命中 → exit 2（注意：tmp 内的子目录会合法命中 tmp 的 .yio——上级
        // 发现是特性不是 bug，所以"非工作区"样本必须放在发现范围之外）。
        let nowhere = std::env::temp_dir().join("yio_config_locate_nowhere");
        let _ = std::fs::remove_dir_all(&nowhere);
        std::fs::create_dir_all(&nowhere).unwrap();
        let err = locate(&nowhere).unwrap_err();
        assert_eq!(err.exit_code, 2);

        // config 指向的 ui 失联（无 workspace.json）→ exit 2（同样须在发现范围外）。
        let broken = std::env::temp_dir().join("yio_config_locate_broken");
        let _ = std::fs::remove_dir_all(&broken);
        std::fs::create_dir_all(broken.join(".yio")).unwrap();
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
        let tmp = std::env::temp_dir().join("yio_config_base_test");
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

    /// 版本漂移告警矩阵：三种 lock 状态 × 有无 `unity_root`，外加 `file:` 本地
    /// 引用与目录失联的跳过语义。
    #[test]
    fn version_drift_matrix() {
        let tmp = std::env::temp_dir().join("yio_config_drift_test");
        let _ = std::fs::remove_dir_all(&tmp);
        let ui = tmp.join("ui");
        let unity = tmp.join("unity");
        std::fs::create_dir_all(ui.join(".yio")).unwrap();
        std::fs::create_dir_all(unity.join("Packages")).unwrap();
        std::fs::write(ui.join(WORKSPACE_FILE), "{}").unwrap();

        let set_lock = |entry: Option<&str>| {
            let deps = match entry {
                Some(v) => format!(r#"{{"{UNITY_PACKAGE}": {{"version": "{v}"}}}}"#),
                None => "{}".to_string(),
            };
            std::fs::write(
                unity.join("Packages").join("packages-lock.json"),
                format!(r#"{{"dependencies": {deps}}}"#),
            )
            .unwrap();
        };
        let set_config = |unity_root: bool| {
            let json = if unity_root {
                r#"{ "ui_root": "..", "unity_root": "../unity" }"#
            } else {
                r#"{ "ui_root": ".." }"#
            };
            std::fs::write(ui.join(".yio").join("config.json"), json).unwrap();
        };
        let cli = crate::scaffold::YIO_VERSION;

        // 无 unity_root（本地模式）——即便 lock 有漂移也不检。
        set_config(false);
        set_lock(Some("0.0.1"));
        assert!(version_drift_warning(&ui).is_none());

        set_config(true);
        // lock 缺 com.yio.unity 条目（未装包）→ 安装期状态，不告警。
        set_lock(None);
        assert!(version_drift_warning(&ui).is_none());
        // 无 lock 文件同理。
        let _ = std::fs::remove_file(unity.join("Packages").join("packages-lock.json"));
        assert!(version_drift_warning(&ui).is_none());
        set_lock(Some("0.0.1"));

        // 同版本 → 无告警。
        set_lock(Some(cli));
        assert!(version_drift_warning(&ui).is_none());

        // Unity 包旧于 CLI → 告警，指向 pkg.bin 前向兼容风险。
        set_lock(Some("0.0.1"));
        let msg = version_drift_warning(&ui).expect("older package warns");
        assert!(msg.contains("older than this yio CLI"), "{msg}");

        // Unity 包新于 CLI → 告警，指向刷新 .yio/ 里的 exe。
        set_lock(Some("99.0.0"));
        let msg = version_drift_warning(&ui).expect("newer package warns");
        assert!(msg.contains("newer than this yio CLI"), "{msg}");

        // 非数值版本段 → 只报不同，不猜方向。
        set_lock(Some("0.0.18-rc.1"));
        let msg = version_drift_warning(&ui).expect("odd version still warns");
        assert!(msg.contains("differs from this yio CLI"), "{msg}");

        // file: 本地引用 → 顺路径读目标 package.json 的版本。
        let local_pkg = tmp.join("localpkg");
        std::fs::create_dir_all(&local_pkg).unwrap();
        std::fs::write(
            local_pkg.join("package.json"),
            format!(r#"{{"name": "{UNITY_PACKAGE}", "version": "0.0.1"}}"#),
        )
        .unwrap();
        set_lock(Some("file:../../localpkg"));
        let msg = version_drift_warning(&ui).expect("file: ref resolves target manifest");
        assert!(msg.contains("older than this yio CLI"), "{msg}");
        std::fs::write(
            local_pkg.join("package.json"),
            format!(r#"{{"name": "{UNITY_PACKAGE}", "version": "{cli}"}}"#),
        )
        .unwrap();
        assert!(version_drift_warning(&ui).is_none());
        // file: 目标失联 → 安装期状态，不告警。
        set_lock(Some("file:../../gone"));
        assert!(version_drift_warning(&ui).is_none());

        // git URL 形态（消费侧 git-URL 装包）：版本取 #v 片段。
        set_lock(Some(&format!(
            "https://github.com/15wtyuan/Yio.git?path=/unity/package#v{cli}"
        )));
        assert!(version_drift_warning(&ui).is_none());
        set_lock(Some(
            "https://github.com/15wtyuan/Yio.git?path=/unity/package#v0.0.1",
        ));
        let msg = version_drift_warning(&ui).expect("git-URL ref warns on drift");
        assert!(msg.contains("older than this yio CLI"), "{msg}");
        // 裸 commit hash（无 tag）→ 离线无从定版，不猜。
        set_lock(Some(
            "https://github.com/15wtyuan/Yio.git?path=/unity/package#88b47c5e7076a1b2c3d4e5f60718293a4b5c6d7e",
        ));
        assert!(version_drift_warning(&ui).is_none());

        // unity_root 目录失联 → 跳过（build 路径的 exit 2 语义不在此重复）。
        std::fs::write(
            ui.join(".yio").join("config.json"),
            r#"{ "ui_root": "..", "unity_root": "../gone" }"#,
        )
        .unwrap();
        assert!(version_drift_warning(&ui).is_none());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn compare_versions_segments() {
        use std::cmp::Ordering;
        assert_eq!(compare_versions("0.0.17", "0.0.17"), Some(Ordering::Equal));
        assert_eq!(compare_versions("0.0.17", "0.1.0"), Some(Ordering::Less));
        assert_eq!(compare_versions("1.0", "0.99.99"), Some(Ordering::Greater));
        // 段数不同补零：0.0.17.1 > 0.0.17。
        assert_eq!(
            compare_versions("0.0.17.1", "0.0.17"),
            Some(Ordering::Greater)
        );
        // 预发布段非数值 → 无法定向。
        assert_eq!(compare_versions("0.0.18-rc.1", "0.0.18"), None);
    }
}
