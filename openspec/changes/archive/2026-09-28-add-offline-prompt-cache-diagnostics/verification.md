# Verification

## Automated fixtures

Command: `python3 -m unittest discover -s scripts -p 'test_analyze_prompt_cache.py'`

Result: 15 tests passed on 2026-09-28. Synthetic fixtures cover all three request protocols, tools order, structured history append, nested cache hints, reasoning/settings change, image budget/native span projection changes, branch checks, retries and Sideband identity, route/source comparability prerequisites, known zero versus missing usage fields (including explicit JSON null), raw provider response usage allowlisting and selected response-to-settlement identity join/read-write cross-check, Sideband auxiliary settlement joining with read/write availability, 100/900 input coverage, BLAKE3 vectors, declared body limits, tampered evidence, symlink rejection, deterministic JSON/text output without prompt content, and unchanged Timeline bytes after a failure. The Timeline fixture sequence is monotonic. `openspec validate --all --strict --no-interactive` passed before archive.

## Snapshot smoke

Not run: `rg --files --hidden -g timeline.jsonl . /Users/lordcasser/.codex` and the same search under `/Users/lordcasser/workspace` found no complete, quiescent session snapshot with sampling artifacts. The synthetic fixture run is not provider hit or performance validation.

## Limits

The analyzer reports visible request/response evidence and durable Timeline settlement separately, including Sideband auxiliary attempts. It does not infer tokenizer token prefixes, provider cache decisions, hidden tenant/deployment identity, or server-side miss causes. The stored endpoint removes query and credentials, so even matching visible endpoints do not confirm identical provider routes. Sideband source refs are not projected into `source_projection`, so those requests remain not comparable under this tool's conservative classification.
