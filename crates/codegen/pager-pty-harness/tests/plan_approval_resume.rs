//! Integration test: a deferred Plan is reviewed after quit/`--continue`,
//! approval reopens, and approving starts exactly one implementation turn.
//!
//! CI stages the pager binary via `PAGER_BINARY`. Also runs under plain cargo
//! (which builds the pager on demand):
//!
//! ```bash
//! cargo test -p pager-pty-harness --test plan_approval_resume -- --nocapture
//! ```

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plan_approval_restored_after_resume() {
    pager_pty_harness::scenarios::plan_approval_resume::assert_plan_approval_restored_after_resume(
    )
    .await
    .expect("live Plan approval must be restored after process restart");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deferred_plan_reviewed_after_resume() {
    pager_pty_harness::scenarios::plan_approval_resume::assert_deferred_plan_reviewed_after_resume(
    )
    .await
    .expect("deferred Plan must remain reviewable after resume");
}
