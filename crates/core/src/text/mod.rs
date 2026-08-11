//! Text 层：字体度量 + 断行 → TextLayout SOA 三表（glyphs/runs/lines）。

pub mod atlas;
pub mod font_effect;
pub mod hit_test;
pub mod layout;
pub mod rich;
pub mod rich_compile;
pub mod sdf;
