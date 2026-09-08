# Design
Run read IO in spawn_blocking. Open a single handle with O_NONBLOCK on Unix, inspect its metadata for ordinary-file type and at most1,048,576 bytes, then read at most allowance+1. Validate actual byte count before UTF8/JSON parsing. Regular symlink targets remain readable. Map read/join failure to the existing empty hidden-ID set; no quarantine, deletion or auto-repair.

Write validation uses the same encoded-byte constant before directory/temp creation. Serialization occurs before the write budget check; this bounds published state and read allocation, not every upstream config allocation. Existing atomic publication remains.

# Validation
Explicit temp paths only: exact padded JSON limit, limit+1, actual-reader excess, malformed input, directory and Unix FIFO rejection, regular symlink, oversized write preserving prior commit. No GROW_HOME mutation or real state access. Slow ordinary/network filesystems and async waiter cancellation remain separate; O_NONBLOCK is not a universal IO deadline.
