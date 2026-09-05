//! 资源宿主（host）与树实例（Stage）的分层。
//!
//! 重资源——字体表（含驻留字节）、字形 atlas、包池、图尺寸表——归属 `ResourceHost`；
//! 树/输入/动画/时钟等每实例态留在 `Stage`。单 Stage 行为不变（`Stage::new` 自建宿主）；
//! 多 Stage 通过 `Stage::new_bound` 挂同一 `Rc<RefCell<ResourceHost>>` 共享一份驻留
//! （字体字节一份、glyph atlas 一份、包解析一份），这是 per-Stage 固定成本从 N 份降回
//! 1 份的地基。FFI 面：`yio_host_*` 操作宿主，`yio_stage_new_bound(host, w, h)` 挂接。

use crate::asset::Package;
use crate::text::atlas::GlyphAtlas;
use crate::text::layout::FontTable;

/// 跨 Stage 共享的资源宿主。
///
/// - `fonts`：family → Face 表（含驻留字体字节；`Font` drop 时回收，无进程泄漏）。
/// - `glyph_atlas`：字形 R8 页集（只增不重排——旧字形 UV 永不变）。共享时
///   `GlyphKey.font_id` 的命名空间随 FontTable 一起归宿主（host 内唯一即可，
///   无跨表撞键）。
/// - `packages` / `last_pkg_load_version`：pkg 模板池 + 版本错配诊断。
/// - `image_sizes`：atlas.json 归一化 path → (w,h)。workspace 级数据，天然宿主侧。
/// - `generation`：注册表失效钩。`register_font` / `set_fallback_families` /
///   `set_image_sizes` 这类宿主侧变更**不经过任何场景 mutation**——taffy 与 measure
///   缓存都无感（Text 的 MeasureContext 只存 family 名，重注册同名换 id 时 ctx 不变）。
///   每次变更 +1；实例在 tick 前对账，不等即强制文本节点失效重测
///   （清 measure 缓存两表 + taffy `mark_dirty` 文本叶子）。
#[derive(Default)]
pub struct ResourceHost {
    pub fonts: FontTable,
    pub glyph_atlas: GlyphAtlas,
    pub packages: std::collections::HashMap<String, Package>,
    /// 最近一次 load_package 版本错配时 pkg 声明的格式版本（0=无记录/非版本错）。
    pub last_pkg_load_version: u32,
    pub image_sizes: std::collections::HashMap<String, (u32, u32)>,
    pub generation: u64,
}

impl ResourceHost {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册表代数 +1（宿主资源构成变化）。实例 tick 前对账。
    pub fn bump_generation(&mut self) {
        self.generation += 1;
    }
}

/// 解析 pkg.bin 存进宿主包池（`Stage::load_package` 与 FFI 宿主级入口共用体）。
/// 版本错配记录进 `last_pkg_load_version`（FFI 诊断「Unity 包与 yio.exe 同版本
/// 重打」的专属指引）；成功/其他错误置 0。
pub fn load_package_into(
    host: &mut ResourceHost,
    name: &str,
    bytes: &[u8],
) -> Result<(), crate::stage::LoadPkgError> {
    use crate::asset::{PkgError, MAX_VERSION, MIN_VERSION};
    use crate::stage::LoadPkgError;
    let parsed = crate::asset::read_package(bytes).map_err(|e| match e {
        PkgError::TooOld(v) => LoadPkgError::TooOld {
            pkg: v,
            min: MIN_VERSION,
        },
        PkgError::TooNew(v) => LoadPkgError::TooNew {
            pkg: v,
            max: MAX_VERSION,
        },
        other => LoadPkgError::Malformed(other.to_string()),
    });
    match parsed {
        Ok(mut pkg) => {
            pkg.name = name.to_string(); // read_package 填空串，覆盖为真实包名
            host.bump_generation();
            host.packages.insert(name.to_string(), pkg);
            host.last_pkg_load_version = 0; // 成功清残留（上一次错配值不得跨成功装载存活）
            Ok(())
        }
        Err(e) => {
            host.last_pkg_load_version = match &e {
                LoadPkgError::TooOld { pkg, .. } | LoadPkgError::TooNew { pkg, .. } => *pkg,
                LoadPkgError::Malformed(_) => 0,
            };
            Err(e)
        }
    }
}
