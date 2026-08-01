//! Integration test: the shell re-parks Plan approval on
//! resume, so approval chrome reappears after quit/`--continue` and approving
//! leaves plan mode + starts the implement turn.
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
    .expect("shell must re-park Plan approval on resume so approval chrome returns");
}
