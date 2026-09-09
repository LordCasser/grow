## Why
Session 01a081be-6168-7772-9e0c-dcc62a76b552 failed immediately after spawning a child: its durable Goal remained Active with no budget, but provider admission reported active Goal none. Child startup shares the root usage window and unconditionally synchronizes it from its empty local Goal tracker.

## What Changes
Restrict shared Goal lifecycle synchronization to the root session, including startup conflict normalization. Descendants retain admission and usage settlement against the root window. Preserve stale request rejection and root pause/budget fences.
