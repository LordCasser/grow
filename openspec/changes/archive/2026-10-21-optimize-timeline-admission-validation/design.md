## Context and invariants

`Timeline::from_events` validates and applies events to a private fold. Live `prepare` and `accept` validate before publishing an event, so any added validation index must remain untouched on rejection. Response admission is valid only for an assistant append with a nonempty identity whose exact request attempt has completed, whose identity has not already been admitted, and whose stored quarantine count matches the candidate Surface.

The active Surface is branch-local. Rewind and replacement can remove or replace current items while immutable response identities and request facts remain in the event history. Compaction replaces only its selected Surface range. Event-history admission indexes and any active-Surface identity summary therefore have different ownership and update rules.

## Design

- Keep admission validation state inside the Timeline fold. Record the latest completed attempt by request ID and seen response-admission identities only as accepted events are applied. The latest-attempt rule must match the existing reverse scan, including a newer completion for the same request that supersedes an earlier attempt.
- Replace candidate Surface cloning and quarantine serialization with a read-only count that uses the same malformed identity rules. Preserve the distinction between malformed ranges and malformed calls, including duplicate IDs, blank IDs or names, and the adjacent reasoning/backend-call and tool-result boundaries used to form a range.
- The baseline grows from a 16.7 ms median at 512 admissions to 337.2 ms at 2048, so the repeated healthy-append Surface scan is material. Keep only the active Surface's valid tool IDs and malformed-assistant count; validate each appended response against those IDs plus a local set, and rebuild after a Surface replacement. Do not reuse the lifecycle fold's historical tool set as active Surface state.
- Rewind, replacement, and compaction continue to be validated against current Surface IDs and then applied through the existing Surface operation. No generic rollback framework is introduced.
- Keep replay failures private to the cold fold. In the live path, indexes change only after validation succeeds, so a rejected or durably unconfirmed append cannot poison a retry.

## Boundary fixture note

The compaction/rewind regression uses the legacy quarantine operation as its independent count oracle. A rewind rebuilds the active Surface from the uncompressed branch, so malformed assistant history hidden by a compaction summary can become active again. The post-rewind candidate's expected quarantine count is therefore 2 for that fixture; this adjusts the fixture expectation and does not change the admission contract. The focused case and full chat-state regression subsequently passed; see verification.md.

## Verification approach

Use an ignored benchmark with legal identity-bearing responses at 128, 512, and 2048 admissions. Generate fixtures outside each timing window; time only cold `Timeline::from_events`. Check the restored Surface against the live fold and check representative admission-validation error outcomes. After implementation, run the same probe under the same profile and compare all sizes, then run focused semantic checks for duplicate identity, attempt mismatch, quarantine count, failed live admission, and rewind/replacement/compaction behavior.
