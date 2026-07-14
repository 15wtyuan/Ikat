//! Packer library: atlas packing, font copying, runtime manifest.
//!
//! The HTML -> .pkg.bin compilation path was removed in R1.1 and will be
//! rebuilt in R3 using the fence crate. Until then, the packer handles only
//! atlases, fonts, and the runtime manifest.

pub mod atlas;
pub mod build;
pub mod runtime;
pub mod workspace;
