//! Fixed-width references into the physical Timeline journal.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};

const REF_BYTES: u64 = 48;
const IDENTITY_BYTES: u64 = 40;

pub(super) fn index_len(records: u64, identities: u64, notices: u64) -> Option<u64> {
    records
        .checked_mul(REF_BYTES)?
        .checked_add(identities.checked_mul(IDENTITY_BYTES)?)?
        .checked_add(notices.checked_mul(8)?)
}

pub(super) fn write_positions(index: &mut File, positions: &[u64]) -> io::Result<()> {
    let mut writer = io::BufWriter::new(index);
    for position in positions {
        writer.write_all(&position.to_le_bytes())?;
    }
    writer.flush()
}

pub(super) fn notice_position(
    index: &mut File,
    records: u64,
    identities: u64,
    rank: u64,
) -> io::Result<u64> {
    let offset = index_len(records, identities, rank)
        .ok_or_else(|| invalid("invalid notice index offset"))?;
    index.seek(SeekFrom::Start(offset))?;
    let mut bytes = [0; 8];
    index.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

pub(super) fn domain_position(
    index: &mut File,
    records: u64,
    identities: u64,
    notices: u64,
    seq: u64,
) -> io::Result<u64> {
    let rank = notices
        .checked_add(seq)
        .ok_or_else(|| invalid("invalid domain index offset"))?;
    notice_position(index, records, identities, rank)
}

pub(super) fn notice_rank(
    index: &mut File,
    records: u64,
    identities: u64,
    count: u64,
    position: u64,
) -> io::Result<usize> {
    let (mut low, mut high) = (0, count);
    while low < high {
        let mid = low + (high - low) / 2;
        if notice_position(index, records, identities, mid)? < position {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    Ok(low as usize)
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

pub(super) fn write_identities(index: &mut File, identities: &[([u8; 32], u64)]) -> io::Result<()> {
    let mut writer = io::BufWriter::new(index);
    for (hash, seq) in identities {
        writer.write_all(hash)?;
        writer.write_all(&seq.to_le_bytes())?;
    }
    writer.flush()
}

pub(super) fn find_identity(
    index: &mut File,
    records: u64,
    count: u64,
    hash: [u8; 32],
    mut resolve: impl FnMut(&mut File, u64) -> io::Result<Option<blake3::Hash>>,
) -> io::Result<Option<blake3::Hash>> {
    let base = records * REF_BYTES;
    let read = |index: &mut File, slot: u64| -> io::Result<([u8; 32], u64)> {
        let mut bytes = [0; IDENTITY_BYTES as usize];
        index.seek(SeekFrom::Start(base + slot * IDENTITY_BYTES))?;
        index.read_exact(&mut bytes)?;
        let (digest, seq) = bytes.split_at(32);
        Ok((
            digest.try_into().unwrap(),
            u64::from_le_bytes(seq.try_into().unwrap()),
        ))
    };
    let (mut low, mut high) = (0, count);
    while low < high {
        let mid = low + (high - low) / 2;
        if read(index, mid)?.0 < hash {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    for slot in low..count {
        let (digest, seq) = read(index, slot)?;
        if digest != hash {
            break;
        }
        if let Some(found) = resolve(index, seq)? {
            return Ok(Some(found));
        }
    }
    Ok(None)
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
    crate::session::history::decode_record(&bytes)
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
