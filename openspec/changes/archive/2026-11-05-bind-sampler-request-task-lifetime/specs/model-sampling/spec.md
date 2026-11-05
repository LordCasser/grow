## ADDED Requirements

### Requirement: Sampler cleanup and owned cancellation bind to the submitted task

A reusable public RequestId SHALL select the current registration for explicit cancel and activity queries. Replacing that registration SHALL cooperatively cancel the preceding task without allowing its later completion, panic, abort cleanup or collect-future destruction to remove or cancel the replacement. A collect future's owned cancellation SHALL apply only to its own submission, including destruction before actor admission. Actor shutdown SHALL retain ownership of all admitted tasks through their existing cancellation and settlement boundaries.

#### Scenario: Replaced task ends late
- **WHEN** A is replaced by B under the same RequestId and A finishes after B is registered
- **THEN** B remains active and controllable until B itself completes or is explicitly cancelled.

#### Scenario: Old collect future is destroyed
- **WHEN** a plain or accounted collect future for A returns or is dropped after B replaced A
- **THEN** only A's submission can be cancelled by that guard, and B is unaffected.

#### Scenario: Request task panics
- **WHEN** one request task panics or is aborted
- **THEN** its own active registration is reclaimed without removing another task or inventing successful usage settlement, and the actor can continue serving requests.

#### Scenario: Cancellation precedes command processing
- **WHEN** a collect future is destroyed after its Submit is sent but before the actor processes it
- **THEN** that submission observes its own cancellation without starting a new provider attempt.
