## MODIFIED Requirements

### Requirement: Confirmed quit requests bounded session tree suspension

Ctrl+C SHALL retain its existing composer/local-control behavior and cancel the current turn through the explicit cancellation path. It SHALL NOT request Quit, arm exit confirmation, suspend the tree, or escalate repeated cancellation into exit. At idle it SHALL remain local or a no-op. The default half-page-down shortcut SHALL be uppercase D so it does not conflict with Quit. The default global Quit shortcut SHALL be Ctrl+D, confirmed by a second press of the same shortcut within the existing one-second PendingAction window. The first press SHALL preserve drafts, admitted queues, current turn and Goal state without starting suspension. Expiry or another key SHALL clear the pending confirmation. Welcome, normal/minimal execution views and Dashboard SHALL use this same default confirmation; local scroll/input widgets SHALL NOT consume the registered global Quit key.

Confirmed Quit SHALL request suspension of every session tree owned by the exiting process and exit only after the required durable boundaries and writer retirement. A child view SHALL resolve to its canonical root. Suspension SHALL retain the existing shared five-second absolute deadline, unconfirmed/retry/explicit-force UI, ownership and command interruption rules. Repeated Quit input while an exit operation is Pending SHALL coalesce without another request, deadline reset or force quit. While Unconfirmed, a newly confirmed Quit SHALL dispatch the existing explicit retry path with a fresh request deadline; r retry and explicit f force quit SHALL remain available. Error and recovery controls SHALL remain visible across Welcome, Agent, Dashboard and minimal rendering, with control space reserved even on a narrow terminal. Stale responses SHALL NOT settle a newer request. Cancel and emergency force quit SHALL remain distinct actions. Read-only surfaces SHALL not start execution.

#### Scenario: Ctrl C preserves composer behavior
- **WHEN** the composer has a draft and the user presses Ctrl+C
- **THEN** its existing clear/local behavior runs without exit confirmation or a suspension request; subsequent Ctrl+C can cancel a running turn once the composer no longer consumes it.

#### Scenario: Ctrl C cancels or retries cancellation
- **WHEN** a turn is running or already cancelling and the composer does not consume Ctrl+C
- **THEN** Pager requests or retries cancellation; repeated Ctrl+C never produces Quit or tree suspension.

#### Scenario: Ctrl C at idle
- **WHEN** an execution view is idle, including minimal mode, or Welcome/Dashboard receives Ctrl+C
- **THEN** local input/view behavior may run but the application does not exit or arm Quit.

#### Scenario: First and second default Quit press
- **WHEN** Ctrl+D is pressed once and then again within one second
- **THEN** the first press only arms confirmation while preserving work and drafts; the second enters the existing tree suspension and exit path once.

#### Scenario: Confirmation expires or is interrupted
- **WHEN** the second Ctrl+D occurs after expiry, or another key including Ctrl+C intervenes
- **THEN** the prior arm cannot confirm exit; a fresh pair is required.

#### Scenario: Repeated Quit while persistence is pending
- **WHEN** the confirmed exit is waiting for persistence and another Quit key arrives
- **THEN** the same request and deadline remain, with no additional cancellation or forced process exit.

#### Scenario: Suspension remains unconfirmed
- **WHEN** the five-second caller deadline expires or a required confirmation fails
- **THEN** Pager reports unconfirmed and offers retry or explicit force quit without reporting successful suspension; repeated Ctrl+D does not select force quit.

#### Scenario: Nested and multiple roots
- **WHEN** a confirmed quit starts from a child view or owns multiple root trees
- **THEN** each owned root is suspended with its descendants; another owner's independent sessions are unaffected, and local transcript/selection behavior cannot replace global Quit confirmation.


#### Scenario: Quit is retried after failure
- **WHEN** suspension is unconfirmed and the user presses the same configured Quit shortcut twice within one second
- **THEN** the first press preserves the unconfirmed operation, the second starts the normal retry with a fresh request identity and deadline, and an old response cannot settle it.

#### Scenario: Error text is long or terminal is narrow
- **WHEN** an unconfirmed exit error wraps beyond its available rows, including on Welcome
- **THEN** the English retry/force controls retain visible reserved space and remain usable without selecting force through Ctrl+D.

#### Scenario: Minimal has an unconfirmed exit
- **WHEN** minimal rendering owns the live viewport and exit suspension is pending or unconfirmed
- **THEN** it uses the shared English exit status and controls with reserved viewport space, even when startup or another panel would normally render; failure is not silently hidden.
