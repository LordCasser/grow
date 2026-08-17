use std::net::SocketAddr;

use anyhow::Context;

#[derive(Debug, clap::Args, Clone)]
pub struct TrajectoryArgs {
    /// Session ID. Defaults to the most recent session for the current directory.
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
                .max_by_key(|summary| summary.last_active_at.unwrap_or(summary.updated_at))
                .map(|summary| summary.info.id.0.to_string())
                .context("no local Grow session exists for the current directory")?
        }
    };
    let no_open = args.no_open;
    let display_session_id = session_id.clone();
    shell::session::trajectory::serve(&session_id, args.bind, move |url| {
        eprintln!("Trajectory for {display_session_id}: {url}");
        if !no_open && !crate::link_opener::open_url(url) {
            eprintln!("Could not open a browser; open the URL manually.");
        }
    })
    .await
}
