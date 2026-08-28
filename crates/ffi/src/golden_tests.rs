//! golden blob 产器：固定场景 → 帧 blob + 事件流字节，落盘 `tests/dotnet/golden/`
//! 供 C# 侧（GoldenBlobTests）跨语言镜像对拍——FrameBlob 23 列布局 / EventRecord
//! 32B 位布局（node_id u64，#26，EventRecord 32B）在 C# 侧此前零断言，magic+version 防整体漂移、防不住列语义错位
//! （列序对调仍 v13 通过）。
//!
//! 更新（改 blob 布局 / 改本场景后）：
//! `IKATGUI_UPDATE_GOLDEN=1 cargo test -p ikat_ffi_c --lib golden`
//!
//! Rust 侧**不做**跨平台字节等值门：渐变方向 / 圆角 mesh 走 libm sin/cos，
//! Windows 与 Linux 的末位 ulp 可能不同。货币性（golden 未过期）由两道信号保：
//! ① 本模块校验入库文件的 magic+version == 当前 `blob::VERSION`；
//! ② C# `FrameBlob.IsValid`（版本耦合）——bump 后忘再生成即红。

use crate::test_helpers::stage_new_with_dejavu;
use crate::*;
use ikat_core::input::{EventRecord, PointerEvent, PointerKind};
use ikat_core::scene::node::{ControlState, NodeId};

/// golden 落盘目录（C# HeadlessTests 按相对路径 `golden/` 消费，csproj 拷到输出目录）。
const GOLDEN_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/dotnet/golden");

fn mk_node(h: *mut StageHandle, kind: &str, css: &str) -> u64 {
    let id = ikat_stage_create_node(h, kind.as_ptr(), kind.len(), css.as_ptr(), css.len());
    assert_ne!(id, u64::MAX, "create_node({kind}) failed");
    id
}

fn set_text(h: *mut StageHandle, node: u64, text: &str) {
    let rc = ikat_stage_set_text(h, node, text.as_ptr(), text.len());
    assert_eq!(rc, 0, "set_text failed");
}

fn set_src(h: *mut StageHandle, node: u64, src: &str) {
    let rc = ikat_stage_set_src(h, node, src.as_ptr(), src.len());
    assert_eq!(rc, 0, "set_src failed");
}

/// 建 golden 场景：刻意覆盖全部 23 列的语义来源——
/// 文本 mesh（program=1）/ 图片 path 表 / 渐变（program=6 + grad_params）/
/// box-shadow blur（program=5 + shadow_params + 圆角）/ opacity（alpha 列 < 1）/
/// user_transform（world 矩阵非纯平移）/ overflow:hidden（clip 表）/
/// Progress 控件（controls 表 → sync_control_visuals）。
fn build_golden_bytes() -> (Vec<u8>, Vec<u8>) {
    let h = stage_new_with_dejavu(800.0, 600.0);
    let root_css =
        "width:800px;height:600px;display:flex;flex-direction:column;background-color:#1a2233";
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, root_css.as_ptr(), root_css.len());
    assert_ne!(root, u64::MAX, "create_root failed");

    let title = mk_node(h, "span", "font-size:20px;color:#ffffff");
    set_text(h, title, "Golden 对拍 Hello 金");

    let img = mk_node(h, "img", "width:48px;height:48px");
    set_src(h, img, "icons/star.png");

    let grad = mk_node(
        h,
        "div",
        "width:400px;height:80px;background:linear-gradient(to right,#ff4400,transparent 60%)",
    );

    let shadow = mk_node(
        h,
        "div",
        "width:300px;height:60px;border-radius:12px;box-shadow:0 2px 8px rgba(0,0,0,0.45)",
    );

    let faded = mk_node(
        h,
        "div",
        "width:200px;height:40px;opacity:0.35;background-color:#00ff00",
    );

    let mover = mk_node(h, "div", "width:100px;height:30px;background-color:#445566");
    // 带缩放的 transform：world 矩阵 2×2 部分非 identity（纯平移会被 C# IsPureTranslation
    // 判真，锁不住矩阵列）。
    let rc = ikat_stage_set_transform(h, mover, 24.0, 16.0, 1.15, 1.15, 0.0, 0.0, 0.0);
    assert_eq!(rc, 0, "set_transform failed");

    let clipper = mk_node(h, "div", "width:500px;height:60px;overflow:hidden");
    let clipped = mk_node(
        h,
        "div",
        "width:480px;height:200px;background-color:#778899",
    );
    let rc = ikat_stage_append_child(h, clipper, clipped);
    assert_eq!(rc, 0, "append clipped failed");

    let bar = mk_node(h, "div", "width:300px;height:10px");
    {
        let sh = unsafe { &mut *h };
        let scene = sh.stage.scene.as_mut().expect("scene built");
        scene.controls.ensure(
            NodeId(bar),
            ControlState::Progress {
                value: 0.55,
                min: 0.0,
                max: 1.0,
                indeterminate: false,
            },
        );
    }

    for child in [title, img, grad, shadow, faded, mover, clipper, bar] {
        let rc = ikat_stage_append_child(h, root, child);
        assert_eq!(rc, 0, "append child failed");
    }

    // 首 tick：布局 + 渲染。帧 golden 在此借用——首帧 prev_node_hashes 空，全节点
    // change_level=Full + mesh 落 arena，是对拍信息量最大的一帧（后续 tick 未变节点
    // Skip，锁不到 mesh/Full 语义）。指针下 tick 失效，立即 to_vec 固化。
    ikat_stage_tick(h, 0.016);
    let mut frame_len = 0usize;
    let frame_ptr = ikat_stage_borrow_frame(h, &mut frame_len);
    assert!(!frame_ptr.is_null() && frame_len > 0, "frame blob empty");
    let frame = unsafe { std::slice::from_raw_parts(frame_ptr, frame_len) }.to_vec();
    // 指针 Down/Up：(150,120) 落点事件流——坐标/类型/命中 node_id 全进 golden，
    // C# 侧按 RawEventRecord 位布局断言。
    let down = PointerEvent {
        kind: PointerKind::Down,
        button: 0,
        pad: [0; 2],
        touch_id: -1,
        x: 150.0,
        y: 120.0,
    };
    ikat_stage_set_input(h, &down, 1);
    ikat_stage_tick(h, 0.016);
    let up = PointerEvent {
        kind: PointerKind::Up,
        button: 0,
        pad: [0; 2],
        touch_id: -1,
        x: 150.0,
        y: 120.0,
    };
    ikat_stage_set_input(h, &up, 1);
    ikat_stage_tick(h, 0.016);

    let mut event_count = 0usize;
    let events_ptr = ikat_stage_borrow_events(h, &mut event_count);
    assert!(!events_ptr.is_null() && event_count > 0, "events empty");
    let events = unsafe {
        std::slice::from_raw_parts(
            events_ptr as *const u8,
            event_count * std::mem::size_of::<EventRecord>(),
        )
    }
    .to_vec();

    ikat_stage_free(h);
    (frame, events)
}

/// 进程内确定性：同一场景两次构建 → 字节全等。这是 C# golden 对拍有意义的前提
/// （生产者不确定性 = golden 锁了个噪声）。不比对入库文件（跨平台 libm 差，
/// 见模块 doc）。
#[test]
fn golden_producer_is_deterministic() {
    let (f1, e1) = build_golden_bytes();
    let (f2, e2) = build_golden_bytes();
    assert_eq!(f1, f2, "frame blob must be deterministic");
    assert_eq!(e1, e2, "event stream must be deterministic");
}

/// 入库 golden 的货币性信号：magic + version == 当前 `blob::VERSION`。
/// blob 布局演进 bump 版本而忘再生成 golden → 此处红（同版本内漂移由 C# 语义
/// 断言 + Unity 真机冒烟兜底，见模块 doc 取舍）。
#[test]
fn golden_files_match_current_blob_version() {
    let frame = std::fs::read(format!("{GOLDEN_DIR}/frame-blob.bin"))
        .expect("golden frame-blob.bin 缺失——跑 IKATGUI_UPDATE_GOLDEN=1 cargo test -p ikat_ffi_c --lib golden 生成");
    assert!(frame.len() > 12);
    assert_eq!(
        u32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]),
        0x4D4F_4F4C,
        "frame golden magic"
    );
    assert_eq!(
        u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]),
        crate::blob::VERSION,
        "golden 帧版本落后/超前于 blob::VERSION——再生成 golden"
    );
    let events = std::fs::read(format!("{GOLDEN_DIR}/events.bin"))
        .expect("golden events.bin 缺失——同上再生成");
    assert!(
        !events.is_empty() && events.len() % std::mem::size_of::<EventRecord>() == 0,
        "events golden 须为非空 EventRecord[]"
    );
}

/// 再生成入口：设 `IKATGUI_UPDATE_GOLDEN=1` 时把当前场景字节落盘。
/// 不设时 no-op（生成机专用，CI 不写仓库文件）。
#[test]
fn write_golden_files_when_env_set() {
    if std::env::var("IKATGUI_UPDATE_GOLDEN").ok().as_deref() != Some("1") {
        return;
    }
    let (frame, events) = build_golden_bytes();
    std::fs::create_dir_all(GOLDEN_DIR).expect("create golden dir");
    std::fs::write(format!("{GOLDEN_DIR}/frame-blob.bin"), &frame).expect("write frame golden");
    std::fs::write(format!("{GOLDEN_DIR}/events.bin"), &events).expect("write events golden");
    eprintln!(
        "golden updated: frame {}B, events {}B -> {GOLDEN_DIR}",
        frame.len(),
        events.len()
    );
}
