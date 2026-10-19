## Context

Suspension needs a canonical child spawn/terminal snapshot and must preserve the existing deadline, exact latest-fact rereads, and all validation. `runtime_snapshot_before` and direct `timeline_events()` callers already return owned event vectors. `resolve_runtime_load_root` receives `root.timeline` from `load_light`; the JSONL loader calls `opened.validated_timeline()` before returning that Timeline.

## Decisions

`CanonicalChildFacts::read` will take `Vec<TimelineEvent>`, move it into `Timeline::from_events`, and extract facts from `validated.events()`. A separate internal `from_validated_timeline(&Timeline)` will extract facts from an existing trusted Timeline; only the cold-load caller uses it. This avoids a clone and a second replay without adding a cache or skipping the loader's validation. Both entry points use the same child relationship checks.

Move owned vectors at all live suspension call sites. Keep the existing post-retirement latest facts reread and its deadline unchanged. No helper will retain history after extraction.

The ignored probe compares child fact values and elapsed time for the previous `to_vec` + Timeline replay + original-slice scan against the owned candidate on 8,000 4 KiB observations plus one spawn/terminal pair. It is a local cost measurement, not a CI threshold or an application-level speed claim.

## Validation risks

The cold-load shortcut depends on `load_light` continuing to return a validated Timeline. The call site and loader are explicitly checked together. Tests must continue rejecting duplicate spawn, mismatched terminal child identity, and malformed event sequence before a handoff can use child facts.
