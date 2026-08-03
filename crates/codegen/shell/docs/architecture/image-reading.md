# Image reading

`read_file` image handling is selected solely by the resolved
`models.image_description` configuration.

## Invariants

- `image_description = None` means no auxiliary image route. Image files and
  rendered PDF pages remain multimodal `tool_result` content for the active
  model. Any active-model vision failure follows the normal sampling failure
  path.
- `image_description = Some(model)` means that configured model owns image
  interpretation. The session describes `ReadFileOutput::ImageContent` and
  `ReadFileOutput::PdfPageImages` before adding the tool result to model
  context, then stores only the textual description in that result.
- An explicitly configured auxiliary model is never silently replaced by the
  active model. Resolution, transport, or empty-response failures become a
  visible text failure in the tool result and contain no inline image.
- PDF `format="text"` remains ordinary `FileContent`; only rendered-page output
  enters the image route.
- User message attachments are independent of this setting. They are persisted
  for workspace reuse and remain structural image content for the active model.

## Ownership

- Configuration preserves `Option<String>` through `Config`, subagent context,
  session spawn, and `SessionActor`; no earlier layer may collapse `None` to the
  active model name.
- `image_describe` owns prompt construction, bounded conversation context,
  auxiliary sampling, output sanitization, and session-local description cache.
- Tool-result routing remains in `handle_bridge_tool_success`, where the typed
  `ReadFileOutput` variant is available.
