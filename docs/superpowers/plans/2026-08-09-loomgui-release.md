# LoomGUI 发布实现计划（git URL 分发）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 `com.loomgui.unity` 能通过 git URL 发布给其他 Unity 项目消费，配套版本管理 + `release-check` 自检 + CI 自动出 GitHub Release。

**Architecture:** 以 `unity/package/` 为 UPM 产物，git tag `v<version>` 锁版本；新增 xtask `release-check` 子命令做发布前完整性自检；新增 `release.yml` 在 tag push 时校验 + 跑测试 + 创建 GitHub Release（正文取自 CHANGELOG）。

**Tech Stack:** Rust（xtask + serde_json + semver）、GitHub Actions、Unity UPM。

## Global Constraints

- 包名 `com.loomgui.unity` 不变；首版 `0.0.1`。
- git tag 命名 `v<version>`（如 `v0.0.1`）；版本号唯一真相源 = `unity/package/package.json` 的 `"version"`。
- Windows-only（dll 仅 Windows）。
- Rust edition 2021；xtask 新依赖钉版本：`serde_json = "1"`、`semver = "1"`（workspace lock 已含，不新增传递依赖）。
- 发布产物 = `unity/package/` 目录；`showcase-unity` 不是产物。
- CI 不编 dll——dll 在本地编好并 commit 进 tag 的 commit（git URL 拉 tag 快照要求 dll 已在内）。
- 代码注释上线品质、自包含、说 WHY。

---

## File Structure

| 文件 | 责任 | 动作 |
|---|---|---|
| `unity/package/package.json` | UPM 包元数据 | Modify（version + 字段） |
| `unity/package/CHANGELOG.md` | 版本变更记录 | Create |
| `crates/xtask/Cargo.toml` | xtask 依赖 | Modify（+serde_json, +semver） |
| `crates/xtask/src/paths.rs` | 共享路径辅助 `repo_root()` | Create |
| `crates/xtask/src/bindings.rs` | 用 `paths::repo_root()` | Modify（去重） |
| `crates/xtask/src/release_check.rs` | release-check 逻辑 + 测试 | Create |
| `crates/xtask/src/main.rs` | 子命令分发 | Modify（+release-check） |
| `.github/workflows/release.yml` | tag 触发出 Release | Create |
| `README.md` | 安装说明 | Modify（+安装小节） |

---

## Task 1: 包元数据（package.json 字段补全 + CHANGELOG）

**Files:**
- Modify: `unity/package/package.json`
- Create: `unity/package/CHANGELOG.md`

**Interfaces:**
- Produces: 合规的 `package.json`（version=`0.0.1`，含 `author`/`keywords`/`changelogUrl`/`documentationUrl`/`licensesUrl`）与 `CHANGELOG.md`，供 Task 2/3 的 `release-check` 校验。

- [ ] **Step 1: 改 `unity/package/package.json`**

把整个文件替换为：
```json
{
  "name": "com.loomgui.unity",
  "version": "0.0.1",
  "displayName": "LoomGUI",
  "description": "跨引擎游戏 UI 框架——Rust 核心 + HTML/CSS DSL，Unity 后端",
  "unity": "6000.0",
  "license": "MIT",
  "author": {
    "name": "15wtyuan",
    "url": "https://github.com/15wtyuan"
  },
  "keywords": ["ui", "html", "css", "game-ui", "rust"],
  "changelogUrl": "https://github.com/15wtyuan/LoomGUI/blob/main/unity/package/CHANGELOG.md",
  "documentationUrl": "https://github.com/15wtyuan/LoomGUI/blob/main/docs/design/main-design.md",
  "licensesUrl": "https://github.com/15wtyuan/LoomGUI/blob/main/LICENSE",
  "dependencies": {
    "com.unity.2d.sprite": "1.0.0",
    "com.unity.inputsystem": "1.19.0",
    "com.unity.render-pipelines.universal": "17.5.0"
  }
}
```

- [ ] **Step 2: 创建 `unity/package/CHANGELOG.md`**

```markdown
# Changelog

All notable changes to `com.loomgui.unity` will be documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.0.1] - 2026-08-09
### Added
- 首个可安装 UPM 包。骨架链（div + 文字 + 图 + flex + cascade）从 HTML/CSS 一路通到 Unity 真机渲染。
- Runtime 公共 API 表面（Node/Container/Button/... 类型化投影层）。
- 围栏验证器（标准 HTML/CSS 子集，打包期报错）。
```

- [ ] **Step 3: 验证 package.json 是合法 JSON**

Run: `python -m json.tool unity/package/package.json > /dev/null && echo OK`
Expected: 输出 `OK`（无 JSON 语法错误）。

- [ ] **Step 4: Commit**

```bash
git add unity/package/package.json unity/package/CHANGELOG.md
git commit -m "feat(release): set version 0.0.1, add package metadata and CHANGELOG"
```

---

## Task 2: release-check — 包内元数据校验（TDD）

**Files:**
- Modify: `crates/xtask/Cargo.toml`
- Create: `crates/xtask/src/paths.rs`
- Modify: `crates/xtask/src/bindings.rs`
- Create: `crates/xtask/src/release_check.rs`
- Modify: `crates/xtask/src/main.rs`

**Interfaces:**
- Consumes: Task 1 产出的 `unity/package/package.json`（version `0.0.1`）与 `CHANGELOG.md`。
- Produces:
  - `paths::repo_root() -> PathBuf`
  - `release_check::run_release_check() -> Result<(), Box<dyn std::error::Error>>`
  - 纯函数 `release_check::parse_and_validate_package(content: &str) -> Result<PackageMeta, CheckError>`
  - 纯函数 `release_check::changelog_has_version(content: &str, version: &str) -> bool`
  - 类型 `release_check::PackageMeta { version: String }`、`release_check::CheckError`

- [ ] **Step 1: 加 xtask 依赖**

Modify `crates/xtask/Cargo.toml`，在 `[dependencies]` 下 `csbindgen = "1"` 之后加两行：
```toml
serde_json = "1"
semver = "1"
```

- [ ] **Step 2: 提取共享 `repo_root()` 到 `paths.rs`**

Create `crates/xtask/src/paths.rs`：
```rust
//! 共享路径辅助。路径推算以 xtask 的 CARGO_MANIFEST_DIR (= crates/xtask) 为基准。

use std::path::PathBuf;

/// 仓库根目录。
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}
```

Modify `crates/xtask/src/bindings.rs`：删除文件内的 `fn repo_root()`（约第 12-16 行），并在文件顶部 `use` 之后加：
```rust
use crate::paths;
```
把 `sync_bindings()` 内对 `repo_root()` 的调用改为 `paths::repo_root()`。

- [ ] **Step 3: 写 `parse_and_validate_package` 失败测试**

Create `crates/xtask/src/release_check.rs`，先只放测试与类型骨架（函数体 `unimplemented!()`）。
> 注意：Task 2 的纯函数不使用 `Path`，故本步不引入 `use std::path::Path;`——延后到 Step 11 `run_release_check` 首次使用时再加，避免 unused-import warning（项目 clippy 严门）。
```rust
//! release-check 子命令：发布前包完整性自检。

/// 解析后的包元数据（只取发布相关字段）。
#[derive(Debug, PartialEq, Eq)]
pub struct PackageMeta {
    pub version: String,
}

/// release-check 检出的问题类别。
#[derive(Debug)]
pub enum CheckError {
    MissingField(String),
    InvalidSemver(String),
    DllNotFound,
    DllStale,
    AsmdefMissing(String),
    ChangelogMissingVersion(String),
    Io(String),
}

impl std::fmt::Display for CheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingField(n) => write!(f, "package.json missing required field: {n}"),
            Self::InvalidSemver(v) => write!(f, "version is not valid SemVer: {v}"),
            Self::DllNotFound => write!(f, "loomgui_ffi_c.dll not found in package"),
            Self::DllStale => write!(f, "committed dll differs from target/release build (forgot to commit?)"),
            Self::AsmdefMissing(n) => write!(f, "asmdef missing: {n}"),
            Self::ChangelogMissingVersion(v) => write!(f, "CHANGELOG.md has no section for version {v}"),
            Self::Io(e) => write!(f, "io error: {e}"),
        }
    }
}
impl std::error::Error for CheckError {}

/// 解析 package.json 内容并校验必填字段 + version 合法性。
pub fn parse_and_validate_package(content: &str) -> Result<PackageMeta, CheckError> {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_package() {
        let s = r#"{"name":"x","version":"0.0.1","unity":"6000.0","displayName":"X"}"#;
        assert_eq!(
            parse_and_validate_package(s).unwrap(),
            PackageMeta { version: "0.0.1".into() }
        );
    }

    #[test]
    fn missing_required_field() {
        let s = r#"{"name":"x","version":"0.0.1"}"#; // 缺 unity、displayName
        assert!(matches!(
            parse_and_validate_package(s),
            Err(CheckError::MissingField(_))
        ));
    }

    #[test]
    fn invalid_semver() {
        let s = r#"{"name":"x","version":"not-a-version","unity":"6000.0","displayName":"X"}"#;
        assert!(matches!(
            parse_and_validate_package(s),
            Err(CheckError::InvalidSemver(_))
        ));
    }
}
```

- [ ] **Step 4: 跑测试确认失败**

Run: `cargo test -p xtask`
Expected: 编译失败或测试失败（`unimplemented!` panic）。

- [ ] **Step 5: 实现 `parse_and_validate_package`**

把 `unimplemented!()` 替换为：
```rust
pub fn parse_and_validate_package(content: &str) -> Result<PackageMeta, CheckError> {
    let v: serde_json::Value =
        serde_json::from_str(content).map_err(|e| CheckError::Io(e.to_string()))?;
    for field in ["name", "version", "unity", "displayName"] {
        let missing = v
            .get(field)
            .map(|x| x.is_null())
            .unwrap_or(true);
        if missing {
            return Err(CheckError::MissingField(field.to_string()));
        }
    }
    let version = v["version"]
        .as_str()
        .ok_or_else(|| CheckError::MissingField("version".to_string()))?;
    semver::Version::parse(version).map_err(|_| CheckError::InvalidSemver(version.into()))?;
    Ok(PackageMeta {
        version: version.to_string(),
    })
}
```

- [ ] **Step 6: 跑测试确认通过**

Run: `cargo test -p xtask`
Expected: 3 个测试全 PASS。

- [ ] **Step 7: 写 `changelog_has_version` 失败测试**

在 `tests` 模块追加：
```rust
    #[test]
    fn changelog_has_matching_section() {
        let s = "## [Unreleased]\n\n## [0.0.1] - 2026-08-09\n- x\n";
        assert!(changelog_has_version(s, "0.0.1"));
    }

    #[test]
    fn changelog_missing_section() {
        let s = "## [Unreleased]\n\n## [0.0.2] - 2026-08-09\n";
        assert!(!changelog_has_version(s, "0.0.1"));
    }
```
并加函数骨架（紧贴 `parse_and_validate_package` 之后）：
```rust
/// CHANGELOG 是否含 `## [<version>]` 段落（Keep a Changelog 格式）。
pub fn changelog_has_version(_content: &str, _version: &str) -> bool {
    unimplemented!()
}
```

- [ ] **Step 8: 跑测试确认失败**

Run: `cargo test -p xtask changelog`
Expected: 2 个新测试 panic。

- [ ] **Step 9: 实现 `changelog_has_version`**

替换骨架为：
```rust
pub fn changelog_has_version(content: &str, version: &str) -> bool {
    let needle = format!("## [{version}]");
    content
        .lines()
        .any(|line| line.trim_start().starts_with(&needle))
}
```

- [ ] **Step 10: 跑测试确认通过**

Run: `cargo test -p xtask`
Expected: 全 PASS。

- [ ] **Step 11: 实现 `run_release_check` 并接入 main**

在 `release_check.rs` 顶部加以下两个 use：
```rust
use std::path::Path;
use crate::paths;
```
在文件末尾追加：
```rust
/// release-check 入口：校验 package.json + CHANGELOG + dll + asmdef。
/// 任意一项失败返回 Err，调用方据此退出非 0。
pub fn run_release_check() -> Result<(), Box<dyn std::error::Error>> {
    let pkg = paths::repo_root().join("unity/package/package.json");
    let meta = parse_and_validate_package(&std::fs::read_to_string(&pkg)?)?;

    let cl = paths::repo_root().join("unity/package/CHANGELOG.md");
    let cl_content = std::fs::read_to_string(&cl)?;
    if !changelog_has_version(&cl_content, &meta.version) {
        return Err(CheckError::ChangelogMissingVersion(meta.version).into());
    }

    // 文件完整性校验（dll / asmdef）见 Task 3 接入。
    let _ = Path::new("");

    println!("release-check: OK (version {})", meta.version);
    Ok(())
}
```

Modify `crates/xtask/src/main.rs`：在 `mod bindings;` 下方加：
```rust
mod paths;
mod release_check;
```
在 usage 提示块加一行 `eprintln!("  release-check  Pre-release package sanity check");`。
在 `match args[0].as_str()` 内 `"sync-bindings" => { ... }` 之后、`other => { ... }` 之前加一个 arm：
```rust
        "release-check" => {
            if let Err(e) = release_check::run_release_check() {
                eprintln!("release-check failed: {e}");
                std::process::exit(1);
            }
        }
```

- [ ] **Step 12: 手动跑 release-check，确认对当前包通过**

Run: `cargo run -p xtask -- release-check`
Expected: 输出 `release-check: OK (version 0.0.1)`，退出码 0。

- [ ] **Step 13: 确认 sync-bindings 仍工作（防 paths 重构回归）**

Run: `cargo run -p xtask -- sync-bindings`
Expected: 输出 `sync-bindings: Unity -> ...LoomGUIBindings.cs`，退出码 0。

- [ ] **Step 14: Commit**

```bash
git add crates/xtask
git commit -m "feat(xtask): add release-check subcommand (package metadata validation)"
```

---

## Task 3: release-check — 文件完整性校验（dll + asmdef，TDD）

**Files:**
- Modify: `crates/xtask/src/release_check.rs`（加文件类校验函数 + 测试 + 接入 `run_release_check`）

**Interfaces:**
- Consumes: Task 2 的 `CheckError`、`run_release_check`、`paths::repo_root`。
- Produces: 完整的 `run_release_check`（含 dll 存在/最新、asmdef 齐全校验）。
  - 纯函数 `dll_status(committed: &Path, built: &Path) -> DllStatus`

- [ ] **Step 1: 加 `DllStatus` 与 `dll_status` 失败测试**

在 `release_check.rs` 的 `CheckError` 之后加枚举：
```rust
/// dll 校验结果。`BuiltMissing` 表示本地无 target 产物（CI 场景），跳过比较。
#[derive(Debug, PartialEq, Eq)]
pub enum DllStatus {
    Ok,
    NotFound,
    BuiltMissing,
    Stale,
}
```
在 `tests` 模块追加（用临时目录，无新依赖）：
```rust
    fn tmp_bytes(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let id = N.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("xtask-rc-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn dll_not_found() {
        let missing = std::env::temp_dir().join("xtask-rc-nope-a");
        let built = std::env::temp_dir().join("xtask-rc-nope-b");
        assert_eq!(dll_status(&missing, &built), DllStatus::NotFound);
    }

    #[test]
    fn dll_built_missing_skips() {
        let committed = tmp_bytes("a.dll", b"AAA");
        let built = std::env::temp_dir().join("xtask-rc-nope-built");
        assert_eq!(dll_status(&committed, &built), DllStatus::BuiltMissing);
    }

    #[test]
    fn dll_stale() {
        let committed = tmp_bytes("a.dll", b"AAA");
        let built = tmp_bytes("b.dll", b"BBB");
        assert_eq!(dll_status(&committed, &built), DllStatus::Stale);
    }

    #[test]
    fn dll_match() {
        let committed = tmp_bytes("a.dll", b"AAA");
        let built = tmp_bytes("b.dll", b"AAA");
        assert_eq!(dll_status(&committed, &built), DllStatus::Ok);
    }
```
加函数骨架（紧贴 `changelog_has_version` 之后）：
```rust
/// 比较「入库 dll」与「本地 target 产物 dll」。
pub fn dll_status(_committed: &Path, _built: &Path) -> DllStatus {
    unimplemented!()
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p xtask dll_`
Expected: 4 个新测试 panic。

- [ ] **Step 3: 实现 `dll_status`**

替换骨架为：
```rust
pub fn dll_status(committed: &Path, built: &Path) -> DllStatus {
    if !committed.exists() {
        return DllStatus::NotFound;
    }
    if !built.exists() {
        return DllStatus::BuiltMissing;
    }
    match (std::fs::read(committed), std::fs::read(built)) {
        (Ok(a), Ok(b)) if a == b => DllStatus::Ok,
        (Ok(_), Ok(_)) => DllStatus::Stale,
        _ => DllStatus::NotFound,
    }
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p xtask`
Expected: 全 PASS。

- [ ] **Step 5: 加 asmdef 校验**

在 `dll_status` 之后加：
```rust
/// 校验三个 asmdef 齐全。任一缺失返回 `AsmdefMissing`。
pub fn check_asmdef_present(pkg_dir: &Path) -> Result<(), CheckError> {
    let expected = [
        "LoomGUI.Runtime.asmdef",
        "Editor/LoomGUI.Editor.asmdef",
        "Plugins/LoomGUI/LoomGUI.Bindings.asmdef",
    ];
    for rel in expected {
        let p = pkg_dir.join(rel);
        if !p.exists() {
            return Err(CheckError::AsmdefMissing(rel.to_string()));
        }
    }
    Ok(())
}
```

- [ ] **Step 6: 接入 `run_release_check`**

把 `run_release_check` 中的占位行 `let _ = Path::new("");` 替换为：
```rust
    // dll：入库必须存在；若本地有 target/release 产物则比对一致性（防编了忘 commit）。
    let pkg_dir = paths::repo_root().join("unity/package");
    let committed_dll = pkg_dir.join("Plugins/LoomGUI/loomgui_ffi_c.dll");
    let built_dll = paths::repo_root().join("target/release/loomgui_ffi_c.dll");
    match dll_status(&committed_dll, &built_dll) {
        DllStatus::NotFound => return Err(CheckError::DllNotFound.into()),
        DllStatus::Stale => return Err(CheckError::DllStale.into()),
        DllStatus::Ok | DllStatus::BuiltMissing => {}
    }

    check_asmdef_present(&pkg_dir)?;
```

- [ ] **Step 7: 手动跑 release-check，确认仍通过**

Run: `cargo run -p xtask -- release-check`
Expected: 输出 `release-check: OK (version 0.0.1)`，退出码 0（dll 入库存在；target 是否有产物不影响通过）。

- [ ] **Step 8: （可选）验证 DllStale 检出**

临时改 `committed_dll` 内容（如追加一个字节）再跑，确认报 `release-check failed: ... dll differs`；验证后 `git checkout` 还原。不强制执行，记此验收点即可。

- [ ] **Step 9: Commit**

```bash
git add crates/xtask/src/release_check.rs
git commit -m "feat(xtask): release-check validates dll + asmdef integrity"
```

---

## Task 4: release.yml CI workflow

**Files:**
- Create: `.github/workflows/release.yml`

**Interfaces:**
- Consumes: Task 2/3 的 `release-check`、Task 1 的 `package.json`/`CHANGELOG.md`。

> 本地无法跑 GitHub Actions；验收 = 推一个临时 tag（如先在 feature 分支或 `v0.0.1-rc1`）观察 CI，或合并后首个正式 tag 验证。

- [ ] **Step 1: 创建 `.github/workflows/release.yml`**

```yaml
name: Release

on:
  push:
    tags: ['v*']

permissions:
  contents: write   # 创建 GitHub Release 需要

env:
  CARGO_TERM_COLOR: always

jobs:
  release:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2

      - name: verify tag matches package version
        shell: bash
        run: |
          TAG="${GITHUB_REF#refs/tags/}"
          PKG_VER=$(jq -r .version unity/package/package.json)
          echo "tag=$TAG pkg_version=$PKG_VER"
          if [ "v$PKG_VER" != "$TAG" ]; then
            echo "ERROR: tag $TAG != package version v$PKG_VER"
            exit 1
          fi

      - name: cargo test --workspace (exclude GUI; release 不依赖 Tauri)
        run: cargo test --workspace --exclude loomgui_gui

      - name: release-check
        run: cargo run -p xtask -- release-check

      - name: extract changelog section for this version
        id: cl
        shell: bash
        run: |
          PKG_VER=$(jq -r .version unity/package/package.json)
          # 截取 `## [<ver>]` 到下一个 `## [` 之间（含标题行）。
          BODY=$(awk -v v="$PKG_VER" '
            $0 ~ "^## \\[" v "\\]"     { p=1 }
            /^## \[/ && p && $0 !~ "^## \\[" v "\\]" { p=0 }
            p { print }
          ' unity/package/CHANGELOG.md)
          {
            echo "BODY<<EOF"
            echo "$BODY"
            echo "EOF"
          } >> "$GITHUB_OUTPUT"

      - name: create github release
        uses: softprops/action-gh-release@v2
        with:
          body: ${{ steps.cl.outputs.BODY }}
          prerelease: ${{ startsWith(github.ref_name, 'v0.') }}
```

- [ ] **Step 2: 本地 YAML 语法快检**

Run: `python -c "import yaml,sys; yaml.safe_load(open('.github/workflows/release.yml')); print('OK')"`
Expected: 输出 `OK`。（若环境无 PyYAML，跳过此步，靠 push 验证。）

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: add release.yml (tag-triggered release with release-check gate)"
```

---

## Task 5: README 安装小节

**Files:**
- Modify: `README.md`

- [ ] **Step 1: 在「快速开始」的 Unity 后端行之后插入安装小节**

锚点行（README 中已存在）：
```
Unity 后端：Unity 6.5 打开 `unity/showcase-unity/`，PlayMode 加载 `.pkg.bin`。
```
在该行之后、「## 文档」标题之前，插入：
```markdown

## 在其他 Unity 项目中使用

LoomGUI 以 UPM 包发布，通过 git URL 安装。在目标工程的 `Packages/manifest.json` 加一行：

```json
"com.loomgui.unity": "https://github.com/15wtyuan/LoomGUI.git?path=/unity/package#v0.0.1"
```

升级：把 `#v0.0.1` 改成目标 tag，或在 Unity 的 Package Manager 窗口选新版本。

> 当前仅 Windows（native dll 仅含 Windows）。发版流程与多平台计划见 [发布设计](docs/superpowers/specs/2026-08-09-loomgui-release-design.md)。
```

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs: add installation section for consuming LoomGUI via git URL"
```

---

## End-to-End 验收（全任务完成后）

1. `cargo run -p xtask -- release-check` 本地通过。
2. `git tag v0.0.1 && git push --tags`，GitHub Actions `Release` 跑绿，Releases 页出现 `v0.0.1`，正文 = CHANGELOG `[0.0.1]` 段落。
3. 在一个干净 Unity 工程的 `manifest.json` 加 git URL 行，能导入 `com.loomgui.unity`，dll 加载正常，`LoomGUI > Open Packer` 菜单可见。
4. （回归）改入库 dll 内容后 `release-check` 报 `dll differs`。

对应 spec 验收标准的 1/2/3 条均由以上覆盖。
