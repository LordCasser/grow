# Design

Retain current signature assignment pending evidence of a supported route sending split signature fragments. Do not invent concatenation heuristics, base64 guessing or provider-name configuration. Signature is opaque; either replacement or concatenation can corrupt it if applied to the wrong wire semantics.

The prior real proxy trace had one signature delta following a signature-less thinking start. It proves late arrival, not fragmentation. The current late-signature fix and unsigned-response portable fallback remain valid independently of this ambiguity.

A future split-signature report must include the event sequence for one block and evidence that the resulting next request is accepted under the proposed assembly. A byte-level HTTP/SSE packet split is not equivalent to multiple signature_delta events. No further paid proxy request was made during this audit.
