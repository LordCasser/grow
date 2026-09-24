# Design

`GROW_HOME` is process input, not per-test mutable state: the production resolver caches it once, and Rust tests share the process environment. The regression boundary is therefore the process. A small test helper reruns only the marked test in the current test executable, passing a private temporary `GROW_HOME` and a child marker in the launch environment. The parent owns the temporary directory until the child exits. The child runs one exact test, so parallel unit tests cannot race its environment.

`GrowHomeFixture` uses the child's configured path and owns only its unique working directory and test-created session trees. Tests that persist trust grants or dismiss plugin CTAs use the same child boundary. Theme cache/system appearance and mouse-capture reset guards restore globals in `Drop`, including panic unwinding.

The ignored leader-cluster scenarios remain a known boundary: they temporarily configure several process-global variables while driving an in-process async cluster and explicitly document `--test-threads=1`. This change does not redesign that harness. `NO_COLOR` is supplied to the PTY subprocesses that need it; no in-process Pager test mutates it.
