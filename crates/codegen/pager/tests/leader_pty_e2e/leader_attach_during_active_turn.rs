// Per-test-case module for the `leader_pty_e2e` integration test crate.
#[allow(unused_imports)]
use super::common::*;

/// Resident session/load must not run cold interrupted-scope recovery while
/// the original client's turn is still streaming.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "PTY e2e; run with cargo test -p pager --test leader_pty_e2e -- --ignored --test-threads=1"]
async fn leader_attach_during_active_turn() {
    let cluster = LeaderCluster::start(DEFAULT_ROWS, DEFAULT_COLS)
        .await
        .expect("start cluster");
    git2::Repository::init(cluster.content().home()).expect("initialize isolated project");
    cluster
        .content()
        .seed_llm_config()
        .expect("seed mock LLM config");
    cluster
        .content()
        .set_chunk_delay(Some(Duration::from_millis(100)));
    cluster
        .content()
        .set_response(format!("{} {}", turn_sentinel(1), "streaming ".repeat(80)));

    let mut a = cluster.spawn_leader(&[]).expect("spawn pager A");
    a.wait_for_text(WELCOME_SCREEN_SENTINEL, LEADER_TIMEOUT)
        .expect("A welcome");
    a.inject_keys(format!("{PROMPT}\r").as_bytes())
        .expect("A submit turn 1");
    a.wait_for_text(&turn_sentinel(1), STREAM_TIMEOUT)
        .expect("A sees the first streamed text");
    assert!(
        !cluster
            .session_updates()
            .iter()
            .any(|update| update["sessionUpdate"] == "turn_completed"),
        "turn 1 must still be active before viewer attaches"
    );

    let mut b = cluster.attach(&[]).expect("attach pager B during turn 1");
    b.wait_for_text("streaming", Duration::from_secs(30))
        .expect("B receives the ongoing stream");
    assert!(
        !cluster
            .session_updates()
            .iter()
            .any(|update| update["sessionUpdate"] == "turn_completed"),
        "B must finish loading before turn 1 ends"
    );
    let terminal = cluster
        .wait_for_turn_completed(STREAM_TIMEOUT)
        .expect("turn 1 real terminal after B attaches");
    assert_eq!(terminal["stop_reason"], "end_turn", "{terminal}");

    cluster.content().set_chunk_delay(None);
    cluster
        .content()
        .set_response(format!("{} second turn payload.", turn_sentinel(2)));
    submit_turn(&mut a, "again", &turn_sentinel(2), STREAM_TIMEOUT);
    b.wait_for_text(&turn_sentinel(2), STREAM_TIMEOUT)
        .expect("B receives A's next turn");
    assert!(
        !a.contains_text("Turn failed"),
        "A screen:\n{}",
        a.screen_contents()
    );

    a.quit().expect("quit A");
    b.quit().expect("quit B");
}
