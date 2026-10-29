# Design

Configure the OCR command as a new process group/job, suspend it until enrollment where the platform helper requires that ordering, and retain the resulting `ProcessGroup` in an RAII guard through every exit from `run_local_ocr`. The guard kills remaining group members on cancellation, timeout, errors, and normal completion, including descendants that closed the captured pipes before the direct helper exited. Tokio `kill_on_drop` remains enabled for direct-child cleanup, and the existing concurrent pipe drains plus `child.wait()` continue to own normal completion.

The focused Unix test helpers start OCR scripts that fork sleeping descendants and record their PIDs. A test-only spawn handshake makes cancellation and timeout deterministic. One success case closes the descendant's inherited output pipes before the direct helper returns, then verifies OCR text is retained and the descendant is terminated. Cleanup assertions verify processes stop; fixture-side cleanup remains armed until assertions complete so a failing test does not strand a descendant. The synchronous asset-batch fixture will be inspected for panic paths; any adjustment will remain test-local.

## Ownership

- The `run_local_ocr` future owns the Tokio child and the process-group guard.
- On normal completion, the direct child is waited before guard drop kills any remaining group members and returns validated OCR text.
- On timeout, cancellation, output/read error, or future drop, the guard synchronously kills the process group/job. Tokio retains its existing direct-child kill-on-drop and runtime reaping behavior.
- Windows enrollment occurs before the suspended child resumes, preventing it from spawning an unowned descendant before job assignment.
