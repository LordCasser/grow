# Design

Resolve the existing desired-target/latest fallback first, then inspect the loaded points for any file changes at or after that resolved target. Preserve selection, draft and inline FilesOnly exclusion. No IPC or backend change. The picker and back paths already use this range.

Late async results can recreate dismissed rewind state; request ownership requires separate lifecycle work and is recorded in backlog.
