# Design
ACP initialize/new_session passes clientIdentifier unchanged into the session and permission actor. state_file_path is shared by reads and writes; change only this mapping to fixed-length lowercase SHA-256 hex. Existing shared fallback remains intentional and tested. No schema or authorization-policy change.

Tests use explicit temporary directories, never global GROW_HOME. Verify separator, underscore, case, Unicode, empty and long identifiers; actual writes and reads must remain isolated.

Separate debt: unbounded permission and diagnostic cache reads, read-error shared fallback, and diagnostic cache validation are not addressed here.
