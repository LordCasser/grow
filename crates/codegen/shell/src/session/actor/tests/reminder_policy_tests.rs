use super::date_rollover_reminder;
use super::support::create_test_actor;
use crate::session::persistence::PersistenceMsg;
use chrono::NaiveDate;
fn ymd(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).expect("valid test date")
}
#[test]
fn date_rollover_reminder_silent_when_same_day() {
    let today = ymd(2026, 4, 24);
    assert!(date_rollover_reminder(today, today).is_none());
}
#[test]
fn date_rollover_reminder_fires_when_day_advances() {
    let last = ymd(2026, 4, 24);
    let today = ymd(2026, 4, 25);
    let msg = date_rollover_reminder(today, last).expect("rollover should fire");
    assert!(
        msg.contains("2026-04-25"),
        "must announce the new date: {msg}"
    );
    assert!(
        !msg.contains("2026-04-24"),
        "must not echo the stale date: {msg}"
    );
}
#[test]
fn date_rollover_reminder_fires_across_month_and_year_boundaries() {
    assert!(date_rollover_reminder(ymd(2026, 5, 1), ymd(2026, 4, 30)).is_some());
    assert!(date_rollover_reminder(ymd(2027, 1, 1), ymd(2026, 12, 31)).is_some());
}
#[test]
fn date_rollover_reminder_silent_when_clock_moves_backward() {
    let last = ymd(2026, 4, 25);
    let today = ymd(2026, 4, 24);
    assert!(date_rollover_reminder(today, last).is_none());
}
#[tokio::test(flavor = "current_thread")]
async fn same_session_rolls_over_once_when_local_date_advances() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _) = tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(50_000, 256_000, 85, gateway_tx, persistence_tx).await;
            let today = chrono::Local::now().date_naive();
            assert_eq!(actor.last_announced_local_date.get(), today);
            actor.maybe_inject_date_rollover_reminder().await;
            assert_eq!(
                actor.chat_state_handle.get_conversation_len().await,
                1,
                "same-day turn must not inject a rollover reminder"
            );
            let yesterday = today.pred_opt().expect("today is never the min date");
            actor.last_announced_local_date.set(yesterday);
            actor.maybe_inject_date_rollover_reminder().await;
            let conv = actor.chat_state_handle.get_conversation().await;
            assert_eq!(conv.len(), 2, "rollover must inject exactly one reminder");
            let text = conv[1].text_content();
            assert!(
                text.contains("<system-reminder>"),
                "rollover reminder must be wrapped in system-reminder tags: {text}"
            );
            assert!(
                text.contains("The local date has changed since this session started"),
                "rollover reminder must announce the date change: {text}"
            );
            assert!(
                text.contains(&today.to_string()),
                "rollover reminder must carry today's date {today}: {text}"
            );
            assert_eq!(actor.last_announced_local_date.get(), today);
            actor.maybe_inject_date_rollover_reminder().await;
            assert_eq!(
                actor.chat_state_handle.get_conversation_len().await,
                2,
                "rollover must not re-fire on a later same-day turn"
            );
        })
        .await;
}
