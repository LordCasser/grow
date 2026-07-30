//! Configured model catalog resolution and session model management.

use std::sync::Arc;

use parking_lot::RwLock;

use agent_client_protocol as acp;
use indexmap::IndexMap;

use crate::agent::config::{self, ModelEntry, resolve_credentials, sampling_config_for_model};
use crate::sampling::SamplerConfig as SamplingConfig;
use globset::{Glob, GlobSet, GlobSetBuilder};
use grow_sampling_types::{ReasoningEffort, ReasoningEffortOption};

pub(crate) fn task_model_error_for_catalog(
    requested: &str,
    available: &IndexMap<String, ModelEntry>,
) -> Option<String> {
    let is_available = |entry: &ModelEntry| entry.info.user_selectable && !entry.info.hidden;
    if config::find_model_by_id(available, requested).is_some_and(&is_available) {
        return None;
    }

    let mut slugs = available
        .iter()
        .filter(|(_, entry)| is_available(entry))
        .map(|(slug, _)| slug.as_str())
        .collect::<Vec<_>>();
    slugs.sort_unstable();
    let guidance = if slugs.is_empty() {
        "No valid model slugs are currently available. Omit `model` to inherit the parent model."
            .to_string()
    } else {
        format!(
            "Valid model slugs: {}. Omit `model` to inherit the parent model.",
            slugs.join(", ")
        )
    };
    Some(format!("Unknown Task.model slug '{requested}'. {guidance}"))
}

/// Thread-safe model manager.
#[derive(Clone)]
pub struct ModelsManager {
    inner: Arc<Inner>,
}

/// Catalog fields written together under one lock, so readers never see a torn mix.
#[derive(Default)]
struct CatalogState {
    models: IndexMap<String, ModelEntry>,
    /// `allowed_models` matched nothing; the prompt path blocks instead.
    allowlist_excludes_all: bool,
}

struct Inner {
    catalog: RwLock<CatalogState>,
    current_model_id: RwLock<acp::ModelId>,
    cfg: RwLock<config::Config>,
    gateway: RwLock<Option<xai_acp_lib::AcpAgentGatewaySender>>,
    /// Model-switch signal: a generation counter bumped when the current model id changes.
    model_switch_watch: tokio::sync::watch::Sender<u64>,
}

impl Default for ModelsManager {
    fn default() -> Self {
        Self::new(
            IndexMap::new(),
            acp::ModelId::new("default"),
            config::Config::default(),
        )
    }
}

impl ModelsManager {
    pub(crate) fn new(
        models: IndexMap<String, ModelEntry>,
        current_model_id: acp::ModelId,
        cfg: config::Config,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                catalog: RwLock::new(CatalogState {
                    allowlist_excludes_all: allowlist_matches_nothing(&cfg, &models),
                    models,
                }),
                current_model_id: RwLock::new(current_model_id),
                cfg: RwLock::new(cfg),
                gateway: RwLock::new(None),
                model_switch_watch: tokio::sync::watch::channel(0u64).0,
            }),
        }
    }

    /// Subscribe to model-switch events. Returns a `watch::Receiver`
    pub fn subscribe_model_switch(&self) -> tokio::sync::watch::Receiver<u64> {
        self.inner.model_switch_watch.subscribe()
    }

    /// Cheap snapshot of the current model-switch generation, for the laziness-check poll loop.
    pub fn model_switch_generation(&self) -> u64 {
        *self.inner.model_switch_watch.borrow()
    }

    /// Build from the explicitly configured provider/model catalog.
    pub fn from_config(cfg: &config::Config) -> Result<Self, String> {
        cfg.validate_llm_configuration()?;
        let catalog = resolve_model_catalog(cfg);
        validate_selectable(cfg, &catalog)?;

        let (current_model_key, current_model, model_source) = resolve_default_model(cfg, &catalog);

        tracing::info!(
            model_id = %current_model.model,
            source = %model_source,
            "default model resolved"
        );

        let current_model_id = acp::ModelId::new(Arc::from(current_model_key));

        Ok(Self::new(catalog, current_model_id, cfg.clone()))
    }

    pub(crate) fn set_gateway(&self, gateway: xai_acp_lib::AcpAgentGatewaySender) {
        *self.inner.gateway.write() = Some(gateway);
    }

    /// Swap config, rebuild catalog, and reselect the model.
    pub fn apply_config(&self, new_config: config::Config) {
        if let Err(e) = new_config.validate_llm_configuration() {
            tracing::error!(error = %e, "ignoring config reload: invalid LLM configuration");
            return;
        }
        if let Err(e) = new_config.validate_model_filters() {
            tracing::error!(error = %e, "ignoring config reload: invalid model filters");
            return;
        }
        let new_catalog = resolve_model_catalog(&new_config);
        if let Err(e) = validate_selectable(&new_config, &new_catalog) {
            tracing::error!(error = %e, "ignoring config reload: allowed_models excludes all models");
            return;
        }

        let (old_preferred, old_default_is_campaign) = {
            let cfg = self.inner.cfg.read();
            (
                cfg.models.default.clone(),
                cfg.models.default_is_campaign_driven,
            )
        };
        let new_preferred = new_config.models.default.clone();
        *self.inner.cfg.write() = new_config.clone();
        {
            let mut cat = self.inner.catalog.write();
            cat.allowlist_excludes_all = allowlist_matches_nothing(&new_config, &new_catalog);
            cat.models = new_catalog;
        }

        let preferred_changed = new_preferred != old_preferred && new_preferred.is_some();
        let mut campaign_defaults = std::collections::HashSet::new();
        if new_config.models.default_is_campaign_driven
            && let Some(d) = &new_preferred
        {
            campaign_defaults.insert(d.clone());
        }
        if old_default_is_campaign && let Some(d) = &old_preferred {
            campaign_defaults.insert(d.clone());
        }
        let campaign_only_flip =
            is_campaign_only_flip(&old_preferred, &new_preferred, &campaign_defaults);
        let current_still_ok = {
            let cat = self.inner.catalog.read();
            let models = &cat.models;
            let cur = self.inner.current_model_id.read();
            models
                .get(cur.0.as_ref())
                .is_some_and(|e| e.info.user_selectable)
        };
        if preferred_changed && !(campaign_only_flip && current_still_ok) {
            self.reselect_default_model(&new_config);
        } else {
            self.reselect_current_model_if_missing(&new_config);
        }

        self.notify_models_updated();
    }

    /// [`Self::apply_config`] plus an unconditional default re-resolve.
    pub fn apply_config_reselecting_default(&self, new_config: config::Config) {
        self.apply_config(new_config.clone());
        self.reselect_default_model(&new_config);
        self.notify_models_updated();
    }

    // ── Accessors ───────────────────────────────────────────────────

    pub fn models(&self) -> IndexMap<String, ModelEntry> {
        self.inner.catalog.read().models.clone()
    }

    pub fn endpoints(&self) -> config::EndpointsConfig {
        self.inner.cfg.read().endpoints.clone()
    }

    /// ACP-visible (non-hidden) projection of the catalog.
    pub fn available(&self) -> IndexMap<acp::ModelId, acp::ModelInfo> {
        let snapshot = {
            let cat = self.inner.catalog.read();
            let models = &cat.models;
            models.clone()
        };

        let selectable: IndexMap<_, _> = snapshot
            .into_iter()
            .filter(|(_, e)| e.info.user_selectable)
            .collect();

        available_models(&selectable)
    }

    pub(crate) fn task_model_error(&self, requested: &str) -> Option<String> {
        let cat = self.inner.catalog.read();
        let models = &cat.models;
        task_model_error_for_catalog(requested, models)
    }

    pub fn current_model_id(&self) -> acp::ModelId {
        self.inner.current_model_id.read().clone()
    }

    pub fn set_current_model_id(&self, id: acp::ModelId) {
        self.set_current_model_id_internal(id);
    }

    fn set_current_model_id_internal(&self, id: acp::ModelId) {
        let changed = {
            let mut cur = self.inner.current_model_id.write();
            let changed = *cur != id;
            *cur = id;
            changed
        };
        if changed {
            self.inner
                .model_switch_watch
                .send_modify(|generation| *generation += 1);
        }
    }

    /// Per-model Layer-3 LazinessDetector config for `model_id` (disabled default when absent).
    pub fn laziness_detector_for(&self, model_id: &str) -> config::LazinessDetectorPerModelConfig {
        self.inner
            .catalog
            .read()
            .models
            .get(model_id)
            .map(|e| e.info().laziness_detector.clone())
            .unwrap_or_default()
    }

    /// Test-only catalog poke: inserts a `ModelEntry` keyed by `id`,
    #[cfg(test)]
    pub(crate) fn insert_test_entry(&self, id: impl Into<String>, entry: ModelEntry) {
        self.inner.catalog.write().models.insert(id.into(), entry);
    }

    /// Whether the given model supports reasoning effort according to the catalog.
    pub fn model_supports_reasoning_effort(&self, model_id: &str) -> bool {
        self.inner
            .catalog
            .read()
            .models
            .get(model_id)
            .map(|e| e.info().supports_reasoning_effort)
            .unwrap_or(false)
    }

    pub fn model_default_reasoning_effort(&self, model_id: &str) -> Option<ReasoningEffort> {
        self.inner
            .catalog
            .read()
            .models
            .get(model_id)
            .and_then(|e| e.info().reasoning_effort)
    }

    /// The raw catalog `reasoning_efforts` list for `model_id` with no fallback,
    pub fn model_reasoning_efforts(&self, model_id: &str) -> Vec<ReasoningEffortOption> {
        self.inner
            .catalog
            .read()
            .models
            .get(model_id)
            .map(|e| e.info().reasoning_efforts.clone())
            .unwrap_or_default()
    }

    pub fn model_compactions_remaining(
        &self,
        model_id: &str,
    ) -> Option<grow_sampling_types::CompactionsRemaining> {
        self.inner
            .catalog
            .read()
            .models
            .get(model_id)
            .and_then(|e| e.info().compactions_remaining)
    }

    pub fn model_compaction_at_tokens(
        &self,
        model_id: &str,
    ) -> Option<grow_sampling_types::CompactionAtTokens> {
        self.inner
            .catalog
            .read()
            .models
            .get(model_id)
            .and_then(|e| e.info().compaction_at_tokens)
    }

    /// Catalog opt-in to display the served-checkpoint fingerprint for this model.
    pub fn model_show_model_fingerprint(&self, model_id: &str) -> bool {
        let cat = self.inner.catalog.read();
        let models = &cat.models;
        resolve_catalog_key(models, &acp::ModelId::new(model_id))
            .and_then(|key| models.get(key.0.as_ref()))
            .map(|e| e.info().show_model_fingerprint)
            .unwrap_or(false)
    }

    /// Resolved next-prompt-suggestion model pin from the live config
    pub fn prompt_suggest_model_pin(&self) -> crate::config::PromptSuggestModelPin {
        self.inner.cfg.read().prompt_suggest_model_pin.clone()
    }

    /// Whether `model_id` resolves in the current catalog — as a config key
    pub fn model_in_catalog(&self, model_id: &str) -> bool {
        let cat = self.inner.catalog.read();
        let models = &cat.models;
        resolve_catalog_key(models, &acp::ModelId::new(model_id)).is_some()
    }

    // ── Mutations ───────────────────────────────────────────────────

    fn notify_models_updated(&self) {
        let available = self.available();
        let current = self.current_model_id();
        let count = available.len();
        grow_diagnostics::unified_log::info(
            "model catalog: notifying clients",
            None,
            Some(serde_json::json!({
                "model_count": count,
                "current_model_id": current.0.as_ref(),
            })),
        );
        if let Some(ref gw) = *self.inner.gateway.read() {
            let model_state =
                acp::SessionModelState::new(current, available.values().cloned().collect());
            if let Ok(params) = serde_json::value::to_raw_value(&model_state) {
                gw.forward_fire_and_forget(acp::ExtNotification::new(
                    "grow/models/update",
                    params.into(),
                ));
            }
        }
    }

    /// Build a `SamplingConfig` from the current model + auth state.
    pub fn sampling_config(&self) -> SamplingConfig {
        let config = self.inner.cfg.read().clone();
        let current_model_id = self.current_model_id();
        let all_models = self.models();
        let current_model = match all_models
            .get(current_model_id.0.as_ref())
            .or_else(|| all_models.values().next())
        {
            Some(m) => m,
            None => panic!("validated LLM configuration produced an empty model catalog"),
        };

        let credentials = resolve_credentials(current_model, None);

        sampling_config_for_model(
            current_model,
            credentials,
            config.endpoints.alpha_test_key.clone(),
        )
    }

    pub fn allowlist_excludes_all(&self) -> bool {
        self.inner.catalog.read().allowlist_excludes_all
    }

    /// Re-pick the default if `current_model_id` is gone from the catalog *or*
    fn reselect_current_model_if_missing(&self, config: &config::Config) {
        let current = self.inner.current_model_id.read().clone();
        let needs_reselection = {
            let cat = self.inner.catalog.read();
            let models = &cat.models;
            match models.get(current.0.as_ref()) {
                None => true,
                Some(entry) => !entry.info.user_selectable,
            }
        };
        if !needs_reselection {
            return;
        }
        let (key, _, source) = {
            let cat = self.inner.catalog.read();
            let models = &cat.models;
            resolve_default_model(config, models)
        };
        let new_id = acp::ModelId::new(Arc::from(key));
        tracing::info!(
            old = %current.0, new = %new_id.0, source = %source,
            "current model not in new catalog, reselecting default"
        );
        self.set_current_model_id_internal(new_id);
    }

    /// Re-resolve the default model against the current catalog.
    fn reselect_default_model(&self, config: &config::Config) {
        let (key, _, source) = {
            let cat = self.inner.catalog.read();
            let models = &cat.models;
            resolve_default_model(config, models)
        };
        let new_id = acp::ModelId::new(Arc::from(key));
        let current = self.inner.current_model_id.read().clone();
        if current.0.as_ref() != new_id.0.as_ref() {
            tracing::info!(
                old = %current.0, new = %new_id.0, source = %source,
                "re-resolved default model after catalog populated"
            );
            self.set_current_model_id_internal(new_id);
        }
    }
}

mod resolution;
pub(crate) use resolution::*;

#[cfg(test)]
mod tests;
