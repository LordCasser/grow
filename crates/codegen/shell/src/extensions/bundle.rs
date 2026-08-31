//! ACP extension handlers for bundled subagent cache sync and status.
//!
//! These endpoints operate on the on-disk bundled cache only. Sync updates the
//! cache for future agent construction / future conversations; it does not live
//! reload the currently running `MvpAgent` instance.
use super::{ExtResult, parse_params, to_ext_response};
use crate::agent::MvpAgent;
use crate::bundle::{self, BundleManifest};
use crate::remote::{BundleServiceCredential, fetch_bundle};
use acp_transport::protocol as acp;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;
/// Default freshness window for the proactive bundle sync. Bypassed by `force`.
pub(crate) const BUNDLE_SYNC_TTL: Duration = Duration::from_secs(60 * 60);
/// Error message returned when no auth source is available for a bundle sync.
///
/// Hoisted to a constant so the user-facing wording stays in lockstep
/// across `sync_bundle`, `sync_bundle_to_root`, and any future call sites.
pub(crate) const NO_BUNDLE_CREDENTIALS_ERROR: &str =
    "bundle sync requires GROW_DEPLOYMENT_KEY and an HTTPS GROW_BUNDLE_SERVICE_BASE_URL";
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BundleSyncRequest {
    #[serde(default)]
    force: bool,
}
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BundleSyncResult {
    pub updated: bool,
    pub version: String,
    pub agents_count: usize,
    pub skills_count: usize,
}
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BundleStatusRequest {}
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BundleStatusResult {
    pub has_cache: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub agents: Vec<String>,
    pub skills: Vec<String>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EntryGetRequest {
    kind: String,
    name: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryGetResult {
    pub kind: String,
    pub name: String,
    pub content: String,
}
pub async fn handle(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    match args.method.as_ref() {
        "grow/bundle/sync" => {
            let req: BundleSyncRequest = parse_params(args)?;
            to_ext_response(sync_bundle(agent, req).await)
        }
        "grow/bundle/status" => {
            let _req: BundleStatusRequest = parse_params(args)?;
            to_ext_response(status_bundle())
        }
        "grow/bundle/entry/get" => {
            let req: EntryGetRequest = parse_params(args)?;
            to_ext_response(get_entry(&req.kind, &req.name))
        }
        _ => Err(acp::Error::method_not_found()),
    }
}
async fn sync_bundle(agent: &MvpAgent, req: BundleSyncRequest) -> anyhow::Result<BundleSyncResult> {
    let Some(credential) = agent.bundle_service_credential() else {
        anyhow::bail!(NO_BUNDLE_CREDENTIALS_ERROR);
    };
    sync_bundle_to_root(&bundle::bundled_root(), &credential, req.force).await
}
/// `true` when `<root>/manifest.json` exists, was written within `ttl`, and
/// is parseable as a [`BundleManifest`].
///
/// The parse check guards against the silent-skip failure mode where the
/// mtime is recent (e.g., a partial/aborted write) but the manifest is
/// truncated or otherwise corrupt. A bare mtime check would let
/// `maybe_sync_bundle_to_root` proactively skip a re-sync, leaving callers
/// (`status_bundle_at`, `SubagentsConfig::resolve`) to fail later with an
/// empty or stale catalog. Treating an unparseable manifest as "not fresh"
/// forces a re-sync on the next post-auth event.
pub(crate) fn bundle_cache_is_fresh(root: &Path, ttl: Duration) -> bool {
    let manifest = root.join("manifest.json");
    let Ok(meta) = std::fs::metadata(&manifest) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };
    let within_ttl = modified
        .elapsed()
        .map(|elapsed| elapsed < ttl)
        .unwrap_or(false);
    if !within_ttl {
        return false;
    }
    matches!(bundle::read_cached_manifest(root), Ok(Some(_)))
}
/// Proactive variant of [`sync_bundle_to_root`] that respects the credential gate
/// and a TTL guard.
///
/// Returns:
/// - `Ok(Some(result))` when a sync was performed.
/// - `Ok(None)` when the call was skipped (no credentials or cache fresh).
/// - `Err(_)` when sync was attempted but the network call or extract failed.
pub(crate) async fn maybe_sync_bundle_to_root(
    root: &Path,
    credential: &BundleServiceCredential,
    force: bool,
    ttl: Duration,
) -> anyhow::Result<Option<BundleSyncResult>> {
    if !force && bundle_cache_is_fresh(root, ttl) {
        tracing::debug!(
            ttl_secs = ttl.as_secs(),
            "proactive bundle sync skipped: cache is fresh"
        );
        return Ok(None);
    }
    sync_bundle_to_root(root, credential, force).await.map(Some)
}
pub(crate) async fn sync_bundle_to_root(
    root: &Path,
    credential: &BundleServiceCredential,
    _force: bool,
) -> anyhow::Result<BundleSyncResult> {
    let bytes = fetch_bundle(credential).await?;
    let root_owned = root.to_path_buf();
    let manifest =
        tokio::task::spawn_blocking(move || bundle::extract_bundle_archive(&root_owned, &bytes))
            .await
            .context("bundle extract task panicked")??;
    Ok(BundleSyncResult {
        updated: true,
        version: manifest.version.clone(),
        agents_count: bundle::count_entries_by_prefix(&manifest, "agents/"),
        skills_count: bundle::count_entries_by_prefix(&manifest, "skills/"),
    })
}
fn get_entry(kind: &str, name: &str) -> anyhow::Result<EntryGetResult> {
    get_entry_at(&bundle::bundled_root(), kind, name)
}
fn validate_entry_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name == "."
    {
        anyhow::bail!("invalid entry name: {name}");
    }
    Ok(())
}
fn get_entry_at(root: &Path, kind: &str, name: &str) -> anyhow::Result<EntryGetResult> {
    validate_entry_name(name)?;
    let (dir_name, ext) = match kind {
        "agent" => ("agents", "md"),
        _ => anyhow::bail!("unknown entry kind: {kind}"),
    };
    let path = root.join(dir_name).join(format!("{name}.{ext}"));
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("{kind} '{name}' not found in bundle cache"))?;
    Ok(EntryGetResult {
        kind: kind.to_owned(),
        name: name.to_owned(),
        content,
    })
}
fn status_bundle() -> anyhow::Result<BundleStatusResult> {
    status_bundle_at(&bundle::bundled_root())
}
fn status_bundle_at(root: &Path) -> anyhow::Result<BundleStatusResult> {
    let Some(manifest) = bundle::read_cached_manifest(root)? else {
        return Ok(BundleStatusResult {
            has_cache: false,
            version: None,
            agents: Vec::new(),
            skills: Vec::new(),
        });
    };
    let agents = list_cached_entries(root, &manifest, "agents", "md");
    let skills = list_cached_skill_entries(root, &manifest);
    Ok(BundleStatusResult {
        has_cache: true,
        version: Some(manifest.version.clone()),
        agents,
        skills,
    })
}
fn list_cached_entries(
    root: &Path,
    manifest: &BundleManifest,
    dir_name: &str,
    extension: &str,
) -> Vec<String> {
    let dir = root.join(dir_name);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() {
                return None;
            }
            let file_name = path.file_name()?.to_str()?;
            let relative_path = format!("{dir_name}/{file_name}");
            if !manifest.checksums.contains_key(&relative_path) {
                return None;
            }
            match path.extension().and_then(|ext| ext.to_str()) {
                Some(ext) if ext == extension => path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .map(ToOwned::to_owned),
                _ => None,
            }
        })
        .collect();
    names.sort();
    names
}
fn list_cached_skill_entries(root: &Path, manifest: &BundleManifest) -> Vec<String> {
    let prefix = "skills/";
    let mut names: Vec<String> = manifest
        .checksums
        .keys()
        .filter_map(|k| {
            let name = k.strip_prefix(prefix)?.strip_suffix("/SKILL.md")?;
            root.join(k).is_file().then(|| name.to_owned())
        })
        .collect();
    names.sort();
    names
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_without_cache_is_empty() {
        let root = tempfile::tempdir().unwrap();
        let status = status_bundle_at(root.path()).unwrap();
        assert!(!status.has_cache);
        assert_eq!(status.version, None);
        assert!(status.agents.is_empty());
        assert!(status.skills.is_empty());
    }

    #[test]
    fn status_reads_only_manifest_backed_entries() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("agents")).unwrap();
        std::fs::create_dir_all(root.path().join("skills/commit")).unwrap();
        std::fs::write(root.path().join("agents/reviewer.md"), "# Reviewer").unwrap();
        std::fs::write(root.path().join("agents/unmanaged.md"), "# Unmanaged").unwrap();
        std::fs::write(root.path().join("skills/commit/SKILL.md"), "# Commit").unwrap();
        std::fs::write(
            root.path().join("manifest.json"),
            serde_json::to_vec(&BundleManifest {
                version: "v2".into(),
                checksums: std::collections::HashMap::from([
                    ("agents/reviewer.md".into(), "x".into()),
                    ("skills/commit/SKILL.md".into(), "y".into()),
                ]),
            })
            .unwrap(),
        )
        .unwrap();
        let status = status_bundle_at(root.path()).unwrap();
        assert!(status.has_cache);
        assert_eq!(status.version.as_deref(), Some("v2"));
        assert_eq!(status.agents, vec!["reviewer"]);
        assert_eq!(status.skills, vec!["commit"]);
    }

    #[test]
    fn entry_get_accepts_only_canonical_agent_kind() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("agents")).unwrap();
        std::fs::write(root.path().join("agents/reviewer.md"), "# Reviewer").unwrap();
        let entry = get_entry_at(root.path(), "agent", "reviewer").unwrap();
        assert_eq!(entry.kind, "agent");
        assert_eq!(entry.content, "# Reviewer");
        assert!(get_entry_at(root.path(), "unknown", "reviewer").is_err());
        assert!(get_entry_at(root.path(), "agent", "../escape").is_err());
    }

    #[test]
    fn cache_freshness_requires_parseable_manifest() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("manifest.json"), "not json").unwrap();
        assert!(!bundle_cache_is_fresh(root.path(), Duration::from_secs(60)));
    }
}
