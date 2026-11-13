//! Read plans for a verified journal prefix. Independent notices and diagnostic
//! observations stay out of executable replay and remain addressable on disk.

use super::ledger::JournalData;
use super::working_index::{self, RecordRef};
use super::{JsonlStorageAdapter, LedgerFileStamp, LedgerPrefix, OpenedSession, ValidatedTimeline};
use crate::session::history::{self, HistoryContent, HistorySnapshot, JournalFact, JournalRecord};
use crate::session::storage::{ContainedDirectory, SessionUpdate, TIMELINE_FILE};
use acp_transport::protocol as acp;
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::fs::File;
use std::io;
#[cfg(unix)]
use std::io::Write as _;
use std::path::Path;
use std::sync::Arc;

const VERSION: u8 = 6;
const MANIFEST: &str = "working-history.json";
const MAX_MANIFEST_BYTES: u64 = 16 * 1024;
#[cfg(unix)]
const MAX_UNPUBLISHED_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Manifest {
    version: u8,
    session: String,
    journal: LedgerFileStamp,
    index: LedgerFileStamp,
    suffix: LedgerFileStamp,
    retained: LedgerFileStamp,
    generation: uuid::Uuid,
    base_bytes: u64,
    base_records: u64,
    base_domain_events: u64,
    bytes: u64,
    records: u64,
    domain_events: u64,
    first_domain_at_ms: Option<i64>,
    last_domain: Option<(u64, [u8; 32], Option<history::ResponseAnchor>)>,
    omitted_notices: u64,
    identities: u64,
    event_counter_floor: u64,
    hash: String,
    retained_records: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RecordClass {
    Retained,
    Notice,
    Diagnostic,
}

pub(super) struct IndexedRecord {
    pub reference: RecordRef,
    class: RecordClass,
    identity: Option<[u8; 32]>,
    domain: bool,
}

impl IndexedRecord {
    pub(super) fn new(record: &JournalRecord, reference: RecordRef) -> Self {
        let class = classify(record);
        let identity = match &record.fact {
            JournalFact::Presentation { event, .. } if class == RecordClass::Notice => event
                .event_id()
                .map(|id| *blake3::hash(id.as_bytes()).as_bytes()),
            _ => None,
        };
        Self {
            reference,
            class,
            identity,
            domain: matches!(record.fact, JournalFact::Domain { .. }),
        }
    }

    pub(super) fn is_retained(&self) -> bool {
        self.class == RecordClass::Retained
    }
}

#[derive(Debug, Clone)]
pub(crate) struct IndexedHistory {
    directory: Arc<ContainedDirectory>,
    manifest: Arc<Manifest>,
}

pub(super) fn classify(record: &JournalRecord) -> RecordClass {
    if let JournalFact::Domain { event, .. } = &record.fact
        && chat_state::Timeline::is_diagnostic(event)
        && event.at_ms >= 0
    {
        return RecordClass::Diagnostic;
    }
    let JournalFact::Presentation { event, .. } = &record.fact else {
        return RecordClass::Retained;
    };
    if event.meta.as_ref().is_none_or(|meta| {
        meta.keys()
            .all(|key| matches!(key.as_str(), "eventId" | "agentTimestampMs"))
    }) && matches!(&event.content, HistoryContent::Grow(value)
            if matches!(value.as_ref(), history::GrowHistoryFact::UiNotice(_)))
    {
        RecordClass::Notice
    } else {
        RecordClass::Retained
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

impl IndexedHistory {
    pub(crate) fn records(&self) -> u64 {
        self.manifest.records
    }

    pub(crate) fn omitted_count(&self) -> usize {
        self.manifest.omitted_notices as usize
    }

    pub(crate) fn event_counter_floor(&self) -> u64 {
        self.manifest.event_counter_floor
    }

    pub(super) fn identity(&self, id: &str) -> io::Result<Option<blake3::Hash>> {
        if self.manifest.identities == 0 {
            return Ok(None);
        }
        let mut reader = self.page_reader()?;
        self.find_identity(&mut reader.journal, &mut reader.index, id)
    }

    fn find_identity(
        &self,
        journal: &mut File,
        index: &mut File,
        id: &str,
    ) -> io::Result<Option<blake3::Hash>> {
        working_index::find_identity(
            index,
            self.manifest.base_records,
            self.manifest.identities,
            *blake3::hash(id.as_bytes()).as_bytes(),
            |index, seq| {
                let record = read_at(journal, index, seq, self.manifest.base_bytes)?;
                let JournalFact::Presentation { event, .. } = record.fact else {
                    return Err(invalid(
                        "non-presentation record in Timeline identity index",
                    ));
                };
                Ok((event.event_id() == Some(id)).then_some(record.digest))
            },
        )
    }

    /// Number of indexed-only display rows before a physical position.
    pub(crate) fn omitted_before(&self, position: u64) -> io::Result<usize> {
        if self.manifest.omitted_notices == 0 {
            return Ok(0);
        }
        let mut reader = self.page_reader()?;
        working_index::notice_rank(
            &mut reader.index,
            self.manifest.base_records,
            self.manifest.identities,
            self.manifest.omitted_notices,
            position,
        )
    }

    pub(crate) fn page_reader(&self) -> io::Result<IndexedHistoryReader<'_>> {
        let (journal, index) = open_index_files(&self.directory, &self.manifest)?;
        self.check_sources(&journal, &index)?;
        Ok(IndexedHistoryReader {
            history: self,
            journal,
            index,
        })
    }

    fn check_sources(&self, journal: &File, index: &File) -> io::Result<()> {
        check_index_sources(&self.manifest, journal, index)
    }
}

fn open_index_files(
    directory: &ContainedDirectory,
    manifest: &Manifest,
) -> io::Result<(File, File)> {
    let journal = directory.open_regular(OsStr::new(TIMELINE_FILE), "Timeline")?;
    let index = directory.open_regular(
        OsStr::new(&index_name(manifest.generation)),
        "Timeline index",
    )?;
    Ok((journal, index))
}

/// Resolve an explicit domain range by physical seek.
pub(super) fn read_domain_range(
    directory: &ContainedDirectory,
    range: std::ops::Range<u64>,
) -> io::Result<Option<Vec<chat_state::TimelineEvent>>> {
    let Some(manifest) = read_manifest(directory)? else {
        return Ok(None);
    };
    let (mut journal, mut index) = match open_index_files(directory, &manifest) {
        Ok(files) => files,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if check_index_sources(&manifest, &journal, &index).is_err() {
        return Ok(None);
    }
    let mut events = Vec::with_capacity((range.end - range.start) as usize);
    for seq in range.start..range.end.min(manifest.base_domain_events) {
        let position = working_index::domain_position(
            &mut index,
            manifest.base_records,
            manifest.identities,
            manifest.omitted_notices,
            seq,
        )?;
        let record = read_at(&mut journal, &mut index, position, manifest.base_bytes)?;
        let JournalFact::Domain { event, .. } = record.fact else {
            return Err(invalid(
                "indexed domain position refers to a presentation fact",
            ));
        };
        if event.seq.get() != seq {
            return Err(invalid("indexed domain sequence differs from journal"));
        }
        events.push(event);
    }
    if range.end.min(manifest.domain_events) > manifest.base_domain_events {
        let mut suffix = directory.open_regular(
            OsStr::new(&suffix_index_name(manifest.generation)),
            "Timeline domain suffix",
        )?;
        let stamp = LedgerFileStamp::from_metadata(&suffix.metadata()?);
        if !manifest.suffix.same_file(&stamp) || stamp.len < manifest.suffix.len {
            return Err(invalid("Timeline domain suffix index was replaced"));
        }
        for seq in
            range.start.max(manifest.base_domain_events)..range.end.min(manifest.domain_events)
        {
            let reference =
                working_index::read_ref(&mut suffix, seq - manifest.base_domain_events)?;
            if reference.offset < manifest.base_bytes
                || reference
                    .offset
                    .checked_add(reference.len)
                    .is_none_or(|end| end >= manifest.bytes)
            {
                return Err(invalid("indexed domain suffix crosses its frontier"));
            }
            let record = working_index::read_record(&mut journal, reference)?;
            let JournalFact::Domain { event, .. } = record.fact else {
                return Err(invalid("indexed domain suffix refers to presentation"));
            };
            if event.seq.get() != seq {
                return Err(invalid(
                    "indexed domain suffix sequence differs from journal",
                ));
            }
            events.push(event);
        }
    }
    if range.end > manifest.domain_events {
        // The index covers the published cut. Stop at the actor's captured
        // domain position so a concurrent later commit cannot enter the read.
        let file_len = journal.metadata()?.len();
        let complete_len = JsonlStorageAdapter::find_previous_newline(&mut journal, file_len)?
            .map_or(0, |pos| pos + 1);
        let mut expected_physical = manifest.records;
        let mut expected_domain = manifest.domain_events;
        JsonlStorageAdapter::scan_journal_bytes(
            &mut journal,
            manifest.bytes..complete_len,
            &directory.display_path().join(TIMELINE_FILE),
            &mut blake3::Hasher::new(),
            None,
            None,
            |record, _, _| {
                if record.seq != expected_physical {
                    return Err(invalid("non-contiguous physical history suffix"));
                }
                expected_physical += 1;
                if let JournalFact::Domain { event, .. } = record.fact {
                    if event.seq.get() != expected_domain {
                        return Err(invalid("non-contiguous domain history suffix"));
                    }
                    expected_domain += 1;
                    if range.contains(&event.seq.get()) {
                        events.push(event);
                    }
                }
                Ok(expected_domain < range.end)
            },
        )?;
        if events.len() as u64 != range.end - range.start {
            return Err(invalid("indexed domain range is incomplete"));
        }
    }
    Ok(Some(events))
}

fn check_index_sources(manifest: &Manifest, journal: &File, index: &File) -> io::Result<()> {
    JsonlStorageAdapter::check_timeline_prefix_stamp(index, &manifest.index)?;
    if working_index::index_len(
        manifest.base_records,
        manifest.identities,
        manifest.omitted_notices,
    )
    .and_then(|len| len.checked_add(manifest.base_domain_events.checked_mul(8)?))
        != Some(manifest.index.len)
    {
        return Err(invalid("incorrect Timeline index length"));
    }
    let stamp = LedgerFileStamp::from_metadata(&journal.metadata()?);
    if !manifest.journal.same_file(&stamp) || stamp.len < manifest.bytes {
        return Err(invalid("indexed Timeline source was replaced"));
    }
    Ok(())
}

impl IndexedHistory {
    #[cfg(unix)]
    pub(super) fn should_advance(&self, bytes: u64) -> bool {
        bytes.saturating_sub(self.manifest.bytes) >= MAX_UNPUBLISHED_BYTES
    }

    #[cfg(unix)]
    pub(super) fn advance(&self, prefix: &LedgerPrefix) -> io::Result<Option<Self>> {
        if prefix.len == self.manifest.bytes {
            return Ok(None);
        }
        let mut manifest = (*self.manifest).clone();
        let published = manifest
            .domain_events
            .checked_sub(manifest.base_domain_events)
            .ok_or_else(|| invalid("invalid domain suffix frontier"))?;
        if prefix.state.events.checked_sub(manifest.domain_events)
            != Some(prefix.unpublished_domain.len() as u64)
        {
            return Err(invalid("incomplete domain suffix publication"));
        }
        if !prefix.unpublished_domain.is_empty() {
            manifest.suffix = append_index_refs(
                &self.directory,
                &suffix_index_name(manifest.generation),
                &manifest.suffix,
                published,
                &prefix.unpublished_domain,
            )?;
        }
        if !prefix.unpublished_records.is_empty() {
            manifest.retained = append_index_refs(
                &self.directory,
                &retained_index_name(manifest.generation),
                &manifest.retained,
                manifest.retained_records,
                &prefix.unpublished_records,
            )?;
            manifest.retained_records += prefix.unpublished_records.len() as u64;
        }
        manifest.journal = prefix.stamp.clone();
        manifest.bytes = prefix.len;
        manifest.records = prefix.state.records;
        manifest.domain_events = prefix.state.events;
        manifest.first_domain_at_ms = prefix.state.first_domain_at_ms;
        manifest.last_domain = prefix
            .state
            .last_domain
            .map(|(seq, digest, anchor)| (seq, *digest.as_bytes(), anchor));
        manifest.hash = prefix.hash.to_hex().to_string();
        #[cfg(test)]
        publication_crash_probe("advance-before");
        if !write_manifest(&self.directory, &manifest)? {
            return Ok(None);
        }
        #[cfg(test)]
        publication_crash_probe("advance-after");
        Ok(Some(Self {
            directory: self.directory.clone(),
            manifest: Arc::new(manifest),
        }))
    }
}

#[cfg(unix)]
fn append_index_refs(
    directory: &ContainedDirectory,
    name: &str,
    published: &LedgerFileStamp,
    position: u64,
    references: &[RecordRef],
) -> io::Result<LedgerFileStamp> {
    let mut file = directory.open_read_write_create(OsStr::new(name))?;
    let stamp = LedgerFileStamp::from_metadata(&file.metadata()?);
    if !published.same_file(&stamp) || stamp.len < published.len {
        return Err(invalid("Timeline index was replaced"));
    }
    if stamp.len > published.len {
        file.set_len(published.len)?;
    }
    for (offset, reference) in references.iter().copied().enumerate() {
        working_index::write_ref(&mut file, position + offset as u64, reference)?;
    }
    JsonlStorageAdapter::sync_file_durable(&file)?;
    Ok(LedgerFileStamp::from_metadata(&file.metadata()?))
}

fn index_name(generation: uuid::Uuid) -> String {
    format!("timeline-{generation}.index")
}

fn suffix_index_name(generation: uuid::Uuid) -> String {
    format!("timeline-{generation}.suffix")
}

fn retained_index_name(generation: uuid::Uuid) -> String {
    format!("timeline-{generation}.retained")
}

pub(crate) struct IndexedHistoryReader<'a> {
    history: &'a IndexedHistory,
    journal: File,
    index: File,
}

impl IndexedHistoryReader<'_> {
    pub(crate) fn read(
        &mut self,
        rank: usize,
        session: &acp::SessionId,
        history: &HistorySnapshot,
    ) -> io::Result<SessionUpdate> {
        let manifest = &self.history.manifest;
        let pos = working_index::notice_position(
            &mut self.index,
            manifest.base_records,
            manifest.identities,
            rank as u64,
        )?;
        let record = read_at(&mut self.journal, &mut self.index, pos, manifest.base_bytes)?;
        if classify(&record) != RecordClass::Notice {
            return Err(invalid("unexpected record at indexed history position"));
        }
        let JournalFact::Presentation { at_ms, event } = record.fact else {
            unreachable!("verified standalone notice");
        };
        let mut update = event
            .into_update(session)?
            .expect("standalone notice has a display row");
        history.stamp_cursor(
            session,
            history.record_count(),
            Some(at_ms),
            pos,
            0,
            true,
            &mut update,
        )?;
        Ok(update)
    }
}

impl LedgerFileStamp {
    fn same_file(&self, other: &Self) -> bool {
        #[cfg(unix)]
        {
            self.device == other.device && self.inode == other.inode
        }
        #[cfg(not(unix))]
        {
            self == other
        }
    }
}

fn read_at(journal: &mut File, index: &mut File, pos: u64, end: u64) -> io::Result<JournalRecord> {
    let reference = working_index::read_ref(index, pos)?;
    if reference
        .offset
        .checked_add(reference.len)
        .is_none_or(|end_of_record| end_of_record >= end)
    {
        return Err(invalid("indexed record crosses published frontier"));
    }
    let record = working_index::read_record(journal, reference)?;
    if record.seq != pos {
        return Err(invalid("indexed record sequence differs from journal"));
    }
    Ok(record)
}

#[cfg(unix)]
pub(super) fn publish(
    directory: Arc<ContainedDirectory>,
    session: &str,
    file: &File,
    prefix: &LedgerPrefix,
    references: &[IndexedRecord],
) -> io::Result<Option<IndexedHistory>> {
    let previous = read_manifest(&directory)?;
    let generation = uuid::Uuid::new_v4();
    let name = index_name(generation);
    let mut index = directory.open_read_write_create(OsStr::new(&name))?;
    let suffix = directory.open_read_write_create(OsStr::new(&suffix_index_name(generation)))?;
    let mut retained =
        directory.open_read_write_create(OsStr::new(&retained_index_name(generation)))?;
    let mut retained_records = 0;
    let mut notices = Vec::new();
    let mut domains = Vec::new();
    let mut identities = Vec::new();
    for (seq, record) in references.iter().enumerate() {
        let seq = seq as u64;
        working_index::write_ref(&mut index, seq, record.reference)?;
        if record.domain {
            domains.push(seq);
        }
        if let Some(identity) = record.identity {
            identities.push((identity, seq));
        }
        match record.class {
            RecordClass::Retained => {
                working_index::write_ref(&mut retained, retained_records, record.reference)?;
                retained_records += 1;
            }
            RecordClass::Notice => notices.push(seq),
            RecordClass::Diagnostic => {}
        }
    }
    identities.sort_unstable();
    working_index::write_identities(&mut index, &identities)?;
    working_index::write_positions(&mut index, &notices)?;
    working_index::write_positions(&mut index, &domains)?;
    index.flush()?;
    JsonlStorageAdapter::sync_file_durable(&index)?;
    JsonlStorageAdapter::sync_file_durable(&suffix)?;
    JsonlStorageAdapter::sync_file_durable(&retained)?;
    JsonlStorageAdapter::sync_file_durable(file)?;
    let journal = LedgerFileStamp::from_metadata(&file.metadata()?);
    if journal != prefix.stamp {
        return Err(invalid("Timeline changed during index publication"));
    }
    let manifest = Manifest {
        version: VERSION,
        session: session.to_owned(),
        journal,
        index: LedgerFileStamp::from_metadata(&index.metadata()?),
        suffix: LedgerFileStamp::from_metadata(&suffix.metadata()?),
        retained: LedgerFileStamp::from_metadata(&retained.metadata()?),
        generation,
        base_bytes: prefix.len,
        base_records: prefix.state.records,
        base_domain_events: prefix.state.events,
        bytes: prefix.len,
        records: prefix.state.records,
        domain_events: prefix.state.events,
        first_domain_at_ms: prefix.state.first_domain_at_ms,
        last_domain: prefix
            .state
            .last_domain
            .map(|(seq, digest, anchor)| (seq, *digest.as_bytes(), anchor)),
        omitted_notices: notices.len() as u64,
        identities: identities.len() as u64,
        event_counter_floor: prefix
            .state
            .identities
            .keys()
            .filter_map(|id| history::event_counter_after(id))
            .max()
            .unwrap_or(0),
        hash: prefix.hash.to_hex().to_string(),
        retained_records,
    };
    #[cfg(test)]
    publication_crash_probe("before");
    if !write_manifest(&directory, &manifest)? {
        directory.remove_file(OsStr::new(&name), false)?;
        directory.remove_file(OsStr::new(&suffix_index_name(generation)), false)?;
        directory.remove_file(OsStr::new(&retained_index_name(generation)), false)?;
        return Ok(None);
    }
    #[cfg(test)]
    publication_crash_probe("after");
    if let Some(previous) = previous {
        let _ = directory.remove_file(OsStr::new(&index_name(previous.generation)), false);
        let _ = directory.remove_file(OsStr::new(&suffix_index_name(previous.generation)), false);
        let _ = directory.remove_file(OsStr::new(&retained_index_name(previous.generation)), false);
    }
    Ok(Some(IndexedHistory {
        directory,
        manifest: Arc::new(manifest),
    }))
}

#[cfg(unix)]
fn write_manifest(directory: &ContainedDirectory, manifest: &Manifest) -> io::Result<bool> {
    let payload = serde_json::to_vec(manifest).map_err(io::Error::other)?;
    if payload.len() as u64 > MAX_MANIFEST_BYTES {
        return Ok(false);
    }
    let mut encoded = Vec::with_capacity(32 + payload.len());
    encoded.extend_from_slice(blake3::hash(&payload).as_bytes());
    encoded.extend_from_slice(&payload);
    directory.write_atomic(OsStr::new(MANIFEST), &encoded, true, true)?;
    Ok(true)
}

pub(super) fn is_rewind_marker(source: &history::HistorySource) -> bool {
    matches!(source, history::HistorySource::Presentation { fact, .. }
        if matches!(&fact.content, HistoryContent::Grow(value)
            if matches!(value.as_ref(), history::GrowHistoryFact::RewindMarker { .. })))
}

pub(super) fn load(
    opened: &OpenedSession,
    file: &mut File,
    complete_len: u64,
    path: &Path,
) -> io::Result<Option<(ValidatedTimeline, LedgerPrefix)>> {
    let directory = opened.directory_handle();
    let Some(manifest) = read_manifest(&directory)? else {
        return Ok(None);
    };
    if manifest.session != opened.summary().info.id.0.as_ref() || manifest.bytes > complete_len {
        return Ok(None);
    }
    let stamp = LedgerFileStamp::from_metadata(&file.metadata()?);
    if stamp.len == manifest.journal.len && stamp != manifest.journal {
        return Ok(None);
    }
    let index = IndexedHistory {
        directory,
        manifest: Arc::new(manifest),
    };
    let disk_index = match index.directory.open_regular(
        OsStr::new(&index_name(index.manifest.generation)),
        "Timeline index",
    ) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if index.check_sources(file, &disk_index).is_err() {
        return Ok(None);
    }
    let retained = match index.directory.open_regular(
        OsStr::new(&retained_index_name(index.manifest.generation)),
        "Timeline retained index",
    ) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let stamp = LedgerFileStamp::from_metadata(&retained.metadata()?);
    let published = &index.manifest.retained;
    if !published.same_file(&stamp)
        || stamp.len < published.len
        || (stamp.len == published.len && stamp != *published)
    {
        return Ok(None);
    }
    hydrate(opened, file, complete_len, path, index, retained)
}

/// Retain indexing when a caller requires the dense projection or opens the
/// append path directly. The caller already verified the full journal; collect
/// references only for the part that has not been published.
#[cfg(unix)]
pub(super) fn attach_to_validated_prefix(
    directory: Arc<ContainedDirectory>,
    file: &mut File,
    prefix: &mut LedgerPrefix,
) -> io::Result<()> {
    let Some(manifest) = read_manifest(&directory)? else {
        return Ok(());
    };
    if manifest.bytes > prefix.len
        || (manifest.journal.len == prefix.stamp.len && manifest.journal != prefix.stamp)
    {
        return Ok(());
    }
    let index = match directory.open_regular(
        OsStr::new(&index_name(manifest.generation)),
        "Timeline index",
    ) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if check_index_sources(&manifest, file, &index).is_err() {
        return Ok(());
    }
    JsonlStorageAdapter::scan_journal_bytes(
        file,
        manifest.bytes..prefix.len,
        &directory.display_path().join(TIMELINE_FILE),
        &mut blake3::Hasher::new(),
        None,
        None,
        |record, offset, json| {
            let reference = RecordRef {
                offset,
                len: json.len() as u64,
                digest: blake3::hash(json),
            };
            if matches!(&record.fact, JournalFact::Domain { .. }) {
                prefix.unpublished_domain.push(reference);
            }
            if classify(&record) != RecordClass::Diagnostic {
                prefix.unpublished_records.push(reference);
            }
            Ok(true)
        },
    )?;
    JsonlStorageAdapter::check_timeline_prefix_stamp(file, &prefix.stamp)?;
    prefix.indexed = Some(IndexedHistory {
        directory,
        manifest: Arc::new(manifest),
    });
    Ok(())
}

fn read_manifest(directory: &ContainedDirectory) -> io::Result<Option<Manifest>> {
    let manifest = match directory.read_bounded(
        OsStr::new(MANIFEST),
        "working history index",
        MAX_MANIFEST_BYTES + 32,
    ) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        // Earlier generations stored cumulative references inside the
        // manifest. They are rebuildable and may exceed the new header size.
        Err(error) if error.kind() == io::ErrorKind::InvalidData => return Ok(None),
        Err(error) => return Err(error),
    };
    let Some((digest, payload)) = manifest.split_at_checked(32) else {
        return Ok(None);
    };
    if blake3::hash(payload).as_bytes() != digest {
        return Ok(None);
    }
    let Ok(manifest) = serde_json::from_slice::<Manifest>(payload) else {
        return Ok(None);
    };
    Ok((manifest.version == VERSION).then_some(manifest))
}

fn hydrate(
    opened: &OpenedSession,
    file: &mut File,
    complete_len: u64,
    path: &Path,
    index: IndexedHistory,
    mut retained: File,
) -> io::Result<Option<(ValidatedTimeline, LedgerPrefix)>> {
    let source = &index.manifest;
    let mut data = JournalData {
        events: Vec::new(),
        state: history::AppendState::default(),
        history: HistorySnapshot::default(),
    };
    let initial_stamp = LedgerFileStamp::from_metadata(&file.metadata()?);
    let mut expected_retained_offset = 0;
    for position in 0..source.retained_records {
        let reference = working_index::read_ref(&mut retained, position)?;
        let end = reference
            .offset
            .checked_add(reference.len)
            .filter(|end| *end < source.bytes)
            .ok_or_else(|| invalid("working record crosses published frontier"))?;
        if reference.offset < expected_retained_offset {
            return Err(invalid("working record crosses published frontier"));
        }
        expected_retained_offset = end + 1;
        accept_retained(
            &mut data,
            working_index::read_record(file, reference)?,
            source.records,
            source.domain_events,
        )?;
    }
    data.state.display_records += source.omitted_notices as usize;
    data.state.records = source.records;
    data.state.events = source.domain_events;
    data.state.first_domain_at_ms = source.first_domain_at_ms;
    data.state.last_domain = source
        .last_domain
        .map(|(seq, digest, anchor)| (seq, blake3::Hash::from_bytes(digest), anchor));
    let has_full_hash = source.bytes == 0;
    let mut hasher = if has_full_hash {
        blake3::Hasher::new()
    } else {
        let mut seeded = blake3::Hasher::new_derive_key("Grow indexed Timeline tail");
        seeded.update(source.hash.as_bytes());
        seeded
    };
    let mut tail = JournalData {
        events: Vec::new(),
        history: HistorySnapshot::default(),
        state: history::AppendState::default(),
    };
    let mut unpublished_records = Vec::new();
    let mut unpublished_domain = Vec::new();
    let mut tail_has_diagnostics = false;
    let mut identity_reader = (source.identities != 0 && complete_len != source.bytes)
        .then(|| index.page_reader())
        .transpose()?;
    JsonlStorageAdapter::scan_journal_range(
        file,
        source.bytes..complete_len,
        path,
        &mut data.state,
        &mut hasher,
        None,
        None,
        |record, offset, json| {
            let reference = RecordRef {
                offset,
                len: json.len() as u64,
                digest: blake3::hash(json),
            };
            if matches!(&record.fact, JournalFact::Domain { .. }) {
                unpublished_domain.push(reference);
            }
            if let JournalFact::Presentation { event, .. } = &record.fact
                && let Some(id) = event.event_id()
                && let Some(reader) = identity_reader.as_mut()
                && index
                    .find_identity(&mut reader.journal, &mut reader.index, id)?
                    .is_some()
            {
                return Err(invalid("duplicate presentation identity in Timeline tail"));
            }
            if classify(&record) == RecordClass::Diagnostic {
                tail_has_diagnostics = true;
                if let JournalFact::Domain { event, .. } = record.fact {
                    tail.events.push(event);
                }
            } else {
                unpublished_records.push(reference);
                collect(&mut tail, record);
            }
            Ok(())
        },
    )?;
    let mut timeline = chat_state::Timeline::from_indexed_prefix(
        data.events,
        chat_state::EventSeq::new(source.domain_events),
        source.first_domain_at_ms,
    )
    .and_then(|base| base.with_replayed_suffix(tail.events))
    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if tail_has_diagnostics {
        timeline.archive_diagnostics();
    }
    data.history.sources.append(&mut tail.history.sources);
    data.history.positions.append(&mut tail.history.positions);
    if timeline.history_branch().is_some() || data.history.sources.iter().any(is_rewind_marker) {
        return Ok(None);
    }
    let stamp = LedgerFileStamp::from_metadata(&file.metadata()?);
    if stamp != initial_stamp {
        return Err(invalid("Timeline changed while reading indexed cut"));
    }
    data.history.bytes = complete_len;
    data.history.prefix_hash = hasher.finalize().to_hex().to_string();
    data.history.indexed = Some(index.clone());
    let validated = opened.validate_timeline_evidence(
        opened.summary().info.id.0.as_ref(),
        timeline,
        data.history,
    )?;
    Ok(Some((
        validated,
        LedgerPrefix {
            len: complete_len,
            hash: hasher.finalize(),
            stamp,
            hasher,
            state: data.state,
            indexed: Some(index),
            has_full_hash,
            unpublished_records,
            unpublished_domain,
        },
    )))
}

fn accept_retained(
    data: &mut JournalData,
    record: JournalRecord,
    records: u64,
    domain_events: u64,
) -> io::Result<()> {
    let seq = record.seq;
    if seq >= records || seq < data.state.records {
        return Err(invalid("invalid retained record position"));
    }
    data.state.records = seq;
    if let JournalFact::Domain { event, .. } = &record.fact {
        if event.seq.get() < data.state.events || event.seq.get() >= domain_events {
            return Err(invalid("invalid retained domain sequence"));
        }
        data.state.events = event.seq.get();
    } else if let JournalFact::Presentation { event, .. } = &record.fact
        && let HistoryContent::Notice { event } = &event.content
        && *event >= data.state.events
    {
        return Err(invalid("retained receipt is missing its domain source"));
    }
    data.state.accept(&record)?;
    collect(data, record);
    Ok(())
}

fn collect(data: &mut JournalData, record: JournalRecord) {
    data.history.positions.push(record.seq);
    data.history.push(&record);
    if let JournalFact::Domain { event, .. } = record.fact {
        data.events.push(event);
    }
}

#[cfg(all(test, unix))]
const CRASH_STAGE_ENV: &str = "GROW_TEST_WORKING_HISTORY_CRASH_STAGE";
#[cfg(all(test, unix))]
const CRASH_HOME_ENV: &str = "GROW_TEST_WORKING_HISTORY_CRASH_HOME";

#[cfg(all(test, unix))]
fn publication_crash_probe(stage: &str) {
    if std::env::var_os(CRASH_HOME_ENV).is_some()
        && std::env::var(CRASH_STAGE_ENV).as_deref() == Ok(stage)
    {
        std::process::exit(73);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::session::{info::Info, persistence::default_model_id, storage::StorageAdapter};

    const TEST: &str = "session::storage::jsonl::working_snapshot::tests::process_exit_around_index_publication_recovers_exact_tail";
    const ADVANCE_TEST: &str = "session::storage::jsonl::working_snapshot::tests::process_exit_around_frontier_advance_preserves_domain_sequence";

    #[test]
    fn process_exit_around_frontier_advance_preserves_domain_sequence() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let fixture = |path: &Path| {
            let storage = JsonlStorageAdapter::with_root(path.to_path_buf());
            let info = Info {
                id: acp::SessionId::new("advance-crash"),
                cwd: path.to_string_lossy().into_owned(),
            };
            (storage, info)
        };
        if let Some(root) = std::env::var_os(CRASH_HOME_ENV) {
            runtime.block_on(async {
                let (storage, info) = fixture(Path::new(&root));
                storage
                    .init_session(&info, default_model_id())
                    .await
                    .unwrap();
                let mut timeline = chat_state::Timeline::default();
                let seed = timeline
                    .append(
                        sampling_types::ConversationItem::system("fixed"),
                        chat_state::MessageCause::Seed,
                    )
                    .unwrap();
                storage
                    .append_timeline_event_durable(&info, &seed)
                    .await
                    .unwrap();
                let request = timeline
                    .record(chat_state::TimelineEventKind::Runtime(
                        chat_state::session_runtime::SessionRuntimeEvent::SuspendRequested {
                            operation_id: "suspend".into(),
                            root_session_id: info.id.to_string(),
                        },
                    ))
                    .unwrap();
                storage
                    .append_timeline_event_durable(&info, &request)
                    .await
                    .unwrap();
                let mut large = diagnostic();
                let chat_state::TimelineEventKind::Observation(observation) = &mut large else {
                    unreachable!()
                };
                observation.data = Some(serde_json::Value::String(
                    "x".repeat(MAX_UNPUBLISHED_BYTES as usize),
                ));
                let archived = timeline.record(large).unwrap();
                storage
                    .append_timeline_event(&info, &archived)
                    .await
                    .unwrap();
            });
            panic!("incremental publication crash probe was not reached");
        }
        for stage in ["advance-before", "advance-after"] {
            let root = tempfile::tempdir().unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", ADVANCE_TEST, "--nocapture"])
                .env(CRASH_HOME_ENV, root.path())
                .env(CRASH_STAGE_ENV, stage)
                .status()
                .unwrap();
            assert_eq!(status.code(), Some(73), "{stage}");
            runtime.block_on(async {
                let (storage, info) = fixture(root.path());
                let directory = storage.open_session(&info).unwrap().directory_handle();
                let prior = read_manifest(&directory).unwrap().unwrap();
                assert_eq!(prior.base_bytes, 0);
                assert_eq!(prior.bytes != 0, stage == "advance-after");
                assert_eq!(
                    prior.retained_records,
                    if stage == "advance-after" { 2 } else { 0 }
                );
                let mut journal = std::fs::OpenOptions::new()
                    .append(true)
                    .open(directory.display_path().join(TIMELINE_FILE))
                    .unwrap();
                journal.write_all(b"{\"torn\":").unwrap();
                journal.sync_all().unwrap();
                let loaded = storage
                    .load_session_for_write_with_indexed_history(&info)
                    .await
                    .unwrap();
                assert_eq!(
                    loaded.history.indexed.as_ref().unwrap().manifest.generation,
                    prior.generation,
                    "first resume must use the normal writer's publication"
                );
                assert_eq!(loaded.timeline.events().len(), 2);
                let exact = read_domain_range(&directory, 0..3).unwrap().unwrap();
                assert_eq!(
                    exact
                        .iter()
                        .map(|event| event.seq.get())
                        .collect::<Vec<_>>(),
                    [0, 1, 2]
                );
                assert_eq!(loaded.timeline.next_seq().get(), 3);
                assert_eq!(
                    loaded.timeline.runtime_lifecycle().phase,
                    chat_state::session_runtime::RuntimePhase::Suspending
                );
                storage
                    .append_timeline_event_durable(&info, &exact[2])
                    .await
                    .unwrap();
                let verified = storage.read_timeline_events_sync(&info).unwrap();
                assert_eq!(verified.len(), 3);
                assert_eq!(verified[2].seq.get(), 2);
                assert!(chat_state::Timeline::is_diagnostic(&verified[2]));
            });
        }
    }

    fn diagnostic() -> chat_state::TimelineEventKind {
        chat_state::TimelineEventKind::Observation(chat_state::ObservationEvent {
            scope: "shell".into(),
            name: "artifact".into(),
            turn: None,
            step: None,
            data: Some(serde_json::Value::String("data".repeat(1024))),
        })
    }

    #[tokio::test]
    async fn indexed_writer_revalidates_a_rewritten_journal_of_the_same_length() {
        use std::io::{Seek, SeekFrom};

        let root = tempfile::tempdir().unwrap();
        let info = Info {
            id: acp::SessionId::new("rewritten-diagnostic"),
            cwd: root.path().to_string_lossy().into_owned(),
        };
        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let mut timeline =
            chat_state::Timeline::from_seed(vec![sampling_types::ConversationItem::system(
                "fixed",
            )])
            .unwrap();
        timeline.record(diagnostic()).unwrap();
        let bytes = JsonlStorageAdapter::encode_journal(timeline.events(), &[]).unwrap();
        let directory = storage.open_session(&info).unwrap().directory_handle();
        directory
            .write_atomic(OsStr::new(TIMELINE_FILE), &bytes, true, true)
            .unwrap();
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        assert_eq!(loaded.timeline.events().len(), 1);
        drop(storage);

        let mut journal = directory
            .open_read_write_create(OsStr::new(TIMELINE_FILE))
            .unwrap();
        let payload = bytes
            .windows(8)
            .position(|part| part == b"datadata")
            .unwrap();
        journal.seek(SeekFrom::Start(payload as u64)).unwrap();
        journal.write_all(b"X").unwrap();
        assert_eq!(journal.metadata().unwrap().len(), bytes.len() as u64);

        let fresh = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        let error = fresh
            .load_session_for_write_with_indexed_history(&info)
            .await
            .err()
            .expect("rewritten journal is invalid");
        assert!(
            error
                .to_string()
                .contains("Timeline record checksum mismatch"),
            "{error}"
        );
    }

    #[test]
    fn process_exit_around_index_publication_recovers_exact_tail() {
        if let Some(root) = std::env::var_os(CRASH_HOME_ENV) {
            run_phase(Path::new(&root), None);
            panic!("publication crash probe was not reached");
        }
        for stage in ["before", "after"] {
            let root = tempfile::tempdir().unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", TEST, "--nocapture"])
                .env(CRASH_HOME_ENV, root.path())
                .env(CRASH_STAGE_ENV, stage)
                .status()
                .unwrap();
            assert_eq!(status.code(), Some(73), "{stage}");
            run_phase(root.path(), Some(stage));
        }
    }

    fn run_phase(root: &Path, crashed_at: Option<&str>) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let storage = JsonlStorageAdapter::with_root(root.to_path_buf());
            let info = Info {
                id: acp::SessionId::new("indexed-crash"),
                cwd: root.to_string_lossy().into_owned(),
            };
            let mut timeline = chat_state::Timeline::default();
            timeline
                .append(
                    sampling_types::ConversationItem::system("fixed"),
                    chat_state::MessageCause::Seed,
                )
                .unwrap();
            let facts = (0..300)
                .map(|n| history::HistoryFact {
                    meta: Some(acp::Meta::from_iter([
                        (
                            "eventId".into(),
                            format!("indexed-crash-{}", n + 1_000).into(),
                        ),
                        ("agentTimestampMs".into(), 123.into()),
                    ])),
                    content: HistoryContent::Grow(Box::new(history::GrowHistoryFact::UiNotice(
                        crate::extensions::notification::UiNotice {
                            correlation_id: format!("notice-{n}"),
                            category: crate::extensions::notification::UiNoticeCategory::Command,
                            subject: None,
                            description: None,
                            message: format!("history {n}"),
                            tone: crate::extensions::notification::UiNoticeTone::Info,
                            details: None,
                        },
                    ))),
                })
                .collect::<Vec<_>>();
            if crashed_at.is_none() {
                let parent = storage.ensure_session_parent(&info).unwrap();
                storage.ensure_writer_lease(&info).unwrap();
                storage
                    .build_publish_and_cache(&info, &parent, |staging| {
                        let summary = super::super::Summary::new(&info, default_model_id())?;
                        staging.write_atomic(
                            OsStr::new(crate::session::storage::SUMMARY_FILE),
                            &crate::session::storage::serialize_summary(&summary)?,
                            true,
                            false,
                        )?;
                        staging.write_atomic(
                            OsStr::new(TIMELINE_FILE),
                            &JsonlStorageAdapter::encode_journal(timeline.events(), &facts)?,
                            true,
                            false,
                        )
                    })
                    .unwrap();
                storage
                    .load_session_for_write_with_indexed_history(&info)
                    .await
                    .unwrap();
                return;
            }
            let directory = storage.open_session(&info).unwrap().directory_handle();
            let prior = read_manifest(&directory).unwrap();
            assert_eq!(prior.is_some(), crashed_at == Some("after"));
            let event = timeline
                .record(chat_state::TimelineEventKind::Observation(
                    chat_state::ObservationEvent {
                        scope: "test".into(),
                        name: "durable tail".into(),
                        turn: None,
                        step: None,
                        data: None,
                    },
                ))
                .unwrap();
            let raw = serde_json::value::to_raw_value(&event).unwrap();
            let committed = history::encode_domain(301, &raw, None).unwrap();
            {
                let mut journal = std::fs::OpenOptions::new()
                    .append(true)
                    .open(directory.display_path().join(TIMELINE_FILE))
                    .unwrap();
                journal.write_all(&committed).unwrap();
                journal.write_all(b"{\"torn\":").unwrap();
                journal.sync_all().unwrap();
            }
            let loaded = storage
                .load_session_for_write_with_indexed_history(&info)
                .await
                .unwrap();
            assert_eq!(loaded.timeline.next_seq(), chat_state::EventSeq::new(2));
            assert_eq!(loaded.history.record_count(), 302);
            assert_eq!(loaded.history.display_record_count(), 300);
            let index = loaded.history.indexed.as_ref().unwrap();
            if let Some(prior) = prior {
                assert_eq!(index.manifest.generation, prior.generation);
            }
            assert_eq!(
                loaded
                    .history
                    .indexed
                    .as_ref()
                    .unwrap()
                    .event_counter_floor(),
                1_300
            );
            let pages = loaded
                .history
                .indexed_pages(
                    loaded
                        .history
                        .prepare(&info.id, &loaded.timeline, None)
                        .unwrap(),
                )
                .unwrap()
                .unwrap();
            let (older, newest) = pages.page(&info.id, pages.len()).unwrap();
            assert_eq!(older, 172);
            assert_eq!(newest.len(), 128);
            assert!(
                serde_json::to_string(&newest.last())
                    .unwrap()
                    .contains("history 299")
            );
            let exact = facts[3].clone();
            let mut conflict = facts[4].clone();
            conflict.meta = exact.meta.clone();
            assert!(
                !storage
                    .append_journal_fact(
                        &info,
                        super::super::ledger::AppendFact::Presentation(exact),
                        super::super::AppendDurability::Durable,
                    )
                    .await
                    .unwrap()
            );
            assert_eq!(
                storage
                    .append_journal_fact(
                        &info,
                        super::super::ledger::AppendFact::Presentation(conflict),
                        super::super::AppendDurability::Durable,
                    )
                    .await
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::InvalidData
            );
            let next = timeline.record(event.kind.clone()).unwrap();
            storage
                .append_timeline_event_durable(&info, &next)
                .await
                .unwrap();
            let verified = storage.read_timeline_events_sync(&info).unwrap();
            assert_eq!(verified.len(), 3);
            assert_eq!(verified.last().unwrap().seq, next.seq);

            // The newest page should not read the oldest notice. Even a
            // replacement with a valid journal checksum must fail when that
            // page is later selected.
            let mut reader = index.page_reader().unwrap();
            let first = working_index::read_ref(&mut reader.index, 1).unwrap();
            let JournalFact::Presentation { at_ms, .. } =
                working_index::read_record(&mut reader.journal, first)
                    .unwrap()
                    .fact
            else {
                unreachable!();
            };
            let mut altered = facts[0].clone();
            let HistoryContent::Grow(update) = &mut altered.content else {
                unreachable!();
            };
            let history::GrowHistoryFact::UiNotice(notice) = update.as_mut() else {
                unreachable!();
            };
            notice.message = "history X".into();
            let raw = serde_json::value::to_raw_value(&altered).unwrap();
            let replacement = history::encode_presentation(1, at_ms, &raw).unwrap();
            assert_eq!(replacement.len() as u64, first.len + 1);
            {
                use std::io::{Seek as _, SeekFrom};
                let mut journal = directory
                    .open_read_write_create(OsStr::new(TIMELINE_FILE))
                    .unwrap();
                journal.seek(SeekFrom::Start(first.offset)).unwrap();
                journal.write_all(&replacement).unwrap();
            }
            assert_eq!(pages.page(&info.id, pages.len()).unwrap().1.len(), 128);
            assert_eq!(
                pages.page(&info.id, 44).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        });
    }
}
