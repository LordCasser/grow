# Evidence

- shell/session/actor/model_switch.rs compares the complete ModelId, then invokes replace_sampling_route. Same provider with a different model already clears native state.
- sampling-types/messages.rs ContentBlock::Thinking previously required signature at deserialization, even at content_block_start. Live proxy Grok supplies it later.
- sampler/stream/messages.rs accumulates signature deltas before producing native continuation. The new final guard rejects unsigned native fragments while keeping visible response items.
- shell/turn/sampling.rs formerly recovered only API 400 when the request contained native state. Serialization missing-signature errors now share the acknowledged reset path; arbitrary serialization errors remain terminal.
- turn/mod.rs reset waits for ChatState acknowledgement and rebuilds the request. After clearing native state, the retry cannot qualify again unless a later successful response creates new native state.
- Collected response success precedes push_response_durably and tool execution; parse failure does not authorize tool calls from the failed response. Durable history is not cleared.
