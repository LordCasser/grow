# Handoff terminal summary without copying response bodies

## Why

The Sampler actor sends a cloned full `ConversationResponse` through its event channel while the original response is delivered to the turn through a oneshot. The Shell event drainer only needs terminal metadata and metrics, so cloning response items and native continuation adds an unnecessary large allocation.

## What Changes

This is an internal handoff refactor. Layer-2 `SamplingEvent::Completed` remains unchanged for direct stream consumers. The Sampler actor emits a metadata-only terminal event to its event subscribers; Shell uses it for completion bookkeeping. No user-visible behavior, persistence format, or ordering changes.

`skip_specs: true` because the change does not alter any archived behavior contract.
