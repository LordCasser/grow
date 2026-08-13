//! Configured model catalog resolution and session model management.

use std::sync::Arc;

use parking_lot::RwLock;

use agent_client_protocol as acp;
use indexmap::IndexMap;

use crate::agent::config::{self, ModelEntry, resolve_credentials, sampling_config_for_model};
use crate::sampling::SamplerConfig as SamplingConfig;
use globset::{Glob, GlobSet, GlobSetBuilder};
use sampling_types::{ReasoningEffort, ReasoningEffortOption};

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

/// Complete live model configuration written under one lock, so readers never
/// observe a new provider config with an old catalog/current selection.
struct CatalogState {
    models: IndexMap<String, ModelEntry>,
    /// `allowed_models` matched nothing; the prompt path blocks instead.
    allowlist_excludes_all: bool,
    current_model_id: acp::ModelId,
    cfg: config::Config,
}

struct Inner {
    catalog: RwLock<CatalogState>,
    gateway: RwLock<Option<acp_transport::AcpAgentGatewaySender>>,
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
                    current_model_id,
                    cfg,
                }),
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

    pub(crate) fn set_gateway(&self, gateway: acp_transport::AcpAgentGatewaySender) {
        *self.inner.gateway.write() = Some(gateway);
    }

    /// Swap config, rebuild catalog, and reselect the model.
    pub fn apply_config(&self, new_config: config::Config) -> Result<(), String> {
        new_config.validate_llm_configuration()?;
        new_config.validate_model_filters()?;
        let new_catalog = resolve_model_catalog(&new_config);
        validate_selectable(&new_config, &new_catalog)?;

        // Serialize selection against an explicit model switch. Validation and
        // catalog construction stay outside the lock, but the old selection is
        // read only once the candidate is ready to commit; a concurrent user
        // choice therefore cannot be overwritten from a stale snapshot.
        let mut state = self.inner.catalog.write();
        let old_preferred = state.cfg.models.default.clone();
        let old_default_is_campaign = state.cfg.models.default_is_campaign_driven;
        let old_current = state.current_model_id.clone();
        let new_preferred = new_config.models.default.clone();
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
        let current_still_ok = new_catalog
            .get(old_current.0.as_ref())
            .is_some_and(|entry| entry.info.user_selectable);
        let new_current = if preferred_changed && !(campaign_only_flip && current_still_ok) {
            let (key, _, _) = resolve_default_model(&new_config, &new_catalog);
            acp::ModelId::new(Arc::from(key))
        } else if current_still_ok {
            old_current.clone()
        } else {
            let (key, _, source) = resolve_default_model(&new_config, &new_catalog);
            let selected = acp::ModelId::new(Arc::from(key));
            tracing::info!(
                old = %old_current.0,
                new = %selected.0,
                source = %source,
                "current model not in new catalog, reselecting default"
            );
            selected
        };

        // Validation and selection are complete before the single commit.
        // A failed candidate leaves every reader on the previous snapshot.
        *state = CatalogState {
            allowlist_excludes_all: allowlist_matches_nothing(&new_config, &new_catalog),
            models: new_catalog,
            current_model_id: new_current.clone(),
            cfg: new_config,
        };
        drop(state);
        if new_current != old_current {
            self.inner
                .model_switch_watch
                .send_modify(|generation| *generation += 1);
        }

        self.notify_models_updated();
        Ok(())
    }

    // ── Accessors ───────────────────────────────────────────────────

    pub fn models(&self) -> IndexMap<String, ModelEntry> {
        self.inner.catalog.read().models.clone()
    }

    pub fn endpoints(&self) -> config::EndpointsConfig {
        self.inner.catalog.read().cfg.endpoints.clone()
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
        self.inner.catalog.read().current_model_id.clone()
    }

    pub fn set_current_model_id(&self, id: acp::ModelId) {
        self.set_current_model_id_internal(id);
    }

    fn set_current_model_id_internal(&self, id: acp::ModelId) {
        let changed = {
            let mut state = self.inner.catalog.write();
            let changed = state.current_model_id != id;
            state.current_model_id = id;
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
    ) -> Option<sampling_types::CompactionsRemaining> {
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
    ) -> Option<sampling_types::CompactionAtTokens> {
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
        self.inner
            .catalog
            .read()
            .cfg
            .prompt_suggest_model_pin
            .clone()
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
        ::diagnostics::unified_log::info(
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
        let state = self.inner.catalog.read();
        let current_model = match state
            .models
            .get(state.current_model_id.0.as_ref())
            .or_else(|| state.models.values().next())
        {
            Some(m) => m,
            None => panic!("validated LLM configuration produced an empty model catalog"),
        };

        let credentials = resolve_credentials(current_model);

        sampling_config_for_model(
            current_model,
            credentials,
            state.cfg.endpoints.alpha_test_key.clone(),
        )
    }

    /// Build the live provider config for one session's selected catalog ID.
    /// Resolution accepts either the catalog slug or the provider model ID.
    pub fn sampling_config_for_model(&self, model_id: &str) -> Option<SamplingConfig> {
        let state = self.inner.catalog.read();
        let model = config::find_model_by_id(&state.models, model_id)?;
        Some(sampling_config_for_model(
            model,
            resolve_credentials(model),
            state.cfg.endpoints.alpha_test_key.clone(),
        ))
    }

    pub fn allowlist_excludes_all(&self) -> bool {
        self.inner.catalog.read().allowlist_excludes_all
    }
}

mod resolution;
pub(crate) use resolution::*;

#[cfg(test)]
mod tests;
