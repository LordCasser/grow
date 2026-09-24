# Design

The child emitter stamps one immutable event ID and sends one parent command without a separate direct gateway copy. The parent actor appends the event with its existing independent-update acknowledgement and then forwards that event through the already budgeted gateway path. External client-origin Grow commands remain persist-only to avoid echoing them.

`SubagentFinished` carries status and accounting metadata, with no copied final output in the parent Grow projection. The final output stays in the validated child result artifact and the parent's completion receipt. Recovery emits the same metadata-only lifecycle projection. The actor's existing live-preview credit failure closes active-attempt admission, and reconnect can reconstruct a skipped lifecycle projection from canonical lifecycle facts.
