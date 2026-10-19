## Reclaim unit

The reclaim unit is the connected set of session directories joined by canonical `Subagent::Spawned` / `SubagentSeed` / `SubagentResult` / `Subagent::Ended` references. Discovery remains bounded by the existing session and dependency scan budgets. A malformed edge, missing endpoint, duplicate or ambiguous identity, unresolved completion receipt, or over-budget traversal makes the component ineligible.

Before deciding eligibility, collect the complete component and sort members by stable storage identity (`cwd`, then session ID). Acquire every member's existing exclusive writer lease in that order and keep all leases through the durable decision and deletion. Reopen each entity and revalidate directory identity, summary, activity cutoff, Timeline, exact edge projections, and local recovery dependencies. If any member is fresh, hidden-excluded, active, unreadable, changed, or has another recovery obligation, release the leases and retain the entire component.

## Durable decision and restart

Per-session quarantine is not group-atomic: a crash between two quarantine renames can leave one endpoint deleted while another still points to it. Therefore no member is removed until a bounded durable reclaim intent containing the complete sorted member identities and storage locations has been written and synced. This intent represents the irreversible decision that the whole validated component is reclaimable.

Cleanup first claims and recovers any existing intent before normal session discovery. It validates the full bounded member list before allowing any session admission, then claims every member writer lease in stable order. It only continues removal for listed members whose storage identity matches the intent, using the existing contained-directory and per-session quarantine checks. A member absent from both its canonical and deterministic quarantine names is considered already removed because the durable whole-component intent itself authorized deletion; no separate per-member tombstone is needed. Identity mismatch, an active writer, malformed intent, or an unprovable state leaves the intent and related sessions retained for repair. The intent is removed and its parent directory synced only after every member is confirmed absent. Fresh writes or session admission for a member named in an unresolved intent must fail closed until recovery completes.

This is a small local storage record, not a second session ledger: it records only the member set of a committed deletion decision, not session lifecycle or result data. It does not promise all-or-nothing physical directory removal; it makes an interrupted committed removal resumable without admitting the surviving half as a valid session.

## Explicit single-session deletion

Explicit `grow/session/delete` remains a single-session operation and does not recursively delete a session tree. Under the target writer lease, it validates the target Timeline and any directly referenced parent or child endpoints. If any cross-session result-chain edge exists, it returns an actionable refusal naming the related session IDs. It does not create a reclaim intent. Sessions without such an edge continue through the existing single-session quarantine path.

## Safety properties

- Never treat missing source evidence as proof of completion or deletion eligibility.
- Never drop a sibling writer lease between component validation and the durable reclaim decision.
- Acquire leases in stable order and retain them until the reclaim intent is durable or the operation aborts.
- Revalidate the same filesystem entities and all edge projections immediately before writing the intent.
- Never automatically recover a corrupt or ambiguous intent by guessing member identity.
- Keep standalone worktree GC's existing `Subagent` skip; this change does not grant it session authority.
