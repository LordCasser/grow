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
    use super::is_primary_session_kind;

    #[test]
    fn implicit_trajectory_never_selects_a_subagent_session() {
        assert!(is_primary_session_kind(None));
        assert!(is_primary_session_kind(Some("fork")));
        assert!(is_primary_session_kind(Some("worktree")));
        assert!(!is_primary_session_kind(Some("subagent")));
        assert!(!is_primary_session_kind(Some("subagent_fork")));
        assert!(!is_primary_session_kind(Some("subagent_workflow")));
    }
}
