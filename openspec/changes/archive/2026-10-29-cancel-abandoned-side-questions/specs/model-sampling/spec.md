## ADDED Requirements

### Requirement: Side question work follows its response receiver

A `/btw` side-question execution SHALL remain active only while its response receiver is open. Closing the receiver before execution starts SHALL prevent the question from starting; closing it during context preparation, Sideband execution, provider work, or retry backoff SHALL cancel the in-flight question. Cancellation SHALL release its execution ownership and preserve existing Sideband usage settlement and terminal cleanup. Session-level Sideband cancellation SHALL retain its existing behavior.

#### Scenario: Receiver closes before the worker starts

- **WHEN** a SideQuestion command's response receiver closes before its detached worker is first polled
- **THEN** the worker does not start context or provider work and creates no Sideband

#### Scenario: Receiver closes during side-question work

- **WHEN** the response receiver closes while the question is waiting or executing
- **THEN** the in-flight future is dropped, its Sideband-owned admitted usage is settled and terminal cleanup proceeds, with no further retry or provider work

#### Scenario: Session cancellation remains effective

- **WHEN** the caller still awaits the answer but the owning session cancels Sideband work
- **THEN** the question follows the existing session cancellation path and returns its existing cancellation failure when the receiver remains open
