## Design
Delete raw prefix construction and fields from client_post and remove AuthInfo.auth_prefix, whose sole consumer is the sampling span. Retain current_sent_bearer_prefix for existing attribution/diagnostic type detection. diagnostics sampling_log forwards structured event/span fields to sampling.jsonl, so fix at the producers. No replacement hash or new runtime abstraction.

## Verification
Capture tracing in memory with a scoped subscriber, build real client requests for both auth schemes using short synthetic keys, emit within the actual request span, and assert logs contain no key/fragments but retain auth metadata. Assert built headers still carry correct credentials. No network or real config access. Run client tests.
