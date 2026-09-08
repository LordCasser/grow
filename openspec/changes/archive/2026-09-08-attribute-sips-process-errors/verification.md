## Verification

`cargo test --locked --offline -p pager-render --lib terminal::image::tests --quiet`:22passed,0failed,0ignored (1.01s), with disabled incremental/debug info and two build jobs.

New tests verify missing-executable startup context with NotFound, and injected Unix pre_exec EPERM with PermissionDenied plus the original OS-error text. Existing timeout, descendant cleanup, successful/nonzero exits, output limits and private-directory tests pass. Injection establishes diagnostic behavior only; it is not reproduction of the original intermittent error.

Before final changes, temporary instrumentation distinguished spawn/attach/wait/cleanup in a bounded4-worker experiment,500 exit0 processes each. All2000 completed without errors (10.40s). Temporary instrumentation and the ignored experiment test were removed. Final errors use normal tracing through the existing conversion failure path, not unconditional stderr.

No retry/suppression or lifecycle policy was changed. ErrorKind and original diagnostic text are retained; the private wrapper is not a promise to preserve raw_os_error() as a structured field. No Windows execution was performed. investigate-sips-process-eperm remains active and unresolved.
