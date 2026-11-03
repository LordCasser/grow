## ADDED Requirements

### Requirement: Terminal ownership ends after command group cleanup

On Unix, a terminal command owner SHALL finish signalling its still-owned process group before reaping its leader, including cancellation, timeout and normal leader completion. It SHALL NOT signal a remembered PGID after the leader has been reaped. Explicit cancellation SHALL allow the existing TERM grace before final group KILL; leader exit alone SHALL NOT waive group cleanup. This guarantee covers members that remain in the command process group, not descendants that deliberately detach from it. Windows SHALL retain Job Object based tree cleanup.

#### Scenario: Leader exits before a TERM-ignoring descendant
- **WHEN** cancellation sends TERM and the leader exits while a same-group descendant ignores TERM
- **THEN** final group KILL precedes leader reap and the descendant does not survive command cleanup.

#### Scenario: Naturally completed leader leaves a same-group descendant
- **WHEN** the terminal observes that the leader exited but a same-group descendant still holds output pipes
- **THEN** the group is terminated before leader reap and command completion drains the closed output without abandoning the descendant.

#### Scenario: Timeout leader exits during asynchronous cleanup
- **WHEN** timeout reports its existing terminal result and the leader exits before the next cleanup poll
- **THEN** the owner completes group cleanup before reaping that leader, preserving the timeout result.
