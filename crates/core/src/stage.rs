//! Stage 层：串起 parse → style → scene → layout → render 的端到端入口。
//!
//! 资源池模型：`load_package(name, bytes)` 进 `packages` 字典不建 scene；
//! scene 由 `create_root`/`create_node` 建（动态树 API）。`tick_and_render` 跑
//! solve + build_render_nodes。`render_json` serde 序列化产渲染 JSON。

use crate::input::{EventRecord, PointerEvent, PointerState};
use crate::layout::solve;
use crate::render::build_render_nodes;
use crate::render::FrameData;
use crate::scene::node::{ControllerChangedEvent, NodeFlags, NodeId, Rect, Scene};
use crate::style::dynamic::rematch_pseudo_classes;
use crate::style::resolved::OverflowMode;
use crate::text::layout::FontTable;

/// transition 自动提交 tween 的 tag（rematch 检测通道变化 → drain kill 旧 + 提交新）。
/// 区分 driver 主动注册的 tween（caller-supplied u32 tag）——使 transition 完成事件可识别。
/// 选 0xFFFF_FFFE 哨兵（接近 u32 上限，避开常见 driver 小整数 tag）。
const TRANSITION_TAG: u32 = 0xFFFF_FFFE;

pub struct Stage {
    pub scene: Option<Scene>,
    pub fonts: FontTable,
    pub root_size: (f32, f32),
    /// 资源池：pkg_name → Package（多包共存）。load_package 填，instantiate 读。
    /// load_package 不建 scene，只填本字典。
    pub packages: std::collections::HashMap<String, crate::asset::Package>,
    /// 图尺寸表：归一化 path → (w, h) 像素。
    /// 运行时由 `set_image_sizes` 灌入（来自 atlas.json，含真实图尺寸）。
    /// `solve`/`build_render_nodes` 查此表算 Image intrinsic 尺寸（measure 三档）+ 九宫格 UV。
    /// path 缺失或 w/h=0 → fallback 64×64（核心不知图集，但知图尺寸）。
    pub image_sizes: std::collections::HashMap<String, (u32, u32)>,
    /// 单指针状态机（hover/active 状态 + 命中 diff + 产事件）。
    pub pointer_state: PointerState,
    /// set_input 缓存的本帧输入；tick_and_render 消费后 clear。
    pub pending_input: Vec<PointerEvent>,
    /// 本帧 tick 产出的事件序列（process 返回）；last_events/borrow_events 读。
    pub last_events: Vec<EventRecord>,
    /// set_key_input 缓存的本帧键盘输入；tick 消费后 clear。
    pub pending_keys: Vec<crate::input::KeyEvent>,
    /// set_wheel_input 缓存的本帧滚轮输入；tick 消费（apply_wheel_to_hit）后 clear。
    /// 累积式（extend，非 clear-then-set）——多组滚轮合并到一帧。
    pub pending_wheel: Vec<crate::scroll::WheelEvent>,
    /// 编程聚焦/清焦点请求（request_focus/blur tick 外调记，tick 最前消费）。
    /// 外层 Some=有请求；内层 Some(id)=聚焦某节点 / None=清焦点。
    pub pending_focus_request: Option<Option<NodeId>>,
    /// tween 引擎（每 tick update 写 scene.anim + 产 complete 事件）。
    pub tweens: crate::tween::TweenManager,
    /// advance_time stash 的本帧 dt（tick_and_render 消费，喂 tweens.update）。
    pub pending_dt: f32,
    /// 上帧每节点 (header_hash, payload_hash)（node_id 键）。跨 tick 持续，供
    /// build_render_nodes 比较定 ChangeLevel。transient 不进 pkg（Stage 字段非 Scene 字段）。
    /// reload/节点数变 → clear → 下帧全 dirty（无基线）。
    pub prev_node_hashes: std::collections::HashMap<u32, (u64, u64)>,
    /// 核心字形 atlas（v1.6 自绘字体）。render build 期 ensure 字形 UV，
    /// FFI 拉 R8 脏页上传。Stage 持有（非 Scene——atlas 是渲染资源，生命周期跨 tick）。
    pub glyph_atlas: crate::text::atlas::GlyphAtlas,
}

impl Stage {
    pub fn new(root_size: (f32, f32)) -> Result<Self, String> {
        Ok(Stage {
            scene: None,
            fonts: FontTable::new(),
            root_size,
            packages: std::collections::HashMap::new(),
            image_sizes: std::collections::HashMap::new(),
            pointer_state: PointerState::new(),
            pending_input: Vec::new(),
            last_events: Vec::new(),
            pending_keys: Vec::new(),
            pending_wheel: Vec::new(),
            pending_focus_request: None,
            tweens: crate::tween::TweenManager::new(),
            pending_dt: 0.0,
            prev_node_hashes: std::collections::HashMap::new(),
            glyph_atlas: crate::text::atlas::GlyphAtlas::new(),
        })
    }

    /// 注册字体进字体表。is_default=true 设为默认（measure 的 fallback）。
    /// FFI 层在首次 tick 前必须注册至少一个 default 字体，否则 measure 时 select panic。
    pub fn register_font(
        &mut self,
        family: &str,
        bytes: Vec<u8>,
        is_default: bool,
    ) -> Result<(), String> {
        self.fonts.register(family, bytes, is_default)
    }

    /// 设全局字体回退链。families 须已 register（未注册的 FontTable 内部跳过）。
    /// 主字体缺字时按序 probe 这些 family，首个含该字的补上（RmlUi fallback 模型）。
    /// 空切片清空回退（退回单字体）。source-agnostic：只收 family 名，后端把系统字体
    /// register 进来后照样能用——核心不问字体来源。
    pub fn set_fallback_families(&mut self, families: &[String]) {
        self.fonts.set_fallback_families(families);
    }

    /// 加载包进资源池（不碰 scene）。重复 load 同名包 = 替换。多包共存。
    ///
    /// `load_package(name, bytes)` 解析 pkg.bin → Package，存进 `self.packages[name]`。
    /// **不建 scene**——加载与实例化解耦（fgui/Unity prefab 模型）。
    /// scene 由 `create_root`/`create_node` 建；组件实例化由 `instantiate` 做。
    /// `root_size` 归 Stage（不从包来）；图集归 Unity（核心不知图集）。
    ///
    /// **图尺寸**：由 `set_image_sizes` 在运行时灌入（来自 atlas.json），不再从包 manifest 自建。
    pub fn load_package(&mut self, name: &str, bytes: &[u8]) -> Result<(), String> {
        let mut pkg = crate::asset::read_package(bytes).map_err(|e| e.to_string())?;
        pkg.name = name.to_string(); // read_package 填空串，这里覆盖为真实包名

        self.packages.insert(name.to_string(), pkg);
        Ok(())
    }

    /// 查图尺寸（path → (w, h) 像素）。供 layout/render 用。
    /// path 缺失或 w/h=0 → None（调用方 fallback 64×64）。
    pub fn image_size(&self, path: &str) -> Option<(u32, u32)> {
        self.image_sizes
            .get(path)
            .copied()
            .filter(|(w, h)| *w != 0 && *h != 0)
    }

    /// 批量灌图尺寸（后端读所有 atlas.json 合并后一次性推入；见 spec §6.4）。
    /// 覆盖式合并：同 path 后写赢。上万条也是 O(n) HashMap 插入，启动一次调用。
    pub fn set_image_sizes(&mut self, sizes: &[(String, u32, u32)]) {
        for (path, w, h) in sizes {
            self.image_sizes.insert(path.clone(), (*w, *h));
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

    /// 按 CSS id 属性查节点（首个匹配）。无 scene / 无匹配 → None。
    /// 供 FFI find_node_by_id：业务用 id 定位节点（注册 listener / 设 disabled）。
    pub fn find_node_by_id(&self, id: &str) -> Option<NodeId> {
        self.scene.as_ref().and_then(|s| s.find_by_id_attr(id))
    }

    /// 在 mount_subtree_root 的子树内找名为 name 的 Controller 挂载点 NodeId。
    /// driver 先 instantiate 拿到组件根，再 get_controller(root, "tab") 取句柄。
    /// DFS 遍历子树（含 mount_subtree_root 自身）找首个 data_controller == Some(name) 的节点。
    /// 返 None = 子树内无 data-controller="name"。无 scene → None。
    pub fn get_controller(&self, mount_subtree_root: NodeId, name: &str) -> Option<NodeId> {
        let scene = self.scene.as_ref()?;
        let mut found = None;
        let mut stack = vec![mount_subtree_root];
        while let Some(nid) = stack.pop() {
            if let Some(n) = scene.get(nid) {
                if n.data_controller.as_deref() == Some(name) {
                    found = Some(nid);
                    break;
                }
                stack.extend(n.children.iter().copied());
            }
        }
        found
    }

    /// 切 Controller 页。无效 mount（无 scene / 节点不存在 / 未挂 data_controller）→ 静默
    /// 返 -1（不 panic，照 FFI no-panic 约定）。prev != idx 时推一条 ControllerChangedEvent
    /// 进 pending_controller_events 供 FFI borrow_controller_changed_events pull。
    pub fn set_selected_index(&mut self, mount: NodeId, idx: i32) -> i32 {
        let Some(scene) = self.scene.as_mut() else {
            return -1;
        };
        // 校验 mount 确实挂了 controller（data_controller.is_some）——否则静默忽略。
        if scene
            .get(mount)
            .and_then(|n| n.data_controller.as_ref())
            .is_none()
        {
            return -1;
        }
        let prev = scene.set_controller_selected(mount, idx);
        if prev != idx {
            scene
                .pending_controller_events
                .push(ControllerChangedEvent {
                    mount_node: mount.0,
                    prev,
                    new: idx,
                });
        }
        prev
    }

    /// 读 Controller 当前选中页。无 scene / 无条目 → -1（调用方据 -1 判无 Controller）。
    pub fn get_selected_index(&self, mount: NodeId) -> i32 {
        self.scene
            .as_ref()
            .and_then(|s| s.controller_selected(mount))
            .unwrap_or(-1)
    }

    /// UI 挡住时游戏不响应点击。委托 PointerState（任一活跃槽命中非根）。
    pub fn is_pointer_on_ui(&self) -> bool {
        match &self.scene {
            None => false,
            Some(scene) => self.pointer_state.is_pointer_on_ui(scene),
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
        self.pending_dt = dt; // stash 给 tick_and_render 喂 tweens.update
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

    /// 查 (world_x, world_y) 落在 RichText 节点哪个链接上 → link_id（0=无/越界/非 RichText）。
    ///
    /// pull 模式：Unity Click 命中节点级 AABB 后调本函数细分到 fragment。独立于 hit_test，
    /// 不改 EventRecord ABI。复用 get_node_world_matrix 的 world_transforms 通路（merge blob
    /// 吞空 div 但 world_transforms 保留全节点）反变换世界点到节点本地坐标，再扫描该节点的
    /// rich_fragments（本地坐标矩形）做 rect.contains。
    pub fn rich_link_at(&self, node: NodeId, world_x: f32, world_y: f32) -> u32 {
        let scene = match self.scene.as_ref() {
            Some(s) => s,
            None => return 0,
        };
        // 取节点 world_matrix 反变换到本地坐标（fragment 矩形存本地坐标）。
        if scene.get(node).is_none() {
            return 0;
        }
        // RichText retired in Spec-2; rich_fragments side table is always empty now.
        let wm = scene
            .world_transforms
            .get(node.index())
            .copied()
            .unwrap_or(crate::transform::IDENTITY);
        let inv = crate::transform::inverse(&wm);
        let (lx, ly) = crate::transform::apply_point(&inv, world_x, world_y);
        let frags = match scene
            .rich_fragments
            .get(node.index())
            .and_then(|f| f.as_ref())
        {
            Some(f) => f,
            None => return 0,
        };
        for fr in frags {
            if lx >= fr.x && lx <= fr.x + fr.w && ly >= fr.y && ly <= fr.y + fr.h {
                return fr.link_id;
            }
        }
        0
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

    /// 拉脏页 page_idx 列表（写入 out，返实际数）。atlas 未用 / 无 scene → 0。
    pub fn font_atlas_dirty_pages(&self, out: &mut [u32]) -> usize {
        let dirty = self.glyph_atlas.dirty_pages();
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
        let (bytes, w, h) = self.glyph_atlas.page_bytes(page);
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
        self.glyph_atlas.clear_dirty();
    }

    /// 编程聚焦（照 fgui RequestFocus）。强制聚焦任意非 disabled 节点
    /// （含 tabindex=None/-1——request_focus 是编程 API，不查 tabindex）。
    /// disabled 拒 / 越界跳过。记 pending_focus_request，下 tick 最前消费（不直接写 last_events）。
    pub fn request_focus(&mut self, node_id: NodeId) {
        if let Some(scene) = self.scene.as_ref() {
            match scene.get(node_id) {
                None => return,
                Some(n) if n.interaction.flags.contains(NodeFlags::DISABLED) => return, // disabled 拒
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

    /// 注册 tween。start/end 取前 value_size 个分量（prop 决定 size）。
    /// duration<=0 → update 首帧即结束并产 complete。无 scene / 越界 node → update 跳过（不报错）。
    #[allow(clippy::too_many_arguments)] // 参数与 C# FFI 签名 1:1 对齐（同 text/layout.rs 惯例）
    pub fn tween(
        &mut self,
        node: NodeId,
        prop: crate::tween::TweenProp,
        start: [f32; 4],
        end: [f32; 4],
        ease: crate::tween::Ease,
        delay: f32,
        duration: f32,
        tag: u32,
    ) {
        self.tweens
            .tween(node, prop, start, end, ease, delay, duration, tag);
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
    /// spec §5.3：删节点联动清持久附属 map，防悬空 NodeId 残留。
    pub fn remove_node(&mut self, node: NodeId) {
        if let Some(scene) = self.scene.as_mut() {
            crate::scene::dynamic::remove_node(scene, &mut self.tweens, node);
        }
    }

    // ---- 动态建树 API（转调 scene::dynamic） ----

    /// scene 不存在则建空骨架（首次 create_root/create_node 调用时初始化）。
    /// spec §4.2：scene 初始由 create_root 建（load_package 不建 scene）。
    /// 多次调用幂等（已存在 scene → no-op）。`pub(crate)` 供集成测试直接初始化场景
    /// （如黄金等价测试需 instantiate 后把孤立根 push 进 scene.roots，不套额外 stage_root）。
    pub(crate) fn ensure_scene(&mut self) {
        if self.scene.is_none() {
            self.scene = Some(crate::scene::node::Scene::default());
            self.prev_node_hashes.clear(); // 新 scene → 无基线，下帧全 dirty
        }
    }

    /// 建根节点：create_node + roots.push(id)。返回新 NodeId。
    /// scene 不存在则首次调用建空骨架（spec：scene 初始由 create_root 建）。
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

    /// 改 Image 节点 src + 标 dirty_mesh。
    pub fn set_src(&mut self, node: NodeId, src: &str) -> Result<(), String> {
        crate::scene::dynamic::set_src(self.scene.as_mut().ok_or("no scene")?, node, src)
    }

    /// 改 base_style（apply_css）+ 标 dirty_mesh。下帧 rematch 从 base 重算 style。
    pub fn set_style(&mut self, node: NodeId, css: &str) -> Result<(), String> {
        crate::scene::dynamic::set_style(self.scene.as_mut().ok_or("no scene")?, node, css)
    }

    /// 设渲染复用键（虚拟列表 slot）。node 无效 → no-op。
    pub fn set_reuse_key(&mut self, node: NodeId, key: u32) {
        if let Some(scene) = self.scene.as_mut() {
            crate::scene::dynamic::set_reuse_key(scene, node, key);
        }
    }

    /// 从包克隆一个组件进当前 scene，返回组件根 NodeId（孤立，parent=None，调用方 append_child 挂载）。
    ///
    /// spec §4.2/§4.4：
    /// 1. 查 `packages[pkg].components[component]`，clone 出 ComponentTemplate（避开 packages/scene 双借）。
    /// 2. 遍历 template.nodes，按 parent_idx 序建 live Node（父先建于子），复用节点构造
    ///    （`create_node_from_template`：kind + baked style → base_style/style 初始 + clip_rect +
    ///    dirty_text + slotmap insert + id 回填），再填 classes/id_attr/draggable/tabindex/data_controller。
    ///    按 parent_idx 串子树（append_child 语义：parent.children.push + child.parent=Some(parent)）。
    ///    根（parent_idx=None）不串父，记录返回。
    /// 3. 伪类规则合并去重：遍历 template.dynamic_rules，相同选择器（ParsedSelector.eq）不重复加进
    ///    scene.dynamic_rules。规则按 class 匹配，多实例共享；hit_test 返具体 NodeId → 各实例独立 :hover。
    /// 4. scene 必须已存在（create_root 建过），否则 Err。
    ///
    /// 多实例独立：同组件多次 instantiate → 各自独立子树（NodeId 不同）+ 各自独立事件/伪类命中。
    /// id_attr 多实例约定限制：find_node_by_id 返首个匹配（不做核心 id 去重，YAGNI）。
    pub fn instantiate(&mut self, pkg: &str, component: &str) -> Result<NodeId, String> {
        let scene = self.scene.as_mut().ok_or("no scene (create_root first)")?;
        // clone 出 template 避开 packages + scene 双借（packages 在 self 上，scene 也在 self 上）。
        let template = self
            .packages
            .get(pkg)
            .and_then(|p| p.components.get(component))
            .cloned()
            .ok_or_else(|| format!("component `{component}` not in pkg `{pkg}`"))?;

        // 遍历 template.nodes 建树（父先建于子——parent_idx < i 由打包器/读保证）。
        // id_map[模板 idx] = live NodeId（slotmap 分配）。
        let mut id_map: Vec<Option<NodeId>> = vec![None; template.nodes.len()];
        let mut root_id: Option<NodeId> = None;
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
            let node_id =
                crate::scene::dynamic::create_node_from_template(scene, tn.kind, tn.style.clone());
            // 填 classes/id_attr/draggable/tabindex/data_controller（create_node_from_template 不填这些，同 create_node）
            let n = scene.get_mut(node_id).unwrap();
            n.classes = tn.classes.clone();
            n.id_attr = tn.id_attr.clone();
            n.interaction.draggable = tn.draggable;
            n.interaction.tabindex = tn.tabindex;
            n.data_controller = tn.data_controller.clone();
            if let Some(c) = &tn.content {
                scene.text_contents.insert(node_id, c.clone());
            }
            if let Some(src) = &tn.src {
                scene.image_srcs.insert(node_id, src.clone());
            }
            id_map[i] = Some(node_id);
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

        // 建 Controller registry：组件内 mount_node_idx → 活 NodeId（经 id_map）。
        // set_controller_selected 懒注册（无条目时建），此处显式建条目写 initial_selected_index。
        // 多实例独立：每次 instantiate 各自 id_map → 不同 NodeId → 独立 registry 条目。
        for c in &template.controllers {
            if let Some(&Some(mount_live)) = id_map.get(c.mount_node_idx as usize) {
                scene.set_controller_selected(mount_live, c.initial_selected_index);
            }
        }

        // 伪类规则合并去重：相同选择器（ParsedSelector PartialEq）不重复加。
        // 规则按 class 匹配，多实例共享同一规则条目；hit_test 返具体 NodeId → 各实例独立命中。
        for rule in &template.dynamic_rules.rules {
            let dup = scene
                .dynamic_rules
                .rules
                .iter()
                .any(|r| r.selector == rule.selector);
            if !dup {
                scene.dynamic_rules.rules.push(rule.clone());
            }
        }
        Ok(root)
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

    /// 本帧 Controller 切页事件（set_selected_index 推入；FFI borrow_controller_changed_events 读）。
    /// pull 模式：out_len 是 COUNT（非字节）。事件存活至下 tick start 清空——C# 在 tick 后、
    /// 下 tick 前的窗口内读（同 last_events 语义窗口）。
    pub fn controller_changed_events(&self) -> &[ControllerChangedEvent] {
        self.scene
            .as_ref()
            .map(|s| s.pending_controller_events.as_slice())
            .unwrap_or(&[])
    }

    /// 每帧管线（支柱1重排——rematch 提到 solve 前，伪类三类全当帧消费）：
    /// ①tween ②focus_request ③process（仲裁+拖拽写 scroll_pos；hit_test 读上帧 world，1帧延迟已认）
    /// ④scroll update ⑤process_keys ⑥rematch_pseudo_classes（提到 solve 前：改 layout/transform/colors
    /// 三类，本帧 solve+compute 全消费）⑥.5 transition drain（rematch 产请求 → kill 旧 tween + 提交新）
    /// ⑦solve（读 rematch 后 taffy_style）
    /// ⑧refresh_content_sizes ⑨compute_world_transforms（读 rematch 后 transform+scroll_pos）
    /// ⑩build_render_nodes
    ///
    /// **compute_world_transforms 时机**：rematch 之后、render 之前，每帧 1 次。
    /// transform 改由 rematch 写 style.transform → compute 同帧读 → world 含缩放。
    /// scroll_pos 同帧进 world matrix（spec §9.3）。
    /// **1 帧延迟语义**：hit_test 用上帧 world_transforms。首帧 world_transforms 为空，
    /// hit_test bounds guard 拦截（越界返 None → 未命中，零回归安全）。仲裁在 Down 未滚动前
    /// 不影响；clip 门控用 viewport 固定主导，不依赖每帧变换精度。
    pub fn tick_and_render(&mut self) -> FrameData {
        // 坑 102：FFI 入口绝不 panic。scene=None（load 前）早返空帧，不 expect
        // （cdylib .expect 遇 None non-unwinding abort 拖垮宿主进程）。
        let scene = match self.scene.as_mut() {
            Some(s) => s,
            None => return FrameData::default(),
        };
        // 清上帧残留的 Controller 切页事件。事件由 set_selected_index（tick 外 C# 调）推入，
        // C# 在上 tick 后、本 tick 前的窗口内已 borrow 读走。同 last_events 语义窗口
        // （last_events 每帧末覆写；pending_controller_events 每帧首清空）。
        scene.pending_controller_events.clear();
        let mut out: Vec<EventRecord> = Vec::new();
        // tween 推进（写 scene.anim + 产 complete 事件进 out）。须在 solve/compute_world_transforms 前。
        let dt = self.pending_dt;
        self.pending_dt = 0.0;
        self.tweens.update(dt, scene, &mut out);
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
        self.last_events = out;
        // 4. 伪类重匹配（提到 solve 前：改 taffy_style/transform/colors，本帧全部消费）
        rematch_pseudo_classes(scene);
        // 4.5 transition drain：rematch 检测可动画通道变化时推入 scene.pending_transitions。
        //     每个请求 kill 旧 (node,prop) tween（override 保留 mid-flight 末值，见 tween.rs kill）
        //     + 提交新 tween（start = mid-flight override → 无闪烁）。切页 kill 语义。
        //     借用：scene 经 self.scene.as_mut() 借；self.tweens 独立字段（同 tweens.update 访问形）。
        let reqs: Vec<crate::tween::TransitionRequest> =
            scene.pending_transitions.drain(..).collect();
        for r in reqs {
            self.tweens.kill(r.node, r.prop); // override 保留（mid-flight 值）
            self.tweens.tween(
                r.node,
                r.prop,
                r.start,
                r.end,
                r.ease,
                r.delay,
                r.duration,
                TRANSITION_TAG,
            );
        }
        // 5. solve（读 rematch 后的 taffy_style → layout_rect）
        // 核心知图尺寸（打包期 PNG IHDR 静态，存 Stage.image_sizes）。solve 查尺寸表算
        // Image intrinsic（三档：CSS > 真实像素 > 64×64）。不知图集（运行时纹理/UV 归 Unity）。
        solve(scene, &self.fonts, self.root_size, &self.image_sizes);
        // 6. content_size 填充（solve 后 content_size/viewport/overlap）
        crate::scroll::refresh_content_sizes(scene);
        // 7. compute_world_transforms（读 rematch 后 transform + scroll_pos → world）
        crate::scene::transform::compute_world_transforms(scene);
        // 8. 渲染（+ 合成 scrollbar）。传上帧 hash 基线，未变节点 change_level=Skip；
        //    返回新 hash 存 self.prev_node_hashes 供下帧比。
        // build_render_nodes 查 Stage.image_sizes 算九宫格 UV（slice_px / src_px）。
        // Image payload 带 path，UV 全图 (0,0)-(1,1)（无 atlas 子区），Unity 查 Sprite 拿真实 UV。
        let (frame, new_hashes, sort_keys, rich_fragments) = build_render_nodes(
            scene,
            &self.fonts,
            &self.prev_node_hashes,
            &self.image_sizes,
            &mut self.glyph_atlas,
        );
        scene.node_sort_keys = sort_keys;
        self.prev_node_hashes = new_hashes;
        // 写回 rich_fragments：resize 对齐 slotmap capacity（remove_node 后 idx 不变），
        // 再按 node_id 索引入表。
        scene
            .rich_fragments
            .resize_with(scene.nodes.capacity() + 1, || None);
        // 每帧先清空所有 slot，再写入本帧有 fragments 的 slot。
        // resize_with 只填充新增 slot，已有的 stale slot 不变——若不主动清空，
        // 上一帧有链接、本帧删了链接的节点会保留 stale fragments，
        // rich_link_at 读到已删 link_id。
        scene.rich_fragments.fill(None);
        for (node_id_u32, frags) in &rich_fragments {
            let idx = crate::scene::node::NodeId(*node_id_u32).index();
            if let Some(slot) = scene.rich_fragments.get_mut(idx) {
                *slot = Some(std::mem::take(&mut frags.clone()));
            }
        }
        frame
    }

    pub fn render_json(&mut self) -> String {
        let frame = self.tick_and_render();
        serde_json::to_string_pretty(&frame.nodes).unwrap()
    }
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

    #[test]
    fn get_node_computed_style_returns_snapshot() {
        let mut s = Stage::new((100.0, 100.0)).unwrap();
        let root = s.create_root("div", "").unwrap();
        let c = s
            .get_node_computed_style(root)
            .expect("root computed style");
        // 默认值（不依赖 rematch 时机）：opacity 1.0、display Flex。精确 cascade 值由 Task 3 验。
        assert_eq!(c.opacity, 1.0);
        assert_eq!(
            s.get_node_computed_style(crate::scene::NodeId::INVALID),
            None,
            "invalid node -> None"
        );
    }
}
