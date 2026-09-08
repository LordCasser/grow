## Why
A proxy can expose different upstream model families under one provider. Model switching already resets native continuation by full model ID, but Messages decoding requires a signature in the initial thinking block and native rejection recovery only accepts API 400. Missing-signature serialization failures therefore terminate instead of recovering.

## What Changes
Allow an absent initial thinking signature to arrive through subsequent signature deltas; never retain unsigned thinking as native continuation. When a native-bearing request fails specifically on a missing signature field, clear native continuation through the acknowledged reset and retry portable context once. Preserve durable conversation and tool results.
