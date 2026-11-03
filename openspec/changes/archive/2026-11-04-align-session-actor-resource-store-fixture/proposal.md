# Align the Shell actor resource-store fixture

## Why

Shell actor tests currently construct each test Agent with `ResourcesPersistence::local`, while production Shell actors use `SessionResourcesStateStore` backed by a pinned `ContainedDirectory`. These tests are running the non-session Local store under a session host. Its strict parent-directory durability barrier is not the Windows behavior of the production session store, so a post-publication `AccessDenied` is a fixture-host mismatch rather than an image-projection failure.

`LocalResourcesStateStore` remains a strict non-session store: a failed parent-directory barrier after publication is surfaced as `PublishedPersistenceError`; its behavior must not be weakened to make Shell tests pass.

## What Changes

Make the shared Shell actor test Agent fixture use the same pinned session-directory resource store as production. Preserve real file synchronization, atomic publication, durable acknowledgements, and the LocalResourcesStateStore contract. This changes no product behavior.
