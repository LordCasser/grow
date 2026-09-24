## ADDED Requirements

### Requirement: Cold replay paint work is bounded by a visible progress cadence
While a visible session is replaying historical notifications, Pager SHALL apply a minimum 100 ms interval between automatically requested paints from ACP replay, animation deadlines, and periodic UI maintenance, unless the configured interval is slower. Explicit user input and other direct UI actions SHALL retain their existing immediate redraw behavior. When the session-loaded boundary arrives, Pager SHALL return to the normal configured cadence and show the final replay state and prompt without waiting for another replay interval. The paint policy SHALL NOT drop notifications, reorder replay, or admit a prompt before the existing load barrier.

#### Scenario: Long history streams faster than painting
- **WHEN** a visible cold session receives many historical notifications while `loading_replay` is true
- **THEN** automatic ACP, animation, and periodic maintenance paints use at least a 100 ms interval while notifications continue to be processed in order.

#### Scenario: User types during replay
- **WHEN** terminal input arrives during a replay interval
- **THEN** its existing immediate handling and redraw are not delayed by the automatic paint cadence, and the draft remains until the load barrier permits submission.

#### Scenario: Replay completes
- **WHEN** the session-loaded boundary ends `loading_replay`
- **THEN** the final history and prompt use the normal configured paint cadence rather than waiting for a pending replay-only interval.
