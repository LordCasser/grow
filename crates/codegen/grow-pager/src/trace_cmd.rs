use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use grow_shell::agent::config::Config as AgentConfig;

use grow_shell::util::grow_home::grow_home;

#[derive(Debug, clap::Args, Clone)]
pub struct TraceArgs {
    /// Session ID to export/upload
    pub session_id: String,
    /// Save locally only, skip remote upload
    #[arg(long)]
    pub local: bool,
    /// Output path (default: $GROW_HOME/trace-exports/<session-id>.tar.gz)
    #[arg(short, long)]
    pub output: Option<PathBuf>,
    /// Emit machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

#[derive(serde::Serialize)]
struct TraceResult {
    session_id: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    local_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

pub async fn run(args: TraceArgs, agent_config: &AgentConfig) -> Result<()> {
    if args.local {
        return run_export(
            &args.session_id,
            args.output.as_deref(),
            args.json,
            agent_config,
        )
        .await;
    }

    if !agent_config.is_trace_upload_enabled() {
        tracing::warn!(
            session_id = %args.session_id,
            "trace_cmd: trace uploads disabled in config"
        );
        if !args.json {
            eprintln!(
                "Trace uploads disabled. Set [telemetry] trace_upload = true in {}",
                crate::util::display_user_grow_path("config.toml")
            );
            eprintln!("Falling back to local export.");
        }
        return run_export(
            &args.session_id,
            args.output.as_deref(),
            args.json,
            agent_config,
        )
        .await;
    }

    run_upload(
        &args.session_id,
        args.output.as_deref(),
        args.json,
        agent_config,
    )
    .await
}

// ---------------------------------------------------------------------------
// Archive construction
// ---------------------------------------------------------------------------

pub fn build_session_tar(
    session_dir: &Path,
    session_id: &str,
    agent_config: &AgentConfig,
) -> Result<Vec<u8>> {
    use flate2::Compression;
    use flate2::write::GzEncoder;

    tracing::info!(
        session_id = %session_id,
        session_dir = %session_dir.display(),
        "trace_cmd: building session tar.gz archive"
    );

    let mut archive_data = Vec::new();
    let mut file_count: u32 = 0;
    {
        let encoder = GzEncoder::new(&mut archive_data, Compression::default());
        let mut archive = tar::Builder::new(encoder);

        file_count += add_directory_to_tar(&mut archive, session_dir, session_id)?;

        let trace_config = build_trace_config_snapshot(agent_config);
        let config_bytes = serde_json::to_vec_pretty(&trace_config)?;
        append_bytes(
            &mut archive,
            &format!("{session_id}/trace_config.json"),
            &config_bytes,
        );
        file_count += 1;

        let metadata = ExportMetadata {
            session_id: session_id.to_owned(),
            grow_version: env!("VERSION_WITH_COMMIT").to_owned(),
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            exported_at: chrono::Utc::now().to_rfc3339(),
        };
        let meta_bytes = serde_json::to_vec_pretty(&metadata)?;
        append_bytes(
            &mut archive,
            &format!("{session_id}/export_metadata.json"),
            &meta_bytes,
        );
        file_count += 1;

        archive
            .into_inner()
            .and_then(|encoder| encoder.finish())
            .context("Failed to finalize tar.gz archive")?;
    }

    tracing::info!(
        session_id = %session_id,
        file_count,
        archive_bytes = archive_data.len(),
        "trace_cmd: archive built"
    );

    Ok(archive_data)
}

#[derive(serde::Serialize)]
struct ExportMetadata {
    session_id: String,
    grow_version: String,
    os: String,
    arch: String,
    exported_at: String,
}

/// No URLs, paths, or bucket names -- only booleans and config source indicators.
#[derive(serde::Serialize)]
struct TraceConfigSnapshot {
    trace_upload_enabled: bool,
    telemetry_trace_upload: Option<bool>,
    custom_upload_url: bool,
    bucket_url_source: String,
    direct_upload_configured: bool,
    has_bucket_configured: bool,
    has_region_configured: bool,
    has_custom_endpoint: bool,
    has_credentials_file: bool,
    has_inline_credentials: bool,
    has_deployment_key: bool,
}

fn build_trace_config_snapshot(agent_config: &AgentConfig) -> TraceConfigSnapshot {
    TraceConfigSnapshot {
        trace_upload_enabled: agent_config.is_trace_upload_enabled(),
        telemetry_trace_upload: agent_config.telemetry.trace_upload,
        custom_upload_url: agent_config.endpoints.trace_upload_url.is_some(),
        bucket_url_source: match agent_config.endpoints.resolve_trace_bucket_url() {
            Some(resolved) => format!("{}", resolved.source),
            None => "unconfigured".to_owned(),
        },
        direct_upload_configured: agent_config
            .endpoints
            .resolve_direct_upload_method()
            .is_some(),
        has_bucket_configured: agent_config.endpoints.trace_upload_bucket.is_some(),
        has_region_configured: agent_config.endpoints.trace_upload_region.is_some(),
        has_custom_endpoint: agent_config.endpoints.trace_upload_endpoint_url.is_some(),
        has_credentials_file: agent_config
            .endpoints
            .trace_upload_credentials_file
            .is_some(),
        has_inline_credentials: agent_config.endpoints.trace_upload_credentials.is_some(),
        has_deployment_key: agent_config.endpoints.deployment_key.is_some(),
    }
}

fn append_bytes<W: std::io::Write>(archive: &mut tar::Builder<W>, path: &str, data: &[u8]) {
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    set_mtime(&mut header);
    if let Err(e) = archive.append_data(&mut header, path, data) {
        tracing::warn!(error = %e, "trace_cmd: failed to add file to archive");
        eprintln!("  Warning: failed to add {path}: {e}");
    }
}

fn set_mtime(header: &mut tar::Header) {
    header.set_mtime(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    );
}

/// Returns the number of files added.
fn add_directory_to_tar<W: std::io::Write>(
    archive: &mut tar::Builder<W>,
    dir: &Path,
    prefix: &str,
) -> Result<u32> {
    let entries =
        std::fs::read_dir(dir).with_context(|| format!("Failed to read {}", dir.display()))?;

    let mut count: u32 = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let archive_path = format!("{prefix}/{name_str}");

        if path.is_dir() {
            count += add_directory_to_tar(archive, &path, &archive_path)?;
        } else if path.is_file() {
            match std::fs::read(&path) {
                Ok(data) => {
                    append_bytes(archive, &archive_path, &data);
                    count += 1;
                }
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "trace_cmd: failed to read file for archive"
                    );
                    eprintln!("  Warning: failed to read {}: {}", path.display(), e);
                }
            }
        }
    }

    Ok(count)
}

// ---------------------------------------------------------------------------
// Upload method diagnostics — removed (cloud upload disabled)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Local export
// ---------------------------------------------------------------------------

pub(crate) fn find_session_dir(session_id: &str) -> Result<PathBuf> {
    grow_shell::session::persistence::find_session_dir_by_id(session_id).with_context(|| {
        format!(
            "Session '{session_id}' not found under {}",
            crate::util::display_user_grow_path("sessions")
        )
    })
}

pub fn trace_exports_dir() -> PathBuf {
    grow_home().join("trace-exports")
}

/// Creates parent directory if needed.
pub fn save_local_bundle(
    archive: &[u8],
    session_id: &str,
    output: Option<&Path>,
) -> Result<PathBuf> {
    let output_path = match output {
        Some(p) => p.to_path_buf(),
        None => trace_exports_dir().join(format!("{session_id}.tar.gz")),
    };

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    std::fs::write(&output_path, archive)
        .with_context(|| format!("Failed to write {}", output_path.display()))?;

    tracing::info!(
        session_id = %session_id,
        path = %output_path.display(),
        size_bytes = archive.len(),
        "trace_cmd: local bundle saved"
    );

    Ok(output_path)
}

async fn run_export(
    session_id: &str,
    output: Option<&Path>,
    json: bool,
    agent_config: &AgentConfig,
) -> Result<()> {
    let session_dir = find_session_dir(session_id)?;
    if !json {
        eprintln!("Found session at: {}", session_dir.display());
        eprintln!("Building session trace archive...");
    }

    let archive = build_session_tar(&session_dir, session_id, agent_config)?;
    let output_path = save_local_bundle(&archive, session_id, output)?;

    if json {
        let result = TraceResult {
            session_id: session_id.to_owned(),
            status: "exported",
            url: None,
            local_path: Some(output_path.display().to_string()),
            error: None,
        };
        println!("{}", serde_json::to_string(&result)?);
    } else {
        let size_kb = archive.len() / 1024;
        eprintln!("Session trace exported ({size_kb} KB):");
        eprintln!("  {}", output_path.display());
        println!("{}", output_path.display());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Upload with fallback
// ---------------------------------------------------------------------------

/// Prints upload URL to stdout on success; saves local bundle and returns Err on failure.
async fn run_upload(
    session_id: &str,
    output: Option<&Path>,
    json: bool,
    agent_config: &AgentConfig,
) -> Result<()> {
    tracing::warn!(
        session_id = %session_id,
        "trace_cmd: cloud upload has been removed, falling back to local export"
    );
    if !json {
        eprintln!("Cloud upload has been removed. Falling back to local export.");
    }
    run_export(session_id, output, json, agent_config).await
}

pub struct UploadAttempt<'a> {
    pub session_id: &'a str,
    pub archive: &'a [u8],
    pub output: Option<&'a Path>,
    pub method_desc: &'a str,
    pub object_path: &'a str,
    pub bucket_url: &'a str,
    pub json: bool,
}

impl UploadAttempt<'_> {
    /// Saves local bundle + debug log, prints diagnostics.
    pub fn handle_failure(&self, error: &anyhow::Error) -> anyhow::Error {
        let export_dir = trace_exports_dir();
        std::fs::create_dir_all(&export_dir).ok();

        let export_path = save_local_bundle(self.archive, self.session_id, self.output)
            .unwrap_or_else(|write_err| {
                eprintln!("Failed to save local bundle: {write_err}");
                export_dir.join(format!("{}.tar.gz", self.session_id))
            });

        let log_path = self.write_debug_log(error, &export_dir);

        if self.json {
            let result = TraceResult {
                session_id: self.session_id.to_owned(),
                status: "failed",
                url: None,
                local_path: Some(export_path.display().to_string()),
                error: Some(format!("{error}")),
            };
            println!("{}", serde_json::to_string(&result).unwrap_or_default());
        } else {
            eprintln!();
            eprintln!("Trace upload failed: {error}");
            eprintln!("  Bundle: {}", export_path.display());
            eprintln!("  Log:    {}", log_path.display());
            eprintln!("  Retry:  grow trace {}", self.session_id);
            println!("{}", export_path.display());
        }

        anyhow::anyhow!("Trace upload failed for session {}", self.session_id)
    }

    fn write_debug_log(&self, error: &anyhow::Error, output_dir: &Path) -> PathBuf {
        use std::fmt::Write;

        let log_path = output_dir.join(format!("{}.upload.log", self.session_id));
        let mut log = String::new();
        let _ = writeln!(log, "Trace upload debug log");
        let _ = writeln!(log, "======================");
        let _ = writeln!(log, "Timestamp:    {}", chrono::Utc::now().to_rfc3339());
        let _ = writeln!(log, "Grow version: {}", env!("VERSION_WITH_COMMIT"));
        let _ = writeln!(
            log,
            "OS:           {} {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        let _ = writeln!(log, "Session ID:   {}", self.session_id);
        let _ = writeln!(log, "Archive size: {} bytes", self.archive.len());
        let _ = writeln!(log, "Object path:  {}", self.object_path);
        let _ = writeln!(log);
        let _ = writeln!(log, "Upload configuration:");
        let _ = writeln!(log, "{}", self.method_desc);
        let _ = writeln!(log);
        let _ = writeln!(log, "Error:\n  {error}");
        let _ = writeln!(log);
        let _ = writeln!(log, "Full error chain:\n  {error:?}");

        if let Err(e) = std::fs::write(&log_path, &log) {
            eprintln!("  Warning: failed to write debug log: {e}");
        }
        log_path
    }
}

// ---------------------------------------------------------------------------
// Upload with retries
// ---------------------------------------------------------------------------

async fn upload_with_retries(
    _config: &(),
    _object_path: &str,
    _archive: &[u8],
) -> anyhow::Result<String> {
    tracing::warn!("Trace upload not available in this build");
    eprintln!("Trace upload not available in this build");
    Ok("upload-not-available".to_string())
}

// ---------------------------------------------------------------------------
// Upload method resolution
// ---------------------------------------------------------------------------

/// Cloud upload has been removed. Always returns None.
pub async fn resolve_upload_method(_agent_config: &AgentConfig) -> Option<()> {
    tracing::warn!("trace_cmd: cloud upload has been removed");
    None
}
