//! Shared announcement types, persistence, and formatting for Grow CLI apps.
//!
//! This crate provides the common logic used by `shell` and
//! `pager` for handling announcements (banner notifications).

use std::collections::BTreeSet;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// Types
// ─────────────────────────────────────────────────────────────────────────────

/// A locally configured announcement shown by Grow clients.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Announcement {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub severity: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub cta: Option<AnnouncementCta>,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub dismissible: Option<bool>,
    #[serde(default)]
    pub persistent: Option<bool>,
}

/// Optional call-to-action rendered as a clickable link or button.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnnouncementCta {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub caption: Option<String>,
}

/// Payload for the local `grow/announcements/update` ACP notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnouncementsUpdated {
    #[serde(default)]
    pub announcements: Vec<Announcement>,
}

/// Built-in content used when local configuration does not declare
/// `announcements`. An explicitly configured empty array disables announcements.
pub fn default_announcements() -> Vec<Announcement> {
    vec![Announcement {
        id: Some("grow-default".to_owned()),
        title: Some("Grow".to_owned()),
        message: Some(
            "Grow is a provider-neutral coding agent. Configure models and tools locally in ~/.grow/config.toml."
                .to_owned(),
        ),
        severity: Some("info".to_owned()),
        dismissible: Some(true),
        persistent: Some(false),
        ..Default::default()
    }]
}

// ─────────────────────────────────────────────────────────────────────────────
// Persistence
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HiddenAnnouncementState<'a> {
    #[serde(borrow)]
    hidden_ids: std::borrow::Cow<'a, BTreeSet<String>>,
}

/// Stable per-announcement hide key: the trimmed non-empty `id`, else a
/// content-derived fallback so id-less items are still hideable. The fallback
/// joins title/message with the unprintable unit separator (\x1f) so distinct
/// title/message splits cannot collide and real ids cannot plausibly match.
pub fn announcement_hide_key(a: &Announcement) -> String {
    match a.id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(id) => id.to_string(),
        None => format!(
            "content:{}\u{1f}{}",
            a.title.as_deref().unwrap_or_default(),
            a.message.as_deref().unwrap_or_default()
        ),
    }
}

/// Parse persisted hidden state into a set of hidden announcement ids.
/// Only the canonical exact shape is accepted; malformed or non-canonical
/// input yields an empty set so visibility fails open.
pub fn parse_hidden_announcement_ids(s: &str) -> BTreeSet<String> {
    serde_json::from_str::<HiddenAnnouncementState<'_>>(s)
        .map(|state| state.hidden_ids.into_owned())
        .unwrap_or_default()
}

/// Serialize hidden announcement ids (writes only the `hidden_ids` shape).
/// `BTreeSet` is load-bearing: deterministic order keeps the on-disk file
/// stable across writes.
pub fn serialize_hidden_announcement_ids(ids: &BTreeSet<String>) -> String {
    serde_json::to_string(&HiddenAnnouncementState {
        hidden_ids: std::borrow::Cow::Borrowed(ids),
    })
    .expect("a string set is always JSON-serializable")
}

/// Drop hidden ids whose announcement is no longer active; returns whether the
/// set changed (so callers can persist). Meant for real update paths only — a
/// per-frame prune would churn on transient list states.
pub fn prune_hidden_announcement_ids(ids: &mut BTreeSet<String>, active: &[Announcement]) -> bool {
    let live: BTreeSet<String> = active.iter().map(announcement_hide_key).collect();
    let before = ids.len();
    ids.retain(|id| live.contains(id));
    ids.len() != before
}

/// Read hidden announcement ids from `~/.grow/announcements.json`.
/// Returns an empty set (everything visible) on missing or malformed file.
pub async fn read_hidden_announcement_ids() -> BTreeSet<String> {
    let path = announcements_state_path();
    match tokio::fs::read_to_string(&path).await {
        Ok(s) => parse_hidden_announcement_ids(&s),
        Err(_) => BTreeSet::new(),
    }
}

/// Write hidden announcement ids to `~/.grow/announcements.json`.
pub async fn write_hidden_announcement_ids(ids: &BTreeSet<String>) {
    let path = announcements_state_path();
    let _ = tokio::fs::write(&path, serialize_hidden_announcement_ids(ids)).await;
}

fn announcements_state_path() -> PathBuf {
    tools::util::grow_home::grow_home().join("announcements.json")
}

// ─────────────────────────────────────────────────────────────────────────────
// Filtering
// ─────────────────────────────────────────────────────────────────────────────

/// Return only announcements with non-empty (trimmed) messages.
pub fn visible_announcements(announcements: &[Announcement]) -> Vec<&Announcement> {
    announcements
        .iter()
        .filter(|a| {
            a.message
                .as_ref()
                .map(|m| !m.trim().is_empty())
                .unwrap_or(false)
        })
        .collect()
}

/// Filter out announcements whose `expires_at` is in the past.
pub fn filter_expired(announcements: impl IntoIterator<Item = Announcement>) -> Vec<Announcement> {
    filter_expired_at(announcements, Utc::now())
}

/// [`filter_expired`] with an injectable clock, so expiry-crossing behavior
/// (an item that was live at the last check and has since passed `expires_at`)
/// is unit-testable.
pub fn filter_expired_at(
    announcements: impl IntoIterator<Item = Announcement>,
    now: DateTime<Utc>,
) -> Vec<Announcement> {
    announcements
        .into_iter()
        .filter(|a| !is_expired_at(a, now))
        .collect()
}

/// Whether `expires_at` parses and is at/behind `now` (strict `dt > now` keeps
/// an item live only before its expiry; missing/unparseable never expires).
/// Allocation-free per call, so draw-time consumers can check every frame.
pub fn is_expired_at(a: &Announcement, now: DateTime<Utc>) -> bool {
    if let Some(exp) = &a.expires_at
        && let Ok(dt) = DateTime::parse_from_rfc3339(exp)
    {
        return dt <= now;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_expired_removes_past() {
        let past = Announcement {
            expires_at: Some("2000-01-01T00:00:00Z".to_string()),
            ..Default::default()
        };
        let future = Announcement {
            expires_at: Some("2100-01-01T00:00:00Z".to_string()),
            ..Default::default()
        };
        let none = Announcement {
            expires_at: None,
            ..Default::default()
        };

        let filtered = filter_expired(vec![past, future, none]);
        assert_eq!(filtered.len(), 2);
    }

    /// The injected clock decides expiry: the same item is live before its
    /// `expires_at` and dropped at/after it (`dt > now` is a strict compare).
    #[test]
    fn filter_expired_at_honors_injected_clock() {
        let item = Announcement {
            expires_at: Some("2030-01-01T00:00:00Z".to_string()),
            ..Default::default()
        };
        let expiry = DateTime::parse_from_rfc3339("2030-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let before = expiry - chrono::Duration::seconds(1);
        assert_eq!(filter_expired_at(vec![item.clone()], before).len(), 1);
        assert!(filter_expired_at(vec![item.clone()], expiry).is_empty());
        let after = expiry + chrono::Duration::seconds(1);
        assert!(filter_expired_at(vec![item], after).is_empty());
    }

    /// The nested `cta` object is optional and per-field tolerant, matching
    /// the parent struct's style (a partial cta parses instead of poisoning).
    #[test]
    fn cta_parses_nested_partial_and_absent() {
        let full: Announcement = serde_json::from_str(
            r#"{"id":"p","severity":"promo","cta":{"label":"Open docs","url":"https://grow.example/docs","caption":"or use Ctrl+O"}}"#,
        )
        .unwrap();
        let cta = full.cta.as_ref().expect("cta present");
        assert_eq!(cta.label.as_deref(), Some("Open docs"));
        assert_eq!(cta.url.as_deref(), Some("https://grow.example/docs"));
        assert_eq!(cta.caption.as_deref(), Some("or use Ctrl+O"));

        let partial: Announcement =
            serde_json::from_str(r#"{"cta":{"label":"only label"}}"#).unwrap();
        assert_eq!(
            partial.cta,
            Some(AnnouncementCta {
                label: Some("only label".into()),
                url: None,
                caption: None,
            })
        );

        let absent: Announcement = serde_json::from_str(r#"{"id":"a"}"#).unwrap();
        assert_eq!(absent.cta, None);
    }

    #[test]
    fn hidden_ids_round_trip() {
        let ids: BTreeSet<String> = ["outage-a".to_string(), "outage-b".to_string()]
            .into_iter()
            .collect();
        let s = serialize_hidden_announcement_ids(&ids);
        assert_eq!(parse_hidden_announcement_ids(&s), ids);
        assert_eq!(s, r#"{"hidden_ids":["outage-a","outage-b"]}"#);

        let empty = BTreeSet::new();
        let s = serialize_hidden_announcement_ids(&empty);
        assert!(parse_hidden_announcement_ids(&s).is_empty());
    }

    #[test]
    fn parse_hidden_ids_rejects_noncanonical_state() {
        for rejected in [
            r#"{"hidden_ids":["a"],"unknown":true}"#,
            r#"{"hidden":true}"#,
            r#"{}"#,
            r#"{"hidden_ids":"oops"}"#,
            "",
            "not json",
        ] {
            assert!(
                parse_hidden_announcement_ids(rejected).is_empty(),
                "non-canonical state must fail open: {rejected}"
            );
        }
    }

    #[test]
    fn prune_hidden_ids_drops_ids_absent_from_active_list() {
        let active = vec![
            Announcement {
                id: Some("live".into()),
                ..Default::default()
            },
            Announcement {
                id: None,
                title: Some("T".into()),
                message: Some("M".into()),
                ..Default::default()
            },
        ];
        let mut ids: BTreeSet<String> = [
            "live".to_string(),
            "gone".to_string(),
            announcement_hide_key(&active[1]),
        ]
        .into_iter()
        .collect();

        assert!(prune_hidden_announcement_ids(&mut ids, &active));
        assert_eq!(ids.len(), 2);
        assert!(ids.contains("live"));
        assert!(ids.contains(&announcement_hide_key(&active[1])));

        // Second prune with the same list is a no-op.
        assert!(!prune_hidden_announcement_ids(&mut ids, &active));
    }

    #[test]
    fn announcement_hide_key_prefers_id_with_content_fallback() {
        let with_id = Announcement {
            id: Some("  spaced-id  ".into()),
            title: Some("T".into()),
            message: Some("M".into()),
            ..Default::default()
        };
        assert_eq!(announcement_hide_key(&with_id), "spaced-id");

        let blank_id = Announcement {
            id: Some("   ".into()),
            title: Some("T".into()),
            message: Some("M".into()),
            ..Default::default()
        };
        assert_eq!(announcement_hide_key(&blank_id), "content:T\u{1f}M");

        let no_id = Announcement::default();
        assert_eq!(announcement_hide_key(&no_id), "content:\u{1f}");

        // The unprintable separator disambiguates title/message splits.
        let ab_c = Announcement {
            title: Some("a|b".into()),
            message: Some("c".into()),
            ..Default::default()
        };
        let a_bc = Announcement {
            title: Some("a".into()),
            message: Some("b|c".into()),
            ..Default::default()
        };
        assert_ne!(announcement_hide_key(&ab_c), announcement_hide_key(&a_bc));
    }

    #[test]
    fn visible_announcements_filters_empty_message() {
        let a1 = Announcement {
            message: Some("valid".into()),
            ..Default::default()
        };
        let a2 = Announcement {
            message: None,
            ..Default::default()
        };
        let a3 = Announcement {
            message: Some("   ".into()),
            ..Default::default()
        };
        assert_eq!(visible_announcements(&[a1, a2, a3]).len(), 1);
    }
}
