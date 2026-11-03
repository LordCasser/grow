## MODIFIED Requirements

### Requirement: Dashboard preserves mixed drop paths

Dashboard's explicit new-session composer SHALL insert recognized image and ordinary-file drop entries in source order for bracketed paste, paste-key text, and deferred file URL completion. Selection and read-only preview SHALL NOT become clipboard recipients or answer pending interactions. Search query SHALL remain a separate text-only owner.

#### Scenario: Image and ordinary file share a paste
- **WHEN** a recognized batch contains an image and an ordinary file path
- **THEN** attach the image and insert the ordinary path as text in the new-session composer, without dropping that path through image-only filtering.

#### Scenario: Image cap does not suppress file text
- **WHEN** the new-session composer rejects an image because its attachment cap is reached and the batch also contains an ordinary path
- **THEN** keep the existing image rejection feedback and still insert the ordinary path.

#### Scenario: Explicit file URL cannot be loaded as an image
- **WHEN** the shared classifier recognizes an explicit file URL as an ordinary path, including a missing image file
- **THEN** insert that path as text and report successful handling without adding an image attachment.

#### Scenario: Question and stale target guards
- **WHEN** a selected read-only preview has a pending Question or changes while a composer clipboard probe is in flight
- **THEN** completion remains bound to the original new-session composer, never answers the preview's question, and preserves the existing discard rules if that actual composer target became stale; search admits query text without attachments.

### Requirement: Session command prefixes survive dashboard attachment

Pager SHALL route the first Ctrl+X to the session command prefix in ordinary, Dashboard-attached and child details, and in the Dashboard list for its explicitly selected target. Supported model, Agent, effort, permission and Behavior continuations SHALL retain availability and exact target identity. Opening the prefix SHALL NOT cancel execution or arm closure/deletion. A repeated prefix SHALL only stop work on the explicit session target; idle state SHALL NOT upgrade this action to close or permanent deletion. Children SHALL NOT stop or close their parent. Visible hints SHALL describe the actual target and action.

#### Scenario: Configure an attached root
- **WHEN** the user presses Ctrl+X followed by a supported configuration letter in ordinary/attached detail or the Dashboard list with a local selected session
- **THEN** the selector targets that session without changing next-session defaults, cancellation or draft insertion.

#### Scenario: Repeat the prefix to stop or close
- **WHEN** the prefix is repeated on a busy or idle target
- **THEN** busy work uses the existing stop path and idle state never triggers close/delete; closing requires the separate explicit C command and runtime acknowledgement.

#### Scenario: Child configuration
- **WHEN** the target is a child or no configurable local session is selected
- **THEN** the child retains its own scope, or the unavailable operation is explained without applying it to a parent/new-session default.

#### Scenario: Prefix expires or is cancelled
- **WHEN** the prefix expires, is cancelled or completed by another command
- **THEN** it cannot authorize a later stop/close/delete and the next prefix starts a fresh window.

### Requirement: Dashboard stop covers the selected session activity

A confirmed Dashboard Stop session work action on a busy local root SHALL target the activity that makes that row busy, including a foreground-idle root with running ordinary children, an active Goal or a manageable active Workflow. It SHALL cancel ordinary children through the existing session cancellation path, pause an active Goal, and stop active manageable Workflows through their existing host control keyed by run identity. It SHALL preserve unrelated roots and drafts, SHALL NOT enter control text into the model prompt queue, and SHALL wait for authoritative activity updates before allowing deletion. Unavailable controls SHALL NOT be reported as successfully stopped.

#### Scenario: Only background children remain
- **WHEN** the selected root foreground is idle but ordinary children are running and the user confirms Stop session work
- **THEN** the existing cancellation path targets that root and its ordinary children, without arming deletion.

#### Scenario: Active Goal at an idle boundary
- **WHEN** only the selected root's Goal keeps the row Working
- **THEN** the stop action sends its existing pause control and retains the row until authoritative state changes.

#### Scenario: Workflow owns the background work
- **WHEN** a selected root has an active manageable Workflow and an idle foreground
- **THEN** the stop action stops that exact run through host control, preserving the composer and unrelated runs in other roots.

#### Scenario: Work has not settled yet
- **WHEN** cancellation or pause/stop has been requested but the row remains Working
- **THEN** repeated stop cannot confirm deletion or manufacture an idle state.

## ADDED Requirements

### Requirement: Session editing is independent of Dashboard entry

Ordinary and Dashboard-opened session details SHALL use the same AgentView editing, pane navigation, prompt paging and local interaction rules. Dashboard metadata SHALL affect only explicit navigation/chrome. Dashboard SHALL NOT add Esc/Left/q backout ahead of the normal session handler. Minimal mode retains its actual capability limits.

#### Scenario: Open the same session through Dashboard
- **WHEN** an ordinary or Dashboard-opened session receives editing, paging, Esc/Left or interaction input
- **THEN** the same session/child owner and normal behavior apply without changing or discarding its draft.

### Requirement: Dashboard overview does not own implicit reply input

Dashboard selection SHALL drive read-only preview without retargeting an editable reply or answering interactions. Opening/replying/handling pending work SHALL enter the existing session-owned view. New session input SHALL remain explicitly labelled and independent of row selection. Search query and new-session draft SHALL have separate ownership, preserving text, undo and attachments when entering/leaving search. Single-click selection SHALL preview only; Enter or a double click in the list SHALL open details. While the new-session composer owns focus, an empty draft SHALL NOT redirect Enter or arrow input to the selected session.

#### Scenario: Select and search while a new-session draft exists
- **WHEN** the user moves rows or enters/exits search
- **THEN** no draft is erased, submitted or retargeted and search text never becomes a prompt.

#### Scenario: Empty composer and pointer selection
- **WHEN** the composer owns focus with an empty draft, or the user single-clicks a list row
- **THEN** editing does not open the selected session and single-click only selects a read-only preview; list Enter/double-click explicitly opens details.

#### Scenario: Handle a pending interaction
- **WHEN** a selected root/child requires permission, Question or Plan input and the user opens it
- **THEN** its real session interaction handles the exact request; overview selection does not answer or reject it.

### Requirement: Session management actions have explicit independent consequences

Return, Stop session work, Close session and Delete history SHALL remain distinct actions with named target and consequences. Close SHALL retain the existing runtime acknowledgement boundary. Permanent deletion SHALL require a separate target-bound confirmation and retain canonical/writer protections. Prefix timers or state changes SHALL NOT authorize a different action.

#### Scenario: Session settles between stopping and another key
- **WHEN** a busy target becomes idle before a repeated stop key
- **THEN** no close or delete is authorized.

#### Scenario: Confirm deletion while selection changes
- **WHEN** selection or binding changes after opening a deletion confirmation
- **THEN** confirmation cannot delete an unintended replacement or newly selected target.

### Requirement: Session cycling is stable and independent of overview filters

Previous/next session SHALL use stable open-session order, independent of Dashboard filter, grouping, live activity and transient preview. Overview SHALL use stable identity order by default, with state/directory grouping as explicit options; it SHALL distinguish execution activity from source/residency and distinguish keyboard focus, hover and current session. Narrow layouts SHALL preserve target, primary actions and input ownership.

#### Scenario: Current session is hidden by an overview filter
- **WHEN** the user cycles from a session absent from the filtered overview
- **THEN** another open session remains reachable in the stable open-session order.

#### Scenario: Activity changes while navigating
- **WHEN** runtime updates change activity or timestamps
- **THEN** the next/previous session order and focused target remain stable.
