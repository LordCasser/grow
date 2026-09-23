## 1. Error facts and classification

- [x] 1.1 Add named SSE `code/message/request_id` parsing at the shared error boundary without treating ordinary incomplete events as errors.
- [x] 1.2 Classify content inspection, throttling, overload, and unknown provider codes with explicit retry safety.

## 2. Production readers and validation

- [x] 2.1 Pass the SSE event name from Chat Completions, Responses, and Messages readers.
- [x] 2.2 Cover three backend HTTP/SSE paths, malformed counterexamples, partial tool candidate, no retry for rejection, and existing bounded retry/usage behavior.
- [x] 2.3 Run focused tests, format/check, strict OpenSpec validation, record evidence, archive and revalidate.
