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

const MAX_HIDDEN_STATE_BYTES: u64 = 1024 * 1024;

/// Read hidden announcement ids from `~/.grow/announcements.json`.
/// Returns an empty set (everything visible) on rejected, missing or malformed input.
pub async fn read_hidden_announcement_ids() -> BTreeSet<String> {
    let path = announcements_state_path();
    tokio::task::spawn_blocking(move || read_hidden_state_at(&path))
        .await.ok().and_then(Result::ok).unwrap_or_default()
}

fn state_admission_error() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData,
        "announcement state must be an ordinary file within 1 MiB")
}

fn read_hidden_state_at(path: &std::path::Path) -> std::io::Result<BTreeSet<String>> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)] {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_HIDDEN_STATE_BYTES {
        return Err(state_admission_error());
    }
    read_hidden_state_from(file)
}

fn read_hidden_state_from(reader: impl std::io::Read) -> std::io::Result<BTreeSet<String>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    reader.take(MAX_HIDDEN_STATE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_HIDDEN_STATE_BYTES { return Err(state_admission_error()); }
    let text = std::str::from_utf8(&bytes).map_err(|_| state_admission_error())?;
    Ok(parse_hidden_announcement_ids(text))
}

/// Write hidden announcement ids to `~/.grow/announcements.json`.
pub async fn write_hidden_announcement_ids(ids: &BTreeSet<String>) -> std::io::Result<()> {
    let path = announcements_state_path();
    let contents = serialize_hidden_announcement_ids(ids);
    tokio::task::spawn_blocking(move || write_hidden_state_at(&path, &contents))
        .await.map_err(std::io::Error::other)?
}

fn write_hidden_state_at(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    use std::io::Write;
    if contents.len() as u64 > MAX_HIDDEN_STATE_BYTES { return Err(state_admission_error()); }
    write_hidden_state_with(path, |file| file.write_all(contents.as_bytes()))
}

fn write_hidden_state_with(
    path: &std::path::Path,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::Builder::new().prefix(".announcements-").tempfile_in(parent)?;
    write(file.as_file_mut())?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|error| error.error)?;
    Ok(())
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
    fn announcement_state_read_and_write_byte_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let ids = BTreeSet::from(["汉字".into()]);
        let mut exact = serialize_hidden_announcement_ids(&ids);
        exact.extend(std::iter::repeat_n(' ', MAX_HIDDEN_STATE_BYTES as usize - exact.len()));
        write_hidden_state_at(&path, &exact).unwrap();
        assert_eq!(read_hidden_state_at(&path).unwrap(), ids);
        let over = exact.clone() + " ";
        assert!(write_hidden_state_at(&path, &over).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), exact);
        let mut cursor = std::io::Cursor::new(vec![b' '; MAX_HIDDEN_STATE_BYTES as usize + 20]);
        assert!(read_hidden_state_from(&mut cursor).is_err());
        assert_eq!(cursor.position(), MAX_HIDDEN_STATE_BYTES + 1);
        std::fs::write(&path, &over).unwrap();
        assert!(read_hidden_state_at(&path).is_err());
        assert_eq!(std::fs::metadata(&path).unwrap().len(), MAX_HIDDEN_STATE_BYTES + 1);
        std::fs::write(&path, "malformed").unwrap();
        assert!(read_hidden_state_at(&path).unwrap().is_empty());
        assert!(read_hidden_state_at(dir.path()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn announcement_state_rejects_fifo_and_accepts_regular_symlink() {
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("fifo");
        let cpath = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) }, 0);
        let (tx, rx) = std::sync::mpsc::channel();
        let source = fifo.clone();
        std::thread::spawn(move || { tx.send(read_hidden_state_at(&source).is_err()).unwrap(); });
        assert!(rx.recv_timeout(std::time::Duration::from_secs(2)).expect("FIFO admission blocked"));
        assert!(fifo.exists());
        let target = dir.path().join("state");
        let ids = BTreeSet::from(["one".into()]);
        write_hidden_state_at(&target, &serialize_hidden_announcement_ids(&ids)).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_eq!(read_hidden_state_at(&link).unwrap(), ids);
    }

    #[test]
    fn announcements_partial_temp_write_preserves_previous_commit() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("announcements.json");
        let previous = serialize_hidden_announcement_ids(&BTreeSet::from(["previous".into()]));
        write_hidden_state_at(&path, &previous).unwrap();
        let result = write_hidden_state_with(&path, |file| {
            file.write_all(b"partial")?;
            Err(std::io::Error::other("injected write failure"))
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), previous);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn announcements_commit_roundtrip_and_replace_complete_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/announcements.json");
        let ids = BTreeSet::from(["one".into(), "汉字".into()]);
        write_hidden_state_at(&path, &serialize_hidden_announcement_ids(&ids)).unwrap();
        assert_eq!(parse_hidden_announcement_ids(&std::fs::read_to_string(&path).unwrap()), ids);
        write_hidden_state_at(&path, &serialize_hidden_announcement_ids(&BTreeSet::new())).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"hidden_ids\":[]}");
        assert_eq!(std::fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn announcements_failed_publication_preserves_target_and_unowned_files() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("announcements.json");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("previous"), "keep").unwrap();
        let unowned = dir.path().join(".announcements-existing");
        std::fs::write(&unowned, "not ours").unwrap();
        assert!(write_hidden_state_at(&target, "{\"hidden_ids\":[]}").is_err());
        assert_eq!(std::fs::read_to_string(target.join("previous")).unwrap(), "keep");
        assert_eq!(std::fs::read_to_string(&unowned).unwrap(), "not ours");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
        let blocked_parent = dir.path().join("blocked");
        std::fs::write(&blocked_parent, "original").unwrap();
        assert!(write_hidden_state_at(&blocked_parent.join("state"), "{}").is_err());
        assert_eq!(std::fs::read_to_string(blocked_parent).unwrap(), "original");
    }

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
