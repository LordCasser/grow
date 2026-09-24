# Design: Verify list layout cache width ownership

Trace `prepare_layout` from its `effective_width` calculation through the per-physical-item `height_cache`, `ListLayoutCache`, and `LayoutStamp`. Verify that full resize, scrollbar phase changes, and the rare phase-2 scrollbar correction keep the cached heights and recorded width aligned. Then inspect every production owner that uses `WrapMode::Wrap` for same-count content changes and verify that its owner invalidates/rebuilds before the next render. Fixed-height list owners do not consume the variable-height cache for row geometry.
