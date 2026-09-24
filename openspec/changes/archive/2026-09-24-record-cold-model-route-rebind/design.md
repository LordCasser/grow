# Design

`model_change_event` already stores catalog model ID, reasoning effort, provider-facing wire model, and `ModelImageInputKey` (backend plus a hash of endpoint/query). The fold validates all four fields but currently returns only the first two. Return the full validated selection to its two callers; `summary.json` remains a projection of ID/effort.

Fresh actor initialization writes a self-transition with reason `session_start_route_baseline` after its initial context is durable and before publishing the actor. This is a baseline, not a user switch. The same ChatState owner allocates the event sequence and persists it, so it cannot collide with fresh context events.

Cold load holds the replacement writer lease. After selecting the current catalog entry and overlaying the saved effort, compare its secret-free route key with the final durable observation. If it differs, record `cold_load_catalog_rebind` from the exact stored identity to the captured current identity, append the event through the owned persistence actor, and add it to the in-memory Timeline passed to the new actor. A crash or lost acknowledgement after a physical append is resolved by rereading Timeline on retry; the next fold sees the new route and does not append a duplicate. Failed append prevents actor publication. A route-free historical Timeline gets a current-route self-transition marked `cold_load_route_baseline`; its earlier endpoint is unknown and is not inferred.

The route hash and model identity remain diagnostic facts, never credentials or reconstructable endpoint data. No special case is added to the strict fold: a genuinely discontinuous stored transition still rejects load.
