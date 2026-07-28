//! `GROW_HOME` override tests in an isolated binary so `grow_home()`'s
//! process-wide `OnceLock` initializes from the overridden env var.

use std::path::PathBuf;

#[test]
fn grow_home_override_path_helpers() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let grow_home = tmp.path().to_path_buf();
    unsafe {
        std::env::set_var("GROW_HOME", &grow_home);
    }

    assert_eq!(
        grow_pager::util::pager_toml_path(),
        grow_home.join("pager.toml")
    );
    assert_eq!(grow_pager::util::display_grow_home_prefix(), "$GROW_HOME");
    assert_eq!(
        grow_pager::util::display_user_grow_path("config.toml"),
        "$GROW_HOME/config.toml"
    );

    let memory_path = grow_home.join("memory/MEMORY.md");
    assert_eq!(
        grow_pager::util::abbreviate_path(&memory_path.display().to_string()),
        "$GROW_HOME/memory/MEMORY.md"
    );

    // Copy-toast paths follow the same abbreviation convention, so a custom
    // $GROW_HOME outside $HOME still displays short.
    assert_eq!(
        grow_pager::clipboard::display_copy_path(&grow_home.join("last-copy.txt")),
        "$GROW_HOME/last-copy.txt"
    );

    assert!(grow_pager::util::is_under_user_grow_home(&memory_path));
    assert!(!grow_pager::util::is_under_user_grow_home(
        PathBuf::from("/tmp/other").as_path()
    ));
}
