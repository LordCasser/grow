# Design

The detached worker will select, with receiver closure biased ahead of work completion, between `respond_to.closed()` and `handle_side_question`. Closing the receiver drops the in-flight future. Before Sideband admission this ends the question without creating a Sideband; after admission, dropping `SidebandRun` uses its existing cleanup path to settle admitted usage and durably finish the interrupted Sideband. Session-wide cancellation remains unchanged and is still observed by the Sideband/provider path.

No new cancellation token or owner registry is needed: the response receiver already expresses whether the caller still owns the answer, and the Sideband RAII object already owns cleanup for admitted work.
