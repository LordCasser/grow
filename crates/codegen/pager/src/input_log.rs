//! Input flight recorder — rolling buffer of recent key events.
//!
//! Esc followed by unmodified d within 500ms dumps to
//! `~/.grow/logs/input-debug-<timestamp>-<random>.json` when the input handler reaches that chord.
//! Can be better utilized once input bugs are fully resolved.
use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
use serde::Serialize;
use std::collections::VecDeque;
use std::time::Instant;
/// Keep a complete private diagnostic snapshot without replacing earlier dumps.
pub(crate) fn write_input_dump(
    directory: &std::path::Path,
    now: chrono::DateTime<chrono::Utc>,
    json: &str,
) -> std::io::Result<std::path::PathBuf> {
    write_input_dump_with(directory, now, |file| {
        std::io::Write::write_all(file, json.as_bytes())
    })
}

fn write_input_dump_with(
    directory: &std::path::Path,
    now: chrono::DateTime<chrono::Utc>,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<std::path::PathBuf> {
    std::fs::create_dir_all(directory)?;
    let mut file = tempfile::Builder::new()
        .prefix(&format!("input-debug-{}-", now.format("%Y%m%d-%H%M%S")))
        .suffix(".json")
        .tempfile_in(directory)?;
    write(file.as_file_mut())?;
    file.as_file().sync_all()?;
    file.keep().map(|(_, path)| path).map_err(|error| error.error)
}

/// Default ring buffer capacity (~10 seconds of fast typing).
const DEFAULT_CAPACITY: usize = 200;
/// Snapshot of textarea state captured by `PromptWidget::handle_key`.
///
/// Stored on `PromptWidget` after each key; read by `AgentView` when
/// building `RawInputEntry`. `None` fields mean the key was handled before
/// reaching the textarea (e.g., file search, slash command).
#[derive(Clone, Debug, Default)]
pub struct LastInputDelta {
    pub cursor_before: Option<usize>,
    pub cursor_after: Option<usize>,
    pub text_len_before: Option<usize>,
    pub text_len_after: Option<usize>,
    pub had_selection_before: Option<bool>,
    pub had_selection_after: Option<bool>,
    pub textarea_changed: Option<bool>,
}
/// Copy-friendly snapshot of the active pane. No heap allocation.
#[derive(Clone, Copy, Debug)]
pub enum ActivePaneSnapshot {
    Prompt,
    Scrollback,
    Todo,
    Queue,
    Tasks,
    Catalog,
    Other,
}
/// Copy-friendly snapshot of the input outcome. No heap allocation.
#[derive(Clone, Copy, Debug)]
pub enum OutcomeSnapshot {
    Changed,
    Unchanged,
    Action,
}
/// Raw entry stored in the ring buffer. No heap allocations —
/// formatting happens only during [`InputRingBuffer::snapshot_entries`].
#[derive(Clone, Debug)]
pub struct RawInputEntry {
    pub wall_ts: u64,
    pub key_code: KeyCode,
    pub key_modifiers: KeyModifiers,
    pub key_kind: KeyEventKind,
    pub active_pane: ActivePaneSnapshot,
    pub outcome: OutcomeSnapshot,
    pub cursor_before: Option<usize>,
    pub cursor_after: Option<usize>,
    pub text_len_before: Option<usize>,
    pub text_len_after: Option<usize>,
    pub sel_before: Option<bool>,
    pub sel_after: Option<bool>,
    pub textarea_changed: Option<bool>,
}
/// Serializable output record, constructed from [`RawInputEntry`] during dump.
#[derive(Clone, Debug, Serialize)]
pub struct InputRecord {
    /// Milliseconds relative to the first entry in the buffer (0-based).
    pub ts_ms: u64,
    /// Wall-clock unix millis for log correlation.
    pub wall_ts: u64,
    /// Sanitized key category (see [`sanitize_key_code`]).
    /// Printable chars are logged as `"Char"` without the character value
    /// to prevent reconstructing typed text from the dump.
    pub key: String,
    /// Modifier flags, e.g. `"NONE"`, `"CONTROL"`.
    pub mods: String,
    /// Key event kind: `"Press"`, `"Repeat"`, `"Release"`.
    pub kind: String,
    /// Which pane was active: `"Prompt"`, `"Scrollback"`, etc.
    pub pane: String,
    /// Input outcome: `"Changed"`, `"Unchanged"`, `"Action"`.
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor_before: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor_after: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_len_before: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_len_after: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sel_before: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sel_after: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub textarea_changed: Option<bool>,
}
/// Fixed-capacity ring buffer of raw input events.
pub struct InputRingBuffer {
    entries: VecDeque<(Instant, RawInputEntry)>,
    capacity: usize,
}
impl InputRingBuffer {
    pub fn new() -> Self {
        Self {
            entries: VecDeque::with_capacity(DEFAULT_CAPACITY),
            capacity: DEFAULT_CAPACITY,
        }
    }
    /// Push a new record, dropping the oldest if at capacity.
    pub fn push(&mut self, entry: RawInputEntry) {
        let now = Instant::now();
        if self.entries.len() >= self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back((now, entry));
    }
    /// Snapshot all entries, formatting raw types into serializable records.
    /// Timestamps are relative to the first entry in the buffer.
    /// Printable characters are scrubbed to `"Char"`.
    pub fn snapshot_entries(&self) -> Vec<InputRecord> {
        let base = self.entries.front().map(|(t, _)| *t);
        self.entries
            .iter()
            .map(|(t, raw)| InputRecord {
                ts_ms: base.map_or(0, |b| t.duration_since(b).as_millis() as u64),
                wall_ts: raw.wall_ts,
                key: sanitize_key_code(&raw.key_code),
                mods: format!("{:?}", raw.key_modifiers),
                kind: format!("{:?}", raw.key_kind),
                pane: format!("{:?}", raw.active_pane),
                outcome: format!("{:?}", raw.outcome),
                cursor_before: raw.cursor_before,
                cursor_after: raw.cursor_after,
                text_len_before: raw.text_len_before,
                text_len_after: raw.text_len_after,
                sel_before: raw.sel_before,
                sel_after: raw.sel_after,
                textarea_changed: raw.textarea_changed,
            })
            .collect()
    }
    /// Number of raw entries in the buffer.
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
    /// Time span covered by the buffer in milliseconds.
    pub fn time_span_ms(&self) -> u64 {
        match (self.entries.front(), self.entries.back()) {
            (Some((first, _)), Some((last, _))) => last.duration_since(*first).as_millis() as u64,
            _ => 0,
        }
    }
}
impl Default for InputRingBuffer {
    fn default() -> Self {
        Self::new()
    }
}
/// Top-level structure for the input debug dump file.
#[derive(Serialize)]
pub struct InputDump {
    pub dumped_at: String,
    pub session_id: Option<String>,
    pub pager_version: &'static str,
    pub terminal: diagnostics::events::TerminalDiagnostic,
    pub active_pane: String,
    pub textarea_cursor: usize,
    pub textarea_text_len: usize,
    pub textarea_has_selection: bool,
    pub entry_count: usize,
    pub time_span_ms: u64,
    pub entries: Vec<InputRecord>,
}
/// Return a privacy-safe description of a key code.
pub fn sanitize_key_code(code: &KeyCode) -> String {
    match code {
        KeyCode::Char('\x08') => "Char(BS)".to_string(),
        KeyCode::Char('\x7f') => "Char(DEL)".to_string(),
        KeyCode::Char(_) => "Char".to_string(),
        other => format!("{other:?}"),
    }
}
#[cfg(test)]
pub fn format_key_code_raw(code: &KeyCode) -> String {
    match code {
        KeyCode::Char(c) if c.is_ascii_graphic() || *c == ' ' => format!("Char({c:?})"),
        other => format!("{other:?}"),
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn input_dump_same_timestamp_preserves_each_private_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        let logs = directory.path().join("nested/logs");
        let now = chrono::DateTime::parse_from_rfc3339("2026-09-07T12:34:56Z").unwrap().to_utc();
        let first = super::write_input_dump(&logs, now, r#"{"snapshot":1}"#).unwrap();
        let second = super::write_input_dump(&logs, now, r#"{"snapshot":2}"#).unwrap();
        assert_ne!(first, second);
        for (path, expected) in [(&first, r#"{"snapshot":1}"#), (&second, r#"{"snapshot":2}"#)] {
            assert_eq!(std::fs::read_to_string(path).unwrap(), expected);
            assert!(path.file_name().unwrap().to_string_lossy().starts_with("input-debug-20260907-123456-"));
            assert_eq!(path.extension().unwrap(), "json");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(std::fs::metadata(path).unwrap().permissions().mode() & 0o777, 0o600);
            }
        }
        let error = super::write_input_dump_with(&logs, now, |file| {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(file.metadata()?.permissions().mode() & 0o777, 0o600);
            }
            std::io::Write::write_all(file, b"partial")?;
            Err(std::io::Error::other("injected write error"))
        }).unwrap_err();
        assert_eq!(error.to_string(), "injected write error");
        assert_eq!(std::fs::read_dir(&logs).unwrap().count(), 2);
        assert_eq!(std::fs::read_to_string(&first).unwrap(), r#"{"snapshot":1}"#);
        assert!(super::write_input_dump(&first.join("invalid"), now, "{}").is_err());
        assert_eq!(std::fs::read_to_string(&first).unwrap(), r#"{"snapshot":1}"#);
    }

    use super::*;
    use std::thread;
    use std::time::Duration;
    /// Helper: build a minimal `RawInputEntry` with the given key code.
    fn stub_entry(key_code: KeyCode) -> RawInputEntry {
        RawInputEntry {
            wall_ts: 0,
            key_code,
            key_modifiers: KeyModifiers::NONE,
            key_kind: KeyEventKind::Press,
            active_pane: ActivePaneSnapshot::Prompt,
            outcome: OutcomeSnapshot::Changed,
            cursor_before: None,
            cursor_after: None,
            text_len_before: None,
            text_len_after: None,
            sel_before: None,
            sel_after: None,
            textarea_changed: None,
        }
    }
    #[test]
    fn sanitize_printable_char_strips_value() {
        assert_eq!(sanitize_key_code(&KeyCode::Char('a')), "Char");
        assert_eq!(sanitize_key_code(&KeyCode::Char('Z')), "Char");
        assert_eq!(sanitize_key_code(&KeyCode::Char(' ')), "Char");
    }
    #[test]
    fn sanitize_control_chars_preserved() {
        assert_eq!(sanitize_key_code(&KeyCode::Char('\x08')), "Char(BS)");
        assert_eq!(sanitize_key_code(&KeyCode::Char('\x7f')), "Char(DEL)");
    }
    #[test]
    fn format_key_code_raw_shows_punctuation() {
        assert_eq!(format_key_code_raw(&KeyCode::Char(';')), "Char(';')");
        assert_eq!(
            format_key_code_raw(&KeyCode::Char('\'')),
            format!("Char({:?})", '\'')
        );
        assert_eq!(
            format_key_code_raw(&KeyCode::Char('\\')),
            format!("Char({:?})", '\\')
        );
        assert_eq!(format_key_code_raw(&KeyCode::Enter), "Enter");
    }
    #[test]
    fn sanitize_named_keys_use_debug() {
        assert_eq!(sanitize_key_code(&KeyCode::Backspace), "Backspace");
        assert_eq!(sanitize_key_code(&KeyCode::Enter), "Enter");
        assert_eq!(sanitize_key_code(&KeyCode::Delete), "Delete");
        assert_eq!(sanitize_key_code(&KeyCode::Tab), "Tab");
    }
    #[test]
    fn ring_buffer_push_and_snapshot() {
        let mut buf = InputRingBuffer::new();
        for _ in 0..5 {
            buf.push(stub_entry(KeyCode::Char('x')));
            thread::sleep(Duration::from_millis(1));
        }
        let entries = buf.snapshot_entries();
        assert_eq!(entries.len(), 5);
        assert_eq!(entries[0].ts_ms, 0);
        for w in entries.windows(2) {
            assert!(w[1].ts_ms >= w[0].ts_ms);
        }
        assert_eq!(entries[0].key, "Char");
        assert_eq!(entries[0].mods, format!("{:?}", KeyModifiers::NONE));
        assert_eq!(entries[0].pane, "Prompt");
    }
    #[test]
    fn ring_buffer_capacity() {
        let mut buf = InputRingBuffer::new();
        for _ in 0..250 {
            buf.push(stub_entry(KeyCode::Backspace));
        }
        assert_eq!(buf.entry_count(), DEFAULT_CAPACITY);
        let entries = buf.snapshot_entries();
        assert_eq!(entries.len(), DEFAULT_CAPACITY);
        assert_eq!(entries[0].key, "Backspace");
    }
    #[test]
    fn ring_buffer_empty() {
        let buf = InputRingBuffer::new();
        assert_eq!(buf.snapshot_entries().len(), 0);
        assert_eq!(buf.time_span_ms(), 0);
    }
}
