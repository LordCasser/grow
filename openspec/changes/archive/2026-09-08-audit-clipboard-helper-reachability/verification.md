# Evidence

Current branch: main. Read-only source audit; no tests or real clipboard operations were run for the audit.

- `client-support/src/clipboard.rs::platform::extension_for_class` is cfg(test), pub(super), inside the macOS platform module. Repository searches found only its definition and `tests::macos_helpers::extension_mapping` import/assertions. It cannot participate in production builds. Its extra JPEGAufs and unknown→bin mappings do not validate production behavior.
- Production `read_clipboard_image_from_class` matches PNGf/TIFF/JPEG directly to the supplied isolated paths and MIME; unknown classes return None. Native reads use `native_image_type_from_types`. Both are real production paths and must remain.
- `pager-render::clipboard::attachment_probe_would_run` is called by production `system_clipboard_probe_attachments`; it is not merely a self-test helper. The underlying gate preserves unavailable-snapshot semantics.
- `pager::tips::clipboard_focus::run_clipboard_check` is passed into the real app poll. It is not unused.
- `client-support/examples/clipboard_probe.rs` is an explicit benchmark harness invoking real get_attachments and reporting time/outcome. Its documented native/fallback comparison exercises the active GROW_CLIPBOARD_NO_NATIVE_READ switch; no removal proposed, and the benchmark was not run against the user's clipboard.

R16 proposes removing only extension_for_class and its one dedicated test module. No source code or feature has been deleted. Existing candidates R1–R15 remain pending user confirmation.

## Cleanup after verification
After confirming no cargo/rustc/linker processes were running, cargo clean removed 12,011 files (6.8 GiB). Filesystem available space afterward: approximately 77 GiB. Spec/active strict validation passed 15 items; archived strict validation passed 148 items.
