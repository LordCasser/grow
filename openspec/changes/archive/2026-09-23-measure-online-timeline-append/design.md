# Design

Use a test-only ignored benchmark to build deterministic Timeline states and sample live append. Vary event history independently from retained lifecycle-fold cardinality, report latency and allocated bytes, and check rejected acceptance against a pre-operation state snapshot. Keep restoration and artifact decoding outside the live append measurement. Preserve normal transactional `prepare`/`accept` semantics; any optimization requires a separate behavior change.
