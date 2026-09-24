## Decision

Treat loss of an unconfirmed candidate prefix during reconnect as an intentional current boundary. The `Reconnect misses the candidate terminal boundary` scenario in `client-surfaces` requires dropping an unconfirmed preview and showing only admitted history. The archived `unify-sampling-attempt-recovery` design also explicitly excludes a protocol for resending active candidate prefixes. Remove that conditional UX item from the debt list.

Keep local-draft restart semantics narrowly stated. `Local draft invalidation retries without restoring stale content` is scoped to a running Pager; its verification records that in-memory deletion intent cannot survive process death. A sustained deletion failure followed by process exit can leave a valid stale draft for startup recovery, so retain this low-frequency reliability boundary.

Keep ACP bridge cancellation delivery as a separate item. The current extension-runtime contract limits guaranteed cancellation notification delivery to stdio/Streamable HTTP and explicitly documents the ACP reverse bridge limitation. The bridge drops id-less notifications, including `notifications/cancelled`.
