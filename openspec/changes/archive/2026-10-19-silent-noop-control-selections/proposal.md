# Keep no-op Agent and model selections quiet

### Why

Selecting the already active Agent or provider/model currently produces a success notice that says it switched, even though the active selection did not change. This makes real Agent, model, and effort changes harder to distinguish from repeated selection.

### What Changes

- Suppress the success notice for applied Agent and Sampling controls that leave the effective selection unchanged.
- Keep durable control receipts and state projection so retries and reconnects still settle correctly.
- Continue showing success for an Agent runtime replacement, a provider-qualified model change, or a reasoning-effort change.

## Scope

Agent no-op comparison remains at the existing application boundary, where the fully built runtime profile and role prompt can be compared. Sampling model identity is the provider-qualified `ModelId`; reasoning effort is a separate selection attribute carried with the same Sampling control. Unchanged pending targets are omitted from the compact progress status, and unchanged applied controls carry no success notice.
