use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use super::ManagedConfigError;

/// Optional syntax checker. `path` is appended after `args`, matching the
/// `bash -n FILE`, `zsh -n FILE`, and `fish -n FILE` interfaces.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntaxValidator {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub timeout: Duration,
}

pub(super) fn validate_temp(
    validator: &SyntaxValidator,
    path: &Path,
) -> Result<(), ManagedConfigError> {
    validate_with_ops(validator, path, &RealProcessOps)
}

trait ProcessOps {
    fn attach_group(&self, child: &Child) -> Result<tty_utils::ProcessGroup, std::io::Error>;
    fn try_wait(&self, child: &mut Child) -> std::io::Result<Option<ExitStatus>>;
    fn teardown(
        &self,
        child: &mut Child,
        group: Option<&tty_utils::ProcessGroup>,
    ) -> Result<(), String>;
}

struct RealProcessOps;
impl ProcessOps for RealProcessOps {
    fn attach_group(&self, child: &Child) -> Result<tty_utils::ProcessGroup, std::io::Error> {
        let mut group = tty_utils::ProcessGroup::new()?;
        group.attach_std(child)?;
        Ok(group)
    }

    fn try_wait(&self, child: &mut Child) -> std::io::Result<Option<ExitStatus>> {
        child.try_wait()
    }

    fn teardown(
        &self,
        child: &mut Child,
        group: Option<&tty_utils::ProcessGroup>,
    ) -> Result<(), String> {
        teardown_child(child, group)
    }
}

fn validate_with_ops(
    validator: &SyntaxValidator,
    path: &Path,
    ops: &dyn ProcessOps,
) -> Result<(), ManagedConfigError> {
    let mut command = Command::new(&validator.program);
    command
        .args(&validator.args)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .envs(tty_utils::pager_env());
    tty_utils::detach_std_command(&mut command);
    #[allow(clippy::disallowed_methods)] // config validator, waited on with a timeout
    let mut child = command
        .spawn()
        .map_err(|source| ManagedConfigError::Validation {
            path: path.to_path_buf(),
            reason: format!("could not start {}: {source}", validator.program.display()),
        })?;
    let group = ops.attach_group(&child).ok();

    let started = Instant::now();
    loop {
        match ops.try_wait(&mut child) {
            Ok(Some(status)) => {
                // A reaped leader does not imply its process group is empty.
                let teardown = ops.teardown(&mut child, group.as_ref()).err();
                if status.success() && teardown.is_none() {
                    return Ok(());
                }
                return Err(validation_error(
                    path,
                    format!("{} exited with {status}", validator.program.display()),
                    teardown,
                ));
            }
            Ok(None) if started.elapsed() < validator.timeout => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let teardown = ops.teardown(&mut child, group.as_ref()).err();
                return Err(validation_error(
                    path,
                    format!("timed out after {:?}", validator.timeout),
                    teardown,
                ));
            }
            Err(source) => {
                let teardown = ops.teardown(&mut child, group.as_ref()).err();
                return Err(validation_error(path, source.to_string(), teardown));
            }
        }
    }
}

fn validation_error(path: &Path, primary: String, teardown: Option<String>) -> ManagedConfigError {
    let reason = match teardown {
        Some(teardown) => format!("{primary}; process teardown also failed: {teardown}"),
        None => primary,
    };
    ManagedConfigError::Validation {
        path: path.to_path_buf(),
        reason,
    }
}

fn process_group_is_gone(error: &std::io::Error) -> bool {
    #[cfg(unix)]
    {
        error.raw_os_error() == Some(libc::ESRCH)
    }
    #[cfg(not(unix))]
    {
        let _ = error;
        false
    }
}

fn teardown_child(
    child: &mut Child,
    group: Option<&tty_utils::ProcessGroup>,
) -> Result<(), String> {
    let mut errors = Vec::new();
    if let Some(group) = group {
        if let Err(error) = group.terminate()
            && !process_group_is_gone(&error)
        {
            errors.push(format!("terminate group: {error}"));
        }
        std::thread::sleep(Duration::from_millis(50));
        if let Err(error) = group.kill()
            && !process_group_is_gone(&error)
        {
            errors.push(format!("kill group: {error}"));
        }
    }
    if let Err(error) = child.kill()
        && error.kind() != std::io::ErrorKind::InvalidInput
    {
        errors.push(format!("kill child: {error}"));
    }
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                errors.push("child did not reap within 1s".to_owned());
                break;
            }
            Err(error) => {
                errors.push(format!("reap child: {error}"));
                break;
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    struct InjectedOps {
        attach_fails: bool,
        wait_fails: bool,
        teardown_fails: bool,
        teardown_called: AtomicBool,
    }

    impl ProcessOps for InjectedOps {
        fn attach_group(&self, child: &Child) -> Result<tty_utils::ProcessGroup, std::io::Error> {
            if self.attach_fails {
                Err(std::io::Error::other("injected attach failure"))
            } else {
                RealProcessOps.attach_group(child)
            }
        }

        fn try_wait(&self, child: &mut Child) -> std::io::Result<Option<ExitStatus>> {
            if self.wait_fails {
                Err(std::io::Error::other("injected try_wait failure"))
            } else {
                child.try_wait()
            }
        }

        fn teardown(
            &self,
            child: &mut Child,
            group: Option<&tty_utils::ProcessGroup>,
        ) -> Result<(), String> {
            self.teardown_called.store(true, Ordering::SeqCst);
            teardown_child(child, group)?;
            if self.teardown_fails {
                Err("injected teardown failure".to_owned())
            } else {
                Ok(())
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn completed_validator_cleans_surviving_descendants() {
        for exit_code in [0, 7] {
            let temp = tempfile::tempdir().unwrap();
            let marker = temp.path().join("leaked");
            let ready = temp.path().join("ready");
            let validator = SyntaxValidator {
                program: "/bin/sh".into(),
                args: vec![
                    "-c".into(),
                    r#"(trap '' TERM; : > "$2"; sleep 1; printf leaked > "$1") &
while [ ! -f "$2" ]; do sleep 0.01; done
exit "$3""#
                        .into(),
                    "validator".into(),
                    marker.as_os_str().to_owned(),
                    ready.as_os_str().to_owned(),
                    exit_code.to_string().into(),
                ],
                timeout: Duration::from_secs(3),
            };
            let result = validate_temp(&validator, &temp.path().join("config"));
            if exit_code == 0 {
                assert!(result.is_ok(), "{result:?}");
            } else {
                assert!(result.unwrap_err().to_string().contains("exited with"));
            }
            // The fixture completes by itself even against the broken implementation.
            std::thread::sleep(Duration::from_millis(1200));
            assert!(!marker.exists(), "exit {exit_code} left a live descendant");
        }
    }

    #[cfg(unix)]
    #[test]
    fn completed_validator_without_descendants_preserves_exit_result() {
        for exit_code in [0, 7] {
            let validator = SyntaxValidator {
                program: "/bin/sh".into(),
                args: vec!["-c".into(), format!("exit {exit_code}").into()],
                timeout: Duration::from_secs(1),
            };
            let result = validate_temp(&validator, Path::new("unused"));
            if exit_code == 0 {
                assert!(result.is_ok(), "{result:?}");
            } else {
                let error = result.unwrap_err().to_string();
                assert!(error.contains("exited with"), "{error}");
                assert!(!error.contains("teardown also failed"), "{error}");
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn completed_validator_reports_teardown_failure() {
        for exit_code in [0, 7] {
            let validator = SyntaxValidator {
                program: "/bin/sh".into(),
                args: vec!["-c".into(), format!("exit {exit_code}").into()],
                timeout: Duration::from_secs(1),
            };
            let ops = InjectedOps {
                attach_fails: false,
                wait_fails: false,
                teardown_fails: true,
                teardown_called: AtomicBool::new(false),
            };
            let error = validate_with_ops(&validator, Path::new("unused"), &ops)
                .unwrap_err()
                .to_string();
            assert!(ops.teardown_called.load(Ordering::SeqCst));
            assert!(error.contains("exited with"), "{error}");
            assert!(error.contains("injected teardown failure"), "{error}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn attach_failure_falls_back_to_bounded_direct_child_teardown() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config");
        std::fs::write(&path, "body").unwrap();
        let validator = SyntaxValidator {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "sleep 5".into()],
            timeout: Duration::from_millis(20),
        };
        let ops = InjectedOps {
            attach_fails: true,
            wait_fails: false,
            teardown_fails: false,
            teardown_called: AtomicBool::new(false),
        };
        let started = Instant::now();
        assert!(validate_with_ops(&validator, &path, &ops).is_err());
        assert!(ops.teardown_called.load(Ordering::SeqCst));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn try_wait_error_still_tears_down_child() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config");
        std::fs::write(&path, "body").unwrap();
        let validator = SyntaxValidator {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "sleep 5".into()],
            timeout: Duration::from_secs(1),
        };
        let ops = InjectedOps {
            attach_fails: false,
            wait_fails: true,
            teardown_fails: false,
            teardown_called: AtomicBool::new(false),
        };
        assert!(validate_with_ops(&validator, &path, &ops).is_err());
        assert!(ops.teardown_called.load(Ordering::SeqCst));
    }
}
