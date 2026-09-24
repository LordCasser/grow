//! Bounded file-logging tracing layer for the debug firehose.
//!
//! The default target is ~/.grow/debug/firehose.txt. Explicit log paths retain
//! their selected path and filter. Every line carries process and session
//! attribution; a single bounded worker keeps disk I/O off the tracing path.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use tracing::Subscriber;
use tracing::field::{Field, Visit};
use tracing_subscriber::filter::{EnvFilter, LevelFilter};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

use crate::session_ctx::SESSION_ID_FIELD;
use config::grow_home;

mod stream_firehose;
use stream_firehose::{Firehose, MAX_RECORD_BYTES};

static ACTIVE_FIREHOSE: OnceLock<Arc<Firehose>> = OnceLock::new();

/// Which env var requested a single-file debug log (drives filter and diagnostics).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DebugSource {
    GrowLogFile,
    GrowDebugLog,
}

impl DebugSource {
    fn label(self) -> &'static str {
        match self {
            Self::GrowLogFile => "GROW_LOG_FILE",
            Self::GrowDebugLog => "GROW_DEBUG_LOG",
        }
    }
}

/// Target for the pager's always-on compact ACP update summary line
/// (kind, ids, status, payload sizes).
///
/// Lives here (not in `pager`) so the firehose directives below and
/// the pager's own filter are built from the same constants — a rename can't
/// silently desync them into a no-op directive.
pub const ACP_UPDATE_TARGET: &str = "acp_update";

/// Target for the pager's full ACP update payload dump (plain JSON).
///
/// Off in the pager's release filter; the firehose is the always-available
/// subscriber for full payloads, and it writes to disk, where the volume is
/// safe. See `pager/src/tracing.rs` for the consumer side.
pub const ACP_UPDATE_PAYLOAD_TARGET: &str = "acp_update_payload";

/// Module path of rmcp 2.1's per-reconnect SSE warn (`sse stream error: ...`),
/// which subscribers demote to `error` to drop the flood. Re-check on rmcp bump.
pub const RMCP_SSE_NOISE_TARGET: &str = "rmcp::transport::common::client_side_sse";

// Broad firehose filter for GROW_DEBUG_LOG: capture our
// crates at debug regardless of a narrowing RUST_LOG, with deps at info so they
// don't flood. Curated first-party allowlist: new grow crates default to `info`
// until added here.
const FIREHOSE_BASE_DIRECTIVES: &str = "info,pager=debug,shell=debug,tools=debug,diagnostics=debug,agent=debug,mcp=debug,acp=debug,sampling_log=off";

// Full firehose directives: the curated crate list plus the pager's ACP
// update target (built from the constant above, not a literal).
fn firehose_directives() -> String {
    format!("{FIREHOSE_BASE_DIRECTIVES},{ACP_UPDATE_TARGET}=debug")
}

// The broad firehose filter used by GROW_DEBUG_LOG.
fn firehose_filter() -> EnvFilter {
    EnvFilter::new(firehose_directives())
}

// RUST_LOG-respecting filter for the GROW_LOG_FILE source.
fn default_file_filter() -> EnvFilter {
    EnvFilter::builder()
        .with_default_directive(LevelFilter::DEBUG.into())
        .from_env_lossy()
        .add_directive(
            "sampling_log=off"
                .parse()
                .expect("static directive is valid"),
        )
}

// ── Attributed event formatting ────────────────────────────────────────────

/// Session label captured once from the span and reused by its events.
#[derive(Clone)]
struct SessionId(String);

/// Visits span attributes to pull out the `session_id` field. Production records
/// it via Display. The single record_debug impl captures every field type.
#[derive(Default)]
struct SessionIdVisitor(Option<String>);

impl Visit for SessionIdVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == SESSION_ID_FIELD {
            self.0 = Some(format!("{value:?}"));
        }
    }
}

// Keep attribution printable, one-line and bounded even for an unexpected ID.
fn sanitize_key(id: &str) -> String {
    let safe: String = id
        .chars()
        .take(128)
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    // Dot-only and empty labels are not useful identifiers.
    if safe.is_empty() || safe.bytes().all(|b| b == b'.') {
        return "_".to_owned();
    }
    safe
}

const TRUNCATED_MARKER: &str = " [truncated]";

#[derive(Default)]
struct BoundedLine {
    text: String,
    truncated: bool,
}

impl std::fmt::Write for BoundedLine {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        let allowance = MAX_RECORD_BYTES - TRUNCATED_MARKER.len() - 1;
        for ch in value.chars() {
            let mut encoded = [0; 4];
            let escaped = match ch {
                '\n' => "\\n",
                '\r' => "\\r",
                _ => ch.encode_utf8(&mut encoded),
            };
            if self.text.len().saturating_add(escaped.len()) > allowance {
                self.truncated = true;
                return Err(std::fmt::Error);
            }
            self.text.push_str(escaped);
        }
        Ok(())
    }
}

impl BoundedLine {
    fn finish(mut self) -> Vec<u8> {
        if self.truncated {
            self.text.push_str(TRUNCATED_MARKER);
        }
        self.text.push('\n');
        self.text.into_bytes()
    }
}

struct BoundedEventVisitor<'a>(&'a mut BoundedLine);

impl Visit for BoundedEventVisitor<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write as _;
        if field.name() == "message" {
            let _ = write!(self.0, "{value:?}");
        } else {
            let _ = write!(self.0, " {}={value:?}", field.name());
        }
    }
}

fn format_stream_event(
    event: &tracing::Event<'_>,
    role: &str,
    pid: u32,
    session: Option<&str>,
) -> Vec<u8> {
    use std::fmt::Write as _;
    let mut line = BoundedLine::default();
    let timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
    let _ = write!(
        line,
        "{timestamp} {} {} role={role} pid={pid} sid={}: ",
        event.metadata().level(),
        event.metadata().target(),
        session.unwrap_or("-"),
    );
    event.record(&mut BoundedEventVisitor(&mut line));
    line.finish()
}

struct StreamLayer {
    firehose: Arc<Firehose>,
    role: String,
    pid: u32,
}

impl StreamLayer {
    fn open(path: PathBuf, role: &str) -> std::io::Result<Self> {
        Ok(Self {
            firehose: Arc::new(Firehose::open(path)?),
            role: sanitize_key(role),
            pid: std::process::id(),
        })
    }
}

impl<S> Layer<S> for StreamLayer
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        ctx: Context<'_, S>,
    ) {
        let mut visitor = SessionIdVisitor::default();
        attrs.record(&mut visitor);
        if let Some(sid) = visitor.0
            && let Some(span) = ctx.span(id)
        {
            span.extensions_mut().insert(SessionId(sanitize_key(&sid)));
        }
    }

    fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) {
        let session_key = ctx.event_scope(event).and_then(|scope| {
            scope
                .into_iter()
                .find_map(|span| span.extensions().get::<SessionId>().map(|s| s.0.clone()))
        });
        self.firehose.submit(format_stream_event(
            event,
            &self.role,
            self.pid,
            session_key.as_deref(),
        ));
    }
}

// Legacy per-session log names, retained only to clean old files and orphans.
const LATEST_LINK_NAME: &str = "latest.txt";
const LATEST_TMP_PREFIX: &str = ".latest.";
const LATEST_TMP_SUFFIX: &str = ".tmp";

// ── Install + lifecycle ──────────────────────────────────────────────────────

/// Resolve the requested debug target and install the matching firehose layer on
/// `registry`, then init the subscriber.
///
/// The default stream uses the broad filter; explicit paths select the filter
/// from their environment source. Open failures warn after subscriber init.
pub fn install_firehose<S>(registry: S, role: &str)
where
    S: Subscriber + for<'span> LookupSpan<'span> + Send + Sync + 'static,
{
    use tracing_subscriber::layer::SubscriberExt as _;
    use tracing_subscriber::util::SubscriberInitExt as _;

    match resolve_debug_target() {
        Some(DebugTarget::DefaultStream { dir }) => {
            match StreamLayer::open(dir.join("firehose.txt"), role) {
                Ok(layer) => {
                    let _ = ACTIVE_FIREHOSE.set(layer.firehose.clone());
                    registry.with(layer.with_filter(firehose_filter())).init();
                    sweep_old_logs();
                }
                Err(error) => {
                    registry.init();
                    tracing::warn!(%error, "failed to open debug firehose");
                }
            }
        }
        Some(DebugTarget::SingleFile { path, src }) => {
            let filter = match src {
                DebugSource::GrowLogFile => default_file_filter(),
                DebugSource::GrowDebugLog => firehose_filter(),
            };
            match StreamLayer::open(path.clone(), role) {
                Ok(layer) => {
                    let _ = ACTIVE_FIREHOSE.set(layer.firehose.clone());
                    registry.with(layer.with_filter(filter)).init();
                }
                Err(e) => {
                    registry.init();
                    tracing::warn!("failed to open {} {path:?}: {e}", src.label());
                }
            }
        }
        None => registry.init(),
    }
}

/// Drain accepted unified-log and firehose records with bounded waits.
pub fn flush() {
    let _ = crate::unified_log::flush_pending();
    if let Some(firehose) = ACTIVE_FIREHOSE.get() {
        let _ = firehose.flush();
    }
}

/// Where the firehose should go, if anywhere.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum DebugTarget {
    /// GROW_DEBUG_LOG=1 selects the default stream under this directory.
    DefaultStream { dir: PathBuf },
    /// An explicit file path.
    SingleFile { path: PathBuf, src: DebugSource },
}

/// Resolve the debug target, honoring precedence: explicit GROW_LOG_FILE wins
/// (single file, RUST_LOG filter); else GROW_DEBUG_LOG — a truthy bool selects
/// the default stream in ~/.grow/debug, and an explicit path selects that file.
///
/// Read via `var_os` (not `var`) so a non-UTF-8 path isn't silently dropped.
pub(crate) fn resolve_debug_target() -> Option<DebugTarget> {
    let grow_log_file = std::env::var_os("GROW_LOG_FILE");
    let grow_debug_log = std::env::var_os("GROW_DEBUG_LOG");
    resolve_debug_target_inner(
        grow_log_file.as_deref(),
        grow_debug_log.as_deref(),
        &grow_home().join("debug"),
    )
}

// Empty / whitespace (when valid UTF-8) counts as unset; a non-UTF-8 value is
// never blank.
fn is_blank(v: &OsStr) -> bool {
    v.to_str().is_some_and(|s| s.trim().is_empty())
}

// Build a path from an env value: trim surrounding whitespace when it is valid
// UTF-8, and preserve the raw bytes otherwise (non-UTF-8 paths must survive).
fn os_path(v: &OsStr) -> PathBuf {
    match v.to_str() {
        Some(s) => PathBuf::from(s.trim()),
        None => PathBuf::from(v),
    }
}

// Env-free precedence core so the resolution rules are unit-testable. The role
// and pid are not part of resolution. Takes
// `OsStr` so non-UTF-8 paths round-trip; only the bool-vs-path discrimination
// needs UTF-8 (a non-UTF-8 value can't be a bool keyword, so it's a path).
fn resolve_debug_target_inner(
    grow_log_file: Option<&OsStr>,
    grow_debug_log: Option<&OsStr>,
    debug_dir: &Path,
) -> Option<DebugTarget> {
    if let Some(raw) = grow_log_file
        && !is_blank(raw)
    {
        return Some(DebugTarget::SingleFile {
            path: os_path(raw),
            src: DebugSource::GrowLogFile,
        });
    }
    let raw = grow_debug_log?;
    match raw.to_str().map(str::trim) {
        Some("" | "0" | "false" | "off" | "no") => None,
        Some("1" | "true" | "on" | "yes") => Some(DebugTarget::DefaultStream {
            dir: debug_dir.to_path_buf(),
        }),
        // Any other UTF-8 value, or a non-UTF-8 value (`None`), is an explicit path.
        _ => Some(DebugTarget::SingleFile {
            path: os_path(raw),
            src: DebugSource::GrowDebugLog,
        }),
    }
}

/// Retention window for legacy per-session debug files.
const LOG_RETENTION: std::time::Duration = std::time::Duration::from_secs(7 * 24 * 60 * 60);

/// Prune old per-session files and orphaned latest-link temporaries. The live
/// firehose stream has its own byte ceiling and is never age-pruned.
pub(crate) fn sweep_old_logs() {
    prune_old_logs(&grow_home().join("debug"), LOG_RETENTION);
}

// Best-effort legacy cleanup. Cooperating old writers hold shared locks;
// their files are spared until those writers retire.
fn prune_old_logs(dir: &Path, max_age: std::time::Duration) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let is_log = name.ends_with(".txt") && name != LATEST_LINK_NAME && name != "firehose.txt";
        // Swap temps matching this shape that survive the age gate below are
        // orphans of a crash between `update_latest_symlink`'s create and rename.
        let is_latest_swap_tmp =
            name.starts_with(LATEST_TMP_PREFIX) && name.ends_with(LATEST_TMP_SUFFIX);
        if !is_log && !is_latest_swap_tmp {
            continue;
        }
        // `DirEntry::metadata` does not follow symlinks, so a dangling orphaned
        // temp still yields its own mtime here.
        let Ok(modified) = entry.metadata().and_then(|m| m.modified()) else {
            continue;
        };
        if now.duration_since(modified).is_ok_and(|age| age > max_age) {
            if is_log {
                // Do not follow a symlink or open a known special file as a log.
                if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
                    continue;
                }
                let Ok(file) = std::fs::File::open(&path) else {
                    continue;
                };
                if file.try_lock().is_err() {
                    continue;
                }
                // A writer may have appended since the directory metadata read.
                let Ok(metadata) = file.metadata() else {
                    continue;
                };
                if !metadata.is_file()
                    || !metadata
                        .modified()
                        .ok()
                        .and_then(|modified| now.duration_since(modified).ok())
                        .is_some_and(|age| age > max_age)
                {
                    continue;
                }
                let _ = std::fs::remove_file(&path);
            } else {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::layer::SubscriberExt as _;

    #[test]
    fn resolution_preserves_precedence_and_explicit_paths() {
        let dir = Path::new("/debug");
        assert_eq!(resolve_debug_target_inner(None, None, dir), None);
        for off in ["", "0", "false", "off", "no"] {
            assert_eq!(
                resolve_debug_target_inner(None, Some(OsStr::new(off)), dir),
                None
            );
        }
        for on in ["1", "true", "on", "yes"] {
            assert_eq!(
                resolve_debug_target_inner(None, Some(OsStr::new(on)), dir),
                Some(DebugTarget::DefaultStream { dir: dir.into() })
            );
        }
        assert_eq!(
            resolve_debug_target_inner(
                Some(OsStr::new("/tmp/explicit.log")),
                Some(OsStr::new("1")),
                dir
            ),
            Some(DebugTarget::SingleFile {
                path: "/tmp/explicit.log".into(),
                src: DebugSource::GrowLogFile,
            })
        );
        assert_eq!(
            resolve_debug_target_inner(
                Some(OsStr::new("  ")),
                Some(OsStr::new("/tmp/custom.log")),
                dir
            ),
            Some(DebugTarget::SingleFile {
                path: "/tmp/custom.log".into(),
                src: DebugSource::GrowDebugLog,
            })
        );
    }

    #[test]
    fn stream_layer_attributes_sessions_and_fallback_in_one_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("firehose.txt");
        let layer = StreamLayer::open(path.clone(), "agent").unwrap();
        let firehose = layer.firehose.clone();
        let subscriber = tracing_subscriber::registry().with(layer.with_filter(firehose_filter()));

        tracing::subscriber::with_default(subscriber, || {
            tracing::info_span!(target: "diagnostics::session_ctx", "session", session_id = %"sid-one")
                .in_scope(|| tracing::debug!(target: "shell", "first"));
            tracing::info_span!(target: "diagnostics::session_ctx", "session", session_id = %"sid-two")
                .in_scope(|| tracing::debug!(target: "shell", "second"));
            tracing::info!(target: "shell", "outside");
        });
        assert!(firehose.flush());
        let text = std::fs::read_to_string(path).unwrap();
        assert!(
            text.lines()
                .any(|line| line.contains("sid=sid-one") && line.contains("first"))
        );
        assert!(
            text.lines()
                .any(|line| line.contains("sid=sid-two") && line.contains("second"))
        );
        assert!(
            text.lines()
                .any(|line| line.contains("sid=-") && line.contains("outside"))
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn large_event_is_bounded_as_one_complete_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("firehose.txt");
        let layer = StreamLayer::open(path.clone(), "agent").unwrap();
        let firehose = layer.firehose.clone();
        let subscriber = tracing_subscriber::registry().with(layer);
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(target: "shell", payload = %"x".repeat(MAX_RECORD_BYTES * 2));
        });
        assert!(firehose.flush());
        let bytes = std::fs::read(path).unwrap();
        assert!(bytes.len() <= MAX_RECORD_BYTES);
        assert_eq!(bytes.last(), Some(&b'\n'));
        assert!(std::str::from_utf8(&bytes).unwrap().contains("[truncated]"));
    }

    #[test]
    fn multiline_fields_cannot_split_a_record() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("firehose.txt");
        let layer = StreamLayer::open(path.clone(), "agent").unwrap();
        let firehose = layer.firehose.clone();
        let subscriber = tracing_subscriber::registry().with(layer);
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(target: "shell", payload = %"first\nsecond\rthird");
        });
        assert!(firehose.flush());
        let text = std::fs::read_to_string(path).unwrap();
        assert_eq!(text.lines().count(), 1);
        assert!(text.contains("first\\nsecond\\rthird"));
    }

    #[test]
    fn legacy_sweep_spares_the_active_stream_and_live_legacy_writer() {
        let dir = tempfile::tempdir().unwrap();
        let now = std::time::SystemTime::now();
        let old = now - LOG_RETENTION - std::time::Duration::from_secs(1);
        let current = std::fs::File::create(dir.path().join("firehose.txt")).unwrap();
        current.set_modified(old).unwrap();
        let legacy = std::fs::File::create(dir.path().join("old-session.txt")).unwrap();
        legacy.set_modified(old).unwrap();
        legacy.lock_shared().unwrap();
        prune_old_logs(dir.path(), LOG_RETENTION);
        assert!(dir.path().join("old-session.txt").exists());
        assert!(dir.path().join("firehose.txt").exists());
        legacy.unlock().unwrap();
        prune_old_logs(dir.path(), LOG_RETENTION);
        assert!(!dir.path().join("old-session.txt").exists());
        assert!(dir.path().join("firehose.txt").exists());
    }
}
