# Design

Perform a synchronous admission pass over the input vector before the first await. Admit up to 25 images whose cumulative base64 payload is at most 80,000,000 bytes; if one image cannot fit, drop it and allow a later smaller image to use remaining capacity. Preserve original indexes and give precise count/byte-limit reasons. This releases rejected encoded payloads before compute starts. Reserve the admitted byte total with one atomic process-wide counter capped at 160,000,000 bytes. A failed reservation drops the admitted batch with an explicit capacity reason rather than waiting with its payload in memory.

Hold the reservation in an `Arc` and clone it into each blocking normalization closure. If its async owner is canceled, the clone in the current closure keeps the reservation charged until that closure exits, matching the existing worker-permit lifetime. The batch processes images sequentially and preserves input-index order for image outcomes and drop grouping.

Set the decode ceiling to 50,000,000 pixels. A 48 MP camera remains accepted; images above 50 MP are dropped before full pixel decode. Keep the provider's 178,956,970-pixel ceiling for persisted-image validity checks; it is a provider constraint rather than the client compute budget. Preserve existing original-image fallback when re-encoding fails within the admitted limits.
