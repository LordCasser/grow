# Design

`ModelChanged` arrives before the applied Sampling receipt. For an effort-only local intent, Pager deliberately parks the model event until control correlation settles. The receipt handler already resolves the exact local intent, but only the RPC-result path drains parked model/Agent projections. A successful RPC reports `AuthoritativeUpdatePending`, so that path returns without draining. The visible model stays stale.

After `apply_control_state_update` resolves an exact local intent, call the existing `apply_deferred_authoritative_controls` helper before returning the notification's changed flag. This one notification path serves root and child views. It releases each domain independently; an unrelated pending Agent control cannot hold a resolved Sampling projection. The helper applies only an authoritative `ModelChanged` already received and still checks catalog availability. Duplicate or foreign terminal receipts do not resolve an intent and therefore cannot release a projection.

The regression sends a `ModelChanged` for the same model with a new effort while an effort patch is pending, then sends the matching applied control state. It asserts that the footer's model state catches up without waiting for a later RPC or turn completion. A second pending domain verifies independent release.
