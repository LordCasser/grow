# Design

`Timeline::apply_validated_event` is the common apply path for live `accept` and replay `accept_replayed`. Add a private `last_rewind_seq: Option<EventSeq>` field and a crate-visible read accessor. Update it in that common apply path for a validated `Messages` event whose cause is `Rewind`, immediately before the event is appended to the canonical event vector. No other event clears or changes it.

`prepare` and failed `accept` do not call the apply path, so they cannot publish an anchor. The actor's durable commit path persists before calling `Timeline::accept`; failed or cancelled persistence therefore leaves the projection untouched. `Timeline::from_events` reconstructs the projection through the same replay apply path. The field is derived from the event log and is neither serialized nor independently persisted.

`prompt_cache_key` formats the getter's `EventSeq::get()` as the same decimal string used by the existing reverse scan, or retains `root` when the getter is `None`. The rest of key composition is unchanged.
