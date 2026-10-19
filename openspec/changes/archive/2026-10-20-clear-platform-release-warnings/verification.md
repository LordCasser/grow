# Verification

## Counterexample and scope

Official candidate f497f6a10b1c25404980513732eb790c7054ccb6 run 39280878575 exposed the same eleven first-party diagnostics in both Windows x86_64 and aarch64 release builds: unused handle import; retained parent anchor handle; Unix-only DirBuilder mutability; terminal closure mutability and login-shell field; unsupported-platform CWD scan helpers/state; Linux-only clipboard process imports and two overwritten optional backend error initializers. macOS default build did not compile those branches. Publication was cancelled and confirmed completed/cancelled before a release existed; gh release view v2.4.1 reports release not found. That candidate is not a released product.

Changes preserve all runtime branch results. ParentAnchor still opens and retains its handle, including its existing failure behavior. Clipboard backend success/empty/error matches retain the Linux guarded empty-result fallback and original error propagation. The shared GC failed state is preserved for matching and portable fail-closed tests; only enumerator helper compilation narrows to supported platforms/tests. Windows release Rust warnings are now errors.

## Third-party diagnostics (not silently suppressed)

The cancelled candidate GCC builds also emit sqlite-vec C warnings; musl Rust builds use an existing deprecated nix libc alias; installing cross 0.2.5 --locked reports its own yanked dunce 1.0.2 (Grow uses 1.0.5). Vendor source/build scripts and cross pin are unchanged from v2.4.0. sqlite-vec text metadata IN has an invalid unsigned sentinel check; Grow's current schema/search use chunk_id/embedding and MATCH/k, not that metadata path. No runtime failure or blanket harmlessness claim is established. Separate debt is recorded in backlog; no vendor upgrade/suppression or MCP change enters this maintenance change.

## Validation

Formatting, local targeted/default CLI and hosted replacement-source results are recorded below as they complete. No previous-source artifact or green gate substitutes for the corrected source.

Local validation: pinned Rust 1.93.1 default CLI build succeeded without warnings; managed_text 34 passed, fast-worktree with metadata enabled and gc filter 67 passed, unified_log 31 passed. The initial fast-worktree filter without metadata selected zero cases and is explicitly not counted as evidence; rerun with the real production feature executes the CWD scan guards. Python workflow/runner regressions: 19 passed. Changed-source rustfmt and whitespace checks pass; strict current/change validation: 17 passed.

Corrected main source 5fcbd12b0c10afef1f34695394d2ad81ff5b5310 is pushed. Old run is completed/cancelled with no public release. The unpublished annotated tag was replaced only after checking its expected old remote object (force-with-lease); tag preflight validates the new exact SHA and official run 39290950832 rebuilds all ten platforms with all release gates enabled. No public asset/tag is replaced.

Hosted corrected-source proof: official run 39290950832 Windows aarch64 job 118191312608 and Windows x86_64 job 118191312635 both completed successfully. Complete logs confirm RUSTFLAGS=-D warnings with zero warning/error diagnostics; each job also passes its native version/process coordination smoke, packaging, attestation and upload steps. The independently reviewed resource/failure boundaries are preserved. This closes the platform-warning maintenance change; the overall release still depends on the remaining official gates.
