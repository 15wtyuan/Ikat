#![cfg(feature = "parse")]

#[cfg(feature = "parse")]
pub mod annotate;
#[cfg(feature = "parse")]
pub mod css_resolve;
#[cfg(feature = "parse")]
pub mod diagnostic;
#[cfg(feature = "parse")]
pub mod fence_gate;
#[cfg(feature = "parse")]
pub mod ir;
#[cfg(feature = "parse")]
pub mod pipeline;
#[cfg(feature = "parse")]
pub mod schema;
#[cfg(feature = "parse")]
pub mod structural;
#[cfg(feature = "parse")]
pub mod tree_builder;

#[cfg(feature = "parse")]
pub use pipeline::{parse_template, ParsedTemplate};
