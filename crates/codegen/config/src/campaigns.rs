//! Local `[[campaigns]]` overlays from `$GROW_HOME/config.toml`.

use serde::{Deserialize, Serialize};

use crate::config_override::{
    ConfigOverrideEntry, PATCH_STRIP_KEYS, PatchPath, apply_patches, patch_touches_any,
    take_patch_array,
};

pub const CAMPAIGNS_KEY: &str = "campaigns";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CampaignMeta {
    #[serde(default)]
    pub id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CampaignEntry {
    pub id: String,
    pub patch: toml::Table,
}

#[derive(Debug, Clone, Default)]
pub struct CampaignOverrides {
    pub user: Vec<CampaignEntry>,
}

pub fn take_campaigns(config: &mut toml::Value) -> Vec<ConfigOverrideEntry<CampaignMeta>> {
    match take_patch_array::<CampaignMeta>(config, CAMPAIGNS_KEY) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!(error = %e, "campaigns: failed to deserialize; ignoring entries");
            Vec::new()
        }
    }
}

pub fn build_campaign_entries(
    taken: Vec<ConfigOverrideEntry<CampaignMeta>>,
    layer: &'static str,
) -> Vec<CampaignEntry> {
    let mut out = Vec::with_capacity(taken.len());
    for entry in taken {
        let Some(id) = entry
            .meta
            .id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
        else {
            tracing::warn!(layer, "campaigns: entry missing id; skipped");
            continue;
        };
        // Skip no-op entries (id only, no fields to overlay).
        if entry.patch.is_empty() {
            continue;
        }
        out.push(CampaignEntry {
            id,
            patch: entry.patch,
        });
    }
    out
}

/// `sources` in priority order; first `id` wins.
pub fn merge_campaign_entries(sources: &[&[CampaignEntry]]) -> Vec<CampaignEntry> {
    let mut seen = std::collections::HashSet::<String>::new();
    let mut out = Vec::new();
    for source in sources {
        for entry in *source {
            if !seen.insert(entry.id.clone()) {
                continue;
            }
            out.push(entry.clone());
        }
    }
    out
}

/// Drop dismissed ids from a priority-merged list, preserving order.
pub fn filter_active_campaigns(
    merged: Vec<CampaignEntry>,
    dismissed_ids: &std::collections::HashSet<String>,
) -> Vec<CampaignEntry> {
    merged
        .into_iter()
        .filter(|e| !dismissed_ids.contains(&e.id))
        .collect()
}

/// Ids of `active` campaigns whose patch touches any of `paths` — used to dismiss
/// campaigns when the user persists a value at one of those paths.
pub fn ids_touching_paths(active: &[CampaignEntry], paths: &[PatchPath]) -> Vec<String> {
    active
        .iter()
        .filter(|e| patch_touches_any(&e.patch, paths))
        .map(|e| e.id.clone())
        .collect()
}

/// `active` is highest-priority-first; patches apply lowest-first (`.rev()`) so the
/// highest-priority source wins a leaf conflict.
pub fn apply_active_campaign_patches(effective: &mut toml::Value, active: &[CampaignEntry]) {
    apply_patches(
        effective,
        active.iter().rev().map(|e| e.patch.clone()),
        PATCH_STRIP_KEYS,
    );
}

pub fn take_campaign_entries(config: &mut toml::Value, layer: &'static str) -> Vec<CampaignEntry> {
    build_campaign_entries(take_campaigns(config), layer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> toml::Value {
        toml::from_str(s).unwrap()
    }

    fn models_default_patch(default: &str) -> toml::Table {
        let mut models = toml::map::Map::new();
        models.insert("default".into(), toml::Value::String(default.into()));
        let mut t = toml::map::Map::new();
        t.insert("models".into(), toml::Value::Table(models));
        t
    }

    #[test]
    fn campaign_overlays_any_field_over_user_config() {
        let mut layer = parse(
            r#"
            [[campaigns]]
            id = "c1"
            [campaigns.models]
            default = "new-model"
            [campaigns.features]
            web_fetch = true
            "#,
        );
        let entries = take_campaign_entries(&mut layer, "local");
        assert!(layer.get(CAMPAIGNS_KEY).is_none());
        assert_eq!(entries.len(), 1);

        let mut effective =
            parse("[models]\ndefault = \"old-model\"\n[features]\nweb_fetch = false\n");
        apply_active_campaign_patches(&mut effective, &entries);
        assert_eq!(effective["models"]["default"].as_str(), Some("new-model"));
        assert_eq!(effective["features"]["web_fetch"].as_bool(), Some(true));
    }

    #[test]
    fn merge_first_source_wins_duplicate_id() {
        let local = [CampaignEntry {
            id: "same".into(),
            patch: models_default_patch("from-local"),
        }];
        let lower_priority = [CampaignEntry {
            id: "same".into(),
            patch: models_default_patch("from-lower-priority"),
        }];
        let merged = merge_campaign_entries(&[&local, &lower_priority]);
        assert_eq!(merged.len(), 1);
        assert_eq!(
            merged[0].patch["models"]["default"].as_str(),
            Some("from-local")
        );
    }

    #[test]
    fn apply_highest_priority_wins_on_leaf_conflict() {
        // Two *distinct* ids both set models.default; the higher-priority source
        // (earlier in the merged list) must win the leaf.
        let local = [CampaignEntry {
            id: "local".into(),
            patch: models_default_patch("from-local"),
        }];
        let lower_priority = [CampaignEntry {
            id: "lower-priority".into(),
            patch: models_default_patch("from-lower-priority"),
        }];
        let merged = merge_campaign_entries(&[&local, &lower_priority]);
        assert_eq!(merged.len(), 2);

        let mut effective = parse("[models]\ndefault = \"user-old\"\n");
        apply_active_campaign_patches(&mut effective, &merged);
        assert_eq!(effective["models"]["default"].as_str(), Some("from-local"));
    }

    #[test]
    fn build_campaign_entries_skips_missing_id() {
        // A `None` id and a whitespace-only id are both dropped (with a warn);
        // only the entry carrying a real id survives.
        let taken = vec![
            ConfigOverrideEntry {
                meta: CampaignMeta { id: None },
                patch: models_default_patch("dropped-none"),
            },
            ConfigOverrideEntry {
                meta: CampaignMeta {
                    id: Some("   ".into()),
                },
                patch: models_default_patch("dropped-blank"),
            },
            ConfigOverrideEntry {
                meta: CampaignMeta {
                    id: Some("valid".into()),
                },
                patch: models_default_patch("kept"),
            },
        ];
        let out = build_campaign_entries(taken, "local");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "valid");
        assert_eq!(out[0].patch["models"]["default"].as_str(), Some("kept"));
    }

    #[test]
    fn campaign_requires_canonical_id() {
        let mut canonical =
            parse("[[campaigns]]\nid = \"c1\"\n[campaigns.models]\ndefault = \"m\"\n");
        let entries = take_campaign_entries(&mut canonical, "user");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "c1");
        assert!(entries[0].patch.get("id").is_none());

        let mut obsolete =
            parse("[[campaigns]]\ncampaign_id = \"c1\"\n[campaigns.models]\ndefault = \"m\"\n");
        assert!(take_campaign_entries(&mut obsolete, "user").is_empty());
    }

    #[test]
    fn effective_config_honors_dismiss() {
        use crate::loader::ConfigLayers;
        // A dismissed campaign id stops overriding; the user's stored value returns.
        let mut layers = ConfigLayers {
            user: parse("[models]\ndefault = \"user-old\"\n"),
            ..Default::default()
        };
        layers.campaigns.user = vec![CampaignEntry {
            id: "c1".into(),
            patch: models_default_patch("new"),
        }];

        let none = std::collections::HashSet::new();
        let mut active = layers.effective_config_base();
        let active_campaigns = layers.resolve_campaigns(&active, &none);
        layers.apply_campaign_overrides(&mut active, &active_campaigns);
        assert_eq!(active["models"]["default"].as_str(), Some("new"));

        let dismissed: std::collections::HashSet<_> = ["c1".into()].into_iter().collect();
        let mut off = layers.effective_config_base();
        let active_campaigns = layers.resolve_campaigns(&off, &dismissed);
        layers.apply_campaign_overrides(&mut off, &active_campaigns);
        assert_eq!(off["models"]["default"].as_str(), Some("user-old"));
    }
}
