## Row identity and order

Keep the existing `(source_peer_id, inquiry_id)` key. Derive phase from the structured audit: outcome means terminal, otherwise approval means approved, otherwise received. The phase belongs to the passive row, not to the generic running/finished tool state used to render its chrome. A lower phase cannot replace a higher phase, and a replay of the same phase cannot overwrite a row already present. A live same-phase update may still refresh details. Terminal remains absorbing.

The existing tail merge calls the same upsert, so cursor/full reloads obey the same order. The start time, manual fold and committed row ID remain attached to the original entry. Separate peers, including parent and child, do not share an entry even with the same inquiry ID.

## Verification

Add state and handler tests for approval-before-start, completion-before-approval, replay and peer interleaving; run focused Pager tests plus type check and OpenSpec validation. No new durable event or protocol field is needed.
