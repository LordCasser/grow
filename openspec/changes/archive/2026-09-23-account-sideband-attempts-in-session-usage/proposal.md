# Account for auxiliary model attempts in session usage

## Why

`/usage` currently omits Sideband model calls such as recap and memory flush even though the active Goal can charge them. A completed Sideband Result contains only the final successful usage; failed and retried provider attempts can consume tokens without a Result. Summing Result objects would undercount and could double-charge a child when its final bill is folded into the parent.

## What Changes

Record each admitted Sideband provider attempt and its known or unknown usage under the owning session Timeline, keyed by Sideband ID and attempt number. The session UsageLedger folds these records into lifetime, model and agent totals without incrementing main-loop turn count. Cold restore and child final-bill folding retain the same ownership and deduplication. `/usage`, headless usage and the normal status surface consume the existing ledger projection; no new public agent breakdown is added.
