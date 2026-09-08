Prior final Linux CI 34229667703 passed all image/OCR regressions and failed only the foreground timeout in the shared compaction fixture (3756 Shell passes, 1 failure, 3 ignored).

Final Linux core regression [34231136363](https://github.com/LordCasser/grow/actions/runs/34231136363), commit `702a703ab8448f8d0f6cfc8ccfe50eee5bb8fb73`, passed all tests and the CLI build:

- ChatState: 465 passed.
- Memory: 302 passed.
- Pager: 7154 passed, 10 existing ignored.
- Pager minimal: 86 passed.
- Sampler: 218 passed.
- Sampling types: 266 passed.
- Shell: 3757 passed, 3 existing ignored.
- Workflow: 65 passed.
- Total: 12,313 passed, 0 failed, 13 existing ignored.
- `cargo build --locked -p cli --bin grow`: passed.

The fix separates the blocked auxiliary summary from foreground requests without changing production code, the 10-second foreground timeout, or the promotion/cancellation assertions. The subsequent archive commit changes documentation only.
