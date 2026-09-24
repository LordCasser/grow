## Decision

The existing mixed-client buffer should retain only events that might be discarded. Untagged ACP and Grow notifications are independent of the candidate; they are durably appended separately and never retracted with an attempt. Sending them through the usual live route removes an input stream that is not controlled by the provider response cap. Accepted candidate content is delivered later, so the live presentation order may differ from durable timeline order. Event IDs and replay retain the canonical order.

Do not disconnect observers on buffer overflow: a disconnect of the sole subscriber evicts the session and can terminate the active attempt. A future hard candidate ceiling must either use a durable-independent bounded spool or provide an explicit resync protocol that preserves session ownership. This change does not claim that a measured RSS workload is a hard process ceiling.

The leader's client delivery and in-flight `session/load` buffers remain separate boundaries. This change does not claim a process-wide RSS ceiling; the existing built-binary benchmark is a measured workload, not a proof of such a ceiling.
