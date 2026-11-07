## ADDED Requirements

### Requirement: Pending steering reconstructs against the preserved turn

Cold input reconstruction SHALL use the canonical runtime checkpoint to distinguish a preserved logical Turn from a target closed by interruption recovery. An admitted, unconsumed Steer input targeting the preserved Turn SHALL retain its original route, identity and content in the existing steering owner until the restored Turn reaches its normal safe boundary. Reconstruction SHALL NOT append a FIFO reroute, rerun admission Hooks, submit the input again or consume it before runtime execution release. Inputs whose targets were durably closed by normal interruption recovery SHALL retain the existing validated FIFO reroute behavior.

#### Scenario: Suspended target survives physical runtime retirement

- **WHEN** a session with a valid suspended checkpoint for Turn T and an allowed unconsumed Steer(T) is cold loaded after the old actor and persistence retire
- **THEN** restoration reaches the original runtime release barrier, the input remains bound to T, and T consumes it exactly once at its existing safe boundary without repeating admission.

#### Scenario: Process interruption has no preserved checkpoint

- **WHEN** interruption recovery closes the input's target Turn without a preserved runtime checkpoint
- **THEN** reconstruction durably reroutes that input to FIFO once and preserves its original payload and input identity under the existing validator.
