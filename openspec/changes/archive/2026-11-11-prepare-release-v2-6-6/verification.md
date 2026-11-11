# Review and release preparation

- The review found no release blocker in the consultation change. The Timeline checks the consumed receipt ID, both session IDs, inquiry ID, reply reference and payload hash. The reply authorization path still consults the original send receipts. The provider adapters use the existing runtime-context projection, which preserves pending tool pairing.
- The pinned Shell test executable passed 84 coordination and message-delivery tests. The consultation change also passed the independent Grow process regression using the build before the version-only bump. [Consultation verification](../2026-11-11-expose-answering-agent-consultation/verification.md)
- Cargo 1.93.1 resolved locked metadata for 57 workspace packages. All 33 packages inheriting the workspace version resolve to 2.6.6. The lockfile diff contains only their version changes.
- `scripts/validate-release.sh v2.6.6` passed against a clean temporary snapshot and an annotated tag. The official workflow requires the final remote tag and runs the gates against its exact commit.
- Release contract and build-identity tests passed 12 cases. The OHOS smoke fixture was skipped on macOS because it requires the aarch64 musl container.
- The remote baseline was v2.6.5 with 11 published assets. The remote had no v2.6.6 tag when checked.
- Cargo 1.93.1 passed `cargo check --locked -p cli --bin grow` and `cargo check --locked --workspace --all-targets`. The all-target check emitted warnings in three existing Shell test files.
- Rust formatting passed for changed files. The workspace-wide formatting check has two pre-existing differences documented in the consultation verification.
