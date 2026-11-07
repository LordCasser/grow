//! Windows-only lifecycle regression: a real child exiting with code 259
//! (STILL_ACTIVE) must be observed as exited instead of being polled forever.

use std::collections::HashMap;
use std::time::Duration;

use super::*;

/// The fixture's last screen line, printed right before the 259 exit.
const LAST_LINE: &str = "PTY259-LAST-LINE";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lifecycle_windows_exit_259_observed_as_exited() {
    let session = PtySession::start(SessionConfig {
        pty: PtyConfig {
            command: vec![
                "powershell".into(),
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-Command".into(),
                format!("Write-Output '{LAST_LINE}'; [Environment]::Exit(259)"),
            ],
            cols: 80,
            rows: 24,
            cwd: None,
            env: HashMap::new(),
        },
        timeout: None,
        linger: false,
    })
    .await
    .unwrap();
    let control = session.control_handle();
    // Environment.Exit(259) really terminates the process with STILL_ACTIVE
    // as its exit code; the owner must settle it, not poll it forever.
    let code = tokio::time::timeout(Duration::from_secs(60), control.wait_for_exit())
        .await
        .expect("owner never settled a child that exited with code 259")
        .unwrap();
    assert_eq!(code, 259);
    assert!(!session.is_alive());
    assert_eq!(session.status_basic().2, Some(259));
    let screen = session.screen(&ScreenOpts::default()).await;
    assert!(
        screen.lines.join("\n").contains(LAST_LINE),
        "final screen lost the last line:\n{}",
        screen.lines.join("\n")
    );
    // Non-linger natural completion closes the service.
    tokio::time::timeout(Duration::from_secs(10), control.wait_for_shutdown())
        .await
        .unwrap()
        .unwrap();
}
