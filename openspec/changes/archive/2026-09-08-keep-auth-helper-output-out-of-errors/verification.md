# Verification

Red: credential_helper_errors_do_not_echo_output failed on the synthetic stderr sentinel against the original mint path (0/1, 0.05s).

Green: shell auth:: group passed 33/33 in 1.05s. New local /bin/sh fixtures cover nonzero exit plus sentinel stderr and invalid JSON expiry containing a sentinel; full error-chain formatting contains neither sentinel nor access token and retains structural failure details. Existing cache, refresh, process/output bounds and parser tests remained green.

Tests used synthetic keys and configured local fixture commands; no real user helper, credentials or provider request was used. Logging safety is verified at the exact error producer used by warning logs, not via a tracing subscriber capture. Existing nonfatal linker __eh_frame warning remained.

Scoped rustfmt and git diff --check passed; strict pre-archive validation 17/17. No cargo/rustc process remained before cleaning. cargo clean removed 7,372 files / 2.7 GiB; free disk 55 GiB.
Post-archive strict validation passed: all 16/16, archives 265/265.
