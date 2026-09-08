## Evidence
Scoped and whole-checkout rg searches covered ImageViewerState::open, open_from_path, open_from_path_deferred, ImageViewerState struct initializers, load_image_data, finish_loading, LoadImageViewer, take_source_path and has_deferred_source. Source reads confirmed cfg(test) boundaries and the real ImagePreview key-handling branch.

No Rust code changed and no Cargo tests were run for this audit. Text search does not establish absence of external public-API consumers. The candidate remains unremoved; the deferred machinery is explicitly retained for the planned production fix.
