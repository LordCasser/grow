## Approach
Use the existing working publication, pinned session directory and validated committed suffix. Keep parent ownership lookup separate from child working-state restoration. Search target lifecycle records in reverse chronological order. Validate the source seed and result against the exact parent spawn, terminal and caller authority. Reuse main-session mechanisms for compacted surfaces, model transitions and canonical presentation. Preserve the ordered inherited display that a newly derived session must persist.

## Verification
Compare indexed and canonical results for published records, unpublished tails, incomplete or mismatched sources and missing indexes. Measure production reads and peak RSS for the same parent at concurrency one, two and four and for a source with substantial history.


## Storage and scheduling

A resident parent captures the domain frontier and both lifecycle positions in the same ChatState query. Under the shared append lock, Shell checks the live writer’s weakly referenced committed file stamp, complete byte length and domain frontier. It reads and compares the terminal and spawn at those two canonical positions. A stale actor, writer or source falls back to indexed or canonical verification. An offline root can validate a lifecycle-only or diagnostic suffix against the published lifecycle index. An offline mixed execution suffix can require the ordinary working Surface to validate the later records.

The source’s working checkpoint stores its execution header and Surface items separately in bounded redb chunks. The seed display index addresses original updates, including intermediate tool rows. History materialization and the derived child’s journal/index publication run on blocking workers. The child actor adopts the Timeline already validated during atomic staging.
