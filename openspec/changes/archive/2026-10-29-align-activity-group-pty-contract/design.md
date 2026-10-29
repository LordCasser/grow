## Design

Keep the existing real PTY scenario and tool sequence. Its five reads, two greps, and one edit are contiguous activity, so assert one folded summary for all eight tool members. On expansion, assert each member is available, including the Edit member. Opening member zero's body must leave the activity group expanded; closing it restores its row within that same expanded group, and a second collapse folds the entire activity.

Synchronize on visible state transitions. Wait for the scripted turn to become idle before navigating. After closing member zero, wait for its body sentinel to disappear within the existing 10-second action budget before asserting the expanded group and remaining rows. After collapsing, wait for member zero's row to disappear, then check the full member set and collapsed header/caret. Do not use fixed redraw sleeps.

This change only aligns test expectations with the archived `unify-tool-thought-aggregation` contract. It adds no test-only timeout, product code, or parallel grouping logic.

Root constrained DONE and actual turn-idle observation to the same absolute 90s deadline, retaining the original settling budget rather than adding another full timeout.
