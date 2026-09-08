# Evidence

read_rewind_jsonl_from_file parsed each nonblank line with serde_json, logged decode failures and continued. Its successful Vec was merged and consumed the lazy source. Both the full tracker load and pinned metadata scan use this helper; old path-based readers are cfg(test)-only.

Red malformed_pinned_history_rejects_partial_load_and_retries: 0 passed / 1 failed (0.01s). A malformed line between valid points 0 and 2 returned Ok([0,2,5]) including the live point 5. The red run stopped at the first syntax-error case; the typed-row failure case was exercised on the fixed source.

The helper now returns InvalidData at the first failing record with label and one-based physical line. Parsing stays local until complete, preserving the existing no-merge/no-consume error path. The shared metadata reader uses its existing fallback on failure.
