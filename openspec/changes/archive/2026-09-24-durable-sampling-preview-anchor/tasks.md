# Tasks

- [x] Verify every producer and reader of candidate markers, untagged updates, and projection replay.
- [x] Replace in-memory untagged staging with one durable payload-free anchor and immediate independent updates.
- [x] Gate Timeline admission on an ordered persistence barrier and fail closed on append errors.
- [x] Bound/coalesce candidate marker and ordinary merged-notification traffic.
- [x] Run focused persistence, actor, replay, and Shell tests; update docs/backlog and archive.
