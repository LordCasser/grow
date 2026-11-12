## Why

Large sessions can take more than five seconds to save. Pager drops the response waiter at that point and Shell uses a separate five-second limit for the tree, which can leave durable suspension incomplete if the user forces exit.

## What Changes

- Let confirmed exit wait for the original suspension through the durable tree barrier and writer retirement.
- Show continued waiting and explicit force quit while saving. Keep retry available for reported failures.
