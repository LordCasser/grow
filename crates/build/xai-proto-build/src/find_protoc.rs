use anyhow::{Context, bail};
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn check_protoc_good(protoc: &Path) -> anyhow::Result<()> {
    let output = Command::new(protoc)
        .arg("--version")
        .output()
        .context("Failed to execute protoc")?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "protoc --version failed, likely dotslash is missing; \
             try `cargo install dotslash`; stdout: {stdout:?}, stderr: {stderr:?}"
        );
    }
    Ok(())
}

fn is_github_actions() -> bool {
    env::var_os("GITHUB_ACTIONS").is_some()
}

/// Find the `protoc` binary to use for proto compilation.
///
/// Search order:
/// 1. `$PROTOC` environment variable (set by Bazel `build_script_env` or user override)
/// 2. `bin/protoc` walking up parent directories (dotslash wrapper for local dev)
/// 3. `protoc` on `$PATH` (system install or other tooling)
///
/// `$PROTOC` is a hard override: if it is set but points at a path that does
/// not exist, lookup fails with an error instead of silently falling back to
/// the other methods. An unreachable `$PROTOC` (e.g. a container path that was
/// never mounted) means the environment is misconfigured; skipping it would
/// only surface a confusing follow-up error (dotslash/PATH) later.
///
/// When `bin/protoc` exists but fails to execute (e.g. the dotslash wrapper
/// running in Bazel remote execution where `dotslash` is not installed), the
/// error is not fatal — we fall through to the PATH-based lookup instead.
///
/// Returns `Ok(None)` if not found and not in a strict environment (GitHub Actions).
pub fn find_protoc() -> anyhow::Result<Option<PathBuf>> {
    find_protoc_inner(
        std::env::var("PROTOC").ok(),
        &std::env::current_dir()?,
        is_github_actions(),
        check_protoc_good(Path::new("protoc")).is_ok(),
    )
}

/// Injectable core of [`find_protoc`] so tests can exercise every lookup
/// branch without touching the process environment or `$PATH`.
fn find_protoc_inner(
    protoc_env: Option<String>,
    cwd: &Path,
    github_actions: bool,
    path_has_protoc: bool,
) -> anyhow::Result<Option<PathBuf>> {
    // 1. Check the PROTOC env var first. This is the standard override used by prost-build
    //    and is set by Bazel cargo_build_script build_script_env to point at a hermetic
    //    protoc binary instead of the dotslash wrapper.
    if let Some(protoc_env) = protoc_env {
        let protoc = PathBuf::from(&protoc_env);
        if protoc.try_exists()? {
            check_protoc_good(&protoc)?;
            return Ok(Some(protoc));
        }
        bail!("$PROTOC is set to `{protoc_env}` but that path does not exist");
    }

    // 2. Walk up from `cwd` looking for bin/protoc (dotslash wrapper).
    let mut dir = cwd.to_path_buf();
    let mut dir_rel = PathBuf::new();
    loop {
        // Return relative path to make build more deterministic.
        let protoc = dir_rel.join("bin/protoc");
        let protoc_abs = cwd.join(&protoc);
        if protoc_abs.try_exists()? {
            match check_protoc_good(&protoc_abs) {
                Ok(()) => return Ok(Some(protoc)),
                Err(e) => {
                    // bin/protoc exists but can't execute — likely the dotslash wrapper
                    // in an environment without dotslash (e.g. Bazel remote execution).
                    // Fall through to PATH-based lookup below.
                    eprintln!(
                        "bin/protoc found at `{}` but failed to execute: {e:#}; \
                         trying protoc from PATH as fallback",
                        protoc.display()
                    );
                    break;
                }
            }
        }
        if !dir.pop() {
            break;
        }
        dir_rel.push("..");
    }

    // 3. Try protoc from PATH (system install or other tooling).
    if path_has_protoc {
        return Ok(Some(PathBuf::from("protoc")));
    }

    // 4. Not found anywhere.
    if github_actions {
        return Err(anyhow::anyhow!(
            "`protoc` not found (checked $PROTOC env, bin/protoc, and PATH)"
        ));
    }
    eprintln!("`protoc` not found; likely it is missing in docker image");
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    fn temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir should succeed")
    }

    /// Create `<root>/bin/protoc` as a plain text file without the exec bit:
    /// it exists but cannot be executed.
    fn write_broken_bin_protoc(root: &Path) {
        fs::create_dir_all(root.join("bin")).expect("create bin dir");
        fs::write(root.join("bin/protoc"), "not an executable\n").expect("write bin/protoc");
    }

    #[cfg(unix)]
    fn make_executable(path: &Path) {
        let mut perms = fs::metadata(path).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).expect("set exec bit");
    }

    /// `$PROTOC` pointing at a path that does not exist is a hard error naming
    /// both `$PROTOC` and the missing path — not a silent fall-through.
    #[test]
    fn protoc_env_missing_path_is_hard_error() {
        let tmp = temp_dir();
        let missing = tmp.path().join("no/such/protoc");
        let err = find_protoc_inner(
            Some(missing.to_string_lossy().into_owned()),
            tmp.path(),
            false,
            false,
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("$PROTOC"),
            "error should mention $PROTOC: {msg}"
        );
        assert!(
            msg.contains("no/such/protoc"),
            "error should mention the missing path: {msg}"
        );
    }

    /// `$PROTOC` pointing at an existing, executable protoc is used as-is.
    #[cfg(unix)]
    #[test]
    fn protoc_env_executable_script_is_used() {
        let tmp = temp_dir();
        let protoc = tmp.path().join("fake-protoc");
        fs::write(&protoc, "#!/bin/sh\nexit 0\n").expect("write fake protoc");
        make_executable(&protoc);

        let found = find_protoc_inner(
            Some(protoc.to_string_lossy().into_owned()),
            tmp.path(),
            false,
            false,
        )
        .expect("should succeed")
        .expect("should find a protoc");
        assert_eq!(found, protoc);
    }

    /// Walk-up lookup from `cwd`: an executable `bin/protoc` under the cwd
    /// tree is found and returned as a relative path (for build determinism).
    #[cfg(unix)]
    #[test]
    fn walk_up_finds_executable_bin_protoc() {
        let tmp = temp_dir();
        let bin_protoc = tmp.path().join("bin/protoc");
        fs::create_dir_all(tmp.path().join("bin")).expect("create bin dir");
        fs::write(&bin_protoc, "#!/bin/sh\nexit 0\n").expect("write bin/protoc");
        make_executable(&bin_protoc);

        let found = find_protoc_inner(None, tmp.path(), false, false)
            .expect("should succeed")
            .expect("should find a protoc");
        assert_eq!(found, PathBuf::from("bin/protoc"));
    }

    /// A `bin/protoc` that exists but cannot execute is not fatal: lookup
    /// falls through, and in GitHub Actions the result is a clear error
    /// (instead of a misleading "dotslash is missing" panic downstream).
    #[test]
    fn broken_bin_protoc_is_error_in_github_actions() {
        let tmp = temp_dir();
        write_broken_bin_protoc(tmp.path());

        let err = find_protoc_inner(None, tmp.path(), true, false).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("`protoc` not found"),
            "unexpected error message: {msg}"
        );
    }

    /// Outside GitHub Actions the same broken `bin/protoc` falls back with a
    /// printed notice and `Ok(None)`. The notice emission is verified by
    /// re-running this test in a child test process with `--nocapture`, so the
    /// `eprintln!` reaches the child's real stderr which the parent asserts on
    /// (the test harness swallows in-process stderr and std has no stable API
    /// to capture it; pattern as in xai-tty-utils).
    #[test]
    fn broken_bin_protoc_prints_fallback_notice() {
        const CHILD_ENV: &str = "__GROW_XAI_PROTO_BUILD_FALLBACK_NOTICE_CHILD";
        if std::env::var(CHILD_ENV).is_err() {
            let output = std::process::Command::new(std::env::current_exe().expect("test exe"))
                .arg("--exact")
                .arg("find_protoc::tests::broken_bin_protoc_prints_fallback_notice")
                .arg("--nocapture")
                .env(CHILD_ENV, "1")
                .output()
                .expect("failed to spawn test subprocess");
            assert!(
                output.status.success(),
                "child test failed\nstderr:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                stderr.contains("trying protoc from PATH as fallback"),
                "fallback notice missing from child stderr:\n{stderr}"
            );
            return;
        }

        let tmp = temp_dir();
        write_broken_bin_protoc(tmp.path());
        let result = find_protoc_inner(None, tmp.path(), false, false).expect("should succeed");
        assert!(result.is_none(), "expected None, got {result:?}");
    }

    /// Nothing available and not in a strict environment: `Ok(None)`.
    #[test]
    fn no_protoc_anywhere_is_ok_none_outside_github_actions() {
        let tmp = temp_dir();
        let result = find_protoc_inner(None, tmp.path(), false, false).expect("should succeed");
        assert!(result.is_none());
    }

    /// Nothing else available in GitHub Actions but `protoc` on PATH: the PATH
    /// entry is used, reported as the bare `protoc` command (original semantics).
    #[test]
    fn path_protoc_in_github_actions_is_used() {
        let tmp = temp_dir();
        let found = find_protoc_inner(None, tmp.path(), true, true)
            .expect("should succeed")
            .expect("should find a protoc");
        assert_eq!(found, PathBuf::from("protoc"));
    }
}
