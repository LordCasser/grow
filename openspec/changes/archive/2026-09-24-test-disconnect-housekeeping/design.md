# Design

In `evict_sessions_notification_on_disconnect`, receive ACP messages until the `grow/internal/evict_sessions` notification arrives, allowing the existing `grow/internal/queue_client_disconnected` housekeeping message to be interleaved. Keep the assertion that the eviction payload contains the detached session.

In `driver_disconnect_transfers_not_evicts`, consume and validate the expected queue-client-disconnected notification, then assert that no eviction notification arrives during the bounded observation window. Preserve the reverse-request check proving the remaining subscriber became driver.

No production code, behavior, protocol, or persistence changes are included.
