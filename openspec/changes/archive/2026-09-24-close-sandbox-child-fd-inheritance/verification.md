# Verification

- Inspected the four Linux child-network launch hooks, `command-fds` mapping order and implementation, shell-state pipe producers, `tty_utils::detach_from_tty`, and the streaming `ProcessSession` wrapper. Static shell maps Grow-owned state input to fd 3; persistent shell maps state input/output to fd 3/4; ordinary and streaming shell need only stdio.
- The Linux regression first runs an unfiltered exec and proves deliberately non-CLOEXEC, pre-connected TCP and UDP sockets can send through `write`. A second exec applies the new hook, proves both socket writes fail, and exchanges bytes over mapped fd 3/4 pipes. This distinguishes the protection from a fixture that never inherited sockets.
- `orb -m ubuntu ... cargo test --locked --offline -p sandbox --lib -- --test-threads=1 --quiet`: 72 passed. OrbStack Ubuntu used Linux 7.0.14; the focused actual-exec regression passed separately after the final TCP/UDP extension.
- `orb -m ubuntu ... cargo check --locked -p tools -p shell`: passed, compiling all four Linux launch call sites.
- `RUST_MIN_STACK=16777216 cargo test --locked --offline -p shell --lib -- --test-threads=1 --quiet`: 3918 passed, 3 ignored. This full run finished before the Linux-only FD change and verifies the preceding session/test repairs; it is not counted as the Linux FD regression.
- `cargo fmt --all -- --check`, `git diff --check`, and `openspec validate --all --strict --no-interactive`: passed before archive.
