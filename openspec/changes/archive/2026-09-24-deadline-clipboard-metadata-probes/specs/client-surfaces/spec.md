## ADDED Requirements

### Requirement: Pager clipboard metadata probes have a caller deadline
On macOS, Pager SHALL run native clipboard change-count and image-type snapshot probes on one bounded background worker. Pager callers SHALL wait at most 10 ms for AppKit initialization and native messaging; a missed deadline or unavailable worker SHALL return unknown metadata. Late results SHALL NOT be applied to a newer probe. The native worker SHALL retain at most one queued probe beyond the active call. Paste-time native image reads SHALL not wait behind an occupied in-process pasteboard lock and SHALL continue through the existing bounded subprocess fallback when that lock is busy.

#### Scenario: Native metadata call stalls
- **WHEN** AppKit initialization or an Objective-C clipboard message does not return promptly
- **THEN** the Pager caller receives unknown metadata within its deadline, remains interactive, and a later probe may retry without spawning another native worker.

#### Scenario: Paste arrives during a native stall
- **WHEN** a native metadata or image read holds the pasteboard lock during an explicit image paste
- **THEN** the new native image read skips the lock and follows the deadline-bound AppleScript fallback instead of waiting for that lock.

#### Scenario: A metadata reply arrives after the caller's deadline
- **WHEN** a timed-out native probe finishes after another probe has started
- **THEN** its reply is discarded and cannot replace the newer probe's metadata.
