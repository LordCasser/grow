# Design
Inspect reusable atomic writer utilities before choosing the smallest implementation. Keep IO off the UI event thread, make destination-parent handling explicit, and preserve the previous committed file if preparation fails. Change the helper return type to a real IO Result and map it in the sole pager caller. Use isolated temp directories and filesystem failure fixtures; do not access real GROW_HOME or introduce a generic persistence framework.

# Separate work
Atomic snapshots do not make independent async effects ordered or merge writes across processes. Reader regular-file/byte/time limits are not part of this commit change.

# Implementation choice
config::fs_atomic::write_atomically was inspected but not reused: its cleanup removes the computed temp path even if create_new failed, so ownership of that file is not proven. This debt is separate. NamedTempFile provides exclusive creation and owned cleanup; the announcements crate adds the already locked workspace tempfile dependency. The wrapper captures path/serialized snapshot then runs blocking filesystem work via spawn_blocking. Parent directories are explicitly created; tempfile is written, sync_all called, then persist atomically replaces the destination. Unix tempfile permissions apply to the resulting state file. No directory fsync/crash-durability claim or cancellation rollback after publication.
