# Measurement controls

- Normal release, default CLI features, Rust 1.93.1; `CARGO_INCREMENTAL=0`, four build jobs; own target only. Compile/validation do not overlap formal measurements.
- Identical modern admission fixture: 8 × 4096 agent source bytes/turn, 128/512/optional2048 turns; repeat alternating AB/BA. Record source bytes, actual projected/applied notification count, binary digest, trace window and test pass count. Cold process with warmed filesystem cache; external machine load uncontrolled.
- Pre-P0 versus P0 is diagnostic overhead, not optimization. Counting RPC and no-draw semantic processing are controls with different boundaries, never directly subtract to claim exclusive CPU cost.
- The CLI/PTY transport is the default direct typed in-process channel, not leader IPC. RPC harness uses ACP JSON duplex and CountingClient; keep labels explicit.
- Loading-only aggregates exclude post-ready key echoes. `pager.replay.end_batch` is an early legacy proxy; the full `pager.replay.finalize` boundary includes final block completion and post-replay reducer work. PTY screen scans establish visible history separately.
- Writer wait includes queue, deliberate test delay and stderr lock wait. Write is actual write/flush. Delivery contains both and is nested, not summed. Presenter sequence links a request to eventual write; neither write ACK nor SessionLoaded proves terminal pixels are visible.
- Slow output is a 40ms delay per WriterPayload; repeat at least three pairs. Frame counts must name their interval. Do not multiply whole-test frames (includes typing) by40ms to explain loading.
- P1 gate: a material measured finalization cost, feasible bounded change preserving source flush, media observation, diagram/layout, raw/pretty copy and all load barriers. Record rejection as an allowed outcome.
