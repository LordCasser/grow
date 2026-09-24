# Release repair review

The release agent correctly synchronized `Cargo.lock` after the first locked build failed, kept the OHOS core and Rust version pinned, and used unpublished OHOS-only runs to isolate the bootstrap failure. Its first full matrix exposed independent Windows compilation and macOS ARM64 smoke-test failures; those were repaired in separate changes. The release is not published.

The first two OHOS repairs were incomplete. Homebrew's `--build-from-source` applies to the requested formula while dependencies can still use bottles. Run `36021539972` demonstrated this directly: the command requested source builds, but `cmake` and `patchelf` were poured from missing bottle URLs. The third repair added those observed names and did pass bootstrap for the current pinned formulas: run `36025406330` reached `cargo build` before it was canceled to unblock the corrected full matrix. Its hand-maintained formula list still did not cover future transitive dependency changes.

The corrected bootstrap resolves the complete pinned Rust dependency closure, including build and implicit dependencies, and installs it in topological order one formula per source-build command before Rust. The documentation and workflow comment previously still described a Rust bottle; they now match the bootstrap. The corrected full matrix passed all ten platform asset jobs before publication.
