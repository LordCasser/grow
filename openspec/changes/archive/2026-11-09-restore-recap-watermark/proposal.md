# Why
Cold loading restores the primary API idle clock but resets the successful recap watermark. Historical recap replay followed by return from away can therefore generate a second summary of unchanged work.

# What changes
Persist the frozen main-turn count in the existing extensible metadata of displayed successful recaps and restore it for existing actors. Preserve manual recap and replay behavior. Legacy recaps without coverage evidence remain display-only; do not infer coverage from notification arrival order.
