use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::acp::meta::NotificationMeta;
use crate::acp::tracker::AcpUpdateTracker;
use crate::scrollback::export::render_blocks_to_markdown;
use crate::scrollback::state::ScrollbackState;

/// A private, disposable snapshot owned by the external pager request.
pub(crate) fn write_pager_transcript(content: &str, ansi: bool) -> std::io::Result<tempfile::TempPath> {
    write_pager_transcript_with(&std::env::temp_dir(), ansi, |file| {
        file.write_all(content.as_bytes())
    })
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
    /// Output file path (default: stdout)
    pub output: Option<PathBuf>,
    /// Copy to clipboard instead of writing to stdout
    #[arg(long, short)]
    pub clipboard: bool,
}

pub fn run(args: ExportArgs) -> Result<()> {
    tracing::info!(session_id = %args.session_id, "export_cmd: starting session export");

    let updates = shell::session::storage::load_updates_for_replay(&args.session_id)?
        .with_context(|| format!("Session '{}' not found.", args.session_id))?;

    let mut tracker = AcpUpdateTracker::new();
    let mut scrollback = ScrollbackState::new();
    let replay_meta = NotificationMeta {
        is_replay: true,
        ..Default::default()
    };

    for update in updates {
        tracker.handle_update(update, &replay_meta, &mut scrollback);
    }

    let blocks: Vec<_> = (0..scrollback.len())
        .filter_map(|i| scrollback.entry(i).map(|e| &e.block))
        .collect();
    let md = render_blocks_to_markdown(blocks);

    if md.is_empty() {
        anyhow::bail!(
            "Session '{}' has no conversation content to export",
            args.session_id
        );
    }

    if let Some(path) = args.output {
        let expanded = PathBuf::from(shellexpand::tilde(&path.to_string_lossy()).as_ref());
        write_export_file(&expanded, &md)
            .with_context(|| format!("Failed to write {}", expanded.display()))?;
        tracing::info!(
            session_id = %args.session_id,
            path = %expanded.display(),
            bytes = md.len(),
            "export_cmd: wrote transcript to file"
        );
        eprintln!("Conversation exported to {}", expanded.display());
    } else if args.clipboard {
        let result = crate::clipboard::copy_text(&md);
        let message = clipboard_export_feedback(&result, &md)?;
        tracing::info!(
            session_id = %args.session_id,
            bytes = md.len(),
            delivery = ?result.delivery,
            "export_cmd: clipboard export completed"
        );
        eprintln!("{message}");
    } else {
        std::io::stdout().write_all(md.as_bytes())?;
        std::io::stdout().write_all(b"\n")?;
    }

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
                return Err(Error::new(ErrorKind::InvalidInput, "Export target must be a regular file"));
            }
            let permissions = metadata.permissions();
            if permissions.readonly() {
                return Err(Error::new(ErrorKind::PermissionDenied, "Export target is read-only"));
            }
            Some(permissions)
        }
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let parent = target.parent().filter(|p| !p.as_os_str().is_empty())
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

fn clipboard_export_feedback(result: &crate::clipboard::CopyResult, text: &str) -> Result<String> {
    if result.delivery.is_failed() {
        anyhow::bail!("Conversation export failed: {}", result.message);
    }
    Ok(format!("{}{}", result.message, crate::clipboard::clipboard_stats_suffix(text)))
}

#[cfg(test)]
mod tests {
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
        }).unwrap_err();
        assert_eq!(error.to_string(), "injected write failure");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    use super::*;
    use crate::clipboard::{ClipboardDelivery, CopyResult};

    #[test]
    fn export_write_failure_keeps_old_content_and_cleans_temp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("export.md");
        std::fs::write(&path, "old export").unwrap();
        let error = write_export_file_with(&path, |file| {
            file.write_all(b"partial new export")?;
            Err(std::io::Error::other("injected write failure"))
        }).unwrap_err();
        assert!(error.to_string().contains("injected write failure"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old export");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        write_export_file(&path, "complete new export").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "complete new export");
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
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        let link = dir.path().join("link.md");
        symlink("export.md", &link).unwrap();
        write_export_file(&link, "new").unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o640);
        let dangling = dir.path().join("dangling.md");
        symlink("missing.md", &dangling).unwrap();
        assert!(write_export_file(&dangling, "no").is_err());
        assert!(std::fs::symlink_metadata(&dangling).unwrap().file_type().is_symlink());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o440)).unwrap();
        assert!(write_export_file(&path, "no").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
    }

    #[test]
    fn clipboard_export_preserves_delivery_outcome() {
        for (delivery, message) in [
            (ClipboardDelivery::Failed, "Copy failed"),
            (ClipboardDelivery::Unverified, "Copy sent; delivery unverified"),
            (ClipboardDelivery::Confirmed, "Copied to tmux buffer"),
        ] {
            let result = CopyResult { message, message_lead: message, ticks: 30, delivery };
            let feedback = clipboard_export_feedback(&result, "你好\nworld");
            if delivery == ClipboardDelivery::Failed {
                assert!(feedback.unwrap_err().to_string().contains("Copy failed"));
            } else {
                assert_eq!(feedback.unwrap(), format!("{message}{}", crate::clipboard::clipboard_stats_suffix("你好\nworld")));
            }
        }
    }
}
