# Evidence
- sampling.rs calls recover_completed_description once per uncached group. It has one production call site.
- image_describe.rs spawns blocking work for each call. JsonlStorageAdapter::recover_completed_image_description_from_directory rereads the committed parent Timeline, folds it, reads every spawned Sideband and runs validate_sideband_ledgers before selecting one matching result.
- Matching requires exact prompt, ImageDescription purpose, completed terminal state, image-group strategy/version, exact source revision, selected/context SurfaceId and parent input/evidence range provenance. Newest matching spawn wins. This audit found no loose-text or cross-revision reuse.
- For G uncached groups, the same parent and Sideband reads and validation repeat G times. This is source-derived multiplicative work, not a measured latency benchmark or observed memory exhaustion.
- validate_sideband_ledgers also checks parent compaction references against other Sidebands. Filtering disk reads to ImageDescription alone would drop existing integrity coverage; that is not the proposed optimization.

# Next step
Read and validate one durable snapshot for the batch of uncached group queries before issuing new provider attempts. Return only matching outputs/provenance, so the full historical ledgers are not retained across provider awaits. Keep same source/prompt/revision matching and newest-result selection. Do not add an independent persistent index or bypass corruption checks.
