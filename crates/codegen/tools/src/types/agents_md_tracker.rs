//! Bounded discovery of project instructions after explicit tool path access.
//!
//! A scan is an immutable snapshot. Only appending its contents to a terminal
//! tool result confirms delivery; failed/cancelled scans leave no negative cache.

use std::collections::HashSet;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cap_fs_ext::{DirExt as _, FollowSymlinks, OpenOptionsFollowExt as _};
use cap_std::fs::{Dir, OpenOptions};
use ignore::gitignore::{Gitignore, GitignoreBuilder};

pub(crate) const AGENT_FILENAME: &str = "AGENTS.md";
pub(crate) const RULES_DIR: &str = ".grow/rules";

// Bound the entire operation, not each of its filesystem calls independently.
pub(crate) const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_WALK_DEPTH: usize = 32;
const MAX_RULE_ENTRIES: usize = 128;
const MAX_RULE_FILES: usize = 64;
const MAX_FILE_BYTES: u64 = 64 * 1024;
const MAX_DISCOVERY_BYTES: usize = 256 * 1024;

/// Session state. Cloned under Resources, scanned outside it, merged on delivery.
#[derive(Debug, Clone, Default)]
pub struct AgentsMdTracker {
    initial_discovery: HashSet<PathBuf>,
    reminded: HashSet<PathBuf>,
    // Pin the directory capability so symlink replacement cannot escape the repo.
    scope: Option<(PathBuf, Arc<Dir>)>,
    gitignore: Option<Gitignore>,
    generation: Arc<()>,
    // Owned by the blocking worker, including after its async caller times out.
    // A stuck mount may disable discovery, but cannot accumulate workers.
    scan_gate: Arc<tokio::sync::Mutex<()>>,
}

/// Successfully read files, still unacknowledged and safe to discard on cancel.
pub(crate) struct AgentsMdDiscovery {
    generation: Arc<()>,
    files: Vec<(PathBuf, String)>,
    display_paths: Option<(PathBuf, PathBuf)>,
}

impl AgentsMdTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Only seed files that were actually included in the initial prompt.
    /// No directory is negatively cached: a failed startup read must be retried.
    pub async fn seed(
        &mut self,
        initial_paths: Vec<PathBuf>,
        git_root: Option<PathBuf>,
        gitignore: Option<Gitignore>,
    ) {
        let prepared = tokio::time::timeout(
            DISCOVERY_TIMEOUT,
            tokio::task::spawn_blocking(move || {
                let root = dunce::canonicalize(git_root?).ok()?;
                let dir = Dir::open_ambient_dir(&root, cap_std::ambient_authority()).ok()?;
                let initial = initial_paths
                    .into_iter()
                    .filter_map(|p| dunce::canonicalize(p).ok())
                    .collect();
                Some((root, Arc::new(dir), initial))
            }),
        )
        .await;
        self.scope = None;
        self.initial_discovery.clear();
        self.on_compaction();
        self.gitignore = gitignore;
        if let Ok(Ok(Some((root, dir, initial)))) = prepared {
            self.scope = Some((root, dir));
            self.initial_discovery = initial;
        }
    }

    /// Scan only known paths, never shell command text or search-result strings.
    /// A timeout/cancellation does not alter the live tracker.
    pub(crate) async fn check_paths(
        &self,
        targets: Vec<PathBuf>,
        deny_read_globs: Vec<String>,
        display_paths: Option<(PathBuf, PathBuf)>,
    ) -> AgentsMdDiscovery {
        let snapshot = self.clone();
        let deadline = Instant::now() + DISCOVERY_TIMEOUT;
        let files = tokio::time::timeout(DISCOVERY_TIMEOUT, async move {
            let permit = snapshot.scan_gate.clone().lock_owned().await;
            tokio::task::spawn_blocking(move || {
                let _permit = permit;
                let display_paths = display_paths.and_then(|(cwd, display)| {
                    dunce::canonicalize(cwd).ok().map(|cwd| (cwd, display))
                });
                (
                    snapshot.scan(targets, deny_read_globs, deadline),
                    display_paths,
                )
            })
            .await
        })
        .await;
        let (files, display_paths) = match files {
            Ok(Ok(result)) => result,
            _ => {
                tracing::debug!("Project instruction discovery timed out or failed");
                (Vec::new(), None)
            }
        };
        AgentsMdDiscovery {
            generation: self.generation.clone(),
            files,
            display_paths,
        }
    }

    fn scan(
        &self,
        targets: Vec<PathBuf>,
        deny_read_globs: Vec<String>,
        deadline: Instant,
    ) -> Vec<(PathBuf, String)> {
        let Some((root, directory)) = &self.scope else {
            return vec![];
        };
        // Literal-separator file globs, applied to every
        // automatic read (including .gitignore), irrespective of RespectGitignore.
        let mut denies = globset::GlobSetBuilder::new();
        for pattern in deny_read_globs {
            let Ok(glob) = globset::GlobBuilder::new(&pattern)
                .literal_separator(true)
                .build()
            else {
                return vec![];
            };
            denies.add(glob);
        }
        let Ok(denies) = denies.build() else {
            return vec![];
        };
        let mut files = Vec::new();
        let mut seen = HashSet::new();
        let mut bytes = 0;
        for target in targets.into_iter().take(2) {
            if Instant::now() >= deadline {
                break;
            }
            // Fail closed on canonicalization failure; never use a raw '..' path
            // or a symlink outside the seeded repository as a discovery boundary.
            let Ok(target) = dunce::canonicalize(&target) else {
                continue;
            };
            if !target.starts_with(root) {
                continue;
            }
            let Ok(relative) = target.strip_prefix(root) else {
                continue;
            };
            let relative = if relative.as_os_str().is_empty() {
                Path::new(".")
            } else {
                relative
            };
            let Ok(metadata) = directory.metadata(relative) else {
                continue;
            };
            let start = if metadata.is_dir() {
                target.as_path()
            } else {
                let Some(parent) = target.parent() else {
                    continue;
                };
                parent
            };
            let mut chain: Vec<_> = start
                .ancestors()
                .take(MAX_WALK_DEPTH)
                .take_while(|p| p.starts_with(root))
                .collect();
            // Without all ancestors we cannot safely evaluate nested ignores.
            if chain.last().copied() != Some(root.as_path()) {
                continue;
            }
            chain.reverse();

            let mut ignores = Vec::new();
            let mut excluded = false;
            for dir in &chain {
                if Instant::now() >= deadline {
                    return files;
                }
                if denied(&denies, root, dir) || self.is_ignored(&ignores, dir, true) {
                    excluded = true;
                    break;
                }
                let path = dir.join(".gitignore");
                if denied(&denies, root, &path) {
                    excluded = true;
                    break;
                }
                match read_regular(directory, root, &path) {
                    Ok(text) => {
                        let mut builder = GitignoreBuilder::new(dir);
                        for line in text.lines() {
                            if builder.add_line(Some(path.clone()), line).is_err() {
                                excluded = true;
                                break;
                            }
                        }
                        let Ok(filter) = builder.build() else {
                            excluded = true;
                            break;
                        };
                        ignores.push((dir.to_path_buf(), filter));
                    }
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    // An unreadable ignore file must not expose excluded rules.
                    Err(_) => {
                        excluded = true;
                        break;
                    }
                }
            }
            if excluded
                || denied(&denies, root, &target)
                || self.is_ignored(&ignores, &target, metadata.is_dir())
            {
                continue;
            }
            for dir in chain {
                if Instant::now() >= deadline || files.len() >= MAX_RULE_FILES {
                    return files;
                }
                let mut candidates = vec![dir.join(AGENT_FILENAME)];
                let rules = dir.join(RULES_DIR);
                if !denied(&denies, root, &rules)
                    && !self.is_ignored(&ignores, &rules, true)
                    && let Ok(entries) = directory.read_dir(rules.strip_prefix(root).unwrap())
                {
                    let mut paths = Vec::new();
                    let mut overflow = false;
                    for (index, entry) in entries.enumerate() {
                        if Instant::now() >= deadline {
                            return files;
                        }
                        if index >= MAX_RULE_ENTRIES {
                            overflow = true;
                            break;
                        }
                        let Ok(entry) = entry else {
                            overflow = true;
                            break;
                        };
                        let path = rules.join(entry.file_name());
                        if path
                            .extension()
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
                        {
                            paths.push(path);
                        }
                    }
                    // Do not depend on an arbitrary OS directory enumeration prefix.
                    if !overflow {
                        paths.sort();
                        candidates.extend(paths);
                    }
                }
                for path in candidates {
                    if Instant::now() >= deadline || files.len() >= MAX_RULE_FILES {
                        return files;
                    }
                    if denied(&denies, root, &path) || self.is_ignored(&ignores, &path, false) {
                        continue;
                    }
                    // The walk starts at a canonical directory. read_regular
                    // rejects aliases in every remaining component, so this is
                    // also the canonical identity without opening special files
                    // through cap-std's canonicalize operation.
                    let canonical = path.clone();
                    if self.initial_discovery.contains(&canonical)
                        || self.reminded.contains(&canonical)
                        || seen.contains(&canonical)
                        || denied(&denies, root, &canonical)
                        || self.is_ignored(&ignores, &canonical, false)
                    {
                        continue;
                    }
                    let Ok(content) = read_regular(directory, root, &path) else {
                        continue;
                    };
                    if bytes + content.len() > MAX_DISCOVERY_BYTES {
                        continue;
                    }
                    bytes += content.len();
                    let content = if path.file_name().is_some_and(|n| n != AGENT_FILENAME) {
                        crate::implementations::skills::skill::extract_skill_body(&content)
                    } else {
                        content
                    };
                    seen.insert(canonical.clone());
                    files.push((canonical, content));
                }
            }
        }
        files
    }

    fn is_ignored(&self, nested: &[(PathBuf, Gitignore)], path: &Path, is_dir: bool) -> bool {
        let Some((root, _)) = &self.scope else {
            return true;
        };
        for (dir, filter) in nested.iter().rev() {
            if path.starts_with(dir) {
                let result = filter.matched_path_or_any_parents(path, is_dir);
                if !result.is_none() {
                    return result.is_ignore();
                }
            }
        }
        self.gitignore
            .as_ref()
            .is_some_and(|gi| ignored(gi, root, path, is_dir))
    }

    /// Called under the resource lock at the final, non-awaiting result boundary.
    /// Recheck live state to deduplicate concurrent scans; confirm only after the
    /// fully read and escaped text has been appended to the model-facing result.
    /// This acknowledges result construction, not durable session persistence;
    /// the tool (including an edit/write) has already executed.
    pub(crate) fn append_to_prompt(
        &mut self,
        discovery: AgentsMdDiscovery,
        prompt: &mut String,
        tag: &str,
    ) {
        if !Arc::ptr_eq(&self.generation, &discovery.generation) {
            return;
        }
        let files: Vec<_> = discovery
            .files
            .into_iter()
            .filter(|(path, _)| {
                !self.initial_discovery.contains(path) && !self.reminded.contains(path)
            })
            .collect();
        if files.is_empty() {
            return;
        }
        let mut text = String::from(
            "Project instructions for the accessed paths (parent directories first; deeper instructions take precedence within their scope):\n",
        );
        for (path, content) in &files {
            let display_path = match &discovery.display_paths {
                Some((cwd, display)) => path
                    .strip_prefix(cwd)
                    .map(|p| display.join(p))
                    .unwrap_or_else(|_| path.clone()),
                _ => path.clone(),
            };
            text.push_str(&format!(
                "\n## From: {}\n{}\n",
                crate::reminders::neutralize_reminder_tags(&display_path.to_string_lossy()),
                crate::reminders::neutralize_reminder_tags(content)
            ));
        }
        *prompt = crate::reminders::format_with_reminders(std::mem::take(prompt), vec![text], tag);
        self.reminded
            .extend(files.into_iter().map(|(path, _)| path));
    }

    /// Startup instructions stay in the system prompt across compaction.
    /// Old in-flight scans cannot repopulate delivery state in the new cycle.
    pub fn on_compaction(&mut self) {
        self.reminded.clear();
        self.generation = Arc::new(());
    }

    pub fn reminded_paths(&self) -> &HashSet<PathBuf> {
        &self.reminded
    }
}

fn ignored(filter: &Gitignore, root: &Path, path: &Path, is_dir: bool) -> bool {
    path.strip_prefix(root).ok().is_some_and(|relative| {
        filter
            .matched_path_or_any_parents(relative, is_dir)
            .is_ignore()
    })
}

fn denied(filter: &globset::GlobSet, root: &Path, path: &Path) -> bool {
    path.ancestors()
        .take_while(|p| p.starts_with(root))
        .any(|p| {
            filter.is_match(p)
                || p.strip_prefix(root)
                    .is_ok_and(|relative| filter.is_match(relative))
        })
}

/// Never follow a rule-file symlink or read special files. The directory
/// capability also prevents ancestor symlinks from escaping the pinned root.
fn read_regular(directory: &Dir, root: &Path, path: &Path) -> io::Result<String> {
    let relative = path.strip_prefix(root).map_err(io::Error::other)?;
    let mut parent = directory.try_clone()?;
    if let Some(ancestors) = relative.parent() {
        for component in ancestors.components() {
            parent = parent.open_dir_nofollow(component.as_os_str())?;
        }
    }
    let name = relative
        .file_name()
        .ok_or_else(|| io::Error::other("missing rule filename"))?;
    let metadata = parent.symlink_metadata(name)?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
        return Err(io::Error::other(
            "instruction file is not a bounded regular file",
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        // A regular file replaced by a FIFO between stat and open must not
        // strand a blocking-pool thread after the caller's timeout.
        use cap_fs_ext::OpenOptionsExt as _;
        options.custom_flags(nix::libc::O_NONBLOCK);
    }
    let file = parent.open_with(name, &options)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
        return Err(io::Error::other(
            "instruction file is not a bounded regular file",
        ));
    }
    let mut text = String::new();
    file.take(MAX_FILE_BYTES + 1).read_to_string(&mut text)?;
    if text.len() as u64 > MAX_FILE_BYTES {
        return Err(io::Error::other(
            "instruction file grew beyond the byte limit",
        ));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignore::gitignore::GitignoreBuilder;
    use std::fs;

    #[tokio::test]
    async fn busy_scan_is_shared_across_snapshots_and_remains_retryable() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("AGENTS.md"), "rule").unwrap();
        let mut tracker = AgentsMdTracker::new();
        tracker
            .seed(vec![], Some(tmp.path().to_path_buf()), None)
            .await;
        // Model the permit retained by a timed-out blocking worker.
        let permit = tracker.scan_gate.clone().lock_owned().await;
        let snapshot = tracker.clone();
        tracker.on_compaction();
        tokio::time::pause();
        for state in [&tracker, &snapshot] {
            let started = tokio::time::Instant::now();
            let scan = state
                .check_paths(vec![tmp.path().to_path_buf()], vec![], None)
                .await;
            assert!(started.elapsed() >= DISCOVERY_TIMEOUT);
            assert!(scan.files.is_empty());
            assert!(state.reminded_paths().is_empty());
        }
        tokio::time::resume();
        drop(permit);
        assert_eq!(access_and_deliver(&mut tracker, tmp.path()).await.len(), 1);
    }

    #[tokio::test]
    async fn discovery_requires_delivery_and_compaction_rejects_stale_scans() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("AGENTS.md"), "rule").unwrap();
        let mut tracker = AgentsMdTracker::new();
        tracker
            .seed(vec![], Some(tmp.path().to_path_buf()), None)
            .await;
        let scan = tracker
            .check_paths(vec![tmp.path().to_path_buf()], vec![], None)
            .await;
        assert_eq!(scan.files.len(), 1);
        assert!(tracker.reminded_paths().is_empty());
        tracker.on_compaction();
        let mut prompt = String::from("tool result");
        tracker.append_to_prompt(scan, &mut prompt, "system-reminder");
        assert_eq!(prompt, "tool result");
        assert!(tracker.reminded_paths().is_empty());
        let retry = tracker
            .check_paths(vec![tmp.path().to_path_buf()], vec![], None)
            .await;
        tracker.append_to_prompt(retry, &mut prompt, "system-reminder");
        assert!(prompt.contains("rule"));
        assert_eq!(tracker.reminded_paths().len(), 1);
    }

    #[tokio::test]
    async fn discarded_discovery_and_reseed_leave_rules_retryable() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("AGENTS.md"), "rule").unwrap();
        let mut tracker = AgentsMdTracker::new();
        tracker
            .seed(vec![], Some(tmp.path().to_path_buf()), None)
            .await;
        let discarded = tracker
            .check_paths(vec![tmp.path().to_path_buf()], vec![], None)
            .await;
        assert_eq!(discarded.files.len(), 1);
        drop(discarded);
        assert!(tracker.reminded_paths().is_empty());
        let old = tracker
            .check_paths(vec![tmp.path().to_path_buf()], vec![], None)
            .await;
        tracker
            .seed(vec![], Some(tmp.path().to_path_buf()), None)
            .await;
        let mut prompt = String::new();
        tracker.append_to_prompt(old, &mut prompt, "system-reminder");
        assert!(prompt.is_empty());
        assert_eq!(access_and_deliver(&mut tracker, tmp.path()).await.len(), 1);
    }

    #[tokio::test]
    async fn scan_bounds_deadline_rule_count_and_total_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let rules = tmp.path().join(RULES_DIR);
        fs::create_dir_all(&rules).unwrap();
        for i in 0..MAX_RULE_FILES + 1 {
            fs::write(rules.join(format!("{i:03}.md")), "rule").unwrap();
        }
        let mut tracker = AgentsMdTracker::new();
        tracker
            .seed(vec![], Some(tmp.path().to_path_buf()), None)
            .await;
        assert!(
            tracker
                .scan(vec![tmp.path().to_path_buf()], vec![], Instant::now())
                .is_empty()
        );
        let scan = tracker
            .check_paths(vec![tmp.path().to_path_buf()], vec![], None)
            .await;
        assert_eq!(scan.files.len(), MAX_RULE_FILES);
        for i in 0..MAX_RULE_FILES + 1 {
            fs::write(
                rules.join(format!("{i:03}.md")),
                vec![b'x'; MAX_FILE_BYTES as usize],
            )
            .unwrap();
        }
        let scan = tracker
            .check_paths(vec![tmp.path().to_path_buf()], vec![], None)
            .await;
        assert_eq!(
            scan.files.iter().map(|(_, c)| c.len()).sum::<usize>(),
            MAX_DISCOVERY_BYTES
        );
        for i in MAX_RULE_FILES + 1..MAX_RULE_ENTRIES + 1 {
            fs::write(rules.join(format!("{i:03}.md")), "rule").unwrap();
        }
        let scan = tracker
            .check_paths(vec![tmp.path().to_path_buf()], vec![], None)
            .await;
        assert!(
            scan.files.is_empty(),
            "oversized directories must not use an arbitrary enumeration prefix"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn special_files_are_rejected_without_blocking_or_acknowledging() {
        use nix::sys::stat::Mode;
        use nix::unistd::mkfifo;
        let tmp = tempfile::tempdir().unwrap();
        mkfifo(&tmp.path().join("AGENTS.md"), Mode::S_IRUSR | Mode::S_IWUSR).unwrap();
        let mut tracker = AgentsMdTracker::new();
        tracker
            .seed(vec![], Some(tmp.path().to_path_buf()), None)
            .await;
        let scan = tokio::time::timeout(
            Duration::from_millis(500),
            tracker.check_paths(vec![tmp.path().to_path_buf()], vec![], None),
        )
        .await
        .unwrap();
        assert!(scan.files.is_empty());
        assert!(tracker.reminded_paths().is_empty());
    }

    // These legacy path cases now model a successful tool access followed by
    // delivery. Scanning alone no longer acknowledges instructions.
    async fn access_and_deliver(tracker: &mut AgentsMdTracker, target: &Path) -> Vec<PathBuf> {
        if !target.exists() && target.parent().is_some_and(Path::is_dir) {
            fs::write(target, "accessed file").unwrap();
        }
        let scan = tracker
            .check_paths(vec![target.to_path_buf()], vec![], None)
            .await;
        let paths = scan.files.iter().map(|(path, _)| path.clone()).collect();
        let mut prompt = String::new();
        tracker.append_to_prompt(scan, &mut prompt, "system-reminder");
        paths
    }

    /// Create a Gitignore from patterns for testing.
    fn build_test_gitignore(root: &Path, patterns: &[&str]) -> Gitignore {
        let mut builder = GitignoreBuilder::new(root);
        for pattern in patterns {
            builder.add_line(None, pattern).unwrap();
        }
        builder.build().unwrap()
    }

    #[tokio::test]
    async fn tracker_seed_marks_initial_as_known() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join("AGENTS.md"), "initial").unwrap();

        let mut tracker = AgentsMdTracker::new();
        tracker
            .seed(vec![root.join("AGENTS.md")], Some(root.to_path_buf()), None)
            .await;

        let results = access_and_deliver(&mut tracker, &root.join("foo.rs")).await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn check_path_finds_new_agents_md() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let sub = root.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("AGENTS.md"), "sub instructions").unwrap();

        let mut tracker = AgentsMdTracker::new();
        tracker.seed(vec![], Some(root.to_path_buf()), None).await;

        let results = access_and_deliver(&mut tracker, &sub.join("foo.rs")).await;
        assert_eq!(results.len(), 1);
        assert!(results[0].ends_with("AGENTS.md"));
    }

    #[tokio::test]
    async fn check_path_ignores_noncanonical_instruction_filename() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let sub = root.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("INSTRUCTIONS.md"), "foreign instructions").unwrap();

        let mut tracker = AgentsMdTracker::new();
        tracker.seed(vec![], Some(root.to_path_buf()), None).await;

        let results = access_and_deliver(&mut tracker, &sub.join("foo.rs")).await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn check_path_skips_already_reminded() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let sub = root.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("AGENTS.md"), "instructions").unwrap();

        let mut tracker = AgentsMdTracker::new();
        tracker.seed(vec![], Some(root.to_path_buf()), None).await;

        let first = access_and_deliver(&mut tracker, &sub.join("foo.rs")).await;
        assert_eq!(first.len(), 1);

        let second = access_and_deliver(&mut tracker, &sub.join("bar.rs")).await;
        assert!(second.is_empty());
    }

    #[tokio::test]
    async fn check_path_skips_initial_discovery() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join("AGENTS.md"), "root instructions").unwrap();

        let mut tracker = AgentsMdTracker::new();
        tracker
            .seed(vec![root.join("AGENTS.md")], Some(root.to_path_buf()), None)
            .await;

        let results = access_and_deliver(&mut tracker, &root.join("foo.rs")).await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn check_path_stops_at_git_root() {
        let tmp = tempfile::tempdir().unwrap();
        let outer = tmp.path();
        let repo = outer.join("repo");
        let sub = repo.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(outer.join("AGENTS.md"), "above root").unwrap();

        let mut tracker = AgentsMdTracker::new();
        tracker.seed(vec![], Some(repo.clone()), None).await;

        let results = access_and_deliver(&mut tracker, &sub.join("foo.rs")).await;
        assert!(
            results.is_empty(),
            "Should not find AGENTS.md above git root"
        );
    }

    #[tokio::test]
    async fn check_path_returns_empty_when_no_git_root() {
        let mut tracker = AgentsMdTracker::new();
        tracker.seed(vec![], None, None).await;

        let results = access_and_deliver(&mut tracker, Path::new("/any/path/file.rs")).await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn check_path_retries_previously_empty_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let deep = root.join("a").join("b").join("c");
        fs::create_dir_all(&deep).unwrap();

        let mut tracker = AgentsMdTracker::new();
        tracker.seed(vec![], Some(root.to_path_buf()), None).await;

        let results = access_and_deliver(&mut tracker, &deep.join("file.rs")).await;
        assert!(results.is_empty());

        fs::write(root.join("a").join("b").join("AGENTS.md"), "late").unwrap();

        let results = access_and_deliver(&mut tracker, &deep.join("other.rs")).await;
        assert!(
            results.len() == 1,
            "A previously empty directory must not hide newly created rules"
        );
    }

    #[tokio::test]
    async fn check_path_idempotent_on_second_call() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let sub = root.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("AGENTS.md"), "content").unwrap();

        let mut tracker = AgentsMdTracker::new();
        tracker.seed(vec![], Some(root.to_path_buf()), None).await;

        let first = access_and_deliver(&mut tracker, &sub.join("file.rs")).await;
        assert_eq!(first.len(), 1);
        let reminded_count_after_first = tracker.reminded.len();

        let second = access_and_deliver(&mut tracker, &sub.join("file2.rs")).await;
        assert!(second.is_empty());
        assert_eq!(tracker.reminded.len(), reminded_count_after_first);
    }

    #[tokio::test]
    async fn check_path_stops_at_max_walk_depth() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        fs::write(root.join("AGENTS.md"), "root").unwrap();

        let mut deep = root.to_path_buf();
        for i in 0..MAX_WALK_DEPTH + 1 {
            deep = deep.join(format!("d{}", i));
        }
        fs::create_dir_all(&deep).unwrap();

        let mut tracker = AgentsMdTracker::new();
        tracker.seed(vec![], Some(root.to_path_buf()), None).await;

        let results = access_and_deliver(&mut tracker, &deep.join("file.rs")).await;
        assert!(results.is_empty(), "Incomplete ancestry must fail closed");
    }

    #[tokio::test]
    async fn check_path_skips_gitignored_agents_md() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let root = dunce::canonicalize(root).unwrap();
        let build_dir = root.join("build");
        fs::create_dir_all(&build_dir).unwrap();
        fs::write(build_dir.join("AGENTS.md"), "build instructions").unwrap();

        let gi = build_test_gitignore(&root, &["build/"]);
        let mut tracker = AgentsMdTracker::new();
        tracker
            .seed(vec![], Some(root.to_path_buf()), Some(gi))
            .await;

        let results = access_and_deliver(&mut tracker, &build_dir.join("output.o")).await;
        assert!(results.is_empty(), "Gitignored AGENTS.md should be skipped");
    }

    #[tokio::test]
    async fn check_path_does_not_skip_non_gitignored() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let root = dunce::canonicalize(root).unwrap();
        let src_dir = root.join("src");
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(src_dir.join("AGENTS.md"), "src instructions").unwrap();

        let gi = build_test_gitignore(&root, &["build/"]);
        let mut tracker = AgentsMdTracker::new();
        tracker
            .seed(vec![], Some(root.to_path_buf()), Some(gi))
            .await;

        let results = access_and_deliver(&mut tracker, &src_dir.join("main.rs")).await;
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn on_compaction_clears_reminded_for_refire() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let sub = root.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("AGENTS.md"), "content").unwrap();

        let mut tracker = AgentsMdTracker::new();
        tracker.seed(vec![], Some(root.to_path_buf()), None).await;

        let first = access_and_deliver(&mut tracker, &sub.join("file.rs")).await;
        assert_eq!(first.len(), 1);
        assert_eq!(tracker.reminded.len(), 1);

        let second = access_and_deliver(&mut tracker, &sub.join("file2.rs")).await;
        assert!(second.is_empty());

        tracker.on_compaction();
        assert!(tracker.reminded.is_empty());

        let refire = access_and_deliver(&mut tracker, &sub.join("file3.rs")).await;
        assert_eq!(
            refire.len(),
            1,
            "AGENTS.md reminder must re-fire after compaction"
        );
        assert_eq!(tracker.reminded.len(), 1);
    }

    #[tokio::test]
    async fn on_compaction_does_not_clear_initial_discovery() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join("AGENTS.md"), "root").unwrap();

        let mut tracker = AgentsMdTracker::new();
        tracker
            .seed(vec![root.join("AGENTS.md")], Some(root.to_path_buf()), None)
            .await;

        tracker.on_compaction();

        let results = access_and_deliver(&mut tracker, &root.join("foo.rs")).await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn reminded_paths_returns_current_set() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let sub1 = root.join("sub1");
        let sub2 = root.join("sub2");
        fs::create_dir_all(&sub1).unwrap();
        fs::create_dir_all(&sub2).unwrap();
        fs::write(sub1.join("AGENTS.md"), "sub1").unwrap();
        fs::write(sub2.join("AGENTS.md"), "sub2").unwrap();

        let mut tracker = AgentsMdTracker::new();
        tracker.seed(vec![], Some(root.to_path_buf()), None).await;

        access_and_deliver(&mut tracker, &sub1.join("file.rs")).await;
        access_and_deliver(&mut tracker, &sub2.join("file.rs")).await;
        assert_eq!(tracker.reminded_paths().len(), 2);

        tracker.on_compaction();
        assert!(tracker.reminded_paths().is_empty());
    }

    #[tokio::test]
    async fn check_path_handles_dot_dot_segments() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let a = root.join("a");
        let b = a.join("b");
        fs::create_dir_all(&b).unwrap();
        fs::write(a.join("AGENTS.md"), "a instructions").unwrap();

        let mut tracker = AgentsMdTracker::new();
        tracker.seed(vec![], Some(root.to_path_buf()), None).await;

        let dotdot_path = b.join("..").join("b").join("file.rs");
        let results = access_and_deliver(&mut tracker, &dotdot_path).await;
        assert_eq!(results.len(), 1);
        assert!(!results[0].to_str().unwrap().contains(".."));
    }

    #[tokio::test]
    async fn check_path_does_not_fall_back_to_unresolved_path() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("AGENTS.md"), "root").unwrap();
        let mut tracker = AgentsMdTracker::new();
        tracker
            .seed(vec![], Some(tmp.path().to_path_buf()), None)
            .await;
        let scan = tracker
            .check_paths(vec![tmp.path().join("missing/../file.rs")], vec![], None)
            .await;
        assert!(scan.files.is_empty());
        assert!(tracker.reminded_paths().is_empty());
    }

    #[tokio::test]
    async fn check_path_with_directory_target() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let sub = root.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("AGENTS.md"), "content").unwrap();

        let mut tracker = AgentsMdTracker::new();
        tracker.seed(vec![], Some(root.to_path_buf()), None).await;

        let results = access_and_deliver(&mut tracker, &sub).await;
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn check_path_discovers_parent_agents_md() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let frontend = root.join("frontend");
        let apps = frontend.join("apps");
        fs::create_dir_all(&apps).unwrap();
        fs::write(frontend.join("AGENTS.md"), "frontend rules").unwrap();

        let mut tracker = AgentsMdTracker::new();
        tracker.seed(vec![], Some(root.to_path_buf()), None).await;

        let results = access_and_deliver(&mut tracker, &apps.join("foo.ts")).await;
        assert_eq!(results.len(), 1);
        assert!(results[0].to_str().unwrap().contains("frontend"));
    }

    // ── Rules directory discovery tests ─────────────────────────────

    #[tokio::test]
    async fn check_path_discovers_grow_rules_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let sub = root.join("sub");
        fs::create_dir_all(&sub).unwrap();

        let rules_dir = sub.join(".grow").join("rules");
        fs::create_dir_all(&rules_dir).unwrap();
        fs::write(rules_dir.join("style.md"), "# Style rules").unwrap();

        let mut tracker = AgentsMdTracker::new();
        tracker.seed(vec![], Some(root.to_path_buf()), None).await;

        let results = access_and_deliver(&mut tracker, &sub.join("foo.rs")).await;
        assert!(
            results
                .iter()
                .any(|p| p.to_str().unwrap().contains("style.md")),
            "Should discover .grow/rules/style.md, got: {:?}",
            results
        );
    }

    #[tokio::test]
    async fn check_path_rules_not_reminded_twice() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let sub = root.join("sub");
        fs::create_dir_all(&sub).unwrap();

        let rules_dir = sub.join(".grow").join("rules");
        fs::create_dir_all(&rules_dir).unwrap();
        fs::write(rules_dir.join("style.md"), "# Style").unwrap();

        let mut tracker = AgentsMdTracker::new();
        tracker.seed(vec![], Some(root.to_path_buf()), None).await;

        let first = access_and_deliver(&mut tracker, &sub.join("foo.rs")).await;
        assert_eq!(first.len(), 1);

        let second = access_and_deliver(&mut tracker, &sub.join("bar.rs")).await;
        assert!(second.is_empty(), "Rules should not be reminded twice");
    }

    #[tokio::test]
    async fn check_path_ignores_foreign_instruction_subdirectory() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let sub = root.join("sub");
        fs::create_dir_all(&sub).unwrap();

        let foreign_dir = sub.join(".foreign");
        fs::create_dir_all(&foreign_dir).unwrap();
        fs::write(
            foreign_dir.join("INSTRUCTIONS.md"),
            "# Project instructions",
        )
        .unwrap();

        let mut tracker = AgentsMdTracker::new();
        tracker.seed(vec![], Some(root.to_path_buf()), None).await;

        let results = access_and_deliver(&mut tracker, &sub.join("foo.rs")).await;
        assert!(results.is_empty());
    }
}
