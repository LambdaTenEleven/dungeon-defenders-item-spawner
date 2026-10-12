//! Local package metadata, preset classification, and versioned catalog caches.
mod model;
pub use model::*;
pub mod cache;
pub mod package;
pub mod rules;
pub use cache::{cache_root, load_cached, scan, scan_at};

pub const SCHEMA_VERSION: u32 = 1;
pub const SCANNER_VERSION: u32 = 1;
pub const RULES_VERSION: u32 = 2;
