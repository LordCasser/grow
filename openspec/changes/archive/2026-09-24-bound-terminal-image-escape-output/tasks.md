## 1. Implement bounded image escape construction

- [x] 1.1 Replace Kitty and iTerm2 full-base64 intermediate construction with checked, bounded direct encoding; verify focused tests prove exact-fit acceptance, one-byte overflow rejection, and preserved chunk framing.
- [x] 1.2 Propagate upload overflow through overlay, inline-media, bounded draw/clear aggregation, and Kitty clear escapes without returning partial sequences or dropping pending cleanup; verify Pager lib compilation and clear-state regressions.
- [x] 1.3 Update the image-memory backlog wording to identify the Grow-owned terminal escape-buffer boundary and explicitly exclude terminal decode/cache memory; verify the unrelated backlog entries remain present.
- [x] 1.4 Share clear budgets across recursive agent trees and dashboard agents, reserving popup bytes before draining stale IDs; verify overflow retains the unappended IDs.
- [x] 1.5 Preserve previously placed image IDs when upload/placement output cannot fit, so the bounded stale-clear path can remove terminal state; discard IDs that were never placed.
- [x] 1.6 Clear iTerm2 emitted-placement state when a fully built escape is rejected by aggregate admission, so the next accepted frame retransmits its placement.

## 2. Validate contract and change artifacts

- [x] 2.1 Run focused `pager-render` image tests and record results in `verification.md`.
- [x] 2.2 Run `openspec validate --all --strict --no-interactive` and record results in `verification.md`.
