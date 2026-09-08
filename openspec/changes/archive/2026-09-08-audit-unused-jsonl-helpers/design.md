# Evidence and disposition

Repository-wide Rust token searches find only definitions for JsonlStorageAdapter::read_optional_json_sync, summary_lock_file and workflows_dir. These are private inherent methods, not StorageAdapter trait implementations; no reflective dispatch or exported API is identified. The optional JSON helper uses exists + unbounded read_to_string and turns parsing/IO failures into None, but no active caller was found, so these properties are not claimed as a current runtime failure.

Active load paths obtain control, signals and announcement state from validated Timeline events. load_workflow_runs_sync opens workflows/<run_id> relative to the pinned session directory and uses bounded reads. Writer ownership is managed by the actual writer lease path, not summary_lock_file. Preserve all those active paths.

rewind_points_file is different: three JSONL tests use it for fixture mutations/assertions, so it is explicitly excluded from removal candidates. read_rewind_points_file in the workspace crate is another distinct test helper and is not part of this proposal.

The only rewind_points_file_path token is an obsolete rustdoc reference on StorageAdapter::load_session_without_updates. persistence::load_session's restoration path opens the rewind file through the session directory into PinnedRewindSource; actor/spawn.rs passes that source to FileStateTracker::with_lazy_source. Correct the documentation to this actual capability path.

R28 covers only read_optional_json_sync. R29 covers only the two unused path constructors. Both require user confirmation before removal. No associated user files, writer lock protocol, Workflow feature or rewind functionality are candidates.
