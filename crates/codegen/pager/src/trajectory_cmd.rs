use std::net::SocketAddr;

use anyhow::Context;

#[derive(Debug, clap::Args, Clone)]
pub struct TrajectoryArgs {
    /// Session ID. Defaults to the most recent primary session for the current directory.
    pub session_id: Option<String>,
    /// Local address for the debug server; port 0 chooses an available port.
    #[arg(long, default_value = "127.0.0.1:0")]
    pub bind: SocketAddr,
    /// Print the URL without opening the system browser.
    #[arg(long)]
    pub no_open: bool,
}

pub async fn run(args: TrajectoryArgs) -> anyhow::Result<()> {
    let session_id = match args.session_id {
        Some(id) => id,
        None => {
            let cwd = std::env::current_dir().context("failed to resolve current directory")?;
            let cwd = cwd.to_string_lossy();
            shell::session::persistence::list_summaries(Some(&cwd))
                .await?
                .into_iter()
                .filter(|summary| is_primary_session_kind(summary.session_kind.as_deref()))
                .max_by_key(|summary| summary.last_active_at.unwrap_or(summary.updated_at))
                .map(|summary| summary.info.id.0.to_string())
                .context("no primary Grow session exists for the current directory")?
        }
    };
    let no_open = args.no_open;
    shell::session::trajectory::serve(
        &session_id,
        args.bind,
        move |canonical_id, url| {
            eprintln!("Trajectory for {canonical_id}: {url}");
            if !no_open && !crate::link_opener::open_url(url) {
                eprintln!("Could not open a browser; open the URL manually.");
            }
        },
        build_transcript_archive,
    )
    .await
}

fn build_transcript_archive(session_id: &str) -> anyhow::Result<tempfile::NamedTempFile> {
    let tree = crate::export_cmd::temporary_tree(session_id)?;
    archive_tree(tree)
}

pub(crate) fn archive_tree(
    tree: crate::export_cmd::TemporaryTranscriptTree,
) -> anyhow::Result<tempfile::NamedTempFile> {
    let mut archive = tempfile::Builder::new()
        .prefix("grow-transcript-")
        .suffix(".tar.gz")
        .tempfile()?;
    let encoder =
        flate2::write::GzEncoder::new(archive.as_file_mut(), flate2::Compression::default());
    let mut tar = tar::Builder::new(encoder);
    tar.append_dir_all(&tree.canonical_id, tree.directory.path())?;
    let encoder = tar.into_inner()?;
    encoder.finish()?;
    archive.as_file().sync_all()?;
    tracing::info!(
        agents = tree.agents,
        "trajectory transcript archive prepared"
    );
    Ok(archive)
}

fn is_primary_session_kind(kind: Option<&str>) -> bool {
    !kind.is_some_and(|kind| kind.starts_with("subagent"))
}

#[cfg(test)]
mod tests {
    use super::{build_transcript_archive, is_primary_session_kind};
    use std::path::Path;

    use acp_transport::protocol as acp;
    use chat_state::{SubagentEvent, SubagentSeedEvent, Timeline, TimelineEventKind};
    use sampling_types::ModelImageInputKey;

    fn write_fixture_session(
        grow_home: &Path,
        id: &str,
        timeline: &Timeline,
        parent: Option<&str>,
        message: &str,
    ) {
        let cwd = "/a/different/worktree";
        let directory = grow_home
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
        let notification = acp::SessionNotification::new(
            acp::SessionId::new(id.to_owned()),
            acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
                acp::TextContent::new(message),
            ))),
        );
        shell::session::testkit::write_timeline_history_fixture(
            &directory,
            timeline.events(),
            &[shell::session::storage::SessionUpdate::Acp(Box::new(
                notification,
            ))],
        )
        .unwrap();
    }

    fn write_parent_child_fixture(grow_home: &Path) {
        let mut parent = Timeline::default();
        let spawn = chat_state::SubagentSpawnEvent {
            subagent_id: "agent-child".into(),
            child_session_id: "child-session".into(),
            origin_tool_call_id: None,
            security_parent_session_id: "root-session".into(),
            parent_step_id: None,
            subagent_type: "explore".into(),
            description: "inspect".into(),
            prompt: "inspect".into(),
            execution_recipe: serde_json::json!({}),
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
                Box::new(spawn.clone()),
            )))
            .unwrap();
        let spawn_seq = parent.events().last().unwrap().seq.get();
        let mut child = Timeline::default();
        child
            .record(TimelineEventKind::SubagentSeed(SubagentSeedEvent {
                parent_timeline_id: "root-session".into(),
                parent_spawn_seq: spawn_seq,
                subagent_id: spawn.subagent_id,
                security_parent_session_id: "root-session".into(),
                context_source: spawn.context_source,
                source_ref: None,
                normalized: false,
            }))
            .unwrap();
        write_fixture_session(
            grow_home,
            "root-session",
            &parent,
            None,
            "parent-only-answer",
        );
        write_fixture_session(
            grow_home,
            "child-session",
            &child,
            Some("root-session"),
            "child-only-answer",
        );
    }

    #[test]
    fn implicit_trajectory_never_selects_a_subagent_session() {
        assert!(is_primary_session_kind(None));
        assert!(is_primary_session_kind(Some("fork")));
        assert!(is_primary_session_kind(Some("worktree")));
        assert!(!is_primary_session_kind(Some("subagent")));
        assert!(!is_primary_session_kind(Some("subagent_fork")));
        assert!(!is_primary_session_kind(Some("subagent_workflow")));
    }

    #[tokio::test]
    async fn transcript_download_http_uses_production_export_archive() {
        let test_name = concat!(
            module_path!(),
            "::transcript_download_http_uses_production_export_archive"
        );
        if crate::test_util::run_with_isolated_grow_home(test_name) {
            return;
        }

        let grow_home = shell::util::grow_home::grow_home();
        write_parent_child_fixture(&grow_home);

        let (url_tx, url_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            shell::session::trajectory::serve(
                "root-session",
                "127.0.0.1:0".parse().unwrap(),
                move |_session_id, url| {
                    let _ = url_tx.send(url.to_owned());
                },
                build_transcript_archive,
            )
            .await
            .unwrap();
        });
        let url = tokio::time::timeout(std::time::Duration::from_secs(5), url_rx)
            .await
            .expect("trajectory server should become ready")
            .unwrap();
        diagnostics::tls::install_ring_provider_once();
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap();
        let response = client
            .get(format!("{url}api/transcript"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(
            response.headers()[reqwest::header::CONTENT_TYPE],
            "application/gzip"
        );
        let archive_bytes = response.bytes().await.unwrap();
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());

        let unpacked = tempfile::tempdir().unwrap();
        tar::Archive::new(flate2::read::GzDecoder::new(archive_bytes.as_ref()))
            .unpack(unpacked.path())
            .unwrap();
        let parent =
            std::fs::read_to_string(unpacked.path().join("root-session/transcript.md")).unwrap();
        let child = std::fs::read_to_string(
            unpacked
                .path()
                .join("root-session/subagents/child-session/transcript.md"),
        )
        .unwrap();
        assert!(parent.contains("parent-only-answer"));
        assert!(parent.contains("subagents/child-session/transcript.md"));
        assert!(!parent.contains("child-only-answer"));
        assert!(child.contains("child-only-answer"));
        assert!(!child.contains("parent-only-answer"));
    }
}
