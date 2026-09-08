## Why
Continue diagnostic-entrypoint audit after input dump ownership. Pager unified logging has a separate buffering and delivery lifecycle from the already audited disk writers.

## What Changes
Read-only audit of pager startup, producers, flush/shutdown, ACP transport and shell ingestion. No runtime behavior changes or new contract in this audit.
