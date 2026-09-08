## Findings
The only production assignment opening an ImageViewerState is agent_view/prompt.rs's ImagePreview element interaction. It invokes ImageViewerState::open synchronously, which may fs::read a durable attachment and convert non-PNG bytes through sips/Rust on the input thread.

open_from_path_deferred constructs loading=true with a private source_path. Every found caller is in a test: pager-render prompt_images, Pager root/mod, agent_view/input, paste and media. No other production struct initializer or loading=true assignment was found for ImageViewerState.

Root prepare_agent_image_load consumes take_source_path and emits LoadImageViewer; effects spawn_blocking calls load_image_data; task_result handles owner/target completion. These production branches exist but their loading-state admission is only exercised by tests. They should be connected to the real entry, not automatically deleted as dead functionality.

The separate synchronous open_from_path constructor has only three direct tests and no found production caller. It duplicates synchronous filesystem read, dimensions, conversion and state assembly; record it as R20 for user confirmation. finish_loading is explicitly a test convenience and is not included in this candidate.

File byte budgets and deferred source/protocol ownership require independent changes; do not repair only the dormant constructor while leaving the live UI path synchronous.
