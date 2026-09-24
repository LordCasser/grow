# Change: Bound debug firehose resources

## Why

`GROW_DEBUG_LOG=1` creates a writer thread and open file for every session seen by a long-lived leader. Its startup-only age sweep does not bound worker count or bytes. An opt-in diagnostic stream must remain available during the failure it is meant to explain without consuming unbounded resources.

## What Changes

- Write firehose events from all sessions to one bounded, line-delimited file, with role, process ID and session identity in each line. The default path is `~/.grow/debug/firehose.txt`; explicit log paths retain their path and filter choice.
- Use one process-local bounded queue and one disk worker. Truncate oversized lines with a marker, and report dropped queued records as a coalesced loss count; a bounded exit flush does not block on a stuck filesystem indefinitely.
- Serialize cooperating cross-process appends and in-place tail retention on the opened inode. Detect path replacement before append and reopen rather than continuing into a detached file.
- Remove obsolete per-session sink, latest-link and guard contracts. Sweep old per-session logs by age as legacy cleanup only.

## Capabilities

### Modified Capabilities

- `client-surfaces`: debug file format, retention and writer lifecycle.

## Impact

`GROW_DEBUG_LOG=1` users read one stream and filter by the recorded session ID. The debug stream is diagnostic rather than a durable session fact; discarded records never affect session state or Timeline.
