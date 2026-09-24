# Close independent preview queue gaps

## Why

The acknowledged ACP path added for active attempts directly appends its notification without first draining a previously buffered update, leaving an ordering gap in that command. One-shot Grow notifications emitted by the session actor during an attempt still enter unbounded persistence and gateway queues without the preview byte budget. This leaves a payload-bearing actor route outside the attempt boundary.

## What Changes

Drain pending persistence work before an acknowledged independent update. Use the same acknowledged command for one-shot Grow notifications from the session actor during an active attempt, and charge its active-attempt Grow gateway notifications to the per-session preview budget until gateway completion. A failed append or gateway reservation marks the attempt failed before canonical admission.
