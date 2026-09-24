# Design

`serde_json::to_writer` sends the serialized `DigestInput` bytes incrementally to a small `std::io::Write` implementation that updates a `Sha256` state. Finalizing that state yields the same digest as hashing the byte vector returned by `serde_json::to_vec`, without retaining a second complete serialized representation.

Serialization errors continue to propagate through the existing `serde_json::Error` result. The adapter's write and flush operations are infallible in practice and report the complete input length.
