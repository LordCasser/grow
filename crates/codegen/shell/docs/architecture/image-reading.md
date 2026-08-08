# Image and document reading

`read_file` has one textual projection (`FileContent`) and one direct image-file
projection (`ImageContent`). Converted document text and images extracted from
PDFs coexist in `FileContent`: text remains Markdown while
`FileContent.extracted_images` carries raster payloads to the session layer.

## Document pipeline

- Office, OpenDocument, RTF, EPUB, and CSV content is converted to
  GitHub-Flavored Markdown by `anydoc`. PDF text uses the same
  `pdf-inspector` extraction engine that backs anydoc's PDF converter, because
  the routing layer also needs its per-page OCR decisions.
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
3. Pages reported as needing OCR are passed to `pdf-rs`; raster image XObjects
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

- With no `models.image_description`, direct image files and PDF-extracted
  images remain multimodal content for the active model.
- With `models.image_description = Some(model)`, that configured model owns
  interpretation of both `ImageContent` and `FileContent.extracted_images`.
  Its textual description is appended to the tool result; raw images are not
  also sent to the active model.
- An explicitly configured auxiliary model is never silently replaced by the
  active model. Resolution, transport, or empty-response failures become a
  visible textual failure.
- Image-description requests inherit the configured model's sampling
  parameters. User message attachments remain independent of this setting.

`image_describe` owns prompt construction, bounded context, auxiliary sampling,
output sanitization, and its session-local cache. Typed tool-result routing
remains in `handle_bridge_tool_success`.
