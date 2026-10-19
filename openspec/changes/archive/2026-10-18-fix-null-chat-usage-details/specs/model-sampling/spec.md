## ADDED Requirements

### Requirement: Chat usage accepts null optional token details

Chat Completions SHALL decode optional usage detail counts as unknown when omitted or explicitly `null`. Explicit zero and positive integer values SHALL remain available as reported values. This applies to prompt `audio_tokens` and completion `reasoning_tokens`, `audio_tokens`, `accepted_prediction_tokens`, and `rejected_prediction_tokens`, for both streaming and non-streaming responses. Nullable details SHALL NOT invalidate otherwise complete usage or response facts. Trusted prompt, completion and total counts, cache-token handling, and existing usage settlement behavior SHALL remain unchanged; unknown reasoning counts SHALL continue through the existing aggregation boundary as zero.

The decoder SHALL continue to reject negative, string, floating-point, and out-of-range token counts, and SHALL reject `null` for required usage totals. It SHALL NOT skip malformed chunks or change retry behavior.

#### Scenario: Optional detail omitted or null

- **WHEN** a streaming or non-streaming Chat response omits an optional detail count or supplies it as `null`
- **THEN** decoding succeeds and that detail remains unknown, while valid total usage and response facts remain available

#### Scenario: Optional detail reports zero or a positive count

- **WHEN** a streaming or non-streaming Chat response supplies an optional detail count as zero or a positive integer
- **THEN** decoding preserves that reported value, including the distinction between zero and unknown

#### Scenario: Invalid optional count or null required total

- **WHEN** a Chat usage detail is negative, a string, a floating-point value, out of range, or a required total is `null`
- **THEN** decoding rejects the malformed usage rather than coercing or skipping it

#### Scenario: Tool-call completion followed by nullable usage tail

- **WHEN** an HTTP Chat SSE response reports `finish_reason: tool_calls` and its usage tail contains nullable optional details with valid required totals
- **THEN** the response completes and trusted usage is settled through the existing accounting path without changing retry behavior
