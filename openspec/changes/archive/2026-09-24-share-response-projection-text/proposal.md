# Share canonical text while preparing response replay projections

## Why

After Timeline admission, the Shell projection builder copies each assistant and visible-reasoning body into an owned ACP notification before committing the replay cache. A large accepted response is already resident in the Timeline and its durable evidence, so this extra full-body allocation raises the single-attempt peak. The replay cache must still carry content because a resident delta can include its projection before the snapshot Timeline knows the admitted response.

## What Changes

Represent replay text in the projection with shared canonical strings and a small channel tag. Bump the projection record version and create ACP notifications only when a replay or fork actually emits them. Keep digest, identity, disposition, durable exact-match behavior, and UI order unchanged. Measure allocation sharing directly and verify cold, resident, and fork replay.
