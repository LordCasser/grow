# Design

Use the existing Rust test-runner environment switch consistently across the three regression workflows. Keep the application's explicitly sized debug/release session threads separate from Rust test threads. No production future layout, release stack capacity, assertions or test cases change.

# Failure evidence

GitHub run 34468814466, Ubuntu job 102843644781 failed with SIGABRT after `session::actor::coordination::tests::cross_workspace_approval_offers_only_one_shot_choices` overflowed its stack. Local full shell tests with RUST_MIN_STACK=16777216 passed, including this case. Log: `/tmp/grow-v2.1.6-linux-coordination-failed.log`.
