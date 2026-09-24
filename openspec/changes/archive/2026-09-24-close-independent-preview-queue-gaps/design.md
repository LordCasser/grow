# Design

`PreviewIndependent` carries the existing `SessionUpdate` enum. The persistence actor drains its one pending merged notification before processing the command, then writes ACP notifications with exact event-ID semantics and Grow notifications with the existing durable append path. It records any failure in the active attempt and acknowledges only after the write result. The session actor awaits this acknowledgement for both ACP and one-shot Grow emissions while preview is active; outside an attempt their ordinary path remains unchanged.

All Grow notifications routed through buffered or one-shot forwarding consult the active preview identity. They reserve serialized gateway bytes without a second serialized copy, hold credits until gateway completion, and send an ordered failure marker on reservation error. Lifecycle control remains payload-small; the same budget gives a single uniform rule for any Grow payload in an attempt.
