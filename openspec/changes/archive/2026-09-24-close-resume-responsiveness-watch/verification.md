# Verification

## Evidence review

- `2026-09-24-throttle-resume-replay-paint/verification.md`: synthetic 128/512-turn PTY history visibility improved to 2,352/6,108 ms; renders fell to 47/71; key-echo p95 was 7.7/7.8 ms.
- `2026-09-24-measure-resume-input-during-replay/verification.md`: 20 echoes occurred while replay remained active with a 40 ms test writer delay; p95 77.6 ms, maximum 78.2 ms; draft survived `SessionLoaded`.
- `2026-09-24-add-dense-tool-resume-pty-probe/verification.md`: synthetic 31,097,749-byte, 512-turn dense-tool case passed with p95 52.9 ms, maximum 58.5 ms, and draft survival. The first streamed replay marker did not appear during the echo window.

The measurements support closing a generic watch because no tested case reproduces the latency concern and the repaired paint path plus measured key echoes meet the stated criterion. Resident/cursor memory, multiple subagents, real long-text histories, and varied/slow terminal combinations remain unmeasured. No universal p95 claim is made.

## Checks

- `openspec validate close-resume-responsiveness-watch --type change --strict --no-interactive` — passed.
- `openspec validate --all --strict --no-interactive` — passed, 16/16.
- `git diff --check -- openspec/backlog.md openspec/changes/close-resume-responsiveness-watch` — passed.
- The empty Pager watch heading had no open item; removing it changes no contract. No product code changed; Cargo was not run.
