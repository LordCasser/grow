## Why

Pager correlates passive inquiry notices by source peer and inquiry ID, but its row state only distinguishes running from terminal. A later-arriving start can replace an approval update because both are nonterminal, discarding the approval detail. Replay, live delivery and parent/child inquiries may interleave, so row updates need a monotonic phase independent of generic tool chrome.

## What changes

- Give passive coordination rows explicit received, approved and terminal phases.
- Reject a lower-phase update for an existing row; replay of an equal phase does not replace a live row. Higher phases update the same row ID, preserving fold/timing behavior.
- Cover out-of-order notices and distinct peers sharing an inquiry ID.

## Impact

Pager presentation only. Shell's inquiry facts and model-visible context remain unchanged.
