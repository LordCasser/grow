//! On-disk session synthesis. [`replay`] writes legacy replay input and
//! `rewind_points.jsonl` directly for exact ACU/rewind control; [`bench`]
//! appends through the real storage adapter up to a byte target for fork/copy
//! benchmarks. [`admission`] converts that input into the unified physical
//! Timeline journal with durable admitted responses.

pub mod admission;
pub mod bench;
pub mod replay;

pub use admission::write_admitted_history;
pub use bench::synthesize_to_target_bytes;
pub use replay::{
    SessionSpec, expected_replay_lines, locate_session_dir, prepare_session, sid,
    write_rewind_jsonl,
};
