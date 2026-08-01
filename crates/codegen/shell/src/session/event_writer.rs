//! Event writer — unified collection entry point with pluggable backends.
//!
//! Architecture:
//!   Event.emit()  →  EventWriter  →  [LocalFileSink, future WebhookSink, ...]
//!
//! Each backend implements [`EventSink`]. Adding a new collection method
//! (e.g. webhook) only requires implementing the trait and registering it
//! in [`EventWriter::open`].

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::event_types::Event;

// ── EventSink trait ──────────────────────────────────────────────────

/// A backend that receives serialized event entries.
///
/// Each `write` call receives one JSONL line (no trailing newline).
/// Implementations are responsible for their own error handling —
/// the writer never inspects return values.
pub trait EventSink: Send + Sync + fmt::Debug {
    fn write(&self, entry: &str);
}

// ── LocalFileSink ────────────────────────────────────────────────────

/// Writes events as JSONL to `{dir}/events.jsonl`.
///
/// Controlled by the `EVENT_LOG_DIR` environment variable.
/// Failures (missing dir, permission denied) are silently ignored
/// after the first warning.
#[derive(Debug)]
struct LocalFileSink {
    file: Mutex<Option<File>>,
    error_logged: std::sync::atomic::AtomicBool,
}

impl LocalFileSink {
    fn new(dir: &str) -> Self {
        let path = PathBuf::from(dir).join("events.jsonl");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| {
                tracing::warn!(path = %path.display(), error = %e, "EventWriter: cannot open events.jsonl");
                e
            })
            .ok();
        Self {
            file: Mutex::new(file),
            error_logged: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

impl EventSink for LocalFileSink {
    fn write(&self, entry: &str) {
        let Ok(mut guard) = self.file.lock() else {
            return;
        };
        if let Some(ref mut f) = *guard {
            let mut line = entry.to_owned();
            line.push('\n');
            if let Err(e) = f.write_all(line.as_bytes()) {
                if !self
                    .error_logged
                    .swap(true, std::sync::atomic::Ordering::Relaxed)
                {
                    tracing::warn!(error = %e, "EventWriter: write to events.jsonl failed");
                }
            }
        }
    }
}

// ── EventWriter ──────────────────────────────────────────────────────

/// Central event dispatcher.  Clone, Send, Sync — safe to share across tasks.
#[derive(Clone)]
pub struct EventWriter {
    sinks: Arc<Vec<Box<dyn EventSink>>>,
}

impl EventWriter {
    /// Create a writer, auto-discovering enabled backends from the environment.
    ///
    /// Currently supported:
    ///   - `EVENT_LOG_DIR` → [`LocalFileSink`]
    ///
    /// Returns a no-op writer if no backends are enabled.
    pub fn open() -> Self {
        let mut sinks: Vec<Box<dyn EventSink>> = Vec::new();

        if let Ok(dir) = std::env::var("EVENT_LOG_DIR") {
            if !dir.is_empty() {
                sinks.push(Box::new(LocalFileSink::new(&dir)));
            }
        }

        // Future extension point:
        // if let Ok(url) = std::env::var("EVENT_WEBHOOK_URL") { ... }

        Self {
            sinks: Arc::new(sinks),
        }
    }

    #[cfg(test)]
    pub fn local(directory: &std::path::Path) -> Self {
        Self {
            sinks: Arc::new(vec![Box::new(LocalFileSink::new(
                &directory.to_string_lossy(),
            ))]),
        }
    }

    /// No-op writer that discards all events.
    pub fn noop() -> Self {
        Self {
            sinks: Arc::new(Vec::new()),
        }
    }

    /// Serialize an event to JSONL and dispatch to all registered sinks.
    pub fn emit(&self, event: Event) {
        if self.sinks.is_empty() {
            return;
        }
        let entry = serde_json::to_string(&EventEntry::new(event));
        let Ok(line) = entry else {
            return;
        };
        for sink in self.sinks.iter() {
            sink.write(&line);
        }
    }

    /// Return a clone of this writer (for sharing across tasks).
    pub fn writer(&self) -> Self {
        self.clone()
    }
}

impl fmt::Debug for EventWriter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventWriter")
            .field("sinks", &self.sinks.len())
            .finish()
    }
}

// ── JSONL entry wrapper ──────────────────────────────────────────────

#[derive(serde::Serialize)]
struct EventEntry {
    ts: String,
    #[serde(flatten)]
    event: Event,
}

impl EventEntry {
    fn new(event: Event) -> Self {
        Self {
            ts: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            event,
        }
    }
}
