# Verification
- main; locked/offline shell --lib tests, incremental/dev-debug/test-debug off, jobs 2, RUST_MIN_STACK=16777216.
- title_route_tests: 3 passed, 0 failed, 0 ignored, 0.00 s. Deterministic state sequences cover claim exclusivity, revoke while claimed then restore/finish, valid retry restoration surviving worker finish, completed claim remaining closed, revocation before claim and after restoration, and initially absent route.
- title_source_tests: 2 passed, 0 failed, 0 ignored, 0.00 s.
- session::helpers::session_title::tests: 15 passed, 0 failed, 0 ignored, 0.00 s.
- All actor slot reads/writes inspected: startup conversion, shared scheduler, eight failure restores, worker finish and manual command revoke. Both direct-command and normal scheduling use shared claim logic.
- No full actor/provider fault injection or actual manual rename during network request was executed. State transition tests exercise the real lifecycle type with an initialized local client but make no provider calls. Canonical title priority and pre-commit revocation semantics are unchanged.
- Existing linker large __eh_frame compact-unwind warning observed; all selected tests passed. No user-state mutation or Windows run.
- Strict all 16 / archived 236 passed. cargo clean removed 7,372 files / 2.7 GiB; final available disk 65 GiB.
