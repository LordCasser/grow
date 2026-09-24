## Decision

Close this backlog item as an optional future capability. The current ACP reverse bridge is intentionally half-duplex: it forwards requests carrying JSON-RPC ids and their responses, while dropping id-less notifications. `notifications/cancelled` is id-less, so it cannot be delivered by this bridge today.

The canonical `Abandoned MCP calls notify their originating service` requirement scopes delivery to stdio and Streamable HTTP and has an `ACP reverse bridge limitation` scenario. That scenario already defines the required behavior: Grow must not claim the ACP-hosted remote server received cancellation. The developer guide repeats this boundary. No correctness claim in the active contract is unmet, so no bridge implementation or remote-delivery claim is warranted.

Remove only the backlog watch. A future request to support cancellation over ACP must be evaluated as a separate behavior change after ACP and MCP peers can carry a correlated server-to-client notification and an end-to-end test can prove delivery. A local timeout or host-side cancellation alone is not remote delivery.
