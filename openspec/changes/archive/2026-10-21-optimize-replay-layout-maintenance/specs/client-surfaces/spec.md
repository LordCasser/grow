## MODIFIED Requirements

### Requirement: Cold replay paint work is bounded by a visible progress cadence
While a visible session is replaying historical notifications, Pager SHALL apply a minimum 100 ms interval between automatically requested paints from ACP replay, animation deadlines, and periodic UI maintenance, unless the configured interval is slower. History SHALL be consumed in order as quickly as processing allows and SHALL become visible at the eligible paint boundaries without simulated playback delays. During the visible session's loading window, ordinary keys, paste and mouse input SHALL NOT edit the composer, submit or queue prompts, invoke commands or change the followed scroll position. The existing global Quit shortcut (Ctrl+D by default) SHALL remain available through its existing confirmation and suspension path, and terminal resize SHALL continue to update layout. An existing draft SHALL survive the loading window. When the session-loaded boundary arrives, Pager SHALL return to normal input and configured paint cadence and show the final replay state and prompt without waiting for another replay interval. The paint policy SHALL NOT drop notifications, reorder replay, or admit a prompt before the existing load barrier.

#### Scenario: Long history streams faster than painting
- **WHEN** a visible cold session receives many historical notifications while `loading_replay` is true
- **THEN** notifications continue to be processed in order without playback delays and eligible automatic paints show the advancing history with at least a 100 ms interval.

#### Scenario: User types during replay
- **WHEN** ordinary keys, Enter, paste or mouse input arrive while the visible session is loading
- **THEN** the input is ignored without editing an existing draft, queuing a prompt, dispatching a command or changing the followed history position.

#### Scenario: User quits or resizes during replay
- **WHEN** the user presses the global Quit shortcut during loading or the terminal is resized
- **THEN** Quit uses its existing confirmation and suspension path, and resize updates the layout without enabling ordinary input.

#### Scenario: Replay completes
- **WHEN** the session-loaded boundary ends `loading_replay`
- **THEN** the final history and preserved draft use the normal configured paint cadence rather than waiting for a pending replay-only interval, and ordinary input becomes available again.
