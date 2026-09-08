# Evidence

ensure_historical_loaded retained lazy_source on read failure but returned unit. get_rewind_points, truncate_from, merge_and_remove_from and max_prompt_index continued afterward. Shell handle_rewind and recover_pending_rewind consumed the getter for effects and transaction completion.

Red failed_historical_load_preserves_live_points_for_retry: 0 passed / 1 failed (0.00s). An invalid-UTF8 temporary ledger plus a live point at index 5 caused old truncate_from(5) to remove the live point despite historical-load failure.

The final regression requires failed getter/max/truncate/merge calls to return errors, checks that the live point remains, repairs the same pinned file with point 0, and confirms retry returns [0, 5]. Workspace file_state tests: 30 passed (0.01s). No actual user history was used.
