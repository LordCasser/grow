# Evidence
- SessionActor::project_conversation_images_for_text_model_once resolves an auxiliary route distinct from the rejected runtime, rejects a known unsupported auxiliary route and retains session-local sampler fields.
- Only DescribeError::Sampling passing is_image_input_unsupported records an auxiliary negative capability. Timeout, blank text, transport and Sideband failures do not. Unresolved groups return ImageDescriptionUnavailable before installing shadows; successful descriptions retain durable Sideband provenance.
- The 240-second recovery deadline begins after route resolution. Each group captures remaining time before durable-description inspection, begin_sideband and attempt_selected. The later relative provider timeout reuses that stale duration. For example, 200 seconds of preparation plus a captured 240-second timeout permits provider work past the recovery deadline. This is a source-ordering finding, not an observed production stall.
- Provider completion settlement and terminal Sideband writes also occur outside the timer. Those writes have durable acknowledgement and Drop repair semantics; blindly timing out the entire function would change lifecycle handling.

# Repair boundary
Start with an absolute provider deadline bounded by the existing recovery deadline and per-call limit. Include a regression where preparation consumes the remaining budget and the provider is not polled. Durable preparation/finalization and route-resolution wall-clock bounds are separate architectural work, not a hard 240-second end-to-end guarantee.
