# Evidence

Repository Rust symbol searches find rewind_files only at its public definition in workspace/src/session/file_state.rs. FileRewindResponse, FileRewindConflict and ConflictType occur only in that file and support that helper. No repository tests invoke rewind_files. Public out-of-repository consumers are unknown.

Actual shell SessionActor::handle_rewind collects checkpoints, gates preview on force, writes its rewind transaction, applies files via apply_file_rewind, persists the next checkpoint projection, commits Timeline changes and clears the transaction. apply_file_rewind captures changed paths and compensates on failure through rollback_rewind_files. recover_pending_rewind owns interrupted transaction recovery. Shell uses RewindResponse/RewindConflictInfo and the shared pure merge_rewind_points_from function, not the standalone workspace response types.

The disconnected helper applies snapshots directly through AsyncFsWrapper, reports conflicts but proceeds to writes, and converts current-file read errors to None. Those are properties of unused code, not evidence of an active product defect. Do not route shell through this helper merely to give it a caller. Its documentation must not imply that it is the shared ACP runtime authority.

FileStateTracker::merge_and_remove_from and max_prompt_index have only test callers. Actual ConversationOnly rewind computes and persists merge_rewind_points_from, then installs replace_rewind_points. Preserve the pure merge algorithm, full-history error handling, live capture, truncate_from (used in cancel), get_rewind_points and replace_rewind_points. If R31 is approved, move any unique assertions to these real consumers before removing method-specific tests.

R30 and R31 await user confirmation. No storage files, checkpoint data, shell command, picker or transaction behavior is removed in this audit.
