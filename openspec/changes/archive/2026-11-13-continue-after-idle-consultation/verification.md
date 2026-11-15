# Verification

Shell message delivery selection passed 17 tests. The live idle consultation test drives a mock Sideband, observes the idle arbiter wake, checks that the single foreground request includes the exact question and answer, and retries the same inquiry without another foreground request. Existing tests cover active sampling, held FIFO, paused and budget-limited Goals, cancellation, suspension and lost receipt or consumption acknowledgement.

The suspension test explicitly closes automatic notification admission before the consultation so it still exercises passive delivery across writer replacement.
