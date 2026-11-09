# Design
Carry recapCoveredMainTurns through the existing notification/history metadata map. The value comes from the frozen recap input, not from the Surface when the asynchronous notification is persisted. Existing bootstrap restores the latest explicit watermark. Fresh/seeded actors start at zero. Existing runtime rewind/compaction count healing remains unchanged.

# Limits
Old records lack an unambiguous frozen-input identity. They cannot safely infer coverage from notification position, a completed Sideband, or matching summary text. Suppressed automatic results have no displayed recap fact and retain the existing cold-load retry behavior. These limitations are explicit and must not be described as complete historical migration.
