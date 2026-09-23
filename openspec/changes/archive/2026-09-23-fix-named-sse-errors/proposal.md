# Change: Preserve named SSE provider errors

## Why

An HTTP 200 Messages stream can end with `event:error` and a flat `code/message/request_id` body. Grow currently discards the SSE event name, fails to recognize that body, and reports `missing field type` instead of the provider's rejection. Some provider codes also receive a retryable 500 classification despite describing a permanent content rejection.

## What Changes

- Recognize a complete flat provider error body only at an SSE `error` event boundary, across Chat Completions, Responses, and Messages.
- Preserve the provider code, message, and optional request ID in the resulting error evidence, and classify confirmed content rejection, throttling, and overload without making arbitrary protocol errors retryable.
- Verify the real SSE decode paths and the existing attempt admission, usage, retry, and tool safety boundaries.

## Capabilities

### Modified Capabilities

- `model-sampling`: streamed provider error facts and recovery classification.

## Impact

The shared sampling error parser and three existing SSE readers change. No wire request, storage format, or retry scheduler is added.
