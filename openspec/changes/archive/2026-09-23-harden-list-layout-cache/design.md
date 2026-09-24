# Design: Harden list layout cache boundaries

`virtual_y` and `item_height` will return `Option`, using the actual item count as the boundary for both cache variants. Existing in-range geometry remains unchanged. Callers that traverse a known visible range skip absent item heights or use the total-height endpoint for stale y lookups.

`extend_heights` will consume the append iterator for either variant. `Variable` appends each height and its prefix sum; `FixedHeight` increases its count by the number of appended items, since its representation defines every row as one line. Prefix-sum overflow and other geometry boundaries remain out of scope.
