# Design

Use the existing rewind point list and any(point.prompt_index >= target && point.has_file_changes) to compute range eligibility. No extra IPC field, metadata parser or cumulative count is needed. The shell already applies the same >= target checkpoint range. Back navigation first resolves its existing target fallback, then computes range eligibility using that target.

This finding came from metadata audit: shell generates checkpoint choices from prompt indices independently of metadata. Validating metadata more strictly would not fix the range mismatch. Metadata failure/unknown-count reporting and nested snapshot validation remain separate. Inline edit continues to hide FilesOnly because resubmission requires conversation rewind.
