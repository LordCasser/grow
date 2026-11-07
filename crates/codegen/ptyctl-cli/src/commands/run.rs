//! `ptyctl run` — spawn a PTY session and start the HTTP server.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use tokio::net::TcpListener;

use ptyctl::pty::PtyConfig;
use ptyctl::server;
use ptyctl::session::{PtySession, SessionConfig};

use crate::registry;

/// Run the `ptyctl run` command.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    command: Vec<String>,
    width: u16,
    height: u16,
    cwd: Option<PathBuf>,
    env_vars: Vec<String>,
    port: u16,
    name: Option<String>,
    force: bool,
    timeout: Option<u64>,
    linger: bool,
    quiet: bool,
) -> Result<()> {
    // Refuse to take over a name whose server is still reachable unless --force; stale entries are replaced.
    if let Some(ref session_name) = name
        && !force
        && let Ok(existing) = registry::lookup_session(session_name)
        && registry::server_alive(existing.port).await
    {
        bail!(
            "session '{session_name}' is already running on port {} (use --force to replace it)",
            existing.port
        );
    }

    // Parse env vars.
    let mut env = HashMap::new();
    for var in &env_vars {
        if let Some((k, v)) = var.split_once('=') {
            env.insert(k.to_string(), v.to_string());
        }
    }

    let cwd_str = cwd
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| ".".into());

    let config = SessionConfig {
        pty: PtyConfig {
            command: command.clone(),
            cols: width,
            rows: height,
            cwd,
            env,
        },
        timeout,
        linger,
    };

    // Retain the actual owner before moving the Session into the service.
    let session = PtySession::start(config).await?;
    let pid = session.status_basic().1;
    let control = session.control_handle();
    let server = server::build_router(session);
    let mut registration = None;

    let result = async {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let listener = TcpListener::bind(addr)
            .await
            .context("failed to bind TCP listener")?;
        let actual_port = listener
            .local_addr()
            .context("failed to get TCP listener address")?
            .port();

        if let Some(session_name) = name {
            let info = registry::SessionInfo {
                port: actual_port,
                pid,
                command: command.clone(),
                cwd: cwd_str,
                started_at: chrono::Utc::now().to_rfc3339(),
            };
            let registered = (session_name, info);
            let to_write = registered.clone();
            // A competing registry transaction may block on the process lock.
            tokio::task::spawn_blocking(move || {
                registry::register_session(&to_write.0, &to_write.1)
            })
            .await
            .context("session registration worker failed")??;
            registration = Some(registered);
        }

        if !quiet {
            eprintln!("Command: {}", command.join(" "));
            if let Some(p) = pid {
                eprintln!("PID: {p}");
            }
            eprintln!("Server listening on port: {actual_port}");
        } else {
            println!("{actual_port}");
        }
        server.serve(listener, &control).await
    }
    .await;

    // Every post-spawn failure keeps its primary cause and settles this owner.
    // SessionControl retains responsibility if settlement itself reports Err.
    let result = match result {
        Ok(()) => Ok(()),
        Err(primary) => match control.stop().await {
            Ok(()) => Err(primary),
            Err(cleanup) => {
                Err(primary.context(format!("owned-child cleanup also failed: {cleanup:#}")))
            }
        },
    };

    let cleanup = match registration {
        Some((name, info)) => tokio::task::spawn_blocking(move || {
            registry::unregister_session_if_current(&name, &info)
        })
        .await
        .context("session cleanup worker failed")
        .and_then(|result| result),
        None => Ok(()),
    };
    match (result, cleanup) {
        (Ok(()), cleanup) => cleanup,
        (Err(primary), Ok(())) => Err(primary),
        (Err(primary), Err(cleanup)) => {
            Err(primary.context(format!("registration cleanup also failed: {cleanup:#}")))
        }
    }
}
