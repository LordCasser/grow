# Design

`install_child_network_filter` already runs as the last network-specific `pre_exec` hook at four launch sites: ordinary tool shell, static shell, persistent shell, and streaming shell. The static and persistent paths first register `command_fds` mappings from Grow-created pipes to child fd 3, or fd 3 and 4. That ordering is required: the descriptor admission step must run after the mappings and before the seccomp filter.

In the forked child, a single Linux `close_range(3, UINT_MAX, CLOSE_RANGE_CLOEXEC)` syscall marks every non-standard descriptor to close on successful exec. This leaves Rust's private exec-error pipe usable until exec or spawn failure, unlike eagerly closing the range. The hook then clears `FD_CLOEXEC` only for the known mapped state-pipe destinations. The allowed destinations are represented by a small enum at the call sites rather than an arbitrary descriptor list. A missing expected pipe fails the spawn. The hook installs seccomp only after descriptor admission succeeds, and a kernel that cannot apply `CLOSE_RANGE_CLOEXEC` also fails closed.

The boundary applies to known Linux child launches while sandbox network restriction is active. It does not retroactively constrain an already-running process or a deliberate stdin/stdout/stderr connection. Network restrictions remain a separate policy from filesystem sandboxing.

The Linux regression should start connected local TCP and UDP sockets, deliberately pass non-close-on-exec duplicates to a control exec, and prove they can send through `write`. A restricted exec must fail to write through both descriptors while mapped pipe fd 3/4 exchange bytes. The test must exercise actual exec rather than only inspecting the close-on-exec flag inside `pre_exec`.
