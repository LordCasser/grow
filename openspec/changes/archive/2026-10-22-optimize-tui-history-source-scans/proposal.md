# Avoid redundant TUI history source scans

## Why

Retained release evidence attributes about 93 ms of a modern 2048-turn replay's TUI consumption to image-reference extraction, including image-free text; message completion also clones the full source for detection. First visible history is not correlated precisely with first delivery/application/frame production.

## What Changes

Borrow completed source for image extraction, conservatively skip impossible image-reference regex searches, reuse image dimension probes and avoid allocating a lowercased source for Mermaid candidate detection. Add opt-in, once-per-replay first-send/application/frame/writer markers using existing cursor and writer identities. Keep existing rendering, extraction, completion-time file observation, copy and layout results.

Pure behavior-preserving optimization and diagnostics: `skip_specs: true`, no invented delta. Full validation/history readiness, persistence/ACK, order, normal-input suppression and Ctrl+D remain unchanged. No pagination/readiness split, draw-cadence change, Timeline decoder change or projection redesign.
