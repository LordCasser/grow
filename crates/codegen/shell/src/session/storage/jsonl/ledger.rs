//! One physical append journal for domain and finite presentation facts.
use super::*;
use crate::session::history::{
    self, HistoryContent, HistoryFact, HistorySnapshot, JournalFact, ResponseAnchor, ResponseKey,
};

pub(crate) struct JournalData {
    pub events: Vec<chat_state::TimelineEvent>,
    pub history: HistorySnapshot,
    pub state: history::AppendState,
}

#[expect(
    clippy::large_enum_variant,
    reason = "A journal fact is a transient decode/append value, not a retained history slot; keep presentation payloads inline to avoid an allocation per record"
)]
pub(super) enum AppendFact {
    Domain(chat_state::TimelineEvent),
    Presentation(HistoryFact),
}

impl JsonlStorageAdapter {
    pub(crate) fn read_domain_history_range(
        directory: &super::super::ContainedDirectory,
        range: std::ops::Range<u64>,
    ) -> io::Result<Vec<chat_state::TimelineEvent>> {
        if range.is_empty() {
            return Ok(Vec::new());
        }
        if let Some(events) = working_snapshot::read_domain_range(directory, range.clone())? {
            return Ok(events);
        }
        let mut data = Self::read_journal_from_directory(directory)?;
        if data.state.events < range.end {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Timeline history ended before the captured domain cut",
            ));
        }
        data.events.truncate(range.end as usize);
        Ok(data.events.drain(range.start as usize..).collect())
    }
    pub(crate) fn read_journal_at_frontier(
        mut file: std::fs::File,
        path: PathBuf,
        frontier: u64,
        remaining_records: &mut usize,
    ) -> io::Result<JournalData> {
        let complete_len =
            Self::find_previous_newline(&mut file, frontier)?.map_or(0, |position| position + 1);
        Self::read_journal_prefix_data(
            &mut file,
            complete_len,
            &path,
            None,
            Some(remaining_records),
        )
        .map(|(data, _)| data)
    }
    pub(crate) fn read_journal_events(
        file: std::fs::File,
        path: PathBuf,
    ) -> io::Result<Vec<chat_state::TimelineEvent>> {
        Ok(Self::read_journal_file(file, path, None, None)?.events)
    }

    pub(crate) fn encode_journal(
        events: &[chat_state::TimelineEvent],
        facts: &[HistoryFact],
    ) -> io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        let mut sources = history::AppendState::default();
        let binds_notices = facts.iter().any(|fact| {
            matches!(
                &fact.content,
                HistoryContent::Grow(value) if matches!(value.as_ref(), history::GrowHistoryFact::UiNotice(_))
            )
        });
        for (position, event) in events.iter().enumerate() {
            if binds_notices {
                sources.index_receipt(event)?;
            }
            let raw = serde_json::value::to_raw_value(event).map_err(io::Error::other)?;
            let anchor = event
                .messages()
                .and_then(|message| message.response_admission.as_ref())
                .map(|_| ResponseAnchor::Admission);
            bytes.extend(history::encode_domain(position as u64, &raw, anchor)?);
        }
        for (index, fact) in facts.iter().enumerate() {
            let mut fact = fact.clone();
            sources.bind_presentation(&mut fact)?;
            let raw = serde_json::value::to_raw_value(&fact).map_err(io::Error::other)?;
            bytes.extend(history::encode_presentation(
                (events.len() + index) as u64,
                chrono::Utc::now().timestamp_millis(),
                &raw,
            )?);
        }
        Ok(bytes)
    }

    pub(crate) fn decode_journal_lines(lines: &[String]) -> io::Result<JournalData> {
        let mut data = JournalData {
            events: Vec::new(),
            history: HistorySnapshot::default(),
            state: history::AppendState::default(),
        };
        let mut hash = blake3::Hasher::new();
        for line in lines {
            if line.contains('\n') || line.len() as u64 + 1 > super::super::MAX_JSONL_ENTRY_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid imported Timeline record size",
                ));
            }
            let record = history::decode_record(line.as_bytes())?;
            data.state.accept(&record)?;
            data.history.push(&record);
            if let JournalFact::Domain { event, .. } = record.fact {
                data.events.push(event);
            }
            hash.update(line.as_bytes());
            hash.update(b"\n");
            data.history.bytes += line.len() as u64 + 1;
        }
        data.history.prefix_hash = hash.finalize().to_hex().to_string();
        Ok(data)
    }
    pub(crate) fn read_journal_from_directory(
        directory: &super::super::ContainedDirectory,
    ) -> io::Result<JournalData> {
        let path = directory.display_path().join(super::super::TIMELINE_FILE);
        let file = directory
            .open_regular(
                std::ffi::OsStr::new(super::super::TIMELINE_FILE),
                "mandatory Timeline ledger",
            )
            .map_err(|error| {
                if error.kind() == io::ErrorKind::NotFound {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "mandatory Timeline ledger is missing",
                    )
                } else {
                    error
                }
            })?;
        Self::read_journal_file(file, path, None, None)
    }

    pub(crate) fn read_journal_file(
        file: std::fs::File,
        path: PathBuf,
        mut remaining_bytes: Option<&mut u64>,
        remaining_records: Option<&mut usize>,
    ) -> io::Result<JournalData> {
        let mut file = file;
        let frontier = file.metadata()?.len();
        if remaining_bytes
            .as_ref()
            .is_some_and(|remaining| frontier > **remaining)
        {
            return Err(bounded_read_exhausted("Timeline ledger"));
        }
        let complete_len =
            Self::find_previous_newline(&mut file, frontier)?.map_or(0, |position| position + 1);
        if let Some(remaining) = remaining_bytes.as_deref_mut() {
            *remaining = remaining
                .checked_sub(frontier - complete_len)
                .ok_or_else(|| bounded_read_exhausted("Timeline ledger"))?;
        }
        let (data, _) = Self::read_journal_prefix_data(
            &mut file,
            complete_len,
            &path,
            remaining_bytes,
            remaining_records,
        )?;
        Ok(data)
    }

    pub(super) fn read_journal_prefix_data(
        file: &mut std::fs::File,
        complete_len: u64,
        path: &Path,
        remaining_bytes: Option<&mut u64>,
        remaining_records: Option<&mut usize>,
    ) -> io::Result<(JournalData, blake3::Hasher)> {
        Self::read_journal_prefix_indexed(
            file,
            complete_len,
            path,
            remaining_bytes,
            remaining_records,
            None::<fn(&history::JournalRecord, working_index::RecordRef) -> bool>,
        )
    }

    pub(super) fn read_journal_prefix_indexed(
        file: &mut std::fs::File,
        complete_len: u64,
        path: &Path,
        remaining_bytes: Option<&mut u64>,
        remaining_records: Option<&mut usize>,
        mut retain: Option<impl FnMut(&history::JournalRecord, working_index::RecordRef) -> bool>,
    ) -> io::Result<(JournalData, blake3::Hasher)> {
        let mut events = Vec::new();
        let mut history = HistorySnapshot::default();
        let mut state = history::AppendState::default();
        let mut hash = blake3::Hasher::new();
        Self::scan_journal_range(
            file,
            0..complete_len,
            path,
            &mut state,
            &mut hash,
            remaining_bytes,
            remaining_records,
            |record, offset, json| {
                let retained = retain.as_mut().is_none_or(|retain| {
                    retain(
                        &record,
                        working_index::RecordRef {
                            offset,
                            len: json.len() as u64,
                            digest: blake3::hash(json),
                        },
                    )
                });
                if retained {
                    if !history.positions.is_empty() || record.seq != history.sources.len() as u64 {
                        if history.positions.is_empty() {
                            history.positions.extend(0..history.sources.len() as u64);
                        }
                        history.positions.push(record.seq);
                    }
                    history.push(&record);
                }
                if retained && let JournalFact::Domain { event, .. } = record.fact {
                    events.push(event);
                }
                Ok(())
            },
        )?;
        history.bytes = complete_len;
        history.prefix_hash = hash.finalize().to_hex().to_string();
        Ok((
            JournalData {
                events,
                history,
                state,
            },
            hash,
        ))
    }

    // One physical scanner for full readers and a proven unchanged prefix.
    // The sink sees records only after the same wire and identity validation.
    #[expect(
        clippy::too_many_arguments,
        reason = "Keep the reader's existing optional byte/record budgets and caller-owned fold state at one scanner"
    )]
    pub(super) fn scan_journal_range(
        file: &mut std::fs::File,
        range: std::ops::Range<u64>,
        path: &Path,
        state: &mut history::AppendState,
        hash: &mut blake3::Hasher,
        remaining_bytes: Option<&mut u64>,
        remaining_records: Option<&mut usize>,
        mut visit: impl FnMut(history::JournalRecord, u64, &[u8]) -> io::Result<()>,
    ) -> io::Result<()> {
        Self::scan_journal_bytes(
            file,
            range,
            path,
            hash,
            remaining_bytes,
            remaining_records,
            |record, offset, json| {
                state.accept(&record)?;
                visit(record, offset, json)?;
                Ok(true)
            },
        )
    }

    pub(super) fn scan_journal_bytes(
        file: &mut std::fs::File,
        range: std::ops::Range<u64>,
        path: &Path,
        hash: &mut blake3::Hasher,
        mut remaining_bytes: Option<&mut u64>,
        mut remaining_records: Option<&mut usize>,
        mut visit: impl FnMut(history::JournalRecord, u64, &[u8]) -> io::Result<bool>,
    ) -> io::Result<()> {
        file.seek(io::SeekFrom::Start(range.start))?;
        let expected = range.end - range.start;
        let mut reader = io::BufReader::new(file.take(expected));
        let mut line = Vec::new();
        let mut consumed = 0u64;
        loop {
            line.clear();
            let read = (&mut reader)
                .take(super::super::MAX_JSONL_ENTRY_BYTES + 1)
                .read_until(b'\n', &mut line)?;
            if read == 0 {
                break;
            }
            Self::validate_jsonl_line_size(&line, "Timeline record")?;
            consumed += read as u64;
            if let Some(bytes) = remaining_bytes.as_deref_mut() {
                *bytes = bytes
                    .checked_sub(read as u64)
                    .ok_or_else(|| bounded_read_exhausted("Timeline ledger"))?;
            }
            if let Some(records) = remaining_records.as_deref_mut() {
                *records = records
                    .checked_sub(1)
                    .ok_or_else(|| bounded_read_exhausted("Timeline ledger"))?;
            }
            let json = line.strip_suffix(b"\n").ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "incomplete record inside committed Timeline prefix",
                )
            })?;
            let record = history::decode_record(json).map_err(|error| {
                if crate::session::persistence::session_version_mismatch_from(&error).is_some() {
                    error
                } else {
                    io::Error::new(
                        error.kind(),
                        format!(
                            "{} at byte {}: {error}",
                            path.display(),
                            range.start + consumed - read as u64
                        ),
                    )
                }
            })?;
            let keep_reading = visit(record, range.start + consumed - read as u64, json)?;
            hash.update(&line);
            if !keep_reading {
                return Ok(());
            }
        }
        if consumed != expected {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Timeline changed while reading committed prefix",
            ));
        }
        Ok(())
    }

    pub(super) async fn append_journal_fact(
        &self,
        info: &Info,
        fact: AppendFact,
        durability: AppendDurability,
    ) -> io::Result<bool> {
        if let AppendFact::Domain(chat_state::TimelineEvent {
            kind:
                chat_state::TimelineEventKind::Notification(chat_state::NotificationEvent::Received {
                    owner_session_id,
                    ..
                }),
            ..
        }) = &fact
            && owner_session_id != info.id.0.as_ref()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "foreign communication receipt in session history",
            ));
        }
        self.ensure_writer_lease(info)?;
        let directory = self.bound_session_directory(info)?;
        let prefix = self.timeline_prefix_state(info)?;
        let owner = self.clone();
        tokio::task::spawn_blocking(move || {
            #[cfg(test)]
            let probe = owner.timeline_sync_probe.as_deref();
            #[cfg(not(test))]
            let _owner = owner;
            Self::append_journal_fact_sync(
                &directory,
                &prefix,
                fact,
                durability,
                #[cfg(test)]
                probe,
            )
        })
        .await
        .map_err(io::Error::other)?
    }

    pub(super) fn append_journal_fact_sync(
        directory: &super::super::ContainedDirectory,
        prefix_state: &std::sync::Mutex<Option<LedgerPrefix>>,
        mut fact: AppendFact,
        durability: AppendDurability,
        #[cfg(test)] sync_probe: Option<&TimelineSyncProbe>,
    ) -> io::Result<bool> {
        let name = std::ffi::OsStr::new(super::super::TIMELINE_FILE);
        let path = directory.display_path().join(name);
        #[cfg(not(any(unix, windows)))]
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "handle-relative Timeline storage is unsupported",
        ));
        #[cfg(any(unix, windows))]
        let lock = Self::lock_append_contained(directory, name, &path)?;
        let result = (|| {
            let mut file = directory.open_read_write_create(name)?;
            let prior_stamp = LedgerFileStamp::from_metadata(&file.metadata()?);
            let complete_len = Self::timeline_complete_len(&mut file, prior_stamp.len)?;
            let mut cached = prefix_state
                .lock()
                .map_err(|_| io::Error::other("Timeline prefix state poisoned"))?;
            if cached.as_ref().is_some_and(|prefix| {
                !prefix.has_full_hash && (prefix.len != complete_len || prefix.stamp != prior_stamp)
            }) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "indexed Timeline changed during writer ownership",
                ));
            }
            if complete_len != prior_stamp.len {
                file.set_len(complete_len)?;
            }
            let stamp = LedgerFileStamp::from_metadata(&file.metadata()?);
            let refreshed = match cached.as_ref() {
                Some(prefix) if !prefix.has_full_hash => None,
                Some(prefix) if prefix.len == complete_len && prefix.stamp == stamp => None,
                Some(expected) => {
                    if expected.len != complete_len {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "Timeline committed prefix length changed",
                        ));
                    }
                    let actual = Self::load_timeline_prefix(&mut file, complete_len)?;
                    if expected.hash != actual.hash {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "Timeline committed prefix changed",
                        ));
                    }
                    Some(actual)
                }
                None => Some(Self::load_timeline_prefix(&mut file, complete_len)?),
            };
            if let Some(mut prefix) = refreshed {
                #[cfg(unix)]
                working_snapshot::attach_to_validated_prefix(
                    std::sync::Arc::new(directory.try_clone()?),
                    &mut file,
                    &mut prefix,
                )?;
                *cached = Some(prefix);
            }
            let prefix = cached.as_mut().expect("validated Timeline prefix");
            prefix.stamp = stamp;
            if let AppendFact::Presentation(event) = &mut fact {
                prefix.state.bind_presentation(event)?;
            }
            // Preserve the exact original source identity on an ambiguous ACK.
            // Presentation timestamps are write metadata, not a second payload.
            let (raw, domain, at_ms) = match &fact {
                AppendFact::Domain(event) => {
                    (serde_json::value::to_raw_value(event), Some(event), 0)
                }
                AppendFact::Presentation(event) => (
                    serde_json::value::to_raw_value(event),
                    None,
                    chrono::Utc::now().timestamp_millis(),
                ),
            };
            let raw = raw.map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            let digest = blake3::hash(raw.get().as_bytes());
            let mut repeated = false;
            let response_anchor = if let Some(event) = domain {
                if let Some((seq, old, anchor)) = prefix.state.last_domain
                    && seq == event.seq.get()
                {
                    if old != digest {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "Timeline domain identity conflicts with persisted payload",
                        ));
                    }
                    repeated = true;
                    anchor
                } else {
                    if event.seq.get() != prefix.state.events {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "Timeline domain append sequence mismatch",
                        ));
                    }
                    event
                        .messages()
                        .and_then(|message| message.response_admission.as_ref())
                        .map(|admission| {
                            let key = ResponseKey {
                                request_id: admission.identity.request_id.clone(),
                                attempt: admission.identity.attempt,
                            };
                            prefix
                                .state
                                .anchors
                                .get(&key)
                                .map_or(ResponseAnchor::Admission, |record| {
                                    ResponseAnchor::Candidate { record: *record }
                                })
                        })
                }
            } else {
                let AppendFact::Presentation(event) = &fact else {
                    unreachable!()
                };
                if let Some(id) = event.event_id() {
                    let old = match (prefix.state.identities.get(id), &prefix.indexed) {
                        (Some(old), _) => Some(*old),
                        (None, Some(index)) => index.identity(id)?,
                        (None, None) => None,
                    };
                    if let Some(old) = old {
                        if old != digest {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "Timeline presentation identity conflicts with persisted payload",
                            ));
                        }
                        repeated = true;
                    }
                }
                if let HistoryContent::Candidate(key) = &event.content
                    && prefix.state.anchors.contains_key(key)
                {
                    // Only the first provisional chunk owns a display position.
                    repeated = true;
                }
                None
            };
            if !repeated {
                let seq = prefix.state.records;
                let line = if domain.is_some() {
                    history::encode_domain(seq, &raw, response_anchor)?
                } else {
                    history::encode_presentation(seq, at_ms, &raw)?
                };
                Self::validate_jsonl_line_size(&line, "Timeline record")?;
                // Do not clone or fold the historical domain state per append.
                // Decode only this bounded new record; source indexes are compact.
                let record = match fact {
                    AppendFact::Domain(event) => history::JournalRecord {
                        seq,
                        fact: JournalFact::Domain {
                            event,
                            response_anchor,
                        },
                        digest,
                    },
                    AppendFact::Presentation(event) => history::JournalRecord {
                        seq,
                        fact: JournalFact::Presentation { at_ms, event },
                        digest,
                    },
                };
                prefix.state.validate(&record)?;
                file.seek(io::SeekFrom::End(0))?;
                if let Err(error) = file.write_all(&line).and_then(|()| file.flush()) {
                    // Roll back only the bytes written by this attempt while the
                    // append lock still excludes other writers.
                    file.set_len(complete_len)?;
                    prefix.stamp = LedgerFileStamp::from_metadata(&file.metadata()?);
                    return Err(error);
                }
                prefix.state.accept(&record)?;
                if prefix.indexed.is_some() {
                    let json = &line[..line.len() - 1];
                    let reference = working_index::RecordRef {
                        offset: complete_len,
                        len: json.len() as u64,
                        digest: blake3::hash(json),
                    };
                    if matches!(&record.fact, JournalFact::Domain { .. }) {
                        prefix.unpublished_domain.push(reference);
                    }
                    if working_snapshot::classify(&record)
                        != working_snapshot::RecordClass::Diagnostic
                    {
                        prefix.unpublished_records.push(reference);
                    }
                }
                prefix.hasher.update(&line);
                prefix.len = complete_len + line.len() as u64;
                prefix.hash = prefix.hasher.finalize();
                prefix.stamp = LedgerFileStamp::from_metadata(&file.metadata()?);
            }
            #[cfg(unix)]
            let publish = prefix
                .indexed
                .as_ref()
                .is_some_and(|index| index.should_advance(prefix.len));
            if matches!(durability, AppendDurability::Durable) {
                Self::sync_file_durable(&file)?;
                #[cfg(test)]
                if let Some(probe) = sync_probe {
                    probe(TimelineSyncPoint::File)?;
                }
                directory.sync()?;
                #[cfg(test)]
                if let Some(probe) = sync_probe {
                    probe(TimelineSyncPoint::Directory)?;
                }
            }
            #[cfg(unix)]
            if publish && let Some(indexed) = &prefix.indexed {
                let advanced = if matches!(durability, AppendDurability::Buffered) {
                    Self::sync_file_durable(&file)
                        .and_then(|()| directory.sync())
                        .and_then(|()| indexed.advance(prefix))
                } else {
                    indexed.advance(prefix)
                };
                match advanced {
                    Ok(Some(indexed)) => {
                        prefix.indexed = Some(indexed);
                        prefix.unpublished_records.clear();
                        prefix.unpublished_domain.clear();
                    }
                    Ok(None) => {}
                    Err(error) => {
                        tracing::warn!(%error, "Timeline committed; working frontier publication deferred")
                    }
                }
            }
            Ok(!repeated)
        })();
        #[cfg(any(unix, windows))]
        let _ = FileExt::unlock(&lock);
        result
    }
}

impl super::super::TimelineLedgerReader {
    /// Consume a previously validated read projection only after proving its
    /// complete prefix against this freshly pinned entity and snapshot.
    pub(crate) fn read_timeline_projection(
        mut self,
        mut previous: Option<super::super::TimelineReadProjection>,
    ) -> io::Result<super::super::TimelineReadProjection> {
        let complete_len =
            JsonlStorageAdapter::find_previous_newline(&mut self.file, self.snapshot_len)?
                .map_or(0, |position| position + 1);
        let mut hash = blake3::Hasher::new();
        let reuse = if let Some(prior) = previous
            .as_ref()
            .filter(|prior| prior.complete_len <= complete_len)
        {
            self.file.seek(io::SeekFrom::Start(0))?;
            let read = io::copy(&mut (&mut self.file).take(prior.complete_len), &mut hash)?;
            read == prior.complete_len && hash.finalize() == prior.prefix_hash
        } else {
            false
        };
        let (mut timeline, mut state, start) = if reuse {
            let prior = previous.take().unwrap();
            (prior.timeline, prior.state, prior.complete_len)
        } else {
            // A changed prefix cannot reuse this fold. Release it before
            // constructing its replacement, rather than retaining both.
            drop(previous.take());
            hash = blake3::Hasher::new();
            (
                chat_state::Timeline::default(),
                history::AppendState::default(),
                0,
            )
        };
        let mut events = Vec::new();
        JsonlStorageAdapter::scan_journal_range(
            &mut self.file,
            start..complete_len,
            &self.label,
            &mut state,
            &mut hash,
            None,
            None,
            |record, _, _| {
                if let JournalFact::Domain { event, .. } = record.fact {
                    events.push(event);
                }
                Ok(())
            },
        )?;
        // Preserve the full reader's error order: validate all physical
        // records before applying any new domain lifecycle facts.
        timeline = timeline
            .with_replayed_suffix(events)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        Ok(super::super::TimelineReadProjection {
            timeline,
            state,
            complete_len,
            prefix_hash: hash.finalize(),
        })
    }
}
