# Verification

## Local candidate

- Last public release is v2.6.16. The remote v2.6.17 reference was absent at candidate preparation. The new workspace version, lockfile and changelog all identify v2.6.17.
- Pinned Rust 1.93.1. Full Shell, ChatState and Sampler libraries passed 5,853 tests in total, with 17 existing ignored tests. The scoped pressure, admission, cold storage and response usage groups passed separately. The independent architecture review and its real indexed-load regression are recorded with the context-recovery change.
- Rustfmt, default CLI check and build, workspace all-targets check, and release-dist profile/feature with `-D warnings` all passed. Existing test-only warnings remained visible in all-targets.
