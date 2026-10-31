// Per-test-case module for the `leader_pty_e2e` integration test crate.
#[allow(unused_imports)]
use super::common::*;
use std::time::Instant;

/// 25. **Leader reattach — cancellation round-trips through the durable log.**
/// After one admitted turn, a second turn driven on leader client A is
/// Ctrl+C-cancelled mid-stream. The leader must persist a `turn_completed`
/// terminal with `stop_reason == cancelled`; reconnect must replay the first
/// admitted response but suppress the second turn's unadmitted candidate. A
/// fresh client must land clean and remain attached after A exits.
///
/// C attaches *before* A is dropped: `PtyHarness` Drop SIGKILLs the child, and
/// under full-suite contention a cold `--resume` handshake racing that teardown
/// flakes with an empty screen for the whole `LEADER_TIMEOUT`. Replaying while A is still
/// up, then proving C survives A's exit, covers the durable-log + multi-client
/// survival invariants without that race.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "PTY e2e; run with cargo test -p pager --test leader_pty_e2e -- --ignored --test-threads=1"]
async fn leader_reattach_cancellation_roundtrips_durable_log() {
    let cluster = LeaderCluster::start(DEFAULT_ROWS, DEFAULT_COLS)
        .await
        .expect("start leader cluster");
    git2::Repository::init(cluster.content().home()).expect("initialize isolated project");
    // Establish canonical history before the cancellation case. The cancelled
    // response below remains an unadmitted candidate and must not appear on
    // reconnect.
    cluster
        .content()
        .set_response(format!("{} admitted response.", turn_sentinel(1)));
    let mut a = cluster.spawn_leader(&[]).expect("spawn leader client A");
    a.wait_for_text(WELCOME_SCREEN_SENTINEL, LEADER_TIMEOUT)
        .expect("A welcome");
    submit_turn(&mut a, PROMPT, &turn_sentinel(1), STREAM_TIMEOUT);
    let first_terminal = cluster
        .wait_for_turn_completed(STREAM_TIMEOUT)
        .expect("first turn_completed persisted");
    assert_eq!(
        first_terminal["stop_reason"], "end_turn",
        "first turn must complete before cancellation, got {first_terminal}"
    );

    // Pace the second response enough to cancel while it streams.
    let long_response = format!(
        "{} {}",
        turn_sentinel(2),
        "more streamed filler to hold the turn open. ".repeat(40)
    );
    cluster.content().set_response(long_response);
    cluster
        .content()
        .set_chunk_delay(Some(Duration::from_millis(40)));

    // Wait until the turn is clearly streaming (sentinel visible); this also
    // closes the leader's rewind window so cancel is not confused with rewind.
    submit_turn(
        &mut a,
        "cancel this response",
        &turn_sentinel(2),
        STREAM_TIMEOUT,
    );

    // Ctrl+C on an empty prompt cancels while streaming.
    a.inject_keys(keys::CTRL_C).expect("A press ctrl+c");
    a.update(Duration::from_millis(200));
    // Generous budget: the heavy multi-client leader cluster drains the paced
    // cancel slower than the single-client path, so match the test's other
    // waits (LEADER/STREAM_TIMEOUT) rather than the single-client 15s.
    a.wait_for_text("Turn cancelled", STREAM_TIMEOUT)
        .expect("A turn cancelled marker");

    // Producer fail-before: the cancel must have persisted a durable terminal
    // carrying the cancelled stop reason.
    let deadline = Instant::now() + STREAM_TIMEOUT;
    let rec = loop {
        let updates = cluster.session_updates();
        if let Some(record) = updates.iter().find(|record| {
            record["sessionUpdate"] == "turn_completed" && record["stop_reason"] == "cancelled"
        }) {
            break record.clone();
        }
        assert!(
            Instant::now() < deadline,
            "cancelled turn_completed did not persist; updates: {updates:?}"
        );
        std::thread::sleep(Duration::from_millis(150));
    };
    assert_eq!(
        rec["stop_reason"], "cancelled",
        "cancelled turn must record stop_reason=cancelled, got {rec}"
    );

    // Keep-alive viewer attaches AFTER the cancel so the leader survives A's
    // exit; it should replay only the earlier admitted turn.
    let mut keep = cluster.attach(&[]).expect("spawn keep-alive viewer");
    keep.wait_for_text(&turn_sentinel(1), LEADER_TIMEOUT)
        .expect("keep-alive replayed the admitted transcript");
    assert!(
        !keep.contains_text(&turn_sentinel(2)),
        "cancelled candidate must not enter durable replay\nscreen:\n{}",
        keep.screen_contents()
    );

    // Reattach must replay from the durable log, not re-drive a turn: the mock
    // must see no new inference request while C catches up.
    let inference_before_reattach = inference_request_count(cluster.content());

    // Fresh reattach while A is still up (see module comment), then prove the
    // original driver's exit does not take C (or the transcript) down.
    let mut c = cluster.attach(&[]).expect("spawn fresh reattach client C");
    c.wait_for_text(&turn_sentinel(1), LEADER_TIMEOUT)
        .expect("C replayed the admitted transcript");
    assert!(
        !c.contains_text(&turn_sentinel(2)),
        "C replayed an unadmitted cancelled candidate\nscreen:\n{}",
        c.screen_contents()
    );

    drop(a);
    // Leader processes A's disconnect; brief settle then re-check C still has
    // the durable replay (not a fixed long sleep that would mask a hang).
    c.update(Duration::from_millis(500));
    assert!(
        c.is_running().expect("poll pager liveness"),
        "C exited after A quit\nscreen:\n{}",
        c.screen_contents()
    );
    assert!(
        c.contains_text(&turn_sentinel(1)),
        "C lost the admitted transcript after A quit\nscreen:\n{}",
        c.screen_contents()
    );
    assert_eq!(
        c.screen_contents().matches(&turn_sentinel(1)).count(),
        1,
        "the admitted response must replay exactly once\nscreen:\n{}",
        c.screen_contents()
    );
    assert!(
        !c.contains_text(&turn_sentinel(2)),
        "cancelled candidate appeared after A quit\nscreen:\n{}",
        c.screen_contents()
    );

    // Same fidelity caveat as the completion case: a fresh reattach lands Idle
    // either way, so absent spinners are regression guards, not fail-befores.
    // Bare substrings (not the full `…`-suffixed labels) are simple stable
    // matches for the spinner labels. Bound the replay wait rather than a fixed
    // settle.
    wait_for_labels_absent(&mut c, &["Waiting", "Cancelling"], Duration::from_secs(5));

    assert!(
        !c.contains_text("panicked"),
        "C rendered a panic\nscreen:\n{}",
        c.screen_contents()
    );
    assert!(
        !c.contains_text("Waiting"),
        "C is stranded on the active-turn spinner\nscreen:\n{}",
        c.screen_contents()
    );
    assert!(
        !c.contains_text("Cancelling"),
        "C is stranded on the cancelling spinner\nscreen:\n{}",
        c.screen_contents()
    );
    assert_eq!(
        inference_request_count(cluster.content()),
        inference_before_reattach,
        "reattach must replay from the durable log, not re-drive a turn (no new inference request)"
    );

    c.quit().expect("quit C");
    keep.quit().expect("quit keep-alive viewer");
}
