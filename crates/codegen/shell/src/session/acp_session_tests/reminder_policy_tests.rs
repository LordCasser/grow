use super::support::create_test_actor;
use super::{
    DEFAULT_TODO_GATE_MAX_FIRES, TodoGateConfig, date_rollover_reminder, laziness_injection_active,
    resolve_todo_gate_config, todo_gate_active,
};
use crate::session::persistence::PersistenceMsg;
use crate::util::config::RemoteSettings;
use agent::AgentDefinition;
use agent::prompt::context::PromptAudience;
/// Helper: a `RemoteSettings` whose only non-default fields are the
/// TodoGate knobs we want to vary. Mirrors `Default::default()` for
/// everything else so the test stays robust to unrelated additions.
fn remote_with_todo_gate(enabled: Option<bool>, cap: Option<u32>) -> RemoteSettings {
    RemoteSettings {
        todo_gate_enabled: enabled,
        todo_gate_max_fires_per_prompt: cap,
        ..RemoteSettings::default()
    }
}
#[test]
fn remote_none_preserves_built_in_defaults() {
    let config = resolve_todo_gate_config(None, false);
    assert_eq!(
        config,
        TodoGateConfig {
            enabled: false,
            max_fires_per_prompt: DEFAULT_TODO_GATE_MAX_FIRES,
        },
    );
}
#[test]
fn remote_disable_matches_default_path() {
    let remote = remote_with_todo_gate(Some(false), None);
    let config = resolve_todo_gate_config(Some(&remote), false);
    assert_eq!(
        config,
        TodoGateConfig {
            enabled: false,
            max_fires_per_prompt: DEFAULT_TODO_GATE_MAX_FIRES,
        },
    );
}
#[test]
fn remote_enable_true_overrides_default() {
    let remote = remote_with_todo_gate(Some(true), None);
    let config = resolve_todo_gate_config(Some(&remote), false);
    assert_eq!(
        config,
        TodoGateConfig {
            enabled: true,
            max_fires_per_prompt: DEFAULT_TODO_GATE_MAX_FIRES,
        },
    );
}
#[test]
fn remote_cap_override_applies_without_enabling_gate() {
    let remote = remote_with_todo_gate(None, Some(5));
    let config = resolve_todo_gate_config(Some(&remote), false);
    assert_eq!(
        config,
        TodoGateConfig {
            enabled: false,
            max_fires_per_prompt: 5,
        },
    );
}
#[test]
fn cli_todo_gate_overrides_remote_enable_false() {
    let remote = remote_with_todo_gate(Some(false), Some(7));
    let config = resolve_todo_gate_config(Some(&remote), true);
    assert_eq!(
        config,
        TodoGateConfig {
            enabled: true,
            // Cap stays whatever remote said; CLI only flips `enabled`.
            max_fires_per_prompt: 7,
        },
    );
}
#[test]
fn remote_settings_without_todo_gate_fields_use_defaults() {
    let settings: RemoteSettings = serde_json::from_str("{}").unwrap();
    assert_eq!(settings.todo_gate_enabled, None);
    assert_eq!(settings.todo_gate_max_fires_per_prompt, None);
    let config = resolve_todo_gate_config(Some(&settings), false);
    assert_eq!(
        config,
        TodoGateConfig {
            enabled: false,
            max_fires_per_prompt: DEFAULT_TODO_GATE_MAX_FIRES,
        },
    );
}
#[test]
fn remote_settings_accepts_explicit_null_todo_gate_fields() {
    let json = r#"{
            "todo_gate_enabled": null,
            "todo_gate_max_fires_per_prompt": null
        }"#;
    let settings: RemoteSettings = serde_json::from_str(json).unwrap();
    assert_eq!(settings.todo_gate_enabled, None);
    assert_eq!(settings.todo_gate_max_fires_per_prompt, None);
}
#[test]
fn remote_settings_preserves_false_and_zero_todo_gate_fields() {
    let json = r#"{
            "todo_gate_enabled": false,
            "todo_gate_max_fires_per_prompt": 0
        }"#;
    let settings: RemoteSettings = serde_json::from_str(json).unwrap();
    assert_eq!(settings.todo_gate_enabled, Some(false));
    assert_eq!(settings.todo_gate_max_fires_per_prompt, Some(0));
}
fn todo_gate_config(enabled: bool) -> TodoGateConfig {
    TodoGateConfig {
        enabled,
        ..TodoGateConfig::default()
    }
}
use crate::session::goal_tracker::GoalStatus;
#[test]
fn laziness_injection_active_predicate_matrix() {
    let def = AgentDefinition::default_grow_build();
    let config_on = todo_gate_config(true);
    for (goal_runtime_available, goal_status, expect) in [
        (false, None, false),
        (false, Some(GoalStatus::Active), false),
        (true, None, false),
        (true, Some(GoalStatus::Active), true),
        (true, Some(GoalStatus::Complete), false),
        (true, Some(GoalStatus::Paused), false),
    ] {
        assert_eq!(
            laziness_injection_active(goal_runtime_available, goal_status),
            expect,
            "goal_runtime_available={goal_runtime_available} status={goal_status:?}",
        );
        assert!(
            !todo_gate_active(
                config_on,
                PromptAudience::Primary,
                &def,
                goal_runtime_available,
                goal_status,
            ),
            "todo gate must be suppressed during the active goal loop",
        );
    }
}
#[test]
fn todo_gate_active_predicate_matrix() {
    let def = AgentDefinition::default_grow_build();
    let config_off = todo_gate_config(false);
    let config_on = todo_gate_config(true);
    for (config, audience, goal_runtime_available, goal_status, expect) in [
        (config_off, PromptAudience::Primary, true, None, false),
        (config_off, PromptAudience::Subagent, true, None, false),
        (
            config_off,
            PromptAudience::Primary,
            true,
            Some(GoalStatus::Active),
            false,
        ),
        (
            config_on,
            PromptAudience::Primary,
            true,
            Some(GoalStatus::Active),
            false,
        ),
        (
            config_on,
            PromptAudience::Subagent,
            true,
            Some(GoalStatus::Active),
            false,
        ),
        (config_on, PromptAudience::Primary, false, None, false),
        (
            config_on,
            PromptAudience::Primary,
            false,
            Some(GoalStatus::Active),
            false,
        ),
        (config_on, PromptAudience::Primary, true, None, false),
    ] {
        assert_eq!(
            todo_gate_active(config, audience, &def, goal_runtime_available, goal_status),
            expect,
            "gate.enabled={} audience={audience:?} goal_runtime_available={goal_runtime_available} status={goal_status:?}",
            config.enabled
        );
    }
    for status in [
        GoalStatus::Complete,
        GoalStatus::Paused,
        GoalStatus::Blocked,
        GoalStatus::BudgetLimited,
    ] {
        assert!(
            !todo_gate_active(config_on, PromptAudience::Primary, &def, true, Some(status)),
            "non-active status {status:?} must not enable gate"
        );
    }
    let def = AgentDefinition::default_grow_build();
    for audience in [PromptAudience::Primary, PromptAudience::Subagent] {
        assert!(
            !todo_gate_active(config_on, audience, &def, true, None),
            "built-in template without active goal must not enable gate"
        );
    }
}
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
