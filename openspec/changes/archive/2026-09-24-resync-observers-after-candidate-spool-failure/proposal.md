## Why

The bounded leader candidate spool currently shuts down the whole leader when its budget or I/O fails. That closes the only non-retracting observer, and the normal disconnect path can evict its still-running session. The candidate must remain private without sacrificing session ownership.

## What Changes

Mark a failed candidate as unavailable to non-retracting observers, release its spool, and keep the leader and subscriptions alive. On Accepted, send those observers a targeted resync request after any in-flight load; Pager reloads canonical history in place. On Discarded, discard the candidate without resync. A failed accepted-spool read also requests resync. The existing finite byte and record limits remain.
