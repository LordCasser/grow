# Evidence
The helper is called by diagnostics agent_id cache with Some(0600) and workspace permission state with None. Both callers already prepare their parent directories. Current open/write/rename chain loses the distinction between creation and later failure, then removes the temporary path unconditionally.

# Minimal fix
Keep the public nonce-derived path selection, extract its existing file operation into a private explicit-temp helper so tests can deterministically collide without depending on process-global nonce. open(create_new)? returns before cleanup on failure. After successful creation, write, close the file, rename on success, and remove owned temp on write/rename error. Closing before cleanup also avoids leaving an open handle during Windows deletion.

No new dependencies or storage framework. No naming retry, fsync policy change, parent creation or cross-process ordering. Parent-directory hostile replacement races remain outside this ownership-at-creation correction.
