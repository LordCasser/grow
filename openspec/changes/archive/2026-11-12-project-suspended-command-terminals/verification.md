## Incident evidence

Read session `01a1c301-0971-7f20-af72-29e37f07151b` without modifying it. The two rows had distinct task IDs. The starts at Timeline presentation positions 17844 and 21254 each had a later durable command settlement (domain sequences 18460 and 23374) and a completion receipt, with no ordinary task completion presentation.

## PTY regression

On macOS arm64 with Rust 1.93.1, the regression ran the same fixture against the pre-change and repaired CLI binaries. The fixture starts a real background process, observes the running Tasks row, suspends with Ctrl+D, confirms the process exited and the interrupted settlement was written, and cold-continues the same history. The pre-change binary displayed the row in running Tasks. The repaired binary showed an empty running list and the row in completed history. It kept the original settlement and ran the command exactly once. The existing foreground command suspension and resume test passed against the repaired CLI.

## Projection checks

All 10 Shell history tests and all 8 orphaned-task tests passed. The new projector case checks two suspension operations with distinct IDs and identical command text, recorded start and end times, history cursor continuation, and one completion when an ordinary presentation fact already exists.
