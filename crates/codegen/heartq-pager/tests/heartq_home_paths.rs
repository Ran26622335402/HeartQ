//! `HEARTQ_HOME` override tests in an isolated binary so `heartq_home()`'s
//! process-wide `OnceLock` initializes from the overridden env var.

use std::path::PathBuf;

#[test]
fn heartq_home_override_path_helpers() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let heartq_home = tmp.path().to_path_buf();
    unsafe {
        std::env::set_var("HEARTQ_HOME", &heartq_home);
    }

    assert_eq!(
        heartq_pager::util::pager_toml_path(),
        heartq_home.join("pager.toml")
    );
    assert_eq!(
        heartq_pager::util::display_heartq_home_prefix(),
        "$HEARTQ_HOME"
    );
    assert_eq!(
        heartq_pager::util::display_user_heartq_path("config.toml"),
        "$HEARTQ_HOME/config.toml"
    );

    let memory_path = heartq_home.join("memory/MEMORY.md");
    assert_eq!(
        heartq_pager::util::abbreviate_path(&memory_path.display().to_string()),
        "$HEARTQ_HOME/memory/MEMORY.md"
    );

    // Copy-toast paths follow the same abbreviation convention, so a custom
    // $HEARTQ_HOME outside $HOME still displays short.
    assert_eq!(
        heartq_pager::clipboard::display_copy_path(&heartq_home.join("last-copy.txt")),
        "$HEARTQ_HOME/last-copy.txt"
    );

    assert!(heartq_pager::util::is_under_user_heartq_home(&memory_path));
    assert!(!heartq_pager::util::is_under_user_heartq_home(
        PathBuf::from("/tmp/other").as_path()
    ));
}
