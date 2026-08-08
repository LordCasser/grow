/// Back-date a viewer's monotonic turn anchor from the shell timestamp.
pub(crate) fn viewer_turn_anchor(turn_start_ms: Option<i64>) -> std::time::Instant {
    let now = std::time::Instant::now();
    let Some(start_ms) = turn_start_ms else {
        return now;
    };
    let elapsed_ms = chrono::Utc::now()
        .timestamp_millis()
        .saturating_sub(start_ms);
    if elapsed_ms <= 0 {
        return now;
    }
    now.checked_sub(std::time::Duration::from_millis(elapsed_ms as u64))
        .unwrap_or(now)
}
