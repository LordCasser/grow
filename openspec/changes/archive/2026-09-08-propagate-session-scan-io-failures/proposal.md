## Why
Session enumeration silently skips all directory-open, summary-read and physical-identity errors. Operational failures such as permission denial therefore become successful partial listings. Cleanup may mutate a subset after incomplete discovery; search bootstrap uses missing IDs for orphan pruning and can stamp completion.

## What Changes
Continue skipping confirmed missing or invalid entries, but propagate operational scan errors. Preserve current filtering, duplicate detection and identity checks. Callers already abort cleanup/reindex on list errors; use those existing boundaries.
