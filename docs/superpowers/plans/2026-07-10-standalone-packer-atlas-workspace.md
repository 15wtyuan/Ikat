# 独立打包器 + 自绘图集 + 工作区重构 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把打包链从「Unity 编辑器绑死」重构成「跨平台独立打包器 + Rust 自绘图集 + 自包含工作区」，设计期与运行期只靠产物目录通信。

**Architecture:** `loomgui_pkg` 保持 Rust 复用 `loomgui_core`（parse/cascade/build_scene/围栏零漂移），新增 atlas/workspace/build 模块 + 零参 CLI。新增 `loomgui_gui`（Tauri 2.x + 原生前端）link 同一 lib。运行期后端读产物目录的 `loom.runtime.json` 自举，加载自绘 atlas.png（普通 Texture2D）+ 查 atlas.json 的 UV 表。pkg.bin 删除 manifest 段，图尺寸真相源改为 atlas.json（含运行时动态图标），经 `set_image_sizes` FFI 批量灌入 core。

**Tech Stack:** Rust 2021 · `image`（PNG 编解码，仅 pkg crate）· `etagere 0.3`（shelf 打包，core 已有）· `serde`/`serde_json`（workspace/atlas/runtime JSON）· `dirs`（跨平台用户目录）· Tauri 2.x + 原生 HTML/CSS/JS · Unity C#（消费侧）。

## Global Constraints

以下为 spec 的项目级约束，**每个任务都隐含遵守**：

- **依赖钉版本**：`taffy 0.5`、`ttf-parser 0.20`、`cssparser 0.34`、`scraper 0.19`、`slotmap 1.1`、`csbindgen 1`、`etagere 0.3`。新增 `image`（PNG 编解码）、`serde`/`serde_json`、`dirs` 只进 `loomgui_pkg`/`loomgui_gui`，**不进 `loomgui_core` runtime**。
- **`image` crate 仅 pkg/gui**：core 不得引入 PNG 解码依赖（保持引擎无关纯核心）。
- **全路径相对工作区根 + 正斜杠**：workspace.json / runtime.json / atlas.json 里所有路径都相对工作区根（或产物目录），用 `/` 分隔，跨平台可移植。
- **img src 相对 html 文件**：打包器把 `<img src>` 相对其 html 文件解析成图相对工作区根的路径，即全局唯一 `sprite_key`。
- **围栏真相源不动**：围栏逻辑仍在 `loomgui_core`，`fence_contract.rs` 是唯一真相源。本重构不改围栏语义。
- **FFI 不 panic**：cdylib 入口 `.expect`/`unwrap` 遇 None 会 abort 拖垮 Unity。新增 FFI 入口状态非法时优雅早返。
- **FFI enum `#[repr(uN)]`** + `size_of` 断言 ABI struct 尺寸。
- **改 Rust 后必重编 + commit `.dll`**：`cargo build -p loomgui_ffi_c --release` → 拷到 `loomgui_unity_package/Plugins/LoomGUI/loomgui_ffi_c.dll`（Unity 关闭时拷）。
- **改 parse-time 逻辑必重打 pkg**：`Node.base_style` 是打包期产物；改 cascade/mapping/parse 后须重打 pkg。
- **push 前本地跑**：`cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings`，否则 CI 红。
- **注释上线品质**：自包含、说 WHY、不引用内部编号/暗语（坑号只属 `docs/pitfalls.md`）。
- **用户只读中文**：问答/总结用中文；代码/commit 英文。

## 阶段总览

1. **阶段一 Rust 引擎层**（Task 1-9）：workspace.json 读写 + atlas 打包 + build 编排 + runtime.json + 零参 CLI。产物能出。
2. **阶段二 运行时消费**（Task 10-16）：pkg.bin 删 manifest 段（v12→v13）+ core `set_image_sizes` FFI + Unity `SpriteResolver` 重写 + 废 `LoomSettings`。闭环。
3. **阶段三 Tauri GUI**（Task 17-22）：GUI 壳 + Tauri commands + 拖拽 + 最近工作区。
4. **阶段四 Unity 拆除**（Task 23-24）：删旧编辑器脚本，留「Open Packer」MenuItem。
5. **阶段五 文档防漂移**（Task 25-27）：main-design / CLAUDE.md / 工作区 skill / roadmap / 记忆同步。

---

# 阶段一：Rust 引擎层

## Task 1: workspace.json 数据模型 + 反序列化

**Files:**
- Create: `loomgui_pkg/src/workspace.rs`
- Modify: `loomgui_pkg/Cargo.toml`（加 serde/serde_json 依赖）
- Modify: `loomgui_pkg/src/lib.rs`（`pub mod workspace;`）
- Test: 同文件 `#[cfg(test)]`

**Interfaces:**
- Produces:
  - `struct Workspace { version: u32, output_dir: String, packages: Vec<PackageCfg>, atlases: Vec<AtlasCfg>, fonts: Vec<FontCfg> }`
  - `struct PackageCfg { name: String, dirs: Vec<String>, html: Vec<String> }`（`html` 空 = 自动态）
  - `struct AtlasCfg { name: String, default: bool, standalone: bool, dirs: Vec<String>, max_size: u32, padding: u32 }`
  - `struct FontCfg { family: String, file: String, default: bool, fallback: bool }`
  - `fn load_workspace(root: &Path) -> Result<Workspace, String>`（读 `root/loom.workspace.json`）
  - `fn save_workspace(root: &Path, ws: &Workspace) -> Result<(), String>`

- [ ] **Step 1: 加依赖**

在 `loomgui_pkg/Cargo.toml` 的 `[dependencies]` 加：

```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

- [ ] **Step 2: 写失败测试**

`loomgui_pkg/src/workspace.rs`：

```rust
//! 工作区配置（loom.workspace.json）：AI 直接编辑的真相源。
//! 全路径相对工作区根 + 正斜杠。见 docs/superpowers/specs/2026-07-10-standalone-packer-atlas-workspace-design.md §4。

use serde::{Deserialize, Serialize};
use std::path::Path;

/// 工作区根下 loom.workspace.json 的文件名。
pub const WORKSPACE_FILE: &str = "loom.workspace.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Workspace {
    pub version: u32,
    pub output_dir: String,
    #[serde(default)]
    pub packages: Vec<PackageCfg>,
    #[serde(default)]
    pub atlases: Vec<AtlasCfg>,
    #[serde(default)]
    pub fonts: Vec<FontCfg>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PackageCfg {
    pub name: String,
    pub dirs: Vec<String>,
    /// 空 = 自动态（打包时扫 dirs 顶层 .html）；非空 = 显式态（锁定这些文件）。
    #[serde(default)]
    pub html: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AtlasCfg {
    pub name: String,
    #[serde(default)]
    pub default: bool,
    /// 每图独立成页不拼合（超大图用）。
    #[serde(default)]
    pub standalone: bool,
    pub dirs: Vec<String>,
    #[serde(default = "default_max_size")]
    pub max_size: u32,
    #[serde(default = "default_padding")]
    pub padding: u32,
}

fn default_max_size() -> u32 {
    2048
}
fn default_padding() -> u32 {
    4
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FontCfg {
    pub family: String,
    pub file: String,
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub fallback: bool,
}

/// 读工作区根下的 loom.workspace.json。
pub fn load_workspace(root: &Path) -> Result<Workspace, String> {
    let path = root.join(WORKSPACE_FILE);
    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))
}

/// 写回工作区根下的 loom.workspace.json（pretty，AI 好读）。
pub fn save_workspace(root: &Path, ws: &Workspace) -> Result<(), String> {
    let path = root.join(WORKSPACE_FILE);
    let text = serde_json::to_string_pretty(ws)
        .map_err(|e| format!("serialize workspace: {e}"))?;
    std::fs::write(&path, text).map_err(|e| format!("write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_workspace() {
        let json = r#"{
            "version": 1,
            "output_dir": "../dist",
            "packages": [{ "name": "showcase", "dirs": ["ui/showcase"], "html": [] }],
            "atlases": [{ "name": "ui", "default": true, "dirs": ["assets/icons"] }],
            "fonts": [{ "family": "NotoSansSC", "file": "fonts/NotoSansSC.ttc", "default": true, "fallback": true }]
        }"#;
        let ws: Workspace = serde_json::from_str(json).expect("parse");
        assert_eq!(ws.version, 1);
        assert_eq!(ws.packages[0].name, "showcase");
        assert!(ws.packages[0].html.is_empty(), "html 空 = 自动态");
        assert_eq!(ws.atlases[0].max_size, 2048, "max_size 缺省默认 2048");
        assert_eq!(ws.atlases[0].padding, 4, "padding 缺省默认 4");
        assert!(ws.atlases[0].default);
        assert_eq!(ws.fonts[0].family, "NotoSansSC");
    }

    #[test]
    fn roundtrip_workspace() {
        let ws = Workspace {
            version: 1,
            output_dir: "../dist".into(),
            packages: vec![PackageCfg {
                name: "p".into(),
                dirs: vec!["ui".into()],
                html: vec!["a.html".into()],
            }],
            atlases: vec![AtlasCfg {
                name: "ui".into(),
                default: false,
                standalone: false,
                dirs: vec!["assets".into()],
                max_size: 1024,
                padding: 2,
            }],
            fonts: vec![],
        };
        let text = serde_json::to_string(&ws).unwrap();
        let back: Workspace = serde_json::from_str(&text).unwrap();
        assert_eq!(ws, back, "round-trip 保持不变");
    }
}
```

在 `loomgui_pkg/src/lib.rs` 顶部加 `pub mod workspace;`。

- [ ] **Step 3: 跑测试确认失败**

Run: `cargo test -p loomgui_pkg workspace`
Expected: 编译错（serde 未加）或 FAIL，直到依赖+代码就位。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p loomgui_pkg workspace`
Expected: PASS（2 测试）。

- [ ] **Step 5: fmt + clippy + commit**

```bash
cargo fmt --all -- --check
cargo clippy -p loomgui_pkg --all-targets -- -D warnings
git add loomgui_pkg/Cargo.toml loomgui_pkg/src/workspace.rs loomgui_pkg/src/lib.rs Cargo.lock
git commit -m "feat(pkg): workspace.json data model + load/save"
```

## Task 2: img src 相对 html → sprite_key 解析

**Files:**
- Create: `loomgui_pkg/src/resolve.rs`
- Modify: `loomgui_pkg/src/lib.rs`（`pub mod resolve;`）
- Test: 同文件 `#[cfg(test)]`

**Interfaces:**
- Consumes: 无（纯路径逻辑）
- Produces:
  - `fn resolve_img_src(workspace_root: &Path, html_file: &Path, src: &str) -> Result<String, String>`
    返回图相对工作区根的正斜杠路径（sprite_key）。`src` 相对 `html_file` 所在目录解析；越出工作区根 → Err。

- [ ] **Step 1: 写失败测试**

`loomgui_pkg/src/resolve.rs`：

```rust
//! img src（相对 html 文件）→ sprite_key（图相对工作区根的正斜杠路径）。
//! 浏览器原生语义：<img src="home.png"> 相对 html 文件所在目录解析。
//! sprite_key 全局唯一（相对根路径），消除不同目录同名文件撞车。

use std::path::{Component, Path, PathBuf};

/// 把 html 里的 img src 解析成图相对工作区根的路径（正斜杠）。
/// - `workspace_root`：工作区根绝对路径。
/// - `html_file`：html 文件绝对路径（src 相对它所在目录解析）。
/// - `src`：<img src> 原值（相对 html 文件）。
///
/// 归一化 `.`/`..` 后若越出工作区根 → Err。
pub fn resolve_img_src(
    workspace_root: &Path,
    html_file: &Path,
    src: &str,
) -> Result<String, String> {
    let base = html_file
        .parent()
        .ok_or_else(|| format!("html file has no parent: {}", html_file.display()))?;
    let joined = base.join(src);
    // 词法归一化（不碰磁盘，图可能还没生成）：逐段消 . 和 ..。
    let mut stack: Vec<&std::ffi::OsStr> = Vec::new();
    for comp in joined.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if stack.pop().is_none() {
                    // 超出起点，交给下面的 strip_prefix 判越界
                    stack.clear();
                }
            }
            Component::Normal(s) => stack.push(s),
            Component::RootDir | Component::Prefix(_) => stack.clear(),
        }
    }
    let mut normalized = PathBuf::new();
    // 重建绝对路径：从 workspace_root 的根开始拼 joined 的绝对部分。
    // 简化：直接用 joined 的 canonical-ish 归一——重走一遍带根。
    for comp in joined.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    let rel = normalized
        .strip_prefix(workspace_root)
        .map_err(|_| {
            format!(
                "img src `{src}` 解析出的路径 {} 越出工作区根 {}",
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
        assert!(err.is_err(), "越出工作区根应报错");
    }
}
```

在 `lib.rs` 加 `pub mod resolve;`。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_pkg resolve`
Expected: 编译/断言前 FAIL。

- [ ] **Step 3: 跑测试确认通过**

Run: `cargo test -p loomgui_pkg resolve`
Expected: PASS（4 测试）。

- [ ] **Step 4: fmt + clippy + commit**

```bash
cargo fmt --all -- --check
cargo clippy -p loomgui_pkg --all-targets -- -D warnings
git add loomgui_pkg/src/resolve.rs loomgui_pkg/src/lib.rs
git commit -m "feat(pkg): resolve img src relative to html into workspace-relative sprite_key"
```

## Task 3: atlas.json 数据模型

**Files:**
- Create: `loomgui_pkg/src/atlas/mod.rs`
- Modify: `loomgui_pkg/src/lib.rs`（`pub mod atlas;`）
- Test: 同文件 `#[cfg(test)]`

**Interfaces:**
- Produces:
  - `struct AtlasManifest { pages: Vec<String>, sprites: BTreeMap<String, SpriteEntry> }`
  - `struct SpriteEntry { page: u32, uv: [f32; 4], orig: [u32; 2] }`（`uv=[u0,v0,u1,v1]` 归一化；`orig=[w,h]` 像素）
  - 二者 `Serialize`/`Deserialize`

- [ ] **Step 1: 写失败测试**

`loomgui_pkg/src/atlas/mod.rs`：

```rust
//! 自绘图集：读 PNG → shelf 打包 → atlas.png + atlas.json。
//! 图集独立于包；sprite_key = 图相对工作区根路径（全局唯一）。
//! 见 spec §5。本模块的子模块 pack 做实际打包，本文件只定义清单格式。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// atlas.json：一个图集的产物清单。后端读它建 sprite_key→UV 表。
/// BTreeMap 保证 key 有序输出（AI diff 稳定）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AtlasManifest {
    /// 页 png 文件名（相对产物 atlas 目录，如 ["ui.png", "ui.1.png"]）。
    pub pages: Vec<String>,
    /// sprite_key → 条目。
    pub sprites: BTreeMap<String, SpriteEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpriteEntry {
    /// 该 sprite 所在页在 pages 中的索引。
    pub page: u32,
    /// 归一化 UV [u0, v0, u1, v1]，直接喂后端线性映射。
    pub uv: [f32; 4],
    /// 原图像素尺寸 [w, h]（measure + 九宫格基准）。
    pub orig: [u32; 2],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_roundtrip() {
        let mut sprites = BTreeMap::new();
        sprites.insert(
            "assets/icons/home.png".to_string(),
            SpriteEntry {
                page: 0,
                uv: [0.012, 0.048, 0.137, 0.170],
                orig: [64, 64],
            },
        );
        let m = AtlasManifest {
            pages: vec!["ui.png".into()],
            sprites,
        };
        let text = serde_json::to_string(&m).unwrap();
        let back: AtlasManifest = serde_json::from_str(&text).unwrap();
        assert_eq!(m, back);
    }
}
```

在 `lib.rs` 加 `pub mod atlas;`。

- [ ] **Step 2: 跑测试确认失败 → 通过**

Run: `cargo test -p loomgui_pkg atlas::tests::manifest_roundtrip`
先 FAIL（模块未建），补齐后 PASS。

- [ ] **Step 3: fmt + clippy + commit**

```bash
cargo fmt --all -- --check
cargo clippy -p loomgui_pkg --all-targets -- -D warnings
git add loomgui_pkg/src/atlas/mod.rs loomgui_pkg/src/lib.rs
git commit -m "feat(pkg): atlas.json manifest data model"
```

## Task 4: PNG 收集 + 解码

**Files:**
- Modify: `loomgui_pkg/Cargo.toml`（加 `image` 依赖）
- Create: `loomgui_pkg/src/atlas/collect.rs`
- Modify: `loomgui_pkg/src/atlas/mod.rs`（`pub mod collect;`）
- Test: 同文件 `#[cfg(test)]`（用 `image` 生成临时 PNG）

**Interfaces:**
- Consumes: `AtlasCfg`（Task 1）
- Produces:
  - `struct SourceImage { key: String, rgba: image::RgbaImage, w: u32, h: u32 }`
  - `fn collect_pngs(workspace_root: &Path, atlas: &AtlasCfg) -> Result<Vec<SourceImage>, String>`
    递归扫 `atlas.dirs` 下 `.png`，key = 相对工作区根路径，解码 RGBA8，按 key 排序去重。

- [ ] **Step 1: 加依赖**

`loomgui_pkg/Cargo.toml`：

```toml
image = { version = "0.25", default-features = false, features = ["png"] }
```

- [ ] **Step 2: 写失败测试**

`loomgui_pkg/src/atlas/collect.rs`：

```rust
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
pub fn collect_pngs(
    workspace_root: &Path,
    atlas: &AtlasCfg,
) -> Result<Vec<SourceImage>, String> {
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
fn collect_dir(
    workspace_root: &Path,
    dir: &Path,
    out: &mut Vec<String>,
) -> Result<(), String> {
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
        let tmp = std::env::temp_dir().join("loom_collect_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("assets/icons")).unwrap();
        std::fs::create_dir_all(tmp.join("assets/sub")).unwrap();
        // 写两张 2x2 PNG。
        let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]));
        img.save(tmp.join("assets/icons/b.png")).unwrap();
        img.save(tmp.join("assets/sub/a.png")).unwrap();

        let cfg = AtlasCfg {
            name: "ui".into(),
            default: false,
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
```

在 `atlas/mod.rs` 加 `pub mod collect;`。

- [ ] **Step 3: 跑测试确认失败 → 通过**

Run: `cargo test -p loomgui_pkg atlas::collect`
Expected: 补齐后 PASS。

- [ ] **Step 4: fmt + clippy + commit**

```bash
cargo fmt --all -- --check
cargo clippy -p loomgui_pkg --all-targets -- -D warnings
git add loomgui_pkg/Cargo.toml loomgui_pkg/src/atlas/collect.rs loomgui_pkg/src/atlas/mod.rs Cargo.lock
git commit -m "feat(pkg): collect + decode atlas source PNGs recursively"
```

## Task 5: shelf 打包 → atlas.png + AtlasManifest

**Files:**
- Create: `loomgui_pkg/src/atlas/pack.rs`
- Modify: `loomgui_pkg/src/atlas/mod.rs`（`pub mod pack;`）
- Test: 同文件 `#[cfg(test)]`

**Interfaces:**
- Consumes: `SourceImage`（Task 4）, `AtlasManifest`/`SpriteEntry`（Task 3）, `AtlasCfg`（Task 1）
- Produces:
  - `struct PackedAtlas { manifest: AtlasManifest, pages: Vec<image::RgbaImage> }`
  - `fn pack_atlas(atlas: &AtlasCfg, images: &[SourceImage]) -> Result<PackedAtlas, String>`
    shelf 分配（etagere），超 `max_size` 开多页；`standalone`=true 时每图独立成页；单图比页大 → Err；UV 归一化 + orig 填入 manifest。

- [ ] **Step 1: 写失败测试**

`loomgui_pkg/src/atlas/pack.rs`：

```rust
//! shelf 打包：SourceImage 列表 → atlas 页（RgbaImage）+ AtlasManifest。
//! 复用 etagere（core 字体图集同款）。禁旋转/trim（轴对齐，对齐 Unity 侧包装约束）。

use super::collect::SourceImage;
use super::{AtlasManifest, SpriteEntry};
use crate::workspace::AtlasCfg;
use etagere::{size2, AtlasAllocator};
use std::collections::BTreeMap;

/// 打包结果：清单 + 每页像素（caller 负责编码写盘）。
pub struct PackedAtlas {
    pub manifest: AtlasManifest,
    pub pages: Vec<image::RgbaImage>,
}

/// 把一个图集的源图 shelf 打包成若干页。
pub fn pack_atlas(atlas: &AtlasCfg, images: &[SourceImage]) -> Result<PackedAtlas, String> {
    let pad = atlas.padding as i32;
    let max = atlas.max_size;

    let mut pages: Vec<image::RgbaImage> = Vec::new();
    let mut allocators: Vec<AtlasAllocator> = Vec::new();
    let mut sprites: BTreeMap<String, SpriteEntry> = BTreeMap::new();

    for img in images {
        // 单图 + padding 比页还大 → 明确报错（AI 可诉诸行动：调大 max_size 或用 standalone）。
        let need_w = img.w as i32 + pad * 2;
        let need_h = img.h as i32 + pad * 2;
        if need_w > max as i32 || need_h > max as i32 {
            return Err(format!(
                "图 `{}`（{}×{} + padding {}）超过图集 `{}` 单页上限 {}；调大 max_size 或改 standalone",
                img.key, img.w, img.h, atlas.padding, atlas.name, max
            ));
        }

        // standalone：每图独立成页；否则尝试塞进已有页，失败再开新页。
        let (page_idx, alloc) = if atlas.standalone {
            new_page(&mut pages, &mut allocators, max);
            let idx = pages.len() - 1;
            let a = allocators[idx]
                .allocate(size2(need_w, need_h))
                .ok_or_else(|| format!("standalone 分配失败：{}", img.key))?;
            (idx, a)
        } else {
            let mut placed: Option<(usize, etagere::Allocation)> = None;
            for (idx, allocator) in allocators.iter_mut().enumerate() {
                if let Some(a) = allocator.allocate(size2(need_w, need_h)) {
                    placed = Some((idx, a));
                    break;
                }
            }
            match placed {
                Some(p) => p,
                None => {
                    new_page(&mut pages, &mut allocators, max);
                    let idx = pages.len() - 1;
                    let a = allocators[idx]
                        .allocate(size2(need_w, need_h))
                        .ok_or_else(|| format!("新页分配失败：{}", img.key))?;
                    (idx, a)
                }
            }
        };

        // blit 到页（跳过 padding 边）。
        let x0 = alloc.rectangle.min.x + pad;
        let y0 = alloc.rectangle.min.y + pad;
        blit(&mut pages[page_idx], &img.rgba, x0 as u32, y0 as u32);

        // 归一化 UV。
        let pw = max as f32;
        let ph = max as f32;
        let u0 = x0 as f32 / pw;
        let v0 = y0 as f32 / ph;
        let u1 = (x0 as u32 + img.w) as f32 / pw;
        let v1 = (y0 as u32 + img.h) as f32 / ph;
        sprites.insert(
            img.key.clone(),
            SpriteEntry {
                page: page_idx as u32,
                uv: [u0, v0, u1, v1],
                orig: [img.w, img.h],
            },
        );
    }

    let page_names: Vec<String> = (0..pages.len())
        .map(|i| page_file_name(&atlas.name, i))
        .collect();

    Ok(PackedAtlas {
        manifest: AtlasManifest {
            pages: page_names,
            sprites,
        },
        pages,
    })
}

/// 页文件名：第 0 页 `<name>.png`，其后 `<name>.<n>.png`。
pub fn page_file_name(atlas_name: &str, idx: usize) -> String {
    if idx == 0 {
        format!("{atlas_name}.png")
    } else {
        format!("{atlas_name}.{idx}.png")
    }
}

fn new_page(pages: &mut Vec<image::RgbaImage>, allocators: &mut Vec<AtlasAllocator>, max: u32) {
    pages.push(image::RgbaImage::from_pixel(
        max,
        max,
        image::Rgba([0, 0, 0, 0]),
    ));
    allocators.push(AtlasAllocator::new(size2(max as i32, max as i32)));
}

fn blit(page: &mut image::RgbaImage, src: &image::RgbaImage, x0: u32, y0: u32) {
    for y in 0..src.height() {
        for x in 0..src.width() {
            page.put_pixel(x0 + x, y0 + y, *src.get_pixel(x, y));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(key: &str, w: u32, h: u32) -> SourceImage {
        SourceImage {
            key: key.into(),
            rgba: image::RgbaImage::from_pixel(w, h, image::Rgba([1, 2, 3, 255])),
            w,
            h,
        }
    }

    fn cfg(max: u32, standalone: bool) -> AtlasCfg {
        AtlasCfg {
            name: "ui".into(),
            default: false,
            standalone,
            dirs: vec![],
            max_size: max,
            padding: 2,
        }
    }

    #[test]
    fn packs_all_into_one_page_no_overlap() {
        let images = vec![img("a.png", 16, 16), img("b.png", 16, 16), img("c.png", 16, 16)];
        let packed = pack_atlas(&cfg(256, false), &images).unwrap();
        assert_eq!(packed.pages.len(), 1);
        // 覆盖：每个输入都有条目。
        assert_eq!(packed.manifest.sprites.len(), 3);
        // 不重叠：任意两 sprite 的像素 rect（由 uv×256 反推）不相交。
        let rects: Vec<[u32; 4]> = packed
            .manifest
            .sprites
            .values()
            .map(|s| {
                [
                    (s.uv[0] * 256.0).round() as u32,
                    (s.uv[1] * 256.0).round() as u32,
                    (s.uv[2] * 256.0).round() as u32,
                    (s.uv[3] * 256.0).round() as u32,
                ]
            })
            .collect();
        for i in 0..rects.len() {
            for j in (i + 1)..rects.len() {
                let a = rects[i];
                let b = rects[j];
                let disjoint = a[2] <= b[0] || b[2] <= a[0] || a[3] <= b[1] || b[3] <= a[1];
                assert!(disjoint, "sprite {i} 与 {j} 重叠：{a:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn overflow_opens_new_page() {
        // max=64, padding=2 → 每张 60x60+pad 只能塞一张/页 → 3 张 = 3 页。
        let images = vec![img("a.png", 58, 58), img("b.png", 58, 58), img("c.png", 58, 58)];
        let packed = pack_atlas(&cfg(64, false), &images).unwrap();
        assert!(packed.pages.len() >= 2, "溢出应多页，实际 {}", packed.pages.len());
        assert_eq!(packed.manifest.pages[0], "ui.png");
        assert_eq!(packed.manifest.pages[1], "ui.1.png");
    }

    #[test]
    fn standalone_one_per_page() {
        let images = vec![img("a.png", 16, 16), img("b.png", 16, 16)];
        let packed = pack_atlas(&cfg(256, true), &images).unwrap();
        assert_eq!(packed.pages.len(), 2, "standalone 每图独立成页");
    }

    #[test]
    fn oversized_single_image_errors() {
        let images = vec![img("huge.png", 300, 10)];
        let err = pack_atlas(&cfg(256, false), &images);
        assert!(err.is_err(), "单图超页应报错");
    }
}
```

在 `atlas/mod.rs` 加 `pub mod pack;`。

- [ ] **Step 2: 跑测试确认失败 → 通过**

Run: `cargo test -p loomgui_pkg atlas::pack`
Expected: 补齐后 PASS（4 测试）。

- [ ] **Step 3: fmt + clippy + commit**

```bash
cargo fmt --all -- --check
cargo clippy -p loomgui_pkg --all-targets -- -D warnings
git add loomgui_pkg/src/atlas/pack.rs loomgui_pkg/src/atlas/mod.rs
git commit -m "feat(pkg): shelf-pack atlas pages + emit AtlasManifest (multi-page, standalone, oversize err)"
```

## Task 6: 改 pack() —— img src 相对 html 归一化成 sprite_key

**Files:**
- Modify: `loomgui_pkg/src/lib.rs`（`scene_to_template` + `pack` 签名）
- Test: `loomgui_pkg/tests/pack.rs`（更新既有测试的期望 src）

**Interfaces:**
- Consumes: `resolve_img_src`（Task 2）
- Produces:
  - `pack` 新签名：`pub fn pack(workspace_root: &Path, name: &str, html_files: &[(String, PathBuf)]) -> Result<PackedPackage, String>`，其中每项 = (相对根的 html 路径→组件名, html 绝对路径)。
  - `struct PackedPackage { pkg_bytes: Vec<u8>, referenced_sprites: Vec<String> }`（**去掉 asset_manifest**，改为「本包引用到的 sprite_key 列表」供交叉验证）。
  - `scene_to_template` 改为收 `referenced: &mut Vec<String>`（sprite_key），src 回写为 sprite_key。

**说明**：parse-time 改动，完成后须重打 pkg。旧 `normalize_path`(相对 res_dir) + `read_png_size` 从 pack 路径移除。**注意两个同名概念**：pkg crate 的 `PackedPackage`（lib.rs）本任务改（去 `asset_manifest`、加 `referenced_sprites`）；core 的 `PackageInput`/`AssetEntry`（asset/mod.rs）在 Task 10 才删。故本任务 `write_package` 调用仍传 `asset_manifest: &[]`（core 侧字段还在，空内容）；Task 10 删 core 字段后同步去掉这个 `&[]` 实参。

- [ ] **Step 1: 更新 scene_to_template**

把 `scene_to_template` 里三处 `normalize_path(src, res_dir)`（img src / background-image / rich 行内图）改为 `crate::resolve::resolve_img_src(workspace_root, html_file, src)`，结果 push 进 `referenced: &mut Vec<String>`（`seen` 去重），并回写 src 为 sprite_key。签名去掉 `manifest`/`res_dir`，加 `workspace_root: &Path, html_file: &Path, referenced: &mut Vec<String>`。解析失败（越出根）→ 返 `Err`（引用根外的图是错误，不再 warn 跳过）。

- [ ] **Step 2: 更新 pack 签名与循环**

```rust
pub struct PackedPackage {
    pub pkg_bytes: Vec<u8>,
    /// 本包所有 img/bg/rich-img 引用到的 sprite_key（去重），供 build 交叉验证。
    pub referenced_sprites: Vec<String>,
}

/// 打包一个包。`html_files` = [(相对根 html 路径, html 绝对路径)]；组件名 = 文件名去 .html。
pub fn pack(
    workspace_root: &Path,
    _name: &str,
    html_files: &[(String, std::path::PathBuf)],
) -> Result<PackedPackage, String> {
    // 每 html：read → extract_component_css → strip_style_and_link → parse_html → parse_css
    //   → extract_dynamic_rules → resolve_styles → desugar_block_divs → build_scene
    //   → collect_controller_pages → scene_to_template(workspace_root, &html_abs, &mut referenced, ...)
    // 组件名 = 相对根 html 路径的 file_stem。
    // write_package 的 asset_manifest 传 &[]（段保留、空；阶段二删段）。
}
```

`_name` 保留（当前未进 header）。scene_to_template 的 `seen`/`referenced` 跨 html 累计。

- [ ] **Step 3: 更新既有 pack.rs 测试**

`loomgui_pkg/tests/pack.rs` 里断言「节点 src 归一化」的期望值：从相对 res_dir（`icons/skin.png`）改为相对工作区根的 sprite_key（按测试目录布局，如 `res/icons/skin.png`）。手搓 scene 的测试改为验 `scene_to_template` 的 `referenced` 收集 + src 回写为 sprite_key。删除依赖旧 `asset_manifest`/`read_png_size` 的断言。

- [ ] **Step 4: 跑测试**

Run: `cargo test -p loomgui_pkg`
Expected: PASS（全 crate）。

- [ ] **Step 5: fmt + clippy + commit**

```bash
cargo fmt --all -- --check
cargo clippy -p loomgui_pkg --all-targets -- -D warnings
git add loomgui_pkg/src/lib.rs loomgui_pkg/tests/pack.rs
git commit -m "feat(pkg): resolve img src relative to html into workspace sprite_key; pack returns referenced_sprites"
```

## Task 7: 交叉验证 —— 引用的图都能在某 atlas 找到 + 冲突检测

**Files:**
- Create: `loomgui_pkg/src/atlas/validate.rs`
- Modify: `loomgui_pkg/src/atlas/mod.rs`（`pub mod validate;`）
- Test: 同文件

**Interfaces:**
- Consumes: `AtlasManifest`（Task 3）
- Produces:
  - `fn assign_and_validate(referenced: &[String], atlases: &[(String, &AtlasManifest)]) -> Result<(), String>`
    单向验证：每个 referenced sprite_key 必须出现在**恰好一个** atlas 的 sprites；0 个 → Err（无归属）；>1 个 → Err（冲突）。**不反向要求** atlas 里的图都被引用（动态图标合法）。

- [ ] **Step 1: 写失败测试 + 实现**

```rust
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
                    "图 `{key}` 被引用但不在任何 atlas；把它所在目录加进某 atlas.dirs，或设一个 default atlas"
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
            sprites.insert(k.to_string(), SpriteEntry { page: 0, uv: [0.0; 4], orig: [1, 1] });
        }
        AtlasManifest { pages: vec![], sprites }
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
        assert!(assign_and_validate(&["a.png".into()], &[("ui".into(), &ui), ("char".into(), &ch)]).is_err());
    }
}
```

在 `atlas/mod.rs` 加 `pub mod validate;`。

- [ ] **Step 2: 跑测试 → 通过**

Run: `cargo test -p loomgui_pkg atlas::validate`
Expected: PASS（4 测试）。

- [ ] **Step 3: fmt + clippy + commit**

```bash
cargo fmt --all -- --check
cargo clippy -p loomgui_pkg --all-targets -- -D warnings
git add loomgui_pkg/src/atlas/validate.rs loomgui_pkg/src/atlas/mod.rs
git commit -m "feat(pkg): cross-validate referenced sprites belong to exactly one atlas"
```

## Task 8: build 编排 + runtime.json + 写盘

**Files:**
- Create: `loomgui_pkg/src/build.rs`
- Create: `loomgui_pkg/src/runtime.rs`
- Modify: `loomgui_pkg/src/lib.rs`（`pub mod build; pub mod runtime;`）
- Test: `loomgui_pkg/tests/build.rs`（端到端）

**Interfaces:**
- Consumes: `Workspace`（T1）、`pack`（T6）、`collect_pngs`（T4）、`pack_atlas`（T5）、`assign_and_validate`（T7）
- Produces:
  - `runtime.rs`: `struct RuntimeManifest { version: u32, packages: Vec<String>, atlases: Vec<String>, fonts: Vec<RuntimeFont> }`, `struct RuntimeFont { family, file, default, fallback }`（Serialize/Deserialize）, `const RUNTIME_FILE: &str = "loom.runtime.json"`
  - `build.rs`: `struct BuildReport { packages: Vec<String>, atlases: Vec<String>, fonts: Vec<String>, log: Vec<String> }`, `fn build(workspace_root: &Path) -> Result<BuildReport, String>`

- [ ] **Step 1: runtime.rs**

```rust
//! loom.runtime.json：后端自举清单（打包器产，替代 Unity LoomSettings SO）。见 spec §6.1。

use serde::{Deserialize, Serialize};

pub const RUNTIME_FILE: &str = "loom.runtime.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeManifest {
    pub version: u32,
    pub packages: Vec<String>, // .pkg.bin 文件名（不含扩展）
    pub atlases: Vec<String>,  // 每个对应 <name>.atlas.json + png
    pub fonts: Vec<RuntimeFont>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeFont {
    pub family: String,
    pub file: String, // 产物 fonts/ 下文件名（源名 + ".bytes"）
    pub default: bool,
    pub fallback: bool,
}
```

- [ ] **Step 2: build.rs 实现**

编排顺序：
1. `load_workspace(root)`。
2. `output_dir = root.join(&ws.output_dir)`；建 `ui/`、`atlas/`、`fonts/`（`create_dir_all`）。
3. **每包**：算 html 列表 —— `pkg.html` 非空则用之（对每个 `dirs` 项定位存在的文件，取首个命中）；空则扫每个 `dir` 顶层 `.html`（不递归、字母序、去重）→ 得 `[(rel_html, abs)]`。调 `pack(root, &pkg.name, &html)` → 写 `ui/<name>.pkg.bin`，累加 `referenced_sprites` 到全局 `all_referenced`。
4. **每 atlas**：`collect_pngs(root, atlas)` → `pack_atlas(atlas, &imgs)` → 每页 `img.save(atlas_dir.join(page_name))` + 写 `atlas/<name>.atlas.json`（`to_string_pretty`）。收 `(name.clone(), manifest)` 到 `atlas_manifests`。
5. `assign_and_validate(&all_referenced, &refs)`（refs = `atlas_manifests` 借用）。
6. **字体**：每 `FontCfg` 拷 `root/<file>` → `fonts/<basename>.bytes`（`fs::copy`）；缺文件 → Err。
7. **runtime.json**：`RuntimeManifest { version:1, packages, atlases, fonts:RuntimeFont{file=basename+".bytes"} }` → 写 `output_dir/loom.runtime.json`。
8. 返 `BuildReport`。

任何步骤失败即 `Err`。

- [ ] **Step 3: 端到端测试** `loomgui_pkg/tests/build.rs`

构造临时工作区：写 `loom.workspace.json`（1 包 dirs=["ui"] html=[]，1 atlas name="ui" default=true dirs=["assets"]，1 font）+ `ui/main.html`（含 `<img src="../assets/home.png">`）+ `assets/home.png`（`image` 生成 4×4）+ 一个字体 stub（任意字节写 `fonts/f.ttf`）+ workspace.json 里 font.file=`fonts/f.ttf`。调 `build(&root)`，断言：
- `output/ui/showcase... .pkg.bin` 存在（按包名）
- `output/atlas/ui.png` + `ui.atlas.json` 存在
- 解析 `ui.atlas.json` 含 sprite_key `assets/home.png`
- `output/fonts/f.ttf.bytes` 存在
- `output/loom.runtime.json` 存在，反序列化后 packages/atlases/fonts 各非空

再写失败用例：html 引用 `assets/missing.png`（磁盘无）——`collect_pngs` 收不到 → 交叉验证 Err；断言 `build` 返 Err。清理临时目录。

- [ ] **Step 4: 跑测试**

Run: `cargo test -p loomgui_pkg build`
Expected: PASS。

- [ ] **Step 5: fmt + clippy + commit**

```bash
cargo fmt --all -- --check
cargo clippy -p loomgui_pkg --all-targets -- -D warnings
git add loomgui_pkg/src/build.rs loomgui_pkg/src/runtime.rs loomgui_pkg/src/lib.rs loomgui_pkg/tests/build.rs
git commit -m "feat(pkg): build orchestration (pack + atlas + fonts + runtime.json) with cross-validation"
```

## Task 9: 零参 CLI —— loom-pkg build <workspace>

**Files:**
- Modify: `loomgui_pkg/src/main.rs`（整体替换）

**Interfaces:**
- Consumes: `build`（Task 8）
- Produces: CLI `loom-pkg build <workspace-dir>`（成功 exit 0；错误 stderr + 非零退出）

- [ ] **Step 1: 替换 main.rs**

```rust
//! loom-pkg CLI：零参 build 读工作区配置一键打包。
//! 用法：loom-pkg build <workspace-dir>
//!   读 <workspace-dir>/loom.workspace.json，全量产出到配置的 output_dir。

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 || args[1] != "build" {
        eprintln!(
            "usage: {} build <workspace-dir>",
            args.first().map(String::as_str).unwrap_or("loom-pkg")
        );
        return ExitCode::from(2);
    }
    let root = PathBuf::from(&args[2]);
    match loomgui_pkg::build::build(&root) {
        Ok(report) => {
            for line in &report.log {
                eprintln!("{line}");
            }
            eprintln!(
                "OK: {} packages, {} atlases, {} fonts",
                report.packages.len(),
                report.atlases.len(),
                report.fonts.len()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("build failed: {e}");
            ExitCode::FAILURE
        }
    }
}
```

- [ ] **Step 2: 编译 + smoke**

Run: `cargo build -p loomgui_pkg`
Expected: 编译通过。

- [ ] **Step 3: fmt + clippy + commit**

```bash
cargo fmt --all -- --check
cargo clippy -p loomgui_pkg --all-targets -- -D warnings
git add loomgui_pkg/src/main.rs
git commit -m "feat(pkg): zero-arg 'loom-pkg build <workspace>' CLI"
```

---

# 阶段二：运行时消费改造

> 目标闭环：CLI 打包 → Unity PlayMode 渲染。本阶段动 pkg 格式（删 manifest 段，v14→v15）、core 加尺寸批量入口 + FFI、Unity 消费侧重写。**改 Rust 后必重编 + commit `.dll`**。

## Task 10: pkg.bin 删除 AssetManifest 段（v14 → v15）

**Files:**
- Modify: `loomgui_core/src/asset/mod.rs`（删 `AssetEntry`、`PackedPackage.asset_manifest`、`PackageInput.asset_manifest`、`Package.asset_manifest`、write/read 的 manifest 段、版本号）
- Modify: `loomgui_pkg/src/lib.rs`（`write_package` 调用去掉 `asset_manifest` 字段）
- Modify: `loomgui_core/src/stage.rs`（`load_package` 去掉从 manifest 自建 `image_sizes` 的逻辑）
- Test: `loomgui_core` 既有 asset round-trip 测试更新

**Interfaces:**
- Produces:
  - `PKG_FORMAT_VERSION = 15`，`MIN_VERSION = MAX_VERSION = 15`
  - `PackageInput` 去掉 `asset_manifest` 字段
  - `Package` 去掉 `asset_manifest` 字段
  - `Stage.load_package` 不再触碰 `image_sizes`（尺寸改由 Task 12 的 `set_image_sizes` 灌入）

**说明**：项目早期、做干净不留冗余段。删段后 pkg 布局末尾不再有 AssetManifest。`AssetEntry` 类型整个删除。

- [ ] **Step 1: 删 write 侧 manifest 段**

`loomgui_core/src/asset/mod.rs`：
- 删 `manifest_idx` 收集块（`// intern asset_manifest path ...`）。
- 删文件末尾 `// AssetManifest: entry_count ...` 的写入块。
- `PackageInput` struct 删 `asset_manifest` 字段。
- `PackedPackage`（若 core 内有）与 `Package` struct 删 `asset_manifest` 字段。
- 删 `pub struct AssetEntry { path, w, h }` 定义。
- 版本号：`PKG_FORMAT_VERSION` `14 → 15`；`MIN_VERSION`/`MAX_VERSION` → 15；更新版本注释（`v15：删 AssetManifest 段，图尺寸改走 atlas.json + set_image_sizes`）。

- [ ] **Step 2: 删 read 侧 manifest 段**

`read_package`：删 `// AssetManifest: entry_count ...` 读取块（`entry_count` + 循环），`Package` 构造去掉 `asset_manifest`。

- [ ] **Step 3: 改 stage.load_package**

`loomgui_core/src/stage.rs` `load_package`：删「清前次包 image_sizes」+「合并本包 manifest 进 image_sizes」两块。`image_sizes` 字段保留（Task 12 灌入）。更新 doc 注释说明尺寸改由 `set_image_sizes` 灌。

- [ ] **Step 4: 改 pkg crate 调用处**

`loomgui_pkg/src/lib.rs`：`write_package` 的 `PackageInput` 构造去掉 `asset_manifest: &[]`（字段已删）。

- [ ] **Step 5: 更新测试**

`loomgui_core` 内 asset round-trip 测试：去掉对 `asset_manifest` 的构造与断言。版本断言从 14 改 15。搜 `asset_manifest`/`AssetEntry` 确保 core 内无残留引用。

- [ ] **Step 6: 跑测试**

Run: `cargo test -p loomgui_core asset`
Run: `cargo test -p loomgui_core`
Expected: PASS。

- [ ] **Step 7: fmt + clippy + commit**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
git add loomgui_core/src/asset/mod.rs loomgui_core/src/stage.rs loomgui_pkg/src/lib.rs
git commit -m "feat(core)!: remove AssetManifest section from pkg.bin (v14->v15); image sizes move to atlas.json"
```

## Task 11: core Stage::set_image_sizes 批量入口

**Files:**
- Modify: `loomgui_core/src/stage.rs`（加方法）
- Test: `loomgui_core/src/stage.rs` `#[cfg(test)]`

**Interfaces:**
- Produces:
  - `pub fn set_image_sizes(&mut self, sizes: &[(String, u32, u32)])` —— 批量覆盖式合并进 `self.image_sizes`（同 path 后写赢；w/h=0 条目也存，`image_size()` 的 filter 会挡 → fallback 64×64）。

- [ ] **Step 1: 写失败测试**

在 stage.rs 测试模块加：

```rust
#[test]
fn set_image_sizes_batch_merges() {
    let mut stage = Stage::new(/* 按现有 Stage::new 签名 */);
    stage.set_image_sizes(&[
        ("icons/a.png".to_string(), 32, 32),
        ("icons/b.png".to_string(), 64, 48),
    ]);
    assert_eq!(stage.image_size("icons/a.png"), Some((32, 32)));
    assert_eq!(stage.image_size("icons/b.png"), Some((64, 48)));
    // 后写赢
    stage.set_image_sizes(&[("icons/a.png".to_string(), 100, 100)]);
    assert_eq!(stage.image_size("icons/a.png"), Some((100, 100)));
}
```

（`Stage::new` 的确切签名照现有测试里的构造方式。）

- [ ] **Step 2: 实现**

```rust
/// 批量灌图尺寸（后端读所有 atlas.json 合并后一次性推入；见 spec §6.4）。
/// 覆盖式合并：同 path 后写赢。上万条也是 O(n) HashMap 插入，启动一次调用。
pub fn set_image_sizes(&mut self, sizes: &[(String, u32, u32)]) {
    for (path, w, h) in sizes {
        self.image_sizes.insert(path.clone(), (*w, *h));
    }
}
```

- [ ] **Step 3: 跑测试 → 通过**

Run: `cargo test -p loomgui_core set_image_sizes`
Expected: PASS。

- [ ] **Step 4: fmt + clippy + commit**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
git add loomgui_core/src/stage.rs
git commit -m "feat(core): Stage::set_image_sizes batch entry for backend-supplied atlas sizes"
```

## Task 12: FFI loomgui_stage_set_image_sizes（批量单次）

**Files:**
- Modify: `loomgui_ffi_c/src/lib.rs`（加 FFI 入口）
- Modify: `loomgui_ffi_c` 的 C# 镜像（csbindgen 自动生成 `LoomGUIBindings.cs`——build.rs 重生成，无需手补，除非是 `#[repr(C)]` struct）
- Test: `loomgui_ffi_c/src/abi_tests.rs`（调用不 panic + 尺寸生效）

**Interfaces:**
- Consumes: `Stage::set_image_sizes`（Task 11）
- Produces:
  - `pub extern "C" fn loomgui_stage_set_image_sizes(h: *mut StageHandle, paths_ptr: *const *const c_char, ws: *const u32, hs: *const u32, count: usize)`
    —— 逐条读 C 字符串 + w/h，组 `Vec<(String,u32,u32)>` 调 `set_image_sizes`。null/count=0 → no-op（不 panic）。

- [ ] **Step 1: 写 FFI 入口**

参照 `loomgui_stage_set_scroll_pos` 风格（null 检查 + 早返），`loomgui_ffi_c/src/lib.rs`：

```rust
/// driver 启动时把所有 atlas.json 合并出的图尺寸批量灌入（一次调用，非逐条）。
/// paths_ptr: count 个 C 字符串指针；ws/hs: count 个 u32。任一为 null 或 count=0 → no-op。
/// 首帧 solve 前调（启动加载阶段）。FFI 入口不 panic。
#[no_mangle]
pub extern "C" fn loomgui_stage_set_image_sizes(
    h: *mut StageHandle,
    paths_ptr: *const *const std::os::raw::c_char,
    ws: *const u32,
    hs: *const u32,
    count: usize,
) {
    if h.is_null() || paths_ptr.is_null() || ws.is_null() || hs.is_null() || count == 0 {
        return;
    }
    let handle = unsafe { &mut *h };
    let paths = unsafe { std::slice::from_raw_parts(paths_ptr, count) };
    let ws = unsafe { std::slice::from_raw_parts(ws, count) };
    let hs = unsafe { std::slice::from_raw_parts(hs, count) };
    let mut sizes: Vec<(String, u32, u32)> = Vec::with_capacity(count);
    for i in 0..count {
        if paths[i].is_null() {
            continue;
        }
        let cstr = unsafe { std::ffi::CStr::from_ptr(paths[i]) };
        if let Ok(s) = cstr.to_str() {
            sizes.push((s.to_string(), ws[i], hs[i]));
        }
    }
    handle.stage.set_image_sizes(&sizes);
}
```

（`#![allow(clippy::not_unsafe_ptr_arg_deref)]` 已在 crate root——沿用现有 FFI 模式。）

- [ ] **Step 2: 写 abi 测试**

`loomgui_ffi_c/src/abi_tests.rs`：建 stage handle，用 `CString` 组两条 path + w/h 数组，调 `loomgui_stage_set_image_sizes`，然后（若有查询口）验尺寸生效，或至少验不 panic + 返回。null 句柄调用验 no-op。

- [ ] **Step 3: 跑测试**

Run: `cargo test -p loomgui_ffi_c set_image_sizes`
Expected: PASS。

- [ ] **Step 4: 重编 .dll + 确认 bindings 生成**

```bash
cargo build -p loomgui_ffi_c --release
```
确认 `loomgui_ffi_c.dll` + `LoomGUIBindings.cs` 里出现 `loomgui_stage_set_image_sizes`。拷 dll（Unity 关闭时）：
```bash
cp target/release/loomgui_ffi_c.dll loomgui_unity_package/Plugins/LoomGUI/loomgui_ffi_c.dll
```

- [ ] **Step 5: fmt + clippy + commit（含 dll + bindings）**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
git add loomgui_ffi_c/src/lib.rs loomgui_ffi_c/src/abi_tests.rs loomgui_unity_package/Plugins/LoomGUI/loomgui_ffi_c.dll loomgui_unity_package/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs
git commit -m "feat(ffi): loomgui_stage_set_image_sizes batch entry + rebuild dll"
```

## Task 13: C# 运行时清单模型 + atlas.json 读取

**Files:**
- Create: `loomgui_unity_package/Runtime/LoomManifests.cs`（RuntimeManifest + AtlasManifest 的 C# 镜像 + JSON 解析）
- Test: `loomgui_unity_package/Tests/LoomManifestsTests.cs`

**Interfaces:**
- Produces:
  - `class RuntimeManifest { int version; List<string> packages; List<string> atlases; List<RuntimeFont> fonts; }`
  - `class RuntimeFont { string family; string file; bool @default; bool fallback; }`
  - `class AtlasManifest { List<string> pages; Dictionary<string, SpriteEntry> sprites; }`
  - `struct SpriteEntry { int page; float[] uv; int[] orig; }`（uv 长 4，orig 长 2）
  - `static RuntimeManifest ParseRuntime(string json)` / `static AtlasManifest ParseAtlas(string json)`

**说明**：Unity `JsonUtility` 不支持 `Dictionary`，用 `Newtonsoft`（Unity 自带 `com.unity.nuget.newtonsoft-json`）或手写极简解析。优先 Newtonsoft（Unity 项目常已装）。若不可用，退回手写解析（atlas.json 结构固定）。计划以 Newtonsoft 为准。

- [ ] **Step 1: 写测试**

`LoomManifestsTests.cs`：喂一段 runtime.json 字符串 → `ParseRuntime` → 断言 packages/atlases/fonts 字段；喂一段 atlas.json → `ParseAtlas` → 断言某 sprite_key 的 page/uv/orig。

- [ ] **Step 2: 实现 LoomManifests.cs**

用 `Newtonsoft.Json.JsonConvert.DeserializeObject<T>`。字段名匹配 JSON（`default` C# 关键字用 `@default` + `[JsonProperty("default")]`）。

- [ ] **Step 3: 跑 EditMode 测试**（Unity Test Runner，家里机或本机 Unity）

Expected: PASS。

- [ ] **Step 4: commit**

```bash
git add loomgui_unity_package/Runtime/LoomManifests.cs loomgui_unity_package/Tests/LoomManifestsTests.cs
git commit -m "feat(unity): C# RuntimeManifest + AtlasManifest models + JSON parse"
```

## Task 14: SpriteResolver 重写（吃 atlas.png + atlas.json）

**Files:**
- Modify: `loomgui_unity_package/Runtime/SpriteResolver.cs`（重写数据来源）
- Test: `loomgui_unity_package/Tests/SpriteResolverTests.cs`（更新）

**Interfaces:**
- Consumes: `AtlasManifest`（Task 13）
- Produces:
  - `struct SpriteLookup { Texture2D tex; Rect uv; int origW; int origH; bool found; }`（`uv` 为 [u0,v0,u1,v1] 换成 Unity Rect(x=u0,y=v0,w=u1-u0,h=v1-v0)）
  - `void Init(List<AtlasManifest> atlases, Func<string, Texture2D> loadPage)` —— 合并所有 atlas 的 sprites 成全局 `key → (atlasIdx, page, uv, orig)` 表；`loadPage(pageFileName)` 懒加载页纹理。
  - `SpriteLookup GetSprite(string key)` —— 查表得纹理（懒加载页）+ uv rect + orig。miss → `found=false`（调用方 fallback），warn 去重。
  - 保留 `RegisterFontAtlasPage(string path, Texture2D tex)`（字体 atlas 页仍走这条，key 命中直接返全区域）。

**说明**：删掉所有 Unity `SpriteAtlas`/`Sprite` 依赖（`using UnityEngine.U2D`、`GetSprite(name)`、`sp.uv`、`_folderToAtlasName` 路由）。UV 已是打包算好的最终值，不再有「顶层子目录→atlasName」路由。

- [ ] **Step 1: 更新测试**

`SpriteResolverTests.cs`：构造两个 `AtlasManifest`（含若干 sprite_key + uv + orig）+ 一个假 `loadPage`（返 `Texture2D.whiteTexture` 或 new Texture2D）→ `Init` → `GetSprite(key)` 断言 `found`、uv rect 正确（由 uv[4] 换算）、orig 正确；`GetSprite("missing")` 断言 `found=false`。删除旧的 folder→atlasName 路由测试、Sprite.uv 包围盒测试。

- [ ] **Step 2: 重写 SpriteResolver.cs**

全局表 `Dictionary<string, (int atlasIdx, int page, float[] uv, int[] orig)>`；页缓存 `Dictionary<(int,int), Texture2D>`（atlasIdx+page → 懒加载）；`loadPage` 委托签名 `Func<string, Texture2D>`（传页文件名如 `ui.png`）。`GetSprite` 查表 → 懒加载页 → 组 `SpriteLookup`。

- [ ] **Step 3: 跑 EditMode 测试**

Expected: PASS。

- [ ] **Step 4: commit**

```bash
git add loomgui_unity_package/Runtime/SpriteResolver.cs loomgui_unity_package/Tests/SpriteResolverTests.cs
git commit -m "feat(unity): rewrite SpriteResolver to consume self-drawn atlas.png + atlas.json (drop Unity SpriteAtlas)"
```

## Task 15: MirrorPool UV 线性映射（删 Sprite.uv 包围盒 workaround）

**Files:**
- Modify: `loomgui_unity_package/Runtime/MirrorPool.cs`（`RemapMeshUvToSprite` 改为用 `SpriteLookup.uv`）
- Test: `loomgui_unity_package/Tests/MirrorPoolTests.cs` / `AtlasMirrorPoolTests.cs`（更新）

**Interfaces:**
- Consumes: `SpriteResolver.GetSprite` → `SpriteLookup`（Task 14）
- Produces: mesh UV 从 core 产的 `[0,1]` 线性映射到 `SpriteLookup.uv` rect。

- [ ] **Step 1: 更新 MirrorPool 取图路径**

把 `Sprite sp = sprites.GetSprite(path); tex = sp.texture;` 改为 `var look = sprites.GetSprite(path); if (look.found) { tex = look.tex; }`。`RemapMeshUvToSprite(ro, sp)` 改签名为 `RemapMeshUvToSprite(ro, look.uv)`——直接用 `look.uv`（打包算好的子区），删掉旧「取 sp.uv min/max 包围盒」逻辑。

- [ ] **Step 2: 更新映射函数**

```csharp
// core 产全图 UV [0,1]；线性映射到该 sprite 在 atlas 页内的子区 uvRect（打包算好）。
static void RemapMeshUvToSprite(RenderObject ro, Rect uvRect) {
    var uv = ro.UvList;
    for (int i = 0; i < uv.Count; i++) {
        uv[i] = new Vector2(
            uvRect.x + uv[i].x * uvRect.width,
            uvRect.y + uv[i].y * uvRect.height);
    }
    // 回写 mesh...
}
```

- [ ] **Step 3: 更新测试**

`MirrorPoolTests`/`AtlasMirrorPoolTests`：把注入 Unity `Sprite` 的用例改为注入 `SpriteLookup`（tex + uv rect）。断言映射后的 UV 落在子区内。

- [ ] **Step 4: 跑 EditMode 测试**

Expected: PASS。

- [ ] **Step 5: commit**

```bash
git add loomgui_unity_package/Runtime/MirrorPool.cs loomgui_unity_package/Tests/MirrorPoolTests.cs loomgui_unity_package/Tests/AtlasMirrorPoolTests.cs
git commit -m "feat(unity): MirrorPool linear UV remap from packed atlas rect (drop Sprite.uv bbox workaround)"
```

## Task 16: LoomStageDriver 自举 —— 读 runtime.json、灌尺寸、废 LoomSettings

**Files:**
- Modify: `loomgui_unity_package/Runtime/LoomStageDriver.cs`（启动流程改：读 runtime.json → load 包 → 读 atlas.json → set_image_sizes → SpriteResolver.Init(loadPage)）
- Modify: `loomgui_unity_package/Runtime/LoomStage.cs`（若持有 SpriteResolver.Init(LoomSettings) 调用，改为新签名）
- Delete: `loomgui_unity_package/Runtime/LoomSettings.cs`
- Test: `loomgui_unity_package/Tests/LoomStageDriverTests.cs`（更新）+ PlayMode 验收

**Interfaces:**
- Consumes: `ParseRuntime`/`ParseAtlas`（T13）, `SpriteResolver.Init`（T14）, `loomgui_stage_set_image_sizes`（T12）
- Produces: driver 启动自举流程；`LoadTexture(pageName)` / `LoadTextFile(name)` 钩子（替代 `LoadSpriteAtlas`）。

- [ ] **Step 1: 更新测试**

`LoomStageDriverTests`：验「读 runtime.json → 加载列出的包」逻辑、「合并 atlas.json 的 orig → 组 (path,w,h) 数组」逻辑（可抽成纯函数单测，不依赖 Unity 运行时）。

- [ ] **Step 2: 改 driver 启动流程**

```
1. LoadTextFile("loom.runtime.json") → ParseRuntime
2. foreach pkg in runtime.packages: LoadBytes("ui/"+pkg+".pkg.bin") → loomgui_stage_load_package
3. foreach atlas in runtime.atlases: LoadTextFile("atlas/"+atlas+".atlas.json") → ParseAtlas，收集
4. 合并所有 atlas.sprites 的 (key, orig[0], orig[1]) → 三个平行数组/marshaled → loomgui_stage_set_image_sizes
5. SpriteResolver.Init(atlasManifests, pageName => LoadTexture("atlas/"+pageName))
6. 字体：runtime.fonts → set_fallback_families + 加载 .bytes（沿用现有字体加载路径，familyName 从 runtime.json 拿）
7. 正常 tick
```

`LoadSpriteAtlas` 钩子删除，加 `LoadTexture(relPath)->Texture2D` + `LoadTextFile(relPath)->string` + `LoadBytes(relPath)->byte[]`（按后端 Resources/StreamingAssets/AB，沿用现有加载模式）。

- [ ] **Step 3: 删 LoomSettings.cs + 清引用**

删 `LoomSettings.cs`。grep `LoomSettings` 清所有运行时引用（`GetOrCreateDefault`/`GetDefault`/`SpriteResolver.Init(settings,...)`）。Editor 侧引用留到阶段四统一删（那些脚本本就要删）。

- [ ] **Step 4: 灌尺寸的 P/Invoke 调用**

C# 侧调 `loomgui_stage_set_image_sizes`：把 keys 组 `IntPtr[]`（`Marshal.StringToHGlobalAnsi` 每条）+ `uint[] ws` + `uint[] hs`，pin 后传指针 + count，调用后释放。（IL2CPP 注意：本调用一次性启动期，非热路径。）

- [ ] **Step 5: 跑 EditMode 测试 + 重编确认**

Run（Unity Test Runner）: 相关 EditMode 测试 PASS。
Run: `cargo build -p loomgui_ffi_c --release`（确保 dll 是最新，已含 T12 入口）。

- [ ] **Step 6: PlayMode 闭环验收**

用 CLI 打包一个 showcase 工作区 → 产物拷进 Unity 加载位置 → PlayMode 加载，逐项过：按钮/文本/图片渲染、图集图正确、九宫格、滚动、hover/active。（CLAUDE.md：SDD 后必跑 showcase PlayMode 逐项，CSS 语义集成只在 PlayMode 显现。）

- [ ] **Step 7: commit**

```bash
git add loomgui_unity_package/Runtime/LoomStageDriver.cs loomgui_unity_package/Runtime/LoomStage.cs loomgui_unity_package/Tests/LoomStageDriverTests.cs
git rm loomgui_unity_package/Runtime/LoomSettings.cs
git commit -m "feat(unity): driver bootstraps from loom.runtime.json; set_image_sizes; drop LoomSettings SO"
```

---

# 阶段三：Tauri GUI 壳

> Tauri 2.x + 原生 HTML/CSS/JS。Rust 后端 link `loomgui_pkg`，command 直接调库函数（不经 shell）。GUI 与 CLI 共享同一 `build()`。

## Task 17: loomgui_gui crate 骨架（Tauri 2.x）

**Files:**
- Create: `loomgui_gui/Cargo.toml`, `loomgui_gui/src/main.rs`, `loomgui_gui/tauri.conf.json`, `loomgui_gui/build.rs`
- Create: `loomgui_gui/dist/index.html`（占位前端）
- Modify: `Cargo.toml`（workspace members 加 `loomgui_gui`）
- Test: 编译 + 启动 smoke

**Interfaces:**
- Produces: 可启动的空 Tauri app（窗口标题 "LoomGUI Packer"，加载 `dist/index.html`）。

- [ ] **Step 1: workspace 加成员**

根 `Cargo.toml`：`members = ["loomgui_core", "loomgui_ffi_c", "loomgui_pkg", "loomgui_gui"]`。

- [ ] **Step 2: crate 骨架**

`loomgui_gui/Cargo.toml`（Tauri 2.x + 依赖 loomgui_pkg + dirs）：

```toml
[package]
name = "loomgui_gui"
version = "0.1.0"
edition = "2021"

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
tauri = { version = "2", features = [] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
dirs = "5"
loomgui_pkg = { path = "../loomgui_pkg" }
```

`build.rs`：`fn main() { tauri_build::build() }`。

`tauri.conf.json`：最小配置（app 名、窗口、`frontendDist: "dist"`、identifier）。

`src/main.rs`：

```rust
//! LoomGUI 打包器 GUI（Tauri 壳）。command 直接调 loomgui_pkg，与 CLI 同一 build()。

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![]) // Task 18 起填 commands
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

`dist/index.html`：占位 `<h1>LoomGUI Packer</h1>`。

- [ ] **Step 3: 编译**

Run: `cargo build -p loomgui_gui`
Expected: 编译通过（首次会拉 tauri 依赖）。

- [ ] **Step 4: commit**

```bash
git add loomgui_gui/ Cargo.toml Cargo.lock
git commit -m "feat(gui): loomgui_gui Tauri 2.x crate skeleton"
```

## Task 18: Tauri commands —— workspace 读写 + recent + build

**Files:**
- Create: `loomgui_gui/src/commands.rs`
- Create: `loomgui_gui/src/recent.rs`（~/.loomgui/recent.json）
- Modify: `loomgui_gui/src/main.rs`（注册 commands）
- Test: `recent.rs` 纯逻辑单测

**Interfaces:**
- Consumes: `loomgui_pkg::workspace::{load_workspace, save_workspace, Workspace}`, `loomgui_pkg::build::build`
- Produces（Tauri commands，均返 `Result<T, String>`）:
  - `recent_workspaces() -> Vec<String>`
  - `open_workspace(path: String) -> Workspace`（读 + 追加到 recent）
  - `create_workspace(path: String) -> Workspace`（建骨架 + 默认配置 + 注入 CLAUDE.md/skill，见 Task 21；本任务先建目录 + 默认 workspace.json）
  - `save_workspace(path: String, ws: Workspace) -> ()`
  - `scan_html(pkg_dir: String) -> Vec<String>`（扫顶层 .html）
  - `run_build(path: String) -> BuildReport`（调 `build`）
  - `recent.rs`: `fn load_recent() -> Vec<String>`, `fn push_recent(path: &str)`（去重 + 上限 10，写 `~/.loomgui/recent.json`）

- [ ] **Step 1: recent.rs + 测试**

```rust
//! 最近打开的工作区列表（~/.loomgui/recent.json）。跨平台用 dirs 定位 home。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Default)]
struct Recent {
    recent: Vec<String>,
}

fn recent_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".loomgui").join("recent.json"))
}

pub fn load_recent() -> Vec<String> {
    let Some(p) = recent_path() else { return vec![] };
    let Ok(text) = std::fs::read_to_string(&p) else { return vec![] };
    serde_json::from_str::<Recent>(&text).map(|r| r.recent).unwrap_or_default()
}

pub fn push_recent(path: &str) {
    let mut list = load_recent();
    list.retain(|p| p != path);
    list.insert(0, path.to_string());
    list.truncate(10);
    if let Some(p) = recent_path() {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&p, serde_json::to_string_pretty(&Recent { recent: list }).unwrap_or_default());
    }
}

/// 纯逻辑：把 path 提到列表首、去重、截断到 max。可单测不碰磁盘。
pub fn merge_recent(existing: &[String], path: &str, max: usize) -> Vec<String> {
    let mut list: Vec<String> = existing.iter().filter(|p| *p != path).cloned().collect();
    list.insert(0, path.to_string());
    list.truncate(max);
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn merge_dedup_and_cap() {
        let start = vec!["a".to_string(), "b".to_string()];
        let r = merge_recent(&start, "b", 10);
        assert_eq!(r, vec!["b", "a"], "b 提到首 + 去重");
        let capped = merge_recent(&(0..15).map(|i| i.to_string()).collect::<Vec<_>>(), "x", 10);
        assert_eq!(capped.len(), 10);
        assert_eq!(capped[0], "x");
    }
}
```

- [ ] **Step 2: commands.rs**

每个 command `#[tauri::command]`，返 `Result<_, String>`，直接转调 `loomgui_pkg` 函数（path `String` → `Path`）。`open_workspace`/`create_workspace` 成功后调 `recent::push_recent`。`create_workspace` 本任务写默认 `Workspace { version:1, output_dir:"../dist", packages:[], atlases:[], fonts:[] }` + `create_dir_all`。

- [ ] **Step 3: 注册 commands**

`main.rs` 的 `generate_handler![]` 填入全部 command 名。

- [ ] **Step 4: 测试 + 编译**

Run: `cargo test -p loomgui_gui recent`
Run: `cargo build -p loomgui_gui`
Expected: PASS + 编译通过。

- [ ] **Step 5: commit**

```bash
git add loomgui_gui/src/commands.rs loomgui_gui/src/recent.rs loomgui_gui/src/main.rs
git commit -m "feat(gui): tauri commands (workspace load/save, recent, build) + recent list"
```

## Task 19: 前端 —— 启动屏（最近工作区 + 新建/打开）

**Files:**
- Modify: `loomgui_gui/dist/index.html`
- Create: `loomgui_gui/dist/app.js`, `loomgui_gui/dist/style.css`

**Interfaces:**
- Consumes: commands `recent_workspaces`, `open_workspace`, `create_workspace`（Task 18）
- Produces: 启动屏 UI —— 最近工作区卡片列表 + 「新建工作区」「打开工作区」按钮（用 Tauri dialog 选目录）。选中后进主界面（Task 20）。

- [ ] **Step 1: 启动屏 HTML/JS**

`app.js` 用 `window.__TAURI__.core.invoke('recent_workspaces')` 拉列表渲染卡片；「打开」用 `window.__TAURI__.dialog.open({directory:true})` 选目录 → `invoke('open_workspace',{path})`；「新建」选目录 → `invoke('create_workspace',{path})`。成功后 `renderMain(ws, path)`。

- [ ] **Step 2: 启动 smoke**

Run: `cargo run -p loomgui_gui`（本机 desktop）
Expected: 窗口显示启动屏，最近列表（首次空）+ 两按钮可点。

- [ ] **Step 3: commit**

```bash
git add loomgui_gui/dist/
git commit -m "feat(gui): start screen (recent workspaces + new/open)"
```

## Task 20: 前端 —— 主界面四区（工作区/包/图集/字体）+ 拖拽 + 改即存 + 打包

**Files:**
- Modify: `loomgui_gui/dist/app.js`, `loomgui_gui/dist/index.html`, `loomgui_gui/dist/style.css`

**Interfaces:**
- Consumes: `save_workspace`, `scan_html`, `run_build`（Task 18）, Tauri `onDragDrop` 事件
- Produces: 主界面 —— 四区编辑 + 拖拽建包/加图集目录/加字体 + 任一字段改动即 `save_workspace` + 「打包」按钮调 `run_build` 显示日志。

- [ ] **Step 1: 四区渲染**

- **工作区**：output_dir 编辑（改即存）。
- **包**：列表每包 name/dirs/html（html 三态：空显示 `scan_html` 结果为灰显「自动」，手动增删则填充 `pkg.html` 固化 + 提供「恢复自动扫」清空回 `[]`）。拖目录到「建包」区 → name=目录名、dirs=[相对根]。
- **图集**：列表每 atlas name/default/standalone/dirs/max_size/padding。拖目录进 dirs。
- **字体**：列表每 font family/file/default(radio)/fallback。拖字体文件 → file=相对根、family=文件名去扩展。

拖拽用 `window.__TAURI__.event.listen('tauri://drag-drop', ...)` 拿绝对路径，前端算相对工作区根（或加个 `relativize(path)` command 交 Rust 算，稳妥——避免前端路径逻辑跨平台踩坑）。**加 command** `relativize(root, abs) -> String`（Task 18 可补，或本任务在 commands.rs 追加）。

- [ ] **Step 2: 改即存**

任一字段 change → 收集当前 UI 状态成 `Workspace` → `invoke('save_workspace',{path,ws})`。

- [ ] **Step 3: 打包按钮**

调 `invoke('run_build',{path})` → 日志区显示 `BuildReport.log` + 成功/失败。

- [ ] **Step 4: smoke（本机）**

Run: `cargo run -p loomgui_gui`
用一个真实工作区：建包、拖目录进图集、改字段确认写回 json、点打包看产物 + 日志。

- [ ] **Step 5: commit**

```bash
git add loomgui_gui/dist/ loomgui_gui/src/commands.rs
git commit -m "feat(gui): main editor (4 sections, drag-drop, save-on-change, build button)"
```

## Task 21: create_workspace 注入 CLAUDE.md + skill

**Files:**
- Modify: `loomgui_gui/src/commands.rs`（`create_workspace` 补注入）
- Create: `loomgui_gui/templates/workspace-CLAUDE.md`, `loomgui_gui/templates/skill/SKILL.md`（模板，含新配置格式 + `loom-pkg build` 用法 + img src 相对 html 约定）

**Interfaces:**
- Produces: 新建工作区时在根写 `CLAUDE.md` + `.claude/skills/loomgui-editor/SKILL.md`（从模板拷，含围栏引用）。

- [ ] **Step 1: 模板文件**

`workspace-CLAUDE.md`：说明工作区结构、`loom.workspace.json` 各字段、`loom-pkg build <此目录>` 一条命令打包、img src 相对 html 写法、sprite_key 概念。`skill/SKILL.md`：AI 编辑工作流（改 html/css → 调 build → 看错误自纠）+ 围栏引用（拷现有 `fence.md` 或引用）。

- [ ] **Step 2: create_workspace 拷模板**

用 `include_str!` 嵌入模板内容，`create_workspace` 时写到工作区根 + `.claude/skills/loomgui-editor/`。

- [ ] **Step 3: 编译 + smoke**

Run: `cargo build -p loomgui_gui`；新建一个工作区确认 CLAUDE.md + skill 落地。

- [ ] **Step 4: commit**

```bash
git add loomgui_gui/src/commands.rs loomgui_gui/templates/
git commit -m "feat(gui): create_workspace injects CLAUDE.md + loomgui-editor skill from templates"
```

## Task 22: 跨平台可执行文件产出 + 打包配置

**Files:**
- Modify: `loomgui_gui/tauri.conf.json`（bundle 配置：Win/mac/Linux target）
- Create: `docs/superpowers/plans/gui-build-notes.md`（构建命令记录，可选）

**Interfaces:**
- Produces: `cargo tauri build` 产出各平台可执行文件（本机 Windows 先验 `.exe`）。

- [ ] **Step 1: bundle 配置**

`tauri.conf.json` 的 `bundle` 段配 identifier、图标、targets。

- [ ] **Step 2: 本机构建验证**

Run: `cargo tauri build`（或 `cargo build -p loomgui_gui --release`）
Expected: 产出 Windows 可执行文件，双击能开、能打包一个工作区。

- [ ] **Step 3: commit**

```bash
git add loomgui_gui/tauri.conf.json
git commit -m "chore(gui): cross-platform bundle config"
```

---

# 阶段四：Unity 拆除

> 工作区完全出 Unity，Unity 只留「加载产物 + 拉起 GUI 按钮」。删所有旧编辑器脚本。

## Task 23: 删旧编辑器脚本

**Files:**
- Delete: `loomgui_unity_package/Editor/LoomSettingsWindow.cs`, `LoomAtlasSync.cs`, `LoomConfigExporter.cs`, `LoomWorkspaceInitializer.cs`, `PkgManifestReader.cs`, `LoomExePath.cs`, `LoomJsonEscape.cs`, `LoomWorkspaceAssetPostprocessor.cs`
- Delete: 对应 `loomgui_unity_package/Tests/` 里的 `LoomConfigExporterTests.cs`, `LoomAtlasSyncTests.cs`
- Modify: 若有 asmdef 引用需清理

**Interfaces:**
- Produces: Editor 目录只剩阶段四 Task 24 的「Open Packer」脚本。

- [ ] **Step 1: 删文件**

```bash
git rm loomgui_unity_package/Editor/LoomSettingsWindow.cs \
       loomgui_unity_package/Editor/LoomAtlasSync.cs \
       loomgui_unity_package/Editor/LoomConfigExporter.cs \
       loomgui_unity_package/Editor/LoomWorkspaceInitializer.cs \
       loomgui_unity_package/Editor/PkgManifestReader.cs \
       loomgui_unity_package/Editor/LoomExePath.cs \
       loomgui_unity_package/Editor/LoomJsonEscape.cs \
       loomgui_unity_package/Editor/LoomWorkspaceAssetPostprocessor.cs \
       loomgui_unity_package/Tests/LoomConfigExporterTests.cs \
       loomgui_unity_package/Tests/LoomAtlasSyncTests.cs
```
连带删对应 `.meta` 文件（Unity）。

- [ ] **Step 2: 清残留引用**

grep `LoomSettingsWindow`/`LoomAtlasSync`/`LoomConfigExporter`/`PkgManifestReader`/`LoomExePath`/`LoomJsonEscape` 确认无残留 using/引用。若 asmdef 或其他脚本引用则清理。

- [ ] **Step 3: 编译确认（Unity 或 dotnet）**

Unity 重新编译无错（家里机/本机 Unity），或至少 grep 干净。

- [ ] **Step 4: commit**

```bash
git commit -m "chore(unity): remove obsolete editor scripts (settings/atlas-sync/config-exporter/manifest-reader)"
```

## Task 24: 「Open Packer」MenuItem

**Files:**
- Create: `loomgui_unity_package/Editor/LoomOpenPacker.cs`

**Interfaces:**
- Produces: `[MenuItem("LoomGUI/Open Packer")]` —— 按平台定位 GUI 可执行文件并 `Process.Start`。

- [ ] **Step 1: 写 MenuItem**

```csharp
using System.Diagnostics;
using System.IO;
using UnityEditor;
using UnityEngine;

namespace LoomGUI.Editor
{
    /// LoomGUI 打包器 GUI 拉起入口（工作区/图集/字体配置全在独立 Tauri app 里，不再进 Unity）。
    public static class LoomOpenPacker
    {
        [MenuItem("LoomGUI/Open Packer")]
        public static void Open()
        {
            string exe = ResolveExe();
            if (!File.Exists(exe))
            {
                UnityEngine.Debug.LogError($"[LoomGUI] 打包器 GUI 未找到：{exe}。请先构建 loomgui_gui 或在设置里配置路径。");
                return;
            }
            Process.Start(new ProcessStartInfo(exe) { UseShellExecute = true });
        }

        /// 按平台定位 GUI 可执行文件。约定放插件包 Editor/Tools/ 下。
        static string ResolveExe()
        {
            string toolsDir = Path.Combine(
                Path.GetDirectoryName(Application.dataPath) ?? ".",
                "Packages/com.loomgui.unity/Editor/Tools");
#if UNITY_EDITOR_WIN
            return Path.Combine(toolsDir, "loomgui_gui.exe");
#elif UNITY_EDITOR_OSX
            return Path.Combine(toolsDir, "loomgui_gui.app/Contents/MacOS/loomgui_gui");
#else
            return Path.Combine(toolsDir, "loomgui_gui");
#endif
        }
    }
}
```

- [ ] **Step 2: 编译确认**

Unity 编译无错；菜单出现 `LoomGUI/Open Packer`。

- [ ] **Step 3: commit**

```bash
git add loomgui_unity_package/Editor/LoomOpenPacker.cs
git commit -m "feat(unity): 'Open Packer' menu item to launch standalone GUI"
```

---

# 阶段五：文档更新防漂移

> 遵循 CLAUDE.md 防漂移三原则：文档写定性、关键 claim 有测试护、改代码后 grep docs/ 清过期引用。

## Task 25: main-design + fence + pkg 格式版本引用同步

**Files:**
- Modify: `docs/design/main-design.md`（图集/资源管线节：Unity 管 → Rust 自绘）
- Modify: `docs/pitfalls.md`（若新踩坑：pkg v15 迁移、image_sizes 改源）
- 检查: grep `docs/` 里 pkg 版本号（14）、`asset_manifest`、`SpriteAtlas`、`LoomSettings`、`res_dir` 的过期引用

**Interfaces:** 纯文档。

- [ ] **Step 1: 改 main-design 资源管线节**

图集从「交 Unity Sprite Atlas」改「Rust 自绘产 atlas.png+atlas.json」；工作区从 `Assets/LoomUI/` 内改「完全独立目录」；尺寸源从「pkg.bin manifest」改「atlas.json + set_image_sizes」。定性描述，不写具体列数/字段数。

- [ ] **Step 2: grep 过期引用**

```bash
grep -rniE "asset_manifest|SpriteAtlas|LoomSettings|res_dir|formatVersion.*1[24]|version=1[24]" docs/
```
逐条核实并更新（pkg 版本号引用改 15、图集叙述改自绘、工作区叙述改独立）。

- [ ] **Step 3: commit**

```bash
git add docs/design/main-design.md docs/pitfalls.md
git commit -m "docs: sync main-design resource pipeline to self-drawn atlas + independent workspace"
```

## Task 26: 根 CLAUDE.md + roadmap 更新

**Files:**
- Modify: `CLAUDE.md`（构建命令加 loomgui_gui；图集打包链改 Rust；Unity 编辑器脚本废弃说明；工作区独立）
- Modify: `docs/roadmap/roadmap.md`（§3 v other 标旧实现被取代 + 代号 v1.8+；G1 图集归 Rust）

**Interfaces:** 纯文档。

- [ ] **Step 1: 改 CLAUDE.md**

- 构建命令区加 `cargo build -p loomgui_gui` / `cargo run -p loomgui_gui` / `loom-pkg build <workspace>`。
- 「Rust → Unity .dll 闭环」区保留；加「图集自绘：CLI/GUI 产 atlas.png+atlas.json，Unity 不再打图集」。
- 架构图资源管线：图集归 Rust。
- 废弃 `LoomSettingsWindow` 等编辑器脚本的说明。

- [ ] **Step 2: 改 roadmap**

§3「v other — 编辑器工作流」：标 `LoomSettingsWindow`/`LoomWorkspaceInitializer`/`LoomConfigExporter` 被本设计取代；代号 v1.8+；引用 spec 路径。G1 打包器描述：图集打包归 Rust 自绘。

- [ ] **Step 3: commit**

```bash
git add CLAUDE.md docs/roadmap/roadmap.md
git commit -m "docs: update CLAUDE.md build commands + roadmap v1.8+ standalone packer"
```

## Task 27: 更新记忆 + 工作区 skill 定稿

**Files:**
- Modify: `C:\Users\yuanwentao01\.claude\projects\F--WorkSpace-projects-LoomGUI\memory\v1-4-package-refactor-direction.md`（图集：交 Unity → Rust 自绘；工作区：Assets/ 内 → 完全独立）
- Modify: `MEMORY.md`（若指针描述需更新）
- 检查: `loomgui_gui/templates/` 的 skill 模板与最终配置格式一致（Task 21 已建，这里定稿校对）

**Interfaces:** 记忆 + 模板。

- [ ] **Step 1: 更新记忆文件**

`v1-4-package-refactor-direction.md` 加一段：v1.8+ 推翻「图集交 Unity」→ Rust 自绘（atlas.png+atlas.json）；工作区从 `Assets/LoomUI/` 内 → 完全独立目录 + `loom.workspace.json`；后端读 `loom.runtime.json` 自举，废 `LoomSettings` SO。链接 `[[design-over-effort]]`。

- [ ] **Step 2: 校对 skill 模板**

确认 `loomgui_gui/templates/skill/SKILL.md` 的配置字段名（dirs/html/atlases/fonts/file）+ `loom-pkg build` 用法 + img src 相对 html 约定，与最终实现一致。

- [ ] **Step 3: commit（模板部分；记忆文件在 memory 目录不入 git）**

```bash
git add loomgui_gui/templates/
git commit -m "docs(gui): finalize workspace skill template to match shipped config format"
```

---

## 附录：验收清单（全阶段完成后）

- [ ] `cargo test`（全 workspace）绿。
- [ ] `cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings` 绿。
- [ ] `loom-pkg build <showcase 工作区>` 产出 `ui/*.pkg.bin` + `atlas/*.png|*.atlas.json` + `fonts/*.bytes` + `loom.runtime.json`。
- [ ] Tauri GUI 能新建/打开工作区、建包、配图集/字体、改即存、一键打包。
- [ ] Unity PlayMode 加载新产物：showcase 逐项渲染正确（按钮/文本/图片/图集图/九宫格/滚动/hover/active）。
- [ ] Unity「LoomGUI/Open Packer」拉起 GUI。
- [ ] docs/ 无 pkg v14/`asset_manifest`/`SpriteAtlas`/`LoomSettings`/`res_dir` 过期引用。
- [ ] `.dll` + `LoomGUIBindings.cs` 已重编 commit。
