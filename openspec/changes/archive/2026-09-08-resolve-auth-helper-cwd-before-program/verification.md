# Verification

Red: provider_relative_cwd_and_program_resolve_once failed against the original implementation with command failed to start: No such file or directory (0/1, 0.05s).

Green: shell auth:: group passed 34/34 in 1.04s. The new temporary-directory fixture executes ./token.sh under a relative configured cwd and reads sibling token.txt. Existing absolute-cwd, shell/direct execution, program resolution, cache, refresh and resource-bound regressions passed.

Configured cwd now passes through std::path::absolute after home expansion and before both executable resolution and child current_dir. No process-global cwd mutation, canonicalization or new entity was introduced. Fixtures use synthetic tokens only; no real provider/helper/config calls were made. No installed Grow binary was updated.

Scoped rustfmt and git diff --check passed. Existing nonfatal linker __eh_frame warning remained.

Pre-archive strict validation passed 17/17. No cargo/rustc process remained before cargo clean, which removed 7,372 files / 2.7 GiB.
Post-archive strict validation passed: all 16/16, archives 266/266. Free disk after cleaning: 56 GiB.
