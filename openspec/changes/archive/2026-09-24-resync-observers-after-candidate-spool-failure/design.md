## Decision

Candidate failure poisons only the active attempt's transient delivery to non-retracting observers. Its spool is dropped immediately. Lifecycle-capable clients continue receiving the attempt and its terminal boundary. Independent notifications keep their live route. The terminal Accepted boundary is already downstream of durable admission, so a targeted resync may safely reconstruct the response from `session/load`; Discarded needs no reconstruction. The leader retains client attachment and session routing throughout.

The resync request is a leader-origin ACP extension notification carrying the affected `sessionId`, not a model-visible session fact. The leader delays it behind an in-flight `session/load` for that observer. Pager resolves a child session to its owning root, opens its existing reload window, requests a full replay without a cursor, then finalizes or rolls back that window based on the load result. A later transport reconnect supersedes the local resync window by generation; stale completion cannot change the replacement view.

If the spool read fails after Accepted has begun streaming, the same full replay replaces any partial presentation. A newly attached observer relies on its own load; no transient candidate is backfilled.
