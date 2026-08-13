# Image and document reading

`read_file` has one textual projection (`FileContent`) and one direct image-file
projection (`ImageContent`). Converted document text and images extracted from
PDFs coexist in `FileContent`: text remains Markdown while
`FileContent.extracted_images` carries raster payloads to the session layer.

## Document pipeline

- Office, OpenDocument, RTF, EPUB, and CSV content is converted to
  GitHub-Flavored Markdown by `anydoc`. PDF text uses the same
  `pdf-inspector` extraction engine that backs anydoc's PDF converter, because
  the routing layer also needs its per-page text/raster classification.
- Content signatures take precedence over the filename; the extension is a
  fallback for formats such as CSV that have no reliable magic bytes.
- Converted Markdown uses the same line windowing, token limit, cursor rules,
  concise projection, and streaming path as ordinary text files.
- `hashline_read` keeps ordinary line numbers for converted binary documents.
  Generated Markdown has no stable relationship to editable source bytes, so
  hash anchors would be misleading.

## PDF routing

PDFs do not use page rendering:

1. `pdf-inspector` performs full-page classification and Markdown extraction.
2. Text remains in `FileContent`.
3. Pages without a usable text layer are passed to `pdf-rs`; raster image XObjects
   are extracted from page resources and nested Form XObjects.
4. Extracted images are normalized by the existing conversation image path and
   stored in `FileContent.extracted_images` in page/resource order.

This supports mixed PDFs without discarding their text layer. A scanned page
whose content is not represented by an extractable raster XObject is reported
explicitly; `read_file` does not silently reintroduce a renderer.

`pages` selects up to 20 PDF pages. `format="image"` skips PDF text extraction
and extracts raster images only; when `pages` is omitted it is limited to the
first automatic request of at most 10 pages. Both parameters are invalid for
non-PDF inputs. No `text` or `markdown` format aliases exist.

## Image-model ownership

- `read_file` always returns its existing typed projection: direct image files
  become base64 `ImageContent`, while PDF text and ordered
  `FileContent.extracted_images` coexist. It does not expose arbitrary binary
  files as base64.
- An active runtime whose image capability is unknown receives the original
  image first. Configuring `models.image_description` does not preempt this
  first multimodal attempt.
- After an explicit image-type HTTP 400 proves the active runtime accepts text
  only, chat-state groups every `User`/`ToolResult` message's attachments in
  order. A distinct configured auxiliary runtime gets one description request
  per group. Successful groups are permanently replaced by sanitized text;
  unavailable, empty, timed-out, or failed groups are permanently replaced by
  an explicit removal marker.
- A known text-only runtime applies the same conversion/removal gate to later
  `read_file` results before sampling, without manufacturing another 400.
  Auxiliary runtimes have their own capability identity and may not resolve to
  the already rejected active runtime.
- There is no OCR service fallback in this pipeline. PDF raster extraction is
  not OCR, and an auxiliary-model failure removes the image from conversation
  history. The source file and user attachment stored under session assets are
  not deleted, so a later `read_file` can create a new image-bearing message.

`image_describe` owns prompt construction, bounded context, auxiliary sampling,
structured error retention, output sanitization, and its session-local cache.
Canonical replacement and persistence remain owned by the chat-state actor.
