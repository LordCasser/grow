use crate::common::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn minimal_fps_hud_toggle_preserves_prompt() {
    let content = ContentController::start()
        .await
        .expect("start isolated content");
    // A project directory avoids the startup project picker consuming test keys.
    git2::Repository::init(content.home()).expect("initialize isolated project");
    content
        .seed_llm_config()
        .expect("seed isolated mock provider");
    let binary = pager_binary().expect("resolve current CLI");
    let mut harness = PtyHarness::spawn_with_content_env(
        &binary,
        28,
        90,
        &content,
        MINIMAL_ARGS,
        &[("GROW_FPS", "1")],
    )
    .expect("spawn minimal");
    harness.set_respond_to_queries(true);
    wait_minimal_ready(&mut harness);
    harness
        .wait_for_text("fps debug", Duration::from_secs(10))
        .expect("startup HUD visible");
    inject_keys_paced(&mut harness, b"/debug fps");
    assert!(
        harness.screen_contents().contains("❯ /debug fps"),
        "command draft: {}",
        harness.screen_contents()
    );
    harness.inject_keys(b"\r").unwrap();
    harness.update(Duration::from_millis(500));
    assert!(
        !harness.contains_text("fps debug"),
        "startup toggle off: {}",
        harness.screen_contents()
    );
    inject_keys_paced(&mut harness, b"/debug fps");
    harness.inject_keys(b"\r").unwrap();
    harness
        .wait_for_text("fps debug", Duration::from_secs(10))
        .expect("HUD visible");
    // A later normal input frame refreshes stats; no idle FPS timer is needed.
    harness.update(Duration::from_millis(300));
    inject_keys_paced(&mut harness, b"fps-draft-sentinel");
    harness
        .wait_for_text("fps-draft-sentinel", Duration::from_secs(5))
        .unwrap();
    let screen = harness.screen_contents();
    assert!(screen.contains("p50:"), "{screen}");
    assert!(!screen.contains("fps:-"), "HUD must have samples: {screen}");
    let hud_row = screen
        .lines()
        .position(|line| line.contains("fps debug"))
        .unwrap();
    let prompt_row = screen
        .lines()
        .position(|line| line.contains("fps-draft-sentinel"))
        .unwrap();
    assert!(
        prompt_row >= hud_row + 2,
        "HUD must not overlap prompt: {screen}"
    );
    assert_eq!(
        harness.cursor_position().0 as usize,
        prompt_row,
        "cursor must remain on draft: {screen}"
    );
    harness.inject_keys(b"\x15").unwrap();
    inject_keys_paced(&mut harness, b"/debug fps");
    harness.inject_keys(b"\r").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while harness.contains_text("fps debug") && Instant::now() < deadline {
        harness.update(Duration::from_millis(100));
    }
    assert!(
        !harness.contains_text("fps debug"),
        "disabled HUD left stale cells: {}",
        harness.screen_contents()
    );
    inject_keys_paced(&mut harness, b"after-fps-sentinel");
    harness
        .wait_for_text("after-fps-sentinel", Duration::from_secs(5))
        .unwrap();
    let screen = harness.screen_contents();
    let prompt_row = screen
        .lines()
        .position(|line| line.contains("after-fps-sentinel"))
        .unwrap();
    assert_eq!(harness.cursor_position().0 as usize, prompt_row, "{screen}");
    assert!(!screen.contains("panicked"), "{screen}");
    quit_minimal(&mut harness);
}
