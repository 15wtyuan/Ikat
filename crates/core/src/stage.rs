//! Stage 层：串起 parse → style → scene → layout → render 的端到端入口。
//!
//! 资源池模型：`load_package(name, bytes)` 进 `packages` 字典不建 scene；
//! scene 由 `create_root`/`create_node` 建（动态树 API）。`tick_and_render` 跑
//! solve + build_render_nodes。`render_json` serde 序列化产渲染 JSON。

use crate::input::{EventRecord, PointerEvent, PointerState};
use crate::layout::solve;

use crate::render::FrameData;
use crate::scene::node::{NodeFlags, NodeId, NodeKind, Rect, Scene};
use crate::style::dynamic::{rematch_pseudo_classes, sync_animation_players, ScopedRule};
use crate::style::resolved::OverflowMode;

// 宿主经 Stage 命名空间消费光标决策（FFI 返回判别值）；裸 use 已在上方供本模块用。
pub use crate::input::CursorIntent;

/// transition 自动提交 tween 的 tag（rematch 检测通道变化 → drain kill 旧 + 提交新）。
/// 区分 driver 主动注册的 tween（caller-supplied u32 tag）——使 transition 完成事件可识别。
/// 选 0xFFFF_FFFE 哨兵（接近 u32 上限，避开常见 driver 小整数 tag）。
const TRANSITION_TAG: u32 = 0xFFFF_FFFE;

/// `Stage::measure_text` 的输出：布局前纯文本预估（无节点、不进树）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextMetrics {
    pub width: f32,
    pub height: f32,
    /// 断行后的行数（不换行测量恒 1，空文本 0）。
    pub line_count: u32,
}

/// `Stage::measure_text` 的错误（FFI 判别码 -2/-3 的来源）。
#[derive(Debug, Clone, PartialEq)]
pub enum MeasureTextError {
    UnknownFamily(String),
    InvalidParams(String),
}

pub struct Stage {
    pub scene: Option<Scene>,
    /// 共享资源宿主（字体表 / 字形 atlas / 包池 / 图尺寸表 / 注册表代数）。
    /// `new` 自建独占宿主（单 Stage 行为与拆分前一致）；`new_bound` 挂外部宿主——
    /// 多 Stage 共享一份字体驻留与 glyph atlas。单线程 FFI 纪律：一个 Stage 的
    /// tick/资源调用运行到完成才轮到下一个，RefCell 重入 = bug 探测器。
    pub host: std::rc::Rc<std::cell::RefCell<crate::host::ResourceHost>>,
    /// 上次 tick 对账看到的宿主 `generation`。不等 = 宿主注册表变过（register_font /
    /// set_fallback / set_image_sizes 都不经场景 mutation），tick 前强制文本失效重测。
    host_generation_seen: u64,
    pub root_size: (f32, f32),
    /// viewport inset 四边 design px [top, right, bottom, left]（env(safe-area-inset-*)
    /// 的取值源）。数值 = root 覆盖 unsafe 屏区的深度：Fit 贴物理边 → 真实 inset，
    /// Letterbox → 恒 0。宿主经 FFI `ikat_stage_set_safe_area` 按 adapt 结果注入；
    /// 默认全 0（桌面/无刘海 = 无 unsafe 区）。与 root_size 同类：Stage 级环境输入，
    /// 不从包来。（packages/image_sizes 等资源字段已迁 ResourceHost——#109 宿主分离。）
    pub safe_insets: [f32; 4],
    /// 单指针状态机（hover/active 状态 + 命中 diff + 产事件）。
    pub pointer_state: PointerState,
    /// set_input 缓存的本帧输入；tick_and_render 消费后 clear。
    pub pending_input: Vec<PointerEvent>,
    /// 本帧 tick 产出的事件序列（process 返回）；last_events/borrow_events 读。
    pub last_events: Vec<EventRecord>,
    /// set_key_input 缓存的本帧键盘输入；tick 消费后 clear。
    pub pending_keys: Vec<crate::input::KeyEvent>,
    /// set_text_input 缓存的本帧字符输入（UTF-32 codepoints，已 shift-mapped）。
    /// tick 消费（插进聚焦 TextField/TextArea）后 clear。与 pending_keys 互补：
    /// keydown 通道走物理键（KeyEvent），textinput 通道走可打印字符（已映射好的 codepoint）。
    pub pending_text_input: Vec<u32>,
    /// set_wheel_input 缓存的本帧滚轮输入；tick 消费（apply_wheel_to_hit）后 clear。
    /// 累积式（extend，非 clear-then-set）——多组滚轮合并到一帧。
    pub pending_wheel: Vec<crate::scroll::WheelEvent>,
    /// 编程聚焦/清焦点请求（request_focus/blur tick 外调记，tick 最前消费）。
    /// 外层 Some=有请求；内层 Some(id)=聚焦某节点 / None=清焦点。
    pub pending_focus_request: Option<Option<NodeId>>,
    /// FFI setter（set_control_text 等）产的事件缓冲。这些 setter 在 tick 外调用，
    /// 不能直接写 last_events（下 tick 会 clear 覆盖）。tick_and_render 最前把它 drain
    /// 进本帧 out，使 setter 产的事件在下一 tick 入 last_events（与 C# 读事件节奏一致）。
    pub pending_events: Vec<EventRecord>,
    /// tween 引擎（每 tick update 写 scene.anim + 产 complete 事件）。
    pub tweens: crate::tween::TweenManager,
    /// advance_time stash 的本帧 dt（tick_and_render 消费，喂 tweens.update）。
    pub pending_dt: f32,
    /// 上帧每节点 (header_hash, payload_hash)（node_id 键）。跨 tick 持续，供
    /// build_render_nodes 比较定 ChangeLevel。transient 不进 pkg（Stage 字段非 Scene 字段）。
    /// reload/节点数变 → clear → 下帧全 dirty（无基线）。
    pub prev_node_hashes: std::collections::HashMap<u64, (u64, u64)>,
    /// A2 增量 render build 缓存（输入指纹 → 上帧产物）。present-set 签名变自动清空；
    /// 新 scene 也清（ensure_scene）。transient 不进 pkg。
    pub render_cache: crate::render::dirty::RenderBuildCache,
    /// 单调帧号（每 tick +1）。增量指纹的 nonce（控件壳永不命中路径）。
    pub frame_no: u64,
    /// A2 增量开关（false = 每帧清空 render 缓存 = 全量重建，等价拆分前行为）。
    /// A/B 对拍测试用（同脚本两 Stage 对比逐帧输出必须全等）。
    pub incremental_render: bool,
    /// 全局 ListView 序号分配器（reuse_key 命名空间隔离用，见 list::encode_reuse_key）。
    /// 每个 ListView 首次进入数据驱动模式时取一个唯 ordinal，确保多 List 的 slot reuse_key
    /// 在场景级全局命名空间不冲突。Stage 持有（跨 tick 单调递增，重置场景也不回卷）。
    pub next_list_ordinal: u32,
}

/// 加载包失败的结构化错误。版本错配单列——宿主须给「Unity 包与 ikat.exe 同版本
/// 重打」的专属指引，不能与普通损坏混在一条文案里。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadPkgError {
    TooOld { pkg: u32, min: u32 },
    TooNew { pkg: u32, max: u32 },
    Malformed(String),
}

impl std::fmt::Display for LoadPkgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadPkgError::TooOld { pkg, min } => {
                write!(f, "package formatVersion {pkg} too old (min {min})")
            }
            LoadPkgError::TooNew { pkg, max } => {
                write!(f, "package formatVersion {pkg} too new (max {max})")
            }
            LoadPkgError::Malformed(s) => write!(f, "{s}"),
        }
    }
}

/// 宿主注册表变更后的文本失效：清 measure 缓存两表 + taffy `mark_dirty` 文本叶子。
///
/// 为什么必须显式失效：字体注册/回退链/图尺寸变更都发生在宿主上，不产生任何场景
/// mutation——Text 的 MeasureContext 只存 family 名（重注册同名换 font id 时 ctx
/// 逐字节相同），taffy 值比较短路后不会重测，measure 缓存与 text_layouts 全部陈旧。
/// mark_dirty 沿祖先上溯，下帧 solve 强制重跑文本叶子闭包；指纹含字体 id 链与
/// color，重测自动取新注册表。image_sizes 变更同理影响 Image intrinsic（ctx 会在
/// sync 重算时自然 diff，这里一并脏化无害——注册表变更是低频事件）。
fn invalidate_text_after_host_change(scene: &mut Scene) {
    for slot in scene.text_measure_cache.iter_mut() {
        *slot = None;
    }
    for slot in scene.text_layouts.iter_mut() {
        *slot = None;
    }
    let ids: Vec<Option<(NodeId, taffy::NodeId)>> = scene.layout_cache.ids.clone();
    let mut to_dirty: Vec<taffy::NodeId> = Vec::new();
    for (scene_id, taffy_id) in ids.into_iter().flatten() {
        let is_text = scene
            .get(scene_id)
            .map(|n| n.kind == crate::scene::NodeKind::TextNode || n.rich_text_block)
            .unwrap_or(false);
        if is_text {
            to_dirty.push(taffy_id);
        }
    }
    let tree = &mut scene.layout_cache.tree;
    for taffy_id in to_dirty {
        let _ = tree.mark_dirty(taffy_id);
    }
}

impl Stage {
    pub fn new(root_size: (f32, f32)) -> Result<Self, String> {
        Self::new_bound(
            std::rc::Rc::new(std::cell::RefCell::new(crate::host::ResourceHost::new())),
            root_size,
        )
    }

    /// 挂外部宿主建 Stage（多 Stage 共享一份资源驻留）。宿主生命周期由调用方管理
    /// （FFI：`ikat_host_new`/`ikat_host_free`）；Stage drop 只放 Rc 引用，最后一个
    /// 引用者 drop 时资源释放。`new` = 自建独占宿主的等价便捷入口。
    pub fn new_bound(
        host: std::rc::Rc<std::cell::RefCell<crate::host::ResourceHost>>,
        root_size: (f32, f32),
    ) -> Result<Self, String> {
        let host_generation_seen = host.borrow().generation;
        Ok(Stage {
            scene: None,
            host,
            host_generation_seen,
            root_size,
            safe_insets: [0.0; 4],
            pointer_state: PointerState::new(),
            pending_input: Vec::new(),
            last_events: Vec::new(),
            pending_keys: Vec::new(),
            pending_text_input: Vec::new(),
            pending_wheel: Vec::new(),
            pending_focus_request: None,
            pending_events: Vec::new(),
            tweens: crate::tween::TweenManager::new(),
            pending_dt: 0.0,
            prev_node_hashes: std::collections::HashMap::new(),
            render_cache: crate::render::dirty::RenderBuildCache::default(),
            frame_no: 0,
            incremental_render: true,
            next_list_ordinal: 0,
        })
    }

    /// 注册字体进宿主字体表（共享宿主 = 所有挂接 Stage 可见）。is_default=true 设为
    /// 默认（measure 的 fallback）。FFI 层在首次 tick 前必须注册至少一个 default 字体，
    /// 否则 measure 时 select panic。注册表变更走 generation 失效钩（见 ResourceHost）。
    pub fn register_font(
        &mut self,
        family: &str,
        bytes: Vec<u8>,
        is_default: bool,
    ) -> Result<(), String> {
        let mut host = self.host.borrow_mut();
        host.bump_generation();
        host.fonts.register(family, bytes, is_default)
    }

    /// 运行时改画布尺寸（分辨率适配 / 窗口 resize / 横竖屏切换）。solve 每帧跑
    /// （taffy 树重建），改完下帧布局即按新 root_size 重排——vw/vh 声明与 % 自动跟随。
    /// 拒绝非有限或 ≤0（保持原值不动，Err 由 FFI 转 -1）。
    pub fn set_root_size(&mut self, w: f32, h: f32) -> Result<(), String> {
        if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 {
            return Err(format!("set_root_size: invalid size {w}x{h}"));
        }
        self.root_size = (w, h);
        Ok(())
    }

    /// 设 viewport inset（env(safe-area-inset-*) 的值，design px [top,right,bottom,left]）。
    /// FFI `ikat_stage_set_safe_area` 按适配结果算好后走这里；拒绝非有限或负值
    /// （语义上 inset 是深度，不为负）。设完下帧 rematch/propagate + solve 即生效
    /// （env() 声明跟随，无需显式触发）。
    pub fn set_safe_insets(&mut self, insets: [f32; 4]) -> Result<(), String> {
        if insets.iter().any(|v| !v.is_finite() || *v < 0.0) {
            return Err(format!("set_safe_insets: invalid insets {insets:?}"));
        }
        self.safe_insets = insets;
        Ok(())
    }

    /// 设全局字体回退链。families 须已 register（未注册的 FontTable 内部跳过）。
    /// 主字体缺字时按序 probe 这些 family，首个含该字的补上（RmlUi fallback 模型）。
    /// 空切片清空回退（退回单字体）。source-agnostic：只收 family 名，后端把系统字体
    /// register 进来后照样能用——核心不问字体来源。
    pub fn set_fallback_families(&mut self, families: &[String]) {
        let mut host = self.host.borrow_mut();
        host.bump_generation();
        host.fonts.set_fallback_families(families);
    }

    /// 取走缺字诊断报告（tofu 取证）：pick 全链缺字的 (family, char)，会话级去重。
    /// 宿主每帧 tick 后调（隔帧也不丢——pending 累积，去重集会话级持久）。
    pub fn take_missing_glyph_reports(&mut self) -> Vec<String> {
        self.host.borrow_mut().fonts.take_missing_glyph_reports()
    }

    /// 无节点纯文本测量：字符串 + 字体 + 字号 → 宽高 + 行数。布局前预估用
    /// （tips 预分行 / 飘字宽估 / 按钮自适应宽——消灭业务侧手数字数）。
    ///
    /// `max_width <= 0` 不换行（单行宽度）；`> 0` 按该宽断行。断行与 solve 内
    /// 文本测量走同一条 `measure_text`，预估即所见。行高按 normal（字号 ×
    /// NORMAL_LINE_HEIGHT）、字距 0、常规字重——与缺省样式节点一致。
    /// family 未注册 → Err（不静默 fallback 到默认字体：拿错字体估宽没有意义）。
    pub fn measure_text(
        &self,
        text: &str,
        family: &str,
        size_px: f32,
        max_width: f32,
    ) -> Result<TextMetrics, MeasureTextError> {
        if !self.host.borrow().fonts.contains_family(family) {
            return Err(MeasureTextError::UnknownFamily(format!(
                "measure_text: family `{family}` not registered (register_font it first; \
                 measure must use the same font that will render)"
            )));
        }
        if !size_px.is_finite() || size_px <= 0.0 {
            return Err(MeasureTextError::InvalidParams(format!(
                "measure_text: invalid font size {size_px}"
            )));
        }
        if !max_width.is_finite() {
            return Err(MeasureTextError::InvalidParams(format!(
                "measure_text: invalid max_width {max_width}"
            )));
        }
        let host = self.host.borrow();
        let stack = host.fonts.stack_for(Some(family));
        let layout = crate::text::layout::measure_text(
            text,
            size_px,
            0.0,
            0.0,
            crate::style::resolved::TextAlign::Left,
            crate::text::layout::WrapControl::default(),
            (max_width > 0.0).then_some(max_width),
            &stack,
            [0.0, 0.0, 0.0, 1.0],
            crate::text::rich::RichWeight::Normal,
        );
        Ok(TextMetrics {
            width: layout.text_width,
            height: layout.text_height,
            line_count: layout.lines.len() as u32,
        })
    }

    /// 加载包进资源池（不碰 scene）。重复 load 同名包 = 替换。多包共存。
    ///
    /// `load_package(name, bytes)` 解析 pkg.bin → Package，存进 `self.packages[name]`。
    /// **不建 scene**——加载与实例化解耦（fgui/Unity prefab 模型）。
    /// scene 由 `create_root`/`create_node` 建；组件实例化由 `instantiate` 做。
    /// `root_size` 归 Stage（不从包来）；图集归 Unity（核心不知图集）。
    ///
    /// **图尺寸**：由 `set_image_sizes` 在运行时灌入（来自 atlas.json），不再从包 manifest 自建。
    pub fn load_package(&mut self, name: &str, bytes: &[u8]) -> Result<(), LoadPkgError> {
        crate::host::load_package_into(&mut self.host.borrow_mut(), name, bytes)
    }

    /// 最近一次 load_package 失败的 pkg 声明格式版本（0=无/非版本错）。配 FFI 返回码 1/2。
    pub fn last_pkg_load_version(&self) -> u32 {
        self.host.borrow().last_pkg_load_version
    }

    /// 卸载包：从资源池移除模板注册（Unity prefab 删除语义——已实例化的活节点是
    /// 独立副本，不受影响；持有旧模板句柄再实例化会报「组件不在包内」）。
    ///
    /// 只动模板注册表。atlas 纹理与字体不在此列：atlas 是 workspace 级共享资源
    /// （runtime.json 的 atlases 列表跨包并行、SpriteResolver 全局懒缓存），字体是
    /// driver 级注册，二者都不隶属任何包——卸载单个包既无可释放也无可破坏的资源。
    /// 未加载的包名 → Err。
    pub fn unload_package(&mut self, name: &str) -> Result<(), String> {
        let mut host = self.host.borrow_mut();
        host.bump_generation();
        host.packages
            .remove(name)
            .map(|_| ())
            .ok_or_else(|| format!("package `{name}` is not loaded"))
    }

    /// 查图尺寸（path → (w, h) 像素）。供 layout/render 用。
    /// path 缺失或 w/h=0 → None（调用方 fallback 64×64）。
    pub fn image_size(&self, path: &str) -> Option<(u32, u32)> {
        self.host
            .borrow()
            .image_sizes
            .get(path)
            .copied()
            .filter(|(w, h)| *w != 0 && *h != 0)
    }

    /// 批量灌图尺寸（后端读所有 atlas.json 合并后一次性推入）。
    /// 覆盖式合并：同 path 后写赢。上万条也是 O(n) HashMap 插入，启动一次调用。
    pub fn set_image_sizes(&mut self, sizes: &[(String, u32, u32)]) {
        let mut host = self.host.borrow_mut();
        host.bump_generation();
        for (path, w, h) in sizes {
            host.image_sizes.insert(path.clone(), (*w, *h));
        }
    }

    /// 缓存本帧指针输入（tick 前调；覆盖式——每帧全量替换 pending_input）。
    pub fn set_input(&mut self, events: &[PointerEvent]) {
        self.pending_input.clear();
        self.pending_input.extend_from_slice(events);
    }

    /// 业务设节点 disabled（伪类源 + active/click 抑制）。悬空 NodeId 静默跳过。
    pub fn set_node_disabled(&mut self, node_id: NodeId, disabled: bool) {
        if let Some(scene) = self.scene.as_mut() {
            if let Some(n) = scene.get_mut(node_id) {
                if disabled {
                    n.interaction.flags.insert(NodeFlags::DISABLED);
                } else {
                    n.interaction.flags.remove(NodeFlags::DISABLED);
                }
            }
        }
    }

    /// 业务设节点 touchable（CSS `pointer-events` 的运行时面：false = 本节点不参与
    /// 命中，子节点照常可命中——透传语义同 CSS）。写两处：interaction.touchable 是
    /// hit_test 的判据（立即生效）；base_style.touchable 是 rematch 的重起源（不写则
    /// 下次伪类重匹配会把它冲回打包期 CSS 值）。悬空 NodeId 静默跳过。
    pub fn set_node_touchable(&mut self, node_id: NodeId, touchable: bool) {
        if let Some(scene) = self.scene.as_mut() {
            if let Some(n) = scene.get_mut(node_id) {
                n.interaction.touchable = touchable;
                n.base_style.touchable = touchable;
            }
        }
    }

    /// 业务设节点运行时可获焦性（公共 Node.Focusable 后端）。true → tabindex=Some(0)
    /// （进 Tab 链 0 组，DOM native 序）；false → Some(-1)（Tab 链/点击聚焦排除；编程
    /// `request_focus` 不查 tabindex，仍可强制聚焦——DOM tabindex=-1 语义）。只写
    /// interaction：rematch 无 tabindex 通道（规则层不重起源），运行时值不被伪类
    /// 重匹配冲掉。悬空 NodeId 静默跳过。
    pub fn set_node_focusable(&mut self, node_id: NodeId, focusable: bool) {
        if let Some(scene) = self.scene.as_mut() {
            if let Some(n) = scene.get_mut(node_id) {
                n.interaction.tabindex = Some(if focusable { 0 } else { -1 });
            }
        }
    }

    /// 业务设节点 draggable（公共 Node.Draggable 的后端；HTML `draggable` 属性的
    /// 运行时面）。true = 节点参与 drag_target 候选（pointer-down 后 DragStart/Move/End
    /// 事件链的使能开关）。只写 interaction：draggable 无 rematch 通道（规则层不
    /// 重起源），运行时值不被伪类重匹配冲掉。悬空 NodeId 静默跳过。
    pub fn set_node_draggable(&mut self, node_id: NodeId, draggable: bool) {
        if let Some(scene) = self.scene.as_mut() {
            if let Some(n) = scene.get_mut(node_id) {
                n.interaction.draggable = draggable;
            }
        }
    }

    /// 业务设 TabList 激活模型（公共 TabList.Activation 的后端；HTML
    /// `data-activation="manual"` 属性的运行时面）。true = manual（方向键只移焦点、
    /// Enter/Space 提交选中）；false = automatic（缺省，焦点跟随选中即时提交）。
    /// 返回是否生效（非 TabList 控件态 / 悬空 NodeId → false）。
    pub fn set_tab_activation(&mut self, node_id: NodeId, manual: bool) -> bool {
        if let Some(scene) = self.scene.as_mut() {
            if let Some(crate::scene::node::ControlState::TabList {
                manual_activation, ..
            }) = scene.controls.get_mut(node_id)
            {
                *manual_activation = manual;
                return true;
            }
        }
        false
    }

    /// 读 TabList 激活模型（manual=true / automatic=false）。非 TabList 控件态 /
    /// 悬空 NodeId → None。
    pub fn get_tab_activation(&self, node_id: NodeId) -> Option<bool> {
        self.scene
            .as_ref()
            .and_then(|s| match s.controls.get(node_id) {
                Some(crate::scene::node::ControlState::TabList {
                    manual_activation, ..
                }) => Some(*manual_activation),
                _ => None,
            })
    }

    /// 按 CSS id 属性查节点（首个匹配）。无 scene / 无匹配 → None。
    /// 供 FFI find_node_by_id：业务用 id 定位节点（注册 listener / 设 disabled）。
    pub fn find_node_by_id(&self, id: &str) -> Option<NodeId> {
        self.scene.as_ref().and_then(|s| s.find_by_id_attr(id))
    }

    /// 在 root 子树内 DFS 查找 id 属性匹配的首个节点（self-exclusive：从 root 的直接子开始，
    /// root 自身 id_attr 不参与匹配，与 DOM querySelectorAll/Query<T> 一致）。
    /// 供 FFI find_node_by_id_in_subtree：组件/slot 内部作用域 id 查找。
    pub fn find_node_by_id_in_subtree(&self, root: NodeId, id: &str) -> Option<NodeId> {
        self.scene
            .as_ref()
            .and_then(|s| s.find_node_by_id_in_subtree(root, id))
    }

    /// UI 挡住时游戏不响应点击。委托 PointerState（任一活跃槽命中非根）。
    pub fn is_pointer_on_ui(&self) -> bool {
        match &self.scene {
            None => false,
            Some(scene) => self.pointer_state.is_pointer_on_ui(scene),
        }
    }

    /// 软件指针形态决策（#93）。逻辑主体在 `PointerState::cursor_intent`
    /// （与 `is_pointer_on_ui` 同款委托模式）；此处为宿主侧稳定入口。
    pub fn cursor_intent(&self) -> CursorIntent {
        match &self.scene {
            None => CursorIntent::Arrow,
            Some(scene) => self.pointer_state.cursor_intent(scene),
        }
    }

    /// 加 touch monitor（C# CaptureTouch 后经 FFI 调）。
    pub fn add_touch_monitor(&mut self, touch_id: i32, node: NodeId) {
        self.pointer_state.add_touch_monitor(touch_id, node);
    }
    /// 移除 touch monitor（C# 主动释放经 FFI 调）。
    pub fn remove_touch_monitor(&mut self, node: NodeId) {
        self.pointer_state.remove_touch_monitor(node);
    }

    /// 累积时间（C# 传 Time.unscaledDeltaTime；双击窗口用）。
    pub fn advance_time(&mut self, dt: f32) {
        self.pointer_state.time_s += dt;
        self.pending_dt = dt;
    }

    /// 外部取消待 click（照 fgui CancelClick）。FFI cancel_click 转发。
    pub fn cancel_click(&mut self, touch_id: i32) {
        self.pointer_state.cancel_click(touch_id);
    }

    /// 缓存本帧键盘输入（tick 前调；覆盖式）。
    pub fn set_key_input(&mut self, keys: &[crate::input::KeyEvent]) {
        self.pending_keys.clear();
        self.pending_keys.extend_from_slice(keys);
    }

    /// 缓存本帧字符输入（tick 前调；覆盖式）。codepoints 为已 shift-mapped 的 UTF-32。
    /// 后端把按键事件映射成可打印字符后由此注入；tick 在处理 keydown 后把 codepoints
    /// 插进聚焦的 TextField/TextArea。非聚焦/非文本控件 → 字符静默丢弃（无副作用）。
    pub fn set_text_input(&mut self, codepoints: &[u32]) {
        self.pending_text_input.clear();
        self.pending_text_input.extend_from_slice(codepoints);
    }

    /// 设文本控件的 IME composition（后端读 Input.compositionString 回灌）。pos 是 composition
    /// 在 value 中的字节偏移。非文本控件 / 越界 node → no-op（不 panic）。下一帧 measure/render
    /// 会把 composition 拼进显示文本（[`ikat_core::scene::control::display_value`]）。
    ///
    /// NumberField 也接受 composition（预编辑期**不**过滤——composition 是 provisional，
    /// 用户可能还在组字；过滤发生在 [`commit_composition`] 落定时）。
    pub fn set_composition(&mut self, node: NodeId, text: &str, _pos: usize) {
        use crate::scene::node::ControlState;
        let Some(scene) = self.scene.as_mut() else {
            return;
        };
        // TextField/TextArea/NumberField 有 EditState；ControlState match 过滤非文本控件。
        // set_composition 原语不收 kind（与 insert_text 不同）——预编辑串原样存，不过滤
        // （「filter only at commit」约定）。
        if let Some(
            ControlState::TextField(e)
            | ControlState::TextArea(e)
            | ControlState::NumberField { edit: e, .. },
        ) = scene.controls.get_mut(node)
        {
            // composition 插入点 = 当前光标（IME 组字在光标处）。FFI 的 pos 参数由 C# 后端
            // 传 0 忽略——后端不知道 cursor byte offset，core 用 e.cursor 最准。
            crate::scene::control::set_composition(e, text, e.cursor);
        }
    }

    /// 提交文本控件的 composition（落定进 value）。返 true = 有 composition 且 value 改变；
    /// false = 无 composition（或插入被 readonly/max_length 拒）。非文本控件 / 越界 node → false。
    ///
    /// NumberField：提交时把 composition.text 用 [`filter_number_field_text`] 滤成数字语法
    /// 字符再落定（照「filter only at commit」约定——预编辑期不过滤，落定时滤）。
    pub fn commit_composition(&mut self, node: NodeId) -> bool {
        use crate::scene::node::ControlState;
        use crate::scene::node::NodeKind;
        let Some(scene) = self.scene.as_mut() else {
            return false;
        };
        let kind = scene.get(node).map(|n| n.kind);
        let Some(kind) = kind else {
            return false;
        };
        if let Some(ControlState::TextField(e) | ControlState::TextArea(e)) =
            scene.controls.get_mut(node)
        {
            return crate::scene::control::commit_composition(e, kind);
        }
        if let Some(ControlState::NumberField { edit, .. }) = scene.controls.get_mut(node) {
            // 先把 provisional composition.text 滤成数字语法字符，再走原 commit 路径。
            // 全被滤掉 → 空串 → commit_composition 原语 insert_text no-op → 返 false（不改值）。
            if let Some(comp) = edit.composition.as_mut() {
                comp.text = crate::input::filter_number_field_text(&comp.text);
            }
            return crate::scene::control::commit_composition(edit, NodeKind::NumberField);
        }
        false
    }

    /// 缓存本帧滚轮输入（tick 前调；**累积式** extend——多组滚轮合并）。
    /// wire 进 tick 消费（apply_wheel_to_hit）。
    pub fn set_wheel_input(&mut self, events: &[crate::scroll::WheelEvent]) {
        self.pending_wheel.extend_from_slice(events);
    }

    /// 编程滚动到指定位置。非 scroll 容器 / 越界 node → no-op（不 panic）。
    /// animated=false 直接 snap+clamp；true 启 cubic-out tween（调 set_pos）。
    pub fn set_scroll_pos(&mut self, node: NodeId, x: f32, y: f32, animated: bool) {
        if let Some(scene) = self.scene.as_mut() {
            if scene.get(node).is_some() {
                if let Some(s) = scene.scroll.get_mut(node) {
                    s.set_pos((x, y), animated);
                }
            }
        }
    }

    /// driver 注入滚动容器 content_size（虚拟列表用）。覆盖子节点 AABB 自动算。
    /// refresh_content_sizes 跳过此容器。node 无效/非滚动容器 → no-op（不 panic）。
    /// **中间态**：set 后至下次 refresh 前 viewport_size/overlap 为 (0,0)——
    /// driver 正常流程 set→tick→读，不要在 set 后同帧写 scroll_pos（会被 clamp 到 0）。
    pub fn set_content_size(&mut self, node: NodeId, w: f32, h: f32) {
        if let Some(scene) = self.scene.as_mut() {
            let n = scene.get(node);
            let is_scroll = n
                .map(|n| {
                    n.style.overflow_x != OverflowMode::Visible
                        || n.style.overflow_y != OverflowMode::Visible
                })
                .unwrap_or(false);
            if !is_scroll {
                return;
            }
            let st = scene.scroll.ensure(node);
            st.content_size = (w, h);
            st.content_size_overridden = true;
            st.viewport_size = (0.0, 0.0); // refresh 会填
            st.overlap = (0.0, 0.0); // refresh 会填
        }
    }

    /// 清除 driver 注入的 content_size override，让核心重回子节点 AABB 自动算。
    /// 列表销毁 / 退回普通滚动时调。node 无效/非滚动容器 → no-op（不 panic）。
    pub fn clear_content_size_override(&mut self, node: NodeId) {
        if let Some(scene) = self.scene.as_mut() {
            if let Some(st) = scene.scroll.get_mut(node) {
                st.content_size_overridden = false;
            }
        }
    }

    /// 读 scroll_pos。node 无效/非滚动容器 → None。
    pub fn get_scroll_pos(&self, node: NodeId) -> Option<(f32, f32)> {
        self.scene.as_ref()?.scroll.get(node).map(|s| s.scroll_pos)
    }

    /// 读节点 layout_rect（solve 产物）。driver 测 itemSize 用。node 无效 → None。
    pub fn get_node_layout_rect(&self, node: NodeId) -> Option<Rect> {
        self.scene.as_ref()?.get(node).map(|n| n.layout_rect)
    }

    /// 节点语义类型（围栏 tag + 结构属性决定，CSS 不改变）。None = 节点不存在。
    pub fn get_node_kind(&self, node: NodeId) -> Option<crate::scene::node::NodeKind> {
        self.scene.as_ref()?.get(node).map(|n| n.kind)
    }

    /// cascade 解析后的非几何样式快照（`Node.style`，rematch 覆写值）。None = 节点不存在。
    /// 几何（w/h/x/y）走 `get_node_layout_rect`；internal set-ness/复杂视觉不暴露。
    pub fn get_node_computed_style(
        &self,
        node: NodeId,
    ) -> Option<crate::style::computed::ComputedNodeStyle> {
        self.scene
            .as_ref()?
            .get(node)
            .map(|n| crate::style::computed::ComputedNodeStyle::from_resolved(&n.style))
    }

    /// 读节点 world transform（compute_world_transforms 产物，全节点含空 div）。
    /// NativeHost FFI 查询用——merge_meshes 后空 div slot 的 RenderNode 消失，
    /// 但 world_transforms 保留全节点（与 node_sort_keys 同）。node 无效 / scene 未建 → None。
    pub fn get_node_world_matrix(&self, node: NodeId) -> Option<crate::transform::Affine2> {
        let scene = self.scene.as_ref()?;
        scene.get(node)?; // gen 校验（slotmap 代际）；失效 → None
        scene.world_transforms.get(node.index()).copied()
    }

    /// 读文本控件光标的世界矩形（IME 候选窗定位用，照 Unity Input.compositionCursorPos）。
    ///
    /// 几何与 render arm 画光标同源：layout 空间 caret = `{rect.x + off_left + cx,
    /// rect.y + line.y, 1.0, line.height}`，再经节点 world transform 投到世界。display 取
    /// [`ikat_core::scene::control::display_value`]（含 composition 拼接），与 measure 缓存的
    /// TextLayout 同源；无缓存（首帧/空 value）→ None（后端 fallback 到节点 layout_rect）。
    ///
    /// 有 composition 时候选窗锁在 composition 的 display 起点（IME 候选窗锁在 composition，
    /// 不是原始光标；用 raw `e.cursor` 会因 composition 拼进 display 后光标字节偏移平移而偏早）。
    ///
    /// 非文本控件 / 节点无效 / scene 未建 → None。
    pub fn cursor_rect(&self, node: NodeId) -> Option<crate::scene::node::Rect> {
        use crate::render::resolve_lp;
        use crate::scene::control::{display_value_masked, value_to_display_byte};
        use crate::scene::node::ControlState;
        use crate::scene::text_cursor::{cursor_pixel_x, line_byte_ranges};
        let scene = self.scene.as_ref()?;
        let n = scene.get(node)?;
        let e = match scene.controls.get(node)? {
            ControlState::TextField(e)
            | ControlState::TextArea(e)
            | ControlState::NumberField { edit: e, .. } => e,
            _ => return None,
        };
        // display 须与 measure_text_controls 缓存同源（含 composition + 掩码），否则光标字节
        // 偏移对不上缓存的 ranges。缓存为空（空 value/placeholder/首帧）→ 无法定位，返 None。
        let layout = scene.text_layouts.get(node.index())?.as_ref()?.clone();
        let mask = n.style.text_security.map(crate::scene::control::mask_char);
        let (display, comp_range) = display_value_masked(e, mask);
        if display.is_empty() {
            return None;
        }
        let ranges = line_byte_ranges(&layout, &display);
        // IME 候选窗锁在 composition 处（而非原始光标字节偏移）——composition 拼进 display 后
        // 光标的 display 偏移会随 comp.text.len() 平移，按原始 e.cursor 取位会偏早。有
        // composition 时用其 display 起点（comp_range），无 composition 时退回光标（掩码下经
        // 字符数换算进 display 字节空间）。
        let cur = match comp_range {
            Some((start, _)) => start,
            None => value_to_display_byte(&e.value, &display, e.cursor),
        };
        let (cx, li) = cursor_pixel_x(&layout, &ranges, cur);
        let line = layout.lines.get(li)?;
        let off_left = resolve_lp(n.style.taffy_style.border.left)
            + resolve_lp(n.style.taffy_style.padding.left);
        let wm = scene
            .world_transforms
            .get(node.index())
            .copied()
            .unwrap_or(crate::transform::IDENTITY);
        // 与 render arm 光标（render/mod.rs）同源：纯平移用 wm[4,5] 作 rect 世界原点
        // （layout_rect 已是绝对 design 坐标，再 apply_point 会双重计数 → x 翻倍，IME 候选窗
        // 偏到屏外）；scale/rotate 用局部原点（0,0）后 apply_point 投世界（render arm 走 push
        // 的 wm transform，MirrorPool scale/rotate 进 _ObjectMatrix）。
        let pure = crate::transform::is_pure_translation(&wm);
        let (rx, ry) = if pure { (wm[4], wm[5]) } else { (0.0, 0.0) };
        // layout 空间 caret 矩形（与 render arm 同公式）。view_x = 单行水平视口
        // （光标跟随滚动）——IME 候选窗须跟随可视光标而非 layout 光标。
        let lx = rx + off_left + cx - e.view_x;
        let ly = ry + line.y;
        let lw = 1.0_f32;
        let lh = line.height;
        let (x0, y0) = if pure {
            (lx, ly)
        } else {
            crate::transform::apply_point(&wm, lx, ly)
        };
        let (x1, y1) = if pure {
            (lx + lw, ly + lh)
        } else {
            crate::transform::apply_point(&wm, lx + lw, ly + lh)
        };
        Some(crate::scene::node::Rect {
            x: x0.min(x1),
            y: y0.min(y1),
            w: (x1 - x0).abs(),
            h: (y1 - y0).abs(),
        })
    }

    /// 读节点 sort_key（assign_sort_keys 在 merge_meshes 前的 DFS 序号快照）。
    /// NativeHost FFI 查询用——merge 后空 div entry 消失，回 scene.node_sort_keys 兜底。
    /// node 无效 / scene 未建 → None。
    pub fn get_node_sort_key(&self, node: NodeId) -> Option<u32> {
        let scene = self.scene.as_ref()?;
        scene.get(node)?;
        scene.node_sort_keys.get(node.index()).copied()
    }

    /// 节点可见性：节点存在 + 非 display:none。remove_node / scene 未建 / display:none → false。
    /// NativeHost FFI 查询用。display 走 taffy::Display（ResolvedStyle.taffy_style.display）。
    pub fn get_node_visible(&self, node: NodeId) -> bool {
        let scene = match self.scene.as_ref() {
            Some(s) => s,
            None => return false,
        };
        match scene.get(node) {
            None => false,
            Some(n) => !matches!(n.style.taffy_style.display, taffy::Display::None),
        }
    }

    /// 节点 disabled 伪类态（`NodeFlags::DISABLED`）。无 scene / 节点缺失 → false。
    /// 业务读当前 disabled 态用（伪类级联查询出口，与 `set_node_disabled` 对称）。
    pub fn get_node_disabled(&self, node: NodeId) -> bool {
        let scene = match self.scene.as_ref() {
            Some(s) => s,
            None => return false,
        };
        match scene.get(node) {
            None => false,
            Some(n) => n.interaction.flags.contains(NodeFlags::DISABLED),
        }
    }

    /// 拉脏页 page_idx 列表（写入 out，返实际数）。atlas 未用 / 无 scene → 0。
    pub fn font_atlas_dirty_pages(&self, out: &mut [u32]) -> usize {
        let host = self.host.borrow();
        let dirty = host.glyph_atlas.dirty_pages();
        let n = dirty.len().min(out.len());
        out[..n].copy_from_slice(&dirty[..n]);
        n
    }

    /// 读某页：尺寸 + R8 像素。buf_len 不够返所需大小（双调法），够则写 buf 返字节数。
    /// 无此页 → 返 0（out_w/out_h 不写）。
    pub fn font_atlas_page(
        &self,
        page: u32,
        out_w: &mut u32,
        out_h: &mut u32,
        out: &mut [u8],
    ) -> usize {
        let host = self.host.borrow();
        let (bytes, w, h) = host.glyph_atlas.page_bytes(page);
        let needed = (w * h) as usize;
        if out.len() < needed {
            return needed; // 双调：caller 扩 buf 重调
        }
        out[..needed].copy_from_slice(bytes);
        *out_w = w;
        *out_h = h;
        needed
    }

    /// 清脏页（backend 拉完调）。
    pub fn font_atlas_clear_dirty(&mut self) {
        self.host.borrow_mut().glyph_atlas.clear_dirty();
    }

    /// 编程聚焦（照 fgui RequestFocus）。强制聚焦任意非 disabled 节点
    /// （含 tabindex=None/-1——request_focus 是编程 API，不查 tabindex）。
    /// disabled 拒 / 越界跳过。记 pending_focus_request，下 tick 最前消费（不直接写 last_events）。
    pub fn request_focus(&mut self, node_id: NodeId) {
        if let Some(scene) = self.scene.as_ref() {
            match scene.get(node_id) {
                None => return,
                Some(n) if n.interaction.flags.contains(NodeFlags::DISABLED) => return,
                _ => {}
            }
        } else {
            return;
        }
        self.pending_focus_request = Some(Some(node_id));
    }

    /// 编程清焦点。记 pending_focus_request = Some(None)，下 tick 消费。
    pub fn blur(&mut self) {
        self.pending_focus_request = Some(None);
    }

    /// 注册 tween（spec 形态；链式 builder 见 `tween_builder`）。
    /// duration<=0 → update 首帧即结束并产 complete。无 scene / 越界 node → update 跳过（不报错）。
    pub fn tween(&mut self, node: NodeId, spec: crate::tween::TweenSpec) {
        self.tweens.tween(node, spec);
    }

    /// 链式 tween builder 入口：
    /// `stage.tween_builder(node, TweenProp::Opacity).from(&[0.]).to(&[1.]).duration(0.3).start()`
    /// from/to 取前 `prop_value_size` 个分量（越界分量忽略，长度不足补 0）。
    pub fn tween_builder(
        &mut self,
        node: NodeId,
        prop: crate::tween::TweenProp,
    ) -> TweenBuilder<'_> {
        TweenBuilder {
            stage: self,
            node,
            spec: crate::tween::TweenSpec::new(prop, [0.0; 8], [0.0; 8]),
        }
    }

    /// 停该节点该 prop 的 tween（override 保留末值）。
    pub fn kill_tween(&mut self, node: NodeId, prop: crate::tween::TweenProp) {
        self.tweens.kill(node, prop);
    }

    /// 清该节点所有动画 override（回 CSS）。
    pub fn clear_anim(&mut self, node: NodeId) {
        if let Some(scene) = self.scene.as_mut() {
            scene.anim.clear_node(node);
        }
    }

    /// 清该节点某 prop 对应通道（回 CSS）。
    pub fn clear_anim_prop(&mut self, node: NodeId, prop: crate::tween::TweenProp) {
        if let Some(scene) = self.scene.as_mut() {
            scene.anim.clear_prop(node, prop);
        }
    }

    /// 删节点（递归删子 + 联动清 anim/scroll/tween + slotmap remove）。
    /// NodeId 此后失效（gen++）。无 scene / 失效节点 → no-op。
    /// 删节点联动清持久附属 map，防悬空 NodeId 残留。
    pub fn remove_node(&mut self, node: NodeId) {
        if let Some(scene) = self.scene.as_mut() {
            crate::scene::dynamic::remove_node(scene, &mut self.tweens, node);
        }
    }

    /// scene 不存在则建空骨架（首次 create_root/create_node 调用时初始化）。
    /// scene 初始由 create_root 建（load_package 不建 scene）。
    /// 多次调用幂等（已存在 scene → no-op）。`pub(crate)` 供集成测试直接初始化场景
    /// （如黄金等价测试需 instantiate 后把孤立根 push 进 scene.roots，不套额外 stage_root）。
    pub(crate) fn ensure_scene(&mut self) {
        if self.scene.is_none() {
            self.scene = Some(crate::scene::node::Scene::default());
            self.prev_node_hashes.clear(); // 新 scene → 无基线，下帧全 dirty
            self.render_cache.entries.clear(); // 新 scene → 产物缓存全失效
        }
    }

    /// 建根节点：create_node + roots.push(id)。返回新 NodeId。
    /// scene 不存在则首次调用建空骨架（scene 初始由 create_root 建）。
    pub fn create_root(&mut self, kind: &str, css: &str) -> Result<NodeId, String> {
        self.ensure_scene();
        let scene = self.scene.as_mut().unwrap();
        crate::scene::dynamic::create_root(scene, kind, css)
    }

    /// 建节点（不挂父）：kind_from_tag + apply_css 填 base_style + slotmap insert。
    /// 返回新 NodeId，需配合 append_child/insert_before 挂到树。
    /// scene 不存在则首次调用建空骨架。
    pub fn create_node(&mut self, kind: &str, css: &str) -> Result<NodeId, String> {
        self.ensure_scene();
        let scene = self.scene.as_mut().unwrap();
        crate::scene::dynamic::create_node(scene, kind, css)
    }

    /// 挂子到 parent 末尾。child 必须当前无父。
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), String> {
        crate::scene::dynamic::append_child(self.scene.as_mut().ok_or("no scene")?, parent, child)
    }

    /// 在 parent.children 中 ref_id 之前插 child。ref_id=INVALID → 末尾追加。
    pub fn insert_before(
        &mut self,
        parent: NodeId,
        child: NodeId,
        ref_id: NodeId,
    ) -> Result<(), String> {
        crate::scene::dynamic::insert_before(
            self.scene.as_mut().ok_or("no scene")?,
            parent,
            child,
            ref_id,
        )
    }

    /// 摘子（不删节点）：从 parent.children 移除 + child.parent=None。节点仍 live 可重挂。
    pub fn remove_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), String> {
        crate::scene::dynamic::remove_child(self.scene.as_mut().ok_or("no scene")?, parent, child)
    }

    /// 改 Text 节点 content + 标 dirty_text。
    pub fn set_text(&mut self, node: NodeId, text: &str) -> Result<(), String> {
        crate::scene::dynamic::set_text(self.scene.as_mut().ok_or("no scene")?, node, text)
    }

    /// 重启子树内声明式动画（class 触发 keyframes）。node 不 live → Err。
    pub fn restart_animations(&mut self, node: NodeId) -> Result<(), String> {
        let scene = self.scene.as_mut().ok_or("no scene")?;
        if !scene.nodes.contains_key(node.to_key()) {
            return Err("node not live".into());
        }
        crate::scene::animation::restart_animations(scene, node);
        Ok(())
    }

    /// 改 Image 节点 src + 标 dirty_mesh。
    pub fn set_src(&mut self, node: NodeId, src: &str) -> Result<(), String> {
        crate::scene::dynamic::set_src(self.scene.as_mut().ok_or("no scene")?, node, src)
    }

    /// 运行时渲染隐藏（世界锚点出屏/相机背后自动隐藏）。与 display:none 正交：不影响
    /// 布局/命中，只控本节点全部渲染行 visible 位（后端保留镜像对象、SetActive(false)）。
    pub fn set_node_render_hidden(&mut self, node: NodeId, hidden: bool) -> Result<(), String> {
        crate::scene::dynamic::set_node_render_hidden(
            self.scene.as_mut().ok_or("no scene")?,
            node,
            hidden,
        )
    }

    /// world-space 挂载登记（#109 C8）：把 node 子树标记为挂载到业务摆放的 3D 容器。
    /// slot 由 driver 分配保证唯一（0 = 解除挂载回屏幕空间）；挂载子树渲染行顶点 re-base
    /// 到挂载根局部系（见 render::mount_rebase），blob mount_id 列写 slot 供后端路由。
    /// v1 约束（机器门）：挂载子树内禁 Dropdown（浮层臂不参与挂载 re-base，会整层落回
    /// 屏幕空间）与非 Visible overflow（clip 平面定义在屏幕系，挂到 3D 后无意义——
    /// render 侧会把挂载行 mask 清 0，这里前置拒绝防「声明的裁剪静默失效」）。
    /// node 不 live / 无 scene → Err。
    pub fn set_node_mount(&mut self, node: NodeId, slot: u32) -> Result<(), String> {
        let scene = self.scene.as_mut().ok_or("no scene")?;
        if scene.get(node).is_none() {
            return Err("node not live".into());
        }
        if slot != 0 {
            let mut bad: Option<String> = None;
            let mut stack = vec![node];
            while let (Some(cur), None) = (stack.pop(), &bad) {
                let n = scene.get(cur).expect("walk live");
                if matches!(
                    n.kind,
                    crate::scene::node::NodeKind::Dropdown
                        | crate::scene::node::NodeKind::OptionItem
                ) {
                    bad = Some("dropdown/option inside mount".into());
                    break;
                }
                if n.style.overflow_x != crate::style::resolved::OverflowMode::Visible
                    || n.style.overflow_y != crate::style::resolved::OverflowMode::Visible
                {
                    bad = Some("overflow (clip/scroll) inside mount".into());
                    break;
                }
                stack.extend(n.children.iter().copied());
            }
            if let Some(reason) = bad {
                return Err(format!("mount rejected (v1): {reason}"));
            }
        }
        if slot == 0 {
            scene.mounts.remove(&node);
        } else {
            scene.mounts.insert(node, slot);
        }
        Ok(())
    }

    /// 写 inline override（便签层，优先级 > 动态规则 > base_style）。node 不 live / 无 scene → Err。
    pub fn set_inline_override(&mut self, node: NodeId, css: &str) -> Result<(), String> {
        crate::scene::dynamic::set_inline_override(
            self.scene.as_mut().ok_or("no scene")?,
            node,
            css,
        )
    }

    /// 清 inline override 的某 prop bit（值保留但下帧 rematch 不再应用）。node 不 live / 无 scene → Err。
    pub fn unset_inline_override(&mut self, node: NodeId, prop: &str) -> Result<(), String> {
        crate::scene::dynamic::unset_inline_override(
            self.scene.as_mut().ok_or("no scene")?,
            node,
            prop,
        )
    }

    /// 读节点子节点数。无 scene / 节点不 live → None。
    pub fn get_child_count(&self, node: NodeId) -> Option<usize> {
        self.scene
            .as_ref()
            .and_then(|s| crate::scene::dynamic::get_child_count(s, node))
    }

    /// 读节点子节点列表（clone Vec）。无 scene / 节点不 live → None；叶子 → Some(vec![])。
    pub fn get_children(&self, node: NodeId) -> Option<Vec<NodeId>> {
        self.scene
            .as_ref()
            .and_then(|s| crate::scene::dynamic::get_children(s, node))
    }

    /// 加 class（重复名不重复 push）+ 标 dirty_mesh。node 不 live / 无 scene → Err。
    pub fn add_class(&mut self, node: NodeId, name: &str) -> Result<(), String> {
        crate::scene::dynamic::add_class(self.scene.as_mut().ok_or("no scene")?, node, name)
    }

    /// 移除 class（全部匹配）+ 标 dirty_mesh。node 不 live / 无 scene → Err。
    pub fn remove_class(&mut self, node: NodeId, name: &str) -> Result<(), String> {
        crate::scene::dynamic::remove_class(self.scene.as_mut().ok_or("no scene")?, node, name)
    }

    /// 查询 class 是否存在。无 scene / 节点不 live → None。
    pub fn has_class(&self, node: NodeId, name: &str) -> Option<bool> {
        self.scene
            .as_ref()
            .and_then(|s| crate::scene::dynamic::has_class(s, node, name))
    }

    /// 设渲染复用键（虚拟列表 slot）。node 无效 → no-op。
    pub fn set_reuse_key(&mut self, node: NodeId, key: u32) {
        if let Some(scene) = self.scene.as_mut() {
            crate::scene::dynamic::set_reuse_key(scene, node, key);
        }
    }

    /// 从包克隆一个组件进当前 scene，返回组件根 NodeId（孤立，parent=None，调用方 append_child 挂载）。
    ///
    /// 1. 查 `packages[pkg].components[component]`，clone 出 ComponentTemplate（避开 packages/scene 双借）。
    /// 2. 遍历 template.nodes，按 parent_idx 序建 live Node（父先建于子），复用节点构造
    ///    （`create_node_from_template`：kind + baked style → base_style/style 初始 + clip_rect +
    ///    dirty_text + slotmap insert + id 回填），再填 classes/id_attr/draggable/tabindex。
    ///    按 parent_idx 串子树（append_child 语义：parent.children.push + child.parent=Some(parent)）。
    ///    根（parent_idx=None）不串父，记录返回。
    /// 3. 作用域规则包装：遍历 template.dynamic_rules.rules，每条包装成 ScopedRule
    ///    （scope_root = 实例根），push 进 scene.dynamic_rules.entries。不再按 selector 去重——
    ///    同模板多实例各带独立 scope_root，rematch 按 scope 隔离匹配
    ///    （Shadow DOM 风格：后代选择器不穿透实例边界）。hit_test 返具体 NodeId → 各实例独立 :hover。
    /// 4. scene 必须已存在（create_root 建过），否则 Err。
    ///
    /// 多实例独立：同组件多次 instantiate → 各自独立子树（NodeId 不同）+ 各自独立事件/伪类命中。
    /// id_attr 多实例约定限制：find_node_by_id 返首个匹配（不做核心 id 去重，YAGNI）。
    pub fn instantiate(&mut self, pkg: &str, component: &str) -> Result<NodeId, String> {
        let scene = self.scene.as_mut().ok_or("no scene (create_root first)")?;
        // clone 出 template 避开 packages + scene 双借（packages 在宿主上，scene 在 Stage 上）。
        let template = self
            .host
            .borrow()
            .packages
            .get(pkg)
            .and_then(|p| p.components.get(component))
            .cloned()
            .ok_or_else(|| format!("component `{component}` not in pkg `{pkg}`"))?;

        // 遍历 template.nodes 建树（父先建于子——parent_idx < i 由打包器/读保证）。
        // id_map[模板 idx] = live NodeId（slotmap 分配）。
        let mut id_map: Vec<Option<NodeId>> = vec![None; template.nodes.len()];
        let mut root_id: Option<NodeId> = None;
        // Dropdown NodeId 收集：建树循环后 reparent 其 option 子节点进 role=listbox
        // （运行时结构；option 是模板里 combobox 的 DOM 子节点，先被挂到 combobox）。
        let mut dropdown_ids: Vec<NodeId> = Vec::new();
        let mut tree_ids: Vec<NodeId> = Vec::new();
        // no-panic 契约：parent_idx 来自 pkg.bin（运行时读，可能 corrupt），不能信任"父先于子"。
        // pidx >= i 同时覆盖前向引用（父排在子后）与越界（pidx >= len，因 i < len）→ Err，不 panic。
        for (i, tn) in template.nodes.iter().enumerate() {
            if let Some(pidx) = tn.parent_idx {
                if pidx >= i {
                    return Err(format!(
                        "corrupt package: node {i} parent_idx {pidx} not yet built (parent must precede child)"
                    ));
                }
            }
        }
        for (i, tn) in template.nodes.iter().enumerate() {
            let node_id = crate::scene::dynamic::create_node_from_template(
                scene,
                tn.kind,
                tn.style.clone(),
                tn.control_init.clone(),
            );
            // 填 classes/id_attr/draggable/tabindex（create_node_from_template 不填这些，同 create_node）
            let n = scene.get_mut(node_id).unwrap();
            n.classes = tn.classes.clone();
            n.id_attr = tn.id_attr.clone();
            n.custom_tag = tn.custom_tag.clone();
            n.rich_text_block = tn.rich_text_block;
            n.interaction.draggable = tn.draggable;
            // HTML disabled 属性（fence 内容属性，#93 验收发现断链：属性过围栏但运行时
            // 无人消费）：instantiate 置 NodeFlags::DISABLED——click 抑制 / active 截断 /
            // :disabled 伪类 / 光标 affordance 全走既有 disabled 语义。
            if tn.disabled {
                n.interaction.flags.insert(NodeFlags::DISABLED);
            }
            // tabindex：显式值优先（含 -1 排除）；None 时按 HTML/ARIA 语义给可聚焦控件补
            // 默认 0（input/textarea/select/button 及 role=textbox/spinbutton/slider/switch/
            // radio/combobox 隐式可聚焦），否则 click-to-focus / Tab 链无法命中控件。
            // ProgressBar 只读不聚焦；OptionItem 焦点由父 Dropdown 管理。
            n.interaction.tabindex = tn.tabindex.or(match tn.kind {
                NodeKind::Button
                | NodeKind::TextField
                | NodeKind::TextArea
                | NodeKind::NumberField
                | NodeKind::Dropdown
                | NodeKind::Slider
                | NodeKind::Toggle
                | NodeKind::RadioButton
                // Tab 镜像 Button：role=tab 隐式可聚焦（WAI-ARIA），补默认 tabindex=0
                // 让 click-to-focus / 键盘 Tab 链能命中（箭头键导航依赖）。
                // TabList 是容器（镜像 ListView），自身不聚焦 → 落 _ => None。
                // TreeItem 同 Tab（roving tabindex 的 item 持焦点）；Tree 容器不聚焦。
                | NodeKind::Tab
                | NodeKind::TreeItem => Some(0),
                _ => None,
            });
            if let Some(c) = &tn.content {
                scene.text_contents.insert(node_id, c.clone());
            }
            if let Some(src) = &tn.src {
                scene.image_srcs.insert(node_id, src.clone());
            }
            // href（#74）：Link 节点的链接目标灌 side table（C# Link.Href 经 FFI 读）。
            if let Some(href) = &tn.href {
                scene.link_hrefs.insert(node_id, href.clone());
            }
            // role/data-slot：从 TemplateNode 填 RoleTable（role-driven controls 地基）。
            // role 驱动语义分派，data-slot 标识控件视觉部件；运行时 find_child_by_role/slot 查表。
            // RoleTable::insert 自带空 info 过滤——无 role 且无 data-slot 的节点不入表，保持稀疏。
            //
            // data-slot 映射成 slots 的 key（值为空串占位）：`data-slot="thumb"` →
            // slots["thumb"]=""。这样 find_child_by_slot(parent,"thumb") 直接比对 key 是否存在，
            // 语义直观（slot 名是 key，不是 value）。
            let info = crate::scene::node::RoleInfo {
                role: tn.role.clone(),
                slots: tn
                    .data_slot
                    .as_ref()
                    .map(|s| [(s.clone(), String::new())].into_iter().collect())
                    .unwrap_or_default(),
                attrs: tn.attrs.clone(),
            };
            scene.roles.insert(node_id, info);
            id_map[i] = Some(node_id);
            // 记 Dropdown，供建树后 reparent option 进 popup（见下方 reparent 循环）。
            if tn.kind == NodeKind::Dropdown {
                dropdown_ids.push(node_id);
            }
            // 记 Tree，供建树后把 ControlInit 的文档序选中项解析成 NodeId（见下方遍）。
            if tn.kind == NodeKind::Tree {
                tree_ids.push(node_id);
            }
            // 按 parent_idx 串子树（根 parent_idx=None 不串）
            if let Some(pidx) = tn.parent_idx {
                let parent = id_map[pidx].expect("parent built before child (parent_idx < i)");
                scene.get_mut(parent).unwrap().children.push(node_id);
                scene.get_mut(node_id).unwrap().parent = Some(parent);
            } else {
                // 组件根（parent_idx=None）——记录返回（多根取最后一个，spec 约定单根组件）
                root_id = Some(node_id);
            }
        }
        let root = root_id.ok_or("component has no root node (parent_idx=None missing)")?;

        // Reparent 直接挂在 combobox 下的 option 进其 listbox（作者通常已把 option 放 listbox
        // 内则 no-op；兜底防错位）。运行时结构详见 reparent_options_into_popup doc。
        // 在 SCOPE_ROOT / dynamic_rules 装载之前做：reparent 只改 parent 指针 + children 列表，
        // 不影响作用域边界（option 仍在 combobox 实例子树内）。
        for sel in &dropdown_ids {
            crate::scene::control::reparent_options_into_popup(scene, *sel);
        }

        // Tree 初始选中解析（#8）：ControlInit::Tree.selected_item 是文档序（DFS）序号，
        // 此处子树建满、Node 身份已定，解析成 NodeId 写回 ControlState::Tree.selected。
        // 与 bridge 侧 treeitem 文档序计数同口径（先序遍历，含折叠隐藏条目）。
        for tree in &tree_ids {
            crate::scene::control::resolve_tree_initial_selection(scene, *tree);
        }

        // 实例根 = 作用域根（Shadow DOM 风格）。
        // 该实例的 CSS 规则只在本实例子树内匹配，不泄漏到其他组件实例。
        // SCOPE_ROOT = CSS 作用域隔离；LOOKUP_SCOPE = Get<T> 查找边界（两语义解耦，保现有行为）。
        scene
            .get_mut(root)
            .unwrap()
            .interaction
            .flags
            .insert(NodeFlags::SCOPE_ROOT | NodeFlags::LOOKUP_SCOPE);

        // 组件展开域（Custom Element 打包期展开实例）：host 打三重标记——对后代是 CSS +
        // 查找边界（SCOPE_ROOT|LOOKUP_SCOPE），自身归外层页面作用域（HOST_IN_PARENT_SCOPE，
        // 页面规则可样式化 host 本体，shadow 树归 host 域）。
        for (i, tn) in template.nodes.iter().enumerate() {
            if tn.component_scope {
                if let Some(nid) = id_map[i] {
                    scene.get_mut(nid).unwrap().interaction.flags.insert(
                        NodeFlags::SCOPE_ROOT
                            | NodeFlags::LOOKUP_SCOPE
                            | NodeFlags::HOST_IN_PARENT_SCOPE,
                    );
                }
            }
        }

        // 组件级 @keyframes 进场景全局表：animation 声明只保存 name，player 在 tick
        // 时按该表查规则。后实例化的组件覆盖同名规则，保持 CSS 全局查找语义。
        for keyframes in &template.keyframes {
            scene
                .keyframes
                .insert(keyframes.name.clone(), keyframes.clone());
        }

        // 模板规则包装成 ScopedRule（scope_root = 实例根），push 进 scene 动态规则表。
        // 不再按 selector 去重：同模板多实例各带独立 scope_root，rematch 按 scope 隔离匹配，
        // 互不干扰（旧实现的 selector-only 去重会把不同组件同名 class 规则误判为重复丢弃——坑）。
        for rule in &template.dynamic_rules.rules {
            scene.dynamic_rules.entries.push(ScopedRule {
                rule: rule.clone(),
                scope_root: root,
            });
        }

        // 组件展开域锚定规则：每展开实例一条 (anchor_idx, 组件模板自带规则)，按
        // scope_root=锚节点（host）包装——组件内部选择器只在该展开域内匹配。
        // host 自身因 HOST_IN_PARENT_SCOPE 归外层作用域，组件规则不落在 host 上（同 DOM
        // shadow 规则不样式化 host，:host 才行）。
        for (anchor_idx, rules) in &template.component_scopes {
            let Some(anchor) = id_map[*anchor_idx] else {
                continue; // 防御 malformed（read 侧已校验 anchor < node_count，理论不可达）
            };
            for rule in &rules.rules {
                scene.dynamic_rules.entries.push(ScopedRule {
                    rule: rule.clone(),
                    scope_root: anchor,
                });
            }
        }
        Ok(root)
    }

    /// 场景级子树克隆（与 instantiate 并列，但不走 pkg 组件）。
    ///
    /// 深拷贝 kind/classes/id_attr/base_style/文本/img src，返回游离新根（不挂树，调用方负责
    /// append_child 挂载）。虚拟列表 slot 填充路径：clone_subtree(模板根) → 得游离实例 →
    /// append_child(slot, 实例)。side table 判定：结构化数据（text/image）拷贝，
    /// 运行时状态（scroll/anim/tween/EditState）不拷——克隆是干净模板，由调用方按需重设。
    pub fn clone_subtree(&mut self, src: NodeId) -> Result<NodeId, String> {
        let scene = self.scene.as_mut().ok_or("no scene (create_root first)")?;
        if scene.get(src).is_none() {
            return Err("clone_subtree: src node not found".into());
        }
        Ok(crate::scene::dynamic::clone_node_recursive(scene, src))
    }

    /// 测试 helper：建空 scene 的 Stage（不依赖 parse feature）。
    /// 供动态建树 API 测试用——用 create_root/create_node 返回的 NodeId，不硬编码值。
    #[cfg(test)]
    pub fn new_for_test() -> Self {
        let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
        let mut s = Stage::new((200.0, 200.0)).unwrap();
        s.register_font("DejaVu", std::fs::read(font_path).unwrap(), true)
            .unwrap();
        s.scene = Some(crate::scene::node::Scene::default());
        s
    }

    /// 本帧产出的事件（tick 后读；FFI borrow_events 用）。
    pub fn last_events(&self) -> &[EventRecord] {
        &self.last_events
    }

    /// 每帧管线（rematch 提到 solve 前，伪类三类全当帧消费）：
    /// ①tween ②focus_request ③process（仲裁+拖拽写 scroll_pos；hit_test 读上帧 world，1帧延迟已认）
    /// ④scroll update ⑤process_keys ⑥rematch_pseudo_classes（提到 solve 前：改 layout/transform/colors
    /// 三类，本帧 solve+compute 全消费）⑥.5 transition drain（rematch 产请求 → kill 旧 tween + 提交新）
    /// ⑥.6 sync_animation_players（rematch 后启停 player：class 触发声明式动画）
    /// ⑦solve（读 rematch 后 taffy_style）
    /// ⑧refresh_content_sizes ⑨compute_world_transforms（读 rematch 后 transform+scroll_pos）
    /// ⑩build_render_nodes
    ///
    /// **compute_world_transforms 时机**：rematch 之后、render 之前，每帧 1 次。
    /// transform 改由 rematch 写 style.transform → compute 同帧读 → world 含缩放。
    /// scroll_pos 同帧进 world matrix。
    /// **1 帧延迟语义**：hit_test 用上帧 world_transforms。首帧 world_transforms 为空，
    /// hit_test bounds guard 拦截（越界返 None → 未命中，零回归安全）。仲裁在 Down 未滚动前
    /// 不影响；clip 门控用 viewport 固定主导，不依赖每帧变换精度。
    pub fn tick_and_render(&mut self) -> FrameData {
        // FFI 入口绝不 panic。scene=None（load 前）早返空帧，不 expect
        // （cdylib .expect 遇 None non-unwinding abort 拖垮宿主进程）。
        let scene = match self.scene.as_mut() {
            Some(s) => s,
            None => return FrameData::default(),
        };
        // 宿主注册表对账：register_font / set_fallback / set_image_sizes 等宿主侧变更
        // 不经场景 mutation（Text 的 MeasureContext 只存 family 名），taffy 与 measure
        // 缓存都无感。代数不等 → 清文本缓存两表 + taffy mark_dirty 文本叶子，本帧重测。
        let host_generation = self.host.borrow().generation;
        if host_generation != self.host_generation_seen {
            self.host_generation_seen = host_generation;
            invalidate_text_after_host_change(scene);
        }
        let mut out: Vec<EventRecord> = Vec::new();
        // drain FFI setter 产的事件（set_control_text 等）进本帧 out。这些 setter 在 tick 外
        // 调用、写 pending_events；此处 drain 使事件在下 tick 入 last_events（与 C# 读
        // 事件节奏一致）。排在最前 = 先于本帧输入事件（setter 发生在上 tick 与本 tick 之间）。
        let ffi_events = std::mem::take(&mut self.pending_events);
        out.extend(ffi_events);
        // tween 推进（写 scene.anim + 产 complete 事件进 out）。须在 solve/compute_world_transforms 前。
        let dt = self.pending_dt;
        self.pending_dt = 0.0;
        self.tweens.update(dt, scene, &mut out);
        // player 推进（写 scene.anim）。在 tweens.update **之后** = 写入顺序即优先级：
        // animation 覆盖 transition 同通道。须在 solve/compute_world_transforms 前。
        crate::scene::animation::update_all(scene, dt, &mut out);
        // 光标闪烁 timer（单一动画时钟：与 tweens 同 dt，每帧 tick 推进一步）。
        crate::scene::control::advance_cursor_blink(scene, dt);
        // 消费 pending_focus_request（编程聚焦/清焦点，tick 外 request_focus/blur 记）。
        // 最前消费——下 tick 才生效，避免 tick 覆写 last_events 丢请求事件。
        if let Some(req) = self.pending_focus_request.take() {
            crate::input::focus_node(scene, req, &mut out);
        }
        // 1. process（仲裁 + 拖拽跟手写 scroll_pos）
        // hit_test 读上帧 world_transforms（1 帧延迟，已认）——首帧 world_transforms 为空，
        // hit_test bounds guard 拦截（越界返 None → 未命中，零回归安全）。
        // 借用冲突解：process 借 &mut scene + &input——scene 与 pending_input 都是 self 字段，
        // 同时借 self 冲突。先 take 出 input（离开 self 借用），process 返回后 drop。
        let input = std::mem::take(&mut self.pending_input);
        let mut ptr_out = self.pointer_state.process(scene, &input);
        out.append(&mut ptr_out);
        // 2. scroll.update（消费 pending_wheel + 惯性/回弹 advance）
        let wheels = std::mem::take(&mut self.pending_wheel);
        for w in &wheels {
            crate::scroll::apply_wheel_to_hit(scene, *w);
        }
        crate::scroll::advance_all(dt, scene);
        // 3. 键盘事件（keydown/up + Tab 导航 + FocusIn/Out）
        let keys = std::mem::take(&mut self.pending_keys);
        crate::input::process_keys(scene, &keys, &mut out);
        // 3.5 字符输入通道（UTF-32 codepoints，已 shift-mapped）。与 keydown 互补：
        //     keydown 走物理键，textinput 走可打印字符。集中到 input.rs 的 process_text_input
        //     处理（含 NumberField 数字 guard；TextField/TextArea 接受任意字符）。
        //     readonly 控件 insert_text 自身返 false（不改值）。
        let cps = std::mem::take(&mut self.pending_text_input);
        crate::input::process_text_input(scene, &cps, &mut out);
        self.last_events = out;
        // 3.6 ListView 虚拟化可见区更新（solve 前：新克隆 slot 本帧布局）。拆 plan/execute 两阶段
        //     解 clone 借用冲突——两阶段都只借 scene，依次调即可。
        let list_ops = crate::list::plan_visible(scene);
        crate::list::execute_visible(scene, list_ops);
        // 4. 伪类重匹配（提到 solve 前：改 taffy_style/transform/colors，本帧全部消费）
        rematch_pseudo_classes(scene, self.root_size, self.safe_insets);
        // 4.5 transition drain：rematch 检测可动画通道变化时推入 scene.pending_transitions。
        //     每个请求 kill 旧 (node,prop) tween（override 保留 mid-flight 末值，见 tween.rs kill）
        //     + 提交新 tween（start = mid-flight override → 无闪烁）。切页 kill 语义。
        //     借用：scene 经 self.scene.as_mut() 借；self.tweens 独立字段（同 tweens.update 访问形）。
        let reqs: Vec<crate::tween::TransitionRequest> =
            std::mem::take(&mut scene.pending_transitions);
        for r in reqs {
            self.tweens.kill(r.node, r.prop);
            let pad = |v: [f32; 5]| {
                let mut buf = [0.0f32; 8];
                buf[..5].copy_from_slice(&v);
                buf
            };
            let start = pad(r.start);
            let end = pad(r.end);
            // 提交即预写起始值（n=0）进 anim：本帧 solve 读 override 而非级联终点——
            // 否则 transition 首帧渲染端点值一帧（展开先满高再塌回起点起播；反向则先消失
            // 一帧）。delay 期间 tween.update 不写槽，此预写兜底 = CSS「延迟期持有旧值」。
            // 与 4.6 sync_animation_players 的「新 player backwards 首帧立即写 NodeAnim」
            // 同纪律：声明式动画的起始态当帧可消费。
            crate::tween::apply(
                &mut scene.anim,
                r.node,
                r.prop,
                &start,
                &end,
                r.shadow.as_deref(),
                0.0,
            );
            self.tweens.tween(
                r.node,
                crate::tween::TweenSpec {
                    prop: r.prop,
                    start,
                    end,
                    ease: r.ease,
                    delay: r.delay,
                    duration: r.duration,
                    tag: TRANSITION_TAG,
                    repeat: 0,
                    yoyo: false,
                    shadow: r.shadow,
                },
            );
        }
        // 4.55 layout transition 跨域/auto 端点的跳变警告（rematch 推入；进本帧事件流，
        // C# EventDemuxer 转日志——运行时 add_class 漏网端的可观测信号）。
        let warns = std::mem::take(&mut scene.pending_anim_warnings);
        self.last_events.extend(warns);
        // 4.6 animation 声明同步：rematch 后读 computed style.animation
        //     启停 player。新 player 的 backwards 首帧立即写 NodeAnim，本帧 solve+render 消费；
        //     回收时通道回 None（tween/base 下帧接管）。在 transition drain 之后：两者都只读
        //     computed style、写各自运行时态，互不干扰（检测独立）。
        sync_animation_players(scene);
        // 4.7 控件状态→视觉同步：ControlState 变化后把 fill width / check display 写进
        //     子节点 inline_override。须在 solve 前（inline 影响布局：fill width 决定 bar 宽度）。
        //     每帧对所有控件节点扫一次（控件稀疏，代价可接受）。读 controls.0.keys() 克隆
        //     避免与 sync_control_visuals 的可变借冲突。
        let control_ids: Vec<NodeId> = scene.controls.0.keys().copied().collect();
        for cid in control_ids {
            crate::scene::control::sync_control_visuals(scene, cid, self.root_size.1);
        }
        // 5. solve（读 rematch 后的 taffy_style → layout_rect）
        // 核心知图尺寸（打包期 PNG IHDR 静态，存宿主 image_sizes）。solve 查尺寸表算
        // Image intrinsic（三档：CSS > 真实像素 > 64×64）。不知图集（运行时纹理/UV 归 Unity）。
        self.frame_no += 1;
        let frame_no = self.frame_no;
        if !self.incremental_render {
            self.render_cache.entries.clear();
        }
        let res_gen = self.host.borrow().generation;
        let mut host_ref = self.host.borrow_mut();
        let host = &mut *host_ref;
        solve(
            scene,
            &host.fonts,
            self.root_size,
            self.safe_insets,
            &host.image_sizes,
        );
        // 5.5 measure 文本控件显示文本——需 solve 产出的 layout_rect.w 定 content width,
        //     且须在 render 前完成（光标命中测试/几何依赖 TextLayout 缓存）。
        crate::scene::control::measure_text_controls(scene, &host.fonts);
        // 5.55 单行文本视口跟随：measure 刷新缓存后、render 前钳 view_x（光标跟随滚动）。
        crate::scene::control::sync_edit_view(scene);
        // 5.6 ListView 高度回填：solve 后 slot 拿到真实 layout_rect.h，回填 HeightCache。
        //     须在 refresh_content_sizes 前——content_size 用 spacer 高度（由可见区算法算出，
        //     下帧用回填后的精准高度而非 estimate）。
        crate::list::collect_heights(scene);
        // 6. content_size 填充（solve 后 content_size/viewport/overlap）
        crate::scroll::refresh_content_sizes(scene);
        // 6.5 Smooth ScrollToItem 锚重算：回填后的最新高度 + 新 overlap 定 tween 终点
        //     （须在 refresh_content_sizes 后——clamp 用新 overlap）。
        crate::list::recompute_smooth_scroll_targets(scene);
        // 7. compute_world_transforms（读 rematch 后 transform + scroll_pos → world）
        crate::scene::transform::compute_world_transforms(scene);
        // 8. 渲染（+ 合成 scrollbar）。传上帧 hash 基线，未变节点 change_level=Skip；
        //    返回新 hash 存 self.prev_node_hashes 供下帧比。
        // build_render_nodes 查宿主 image_sizes 算九宫格 UV（slice_px / src_px）。
        // Image payload 带 path，UV 全图 (0,0)-(1,1)（无 atlas 子区），Unity 查 Sprite 拿真实 UV。
        let (frame, new_hashes, sort_keys) = crate::render::build_render_nodes_cached(
            scene,
            &host.fonts,
            &self.prev_node_hashes,
            &host.image_sizes,
            &mut host.glyph_atlas,
            &mut self.render_cache,
            res_gen,
            frame_no,
        );
        drop(host_ref);
        scene.node_sort_keys = sort_keys;
        self.prev_node_hashes = new_hashes;
        frame
    }

    pub fn render_json(&mut self) -> String {
        let frame = self.tick_and_render();
        serde_json::to_string_pretty(&frame.nodes).unwrap()
    }
}

/// 链式 tween builder（`Stage::tween_builder` 的返回值形态）。
///
/// ```ignore
/// stage.tween_builder(node, TweenProp::Opacity)
///     .from(&[0.0]).to(&[1.0])
///     .duration(0.3).delay(0.1)
///     .ease(Ease::CubicOut)
///     .repeat(2, true)   // 额外重播 2 次 + yoyo 往返
///     .tag(7)            // complete 事件按 tag 路由
///     .start();
/// ```
///
/// 消费型 builder：每个方法吃 self 返 self，`start()` 提交进 TweenManager 并返 builder
/// 链结束。from/to 拷贝前 `prop_value_size` 个分量（不足补 0，超出忽略）。
pub struct TweenBuilder<'a> {
    stage: &'a mut Stage,
    node: NodeId,
    spec: crate::tween::TweenSpec,
}

impl<'a> TweenBuilder<'a> {
    /// 起始值（前 value_size 个分量有效）。
    pub fn from(mut self, start: &[f32]) -> Self {
        self.spec.start = pad_values(start);
        self
    }

    /// 目标值（前 value_size 个分量有效）。
    pub fn to(mut self, end: &[f32]) -> Self {
        self.spec.end = pad_values(end);
        self
    }

    pub fn duration(mut self, secs: f32) -> Self {
        self.spec.duration = secs;
        self
    }

    pub fn delay(mut self, secs: f32) -> Self {
        self.spec.delay = secs;
        self
    }

    pub fn ease(mut self, ease: crate::tween::Ease) -> Self {
        self.spec.ease = ease;
        self
    }

    /// repeat = 额外重播次数（0 = 单次）；yoyo = 奇数轮反向（alternate）。
    pub fn repeat(mut self, extra: u32, yoyo: bool) -> Self {
        self.spec.repeat = extra;
        self.spec.yoyo = yoyo;
        self
    }

    /// complete 事件载荷（FFI 事件 touch_id 槽位回传，C# OnComplete 按此路由）。
    pub fn tag(mut self, tag: u32) -> Self {
        self.spec.tag = tag;
        self
    }

    /// 提交（注册进 TweenManager，本帧起生效）。
    pub fn start(self) {
        self.stage.tween(self.node, self.spec);
    }
}

/// 切片 → TweenValue 8 槽缓冲（不足补 0，超出截断到 8）。
fn pad_values(v: &[f32]) -> crate::tween::TweenValue {
    let mut buf = [0.0f32; 8];
    let n = v.len().min(8);
    buf[..n].copy_from_slice(&v[..n]);
    buf
}

/// 动态建树 API 测试（不依赖 parse feature——runtime API 可用性门）。
/// 用 Stage::new_for_test() 建空 scene，用 create_root/create_node 返回的 NodeId，不硬编码值。
#[cfg(test)]
mod dynamic_tests;

/// 资源池测试：load_package 进 packages 字典不建 scene + 多包共存 + 同名替换。
/// 不依赖 parse feature——用内存 pkg（write_package）。
#[cfg(test)]
mod load_package_tests;

/// instantiate 测试：从包克隆组件子树进 scene + 伪类规则合并去重 + 多实例独立。
/// 不依赖 parse feature——用内存 PackageInput（write_package）。
#[cfg(test)]
mod instantiate_tests;

/// 集成测试：运行时灌入 image_sizes → solve 用真实尺寸。
/// 验证端到端链路：set_image_sizes 灌入 (w,h) → solve 查表算 Image intrinsic（三档：CSS > 真实像素 > 64×64）。
#[cfg(test)]
mod image_size_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试字体：仓库内 DejaVuSans.ttf（跨平台一致），缺则跳过（与 text 模块同款）。
    fn test_font_bytes() -> Option<Vec<u8>> {
        std::fs::read(format!(
            "{}/tests/fixtures/DejaVuSans.ttf",
            env!("CARGO_MANIFEST_DIR")
        ))
        .ok()
    }

    #[test]
    fn measure_text_rejects_unknown_family_and_bad_params() {
        let s = Stage::new((100.0, 100.0)).unwrap();
        assert!(
            s.measure_text("hi", "no-such-family", 16.0, 0.0).is_err(),
            "未注册 family 拒绝（不静默 fallback 到默认字体）"
        );
        if let Some(bytes) = test_font_bytes() {
            let mut s = Stage::new((100.0, 100.0)).unwrap();
            s.register_font("dejavu", bytes, true).unwrap();
            assert!(
                s.measure_text("hi", "dejavu", 0.0, 0.0).is_err(),
                "非正字号拒绝"
            );
            assert!(
                s.measure_text("hi", "dejavu", 16.0, f32::NAN).is_err(),
                "NaN max_width 拒绝"
            );
        }
    }

    #[test]
    fn measure_text_wraps_by_max_width() {
        let Some(bytes) = test_font_bytes() else {
            return;
        };
        let mut s = Stage::new((1920.0, 1080.0)).unwrap();
        s.register_font("dejavu", bytes, true).unwrap();
        let m = s.measure_text("hello world", "dejavu", 16.0, 0.0).unwrap();
        assert_eq!(m.line_count, 1, "无 max_width = 单行");
        let single_w = m.width;
        assert!(m.width > 0.0 && m.height > 0.0);

        // 窄约束把 "hello world" 断成两行：行数 2、宽 ≤ 约束、高约两倍单行。
        let m2 = s
            .measure_text("hello world", "dejavu", 16.0, single_w * 0.6)
            .unwrap();
        assert_eq!(m2.line_count, 2, "窄 max_width 断行");
        assert!(m2.width <= single_w * 0.6 + f32::EPSILON);
        assert!(m2.height > m.height, "两行高于一行");
    }

    /// 20B 头（magic + version + flags + comp_count + str_count）：版本检查先于 body 解析，
    /// 烂 body 也能测版本错配分支。
    fn pkg_header_with_version(version: u32) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&crate::asset::PKG_MAGIC.to_le_bytes());
        b.extend_from_slice(&version.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b
    }

    #[test]
    fn load_package_reports_version_mismatch_structured() {
        use crate::asset::{MAX_VERSION, MIN_VERSION};
        let mut s = Stage::new((100.0, 100.0)).unwrap();
        assert_eq!(
            s.load_package("old", &pkg_header_with_version(MIN_VERSION - 1)),
            Err(LoadPkgError::TooOld {
                pkg: MIN_VERSION - 1,
                min: MIN_VERSION
            })
        );
        assert_eq!(
            s.last_pkg_load_version(),
            MIN_VERSION - 1,
            "版本错配记录进宿主（FFI 只读不写）"
        );
        assert_eq!(
            s.load_package("new", &pkg_header_with_version(MAX_VERSION + 1)),
            Err(LoadPkgError::TooNew {
                pkg: MAX_VERSION + 1,
                max: MAX_VERSION
            })
        );
        let mut bad_magic = pkg_header_with_version(MIN_VERSION);
        bad_magic[0] ^= 0xFF;
        assert!(matches!(
            s.load_package("bad", &bad_magic),
            Err(LoadPkgError::Malformed(_))
        ));
    }

    #[test]
    fn get_node_kind_returns_builtin_kinds() {
        let mut s = Stage::new((100.0, 100.0)).unwrap();
        let root = s.create_root("div", "").unwrap();
        let btn = s.create_node("button", "").unwrap();
        s.append_child(root, btn).unwrap();
        let img = s.create_node("img", "").unwrap();
        s.append_child(root, img).unwrap();
        let sp = s.create_node("span", "").unwrap();
        s.append_child(root, sp).unwrap();
        use crate::scene::node::NodeKind;
        assert_eq!(s.get_node_kind(root), Some(NodeKind::Container));
        assert_eq!(s.get_node_kind(btn), Some(NodeKind::Button));
        assert_eq!(s.get_node_kind(img), Some(NodeKind::Image));
        assert_eq!(s.get_node_kind(sp), Some(NodeKind::TextNode));
        // 无效句柄 → None（不撞 Container=0）。
        assert_eq!(s.get_node_kind(crate::scene::NodeId::INVALID), None);
    }

    /// 宿主共享契约：同宿主挂两个 Stage，一边 register_font 另一边立即可 measure；
    /// 字体驻留只有一份（ResourceHost 单实例）；两 Stage 场景互不可见（实例态隔离）。
    #[test]
    fn bound_stages_share_host_resources() {
        let Some(bytes) = test_font_bytes() else {
            return;
        };
        let host = std::rc::Rc::new(std::cell::RefCell::new(crate::host::ResourceHost::new()));
        let mut a = Stage::new_bound(host.clone(), (800.0, 600.0)).unwrap();
        let b = Stage::new_bound(host.clone(), (1920.0, 1080.0)).unwrap();
        a.register_font("dejavu", bytes, true).unwrap();
        assert!(
            b.measure_text("hi", "dejavu", 16.0, 0.0).is_ok(),
            "宿主字体对后挂的 Stage 可见（共享 FontTable）"
        );
        // 场景互不可见：a 的 root 不在 b 里。
        let root = a.create_root("div", "").unwrap();
        assert!(
            b.get_node_kind(root).is_none(),
            "树实例隔离——NodeId 不跨 Stage 泄漏"
        );
        // 宿主引用计数：两 Stage + 本地 host clone 各持一份。
        assert_eq!(std::rc::Rc::strong_count(&host), 3);
    }

    /// 注册表失效钩：字体重注册（同名换 font id）不产生场景 mutation，MeasureContext
    /// 逐字节不变——若只靠 taffy 值比较，文本永远吃旧缓存。generation 对账后下一 tick
    /// 必须重测（text_layouts 换新，run 的 font_id 用新注册的 id）。
    #[test]
    fn host_registry_change_forces_text_remeasure() {
        let Some(bytes) = test_font_bytes() else {
            return;
        };
        let mut s = Stage::new((800.0, 600.0)).unwrap();
        s.register_font("dejavu", bytes.clone(), true).unwrap();
        let root = s.create_root("div", "").unwrap();
        let text = s.create_node("span", "hello").unwrap();
        s.append_child(root, text).unwrap();
        s.tick_and_render();
        let scene = s.scene.as_ref().unwrap();
        let font_id_v1 =
            scene.text_layouts[text.index()].as_ref().unwrap().lines[0].runs[0].font_id;

        // 同名重注册：分配新 id（register 覆盖 family，next_id 递增不复用）。
        s.register_font("dejavu", bytes, true).unwrap();
        s.tick_and_render();
        let scene = s.scene.as_ref().unwrap();
        let layout = scene.text_layouts[text.index()]
            .as_ref()
            .expect("失效钩后 text_layouts 仍须被重新填充");
        let font_id_v2 = layout.lines[0].runs[0].font_id;
        assert_ne!(
            font_id_v1, font_id_v2,
            "重注册换 id 后重测必须取新 id（陈旧 = 旧字体面 + atlas 撞键）"
        );
    }

    #[test]
    fn get_node_computed_style_returns_snapshot() {
        use crate::style::resolved::DisplayMode;
        let mut s = Stage::new((100.0, 100.0)).unwrap();
        let root = s.create_root("div", "").unwrap();
        let c = s
            .get_node_computed_style(root)
            .expect("root computed style");
        // 默认值（不依赖 rematch 时机）：opacity 1.0、div 默认 display Block
        // （运行时 create_root 复刻 css_resolve 的 tag DisplayDefault 铺底）。精确 cascade 值由专门单测验。
        assert_eq!(c.opacity, 1.0);
        assert_eq!(c.display_mode, DisplayMode::Block);
        assert_eq!(
            s.get_node_computed_style(crate::scene::NodeId::INVALID),
            None,
            "invalid node -> None"
        );
    }

    #[test]
    fn tick_measures_textfield_layout_after_solve() {
        // TextField TextLayout 应在 tick_and_render 的 solve 后即 measure 并缓存，
        // 而非推迟到 render 阶段 lazily 计算——光标命中和几何
        // 在 render 前就需 TextLayout。
        let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
        let mut stage = Stage::new((200.0, 100.0)).unwrap();
        stage
            .register_font("DejaVu", std::fs::read(font_path).unwrap(), true)
            .unwrap();
        // 用 create_node_from_template 直接建 TextField（kind_from_tag 白名单不含 input 标签）。
        stage.ensure_scene();
        let scene = stage.scene.as_mut().unwrap();
        let root = crate::scene::dynamic::create_root(scene, "div", "").unwrap();
        let tf = crate::scene::dynamic::create_node_from_template(
            scene,
            crate::scene::node::NodeKind::TextField,
            crate::style::resolved::ResolvedStyle::default(),
            Some(crate::asset::ControlInit::TextField(
                crate::asset::EditInit {
                    value: "hello".into(),
                    placeholder: String::new(),
                    max_length: 0,
                    readonly: false,
                },
            )),
        );
        crate::scene::dynamic::append_child(scene, root, tf).unwrap();
        stage.tick_and_render();
        // solve 后 measure_text_controls 已写 text_layouts —— 不应为空。
        let scene = stage.scene.as_ref().unwrap();
        assert!(
            scene.text_layouts[tf.index()].is_some(),
            "TextField TextLayout must be measured at layout stage (after solve), not lazily at render"
        );
    }

    #[test]
    fn tick_empty_textfield_measures_placeholder_height() {
        // 空 value TextField 用 placeholder 做 layout measure（intrinsic size 含 placeholder
        // 文字行高，不是 padding-only）。layout measure 闭包缓存 placeholder TextLayout，
        // render 直接复用（不再 lazy fallback）。这在 pivot 后空 div 形态尤其重要：
        // 无 measure 则 taffy content=0、高度塌成 padding-only，文字不参与布局。
        let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
        let mut stage = Stage::new((200.0, 100.0)).unwrap();
        stage
            .register_font("DejaVu", std::fs::read(font_path).unwrap(), true)
            .unwrap();
        stage.ensure_scene();
        let scene = stage.scene.as_mut().unwrap();
        let root = crate::scene::dynamic::create_root(scene, "div", "").unwrap();
        let tf = crate::scene::dynamic::create_node_from_template(
            scene,
            crate::scene::node::NodeKind::TextField,
            crate::style::resolved::ResolvedStyle::default(),
            Some(crate::asset::ControlInit::TextField(
                crate::asset::EditInit {
                    value: String::new(),
                    placeholder: "enter text".into(),
                    max_length: 0,
                    readonly: false,
                },
            )),
        );
        crate::scene::dynamic::append_child(scene, root, tf).unwrap();
        stage.tick_and_render();
        let scene = stage.scene.as_ref().unwrap();
        // placeholder TextLayout 被缓存（layout measure 用 placeholder 算 intrinsic size）
        assert!(
            scene.text_layouts[tf.index()].is_some(),
            "空 value TextField 应缓存 placeholder TextLayout（layout measure 用它算高度）"
        );
        // 高度含 placeholder 文字行高（default style padding=0，h≈文字行高 > 0）
        let h = scene.get(tf).unwrap().layout_rect.h;
        assert!(
            h > 1.0,
            "空 TextField 高度应含 placeholder 文字行高（h={:.1}），非 0",
            h
        );
    }

    #[test]
    fn tick_textarea_and_numberfield_measure_intrinsic_height() {
        // 回归守卫：measure MeasureContext arm 覆盖 TextField | TextArea | NumberField。
        // tick_empty_textfield_measures_placeholder_height 只测 TextField；TextArea/NumberField
        // 的 measure arm 若脱落（退回 padding-only 高度），文字不参与布局，本断言即失败。
        let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
        let cases: Vec<(
            crate::scene::node::NodeKind,
            crate::asset::ControlInit,
            &str,
        )> = vec![
            (
                crate::scene::node::NodeKind::TextArea,
                crate::asset::ControlInit::TextArea(crate::asset::EditInit {
                    value: "multi\nline".into(),
                    placeholder: String::new(),
                    max_length: 0,
                    readonly: false,
                }),
                "TextArea",
            ),
            (
                crate::scene::node::NodeKind::NumberField,
                crate::asset::ControlInit::NumberField {
                    edit: crate::asset::EditInit {
                        value: "42".into(),
                        placeholder: String::new(),
                        max_length: 0,
                        readonly: false,
                    },
                    min: 0.0,
                    max: 100.0,
                    step: 1.0,
                },
                "NumberField",
            ),
        ];
        for (kind, init, label) in cases {
            let mut stage = Stage::new((200.0, 100.0)).unwrap();
            stage
                .register_font("DejaVu", std::fs::read(font_path).unwrap(), true)
                .unwrap();
            stage.ensure_scene();
            let scene = stage.scene.as_mut().unwrap();
            let root = crate::scene::dynamic::create_root(scene, "div", "").unwrap();
            let node = crate::scene::dynamic::create_node_from_template(
                scene,
                kind,
                crate::style::resolved::ResolvedStyle::default(),
                Some(init),
            );
            crate::scene::dynamic::append_child(scene, root, node).unwrap();
            stage.tick_and_render();
            let scene = stage.scene.as_ref().unwrap();
            assert!(
                scene.text_layouts[node.index()].is_some(),
                "{} TextLayout must be measured at layout stage (after solve)",
                label
            );
            let h = scene.get(node).unwrap().layout_rect.h;
            assert!(
                h > 1.0,
                "{} 高度应含文字行高（h={:.1}），非 padding-only 塌缩",
                label,
                h
            );
        }
    }
}
