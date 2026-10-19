# Verification

## Counterexample and scope

Official candidate f497f6a10b1c25404980513732eb790c7054ccb6 run 39280878575 exposed the same eleven first-party diagnostics in both Windows x86_64 and aarch64 release builds: unused handle import; retained parent anchor handle; Unix-only DirBuilder mutability; terminal closure mutability and login-shell field; unsupported-platform CWD scan helpers/state; Linux-only clipboard process imports and two overwritten optional backend error initializers. macOS default build did not compile those branches. Publication was cancelled and confirmed completed/cancelled before a release existed; gh release view v2.4.1 reports release not found. That candidate is not a released product.

Changes preserve all runtime branch results. ParentAnchor still opens and retains its handle, including its existing failure behavior. Clipboard backend success/empty/error matches retain the Linux guarded empty-result fallback and original error propagation. The shared GC failed state is preserved for matching and portable fail-closed tests; only enumerator helper compilation narrows to supported platforms/tests. Windows release Rust warnings are now errors.

## Third-party diagnostics (not silently suppressed)

The cancelled candidate GCC builds also emit sqlite-vec C warnings; musl Rust builds use an existing deprecated nix libc alias; installing cross 0.2.5 --locked reports its own yanked dunce 1.0.2 (Grow uses 1.0.5). Vendor source/build scripts and cross pin are unchanged from v2.4.0. sqlite-vec text metadata IN has an invalid unsigned sentinel check; Grow's current schema/search use chunk_id/embedding and MATCH/k, not that metadata path. No runtime failure or blanket harmlessness claim is established. Separate debt is recorded in backlog; no vendor upgrade/suppression or MCP change enters this maintenance change.

## Validation

Formatting, local targeted/default CLI and hosted replacement-source results are recorded below as they complete. No previous-source artifact or green gate substitutes for the corrected source.
