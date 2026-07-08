## Task 9: text-decoration -- Report

**Status:** done

**Commit:** `34a53c9`

**Files changed:**
- `loomgui_core/src/text/atlas.rs` -- `GlyphAtlas::ensure_solid()` (1x1 white pixel, sentinel key)
- `loomgui_core/src/text/layout.rs` -- `Glyph.advance` field + store at both creation sites
- `loomgui_core/src/render/mod.rs` -- decoration quad logic in `build_text_mesh`
- `loomgui_core/src/render/tests.rs` -- 3 new tests

**Test summary (620 total, 0 failed):**
- `rich_deco_underline_adds_quad` -- underline run produces 12 verts (8 glyph + 4 deco), all color = run.color (red)
- `rich_deco_strike_adds_quad` -- strike run produces >8 verts, all color = run.color (blue)
- `ensure_solid_hit_returns_same_uv` -- solid pixel 2nd call hits cache, returns identical UV, px_w=px_h=1

**Design notes:**
- `Glyph.advance: f32` added (needed beyond the brief's listed files to compute per-run total advance width in `build_text_mesh`).
- Decoration quads use BL/BR/TR/TL vertex order (CCW winding, matches existing glyph quad convention).
- `underline_metrics()` in ttf-parser 0.20 returns `LineMetrics { position: i16, thickness: i16 }` in design units. Verified against crate source.
- Solid pixel atlas page: may differ from glyph pages. Decoration quad entry goes to `solid.page` (BTreeMap lookup), independent of glyph pages.

**Concerns:** none.
