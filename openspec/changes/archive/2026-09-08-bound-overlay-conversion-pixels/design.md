## Evidence
PromptImagePreviewPreparation::run calls prepare_kitty_overlay_image_bytes for non-PNG data; ImageViewerState uses the same overlay conversion. The former dimensions validation is header-only, and the conversion currently tries sips before Rust decode. tools image transcode already uses a 16,000,000 source-pixel limit, but the overlay does not use that guarded entry.
## Design
Add a private header-based admission predicate in terminal/image.rs, using the existing unrestricted header validator and u64 multiplication. Check it immediately after the direct-PNG branch and before sips or ImageReader::decode. Keep backend/color-profile behavior for accepted images.
## Limits
This bounds source pixel count for Grow conversion, not RSS, output bytes, process duration or terminal-side PNG decoding. sips temporary-file ownership and process/output limits are separate recorded debts. No conversion retry for rejected dimensions.
