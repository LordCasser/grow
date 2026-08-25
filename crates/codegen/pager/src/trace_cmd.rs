use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use shell::util::grow_home::grow_home;

#[derive(Debug, clap::Args, Clone)]
pub struct TraceArgs {
    /// Session ID to save
    pub session_id: String,
    /// Output path (default: $GROW_HOME/traces/<session-id>.tar.gz)
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
    path: String,
}

pub async fn run(args: TraceArgs) -> Result<()> {
    run_save(&args.session_id, args.output.as_deref(), args.json).await
}

// ---------------------------------------------------------------------------
// Archive construction
// ---------------------------------------------------------------------------

const MAX_TRACE_ARCHIVE_BYTES: usize = 128 * 1024 * 1024;

pub fn build_session_tar(
    snapshot: shell::session::storage::SessionTraceSnapshot,
) -> Result<Vec<u8>> {
    use flate2::Compression;
    use flate2::write::GzEncoder;

    let session_id = snapshot.session_id;

    tracing::info!(
        session_id = %session_id,
        "trace_cmd: building session tar.gz archive"
    );

    let mut archive_data = Vec::new();
    let mut file_count: u32 = 0;
    {
        let encoder = GzEncoder::new(&mut archive_data, Compression::default());
        let mut archive = tar::Builder::new(encoder);

        for file in snapshot.files {
            let archive_path = Path::new(&session_id).join(file.relative_path);
            append_bytes(&mut archive, &archive_path, &file.bytes)?;
            file_count = file_count.saturating_add(1);
        }

        let metadata = ExportMetadata {
            session_id: session_id.clone(),
            version: env!("VERSION_WITH_COMMIT").to_owned(),
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            exported_at: chrono::Utc::now().to_rfc3339(),
        };
        let meta_bytes = serde_json::to_vec_pretty(&metadata)?;
        append_bytes(
            &mut archive,
            &Path::new(&session_id).join("export_metadata.json"),
            &meta_bytes,
        )?;
        file_count += 1;

        archive
            .into_inner()
            .and_then(|encoder| encoder.finish())
            .context("Failed to finalize tar.gz archive")?;
    }

    if archive_data.len() > MAX_TRACE_ARCHIVE_BYTES {
        anyhow::bail!("Session trace archive exceeds the output byte limit");
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
    version: String,
    os: String,
    arch: String,
    exported_at: String,
}

fn append_bytes<W: std::io::Write>(
    archive: &mut tar::Builder<W>,
    path: &Path,
    data: &[u8],
) -> Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    set_mtime(&mut header);
    archive
        .append_data(&mut header, path, data)
        .with_context(|| format!("Failed to add {} to trace archive", path.display()))?;
    Ok(())
}

fn set_mtime(header: &mut tar::Header) {
    header.set_mtime(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    );
}

// ---------------------------------------------------------------------------
// Local export
// ---------------------------------------------------------------------------

pub fn traces_dir() -> PathBuf {
    grow_home().join("traces")
}

/// Creates parent directory if needed.
pub fn save_local_bundle(
    archive: &[u8],
    session_id: &str,
    output: Option<&Path>,
) -> Result<PathBuf> {
    let output_path = match output {
        Some(p) => p.to_path_buf(),
        None => traces_dir().join(format!("{session_id}.tar.gz")),
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

async fn run_save(session_id: &str, output: Option<&Path>, json: bool) -> Result<()> {
    let snapshot = shell::session::storage::load_session_trace(session_id)?
        .with_context(|| format!("Session '{session_id}' not found"))?;
    let canonical_session_id = snapshot.session_id.clone();
    if !json {
        eprintln!("Found session: {canonical_session_id}");
        eprintln!("Building local session trace archive...");
    }

    let archive = build_session_tar(snapshot)?;
    let output_path = save_local_bundle(&archive, &canonical_session_id, output)?;

    if json {
        let result = TraceResult {
            session_id: canonical_session_id,
            status: "saved",
            path: output_path.display().to_string(),
        };
        println!("{}", serde_json::to_string(&result)?);
    } else {
        let size_kb = archive.len() / 1024;
        eprintln!("Session trace saved ({size_kb} KB):");
        eprintln!("  {}", output_path.display());
        println!("{}", output_path.display());
    }
    Ok(())
}
