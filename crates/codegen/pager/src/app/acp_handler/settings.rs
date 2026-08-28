use super::*;
use serde::Deserialize;

/// Handle `grow/models/update` — model list changed (etag-triggered refresh).
pub(super) fn handle_models_update(notif: &acp::ExtNotification, app: &mut AppView) -> bool {
    if let Ok(model_state) = serde_json::from_str::<acp::SessionModelState>(notif.params.get()) {
        apply_models_state_update(model_state, app)
    } else {
        tracing::warn!("Failed to parse grow/models/update");
        false
    }
}

pub(crate) fn apply_models_state_update(
    model_state: acp::SessionModelState,
    app: &mut AppView,
) -> bool {
    use crate::acp::model_state::ModelState;
    let new_models = ModelState::from(Some(model_state));
    tracing::info!(
        count = new_models.available.len(),
        "models updated from authoritative process catalog"
    );

    let shell_fallback_current = new_models.current.clone();
    // This is the process-owned template for future sessions, not a live
    // session selection. Always adopt the Shell's published default even when
    // the previous default remains in the catalog; concrete sessions below
    // independently preserve their own current model.
    app.models = new_models.clone();

    for agent in app.agents.values_mut() {
        update_model_catalog_recursively(
            agent,
            &new_models.available,
            shell_fallback_current.as_ref(),
        );
        retry_authoritative_controls_recursively(agent);
        super::sync_all_subagent_control_projections(agent);
    }
    true
}

/// A ModelChanged event can legitimately precede the catalog generation that
/// defines its model. Keep the authoritative value on the concrete session
/// and retry it after every catalog publication; dropping it here would leave
/// the selector permanently behind the Shell until another control event.
fn retry_authoritative_controls_recursively(agent: &mut crate::app::agent_view::AgentView) -> bool {
    let session_changed = if let Some(session_id) = agent.session.session_id.clone() {
        super::apply_deferred_authoritative_controls(agent, session_id.0.as_ref())
    } else {
        false
    };
    let child_changed = agent
        .subagent_views
        .values_mut()
        .fold(false, |changed, child| {
            retry_authoritative_controls_recursively(child) || changed
        });
    session_changed || child_changed
}

/// Keep every concrete session view on the current catalog. A subagent has its
/// own model/effort selection and may be the active view, so updating only the
/// root's catalog makes a later `ModelChanged` impossible to resolve there.
fn update_model_catalog_recursively(
    agent: &mut crate::app::agent_view::AgentView,
    available: &indexmap::IndexMap<acp::ModelId, acp::ModelInfo>,
    fallback_current: Option<&acp::ModelId>,
) {
    if let Some(current) = agent.session.models.current.as_ref()
        && !available.contains_key(current)
    {
        tracing::warn!(
            current_model = %current.0,
            fallback = ?fallback_current.map(|m| m.0.as_ref()),
            available_count = available.len(),
            "models update removed this session's current model; falling back"
        );
    }
    agent
        .session
        .models
        .update_catalog(available.clone(), fallback_current.cloned());
    for (child_session_id, child) in &mut agent.subagent_views {
        // Workflow Runs own an immutable runtime route. Shell deliberately
        // leaves those child catalogs pinned, so mirroring a process-wide
        // catalog publication into the Pager would invent a fallback model
        // that the child never adopted. Descendants share the same frozen
        // route and are skipped with their owner.
        if agent
            .session
            .subagent_sessions
            .get(child_session_id)
            .is_some_and(|info| info.workflow_run_id.is_some())
        {
            continue;
        }
        update_model_catalog_recursively(child, available, fallback_current);
    }
}

/// Handle `grow/settings/update` — remote settings refreshed on `/new`.
pub(super) fn handle_settings_update(notif: &acp::ExtNotification, app: &mut AppView) -> bool {
    let Ok(update) = serde_json::from_str::<PagerSettingsUpdate>(notif.params.get()) else {
        tracing::warn!("Failed to parse grow/settings/update");
        return false;
    };

    // Reseed this process's remote-campaign cache. In leader mode no in-process
    // agent seeds the TUI process, and the bounded startup prefetch can miss —
    // without this reseed a remote campaign stays invisible to
    // `resolve_dismissable_campaigns`, so a `/model` pick never records its
    // dismissal and the leader re-nudges every new session. Idempotent in
    // embedded mode, where the in-process agent seeds the same cache.
    if let Some(campaigns) = update.campaigns.clone() {
        let rs = shell::util::config::RemoteSettings {
            campaigns,
            ..Default::default()
        };
        shell::util::config::set_remote_campaigns_from_settings(Some(&rs));
    }

    if let Some(v) = update.auto_permission_mode_enabled {
        // Keep the pager's auto-permission-mode gate live with the remote settings
        // remote tier (the leader caches it agent-side; the pager process needs
        // its own copy). Refresh the startup snapshot so permission selectors
        // and the settings modal both reflect a remote-only enablement/kill-switch
        // without a restart.
        shell::util::config::cache_remote_auto_permission_mode_enabled(Some(v));
        app.auto_mode_gate = shell::util::config::auto_permission_mode_enabled_from_disk();
        // Mid-session kill switch: when the gate just went off, drop displayed
        // Auto to Ask + clear every agent's per-session flag (shared with the
        // startup reconcile), AND tell live sessions to leave Auto. Clearing only
        // the display would let the agent keep classifier-approving while the UI
        // shows "Ask" — the emergency-off must actually disable enforcement.
        if !app.auto_mode_gate {
            // Sessions to notify: agents that HAD Auto on (capture before the
            // downgrade clears the flag) and have a live session id.
            let leaving_auto: Vec<acp::SessionId> = app
                .agents
                .values()
                .filter(|a| a.session.is_auto())
                .filter_map(|a| a.session.session_id.clone())
                .collect();
            super::super::root::dispatch::downgrade_displayed_auto_if_gated(app);
            notify_sessions_leave_auto(app, &leaving_auto);
        }
        // Reveal/hide `/auto` on every slash surface in lockstep with the gate
        // (covers both a mid-session kill-switch and re-enablement).
        app.sync_permission_mode_slash_gate();
    }

    // `permission_mode` is presence-aware (omit / null / string). While the
    // soft default still owns the mode, a push re-arms the typed default + UI for
    // the next `/new`; once the user claims a mode through a session selector,
    // the latch is cleared and pushes leave it alone.
    if let Some(remote_opt) = update.permission_mode.as_ref()
        && app.permission_mode_from_soft_default
    {
        // One config read at the I/O boundary; the applier is deterministic.
        let root = shell::config::load_effective_config().ok();
        apply_soft_default_permission_mode(
            app,
            root.as_ref().and_then(|r| r.get("ui")),
            remote_opt.as_deref(),
        );
    }

    if let Some(v) = update.show_resolved_model {
        app.show_resolved_model = v;
    }
    // TODO: extract resolve_session_picker_grouped helper (duplicates event_loop.rs:143-160)
    // Respect env var > config > remote precedence (mirrors event_loop.rs startup).
    if let Some(remote_val) = update.session_picker_grouped {
        let resolved = std::env::var("GROW_SESSION_PICKER_GROUPED")
            .ok()
            .and_then(|v| match v.as_str() {
                "1" | "true" => Some(true),
                "0" | "false" => Some(false),
                _ => None,
            })
            .or_else(|| {
                shell::config::load_effective_config()
                    .ok()
                    .and_then(|cfg| cfg.get("cli")?.get("session_picker_grouped")?.as_bool())
            })
            .unwrap_or(remote_val);
        app.session_picker_grouped = resolved;
    }
    // Load config layers once for tips + group_tool_verbs resolution. Loaded
    // unconditionally because updates are rare (post-auth refresh, `/new`).
    let (requirements, user_config, managed_config) = (
        shell::config::load_merged_requirements(),
        shell::config::load_from_disk().ok(),
        shell::config::load_managed_config().ok(),
    );

    // Local layers may beat remote — re-resolve the full chain into the render
    // cache (mirrors the event_loop.rs startup resolve). Runs on None too: the
    // shell always publishes this field from its live remote tier, so None
    // means remote settings cleared it (or an older shell that cannot deliver the
    // remote tier at all) — either way resolving without a remote value is
    // correct, and it reverts a previously cached remote enable back to the
    // local/default (off) resolution instead of leaving Some(true) stuck
    // until restart.
    let remote = shell::util::config::RemoteSettings {
        group_tool_verbs: update.group_tool_verbs,
        ..Default::default()
    };
    let resolved = shell::util::config::resolve_group_tool_verbs(
        requirements.as_ref(),
        user_config.as_ref(),
        managed_config.as_ref(),
        Some(&remote),
    )
    .value;
    // On a real flip, re-fold every live transcript (mirrors dispatch's
    // set_group_tool_verbs_inner); unchanged values keep `/new` cheap.
    // Stale expansion ids describe the old grouping shape — drop them so the
    // re-fold can't reopen a verb slot expanded or mark a coincident dense
    // group expanded (see `clear_group_expansion`).
    if resolved != crate::appearance::cache::load_group_tool_verbs() {
        crate::appearance::cache::set_group_tool_verbs(resolved);
        for agent in app.agents.values_mut() {
            agent.scrollback.clear_group_expansion();
            agent.scrollback.invalidate_heights();
            for child in agent.subagent_views.values_mut() {
                child.scrollback.clear_group_expansion();
                child.scrollback.invalidate_heights();
            }
        }
    }

    // Re-resolve tips from config layers + the updated remote tips.
    if let Some(remote_tips) = update.tips {
        use shell::util::config::resolve_tips;

        app.tips = resolve_tips(
            requirements.as_ref(),
            user_config.as_ref(),
            managed_config.as_ref(),
            Some(&remote_tips),
        );
        if !app.tips.is_empty() {
            let grow_home = tools::util::grow_home::grow_home();
            app.tip = shell::util::tips::pick_and_advance(&app.tips, &grow_home);
        } else {
            app.tip = None;
        }
    }

    // Re-resolve dropdown tags only when the update carries the field. Some(None) =
    // remote cleared (drop remote layer); Some(Some(map)) = set; outer None = field
    // absent (older shell) → keep the tags resolved at startup. Env + local
    // [slash_command_tags] always apply via resolve_slash_command_tags.
    if let Some(remote_tags) = update.slash_command_tags.as_ref() {
        use shell::util::config::resolve_slash_command_tags;
        let effective_config = shell::config::load_effective_config().ok();
        let empty_toml = toml::Value::Table(Default::default());
        let tags_config = effective_config.as_ref().unwrap_or(&empty_toml);
        *app.command_tags.borrow_mut() =
            resolve_slash_command_tags(tags_config, remote_tags.as_ref());
    }

    tracing::info!("settings updated via grow/settings/update");
    true
}

/// Re-arm the soft-defaulted launch mode from a pushed `permission_mode`
/// (TOML `[ui]` > remote > Ask), for the next `/new` only — live sessions are
/// untouched and nothing is persisted. `effective_ui` is injected so the
/// resolve is deterministic under test. Enforcement gating reuses the app's
/// startup snapshots (`always_approve_policy_block`, `auto_mode_gate`); the agent's
/// permission manager re-clamps authoritatively at decision time.
pub(super) fn apply_soft_default_permission_mode(
    app: &mut AppView,
    effective_ui: Option<&toml::Value>,
    remote: Option<&str>,
) {
    let requested = shell::util::config::resolve_permission_mode(effective_ui, remote);
    let mode = match requested {
        shell::util::config::PermissionMode::AlwaysApprove
            if app.always_approve_policy_block.is_some() =>
        {
            shell::util::config::PermissionMode::Ask
        }
        shell::util::config::PermissionMode::Auto if !app.auto_mode_gate => {
            shell::util::config::PermissionMode::Ask
        }
        mode => mode,
    };
    app.default_permission_mode = mode;
    app.current_ui.permission_mode =
        Some(shell::util::config::permission_mode_canonical_str(mode).to_string());
}

/// Tell live sessions to leave Auto on the mid-session kill-switch: fire the
/// canonical session-scoped permission notification, fire-and-forget over the
/// shared ACP channel.
pub(super) fn notify_sessions_leave_auto(app: &AppView, session_ids: &[acp::SessionId]) {
    for session_id in session_ids {
        let params = serde_json::json!({
            "sessionId": session_id,
            "permissionMode": "ask",
        });
        let notification = acp::ExtNotification::new(
            "grow/permission_mode_changed",
            serde_json::value::to_raw_value(&params)
                .expect("serialize permission_mode_changed params")
                .into(),
        );
        let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
        let args = acp_transport::AcpArgs {
            request: notification,
            response_tx,
        };
        let _ = app.acp_tx.send(args.into());
    }
}

/// Handle `grow/sessions/changed` — the leader broadcasts roster
/// upserts/removals to all clients (FleetView dashboard).
pub(super) fn handle_sessions_changed(notif: &acp::ExtNotification, app: &mut AppView) -> bool {
    let Ok(changed) = serde_json::from_str::<crate::app::roster::RosterChanged>(notif.params.get())
    else {
        tracing::warn!("Failed to parse grow/sessions/changed");
        return false;
    };
    let mut affected = false;
    for entry in changed.upserted {
        app.upsert_roster_entry(entry);
        affected = true;
    }
    for sid in changed.removed {
        app.remove_roster_entry(&sid);
        affected = true;
    }
    affected
}

pub(super) fn handle_announcements_update(notif: &acp::ExtNotification, app: &mut AppView) -> bool {
    let Ok(parsed) =
        serde_json::from_str::<announcements::AnnouncementsUpdated>(notif.params.get())
    else {
        return false;
    };

    app.active_announcements = announcements::filter_expired(parsed.announcements);
    app.announcement = app.active_announcements.first().cloned();
    if announcements::prune_hidden_announcement_ids(
        &mut app.hidden_announcement_ids,
        &app.active_announcements,
    ) {
        app.pending_effects
            .push(Effect::PersistAnnouncementsHidden {
                hidden_ids: app.hidden_announcement_ids.clone(),
            });
    }
    app.sync_session_announcement_slash_gate();
    true
}

/// Deserialization type for the `grow/settings/update` notification payload.
///
/// This is intentionally a separate struct from `SettingsUpdateNotification` in
/// `shell/src/agent/mvp_agent.rs`. The shell side derives `Serialize`
/// and owns the canonical field set from `RemoteSettings`; this pager side
/// derives `Deserialize` and selectively consumes only the fields relevant to
/// the TUI. Keeping them separate avoids coupling the pager to shell internals
/// and lets each side evolve independently (e.g. adding a shell-only field
/// doesn't require a pager change). All fields are `Option` with
/// `#[serde(default)]` so that partial updates and forward-compatible additions
/// are handled gracefully.
///
/// **Keep in sync** with field names/types in `SettingsUpdateNotification` at
/// `shell/src/agent/mvp_agent.rs` when adding fields that both sides
/// need.
#[derive(serde::Deserialize)]
pub(super) struct PagerSettingsUpdate {
    #[serde(default)]
    show_resolved_model: Option<bool>,
    #[serde(default)]
    session_picker_grouped: Option<bool>,
    #[serde(default)]
    tips: Option<Vec<String>>,
    /// Free-form per-command slash-dropdown tags (canonical name → tag).
    /// Presence-aware and tolerant: omit = no update (older shell), `null` =
    /// remote cleared, map = set, malformed = warn + treat as absent so a
    /// bad value never fails the whole `PagerSettingsUpdate` parse.
    #[serde(default, deserialize_with = "deserialize_settings_update_tags")]
    slash_command_tags: Option<Option<std::collections::BTreeMap<String, String>>>,
    // `announcements` is deliberately not consumed. Grow announcements come
    // from local configuration, not the upstream vendor promotion feed.
    /// Remote campaigns snapshot. `Some` whenever the shell has settings
    /// (empty = campaigns withdrawn); `None`/omitted (settings-less push,
    /// older shell) must leave this process's campaign cache untouched.
    #[serde(default)]
    campaigns: Option<Vec<shell::util::config::CampaignOverride>>,
    #[serde(default)]
    auto_permission_mode_enabled: Option<bool>,
    /// Soft-default permission mode. Presence-aware: omit = no update,
    /// `null` = recompute with remote=None, string = that soft-default.
    /// Omission happens with older shells that predate the field (they can
    /// never clear a mode they don't know about) — that version skew is why
    /// this is tri-state instead of a plain `Option`.
    #[serde(default, deserialize_with = "deserialize_presence_aware_string")]
    permission_mode: Option<Option<String>>,
    #[serde(default)]
    group_tool_verbs: Option<bool>,
}

/// Presence-aware string: omit → `None` (`#[serde(default)]`), null →
/// `Some(None)`, string → `Some(Some(_))`.
fn deserialize_presence_aware_string<'de, D>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<String>::deserialize(deserializer)?))
}

/// Presence-aware + tolerant tags map for live settings updates.
/// Only invoked when the field is present (`#[serde(default)]` covers omit).
/// - JSON null → `Some(None)` (explicit remote clear)
/// - valid object → `Some(Some(map))`
/// - malformed → warn + `Ok(None)` (leave tags alone; do not fail the struct)
fn deserialize_settings_update_tags<'de, D>(
    deserializer: D,
) -> Result<Option<Option<std::collections::BTreeMap<String, String>>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Null => Ok(Some(None)),
        v => match serde_json::from_value::<std::collections::BTreeMap<String, String>>(v) {
            Ok(m) => Ok(Some(Some(m))),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "malformed slash_command_tags in settings update; leaving tags unchanged"
                );
                Ok(None)
            }
        },
    }
}

#[cfg(test)]
mod presence_aware_dto_tests {
    use super::*;

    #[derive(Deserialize)]
    struct Probe {
        #[serde(default, deserialize_with = "deserialize_presence_aware_string")]
        permission_mode: Option<Option<String>>,
    }

    #[test]
    fn permission_mode_dto_distinguishes_omit_from_null() {
        let omit: Probe = serde_json::from_value(serde_json::json!({
            "show_resolved_model": true,
        }))
        .unwrap();
        assert_eq!(omit.permission_mode, None, "omit must be None (no update)");

        let null_v: Probe = serde_json::from_value(serde_json::json!({
            "permission_mode": null,
        }))
        .unwrap();
        assert_eq!(
            null_v.permission_mode,
            Some(None),
            "explicit null must be Some(None)"
        );

        let some_v: Probe = serde_json::from_value(serde_json::json!({
            "permission_mode": "always-approve",
        }))
        .unwrap();
        assert_eq!(
            some_v.permission_mode,
            Some(Some("always-approve".into())),
            "string must be Some(Some(_))"
        );
    }

    #[test]
    fn slash_command_tags_dto_absent_null_map_and_malformed() {
        // 1. field absent → outer None (leave tags alone)
        let absent: PagerSettingsUpdate = serde_json::from_value(serde_json::json!({
            "tips": ["hello"],
        }))
        .expect("absent slash_command_tags must not fail parse");
        assert_eq!(absent.slash_command_tags, None, "omit must be None");
        assert_eq!(absent.tips.as_deref(), Some(&["hello".to_string()][..]));

        // 2. explicit null → Some(None) (remote cleared)
        let null_v: PagerSettingsUpdate = serde_json::from_value(serde_json::json!({
            "slash_command_tags": null,
        }))
        .expect("null slash_command_tags must parse");
        assert_eq!(
            null_v.slash_command_tags,
            Some(None),
            "explicit null must be Some(None)"
        );

        // 3. valid map → Some(Some(map))
        let map_v: PagerSettingsUpdate = serde_json::from_value(serde_json::json!({
            "slash_command_tags": {"workflows": "new"},
        }))
        .expect("valid slash_command_tags map must parse");
        let tags = map_v
            .slash_command_tags
            .as_ref()
            .and_then(|inner| inner.as_ref())
            .expect("expected Some(Some(map))");
        assert_eq!(tags.get("workflows").map(String::as_str), Some("new"));
        assert_eq!(tags.len(), 1);

        // 4. malformed must NOT fail the whole struct; sibling fields still apply
        let bad: PagerSettingsUpdate = serde_json::from_value(serde_json::json!({
            "slash_command_tags": ["oops"],
            "tips": ["still-applied"],
            "permission_mode": "always-approve",
        }))
        .expect("malformed slash_command_tags must not fail PagerSettingsUpdate parse");
        assert_eq!(
            bad.slash_command_tags, None,
            "malformed tags treated as absent"
        );
        assert_eq!(
            bad.tips.as_deref(),
            Some(&["still-applied".to_string()][..]),
            "sibling tips must still parse"
        );
        assert_eq!(
            bad.permission_mode,
            Some(Some("always-approve".into())),
            "sibling permission_mode must still parse"
        );
    }
}
