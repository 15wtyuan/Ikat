    use super::*;
    use std::ffi::CStr;

    #[test]
    fn version_returns_c_string() {
        unsafe {
            let s = CStr::from_ptr(loomgui_version() as *const i8);
            assert_eq!(s.to_str().unwrap(), "v1e");
        }
    }

    /// FFI tween：注册 opacity tween → tick 结束 → borrow_events 验 complete(tag)。
    /// borrow_events 返回 *const u8 + len=记录数（非字节数；见 lib.rs:237 注释）。
    /// 单切片：按记录数 len 切 typed slice，扫 event_type=EVT_TWEEN_COMPLETE && touch_id==tag。
    #[cfg(feature = "parse")]
    #[test]
    fn stage_tween_complete_event_via_ffi() {
        let fp = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../loomgui_core/tests/fixtures/DejaVuSans.ttf"
        );
        let fplen = fp.len();
        let h = loomgui_stage_new(fp.as_ptr() as *const u8, fplen, 200.0, 100.0);
        let html = b"<div class=\"b\"></div>";
        let css = b".b{width:100px;height:50px;}";
        loomgui_stage_load_html(h, html.as_ptr(), html.len(), css.as_ptr(), css.len());
        // 经 slotmap 分配的真实根 NodeId（动态树：root_id 非 0，是 idx<<12|gen）。
        // 传 node_id=0 会因 scene.get(NodeId(0)) 悬空 → update 跳过 → 无 complete 事件。
        let root_id = unsafe { (*h).stage.scene.as_ref().unwrap().roots[0].0 };
        let start = [0.0f32; 4];
        let end = [1.0f32, 0.0, 0.0, 0.0];
        // prop=0 (Opacity), ease=0 (Linear), duration=1.0, delay=0, tag=55
        loomgui_stage_tween(h, root_id, 0, start.as_ptr(), end.as_ptr(), 1.0, 0, 0.0, 55);
        loomgui_stage_tick(h, 1.0); // 推进到结束
        let mut len = 0usize;
        let p = loomgui_stage_borrow_events(h, &mut len);
        // len 是记录数（borrow_events out_len = events.len()）；直接切 typed slice。
        let recs = unsafe {
            std::slice::from_raw_parts(p as *const loomgui_core::input::EventRecord, len)
        };
        assert!(
            recs.iter().any(
                |e| e.event_type == loomgui_core::input::EVT_TWEEN_COMPLETE && e.touch_id == 55
            ),
            "FFI tween 结束 → complete(tag=55)"
        );
        loomgui_stage_free(h);
    }
