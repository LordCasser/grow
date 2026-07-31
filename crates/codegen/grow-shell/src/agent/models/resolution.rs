use super::*;

/// Map a model id (catalog key or routing slug) to its catalog key.
pub(crate) fn resolve_catalog_key(
    models: &IndexMap<String, ModelEntry>,
    id: &acp::ModelId,
) -> Option<acp::ModelId> {
    let id_str = id.0.as_ref();
    if models.contains_key(id_str) {
        return Some(id.clone());
    }
    models
        .iter()
        .rev()
        .find(|(_, entry)| entry.info.model == id_str)
        .map(|(key, _)| acp::ModelId::new(key.clone()))
}

/// Restore a persisted session model by its stable catalog identity.
///
/// Persisted sessions never guess by routing slug: two providers may expose
/// the same upstream model name, while `provider/model` remains unambiguous.
///
/// Exception — unique slug recovery: sessions created before write-path
/// normalization could persist a bare client slug (`_meta.modelId` passed
/// through verbatim, e.g. `deepseek-v4-flash` instead of
/// `deepseek/deepseek-v4-flash`). A bare slug is recovered only when
/// **exactly one** catalog entry exposes it and that entry's key is in
/// `available` — the same slug semantics as [`resolve_catalog_key`] but
/// strictly unique, so an ambiguous slug (two providers exposing the same
/// model name) still fails rather than guessing which provider was meant.
pub(crate) fn selectable_catalog_key_for_persisted(
    models: &IndexMap<String, ModelEntry>,
    available: &IndexMap<acp::ModelId, acp::ModelInfo>,
    id: &acp::ModelId,
) -> Option<acp::ModelId> {
    if models.contains_key(id.0.as_ref()) && available.contains_key(id) {
        return Some(id.clone());
    }
    let id_str = id.0.as_ref();
    let mut slug_matches = models
        .iter()
        .filter(|(_, entry)| entry.info.model == id_str)
        .map(|(key, _)| key.clone());
    let unique_key = slug_matches.next()?;
    if slug_matches.next().is_some() {
        return None;
    }
    let key = acp::ModelId::new(unique_key);
    available.contains_key(&key).then_some(key)
}

/// Notice the caller must surface to the user when a requested session model
/// cannot be resolved to a catalog key and the new session falls back to the
/// default model instead (never a silent fallback).
pub(crate) struct ModelFallbackNotice {
    pub requested: acp::ModelId,
    pub reason: String,
}

/// Resolve the model id to persist for a brand-new session.
///
/// A client-requested model (`_meta.modelId`, already gated by
/// `resolve_model_id`) is normalized to its canonical `provider/model`
/// catalog key so the persisted summary never stores a bare slug. When
/// normalization fails (the catalog changed between validation and
/// persistence), the default model is returned together with a
/// [`ModelFallbackNotice`] describing what was requested and why it fell back.
pub(crate) fn resolve_new_session_model_id(
    models: &IndexMap<String, ModelEntry>,
    resolved_custom_model: Option<&str>,
    current_default: &acp::ModelId,
) -> (acp::ModelId, Option<ModelFallbackNotice>) {
    let Some(requested) = resolved_custom_model else {
        return (current_default.clone(), None);
    };
    let requested_id = acp::ModelId::new(requested.to_string());
    match resolve_catalog_key(models, &requested_id) {
        Some(canonical) => (canonical, None),
        None => (
            current_default.clone(),
            Some(ModelFallbackNotice {
                requested: requested_id,
                reason: format!(
                    "\"{requested}\" is no longer configured, so this session is using \"{}\".",
                    current_default.0
                ),
            }),
        ),
    }
}

/// A "campaign-only" preferred flip: the default changed and either side's value
pub(crate) fn is_campaign_only_flip(
    old_preferred: &Option<String>,
    new_preferred: &Option<String>,
    campaign_defaults: &std::collections::HashSet<String>,
) -> bool {
    if new_preferred == old_preferred || new_preferred.is_none() {
        return false;
    }
    new_preferred
        .as_ref()
        .is_some_and(|p| campaign_defaults.contains(p))
        || old_preferred
            .as_ref()
            .is_some_and(|p| campaign_defaults.contains(p))
}

/// Pick the default model: explicit CLI/env override, then `[models].default`.
pub(crate) fn resolve_default_model(
    cfg: &config::Config,
    catalog: &IndexMap<String, ModelEntry>,
) -> (String, ModelEntry, config::ConfigSource) {
    let visible: IndexMap<String, ModelEntry> = catalog
        .iter()
        .filter(|(_, e)| !e.info.hidden && e.info.user_selectable)
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let model_pref = config::resolve_string_flag(
        cfg.default_model_override.as_deref(),
        "GROW_DEFAULT_MODEL",
        cfg.models.default.as_deref(),
        None,
    );

    let first_or_fallback = || -> (String, ModelEntry) {
        if let Some((key, first)) = visible.first() {
            return (key.clone(), first.clone());
        }
        if let Some((key, entry)) = catalog.iter().find(|(_, e)| e.info.user_selectable) {
            tracing::warn!("no auth-visible selectable model; using first selectable entry");
            return (key.clone(), entry.clone());
        }
        panic!("validated LLM configuration produced no selectable model")
    };

    match &model_pref {
        None => {
            let (key, first) = first_or_fallback();
            (key, first, config::ConfigSource::Default)
        }
        Some(pref) => {
            let found = visible
                .get_key_value(&pref.value)
                .or_else(|| visible.iter().find(|(_, m)| m.model == pref.value));

            if let Some((key, entry)) = found {
                (key.clone(), entry.clone(), pref.source)
            } else {
                let is_explicit = matches!(
                    pref.source,
                    config::ConfigSource::Cli
                        | config::ConfigSource::Env
                        | config::ConfigSource::Config
                );
                if is_explicit {
                    tracing::warn!(
                        model_id = %pref.value, source = %pref.source,
                        "preferred model not in available models, falling back"
                    );
                } else {
                    tracing::debug!(
                        model_id = %pref.value, source = %pref.source,
                        "remote default_model not in available models, skipping"
                    );
                }
                let campaign_pref_missing = cfg.models.default_is_campaign_driven
                    && matches!(pref.source, config::ConfigSource::Config);
                if campaign_pref_missing
                    && let Some(prev) = cfg
                        .models
                        .pre_campaign_default
                        .as_deref()
                        .filter(|s| !s.is_empty())
                    && let Some((key, entry)) = visible
                        .get_key_value(prev)
                        .or_else(|| visible.iter().find(|(_, m)| m.model == prev))
                {
                    tracing::info!(
                        unavailable = %pref.value, fallback = %prev,
                        "campaign-driven default unavailable in catalog; recovering the pre-campaign default"
                    );
                    return (key.clone(), entry.clone(), config::ConfigSource::Config);
                }
                let (key, first) = first_or_fallback();
                (key, first, config::ConfigSource::Default)
            }
        }
    }
}

/// Filter hidden and auth-gated entries out of `catalog` and convert to ACP wire format.
pub fn available_models(
    catalog: &IndexMap<String, ModelEntry>,
) -> IndexMap<acp::ModelId, acp::ModelInfo> {
    let visible: IndexMap<String, ModelEntry> = catalog
        .iter()
        .filter(|(_, e)| !e.info.hidden)
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    config::to_acp_model_info(&visible)
}

/// Compiled glob matcher shared by `allowed_models`, `disabled_models`, and `hidden_models` (matched against catalog key or model id).
pub(crate) struct ModelGlobSet(GlobSet);

impl ModelGlobSet {
    /// Compile a filter list (`Ok(None)` for `None`/empty). Fails **closed**: an invalid pattern returns `Err` listing every bad one.
    pub(crate) fn compile(patterns: Option<&Vec<String>>) -> Result<Option<Self>, Vec<String>> {
        let patterns = match patterns {
            Some(p) if !p.is_empty() => p,
            _ => return Ok(None),
        };
        let mut builder = GlobSetBuilder::new();
        let mut invalid = Vec::new();
        for pat in patterns {
            match Glob::new(pat) {
                Ok(glob) => {
                    builder.add(glob);
                }
                Err(_) => invalid.push(pat.clone()),
            }
        }
        if !invalid.is_empty() {
            return Err(invalid);
        }
        builder
            .build()
            .map(|set| Some(Self(set)))
            .map_err(|e| vec![e.to_string()])
    }

    fn matches(&self, key: &str, model: &str) -> bool {
        self.0.is_match(key) || self.0.is_match(model)
    }
}

/// Single source of truth for the catalog. Applies, in order: `disabled_models`
pub fn resolve_model_catalog(cfg: &config::Config) -> IndexMap<String, ModelEntry> {
    let mut catalog: IndexMap<String, ModelEntry> = config::resolve_model_list(cfg);

    if let Ok(Some(disabled)) = ModelGlobSet::compile(cfg.models.disabled_models.as_ref()) {
        let before = catalog.len();
        catalog.retain(|key, entry| !disabled.matches(key, &entry.model));
        let removed = before - catalog.len();
        if removed > 0 {
            tracing::info!(count = removed, "disabled_models: removed from catalog");
        }
    }

    match ModelGlobSet::compile(cfg.models.allowed_models.as_ref()) {
        Ok(None) => {
            for entry in catalog.values_mut() {
                entry.info.user_selectable = true;
            }
        }
        Ok(Some(allowed)) => {
            for (key, entry) in catalog.iter_mut() {
                entry.info.user_selectable = allowed.matches(key, &entry.model);
            }
        }
        Err(bad) => {
            tracing::error!(patterns = ?bad, "allowed_models: invalid glob(s); marking nothing selectable");
            for entry in catalog.values_mut() {
                entry.info.user_selectable = false;
            }
        }
    }

    if let Ok(Some(hidden)) = ModelGlobSet::compile(cfg.models.hidden_models.as_ref()) {
        for (key, entry) in catalog.iter_mut() {
            if hidden.matches(key, &entry.model) {
                entry.info.hidden = true;
            }
        }
    }

    for entry in catalog.values_mut() {
        if entry.info.supports_reasoning_effort && entry.info.reasoning_effort.is_none() {
            entry.info.reasoning_effort = cfg
                .models
                .default_reasoning_effort
                .filter(|effort| model_offers_reasoning_effort(&entry.info, *effort))
                .or_else(|| lowest_reasoning_effort(&entry.info));
        }
    }

    if let Some(effort) = cfg.reasoning_effort_override {
        for entry in catalog.values_mut() {
            if model_offers_reasoning_effort(&entry.info, effort) {
                entry.info.reasoning_effort = Some(effort);
            }
        }
    }

    catalog
}

/// Lowest effort a model declares it can accept. Models using the legacy
/// support flag have the legacy `low..xhigh` menu, whose lowest value is low.
fn lowest_reasoning_effort(info: &config::ModelInfo) -> Option<ReasoningEffort> {
    if !info.supports_reasoning_effort {
        return None;
    }
    if info.reasoning_efforts.is_empty() {
        return Some(ReasoningEffort::Low);
    }
    info.reasoning_efforts
        .iter()
        .min_by_key(|opt| reasoning_effort_rank(opt.value))
        .map(|opt| opt.value)
}

fn reasoning_effort_rank(effort: ReasoningEffort) -> u8 {
    match effort {
        ReasoningEffort::None => 0,
        ReasoningEffort::Minimal => 1,
        ReasoningEffort::Low => 2,
        ReasoningEffort::Medium => 3,
        ReasoningEffort::High => 4,
        ReasoningEffort::Xhigh => 5,
        ReasoningEffort::Max => 6,
    }
}

/// Whether `effort` is a value this model will accept on the wire.
fn model_offers_reasoning_effort(info: &config::ModelInfo, effort: ReasoningEffort) -> bool {
    if !info.supports_reasoning_effort {
        return false;
    }
    if info.reasoning_efforts.is_empty() {
        matches!(
            effort,
            ReasoningEffort::Low
                | ReasoningEffort::Medium
                | ReasoningEffort::High
                | ReasoningEffort::Xhigh
        )
    } else {
        info.reasoning_efforts.iter().any(|opt| opt.value == effort)
    }
}

/// True when an active `allowed_models` allowlist leaves no selectable model.
pub(crate) fn allowlist_matches_nothing(
    cfg: &config::Config,
    catalog: &IndexMap<String, ModelEntry>,
) -> bool {
    cfg.models
        .allowed_models
        .as_ref()
        .is_some_and(|a| !a.is_empty())
        && !catalog.values().any(|e| e.info.user_selectable)
}

/// Reject an `allowed_models` allowlist that leaves no selectable model, or excludes an explicitly configured default; run only against a real catalog.
pub(crate) fn validate_selectable(
    cfg: &config::Config,
    catalog: &IndexMap<String, ModelEntry>,
) -> Result<(), String> {
    let Some(allowed) = cfg.models.allowed_models.as_ref().filter(|a| !a.is_empty()) else {
        return Ok(());
    };
    let patterns = allowed.join(", ");
    if !catalog.values().any(|e| e.info.user_selectable) {
        return Err(format!(
            "None of your available models match allowed_models ({patterns}). \
             Broaden the patterns or remove allowed_models, then try again."
        ));
    }
    for (src, id) in [
        ("default", cfg.models.default.as_deref()),
        ("-m flag", cfg.default_model_override.as_deref()),
    ] {
        if let Some(id) = id
            && let Some(entry) = catalog
                .get(id)
                .or_else(|| catalog.values().find(|e| e.model == id))
            && !entry.info.user_selectable
        {
            return Err(format!(
                "\"{id}\" (your {src}) isn't allowed by allowed_models ({patterns}). \
                 Add it to allowed_models, or set a different model."
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(slug: &str) -> ModelEntry {
        config::ModelEntry::fallback(slug)
    }

    /// `(catalog_key, routing_slug)` pairs -> catalog map.
    fn catalog(pairs: &[(&str, &str)]) -> IndexMap<String, ModelEntry> {
        pairs
            .iter()
            .map(|(key, slug)| (key.to_string(), entry(slug)))
            .collect()
    }

    /// The `user_selectable`-style projection `ModelsManager::available()`
    /// feeds the loader: only the given keys, non-hidden.
    fn available_for(
        models: &IndexMap<String, ModelEntry>,
        keys: &[&str],
    ) -> IndexMap<acp::ModelId, acp::ModelInfo> {
        let subset: IndexMap<String, ModelEntry> = keys
            .iter()
            .filter_map(|k| models.get(*k).map(|e| (k.to_string(), e.clone())))
            .collect();
        available_models(&subset)
    }

    #[test]
    fn persisted_exact_key_match_returns_the_key() {
        let models = catalog(&[("deepseek/deepseek-v4-flash", "deepseek-v4-flash")]);
        let available = available_for(&models, &["deepseek/deepseek-v4-flash"]);
        let id = acp::ModelId::new("deepseek/deepseek-v4-flash");
        assert_eq!(
            selectable_catalog_key_for_persisted(&models, &available, &id),
            Some(id)
        );
    }

    #[test]
    fn persisted_exact_key_match_not_selectable_returns_none() {
        let models = catalog(&[
            ("deepseek/deepseek-v4-flash", "deepseek-v4-flash"),
            ("other/other-model", "other-model"),
        ]);
        let available = available_for(&models, &["other/other-model"]);
        assert_eq!(
            selectable_catalog_key_for_persisted(
                &models,
                &available,
                &acp::ModelId::new("deepseek/deepseek-v4-flash"),
            ),
            None
        );
    }

    #[test]
    fn persisted_unique_slug_recovers_the_catalog_key() {
        let models = catalog(&[("deepseek/deepseek-v4-flash", "deepseek-v4-flash")]);
        let available = available_for(&models, &["deepseek/deepseek-v4-flash"]);
        assert_eq!(
            selectable_catalog_key_for_persisted(
                &models,
                &available,
                &acp::ModelId::new("deepseek-v4-flash"),
            ),
            Some(acp::ModelId::new("deepseek/deepseek-v4-flash"))
        );
    }

    #[test]
    fn persisted_ambiguous_slug_returns_none() {
        let models = catalog(&[
            ("provider-a/deepseek-v4-flash", "deepseek-v4-flash"),
            ("provider-b/deepseek-v4-flash", "deepseek-v4-flash"),
        ]);
        let available = available_for(
            &models,
            &[
                "provider-a/deepseek-v4-flash",
                "provider-b/deepseek-v4-flash",
            ],
        );
        assert_eq!(
            selectable_catalog_key_for_persisted(
                &models,
                &available,
                &acp::ModelId::new("deepseek-v4-flash"),
            ),
            None,
            "an ambiguous slug must never be guessed, even though a unique \
             slug is recoverable"
        );
    }

    #[test]
    fn persisted_slug_match_not_selectable_returns_none() {
        let models = catalog(&[
            ("deepseek/deepseek-v4-flash", "deepseek-v4-flash"),
            ("other/other-model", "other-model"),
        ]);
        // The slug matches exactly one entry, but that key is absent from the
        // selectable projection (e.g. `user_selectable = false`).
        let available = available_for(&models, &["other/other-model"]);
        assert_eq!(
            selectable_catalog_key_for_persisted(
                &models,
                &available,
                &acp::ModelId::new("deepseek-v4-flash"),
            ),
            None
        );
    }

    #[test]
    fn persisted_unknown_id_returns_none() {
        let models = catalog(&[("deepseek/deepseek-v4-flash", "deepseek-v4-flash")]);
        let available = available_for(&models, &["deepseek/deepseek-v4-flash"]);
        assert_eq!(
            selectable_catalog_key_for_persisted(
                &models,
                &available,
                &acp::ModelId::new("totally-unknown"),
            ),
            None
        );
    }

    #[test]
    fn new_session_model_normalizes_bare_slug_to_canonical_key() {
        let models = catalog(&[("deepseek/deepseek-v4-flash", "deepseek-v4-flash")]);
        let default = acp::ModelId::new("deepseek/deepseek-v4-pro");
        let (model_id, notice) =
            resolve_new_session_model_id(&models, Some("deepseek-v4-flash"), &default);
        assert_eq!(model_id, acp::ModelId::new("deepseek/deepseek-v4-flash"));
        assert!(notice.is_none());
    }

    #[test]
    fn new_session_model_keeps_an_already_qualified_key() {
        let models = catalog(&[("deepseek/deepseek-v4-flash", "deepseek-v4-flash")]);
        let default = acp::ModelId::new("deepseek/deepseek-v4-pro");
        let (model_id, notice) =
            resolve_new_session_model_id(&models, Some("deepseek/deepseek-v4-flash"), &default);
        assert_eq!(model_id, acp::ModelId::new("deepseek/deepseek-v4-flash"));
        assert!(notice.is_none());
    }

    #[test]
    fn new_session_model_without_custom_request_uses_default() {
        let models = catalog(&[("deepseek/deepseek-v4-flash", "deepseek-v4-flash")]);
        let default = acp::ModelId::new("deepseek/deepseek-v4-pro");
        let (model_id, notice) = resolve_new_session_model_id(&models, None, &default);
        assert_eq!(model_id, default);
        assert!(notice.is_none());
    }

    #[test]
    fn new_session_model_unresolvable_request_signals_fallback_notice() {
        let models = catalog(&[("other/other-model", "other-model")]);
        let default = acp::ModelId::new("other/other-model");
        let (model_id, notice) =
            resolve_new_session_model_id(&models, Some("deepseek-v4-flash"), &default);
        assert_eq!(
            model_id, default,
            "an unresolvable request must fall back to the default model"
        );
        let notice = notice.expect("an unresolvable request must surface a fallback notice");
        assert_eq!(notice.requested, acp::ModelId::new("deepseek-v4-flash"));
        assert!(
            notice.reason.contains("deepseek-v4-flash"),
            "reason should name the requested model: {}",
            notice.reason
        );
        assert!(
            notice.reason.contains("other/other-model"),
            "reason should name the fallback model: {}",
            notice.reason
        );
    }
}
