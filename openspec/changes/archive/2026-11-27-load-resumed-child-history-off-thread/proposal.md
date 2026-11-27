## Why
Launching a child with resume_from can synchronously read, validate and render its inherited transcript on the Pager input loop. Multiple concurrent source validations may also read the same parent journal.

## What Changes
Publish the end of the newly created child’s inherited history, project that immutable prefix off the UI thread, and install it ahead of live output. Measure parent journal reads, memory and elapsed time with simultaneous durable restores.
