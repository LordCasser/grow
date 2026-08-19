//! I/O integration tests for the auto-update crate.
//!
//! These tests touch global process state — `GROW_HOME` (a `OnceLock` in
//! `config`) and `GROW_TEST_VERSION` — so they
//! must run serially. Once `GROW_HOME` is initialized for a process, it can't
//! be changed; we set it from a single shared `OnceLock` and reset the
//! contents of the directory between tests.
//!
//! The patterns here mirror the GROW_HOME isolation used in other
//! integration tests.

/// reqwest is built with `rustls-no-provider` (see the vendoring notes on the
/// workspace's rustls setup): production installs the ring provider at CLI
/// startup, but test binaries bypass startup, so install it once here. The
/// install-path integration tests exercise the full
/// `install_gh_release_from` pipeline, which downloads via reqwest.
#[ctor::ctor]
fn install_rustls_provider() {
    diagnostics::tls::install_ring_provider_once();
}

mod common;

use std::path::PathBuf;
use std::time::Duration;

use serial_test::serial;

use common::{reset_home, test_home};
use update::write_version_cache;

/// Path to the version cache file inside the test home.
fn version_cache_path() -> PathBuf {
    test_home().join("version.json")
}

/// Local alias kept so existing test bodies don't need to change.
fn reset() {
    reset_home();
}

// ─────────────────────────────────────────────────────────────────────────────
// write_version_cache
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
#[serial]
async fn write_version_cache_creates_file_at_grow_home() {
    let _ = test_home();
    reset();

    write_version_cache("0.1.180", None).await;

    let path = version_cache_path();
    assert!(
        path.exists(),
        "version.json should exist at {}",
        path.display()
    );

    let body = std::fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["version"], "0.1.180");
    assert!(
        parsed["checked_at"].as_str().is_some(),
        "checked_at should be a string: {body}"
    );
}

#[tokio::test]
#[serial]
async fn write_version_cache_overwrites_existing_atomically() {
    let _ = test_home();
    reset();

    write_version_cache("0.1.180", None).await;
    write_version_cache("0.1.181", None).await;

    let body = std::fs::read_to_string(version_cache_path()).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        parsed["version"], "0.1.181",
        "second write must overwrite first"
    );
}

#[tokio::test]
#[serial]
async fn write_version_cache_does_not_leave_tmp_file_behind() {
    let _ = test_home();
    reset();

    write_version_cache("0.1.180", None).await;

    let tmp = test_home().join("version.json.tmp");
    assert!(
        !tmp.exists(),
        "atomic rename must clean up tmp file: {}",
        tmp.display()
    );
}

#[tokio::test]
#[serial]
async fn write_version_cache_writes_valid_json_object() {
    let _ = test_home();
    reset();

    write_version_cache("0.1.182-alpha.3", None).await;

    let body = std::fs::read_to_string(version_cache_path()).unwrap();
    // Must parse as JSON.
    let parsed: serde_json::Value =
        serde_json::from_str(&body).unwrap_or_else(|e| panic!("not valid JSON: {e}\nbody: {body}"));
    let obj = parsed.as_object().unwrap();
    assert!(obj.contains_key("version"));
    assert!(obj.contains_key("checked_at"));
    assert_eq!(parsed["version"], "0.1.182-alpha.3");
}

#[tokio::test]
#[serial]
async fn write_version_cache_records_recent_timestamp() {
    let _ = test_home();
    reset();

    let before = time::OffsetDateTime::now_utc();
    write_version_cache("0.1.180", None).await;
    let after = time::OffsetDateTime::now_utc();

    let body = std::fs::read_to_string(version_cache_path()).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    let ts_str = parsed["checked_at"].as_str().unwrap();
    let ts = time::OffsetDateTime::parse(ts_str, &time::format_description::well_known::Rfc3339)
        .unwrap();

    assert!(
        ts >= before - Duration::from_secs(5) && ts <= after + Duration::from_secs(5),
        "timestamp should be within the test window: ts={ts}, before={before}, after={after}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// is_version_cache_fresh — exercised via the public re-export. Each scenario
// writes the file directly so we can control the timestamp.
// ─────────────────────────────────────────────────────────────────────────────

/// Write a `GrowVersion`-shaped JSON file with an arbitrary timestamp.
fn write_cache_with_timestamp(version: &str, ts: time::OffsetDateTime) {
    let ts_str = ts
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    let body = serde_json::json!({
        "version": version,
        "checked_at": ts_str,
    });
    std::fs::write(
        version_cache_path(),
        serde_json::to_vec_pretty(&body).unwrap(),
    )
    .unwrap();
}

/// Re-implement the cache-freshness check using the public API. We can't
/// import the private `is_version_cache_fresh` directly, but we can verify
/// its on-disk contract: file shape + freshness logic via the public
/// `GrowVersion` JSON layout.
async fn cache_is_fresh() -> bool {
    // Mirror the implementation: look at version.json under GROW_HOME,
    // parse, and check the TTL.
    let path = version_cache_path();
    let Ok(body) = tokio::fs::read_to_string(&path).await else {
        return false;
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&body) else {
        return false;
    };
    let Some(ts_str) = parsed["checked_at"].as_str() else {
        return false;
    };
    let Ok(ts) =
        time::OffsetDateTime::parse(ts_str, &time::format_description::well_known::Rfc3339)
    else {
        return false;
    };
    let now = time::OffsetDateTime::now_utc();
    now - ts < Duration::from_secs(60 * 30)
}

#[tokio::test]
#[serial]
async fn version_cache_is_fresh_after_write() {
    let _ = test_home();
    reset();

    write_version_cache("0.1.180", None).await;
    assert!(
        cache_is_fresh().await,
        "cache should be fresh right after write"
    );
}

#[tokio::test]
#[serial]
async fn version_cache_is_stale_when_old() {
    let _ = test_home();
    reset();

    let two_hours_ago = time::OffsetDateTime::now_utc() - Duration::from_secs(2 * 60 * 60);
    write_cache_with_timestamp("0.1.180", two_hours_ago);

    assert!(
        !cache_is_fresh().await,
        "2-hour-old cache should be stale (TTL is 30 min)"
    );
}

#[tokio::test]
#[serial]
async fn version_cache_missing_file_is_not_fresh() {
    let _ = test_home();
    reset();

    assert!(
        !cache_is_fresh().await,
        "missing file should not be considered fresh"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// version.json wire format — the on-disk file is read by every grow launch.
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
#[serial]
async fn version_cache_file_is_round_trippable() {
    let _ = test_home();
    reset();

    write_version_cache("0.1.182-alpha.3", Some("0.1.180")).await;

    let body = std::fs::read_to_string(version_cache_path()).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();

    // The shape must match what a manually-written file would look like.
    let manual = serde_json::json!({
        "version": parsed["version"].as_str().unwrap(),
        "stable_version": parsed["stable_version"].as_str().unwrap(),
        "checked_at": parsed["checked_at"].as_str().unwrap(),
    });
    assert_eq!(parsed, manual);
}

#[tokio::test]
#[serial]
async fn write_version_cache_handles_long_prerelease_string() {
    let _ = test_home();
    reset();

    // Realistic alpha string with multi-segment pre-release id.
    write_version_cache("0.1.190-alpha.42.beta.7", None).await;

    let body = std::fs::read_to_string(version_cache_path()).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["version"], "0.1.190-alpha.42.beta.7");
}

#[tokio::test]
#[serial]
async fn write_version_cache_idempotent_for_same_version() {
    let _ = test_home();
    reset();

    write_version_cache("0.1.180", None).await;
    let body1 = std::fs::read_to_string(version_cache_path()).unwrap();
    // Force a small wait so the timestamp could differ.
    tokio::time::sleep(Duration::from_millis(50)).await;
    write_version_cache("0.1.180", None).await;
    let body2 = std::fs::read_to_string(version_cache_path()).unwrap();

    // Both writes should leave the same version field, but timestamps may
    // differ — verify the version is preserved.
    let v1: serde_json::Value = serde_json::from_str(&body1).unwrap();
    let v2: serde_json::Value = serde_json::from_str(&body2).unwrap();
    assert_eq!(v1["version"], v2["version"]);
    assert_eq!(v1["version"], "0.1.180");
}

// ─────────────────────────────────────────────────────────────────────────────
// get_installed_version env override
//
// The function honors `GROW_TEST_VERSION` for testing. We exercise it
// via the public re-export only — no private items leaked.
// ─────────────────────────────────────────────────────────────────────────────
//
// Note: `get_installed_version` is not re-exported from `lib.rs`, but
// it's `pub` from `version` module and accessible via `version::`.

#[tokio::test]
#[serial]
async fn get_installed_version_uses_env_var_override() {
    let _ = test_home();
    reset();

    unsafe {
        std::env::set_var("GROW_TEST_VERSION", "9.9.9");
    }
    let v = update::version::get_installed_version();
    assert_eq!(v, "9.9.9");
    unsafe {
        std::env::remove_var("GROW_TEST_VERSION");
    }
}

#[tokio::test]
#[serial]
async fn get_installed_version_falls_back_to_cargo_pkg_version_when_env_unset() {
    let _ = test_home();
    reset();

    unsafe {
        std::env::remove_var("GROW_TEST_VERSION");
    }
    let v = update::version::get_installed_version();
    // The compile-time CARGO_PKG_VERSION must be a parseable semver string.
    let _: semver::Version = v
        .parse()
        .unwrap_or_else(|e| panic!("CARGO_PKG_VERSION is not a valid semver: '{v}': {e}"));
}

#[tokio::test]
#[serial]
async fn get_installed_version_with_env_var_takes_precedence() {
    let _ = test_home();
    reset();

    let real = {
        unsafe {
            std::env::remove_var("GROW_TEST_VERSION");
        }
        update::version::get_installed_version()
    };

    unsafe {
        std::env::set_var("GROW_TEST_VERSION", "0.0.0-test");
    }
    let overridden = update::version::get_installed_version();
    assert_ne!(real, overridden);
    assert_eq!(overridden, "0.0.0-test");

    unsafe {
        std::env::remove_var("GROW_TEST_VERSION");
    }
}

#[tokio::test]
#[serial]
async fn get_installed_version_handles_alpha_prerelease_in_env() {
    let _ = test_home();
    reset();

    unsafe {
        std::env::set_var("GROW_TEST_VERSION", "0.1.200-alpha.5");
    }
    let v = update::version::get_installed_version();
    assert_eq!(v, "0.1.200-alpha.5");
    unsafe {
        std::env::remove_var("GROW_TEST_VERSION");
    }
}

#[tokio::test]
#[serial]
async fn get_installed_version_does_not_validate_env_var_format() {
    // The function returns whatever's in the env var verbatim, even garbage.
    // Document this so callers know they need to validate downstream.
    let _ = test_home();
    reset();

    unsafe {
        std::env::set_var("GROW_TEST_VERSION", "not-a-version");
    }
    let v = update::version::get_installed_version();
    assert_eq!(v, "not-a-version");
    unsafe {
        std::env::remove_var("GROW_TEST_VERSION");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Install-target landing — full pipeline via `install_gh_release_from`
// (wiremock + tar/flate2 fixture). The fixture is an executable script
// `#!/bin/sh\necho "grow <version>"`, which the smoke test and the version
// probe exec. Unix-only: Windows cannot exec shebang scripts.
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(unix)]
mod install_pipeline {
    use std::path::{Path, PathBuf};

    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use update::auto_update::{
        InstallTarget, TargetKind, install_gh_release_from, land_binary_on_target,
        probe_target_version, resolve_install_target,
    };

    /// Release asset platform for this test binary (mirror of the Unix
    /// branches of `detect_platform`).
    fn test_platform() -> &'static str {
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            "macos-aarch64"
        } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
            "macos-x86_64"
        } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
            "linux-aarch64"
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            "linux-x86_64"
        } else {
            panic!("unsupported test platform");
        }
    }

    /// A one-file release archive whose `grow` entry echoes `grow <version>`.
    fn release_fixture(version: &str) -> Vec<u8> {
        let body = format!("#!/bin/sh\necho \"grow {version}\"\n");
        let file = std::io::Cursor::new(Vec::new());
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        archive
            .append_data(&mut header, "grow", body.as_bytes())
            .unwrap();
        let encoder = archive.into_inner().unwrap();
        encoder.finish().unwrap().into_inner()
    }

    fn make_executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn assert_executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o111,
            0,
            "{} must be executable",
            path.display()
        );
    }

    /// Snapshot of PATH/HOME restored on drop, so a panicking install test
    /// can't leak env changes into later serial tests.
    struct EnvGuard {
        path: Option<std::ffi::OsString>,
        home: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn capture() -> Self {
            Self {
                path: std::env::var_os("PATH"),
                home: std::env::var_os("HOME"),
            }
        }
        fn set_path(&self, value: &Path) {
            unsafe { std::env::set_var("PATH", value) };
        }
        fn set_home(&self, value: &Path) {
            unsafe { std::env::set_var("HOME", value) };
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.path {
                    Some(p) => std::env::set_var("PATH", p),
                    None => std::env::remove_var("PATH"),
                }
                match &self.home {
                    Some(h) => std::env::set_var("HOME", h),
                    None => std::env::remove_var("HOME"),
                }
            }
        }
    }

    /// Mount a mock serving the `version` release asset, expecting exactly
    /// `expect` GETs (any extra request trips verification on drop).
    async fn mount_release(server: &MockServer, version: &str, expect: u64) {
        let asset = format!(
            "/releases/download/v{version}/grow-{version}-{}.tar.gz",
            test_platform()
        );
        Mock::given(method("GET"))
            .and(path(asset))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(release_fixture(version)))
            .expect(expect)
            .mount(server)
            .await;
    }

    /// Script for a pre-existing (old) target that echoes a grow version.
    fn old_grow_script(version: &str) -> String {
        format!("#!/bin/sh\necho \"grow {version}\"\n")
    }

    // 2a: a plain file on PATH is replaced in place, executable, probed at
    // the new version; the returned landing path is the resolved target
    // (the success message is formatted from exactly that value).
    #[tokio::test]
    #[serial]
    async fn install_replaces_plain_file_on_path() {
        let _ = test_home();
        reset();

        let bin = tempfile::tempdir().unwrap();
        let target = bin.path().join("grow");
        std::fs::write(&target, old_grow_script("0.1.180")).unwrap();
        make_executable(&target);

        let env = EnvGuard::capture();
        let fake_home = tempfile::tempdir().unwrap();
        env.set_path(bin.path());
        env.set_home(fake_home.path());

        let server = MockServer::start().await;
        mount_release(&server, "0.1.181", 1).await;

        let landed = install_gh_release_from(&server.uri(), Some("0.1.181"))
            .await
            .unwrap();
        assert_eq!(landed, target, "landing path is the PATH-resolved target");
        assert_executable(&target);
        let content = std::fs::read_to_string(&target).unwrap();
        assert!(
            content.contains("0.1.181"),
            "target must be replaced with the new binary: {content}"
        );
        assert!(
            !content.contains("0.1.180"),
            "old target bytes must be gone"
        );
        // The post-install probe sees the new version — the dedup closure
        // that makes a second pass skip the download.
        assert_eq!(
            probe_target_version(&target).await.as_deref(),
            Some("0.1.181")
        );
    }

    // 2b: the managed layout keeps working — both entrypoints swapped,
    // grow-latest updated, disk-version probe reads the new version.
    #[tokio::test]
    #[serial]
    async fn install_replaces_managed_symlink_layout() {
        let _ = test_home();
        reset();

        let home = test_home().clone();
        let bin_dir = home.join("bin");
        let downloads = home.join("downloads");
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::create_dir_all(&downloads).unwrap();

        let old_name = format!("grow-0.1.180-{}", test_platform());
        let old = downloads.join(&old_name);
        std::fs::write(&old, old_grow_script("0.1.180")).unwrap();
        make_executable(&old);
        let rel_old = format!("../downloads/{old_name}");
        std::os::unix::fs::symlink(&rel_old, bin_dir.join("grow")).unwrap();
        std::os::unix::fs::symlink(&rel_old, bin_dir.join("agent")).unwrap();

        let env = EnvGuard::capture();
        let fake_home = tempfile::tempdir().unwrap();
        env.set_path(&bin_dir);
        env.set_home(fake_home.path());

        let server = MockServer::start().await;
        mount_release(&server, "0.1.181", 1).await;

        let landed = install_gh_release_from(&server.uri(), Some("0.1.181"))
            .await
            .unwrap();
        assert_eq!(landed, bin_dir.join("grow"));

        // Both entrypoints swapped to the new versioned binary (agent
        // reconciled in lockstep).
        let new_target = format!("../downloads/grow-0.1.181-{}", test_platform());
        assert_eq!(
            std::fs::read_link(bin_dir.join("grow")).unwrap(),
            PathBuf::from(&new_target)
        );
        assert_eq!(
            std::fs::read_link(bin_dir.join("agent")).unwrap(),
            PathBuf::from(&new_target)
        );
        // grow-latest follows.
        assert_eq!(
            std::fs::read_link(downloads.join("grow-latest")).unwrap(),
            PathBuf::from(format!("grow-0.1.181-{}", test_platform()))
        );
        // The disk-version probe reads the new version without exec.
        assert_eq!(
            update::version::installed_on_disk_version().as_deref(),
            Some("0.1.181")
        );
        // The versioned binary landed in downloads.
        assert!(
            downloads
                .join(format!("grow-0.1.181-{}", test_platform()))
                .exists()
        );
    }

    // 2c: an external symlink (outside grow_home) is swapped to the new
    // versioned binary; the old target file is untouched.
    #[tokio::test]
    #[serial]
    async fn install_swaps_external_symlink_target() {
        let _ = test_home();
        reset();

        let bin = tempfile::tempdir().unwrap();
        let real_dir = tempfile::tempdir().unwrap();
        let real = real_dir.path().join("grow-old");
        std::fs::write(&real, old_grow_script("0.1.180")).unwrap();
        make_executable(&real);
        let link = bin.path().join("grow");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let env = EnvGuard::capture();
        let fake_home = tempfile::tempdir().unwrap();
        env.set_path(bin.path());
        env.set_home(fake_home.path());

        let server = MockServer::start().await;
        mount_release(&server, "0.1.181", 1).await;

        let landed = install_gh_release_from(&server.uri(), Some("0.1.181"))
            .await
            .unwrap();
        assert_eq!(landed, link);
        // The link now points at the new versioned binary (absolute target:
        // the link lives outside grow_home, so no relative short form).
        let expected = test_home()
            .join("downloads")
            .join(format!("grow-0.1.181-{}", test_platform()));
        assert_eq!(std::fs::read_link(&link).unwrap(), expected);
        // The old target file is left untouched.
        assert!(real.exists());
        // Probe of the link resolves the new version by file name.
        assert_eq!(
            probe_target_version(&link).await.as_deref(),
            Some("0.1.181")
        );
    }

    // 2c (dangling): `land_binary_on_target` swaps a dangling link too.
    // (The full pipeline would resolve PATH past a dangling link — metadata
    // follows it and fails, so the lookup skips it and falls back to
    // current_exe — hence the land-layer contract is tested directly.)
    #[tokio::test]
    #[serial]
    async fn land_swaps_dangling_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("grow");
        let dangling = dir.path().join("gone");
        std::os::unix::fs::symlink(&dangling, &link).unwrap();

        let new_binary = dir.path().join("grow-0.1.181-linux-x86_64");
        std::fs::write(&new_binary, old_grow_script("0.1.181")).unwrap();
        make_executable(&new_binary);

        let landed = land_binary_on_target(
            &new_binary,
            &InstallTarget {
                path: link.clone(),
                kind: TargetKind::Symlink,
            },
        )
        .await
        .unwrap();
        assert_eq!(landed, link);
        assert!(link.exists(), "link now resolves");
        // Same-directory swap: the relative symlink target is the bare
        // file name.
        assert_eq!(
            std::fs::read_link(&link).unwrap(),
            PathBuf::from("grow-0.1.181-linux-x86_64")
        );
    }

    // 2d: a non-grow target (its --version output doesn't parse) aborts the
    // install with the target byte-for-byte untouched.
    #[tokio::test]
    #[serial]
    async fn install_refuses_non_grow_target_and_leaves_bytes() {
        let _ = test_home();
        reset();

        let bin = tempfile::tempdir().unwrap();
        let target = bin.path().join("grow");
        let original = "#!/bin/sh\necho hello\n";
        std::fs::write(&target, original).unwrap();
        make_executable(&target);

        let env = EnvGuard::capture();
        let fake_home = tempfile::tempdir().unwrap();
        env.set_path(bin.path());
        env.set_home(fake_home.path());

        let server = MockServer::start().await;
        mount_release(&server, "0.1.181", 1).await;

        let err = install_gh_release_from(&server.uri(), Some("0.1.181"))
            .await
            .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("refusing to replace"), "msg: {msg}");
        assert!(msg.contains("--version check failed"), "msg: {msg}");
        // Byte-for-byte untouched.
        assert_eq!(std::fs::read_to_string(&target).unwrap(), original);
    }

    // 2e: after a landing, the disk probe reports the new version — the
    // second-pass download decision (probe + needs_update) finds the disk
    // current and skips the download. The wiremock `expect(1)` makes any
    // actual re-download fail verification. The probe memo was invalidated
    // by the landing; without that, the pre-install probe result would be
    // served and the disk would look stale forever (re-download loop).
    #[tokio::test]
    #[serial]
    async fn second_pass_skips_download_when_disk_current() {
        let _ = test_home();
        reset();

        let bin = tempfile::tempdir().unwrap();
        let target = bin.path().join("grow");
        std::fs::write(&target, old_grow_script("0.1.180")).unwrap();
        make_executable(&target);

        let env = EnvGuard::capture();
        let fake_home = tempfile::tempdir().unwrap();
        env.set_path(bin.path());
        env.set_home(fake_home.path());

        let server = MockServer::start().await;
        mount_release(&server, "0.1.181", 1).await;

        let landed = install_gh_release_from(&server.uri(), Some("0.1.181"))
            .await
            .unwrap();
        assert_eq!(landed, target);

        assert_eq!(
            probe_target_version(&target).await.as_deref(),
            Some("0.1.181"),
            "post-landing probe must see the new version so a second pass \
             skips the download"
        );
    }

    // 2f: PATH miss falls back to the current executable (or the injected
    // override), classified as a plain file when outside grow_home.
    #[tokio::test]
    #[serial]
    async fn resolve_falls_back_to_current_exe_when_not_on_path() {
        let _ = test_home();
        reset();

        let empty = tempfile::tempdir().unwrap();
        let env = EnvGuard::capture();
        env.set_path(empty.path());

        let resolved = resolve_install_target(None).unwrap();
        assert_eq!(resolved.path, std::env::current_exe().unwrap());
        assert_eq!(resolved.kind, TargetKind::RegularFile);

        // exe_override wins over current_exe when PATH misses.
        let override_path = empty.path().join("grow");
        std::fs::write(&override_path, "x").unwrap();
        make_executable(&override_path);
        let resolved = resolve_install_target(Some(override_path.clone())).unwrap();
        assert_eq!(resolved.path, override_path);
        assert_eq!(resolved.kind, TargetKind::RegularFile);
    }

    // 2f: Unix rename-over-running — replacing a file a process is
    // currently executing succeeds and doesn't kill the running process
    // (the old inode stays alive; only the directory entry is re-pointed).
    // This is what makes replacing `current_exe()` itself legal.
    #[tokio::test]
    #[serial]
    async fn land_replaces_running_executable() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("grow");
        std::fs::write(&target, "#!/bin/sh\nsleep 30\n").unwrap();
        make_executable(&target);

        // The child is our own fixture process, killed and waited on
        // explicitly below — no session enrollment needed.
        #[allow(clippy::disallowed_methods)]
        let mut child = tokio::process::Command::new(&target).spawn().unwrap();
        // Give the child a moment to exec the script.
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(child.try_wait().unwrap().is_none(), "child must be running");

        let new_binary = dir.path().join("grow-new");
        std::fs::write(&new_binary, old_grow_script("0.1.181")).unwrap();
        make_executable(&new_binary);

        let landed = land_binary_on_target(
            &new_binary,
            &InstallTarget {
                path: target.clone(),
                kind: TargetKind::RegularFile,
            },
        )
        .await
        .unwrap();
        assert_eq!(landed, target);
        assert!(
            std::fs::read_to_string(&target)
                .unwrap()
                .contains("0.1.181"),
            "target must be replaced"
        );
        // The still-running process survived the rename-over.
        assert!(
            child.try_wait().unwrap().is_none(),
            "running process must survive the rename-over"
        );
        child.kill().await.unwrap();
        let _ = child.wait().await;
    }
}
