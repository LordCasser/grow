# Change: Preserve Responses message phases in portable history

## Why

The native Responses continuation preserves output message `phase`, but the provider-neutral Assistant record joins every output message into one string. A route change or native reset consequently rebuilds Responses input with one phase-less message. This loses commentary/final boundaries even when the source messages remain in un-compacted Timeline history.

## What Changes

- Record each Responses output message's text range and phase alongside the existing flattened Assistant content, without duplicating full text or provider output IDs.
- Rebuild portable Responses input as one assistant message per recorded range with its phase, while preserving one trailing local tool-call batch and paired results.
- Keep the range metadata through durable recovery and portable projection. On text redaction or truncation, discard stale ranges; compaction intentionally replaces its selected source span with a summary and does not claim exact phase replay for discarded source.

## Capabilities

### Modified Capabilities

- `model-sampling`: portable history and Responses request projection.

## Impact

Chat Completions and Messages continue to use flattened text. Legacy Assistant records without ranges continue to produce one phase-less Responses message. The display text remains flattened; no historical Timeline rewrite is needed.
