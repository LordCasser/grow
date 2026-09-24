## ADDED Requirements

### Requirement: Explicit scroll log paths have exclusive writer ownership
A scroll log recorder opening an explicit target SHALL acquire a nonblocking exclusive advisory lock on the opened regular file before truncating it, and SHALL retain ownership while writing. If ownership is unavailable, the recorder SHALL disable itself without modifying the existing file. The existing non-regular target rejection and per-recorder byte limit SHALL remain in effect.

#### Scenario: Second recorder targets the same file
- **WHEN** one recorder owns an explicit scroll log path and a second recorder attempts to open that same file, including through a path alias resolving to the same file
- **THEN** the second recorder disables itself without truncating or writing, and the first recorder's complete records remain intact

#### Scenario: Ownership is released
- **WHEN** the owning recorder is dropped and another recorder opens the target
- **THEN** the later recorder can acquire ownership and begin a new recording using the existing truncate-on-first-record behavior
