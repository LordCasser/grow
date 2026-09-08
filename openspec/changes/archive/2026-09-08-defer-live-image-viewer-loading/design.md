## Architecture
ImageViewerState::open now captures an ImageViewerLoadSource (shared Arc bytes or durable path) and the requesting GraphicsProtocol. It returns loading state without file reads, byte copies or conversion. Memory retains precedence over a durable path. Every opening obtains a fresh overlay owner, independent of attachment preview identity.

The existing root prepare_agent_image_load consumes the source/protocol once and emits LoadImageViewer. Its spawn_blocking worker calls load_image_data with that owned source and explicit protocol. Memory copying, file reads, header validation and conversion happen there; LoadedImageData returns through the existing task result. Existing agent/child/owner validation remains the completion authority.

The real ImagePreview key branch retains its terminal-support guard and calls the now-deferred constructor. Image display number and title behavior remain. Path-only deferred test construction and finish_loading convenience use the same source/protocol model. Candidate R20 open_from_path remains unremoved.

## Validation
The new root integration test starts with the actual prompt Enter handler, rather than manually creating a loading state. Both memory and file cases return empty loading buffers, enqueue an owned effect, convert JPEG through the captured Kitty protocol on another thread and apply the task result. Reopening the same image produces a different owner and rejects the old result. Shared loader tests cover missing and corrupt sources; existing current-owner failure and child-view tests remain.

## Boundaries
Moving I/O off the input thread does not supply file-byte or aggregate memory budgets. Those remain separate follow-ups. This change does not cancel an already running blocking conversion when the viewer closes; it rejects that result by owner identity, with existing converter execution limits still applicable.
