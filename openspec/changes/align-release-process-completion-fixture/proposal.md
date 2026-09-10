# Why

The completed explicit Turn protocol is included in v2.1.6. The independent-process mock still returns a bare provider terminator for ordinary final answers, which no longer satisfies the archived model-sampling contract.

# What changes

Make only ordinary final-response fixture branches emit visible answer text and a unique, standalone FinishTurn call after asserting the host advertises it. Business-tool responses and tool-free Sideband responses retain their existing paths. Preserve all ten process groups and their inference-count/lifecycle assertions.

# Capabilities

No production behavior or contract changes; skip_specs is true because this aligns a test fixture with the archived explicit completion protocol.
