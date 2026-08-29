//! Campaign dismiss state, local campaign overlays, and effective-config overlay.
//!
//! Design, invariants, and the "adding a second governed field" recipe are
//! documented alongside this module.

use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use config::campaigns::{CampaignEntry, filter_active_campaigns, ids_touching_paths};
use config::config_override::{PatchPath, patch_touches_any};
use config::{
    CampaignsState, ConfigLayers, campaigns_state_path, load_dismissed_ids_from_home,
    user_grow_home,
};

/// FIFO cap on persisted dismissed ids; evicting the oldest can re-nudge for a
/// still-live campaign after a user dismisses more than this over the CLI's life.
const MAX_DISMISSED_IDS: usize = 32;

static DISMISS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
static DISMISS_TMP_NONCE: AtomicU64 = AtomicU64::new(0);

/// Fail-open dismissed campaign ids from `campaigns_state.json`.
pub fn load_dismissed_ids() -> HashSet<String> {
    load_dismissed_ids_from_home()
}

pub fn dismiss_campaign_ids(ids: impl IntoIterator<Item = String>) {
    let Some(home) = user_grow_home() else {
        return;
    };
    if let Err(e) = dismiss_campaign_ids_at(&home, ids) {
        tracing::warn!(error = %e, "campaigns: failed to persist dismiss state");
    }
}

/// Append `ids` to the dismissed set and write `campaigns_state.json` atomically
/// (temp + rename). Corrupt prior state is renamed aside, not discarded.
fn dismiss_campaign_ids_at(
    home: &Path,
    ids: impl IntoIterator<Item = String>,
) -> std::io::Result<()> {
    use fs2::FileExt as _;
    let _guard = DISMISS_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let path = campaigns_state_path(home);
    // Cross-process advisory lock over the read-modify-write: in leader mode
    // several grow processes share `$GROW_HOME`; the in-process mutex alone would
    // let them lose-update the set. Best-effort; a lock failure still proceeds.
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path.with_extension("json.lock"));
    if let Ok(ref f) = lock {
        let _ = f.lock_exclusive();
    }
    let mut ordered = match std::fs::read_to_string(&path) {
        Ok(contents) => match serde_json::from_str::<CampaignsState>(&contents) {
            Ok(s) => s.dismissed_ids,
            Err(e) => {
                let _ = std::fs::rename(&path, path.with_extension("json.corrupt"));
                tracing::warn!(error = %e, "campaigns: corrupt dismiss state; renamed aside");
                Vec::new()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e),
    };
    let mut seen: HashSet<String> = ordered.iter().cloned().collect();
    for id in ids {
        if id.is_empty() || !seen.insert(id.clone()) {
            continue;
        }
        ordered.push(id);
    }
    if ordered.len() > MAX_DISMISSED_IDS {
        let drop_n = ordered.len() - MAX_DISMISSED_IDS;
        ordered.drain(..drop_n);
    }
    let json = serde_json::to_string(&CampaignsState {
        dismissed_ids: ordered,
    })
    .map_err(std::io::Error::other)?;
    let nonce = DISMISS_TMP_NONCE.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_extension(format!("json.{}.{}.tmp", std::process::id(), nonce));
    std::fs::write(&tmp, &json)?;
    std::fs::rename(&tmp, &path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// The single campaign-resolution path: local layer → kill switch → dismiss.
pub fn resolve_active_campaigns_from_layers(
    layers: &ConfigLayers,
    base: &toml::Value,
    dismissed: &HashSet<String>,
) -> Vec<CampaignEntry> {
    layers.resolve_campaigns(base, dismissed)
}

/// Campaigns eligible for dismissal when the user persists a choice (loads
/// local campaigns + dismiss state).
///
/// Unlike the apply path this deliberately **ignores the kill switch**:
/// dismissing a suppressed campaign is harmless, while skipping the dismissal
/// lets a later re-enabled campaign override a choice the user already made
/// ("user pick wins, forever"). A layer-load failure likewise falls back to
/// disk-layer campaigns can be missed until the transient failure clears (they
/// re-dismiss on the next pick).
fn resolve_dismissable_campaigns() -> Vec<CampaignEntry> {
    let dismissed = load_dismissed_ids();
    match ConfigLayers::load() {
        Ok(layers) => filter_active_campaigns(layers.campaigns.user, &dismissed),
        Err(e) => {
            tracing::warn!(error = %e, "campaigns: layer load failed; dismiss bookkeeping skipped");
            Vec::new()
        }
    }
}

/// Effective config with local campaign overlay
/// (base → resolve [override/kill/dismiss] → apply), one `ConfigLayers::load`.
pub fn load_effective_config() -> std::io::Result<toml::Value> {
    let layers = ConfigLayers::load()?;
    let dismissed = load_dismissed_ids();
    let mut effective = layers.effective_config_base();
    let active = resolve_active_campaigns_from_layers(&layers, &effective, &dismissed);
    layers.apply_campaign_overrides(&mut effective, &active);
    Ok(effective)
}

/// Effective config resolved from disk-only local sources. Kept as a named
/// entrypoint for one-shot CLI callers that do not load runtime settings.
pub fn load_effective_config_disk_only() -> std::io::Result<toml::Value> {
    Ok(ConfigLayers::load()?.effective_config_disk_only())
}

/// The effective `models.default` while an **active** campaign drives it, plus
/// the pre-campaign base value it overrode.
pub struct CampaignModelsDefault {
    /// The campaign-nudged default model.
    pub value: String,
    /// The pre-campaign base `models.default` (`None` when the user had none).
    pub pre_campaign: Option<String>,
}

/// Resolve [`CampaignModelsDefault`] fresh from the local config layers and the
/// on-disk dismiss state.
///
/// `None` unless an active (non-dismissed, kill-switch-respecting) campaign
/// changes the effective `models.default`.
/// Session creation uses this to apply a campaign to `/new` after boot: the
/// `ModelsManager`'s `current_model_id`
/// was resolved pre-campaign, and a campaign-only flip deliberately never
/// re-targets it (see `ModelsManager::apply_config`), so `/new` re-evaluates
/// here instead.
///
/// Reading the dismiss state fresh makes a `/model` pick win instantly:
/// [`persist_user_choice`] records the dismissal before the config write, so
/// the very next `/new` resolves campaign-free.
pub fn campaign_driven_models_default() -> Option<CampaignModelsDefault> {
    let layers = ConfigLayers::load().ok()?;
    campaign_driven_models_default_from(&layers, &load_dismissed_ids())
}

/// Env-free resolution core of [`campaign_driven_models_default`] (unit-testable
/// without touching `GROW_HOME` / the process-global cache).
fn campaign_driven_models_default_from(
    layers: &ConfigLayers,
    dismissed: &HashSet<String>,
) -> Option<CampaignModelsDefault> {
    let base = layers.effective_config_base();
    let active = resolve_active_campaigns_from_layers(layers, &base, dismissed);
    if active.is_empty() {
        return None;
    }
    let mut effective = base.clone();
    layers.apply_campaign_overrides(&mut effective, &active);
    let base_value = read_path(&base, MODELS_DEFAULT_PATH);
    let value = read_path(&effective, MODELS_DEFAULT_PATH);
    if value == base_value {
        return None;
    }
    Some(CampaignModelsDefault {
        value: as_string(value)?,
        pre_campaign: as_string(base_value),
    })
}

/// Read the value at `path` from an effective-config tree.
fn read_path(tree: &toml::Value, path: PatchPath) -> Option<toml::Value> {
    let mut cur = tree;
    for key in path {
        cur = cur.get(*key)?;
    }
    Some(cur.clone())
}

fn as_string(v: Option<toml::Value>) -> Option<String> {
    v.and_then(|v| v.as_str().map(str::to_owned))
}

/// Resolved campaign state for one [`CampaignField`] after the overlay.
struct CampaignFieldValue {
    /// Effective value (campaign value if it won, else the merged base value).
    value: Option<toml::Value>,
    /// Whether an active campaign actually changed the effective value.
    driven: bool,
    /// Pre-campaign value to recover to; `Some` only when `driven` and the base had one.
    recovery: Option<toml::Value>,
}

/// A config field a campaign may temporarily override until the user sets it.
/// `apply_campaign_fields` drives every [`CAMPAIGN_FIELDS`] entry, so the resolve
/// pass is one row here. A field still needs its runtime state, a `persist_*`
/// writer through [`persist_user_choice`], and any field-specific reaction (e.g.
/// the model catalog-miss/live-session handling in `agent::models`).
struct CampaignField {
    /// Path into the effective config; also the dismiss key shared with the writer.
    path: PatchPath,
    /// Store the resolved value, flag, and recovery onto the agent config.
    store: fn(&mut crate::agent::config::Config, CampaignFieldValue),
    /// Clear the campaign-driven flag + recovery (value untouched). Used when
    /// resolution fails so the runtime state is defined (fail closed, matching
    /// the apply path) instead of stale.
    reset: fn(&mut crate::agent::config::Config),
}

/// Path of the `models.default` campaign field, shared by the registry row and
/// its dismiss writer so the two can't drift.
const MODELS_DEFAULT_PATH: PatchPath = &["models", "default"];

const CAMPAIGN_FIELDS: &[CampaignField] = &[CampaignField {
    path: MODELS_DEFAULT_PATH,
    store: |cfg, r| {
        cfg.models.default = as_string(r.value);
        cfg.models.default_is_campaign_driven = r.driven;
        cfg.models.pre_campaign_default = as_string(r.recovery);
    },
    reset: |cfg| {
        cfg.models.default_is_campaign_driven = false;
        cfg.models.pre_campaign_default = None;
    },
}];

/// Resolve each [`CAMPAIGN_FIELDS`] entry's value, campaign-driven flag, and
/// recovery value from the campaign overlay and store them onto `cfg`. Pure given
/// the resolved `base`/`effective`/`active`; the I/O lives in [`sync_campaign_fields`].
fn apply_campaign_fields(
    cfg: &mut crate::agent::config::Config,
    base: &toml::Value,
    effective: &toml::Value,
    active: &[CampaignEntry],
) {
    for field in CAMPAIGN_FIELDS {
        let value = read_path(effective, field.path);
        let base_value = read_path(base, field.path);
        // A campaign only drives a field when it actually changed the effective
        // value (don't flag a no-op).
        let driven = value != base_value
            && active
                .iter()
                .any(|e| patch_touches_any(&e.patch, &[field.path]));
        let recovery = if driven { base_value } else { None };
        (field.store)(
            cfg,
            CampaignFieldValue {
                value,
                driven,
                recovery,
            },
        );
    }
}

/// Set every [`CAMPAIGN_FIELDS`] entry (value + flag + recovery) from the local
/// campaign overlay.
pub fn sync_campaign_fields(cfg: &mut crate::agent::config::Config) {
    let Ok(layers) = ConfigLayers::load() else {
        // Fail closed like the apply path: leave the field values as loaded but
        // clear the campaign-driven flags/recovery so they can't go stale (a
        // stale flag would mislabel a user value as campaign-driven, or vice
        // versa disarm the live-session guard for a campaign value).
        tracing::warn!("campaigns: config layer load failed; clearing campaign-driven field state");
        for field in CAMPAIGN_FIELDS {
            (field.reset)(cfg);
        }
        return;
    };
    let dismissed = load_dismissed_ids();
    let base = layers.effective_config_base();
    let active = resolve_active_campaigns_from_layers(&layers, &base, &dismissed);
    let mut effective = base.clone();
    layers.apply_campaign_overrides(&mut effective, &active);
    apply_campaign_fields(cfg, &base, &effective, &active);
}

/// Dismiss any active campaign whose patch touches `path`, then persist the
/// setting via `update_config`. The single field-keyed chokepoint, so a new
/// campaign-governable field is one call here with no per-field dismiss wiring.
///
/// Dismiss is recorded **before** the config write so a crash between the two
/// can't leave the campaign active over the user's just-saved value (re-nudge).
/// A dismiss-then-failed-write leaves the dismiss standing (fail-toward-no-nudge).
pub async fn persist_user_choice(
    path: PatchPath,
    write: impl FnOnce(&mut super::mcp::Config),
) -> anyhow::Result<()> {
    // Config-layer reads + the flock'd read-modify-write are blocking I/O;
    // keep them off the async worker. Awaited before the config write so the
    // dismiss-before-write ordering above holds. A panicked/cancelled dismiss
    // task must NOT abort the user's write: bookkeeping failure is logged and
    // the write proceeds (the campaign may re-nudge; the pick is never lost).
    let dismissed = tokio::task::spawn_blocking(move || {
        let ids = ids_touching_paths(&resolve_dismissable_campaigns(), &[path]);
        if !ids.is_empty() {
            tracing::info!(
                ?ids,
                ?path,
                "campaigns: dismissed after the user set the field"
            );
            dismiss_campaign_ids(ids);
        }
    })
    .await;
    if let Err(e) = dismissed {
        tracing::warn!(error = %e, "campaigns: dismiss bookkeeping task failed; persisting the choice anyway");
    }
    super::persist::update_config(write).await
}

/// Persist the default model (+ optional reasoning effort) through
/// [`persist_user_choice`], so picking a model dismisses a campaign nudging
/// `models.default`. `None` clears the field.
pub async fn persist_models_default(
    value: Option<String>,
    reasoning_effort: Option<sampling_types::ReasoningEffort>,
) -> anyhow::Result<()> {
    let s = value.unwrap_or_default();
    if s.len() > super::settings_writes::MAX_DEFAULT_MODEL_LEN {
        anyhow::bail!(
            "model name too long ({} > {} bytes)",
            s.len(),
            super::settings_writes::MAX_DEFAULT_MODEL_LEN
        );
    }
    persist_user_choice(MODELS_DEFAULT_PATH, move |cfg| {
        cfg.models.default = if s.is_empty() { None } else { Some(s) };
        if let Some(effort) = reasoning_effort {
            cfg.models.default_reasoning_effort = Some(effort);
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use config::ConfigLayers;
    use tempfile::tempdir;

    fn models_default_patch(default: &str) -> toml::Table {
        let mut models = toml::map::Map::new();
        models.insert("default".into(), toml::Value::String(default.into()));
        let mut t = toml::map::Map::new();
        t.insert("models".into(), toml::Value::Table(models));
        t
    }

    /// `campaign_driven_models_default_from` tracks local entries and
    /// dismissals: `Some` while the campaign is active, `None` the instant its
    /// dismissal lands, so a `/new` right after a `/model` pick never re-nudges.
    #[test]
    fn campaign_driven_models_default_tracks_local_and_dismissals() {
        let layers = ConfigLayers {
            user: toml::from_str("[models]\ndefault = \"config-model\"\n").unwrap(),
            ..Default::default()
        };
        let campaign = CampaignEntry {
            id: "t-models-nudge".into(),
            patch: models_default_patch("campaign-model"),
        };
        let mut layers = layers;
        layers.campaigns.user.push(campaign);

        let nudge = campaign_driven_models_default_from(&layers, &HashSet::new())
            .expect("active campaign drives the default");
        assert_eq!(nudge.value, "campaign-model");
        assert_eq!(nudge.pre_campaign.as_deref(), Some("config-model"));

        // A dismissal (what a `/model` pick records first) deactivates the
        // nudge for the very next resolution.
        let dismissed: HashSet<String> = ["t-models-nudge".to_string()].into_iter().collect();
        assert!(
            campaign_driven_models_default_from(&layers, &dismissed).is_none(),
            "a dismissed campaign must not nudge"
        );
    }

    /// Contract: `persist_user_choice(["models","default"], ..)` dismisses only
    /// campaigns that touch that path, never a sibling-field campaign. The full
    /// wiring starts at `set_default_model` and funnels through persist/dismiss.
    #[test]
    fn models_default_persist_targets_only_model_campaigns() {
        let model_campaign = CampaignEntry {
            id: "release".into(),
            patch: models_default_patch("new-model"),
        };
        let other_campaign = CampaignEntry {
            id: "other".into(),
            patch: toml::from_str::<toml::Table>("[features]\nweb_fetch = true\n").unwrap(),
        };
        let path: &[PatchPath] = &[&["models", "default"]];
        let ids = ids_touching_paths(&[model_campaign, other_campaign], path);
        assert_eq!(ids, vec!["release".to_string()]);
    }

    /// `apply_campaign_fields` flags a field campaign-driven only when the campaign
    /// actually changed the effective value: a campaign win sets the flag and
    /// recovery, while an unchanged effective value does not.
    #[test]
    fn campaign_field_flags_campaign_win_not_local_config_win() {
        let active = vec![CampaignEntry {
            id: "release".into(),
            patch: models_default_patch("campaign-model"),
        }];
        let base: toml::Value = toml::from_str("[models]\ndefault = \"base-model\"\n").unwrap();

        // Campaign won the effective default.
        let mut cfg = crate::agent::config::Config::default();
        let won: toml::Value = toml::from_str("[models]\ndefault = \"campaign-model\"\n").unwrap();
        apply_campaign_fields(&mut cfg, &base, &won, &active);
        assert_eq!(cfg.models.default.as_deref(), Some("campaign-model"));
        assert!(cfg.models.default_is_campaign_driven);
        assert_eq!(
            cfg.models.pre_campaign_default.as_deref(),
            Some("base-model")
        );

        // Effective config stayed at the base value.
        let mut cfg = crate::agent::config::Config::default();
        apply_campaign_fields(&mut cfg, &base, &base, &active);
        assert_eq!(cfg.models.default.as_deref(), Some("base-model"));
        assert!(!cfg.models.default_is_campaign_driven);
        assert_eq!(cfg.models.pre_campaign_default, None);

        // No active campaign touching the field: never driven.
        let mut cfg = crate::agent::config::Config::default();
        apply_campaign_fields(&mut cfg, &base, &won, &[]);
        assert!(!cfg.models.default_is_campaign_driven);
        assert_eq!(cfg.models.pre_campaign_default, None);
    }

    /// A local campaign whose id is already dismissed is dropped.
    #[test]
    fn dismissed_id_is_dropped_from_local_layer() {
        let mut layers = ConfigLayers::default();
        layers.campaigns.user.push(CampaignEntry {
            id: "seen".to_owned(),
            patch: models_default_patch("m"),
        });
        let base = toml::Value::Table(Default::default());
        let dismissed: HashSet<String> = ["seen".to_owned()].into_iter().collect();
        let active = resolve_active_campaigns_from_layers(&layers, &base, &dismissed);
        assert!(active.is_empty(), "a dismissed id must not re-apply");
    }

    /// Corrupt `campaigns_state.json` is preserved as `*.json.corrupt`, the new
    /// dismiss still lands, and the cap drops the oldest ids.
    #[test]
    fn dismiss_persists_handles_corrupt_and_caps() {
        let home = tempdir().unwrap();
        std::fs::write(campaigns_state_path(home.path()), "{ not json").unwrap();
        dismiss_campaign_ids_at(home.path(), ["new-id".to_owned()]).unwrap();
        assert!(
            home.path().join("campaigns_state.json.corrupt").exists(),
            "corrupt state must be renamed aside, not discarded"
        );

        dismiss_campaign_ids_at(home.path(), (0..40).map(|i| format!("id-{i}"))).unwrap();
        let contents = std::fs::read_to_string(campaigns_state_path(home.path())).unwrap();
        let set: HashSet<String> = serde_json::from_str::<CampaignsState>(&contents)
            .unwrap()
            .dismissed_ids
            .into_iter()
            .collect();
        assert_eq!(set.len(), MAX_DISMISSED_IDS);
        assert!(set.contains("id-39"));
        assert!(!set.contains("new-id"), "oldest ids evicted past the cap");
    }

    /// Every registry row's `reset` clears the campaign-driven runtime state a
    /// prior `store` set (used on the resolution-failure path so flags can't go
    /// stale).
    #[test]
    fn campaign_field_reset_clears_driven_state() {
        let mut cfg = crate::agent::config::Config::default();
        for field in CAMPAIGN_FIELDS {
            (field.store)(
                &mut cfg,
                CampaignFieldValue {
                    value: Some(toml::Value::String("campaign-model".into())),
                    driven: true,
                    recovery: Some(toml::Value::String("base-model".into())),
                },
            );
        }
        assert!(cfg.models.default_is_campaign_driven);
        assert!(cfg.models.pre_campaign_default.is_some());

        for field in CAMPAIGN_FIELDS {
            (field.reset)(&mut cfg);
        }
        assert!(!cfg.models.default_is_campaign_driven);
        assert_eq!(cfg.models.pre_campaign_default, None);
        // The field *value* is left as loaded; reset only clears the metadata.
        assert_eq!(cfg.models.default.as_deref(), Some("campaign-model"));
    }
}
