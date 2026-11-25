//! Disk-backed byte pages for oversized Tool display payloads.

use crate::extensions::notification::ToolOutputPayload;
use acp_transport::protocol as acp;
use redb::{ReadTransaction, ReadableTable, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};
use std::io;

const TOOL_FRAGMENTS: TableDefinition<u64, &[u8]> = TableDefinition::new("tool_fragments");
const TOOL_FRAGMENT_CHUNKS: TableDefinition<(u64, u64), &[u8]> =
    TableDefinition::new("tool_fragment_chunks");

const INLINE_BODY_BYTES: usize = 1024 * 1024;
const CHUNK_BYTES: usize = 128 * 1024;
const FRAGMENT_PAGE_CHUNKS: usize = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Header {
    tool_call_id: acp::ToolCallId,
    source_position: u64,
    digest: String,
    title: String,
    kind: acp::ToolKind,
    status: acp::ToolCallStatus,
    at_ms: i64,
    total_bytes: u64,
}

pub(super) fn initialize(tx: &WriteTransaction) -> io::Result<()> {
    tx.open_table(TOOL_FRAGMENTS).map_err(io::Error::other)?;
    tx.open_table(TOOL_FRAGMENT_CHUNKS)
        .map_err(io::Error::other)?;
    Ok(())
}

/// Index a terminal Tool call if its display payload is too large to carry as
/// one page row. Chunks and metadata commit in the same redb transaction as
/// their display-row reference.
pub(super) fn publish(
    tx: &WriteTransaction,
    source_position: u64,
    at_ms: i64,
    call: acp::ToolCall,
) -> io::Result<bool> {
    let json = serde_json::to_string(&ToolOutputPayload {
        content: call.content,
        locations: call.locations,
        raw_input: call.raw_input,
        raw_output: call.raw_output,
        meta: call.meta,
    })?;
    if json.len() <= INLINE_BODY_BYTES {
        return Ok(false);
    }

    let digest = blake3::hash(json.as_bytes()).to_hex().to_string();
    let mut chunks = tx
        .open_table(TOOL_FRAGMENT_CHUNKS)
        .map_err(io::Error::other)?;
    let mut start = 0;
    while start < json.len() {
        let mut end = (start + CHUNK_BYTES).min(json.len());
        while !json.is_char_boundary(end) {
            end -= 1;
        }
        chunks
            .insert((source_position, start as u64), json[start..end].as_bytes())
            .map_err(io::Error::other)?;
        start = end;
    }
    drop(chunks);

    let header = Header {
        tool_call_id: call.tool_call_id,
        source_position,
        digest,
        title: call.title,
        kind: call.kind,
        status: call.status,
        at_ms,
        total_bytes: json.len() as u64,
    };
    tx.open_table(TOOL_FRAGMENTS)
        .map_err(io::Error::other)?
        .insert(source_position, serde_json::to_vec(&header)?.as_slice())
        .map_err(io::Error::other)?;
    Ok(true)
}

/// Read at most three stored chunks ending at `end_offset`; `None` starts at
/// the final byte. Returned `byte_start` is the exclusive end for the older
/// page, so pagination advances without loading the rest of the Tool group.
pub(super) fn read(
    tx: &ReadTransaction,
    source_position: u64,
    end_offset: Option<u64>,
) -> io::Result<
    Option<(
        Option<u64>,
        i64,
        crate::extensions::notification::ToolOutputFragment,
    )>,
> {
    let Some(header) = tx
        .open_table(TOOL_FRAGMENTS)
        .map_err(io::Error::other)?
        .get(source_position)
        .map_err(io::Error::other)?
        .map(|value| serde_json::from_slice::<Header>(value.value()))
        .transpose()
        .map_err(io::Error::other)?
    else {
        return Ok(None);
    };
    if header.source_position != source_position {
        return Err(invalid("Tool fragment identity differs from its index key"));
    }
    let end = end_offset.unwrap_or(header.total_bytes);
    if end == 0 || end > header.total_bytes {
        return Err(invalid("Tool fragment cursor is outside its indexed body"));
    }

    let table = tx
        .open_table(TOOL_FRAGMENT_CHUNKS)
        .map_err(io::Error::other)?;
    let mut range = table
        .range((source_position, 0)..(source_position, end))
        .map_err(io::Error::other)?;
    let mut page = Vec::with_capacity(FRAGMENT_PAGE_CHUNKS);
    for entry in range.by_ref().rev().take(FRAGMENT_PAGE_CHUNKS) {
        let (key, value) = entry.map_err(io::Error::other)?;
        page.push((key.value().1, value.value().to_vec()));
    }
    page.reverse();
    let Some((byte_start, first)) = page.first() else {
        return Err(invalid("Tool fragment cursor has no indexed bytes"));
    };
    if *byte_start + first.len() as u64 > end {
        return Err(invalid("Tool fragment crosses its requested byte end"));
    }
    let mut json = Vec::with_capacity(page.iter().map(|(_, bytes)| bytes.len()).sum());
    let mut expected_start = *byte_start;
    for (start, bytes) in &page {
        if *start != expected_start {
            return Err(invalid("Tool fragment chunks are not contiguous"));
        }
        expected_start += bytes.len() as u64;
        json.extend_from_slice(bytes);
    }
    if expected_start != end {
        return Err(invalid(
            "Tool fragment cursor does not align with an indexed chunk",
        ));
    }
    let json = String::from_utf8(json).map_err(io::Error::other)?;
    Ok(Some((
        (*byte_start != 0).then_some(*byte_start),
        header.at_ms,
        crate::extensions::notification::ToolOutputFragment {
            tool_call_id: header.tool_call_id,
            source_position: header.source_position,
            digest: header.digest,
            title: header.title,
            kind: header.kind,
            status: header.status,
            byte_start: *byte_start,
            total_bytes: header.total_bytes,
            json,
        },
    )))
}

/// Read the next bounded set of chunks from an exact serialized JSON byte start.
pub(super) fn read_after(
    tx: &ReadTransaction,
    source_position: u64,
    start: u64,
) -> io::Result<
    Option<(
        Option<u64>,
        i64,
        crate::extensions::notification::ToolOutputFragment,
    )>,
> {
    let table = tx.open_table(TOOL_FRAGMENTS).map_err(io::Error::other)?;
    let Some(value) = table.get(source_position).map_err(io::Error::other)? else {
        return Ok(None);
    };
    let header: Header = serde_json::from_slice(value.value())?;
    if header.source_position != source_position || start >= header.total_bytes {
        return Err(invalid("Tool fragment start is outside its indexed body"));
    }
    let chunks = tx
        .open_table(TOOL_FRAGMENT_CHUNKS)
        .map_err(io::Error::other)?;
    let mut json = Vec::new();
    let mut end = start;
    for entry in chunks
        .range((source_position, start)..=(source_position, u64::MAX))
        .map_err(io::Error::other)?
        .take(FRAGMENT_PAGE_CHUNKS)
    {
        let (key, value) = entry.map_err(io::Error::other)?;
        if key.value().1 != end {
            return Err(invalid(
                "Tool fragment start does not align with an indexed chunk",
            ));
        }
        end += value.value().len() as u64;
        json.extend_from_slice(value.value());
    }
    if end == start || end > header.total_bytes {
        return Err(invalid("Tool fragment chunks cross their indexed body"));
    }
    Ok(Some((
        (end < header.total_bytes).then_some(end),
        header.at_ms,
        crate::extensions::notification::ToolOutputFragment {
            tool_call_id: header.tool_call_id,
            source_position,
            digest: header.digest,
            title: header.title,
            kind: header.kind,
            status: header.status,
            byte_start: start,
            total_bytes: header.total_bytes,
            json: String::from_utf8(json).map_err(io::Error::other)?,
        },
    )))
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use redb::ReadableDatabase;

    #[test]
    fn indexed_tool_output_is_read_as_verified_byte_pages() {
        let db = redb::Database::builder()
            .create_file(tempfile::tempfile().unwrap())
            .unwrap();
        let body = "tool-output-界".repeat(180_000);
        let call = acp::ToolCall::new("tool-1", "Large tool")
            .kind(acp::ToolKind::Execute)
            .status(acp::ToolCallStatus::Completed)
            .raw_output(Some(serde_json::json!({"text": body})));
        let mut tx = db.begin_write().unwrap();
        assert!(publish(&tx, 42, 123, call).unwrap());
        tx.commit().unwrap();

        let tx = db.begin_read().unwrap();
        let mut offset = None;
        let mut pages = Vec::new();
        let mut identity = None;
        loop {
            let Some((older, at_ms, fragment)) = read(&tx, 42, offset).unwrap() else {
                panic!("missing Tool fragment");
            };
            assert_eq!(at_ms, 123);
            assert_eq!(fragment.title, "Large tool");
            assert_eq!(fragment.kind, acp::ToolKind::Execute);
            assert_eq!(fragment.status, acp::ToolCallStatus::Completed);
            assert_eq!(
                fragment.byte_start + fragment.json.len() as u64,
                offset.unwrap_or(fragment.total_bytes)
            );
            identity.get_or_insert((
                fragment.source_position,
                fragment.digest.clone(),
                fragment.total_bytes,
            ));
            pages.push(fragment.json);
            let Some(older) = older else { break };
            offset = Some(older);
        }
        pages.reverse();
        let json = pages.concat();
        let (_, digest, total) = identity.unwrap();
        assert_eq!(json.len() as u64, total);
        assert_eq!(blake3::hash(json.as_bytes()).to_hex().as_str(), digest);
        assert!(serde_json::from_str::<serde_json::Value>(&json).is_ok());
        assert!(read(&tx, 42, Some(1)).is_err());
    }
}
