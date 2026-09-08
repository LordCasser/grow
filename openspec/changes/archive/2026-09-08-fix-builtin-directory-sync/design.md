# Design
The existing sync_managed_parent clones/opens cap-std Dir then directly fsyncs its underlying descriptor. On Linux that descriptor is O_PATH; fsync returns EBADF. An Ubuntu noble arm64 probe using the actual production transaction reproduced EBADF before the version marker. A cap-std 4.0.2 probe verified Dir::open(".").sync_all succeeds relative to the same pinned capability.

Keep capability-based lookup and reopen only dot as a readable file descriptor. Do not resolve an ambient path or suppress fsync errors. Verify one complete transaction on a fresh temporary home, all existing extraction tests, and the Linux production-code probe. Stack overflow and tracing test stability remain separate investigation tasks.
