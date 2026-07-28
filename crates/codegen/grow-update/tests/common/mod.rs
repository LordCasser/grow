//! Shared helpers for integration tests.
//!
//! Each `tests/*.rs` integration test is its own binary, so each binary has
//! its own `OnceLock<GROW_HOME>`. The helpers below ensure the per-binary
//! initialization is identical: same env-var set, same isolation guarantees,
//! same reset between tests.
//!
//! Mirrors the GROW_HOME isolation pattern used in other integration tests.
//!
//! ## Usage
//!
//! ```ignore
//! mod common;
//! use common::{test_home, reset_home};
//!
//! #[tokio::test]
//! #[serial_test::serial]
//! async fn my_test() {
//!     let _ = test_home();   // initializes GROW_HOME once per binary
//!     reset_home();          // wipes state between tests
//!     // ...
//! }
//! ```

#![allow(dead_code)] // each test binary uses a different subset

use std::path::PathBuf;
use std::sync::OnceLock;

// ─────────────────────────────────────────────────────────────────────────────
// GROW_HOME isolation
// ─────────────────────────────────────────────────────────────────────────────

/// Returns a process-wide test `GROW_HOME`, initialized exactly once per test
/// binary. Once initialized, `grow_config::grow_home()` will resolve to
/// this directory for the lifetime of the process.
///
pub fn test_home() -> &'static PathBuf {
    static HOME: OnceLock<PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.keep();
        // SAFETY: called once at OnceLock init, before any other thread touches
        // these env vars. Tests using this helper must be `#[serial]`.
        unsafe {
            std::env::set_var("GROW_HOME", &path);
            std::env::remove_var("GROW_TEST_VERSION");
        }
        path
    })
}

/// Wipe state in `GROW_HOME` between tests so each test sees a clean home.
/// Removes the well-known files and subdirectories the update path writes,
/// and clears env vars that individual tests may set.
pub fn reset_home() {
    let home = test_home();
    let _ = std::fs::remove_file(home.join("config.toml"));
    let _ = std::fs::remove_file(home.join("version.json"));
    let _ = std::fs::remove_file(home.join("version.json.tmp"));
    let _ = std::fs::remove_dir_all(home.join("bin"));
    let _ = std::fs::remove_dir_all(home.join("downloads"));
    // SAFETY: tests using this helper must be `#[serial]`.
    unsafe {
        std::env::remove_var("GROW_TEST_VERSION");
    }
}
