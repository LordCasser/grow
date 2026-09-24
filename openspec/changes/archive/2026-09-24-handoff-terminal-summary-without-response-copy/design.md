Keep `SamplingEvent::Completed` as the Layer-2 terminal event consumed by `collect_response`. Add an actor-level terminal summary event containing only the values the Shell drainer currently reads: request id, usage, item count, provider terminal metadata, doom-loop signals, and inference metrics. Emit that summary before sending the original response through the completion oneshot, preserving FIFO ordering and the turn-stream-drained barrier.

The metadata fields are cloned independently; the response's item vector and native continuation are never cloned for the event channel.
