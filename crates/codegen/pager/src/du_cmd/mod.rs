//! `grow du` — disk usage of the Grow home directory.
//!
//! Pure read-only scan of `~/.grow` (or `$GROW_HOME`): the walk is
//! metadata-only, so no file is ever opened (SQLite databases therefore
//! cannot gain `-wal`/`-shm` sidecars from a scan) and no Grow state is
//! written. Symlinks are billed as their own entry and never followed;
//! directory recursion stops at volume boundaries; on Unix entries bill
//! physical blocks (`st_blocks * 512`), on other platforms logical size.

use std::path::{Path, PathBuf};

use anyhow::Result;

mod human;
mod json;

pub const SCHEMA_VERSION: &str = "1";

#[derive(Clone, Debug, Default, Eq, PartialEq, clap::Args)]
pub struct DuArgs {
    /// Print the usage report as JSON.
    #[arg(long)]
    pub json: bool,
}

/// One top-level entry under the Grow home with its billed subtree size.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DuEntry {
    pub name: String,
    pub bytes: u64,
}

/// Complete scan report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DuReport {
    pub root: PathBuf,
    pub total_bytes: u64,
    pub entries: Vec<DuEntry>,
    pub warnings: Vec<String>,
}

/// Volume identity probe. Takes the entry's `symlink_metadata` (already
/// fetched) and path, returns the volume identity used for the
/// stop-at-mount-boundary check. The path parameter exists so tests can
/// simulate a boundary without mounting a real filesystem.
type DeviceProbe = fn(meta: &std::fs::Metadata, path: &Path) -> u64;

pub fn run(args: DuArgs) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    run_with_writer(args, &mut stdout)
}

fn run_with_writer(args: DuArgs, writer: &mut impl std::io::Write) -> Result<()> {
    let report = scan(&resolve_grow_home()?)?;
    write_report(&report, args.json, writer)?;
    for warning in &report.warnings {
        eprintln!("grow du: {warning}");
    }
    Ok(())
}

/// Resolve the Grow home without creating it: `$GROW_HOME`, else the
/// authoritative `~/.grow` (`config::default_grow_home`). Unlike
/// `config::grow_home()` this never creates the directory and never
/// caches, so `grow du` leaves no state behind.
fn resolve_grow_home() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("GROW_HOME") {
        return Ok(PathBuf::from(dir));
    }
    Ok(config::default_grow_home())
}

/// Scan `root` (followed when the root itself is a symlink; children never
/// are) into a top-level breakdown with one billed size per entry.
pub fn scan(root: &Path) -> Result<DuReport> {
    scan_with(root, device_of)
}

fn scan_with(root: &Path, probe: DeviceProbe) -> Result<DuReport> {
    let root_meta = match std::fs::metadata(root) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(DuReport {
                root: root.to_path_buf(),
                total_bytes: 0,
                entries: Vec::new(),
                warnings: Vec::new(),
            });
        }
        Err(e) => anyhow::bail!("cannot access Grow home {}: {e}", root.display()),
    };
    if !root_meta.is_dir() {
        anyhow::bail!("Grow home {} is not a directory", root.display());
    }
    let root_dev = probe(&root_meta, root);
    let read = std::fs::read_dir(root)
        .map_err(|e| anyhow::anyhow!("cannot read Grow home {}: {e}", root.display()))?;

    let mut warnings = Vec::new();
    let mut entries = Vec::new();
    let mut total = 0u64;
    for child in read {
        match child {
            Ok(entry) => {
                let bytes = subtree_size(&entry.path(), root_dev, probe, &mut warnings);
                total += bytes;
                entries.push(DuEntry {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    bytes,
                });
            }
            Err(e) => warnings.push(format!("cannot read an entry in {}: {e}", root.display())),
        }
    }
    entries.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
    Ok(DuReport {
        root: root.to_path_buf(),
        total_bytes: total,
        entries,
        warnings,
    })
}

/// Billed size of `path`: the entry itself plus, for real directories on the
/// root volume, their contents. Symlinks stop here (never followed); a
/// directory on another volume is billed as itself without descending.
fn subtree_size(path: &Path, root_dev: u64, probe: DeviceProbe, warnings: &mut Vec<String>) -> u64 {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) => {
            warnings.push(format!("cannot stat {}: {e}", path.display()));
            return 0;
        }
    };
    let mut bytes = bill(&meta);
    if meta.file_type().is_dir() && probe(&meta, path) == root_dev {
        match std::fs::read_dir(path) {
            Ok(read) => {
                for child in read {
                    match child {
                        Ok(entry) => {
                            bytes += subtree_size(&entry.path(), root_dev, probe, warnings)
                        }
                        Err(e) => warnings
                            .push(format!("cannot read an entry in {}: {e}", path.display())),
                    }
                }
            }
            Err(e) => warnings.push(format!("cannot read directory {}: {e}", path.display())),
        }
    }
    bytes
}

/// Billed size of one entry: on Unix the allocated physical blocks
/// (`st_blocks`, 512-byte units); elsewhere the logical file size.
fn bill(meta: &std::fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        meta.blocks() * 512
    }
    #[cfg(not(unix))]
    {
        meta.len()
    }
}

/// Volume identity for the mount-boundary check. Unix uses `st_dev`;
/// platforms without a device identity treat the tree as one volume.
fn device_of(meta: &std::fs::Metadata, _path: &Path) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        meta.dev()
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        0
    }
}

fn write_report(
    report: &DuReport,
    json_output: bool,
    writer: &mut impl std::io::Write,
) -> Result<()> {
    if json_output {
        json::write(report, writer)
    } else {
        writer.write_all(human::format(report).as_bytes())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
