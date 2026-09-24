# Design

Represent an active sampling staging window as its attempt key, an optional count of untagged notifications preceding the first candidate, and an ordered vector of untagged notifications. Candidate notifications are dropped immediately. The count is the insertion index for the canonical projection and stays `None` when no candidate arrived.

With no projection, accepted notification boundaries remain notification-only and do not make candidate text durable; discard, supersession, and stop drop candidate text and flush staged untagged events in their original order. With a projection, append untagged events before the anchor, commit the canonical response projection, then append the remaining untagged events. A window with no candidate writes all untagged events before the projection.

Memory remains proportional to untagged events, which are needed to preserve external event order. This change bounds the candidate-specific portion to constant size.
