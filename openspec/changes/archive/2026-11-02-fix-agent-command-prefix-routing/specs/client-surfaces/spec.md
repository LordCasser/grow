## ADDED Requirements

### Requirement: Session command prefixes survive dashboard attachment

Pager SHALL route the first Ctrl+X in an execution detail view to the current AgentView command prefix, including dashboard-attached roots, session cycling and child views. The existing model, Agent, effort, permission and Behavior continuations SHALL retain their current availability and target identity. Opening the prefix SHALL NOT cancel execution or arm a separate close timer. An attached root's second Ctrl+X within the existing prefix window SHALL cancel an active turn or confirm closing an idle detail view. A child prefix SHALL NOT stop or close its parent. Dashboard list Ctrl+X and explicitly rebound overlay-stop shortcuts SHALL keep their own contextual behavior. Visible hints SHALL describe the action that receives the displayed key.

#### Scenario: Configure an attached root
- **WHEN** the user presses Ctrl+X followed by a supported configuration letter in a Dashboard-attached session, whether idle or running
- **THEN** the current session's selector opens without cancellation or closure, and the letter does not enter the draft.

#### Scenario: Repeat the prefix to stop or close
- **WHEN** an attached root receives two Ctrl+X presses within the existing prefix window
- **THEN** an active turn uses the existing cancellation action without closing; an idle root dispatches the confirmed overlay close action and clears its prefix.

#### Scenario: Child configuration
- **WHEN** a child view receives Ctrl+X and a supported continuation
- **THEN** the child handles its own configuration scope; the parent's stop/close shortcut cannot consume it.

#### Scenario: Prefix expires or is cancelled
- **WHEN** a prefix expires, is cancelled, or is completed by another command
- **THEN** it cannot later authorize root closure; the next prefix starts a fresh window.
