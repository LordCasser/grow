# Design
SessionThreadOwner::drop enqueues a JoinHandle and weak persistence sender. The current reaper immediately joins each FIFO job. Other concurrently tested/live sessions can keep the first join blocked beyond the later session's lifetime; this is a production cleanup fairness issue, not a reason to relax the assertion.

Retain the existing one worker and queue. When pending jobs exist, wait at most 20 ms for another job, then scan pending handles and join completed ones. When no jobs remain, block on recv so idle reapers do not poll. If the channel closes while jobs remain, wait between scans and finish cleanup without spinning. No per-session cleanup thread or new runtime owner. Stop remains after join, never before actor exit.

Regression enqueues a blocked actor before an already-finished actor, verifies the latter receives Stop without releasing the former, then releases the blocker even on assertion failure. Run lifecycle tests and full Shell concurrency regression.
