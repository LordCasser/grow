## MODIFIED Requirements

### Requirement: Confirmed quit waits for session tree suspension

Ctrl+C SHALL retain its existing composer/local-control behavior and cancel the current turn through the explicit cancellation path. It SHALL NOT request Quit, arm exit confirmation, suspend the tree, or escalate repeated cancellation into exit. At idle it SHALL remain local or a no-op. The default half-page-down shortcut SHALL be uppercase D so it does not conflict with Quit. The default global Quit shortcut SHALL be Ctrl+D, confirmed by a second press of the same shortcut within the existing one-second PendingAction window. The first press SHALL preserve drafts, admitted queues, current turn and Goal state without starting suspension. Expiry or another key SHALL clear the pending confirmation. Welcome, normal/minimal execution views and Dashboard SHALL use this same default confirmation; local scroll/input widgets SHALL NOT consume the registered global Quit key.

Confirmed Quit SHALL request suspension of every session tree owned by the exiting process and exit only after the required durable boundaries and writer retirement. A child view SHALL resolve to its canonical root. Confirmed exit SHALL wait for the same suspension request through durable completion and writer retirement without a fixed overall deadline. Pending UI SHALL show that waiting continues automatically and offer explicit force quit with a recovery warning. Suspension SHALL retain ownership and command interruption rules. Repeated Quit input while an exit operation is Pending SHALL coalesce without another request or force quit. While Unconfirmed, a newly confirmed Quit SHALL dispatch the existing explicit retry path with a fresh request identity; r retry and explicit f force quit SHALL remain available. Error and recovery controls SHALL remain visible across Welcome, Agent, Dashboard and minimal rendering, with control space reserved even on a narrow terminal. Stale responses SHALL NOT settle a newer request. Cancel and emergency force quit SHALL remain distinct actions. Read-only surfaces SHALL not start execution.

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
- **THEN** the same request remains, with no additional cancellation or forced process exit.

#### Scenario: Saving takes longer than five seconds
- **WHEN** a confirmed exit is still waiting for the checkpoint, tree barrier or writer retirement after five seconds
- **THEN** Pager keeps waiting on the same request, shows the explicit force option and its recovery risk, and exits normally when the original request confirms suspension.

#### Scenario: Suspension remains unconfirmed
- **WHEN** a required confirmation fails
- **THEN** Pager reports unconfirmed and offers retry or explicit force quit without reporting successful suspension; repeated Ctrl+D does not select force quit.

#### Scenario: Nested and multiple roots
- **WHEN** a confirmed quit starts from a child view or owns multiple root trees
- **THEN** each owned root is suspended with its descendants; another owner's independent sessions are unaffected, and local transcript/selection behavior cannot replace global Quit confirmation.

#### Scenario: Quit is retried after failure
- **WHEN** suspension is unconfirmed and the user presses the same configured Quit shortcut twice within one second
- **THEN** the first press preserves the unconfirmed operation, the second starts the normal retry with a fresh request identity, and an old response cannot settle it.

#### Scenario: Error text is long or terminal is narrow
- **WHEN** an unconfirmed exit error wraps beyond its available rows, including on Welcome
- **THEN** the English retry/force controls retain visible reserved space and remain usable without selecting force through Ctrl+D.

#### Scenario: Minimal has an unconfirmed exit
- **WHEN** minimal rendering owns the live viewport and exit suspension is pending or unconfirmed
- **THEN** it uses the shared English exit status and controls with reserved viewport space, even when startup or another panel would normally render; failure is not silently hidden.

### Requirement: Resident exit tree evidence reuses the canonical validated owner

Resident session-tree suspension SHALL query the existing canonical owner for a coherent runtime lifecycle and complete Spawned/Ended projection, without copying unrelated history bodies or replaying the entire already-validated Timeline solely to reconstruct child facts. The projection SHALL retain original event identity, sequence and child/result references. Cold or otherwise unvalidated storage sources SHALL retain their complete existing validation. Suspension SHALL reobserve child facts after the existing retirement frontier and SHALL preserve durable checkpoints and physical writer retirement requirements.

#### Scenario: Long unrelated history
- **WHEN** a resident session with long ordinary history requests confirmed exit
- **THEN** tree coordination obtains the same canonical child facts through the narrow owner projection without copying unrelated history bodies or reconstructing their Surface.

#### Scenario: Child finishes during suspension
- **WHEN** a child terminal is committed while suspension drains its owners
- **THEN** the post-retirement projection observes that original terminal and the final tree remains consistent with verified child identity and result references.

#### Scenario: Source is cold or unavailable
- **WHEN** a tree source must be loaded from storage or its canonical query fails
- **THEN** cold source validation remains required and query failure remains an unconfirmed error rather than an empty successful tree.

### Requirement: Live image maintenance uses committed owner evidence

Startup image-asset maintenance for a resident session SHALL obtain retained image references from the existing validated canonical owner, including committed historical User references no longer present in the Surface, without rereading and replaying the whole live Timeline. The existing artifact gate, contained-directory deletion and writer retirement ownership SHALL remain in force. Failed authority queries SHALL preserve all assets, and cancellation SHALL stop optional cleanup without detaching an accepted deletion or claiming an unfinished worker has retired.

#### Scenario: Immediate exit after loading long history
- **WHEN** a long-history session begins startup image maintenance and immediately requests exit
- **THEN** image maintenance does not add another full-history disk decode and replay to the service retirement barrier, and exit still waits for all required confirmations.

#### Scenario: Historical and orphan image batches
- **WHEN** image assets include a committed historical batch removed from the current Surface and an unreferenced recognized batch
- **THEN** maintenance preserves the committed batch and may remove only the unreferenced recognized batch under the existing gate and contained directory authority.

#### Scenario: Maintenance is cancelled or authority fails
- **WHEN** cleanup is cancelled or the committed-reference query is unavailable
- **THEN** no unconfirmed reference is treated as absent, pending cleanup stops conservatively, and the exact worker ownership barrier still completes before successful exit.

### Requirement: Exit suspension uses runtime ownership rather than visible pages

Pager SHALL ask the runtime to confirm suspension before exit even when Welcome or only viewer pages remain. A startup without confirmed session identity SHALL retain its owner and show an actionable unconfirmed exit. A leader SHALL derive and overwrite the requesting connection's scope from its existing session drivers; one client's exit SHALL NOT suspend another client's roots or reuse another client's pending exit. Standalone process exit SHALL cover all its roots. Confirmation SHALL cover a concurrently closing root's writer through the existing lifecycle transaction even if the writer is still retiring after five seconds. Global Quit input SHALL cancel any interrupted command prefix. Pending exit SHALL block new user admission while retaining resize and explicit force quit; startup replay SHALL NOT swallow those actions. Leader startup requests SHALL settle registration before that connection can confirm exit.

#### Scenario: Visible pages do not establish runtime emptiness
- **WHEN** Quit is confirmed from Welcome or a viewer-only surface
- **THEN** runtime confirmation is requested before Quit; local view flags cannot skip it.

#### Scenario: Startup identity is not known yet
- **WHEN** Quit is confirmed while session creation or fork/load startup has no confirmed identity
- **THEN** the startup owner remains and exit is unconfirmed with an explicit retry; the process does not exit as if no runtime existed.

#### Scenario: Two clients request exit
- **WHEN** clients with distinct driver roots request exit, including overlapping in-flight requests
- **THEN** each request covers only its authoritative scope and has its own pending owner; forged client scope is overwritten by the leader.

#### Scenario: Runtime close is draining
- **WHEN** a root has left the resident map but its close still owns a writer
- **THEN** exit cannot succeed until the existing lifecycle transaction proves that writer has settled, and failure retains the existing retry/force flow.

#### Scenario: Quit interrupts a command prefix
- **WHEN** the user opens Ctrl+X guidance and then presses global Quit
- **THEN** the old prefix cannot make a later Ctrl+X confirm closing the session.

#### Scenario: A host notification turn changes the viewer projection
- **WHEN** a locally owned parent receives a child completion and its host-started Turn is projected as a viewer while another child remains active, then the user confirms Quit with two Ctrl+D presses
- **THEN** the first press preserves work, the second requests runtime tree suspension, and Quit is emitted only after the required durable boundary and writer retirement; the viewer projection never skips suspension or authorizes child cancellation.

## RENAMED Requirements

- FROM: `### Requirement: Confirmed quit requests bounded session tree suspension`
- TO: `### Requirement: Confirmed quit waits for session tree suspension`
