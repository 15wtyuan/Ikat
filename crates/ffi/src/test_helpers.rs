//! ffi internal test helper. Uses `use crate::*` to access extern "C" functions + private StageHandle.
use crate::*;

/// Create a Stage via FFI and register the test default DejaVu font.
/// Panic = test failure (test-internal use only).
pub(crate) fn stage_new_with_dejavu(w: f32, h: f32) -> *mut StageHandle {
    let h = yio_stage_new(w, h);
    assert!(!h.is_null(), "stage_new must succeed");
    let font_bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../core/tests/fixtures/DejaVuSans.ttf"
    ))
    .expect("DejaVuSans.ttf fixture must exist");
    let family = b"DejaVu";
    let rc = yio_stage_register_font(
        h,
        family.as_ptr(),
        family.len(),
        font_bytes.as_ptr(),
        font_bytes.len(),
        1,
    );
    assert_eq!(rc, 0, "register_font DejaVu must return 0");
    h
}
