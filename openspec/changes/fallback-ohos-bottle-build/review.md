# Release repair review

The release agent correctly synchronized `Cargo.lock` after the first locked build failed, kept the OHOS core and Rust version pinned, and used unpublished OHOS-only runs to isolate the bootstrap failure. The release is not published; Linux, macOS, and Windows have not yet passed this v2.2.0 release workflow.

The first two OHOS repairs were incomplete. Homebrew's `--build-from-source` applies to the requested formula while dependencies can still use bottles. Run `36021539972` demonstrated this directly: the command requested source builds, but `cmake` and `patchelf` were poured from missing bottle URLs. The third repair added those observed names to one command but retained the same dependency-resolution behavior and did not prove coverage for future transitive dependencies.

The corrected bootstrap resolves the complete pinned Rust dependency closure, including build and implicit dependencies, and installs it in topological order one formula per source-build command before Rust. The documentation and workflow comment previously still described a Rust bottle; they now match the bootstrap. The full OHOS and all-platform release jobs remain the acceptance gate before publication.
