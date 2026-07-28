//! Shared utilities used by both `grow-shell` and its downstream clients
//! (e.g. `grow-pager-render`). This crate sits upstream of `grow-shell`
//! so it must never depend on it.

pub mod clipboard;
pub mod placeholder_images;
pub mod session;
pub mod stderr;
pub mod ui_config;
