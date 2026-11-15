//! Fixed-width references into the physical Timeline journal.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};

const REF_BYTES: u64 = 48;
pub(super) fn index_len(records: u64) -> Option<u64> {
    records.checked_mul(REF_BYTES)
}

pub(super) fn write_positions(index: &mut File, positions: &[u64]) -> io::Result<()> {
    let mut writer = io::BufWriter::new(index);
    for position in positions {
        writer.write_all(&position.to_le_bytes())?;
    }
    writer.flush()
}

pub(super) fn domain_position(index: &mut File, records: u64, seq: u64) -> io::Result<u64> {
    let offset = index_len(records)
        .and_then(|base| base.checked_add(seq.checked_mul(8)?))
        .ok_or_else(|| invalid("invalid domain index offset"))?;
    index.seek(SeekFrom::Start(offset))?;
    let mut bytes = [0; 8];
    index.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) struct RecordRef {
    pub offset: u64,
    pub len: u64,
    #[serde(with = "digest_bytes")]
    pub digest: blake3::Hash,
}

mod digest_bytes {
    use serde::{Deserialize, Serialize};

    pub fn serialize<S: serde::Serializer>(
        hash: &blake3::Hash,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        hash.as_bytes().serialize(serializer)
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<blake3::Hash, D::Error> {
        <[u8; 32]>::deserialize(deserializer).map(blake3::Hash::from_bytes)
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn validate(reference: RecordRef) -> io::Result<()> {
    if reference.len == 0 || reference.len.saturating_add(1) > super::super::MAX_JSONL_ENTRY_BYTES {
        return Err(invalid("invalid Timeline index reference length"));
    }
    reference
        .offset
        .checked_add(reference.len)
        .ok_or_else(|| invalid("Timeline index reference range overflow"))?;
    Ok(())
}

pub(super) fn read_ref(index: &mut File, seq: u64) -> io::Result<RecordRef> {
    let offset = seq
        .checked_mul(REF_BYTES)
        .ok_or_else(|| invalid("Timeline index sequence overflow"))?;
    index.seek(SeekFrom::Start(offset))?;
    let mut bytes = [0; REF_BYTES as usize];
    index.read_exact(&mut bytes).map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            invalid("short Timeline index reference")
        } else {
            error
        }
    })?;
    let reference = RecordRef {
        offset: u64::from_le_bytes(bytes[0..8].try_into().expect("fixed slice")),
        len: u64::from_le_bytes(bytes[8..16].try_into().expect("fixed slice")),
        digest: blake3::Hash::from_bytes(bytes[16..48].try_into().expect("fixed slice")),
    };
    validate(reference)?;
    Ok(reference)
}

pub(super) fn write_ref(index: &mut File, seq: u64, reference: RecordRef) -> io::Result<()> {
    validate(reference)?;
    let offset = seq.checked_mul(REF_BYTES).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Timeline index sequence overflow",
        )
    })?;
    let _end = offset.checked_add(REF_BYTES).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Timeline index offset overflow",
        )
    })?;
    let len = index.metadata()?.len();
    if len % REF_BYTES != 0 || offset > len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Timeline index write would create a gap or follow a short reference",
        ));
    }
    index.seek(SeekFrom::Start(offset))?;
    index.write_all(&reference.offset.to_le_bytes())?;
    index.write_all(&reference.len.to_le_bytes())?;
    index.write_all(reference.digest.as_bytes())
}

pub(super) fn read_record(
    journal: &mut File,
    reference: RecordRef,
) -> io::Result<crate::session::history::JournalRecord> {
    crate::session::history::decode_record(&read_record_bytes(journal, reference)?)
}

pub(super) fn read_record_bytes(journal: &mut File, reference: RecordRef) -> io::Result<Vec<u8>> {
    validate(reference)?;
    let len = usize::try_from(reference.len)
        .map_err(|_| invalid("Timeline record length does not fit memory"))?;
    let mut bytes = vec![0; len];
    journal.seek(SeekFrom::Start(reference.offset))?;
    journal.read_exact(&mut bytes).map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            invalid("short Timeline record referenced by index")
        } else {
            error
        }
    })?;
    if blake3::hash(&bytes) != reference.digest {
        return Err(invalid("Timeline record digest mismatch"));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(offset: u64, bytes: &[u8]) -> RecordRef {
        RecordRef {
            offset,
            len: bytes.len() as u64,
            digest: blake3::hash(bytes),
        }
    }

    #[test]
    fn fixed_width_reference_can_be_overwritten() {
        let mut index = tempfile::tempfile().unwrap();
        let first = reference(2, b"first");
        let second = reference(9, b"second");
        write_ref(&mut index, 0, first).unwrap();
        write_ref(&mut index, 0, second).unwrap();
        assert_eq!(index.metadata().unwrap().len(), REF_BYTES);
        assert_eq!(read_ref(&mut index, 0).unwrap(), second);
    }

    #[test]
    fn short_reference_is_rejected() {
        let mut index = tempfile::tempfile().unwrap();
        index.write_all(&[0; 47]).unwrap();
        assert_eq!(
            read_ref(&mut index, 0).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn mutated_referenced_record_is_rejected() {
        let mut journal = tempfile::tempfile().unwrap();
        journal.write_all(b"record bytes").unwrap();
        journal.seek(SeekFrom::Start(0)).unwrap();
        journal.write_all(b"Record bytes").unwrap();
        assert_eq!(
            read_record(&mut journal, reference(0, b"record bytes"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn write_cannot_skip_a_reference_slot() {
        let mut index = tempfile::tempfile().unwrap();
        assert_eq!(
            write_ref(&mut index, 1, reference(0, b"x"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }
}
