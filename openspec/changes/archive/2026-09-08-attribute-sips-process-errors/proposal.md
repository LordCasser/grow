## Why
An intermittent raw EPERM from the converter cannot currently be attributed to spawn/detach, process-group attachment, waiting or cleanup. Bounded reproduction attempts have not repeated it, so changing cleanup semantics would be speculative.
## What Changes
Add stage context to errors from the private sips runner while preserving ErrorKind and the underlying error text. Keep all failure handling and deadline behavior intact. The EPERM investigation remains unresolved separately.
