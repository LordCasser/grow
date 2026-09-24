# Verification

- `cargo test --locked --offline -p shell --lib session::image_normalize::tests -- --test-threads=2`: 51 passed, one ignored. After adding the cancellation-reservation regression, its targeted test passed separately.
- The manual 48 MP camera fixture (`8000×6000`) normalized successfully in 20.1 s with 352,141,312 bytes maximum RSS (`/usr/bin/time -l`). The existing 20.16 MP fixture normalized successfully in 9.55 s with 202,932,224 bytes maximum RSS. These represent flat-color JPEGs and do not prove a hard RSS ceiling for all formats; the 50 MP pixel and encoded-input limits are the enforced budget.
- Unit tests cover admission count/bytes, atomic concurrent reservations, reservation lifetime after waiter cancellation, and rejection of a 67 MP JPEG header before full decode. The provider-side persisted-image ceiling is unchanged.
- The process-wide 160 MB limit covers encoded payloads owned by the normalizer's admitted batches, including a canceled worker. OS clipboard acquisition, inline media rendering, and external terminal caches use separate existing limits; this change does not claim a process-wide RSS ceiling across those domains.
