use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};

const BUNDLED_DIR_NAME: &str = "bundled";
const MANIFEST_FILE_NAME: &str = "manifest.json";

const ARCHIVE_MAX_DECOMPRESSED_SIZE: usize = 50 * 1024 * 1024;
const ARCHIVE_MAX_ENTRIES: usize = 1000;
const ARCHIVE_MAX_ENTRY_SIZE: u64 = 1024 * 1024;

#[derive(Deserialize)]
struct ArchiveBundleMetadata {
    version: String,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestBundleFixture {
    pub version: String,
    pub agents: HashMap<String, String>,
    #[serde(default)]
    pub skills: HashMap<String, String>,
}

#[cfg(test)]
impl TestBundleFixture {
    pub fn empty(version: impl Into<String>) -> Self {
        Self {
            version: version.into(),
            agents: HashMap::new(),
            skills: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleManifest {
    pub version: String,
    pub checksums: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BundleFileKind {
    Agent,
    Skill,
}

impl BundleFileKind {
    fn dir_name(self) -> &'static str {
        match self {
            Self::Agent => "agents",
            Self::Skill => "skills",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Agent | Self::Skill => "md",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Skill => "skill",
        }
    }

    fn from_dir_name(dir_name: &str) -> Option<Self> {
        match dir_name {
            "agents" => Some(Self::Agent),
            "skills" => Some(Self::Skill),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BundleFileState {
    Absent,
    MatchesManaged,
    ModifiedOrUnmanaged,
}

#[derive(Debug)]
#[cfg(test)]
struct BundleFile<'a> {
    relative_path: String,
    checksum: String,
    content: &'a str,
}

pub fn bundled_root() -> PathBuf {
    config::grow_home().join(BUNDLED_DIR_NAME)
}

pub fn read_cached_manifest(root: &Path) -> Result<Option<BundleManifest>> {
    let manifest_path = manifest_path(root);
    let bytes = match std::fs::read(&manifest_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to read {}", manifest_path.display()));
        }
    };

    serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse {}", manifest_path.display()))
        .map(Some)
}

#[cfg(test)]
pub fn install_test_bundle_fixture(
    root: &Path,
    bundle: &TestBundleFixture,
) -> Result<BundleManifest> {
    let old_manifest = read_cached_manifest(root)?.map(sanitize_manifest);
    ensure_bundle_dirs(root)?;

    let bundle_files = bundle_files(bundle)?;
    let mut next_checksums = HashMap::new();

    for bundle_file in &bundle_files {
        let previous_checksum = old_manifest
            .as_ref()
            .and_then(|manifest| manifest.checksums.get(&bundle_file.relative_path));
        let absolute_path = root.join(&bundle_file.relative_path);

        match bundle_file_state(&absolute_path, previous_checksum.map(String::as_str))? {
            BundleFileState::Absent | BundleFileState::MatchesManaged => {
                write_bundle_file(&absolute_path, bundle_file.content.as_bytes())?;
                next_checksums.insert(
                    bundle_file.relative_path.clone(),
                    bundle_file.checksum.clone(),
                );
            }
            BundleFileState::ModifiedOrUnmanaged => {
                if let Some(previous_checksum) = previous_checksum {
                    next_checksums
                        .insert(bundle_file.relative_path.clone(), previous_checksum.clone());
                }
            }
        }
    }

    if let Some(old_manifest) = old_manifest.as_ref() {
        prune_removed_files(root, old_manifest, &mut next_checksums)?;
    }

    let next_manifest = BundleManifest {
        version: bundle.version.clone(),
        checksums: next_checksums,
    };
    let manifest_json =
        serde_json::to_vec_pretty(&next_manifest).context("failed to serialize bundle manifest")?;
    std::fs::write(manifest_path(root), manifest_json)
        .with_context(|| format!("failed to write {}", manifest_path(root).display()))?;

    Ok(next_manifest)
}

pub fn extract_bundle_archive(root: &Path, archive_bytes: &[u8]) -> Result<BundleManifest> {
    let decoder = flate2::read::GzDecoder::new(archive_bytes);
    let mut archive = tar::Archive::new(decoder);

    let old_manifest = read_cached_manifest(root)?.map(sanitize_manifest);
    ensure_bundle_dirs(root)?;

    let mut next_checksums = HashMap::new();
    let mut version = String::new();
    let mut total_decompressed: usize = 0;
    let mut entry_count: usize = 0;

    for entry_result in archive
        .entries()
        .context("failed to read archive entries")?
    {
        let mut entry = entry_result.context("failed to read archive entry")?;

        if entry.header().entry_type() != tar::EntryType::Regular {
            continue;
        }

        entry_count += 1;
        if entry_count > ARCHIVE_MAX_ENTRIES {
            bail!("archive exceeds maximum entry count ({ARCHIVE_MAX_ENTRIES})");
        }

        let entry_size = entry.header().size().context("failed to read entry size")?;
        if entry_size > ARCHIVE_MAX_ENTRY_SIZE {
            bail!("archive entry exceeds maximum size ({ARCHIVE_MAX_ENTRY_SIZE} bytes)");
        }

        total_decompressed = total_decompressed
            .checked_add(entry_size as usize)
            .context("decompressed size overflow")?;
        if total_decompressed > ARCHIVE_MAX_DECOMPRESSED_SIZE {
            bail!(
                "archive exceeds maximum decompressed size ({ARCHIVE_MAX_DECOMPRESSED_SIZE} bytes)"
            );
        }

        let raw_path = entry
            .path()
            .context("failed to read entry path")?
            .to_string_lossy()
            .into_owned();
        let path = raw_path.strip_prefix("./").unwrap_or(&raw_path);

        if path == "bundle.json" {
            let mut content = String::new();
            entry
                .read_to_string(&mut content)
                .context("failed to read bundle.json")?;
            let meta: ArchiveBundleMetadata =
                serde_json::from_str(&content).context("failed to parse bundle.json")?;
            version = meta.version;
            continue;
        }

        let cache_relative_path = match map_archive_path_to_cache_path(path) {
            Some(p) => p,
            None => continue,
        };

        let mut content = Vec::with_capacity(entry_size as usize);
        entry
            .read_to_end(&mut content)
            .with_context(|| format!("failed to read archive entry: {path}"))?;
        let checksum = checksum_bytes(&content);

        let absolute_path = root.join(&cache_relative_path);
        let previous_checksum = old_manifest
            .as_ref()
            .and_then(|m| m.checksums.get(&cache_relative_path));

        match bundle_file_state(&absolute_path, previous_checksum.map(String::as_str))? {
            BundleFileState::Absent | BundleFileState::MatchesManaged => {
                write_bundle_file(&absolute_path, &content)?;
                next_checksums.insert(cache_relative_path, checksum);
            }
            BundleFileState::ModifiedOrUnmanaged => {
                if let Some(prev) = previous_checksum {
                    next_checksums.insert(cache_relative_path, prev.clone());
                }
            }
        }
    }

    if version.is_empty() {
        bail!("archive missing bundle.json with version field");
    }

    if let Some(old_manifest) = old_manifest.as_ref() {
        prune_removed_files(root, old_manifest, &mut next_checksums)?;
    }

    let next_manifest = BundleManifest {
        version,
        checksums: next_checksums,
    };
    let manifest_json =
        serde_json::to_vec_pretty(&next_manifest).context("failed to serialize bundle manifest")?;
    std::fs::write(manifest_path(root), manifest_json)
        .with_context(|| format!("failed to write {}", manifest_path(root).display()))?;

    Ok(next_manifest)
}

pub fn checksum_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

pub fn checksum_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read {} for checksum", path.display()))?;
    Ok(checksum_bytes(&bytes))
}

pub fn prune_removed_files(
    root: &Path,
    old_manifest: &BundleManifest,
    retained_checksums: &mut HashMap<String, String>,
) -> Result<()> {
    for (relative_path, previous_checksum) in sanitize_manifest(old_manifest.clone()).checksums {
        if retained_checksums.contains_key(&relative_path) {
            continue;
        }

        let absolute_path = root.join(&relative_path);
        match bundle_file_state(&absolute_path, Some(previous_checksum.as_str()))? {
            BundleFileState::Absent => {}
            BundleFileState::MatchesManaged => {
                std::fs::remove_file(&absolute_path)
                    .with_context(|| format!("failed to remove {}", absolute_path.display()))?;
            }
            BundleFileState::ModifiedOrUnmanaged => {
                retained_checksums.insert(relative_path, previous_checksum);
            }
        }
    }

    Ok(())
}

fn ensure_bundle_dirs(root: &Path) -> Result<()> {
    std::fs::create_dir_all(root)
        .with_context(|| format!("failed to create {}", root.display()))?;

    for dir_name in ["agents", "skills"] {
        let dir = root.join(dir_name);
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create {}", dir.display()))?;
    }

    Ok(())
}

fn manifest_path(root: &Path) -> PathBuf {
    root.join(MANIFEST_FILE_NAME)
}

fn checksum_file_if_exists(path: &Path) -> Result<Option<String>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(checksum_bytes(&bytes))),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => {
            Err(error).with_context(|| format!("failed to read {} for checksum", path.display()))
        }
    }
}

fn write_bundle_file(absolute_path: &Path, content: &[u8]) -> Result<()> {
    if let Some(parent) = absolute_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    std::fs::write(absolute_path, content)
        .with_context(|| format!("failed to write {}", absolute_path.display()))?;
    Ok(())
}

fn bundle_file_state(path: &Path, old_checksum: Option<&str>) -> Result<BundleFileState> {
    let current_checksum = match checksum_file_if_exists(path)? {
        Some(checksum) => checksum,
        None => return Ok(BundleFileState::Absent),
    };

    Ok(match old_checksum {
        Some(old_checksum) if current_checksum == old_checksum => BundleFileState::MatchesManaged,
        Some(_) | None => BundleFileState::ModifiedOrUnmanaged,
    })
}

fn sanitize_manifest(manifest: BundleManifest) -> BundleManifest {
    let checksums = manifest
        .checksums
        .into_iter()
        .filter_map(|(relative_path, checksum)| {
            sanitize_relative_path(&relative_path).map(|relative_path| (relative_path, checksum))
        })
        .collect();

    BundleManifest {
        version: manifest.version,
        checksums,
    }
}

fn sanitize_relative_path(relative_path: &str) -> Option<String> {
    if relative_path.is_empty() || relative_path.starts_with('/') || relative_path.contains('\\') {
        return None;
    }

    let mut parts = relative_path.split('/');
    let dir_name = parts.next()?;
    let second = parts.next()?;

    match parts.next() {
        None => {
            if second.is_empty() {
                return None;
            }
            let kind = BundleFileKind::from_dir_name(dir_name)?;
            if kind == BundleFileKind::Skill {
                return None;
            }
            let file_stem = second.strip_suffix(&format!(".{}", kind.extension()))?;
            validate_bundle_name(kind, file_stem).ok()?;
            Some(relative_path_for(kind, file_stem))
        }
        Some(third) => {
            if dir_name != "skills" {
                return None;
            }
            validate_bundle_name(BundleFileKind::Skill, second).ok()?;
            // Reject components that would let extraction escape the per-skill directory.
            for component in std::iter::once(third).chain(parts) {
                if component.is_empty()
                    || component == "."
                    || component == ".."
                    || component.chars().any(char::is_control)
                {
                    return None;
                }
            }
            Some(relative_path.to_string())
        }
    }
}

fn map_archive_path_to_cache_path(archive_path: &str) -> Option<String> {
    if let Some(rest) = archive_path.strip_prefix("subagents/") {
        return sanitize_relative_path(rest);
    }
    if archive_path.starts_with("skills/") {
        return sanitize_relative_path(archive_path);
    }
    None
}

pub fn count_entries_by_prefix(manifest: &BundleManifest, prefix: &str) -> usize {
    manifest
        .checksums
        .keys()
        .filter(|k| k.starts_with(prefix))
        .count()
}

#[cfg(test)]
fn bundle_files(bundle: &TestBundleFixture) -> Result<Vec<BundleFile<'_>>> {
    let mut files = Vec::new();
    extend_bundle_files(&mut files, BundleFileKind::Agent, &bundle.agents)?;
    extend_bundle_files(&mut files, BundleFileKind::Skill, &bundle.skills)?;
    Ok(files)
}

#[cfg(test)]
fn extend_bundle_files<'a>(
    files: &mut Vec<BundleFile<'a>>,
    kind: BundleFileKind,
    entries: &'a HashMap<String, String>,
) -> Result<()> {
    for (name, content) in entries {
        validate_bundle_name(kind, name)?;
        files.push(BundleFile {
            relative_path: relative_path_for(kind, name),
            checksum: checksum_bytes(content.as_bytes()),
            content,
        });
    }
    Ok(())
}

fn relative_path_for(kind: BundleFileKind, name: &str) -> String {
    match kind {
        BundleFileKind::Skill => format!("{}/{name}/SKILL.md", kind.dir_name()),
        _ => format!("{}/{name}.{}", kind.dir_name(), kind.extension()),
    }
}

fn validate_bundle_name(kind: BundleFileKind, name: &str) -> Result<()> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.chars().any(char::is_control)
    {
        bail!("invalid bundled {} name: {name:?}", kind.label());
    }

    Ok(())
}

#[cfg(test)]
pub(crate) mod test_helpers {
    pub fn make_test_archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        for &(path, content) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, path, content).unwrap();
        }
        let encoder = builder.into_inner().unwrap();
        encoder.finish().unwrap()
    }

    pub fn bundle_json(version: &str) -> String {
        format!(r#"{{"version":"{version}"}}"#)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::Write;

    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let encoder = GzEncoder::new(Vec::new(), Compression::default());
        let mut builder = tar::Builder::new(encoder);
        for (path, content) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, path, *content).unwrap();
        }
        let encoder = builder.into_inner().unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn canonical_paths_accept_only_agents_and_skills() {
        assert_eq!(
            sanitize_relative_path("agents/reviewer.md"),
            Some("agents/reviewer.md".to_string())
        );
        assert_eq!(
            sanitize_relative_path("skills/commit/SKILL.md"),
            Some("skills/commit/SKILL.md".to_string())
        );
        assert_eq!(sanitize_relative_path("agents/../escape.md"), None);
        assert_eq!(sanitize_relative_path("unknown/entry.md"), None);
    }

    #[test]
    fn archive_extracts_canonical_catalog() {
        let root = tempfile::tempdir().unwrap();
        let bytes = archive(&[
            ("bundle.json", br#"{"version":"v2"}"#),
            ("subagents/agents/reviewer.md", b"# Reviewer"),
            ("skills/commit/SKILL.md", b"# Commit"),
        ]);
        let manifest = extract_bundle_archive(root.path(), &bytes).unwrap();
        assert_eq!(manifest.version, "v2");
        assert_eq!(
            std::fs::read_to_string(root.path().join("agents/reviewer.md")).unwrap(),
            "# Reviewer"
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("skills/commit/SKILL.md")).unwrap(),
            "# Commit"
        );
        assert!(manifest.checksums.contains_key("agents/reviewer.md"));
        assert!(manifest.checksums.contains_key("skills/commit/SKILL.md"));
    }

    #[test]
    fn archive_rejects_traversal_and_oversized_entries() {
        let root = tempfile::tempdir().unwrap();
        let traversal = archive(&[
            ("bundle.json", br#"{"version":"v2"}"#),
            ("subagents/agents/../../escape.md", b"bad"),
        ]);
        assert!(extract_bundle_archive(root.path(), &traversal).is_err());

        let large = vec![b'x'; ARCHIVE_MAX_ENTRY_SIZE as usize + 1];
        let oversized = archive(&[
            ("bundle.json", br#"{"version":"v2"}"#),
            ("subagents/agents/large.md", &large),
        ]);
        assert!(extract_bundle_archive(root.path(), &oversized).is_err());
    }

    #[test]
    fn modified_managed_files_survive_pruning() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("agents/reviewer.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "local edit").unwrap();
        let old = BundleManifest {
            version: "v1".into(),
            checksums: HashMap::from([(
                "agents/reviewer.md".into(),
                checksum_bytes(b"managed value"),
            )]),
        };
        let mut retained = HashMap::new();
        prune_removed_files(root.path(), &old, &mut retained).unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), "local edit");
        assert!(retained.contains_key("agents/reviewer.md"));
    }
}
