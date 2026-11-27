## Why
Each resume_from currently validates and materializes the entire lifecycle root and source child. Concurrent resumes repeat the parent's unrelated history reads. Cold root session resume already uses committed working publications and canonical record references.

## What Changes
Resolve the target delegation newest-first and restore the source child's required working state and inherited display from the indexed recovery machinery. Verify actual reads, memory, timing and dense equivalence.
