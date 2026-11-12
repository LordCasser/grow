# Release candidate verification

- Locked metadata resolves v2.6.7 for all 33 workspace-versioned packages. The lockfile changes those packages' versions only. The default CLI build and CLI check pass on Rust 1.93.1.
- Both real PTY release checks pass against the candidate grow binary. The command settled interrupted and displayed 3.6s in Tasks history after cold resume. The restarted Goal automatically completed. The old binary reproduced a successful restart notice with no automatic continuation.
- The existing Goal sampling failure, explicit restart and budget gate process test passes against the candidate. The recent observation and recovery changes passed 721 targeted and ChatState tests before the version bump.
- The versioned changelog, Shell changelog and user guides reflect the release source. Both PTY checks are selected by the core release regression workflow.

- Both default CLI and workspace all-targets checks pass with the locked dependencies. The workspace check emitted existing Shell warnings.
- The release workflow verifies the annotated tag against the exact source commit and gates publication on core regression, native coordination and export, Windows storage and OpenSpec.
