# Verification

All model traffic in these process and PTY journeys is served by loopback mock providers. Tests use the freshly built `target/debug/grow` binary.

## Passed

- `GROW_BINARY=/Users/lordcasser/workspace/projects/grow/target/debug/grow cargo test --locked -p shell --test test_session_protocol_recovery auxiliary_image_process_journeys -- --ignored --test-threads=1 --nocapture` — 5 passed, 12.67s. Covers typed ACP image input, refusal and image-description recovery, reload/fork/rewind/load, coordination, btw, recap, ACP and headless Memory Flush, JSONL persistence, cross-provider flush routing, actual `read_file` image tool result, and a held foreground turn.
- `GROW_BINARY=/Users/lordcasser/workspace/projects/grow/target/debug/grow PAGER_BINARY=/Users/lordcasser/workspace/projects/grow/target/debug/grow cargo test --locked -p pager --test pty_e2e_auxiliary_commands full_and_minimal_recap_and_flush_use_real_auxiliary_commands -- --ignored --test-threads=1 --nocapture` — 1 passed, 18.28s. Covers `/recap` and `/flush` in full and minimal Pager, waits for each turn to become idle, and checks the actual session daily Markdown log under `memory/**/sessions/*.md`.
- `GROW_BINARY=/Users/lordcasser/workspace/projects/grow/target/debug/grow PAGER_BINARY=/Users/lordcasser/workspace/projects/grow/target/debug/grow cargo test -p pager --test pty_e2e_auxiliary_commands -- --ignored --test-threads=3 --nocapture` — 3 passed, 195.29s after a fresh test build (2m27s). Covers full and minimal `/recap` + `/flush`, and full and minimal real CSI focus loss/return after the production away/idle gates. Both focus journeys send an unresolved image-bearing recap through the switched text route, observe image rejection → vision description → text retry, verify the original image remains on the primary Surface, verify the unique marker is displayed, and load the canonical session trace to assert the exact SessionRecap Sideband's frozen source refs, Result marker, and Completed End.
- `openspec validate verify-auxiliary-image-process-journeys --strict --no-interactive` — valid.
- `openspec validate --all --strict --no-interactive` — 23 passed, 0 failed.
