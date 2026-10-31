//! Real PTY coverage from the updater's result channel through UI rendering.
//! The child runs the production Pager with an embedded agent; no installer runs.
use clap::Parser;
use pager_pty_harness::{ContentController, PtyHarness};
use std::path::Path;
use std::time::Duration;

const VERSION: &str = "99.8.7";
const NOTICE: &str = "Update available: v99.8.7";
const READY: Duration = Duration::from_secs(30);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "PTY child entry point, invoked only by the parent scenarios"]
async fn update_notice_child() {
    let Ok(mode) = std::env::var("UPDATE_NOTICE_TEST_MODE") else {
        return;
    };
    diagnostics::tls::install_ring_provider_once();
    pager_minimal::install();
    let trigger = std::env::var_os("UPDATE_NOTICE_TEST_TRIGGER").expect("trigger path");
    let (tx, rx) = tokio::sync::oneshot::channel();
    let minimal = mode.starts_with("minimal");
    tokio::spawn(async move {
        while !Path::new(&trigger).exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let result = (!mode.ends_with("none")).then(|| update::auto_update::UpdateAvailable {
            latest_version: VERSION.to_string(),
        });
        let _ = tx.send(result);
    });
    let args = pager::app::PagerArgs::parse_from([
        "grow",
        "--no-leader",
        "--trust",
        if minimal { "--minimal" } else { "--fullscreen" },
    ]);
    pager::app::run(args, Some(rx)).await.expect("Pager run");
}

async fn run_scenario(mode: &str) {
    let content = ContentController::start().await.expect("mock inference");
    assert!(
        std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(content.home())
            .status()
            .expect("initialize isolated project")
            .success()
    );
    content.seed_llm_config().expect("isolated config");
    let trigger = content.home().join("deliver-update");
    let trigger_text = trigger.to_str().expect("trigger UTF-8");
    let mut ui = PtyHarness::spawn_with_content_env_in_dir(
        &std::env::current_exe().expect("test executable"),
        40,
        120,
        &content,
        &["--exact", "update_notice_child", "--ignored", "--nocapture"],
        &[
            ("UPDATE_NOTICE_TEST_MODE", mode),
            ("UPDATE_NOTICE_TEST_TRIGGER", trigger_text),
        ],
        Some(content.home()),
    )
    .expect("spawn real Pager in PTY");
    ui.set_respond_to_queries(true);
    if mode.starts_with("minimal") {
        ui.wait_for_text("minimal · /help", READY)
            .expect("minimal ready");
    } else {
        ui.wait_for_text("Quit", READY).expect("welcome ready");
        if mode.starts_with("session") {
            ui.inject_keys(b"\r").expect("open session");
            ui.wait_for_text("grow · Mock", READY)
                .expect("session prompt ready");
            ui.wait_for_text_absent("Starting session", READY)
                .expect("session startup settled");
        }
    }
    assert!(!ui.full_text().contains(NOTICE), "no notice before result");
    std::fs::write(&trigger, b"deliver").expect("deliver result");
    if mode.ends_with("none") {
        ui.update(Duration::from_secs(2));
        assert!(
            !ui.full_text().contains(NOTICE),
            "None must not announce an update"
        );
        assert!(!ui.screen_contents().contains("v99.8.7 available"));
    } else if mode == "welcome" {
        ui.wait_for_text("v99.8.7 available", READY)
            .expect("welcome update hint");
        assert!(ui.screen_contents().contains("ctrl+u"));
        ui.wait_for_text(NOTICE, READY).expect("welcome toast");
    } else {
        ui.wait_for_text(NOTICE, READY)
            .expect("visible update notice");
        assert!(ui.screen_contents().contains("restart to apply"));
    }
    if let Some(dir) = std::env::var_os("UPDATE_NOTICE_ARTIFACT_DIR") {
        let dir = Path::new(&dir);
        std::fs::create_dir_all(dir).expect("artifact directory");
        std::fs::write(dir.join(format!("{mode}.html")), ui.screen_html()).expect("HTML snapshot");
        std::fs::write(dir.join(format!("{mode}.txt")), ui.full_text()).expect("text snapshot");
    }
    ui.inject_keys(b"\x11").expect("arm quit");
    ui.update(Duration::from_millis(200));
    ui.inject_keys(b"\x11").expect("confirm quit");
    assert_eq!(
        ui.wait_for_exit_and_drain(Duration::from_secs(15), Duration::from_secs(1))
            .expect("clean Pager exit"),
        0,
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "spawns the real Pager UI in a PTY"]
async fn update_notices_render_in_real_terminal() {
    for mode in [
        "welcome",
        "session",
        "minimal",
        "session-none",
        "minimal-none",
    ] {
        run_scenario(mode).await;
    }
}
