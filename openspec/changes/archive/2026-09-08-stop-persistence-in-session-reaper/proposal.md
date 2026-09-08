## Why
The last SessionThread owner only forwards its OS thread to the reaper, dropping the weak persistence stop route. A Joined owner also returns from Drop without issuing Stop. Explicit lifecycle drain was fixed separately; last-owner cleanup still needs to terminate admission after thread exit even when external handles retain senders.

## What Changes
Carry the weak stop route through the existing reaper. Join the actor thread before requesting persistence Stop. A previously joined owner's final drop requests Stop directly. Do not block a Tokio runtime waiting for its own persistence task, and do not turn destructor cleanup into an acknowledgement of full writer release.
