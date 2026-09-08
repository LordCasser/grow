## Why
Startup awaits an unbounded read_to_string of announcement preferences. Oversized or special files can consume excessive memory or block the filesystem worker. The writer must share the read limit so normal persistence cannot publish an unreadable snapshot.

## What Changes
Read only ordinary files up to 1 MiB, check actual bytes through a bounded reader, and use Unix nonblocking open for special-file rejection. Reject oversized writes before preparing replacement. Preserve fail-open visibility on rejected/missing/malformed input and preserve old committed files on write rejection.
