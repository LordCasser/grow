## Why

`CanonicalChildFacts::read` replays a complete Timeline from `events.to_vec()` and then scans the original event slice. Several callers already own the event vector returned by a runtime query, so this performs an avoidable clone of a potentially long history. A cold-load caller also already holds a Timeline validated by `load_light` but currently clones and replays its events again.

## What Changes

- Let the event-based reader consume owned events, replay them through `Timeline::from_events`, then scan the validated Timeline's events.
- For the cold-load caller, scan the Timeline returned by `load_light`, whose loader has already validated the stored Timeline; preserve the child spawn/terminal consistency checks.
- Add malformed-history regressions and an ignored local measurement probe comparing the old borrowed clone/replay path with the owned candidate on 8,000 4 KiB observations.

## Contract Impact

No behavior or persisted contract changes. Invalid Timeline history and inconsistent child facts remain rejected; the change only removes a redundant copy and, for the cold-load path, avoids revalidating an already validated Timeline.
