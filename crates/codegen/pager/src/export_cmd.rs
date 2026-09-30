use std::io::Write;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::scrollback::export::render_entries_to_full_markdown;
use crate::transcript_projection::TranscriptProjection;
use shell::session::storage::transcript::{TranscriptSnapshot, capture_tree_at};

const MAX_OUTPUT_BYTES: u64 = 512 * 1024 * 1024;

/// A private, disposable snapshot owned by the external pager request.
pub(crate) fn write_pager_transcript(
    content: &str,
    ansi: bool,
) -> std::io::Result<tempfile::TempPath> {
    write_pager_transcript_with(&std::env::temp_dir(), ansi, |file| {
        file.write_all(content.as_bytes())
    })
}

pub(crate) async fn write_pager_transcript_background(
    content: String,
    ansi: bool,
) -> Result<tempfile::TempPath, String> {
    let directory = std::env::temp_dir();
    write_pager_transcript_background_with(directory, ansi, move |file| {
        file.write_all(content.as_bytes())
    })
    .await
}

async fn write_pager_transcript_background_with(
    directory: PathBuf,
    ansi: bool,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()> + Send + 'static,
) -> Result<tempfile::TempPath, String> {
    tokio::task::spawn_blocking(move || write_pager_transcript_with(&directory, ansi, write))
        .await
        .map_err(|error| format!("Transcript writer failed: {error}"))?
        .map_err(|error| error.to_string())
}

fn write_pager_transcript_with(
    directory: &std::path::Path,
    ansi: bool,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<tempfile::TempPath> {
    let mut file = tempfile::Builder::new()
        .prefix("grow-transcript-")
        .suffix(if ansi { ".ansi" } else { ".md" })
        .tempfile_in(directory)?;
    write(file.as_file_mut())?;
    Ok(file.into_temp_path())
}

#[derive(Debug, clap::Args, Clone)]
pub struct ExportArgs {
    /// Session ID to export
    pub session_id: String,
    /// Output directory (default: ./<session-id>; existing targets are rejected)
    pub output: Option<PathBuf>,
}

pub fn run(args: ExportArgs) -> Result<()> {
    tracing::info!(session_id = %args.session_id, "export_cmd: starting session export");
    let mut snapshot = capture_tree_at(&args.session_id, &shell::util::grow_home::grow_home())?;
    let nodes = snapshot.nodes.clone();
    let canonical = &nodes.first().context("empty transcript tree")?.session_id;
    let target = match args.output {
        Some(output) => {
            let expanded = PathBuf::from(shellexpand::tilde(&output.to_string_lossy()).as_ref());
            if expanded.is_absolute() {
                expanded
            } else {
                std::env::current_dir()?.join(expanded)
            }
        }
        None => std::env::current_dir()?.join(canonical),
    };
    let parent = target.parent().context("output directory has no parent")?;
    std::fs::create_dir_all(parent)?;
    let temp = tempfile::Builder::new()
        .prefix(".grow-export-")
        .tempdir_in(parent)?;
    write_tree(temp.path(), &mut snapshot)?;
    crate::local_drafts::rename_no_replace(temp.path(), &target).with_context(|| {
        format!(
            "Cannot publish {} (choose a different output directory or move the previous export)",
            target.display()
        )
    })?;
    eprintln!(
        "Exported {} agent transcript(s) to {}",
        nodes.len(),
        target.display()
    );
    Ok(())
}

pub(crate) struct TemporaryTranscriptTree {
    pub(crate) directory: tempfile::TempDir,
    pub(crate) canonical_id: String,
    pub(crate) agents: usize,
}

/// Build the same tree in a private temporary directory for trajectory download.
pub(crate) fn temporary_tree(session_id: &str) -> Result<TemporaryTranscriptTree> {
    temporary_tree_at(session_id, &shell::util::grow_home::grow_home())
}

fn temporary_tree_at(session_id: &str, grow_home: &Path) -> Result<TemporaryTranscriptTree> {
    let mut snapshot = capture_tree_at(session_id, grow_home)?;
    let nodes = snapshot.nodes.clone();
    let directory = tempfile::Builder::new()
        .prefix("grow-transcript-")
        .tempdir()?;
    write_tree(directory.path(), &mut snapshot)?;
    Ok(TemporaryTranscriptTree {
        canonical_id: nodes
            .first()
            .context("empty transcript tree")?
            .session_id
            .clone(),
        agents: nodes.len(),
        directory,
    })
}

fn valid_component(id: &str) -> Result<()> {
    if Path::new(id).components().count() != 1
        || !matches!(
            Path::new(id).components().next(),
            Some(Component::Normal(_))
        )
    {
        bail!("invalid session ID path component: {id:?}");
    }
    Ok(())
}

fn write_tree(root: &Path, snapshot: &mut TranscriptSnapshot) -> Result<()> {
    let nodes = snapshot.nodes.clone();
    let mut paths: std::collections::BTreeMap<String, PathBuf> = std::collections::BTreeMap::new();
    let mut output_bytes = 0_u64;
    for node in nodes {
        valid_component(&node.session_id)?;
        let relative = match &node.parent_id {
            None => PathBuf::new(),
            Some(parent) => paths
                .get(parent)
                .context("transcript tree is not in parent-first order")?
                .join("subagents")
                .join(&node.session_id),
        };
        let directory = root.join(&relative);
        std::fs::create_dir_all(&directory)?;
        let session = snapshot.read_session(&node.session_id)?;
        let mut projection = TranscriptProjection::default();
        for event in session.events {
            projection.apply(event);
        }
        let entries =
            (0..projection.scrollback.len()).filter_map(|index| projection.scrollback.entry(index));
        let body = render_entries_to_full_markdown(entries, &node.children);
        let mut markdown = format!("# Grow transcript\n\nSession: `{}`\n\n", node.session_id);
        if let Some(title) = node.title.as_ref().filter(|title| !title.is_empty()) {
            markdown.push_str(&format!("Title: {}\n\n", title.replace('\n', " ")));
        }
        if !node.children.is_empty() {
            markdown.push_str("## Subagents\n\n");
            for child in &node.children {
                valid_component(child)?;
                markdown.push_str(&format!("- [{}](subagents/{child}/transcript.md)\n", child));
            }
            markdown.push('\n');
        }
        if body.is_empty() {
            markdown.push_str("No recorded conversation content.\n");
        } else {
            markdown.push_str(&body);
            markdown.push('\n');
        }
        output_bytes = output_bytes
            .checked_add(markdown.len() as u64)
            .context("output byte count overflow")?;
        if output_bytes > MAX_OUTPUT_BYTES {
            bail!("transcript output exceeds 512 MiB limit");
        }
        write_private_file(&directory.join("transcript.md"), markdown.as_bytes())?;
        paths.insert(node.session_id.clone(), relative);
    }
    sync_tree(root)?;
    Ok(())
}

fn write_private_file(path: &Path, contents: &[u8]) -> Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(contents)?;
    file.sync_all()?;
    Ok(())
}

fn sync_tree(path: &Path) -> Result<()> {
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            sync_tree(&entry.path())?;
        }
    }
    std::fs::File::open(path)?.sync_all()?;
    Ok(())
}

/// Commit a completed transcript without truncating an existing export first.
pub(crate) fn write_export_file(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    write_export_file_with(path, |file| file.write_all(text.as_bytes()))
}

fn write_export_file_with(
    path: &std::path::Path,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind};
    let target = match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => path.canonicalize()?,
        Ok(_) => path.to_path_buf(),
        Err(error) if error.kind() == ErrorKind::NotFound => path.to_path_buf(),
        Err(error) => return Err(error),
    };
    let permissions = match std::fs::metadata(&target) {
        Ok(metadata) => {
            if !metadata.is_file() {
                return Err(Error::new(
                    ErrorKind::InvalidInput,
                    "Export target must be a regular file",
                ));
            }
            let permissions = metadata.permissions();
            if permissions.readonly() {
                return Err(Error::new(
                    ErrorKind::PermissionDenied,
                    "Export target is read-only",
                ));
            }
            Some(permissions)
        }
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let parent = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    if let Some(permissions) = permissions {
        temp.as_file().set_permissions(permissions)?;
    }
    write(temp.as_file_mut())?;
    temp.as_file().sync_all()?;
    temp.persist(&target).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use acp_transport::protocol as acp;
    use chat_state::{SubagentEvent, SubagentSeedEvent, Timeline, TimelineEventKind};
    use sampling_types::ModelImageInputKey;

    fn write_fixture_session(
        home: &Path,
        id: &str,
        timeline: &Timeline,
        parent: Option<&str>,
        message: &str,
    ) {
        let cwd = "/a/different/worktree";
        let directory = home
            .join("sessions")
            .join(shell::util::grow_home::encode_cwd_dirname(cwd))
            .join(id);
        std::fs::create_dir_all(&directory).unwrap();
        let info = shell::session::info::Info {
            id: acp::SessionId::new(id.to_owned()),
            cwd: cwd.into(),
        };
        let mut summary = shell::session::persistence::Summary::new(
            &info,
            shell::agent::models::ModelId::new("model"),
        )
        .unwrap();
        if let Some(parent) = parent {
            summary.parent_session_id = Some(parent.into());
            summary.session_kind = Some("subagent".into());
        }
        std::fs::write(
            directory.join("summary.json"),
            serde_json::to_vec(&summary).unwrap(),
        )
        .unwrap();
        let timeline_json = timeline
            .events()
            .iter()
            .map(|event| serde_json::to_string(event).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(
            directory.join("timeline.jsonl"),
            format!("{timeline_json}\n"),
        )
        .unwrap();
        let notification = acp::SessionNotification::new(
            acp::SessionId::new(id.to_owned()),
            acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
                acp::TextContent::new(message),
            ))),
        );
        let mut update = serde_json::to_value(shell::session::storage::SessionUpdate::Acp(
            Box::new(notification),
        ))
        .unwrap();
        update["timestamp"] = serde_json::json!(1_700_000_000);
        std::fs::write(
            directory.join("updates.jsonl"),
            format!("{}\n", serde_json::to_string(&update).unwrap()),
        )
        .unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn blocked_minimal_snapshot_write_keeps_runtime_responsive() {
        let directory = tempfile::tempdir().unwrap();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let directory_path = directory.path().to_path_buf();
        let write = tokio::spawn(super::write_pager_transcript_background_with(
            directory_path,
            true,
            move |file| {
                let _ = started_tx.send(());
                release_rx
                    .recv_timeout(Duration::from_secs(2))
                    .map_err(std::io::Error::other)?;
                file.write_all(b"blocked ansi snapshot")
            },
        ));

        started_rx.await.unwrap();
        let (input_tx, mut input_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            let _ = input_tx.send("key");
        });
        let input = tokio::time::timeout(Duration::from_millis(250), &mut input_rx)
            .await
            .expect("input event should be processed while snapshot IO is blocked")
            .unwrap();
        assert_eq!(input, "key");

        release_tx.send(()).unwrap();
        let path = write.await.unwrap().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"blocked ansi snapshot");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn pager_transcript_partial_write_cleans_private_file() {
        let directory = tempfile::tempdir().unwrap();
        let error = super::write_pager_transcript_with(directory.path(), false, |file| {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(file.metadata()?.permissions().mode() & 0o777, 0o600);
            }
            std::io::Write::write_all(file, b"partial secret")?;
            Err(std::io::Error::other("injected write failure"))
        })
        .unwrap_err();
        assert_eq!(error.to_string(), "injected write failure");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    use super::*;

    #[test]
    fn tree_export_keeps_parent_and_child_conversations_separate() {
        let home = tempfile::tempdir().unwrap();
        let mut parent = Timeline::default();
        let spawn = chat_state::SubagentSpawnEvent {
            subagent_id: "agent-child".into(),
            child_session_id: "child".into(),
            security_parent_session_id: "root".into(),
            subagent_type: "explore".into(),
            description: "inspect".into(),
            prompt: "inspect".into(),
            context_source: chat_state::SubagentContextSource::New,
            source_ref: None,
            context_normalized: false,
            resumed_from: None,
            parent_prompt_id: None,
            capability_mode: None,
            permission_mode: None,
            effective_permission_mode: None,
            workflow_run_id: None,
            goal_id: None,
            goal_definition_revision: None,
            surface_completion: true,
            child_cwd: "/a/different/worktree".into(),
            worktree_path: None,
            effective_model_id: "model".into(),
            model_transport_key: ModelImageInputKey::new("model", "responses", "test"),
            reasoning_effort: None,
        };
        parent
            .record(TimelineEventKind::Subagent(SubagentEvent::Spawned(
                spawn.clone(),
            )))
            .unwrap();
        let spawn_seq = parent.events().last().unwrap().seq.get();
        let mut child = Timeline::default();
        child
            .record(TimelineEventKind::SubagentSeed(SubagentSeedEvent {
                parent_timeline_id: "root".into(),
                parent_spawn_seq: spawn_seq,
                subagent_id: spawn.subagent_id,
                security_parent_session_id: "root".into(),
                context_source: spawn.context_source,
                source_ref: None,
                normalized: false,
            }))
            .unwrap();
        write_fixture_session(home.path(), "root", &parent, None, "parent-only-answer");
        write_fixture_session(
            home.path(),
            "child",
            &child,
            Some("root"),
            "child-only-answer",
        );

        let mut snapshot = capture_tree_at("root", home.path()).unwrap();
        let output = tempfile::tempdir().unwrap();
        write_tree(output.path(), &mut snapshot).unwrap();
        let parent_doc = std::fs::read_to_string(output.path().join("transcript.md")).unwrap();
        let child_doc =
            std::fs::read_to_string(output.path().join("subagents/child/transcript.md")).unwrap();
        assert!(parent_doc.contains("parent-only-answer"));
        assert!(parent_doc.contains("subagents/child/transcript.md"));
        assert!(!parent_doc.contains("child-only-answer"));
        assert!(child_doc.contains("child-only-answer"));
        assert!(!child_doc.contains("parent-only-answer"));

        let download_tree = temporary_tree_at("root", home.path()).unwrap();
        assert_eq!(download_tree.agents, 2);
        assert_eq!(
            std::fs::read_to_string(download_tree.directory.path().join("transcript.md")).unwrap(),
            parent_doc
        );
        assert_eq!(
            std::fs::read_to_string(
                download_tree
                    .directory
                    .path()
                    .join("subagents/child/transcript.md")
            )
            .unwrap(),
            child_doc
        );
        let archive = crate::trajectory_cmd::archive_tree(download_tree).unwrap();
        let unpacked = tempfile::tempdir().unwrap();
        tar::Archive::new(flate2::read::GzDecoder::new(archive.reopen().unwrap()))
            .unpack(unpacked.path())
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(unpacked.path().join("root/transcript.md")).unwrap(),
            parent_doc
        );
        assert_eq!(
            std::fs::read_to_string(unpacked.path().join("root/subagents/child/transcript.md"))
                .unwrap(),
            child_doc
        );
    }

    #[test]
    fn directory_publication_rejects_every_existing_target_kind() {
        let home = tempfile::tempdir().unwrap();
        let target = home.path().join("transcript-tree");
        for kind in ["file", "empty-dir", "nonempty-dir", "symlink"] {
            match kind {
                "file" => std::fs::write(&target, b"previous").unwrap(),
                "empty-dir" | "nonempty-dir" => {
                    std::fs::create_dir(&target).unwrap();
                    if kind == "nonempty-dir" {
                        std::fs::write(target.join("keep"), b"previous").unwrap();
                    }
                }
                "symlink" => {
                    #[cfg(unix)]
                    std::os::unix::fs::symlink("missing", &target).unwrap();
                    #[cfg(not(unix))]
                    continue;
                }
                _ => unreachable!(),
            }
            let prepared = tempfile::tempdir_in(home.path()).unwrap();
            std::fs::write(prepared.path().join("transcript.md"), b"new").unwrap();
            assert!(crate::local_drafts::rename_no_replace(prepared.path(), &target).is_err());
            assert!(!target.join("transcript.md").exists());
            if kind == "file" {
                assert_eq!(std::fs::read(&target).unwrap(), b"previous");
            }
            if kind == "nonempty-dir" {
                assert_eq!(std::fs::read(target.join("keep")).unwrap(), b"previous");
            }
            if kind == "symlink" {
                assert!(
                    std::fs::symlink_metadata(&target)
                        .unwrap()
                        .file_type()
                        .is_symlink()
                );
            }
            if target.is_dir() {
                std::fs::remove_dir_all(&target).unwrap();
            } else {
                std::fs::remove_file(&target).unwrap();
            }
        }
    }

    #[test]
    fn export_write_failure_keeps_old_content_and_cleans_temp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("export.md");
        std::fs::write(&path, "old export").unwrap();
        let error = write_export_file_with(&path, |file| {
            file.write_all(b"partial new export")?;
            Err(std::io::Error::other("injected write failure"))
        })
        .unwrap_err();
        assert!(error.to_string().contains("injected write failure"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old export");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        write_export_file(&path, "complete new export").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "complete new export"
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        assert!(write_export_file(dir.path(), "no").is_err());
        let nested = dir.path().join("nested/new.md");
        write_export_file(&nested, "new").unwrap();
        assert_eq!(std::fs::read_to_string(nested).unwrap(), "new");
    }

    #[cfg(unix)]
    #[test]
    fn export_preserves_links_and_file_permissions() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("export.md");
        write_export_file(&path, "old").unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        let link = dir.path().join("link.md");
        symlink("export.md", &link).unwrap();
        write_export_file(&link, "new").unwrap();
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o640
        );
        let dangling = dir.path().join("dangling.md");
        symlink("missing.md", &dangling).unwrap();
        assert!(write_export_file(&dangling, "no").is_err());
        assert!(
            std::fs::symlink_metadata(&dangling)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o440)).unwrap();
        assert!(write_export_file(&path, "no").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
    }
}
