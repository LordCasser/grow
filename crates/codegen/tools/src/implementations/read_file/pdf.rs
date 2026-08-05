//! PDF text extraction and page rendering shared by read tools.

use std::fmt::Write as _;

use base64::Engine as _;
use base64::engine::general_purpose;

use crate::types::output::{FileContent, PdfPageImage, PdfPageImages, ReadFileOutput};

use super::metadata::{bytes_to_metadata, is_pdf_magic};

pub const MAX_PDF_BYTES: usize = 50 * 1024 * 1024;
const PDF_AUTO_READ_THRESHOLD: usize = 10;
const PDF_RENDER_DPI: u32 = 150;
const PDF_RENDER_JPEG_QUALITY: u8 = 85;
pub const PDF_PROCESS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Maximum pages per read_file call when using explicit `pages` param.
pub const PDF_MAX_PAGES_PER_READ: usize = 20;

/// Shared async wrapper for document extraction (PDF, PPTX, etc.).
pub async fn run_document_extraction<F>(
    file_bytes: Vec<u8>,
    path: &std::path::Path,
    format_label: &str,
    max_bytes: usize,
    timeout: std::time::Duration,
    extract_fn: F,
) -> Result<ReadFileOutput, tool_runtime::ToolError>
where
    F: FnOnce(Vec<u8>) -> Result<ReadFileOutput, String> + Send + 'static,
{
    if file_bytes.len() > max_bytes {
        return Ok(ReadFileOutput::FileReadError(format!(
            "{format_label} file is {:.1} MB, exceeds the {:.0} MB limit.",
            file_bytes.len() as f64 / 1_048_576.0,
            max_bytes as f64 / 1_048_576.0,
        )));
    }

    tracing::info!(
        size_bytes = file_bytes.len(),
        format_label,
        "processing document"
    );

    let result = tokio::time::timeout(
        timeout,
        tokio::task::spawn_blocking(move || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| extract_fn(file_bytes)))
        }),
    )
    .await;

    match result {
        Ok(Ok(Ok(Ok(mut output)))) => {
            if let ReadFileOutput::FileContent(ref mut fc) = output {
                fc.absolute_path = path.to_path_buf();
            }
            Ok(output)
        }
        Err(_elapsed) => Ok(ReadFileOutput::FileReadError(format!(
            "{format_label} processing timed out after {}s: {}",
            timeout.as_secs(),
            path.display()
        ))),
        Ok(Ok(Ok(Err(e)))) => Ok(ReadFileOutput::FileReadError(e)),
        Ok(Ok(Err(_panic))) => Ok(ReadFileOutput::FileReadError(format!(
            "{format_label} processing failed (internal error): {}",
            path.display()
        ))),
        Ok(Err(e)) => Ok(ReadFileOutput::FileReadError(format!(
            "{format_label} processing failed: {}",
            e
        ))),
    }
}

/// Requested PDF output mode, resolved from the `format` parameter.
#[derive(Clone, Copy)]
enum PdfFormatKind {
    /// No format given: classify pages and route automatically.
    Auto,
    /// Force full-document page-image rendering.
    Render,
    /// Reading-ordered plain text.
    Text,
    /// Markdown-style text via the markdown converter.
    Markdown,
}

pub(crate) async fn handle_pdf(
    file_bytes: Vec<u8>,
    path: &std::path::Path,
    pages: Option<String>,
    format: Option<&str>,
) -> Result<ReadFileOutput, tool_runtime::ToolError> {
    let format_kind = match format {
        None => PdfFormatKind::Auto,
        Some("image") => PdfFormatKind::Render,
        Some("text") => PdfFormatKind::Text,
        Some("markdown") => PdfFormatKind::Markdown,
        Some(other) => {
            return Ok(ReadFileOutput::FileReadError(format!(
                "Invalid format '{}'. Supported values: 'image' (default), 'text', 'markdown'.",
                other
            )));
        }
    };

    run_document_extraction(
        file_bytes,
        path,
        "PDF",
        MAX_PDF_BYTES,
        PDF_PROCESS_TIMEOUT,
        move |bytes| match format_kind {
            PdfFormatKind::Auto => {
                // Classification scan needs the bytes; the chosen extraction
                // path re-opens the document itself.
                if default_route_prefers_text(bytes.clone(), pages.as_deref()) {
                    extract_pdf_text(bytes, pages.as_deref())
                } else {
                    let file_size = bytes.len();
                    render_pdf_pages(bytes, pages.as_deref(), file_size)
                }
            }
            PdfFormatKind::Render => {
                let file_size = bytes.len();
                render_pdf_pages(bytes, pages.as_deref(), file_size)
            }
            PdfFormatKind::Text => extract_pdf_text(bytes, pages.as_deref()),
            PdfFormatKind::Markdown => extract_pdf_markdown(bytes, pages.as_deref()),
        },
    )
    .await
}

/// Decide the default (no `format`) route: text when every requested page has
/// a usable native text layer, full-page rendering as soon as any page is
/// scanned or image-based (early-exit scan).
///
/// Any classification error — including encrypted/unauthenticated documents —
/// conservatively falls back to the historical default (rendering), so the
/// error semantics of the pre-routing behaviour are preserved.
fn default_route_prefers_text(bytes: Vec<u8>, pages_spec: Option<&str>) -> bool {
    let (doc, _page_count, page_indices) = match open_pdf_and_resolve_pages(bytes, pages_spec) {
        Ok(resolved) => resolved,
        Err(_) => return false, // open/parse errors surface unchanged via the render path
    };
    for &page_idx in &page_indices {
        match doc.classify_page(page_idx) {
            Ok(cls) => match cls.kind {
                pdf_oxide::extractors::PageKind::Scanned
                | pdf_oxide::extractors::PageKind::ImageText
                | pdf_oxide::extractors::PageKind::Mixed => return false,
                pdf_oxide::extractors::PageKind::TextLayer
                | pdf_oxide::extractors::PageKind::Empty => continue,
                // Unknown future page kinds: conservative fallback to render.
                _ => return false,
            },
            Err(_) => return false, // conservative fallback to render
        }
    }
    true
}

/// Parse a page range specification into sorted, deduplicated 0-based page indices.
pub fn parse_page_range(spec: &str, page_count: usize) -> Result<Vec<usize>, String> {
    let mut pages = Vec::new();
    for part in spec.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((start, end)) = part.split_once('-') {
            let start: usize = start
                .trim()
                .parse()
                .map_err(|_| format!("invalid page number: '{}'", start.trim()))?;
            let end = if end.trim().is_empty() {
                page_count
            } else {
                end.trim()
                    .parse()
                    .map_err(|_| format!("invalid page number: '{}'", end.trim()))?
            };
            if start < 1 || start > page_count {
                return Err(format!(
                    "page {} out of range (document has {} pages)",
                    start, page_count
                ));
            }
            if start > end {
                return Err(format!(
                    "invalid page range: {}-{} (start must be ≤ end)",
                    start, end
                ));
            }
            let end = end.min(page_count);
            for p in start..=end {
                pages.push(p - 1);
            }
        } else {
            let p: usize = part
                .parse()
                .map_err(|_| format!("invalid page number: '{}'", part))?;
            if p < 1 || p > page_count {
                return Err(format!(
                    "page {} out of range (document has {} pages)",
                    p, page_count
                ));
            }
            pages.push(p - 1);
        }
    }
    pages.sort_unstable();
    pages.dedup();
    if pages.len() > PDF_MAX_PAGES_PER_READ {
        return Err(format!(
            "requested {} pages, maximum is {} per call",
            pages.len(),
            PDF_MAX_PAGES_PER_READ
        ));
    }
    if pages.is_empty() {
        return Err("no pages specified".to_string());
    }
    Ok(pages)
}

fn open_pdf_document(bytes: Vec<u8>) -> Result<(pdf_oxide::PdfDocument, usize), String> {
    let doc = pdf_oxide::PdfDocument::from_bytes(bytes)
        .map_err(|e| format!("Failed to open PDF: {e}"))?;

    let page_count = doc
        .page_count()
        .map_err(|e| format!("Failed to read PDF page count: {e}"))?;

    if page_count == 0 {
        return Err("PDF has no pages".to_string());
    }

    Ok((doc, page_count))
}

fn open_pdf_and_resolve_pages(
    bytes: Vec<u8>,
    pages_spec: Option<&str>,
) -> Result<(pdf_oxide::PdfDocument, usize, Vec<usize>), String> {
    let (doc, page_count) = open_pdf_document(bytes)?;

    let page_indices = match pages_spec {
        Some(spec) => parse_page_range(spec, page_count)?,
        None => {
            if page_count > PDF_AUTO_READ_THRESHOLD {
                return Err(format!(
                    "PDF has {} pages which exceeds the {} page auto-read limit. \
                     Use the `pages` parameter to specify which pages to read \
                     (e.g. pages=\"1-5\"). Maximum {} pages per call.",
                    page_count, PDF_AUTO_READ_THRESHOLD, PDF_MAX_PAGES_PER_READ
                ));
            }
            (0..page_count).collect()
        }
    };

    Ok((doc, page_count, page_indices))
}

pub(crate) fn render_pdf_pages(
    bytes: Vec<u8>,
    pages_spec: Option<&str>,
    file_size: usize,
) -> Result<ReadFileOutput, String> {
    let (doc, page_count, page_indices) = open_pdf_and_resolve_pages(bytes, pages_spec)?;

    let opts = pdf_oxide::rendering::RenderOptions::with_dpi(PDF_RENDER_DPI)
        .as_jpeg(PDF_RENDER_JPEG_QUALITY);

    let mut page_images = Vec::with_capacity(page_indices.len());
    for &page_idx in &page_indices {
        let image = pdf_oxide::rendering::render_page(&doc, page_idx, &opts)
            .map_err(|e| format!("Failed to render page {}: {e}", page_idx + 1))?;

        let b64 = general_purpose::STANDARD.encode(&image.data);
        page_images.push(PdfPageImage {
            data: b64,
            mime_type: "image/jpeg".to_string(),
            page_number: page_idx + 1,
        });
    }

    Ok(ReadFileOutput::PdfPageImages(PdfPageImages {
        pages: page_images,
        total_pages: page_count,
        file_size,
    }))
}

pub fn raw_text_to_file_content(text: String) -> ReadFileOutput {
    let total_lines = text.matches('\n').count() + 1;
    let mut content = String::new();
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            content.push('\n');
        }
        write!(&mut content, "{}\u{2192}{line}", i + 1).ok();
    }

    ReadFileOutput::FileContent(FileContent {
        content,
        content_concise: None,
        absolute_path: std::path::PathBuf::new(),
        offset: None,
        limit: None,
        raw_output: text,
        total_lines,
        extracted_images: Vec::new(),
    })
}

enum PageTextStyle {
    Grow,
    Cursor { total_pages: usize },
}

/// Geometric separator between two reading-ordered spans: `'\n'` when the
/// vertical jump exceeds 0.6 × the page's largest font size (a new line),
/// `' '` when the horizontal gap exceeds 0.5 pt (a word boundary), otherwise
/// nothing (tight same-line run). The vertical check must come first: on a
/// line break the X can regress, producing a negative gap that would
/// otherwise suppress the line break.
fn span_separator(
    prev_x: f32,
    prev_y: f32,
    prev_width: f32,
    cur_x: f32,
    cur_y: f32,
    max_font_size: f32,
) -> Option<char> {
    let dy = (cur_y - prev_y).abs();
    if dy > 0.6 * max_font_size {
        return Some('\n');
    }
    let gap = cur_x - (prev_x + prev_width);
    if gap > 0.5 {
        return Some(' ');
    }
    None
}

fn append_page_body(text: &mut String, doc: &pdf_oxide::PdfDocument, page_idx: usize) {
    // Reading-order aware extraction; falls back to the classic extractor
    // when the typed API fails so the error marker semantics are unchanged.
    let spans = match doc.extract_page_text(page_idx) {
        Ok(page_text) => page_text.spans,
        Err(_) => match doc.extract_text(page_idx) {
            Ok(page_text) => {
                text.push_str(&page_text);
                return;
            }
            Err(e) => {
                writeln!(
                    text,
                    "[Failed to extract text from page {}: {e}]",
                    page_idx + 1
                )
                .ok();
                return;
            }
        },
    };
    let max_font_size = spans
        .iter()
        .map(|span| span.font_size)
        .fold(0.0_f32, f32::max);
    let mut prev_span: Option<&pdf_oxide::layout::TextSpan> = None;
    for span in &spans {
        // Drop /Artifact-marked spans (headers, footers, page numbers,
        // decorations) per ISO 32000-1 §14.8.2.2: pdf_oxide tags them via
        // `artifact_type` but keeps them in the returned spans.
        if span.artifact_type.is_some() {
            continue;
        }
        // Insert the geometric separator against the previous *kept* span
        // (artifact spans are skipped without updating the anchor).
        if let Some(sep) = prev_span.and_then(|prev| {
            span_separator(
                prev.bbox.x,
                prev.bbox.y,
                prev.bbox.width,
                span.bbox.x,
                span.bbox.y,
                max_font_size,
            )
        }) {
            text.push(sep);
        }
        text.push_str(&span.text);
        prev_span = Some(span);
    }
}

/// Append structured extraction warnings (missing ToUnicode maps, xref
/// recovery, spec violations, …) as diagnostic lines mirroring the
/// `[Failed to extract text from page N: ...]` style. `Warning` has no
/// `Display` impl, so the category's `Debug` name is used (PascalCase).
fn append_extraction_warnings(text: &mut String, doc: &pdf_oxide::PdfDocument) {
    for warning in doc.take_structured_warnings() {
        match warning.page {
            Some(page) => {
                writeln!(
                    text,
                    "[PDF extraction warning: {:?} on page {}]",
                    warning.category,
                    page + 1
                )
                .ok();
            }
            None => {
                writeln!(text, "[PDF extraction warning: {:?}]", warning.category).ok();
            }
        }
    }
}

fn extract_page_texts(
    doc: &pdf_oxide::PdfDocument,
    page_indices: &[usize],
    style: PageTextStyle,
) -> Result<String, String> {
    let mut text = String::new();
    for (i, &page_idx) in page_indices.iter().enumerate() {
        if i > 0 {
            text.push('\n');
        }
        match style {
            PageTextStyle::Grow => {
                writeln!(&mut text, "--- Page {} ---", page_idx + 1).ok();
            }
            PageTextStyle::Cursor { .. } => {}
        }
        append_page_body(&mut text, doc, page_idx);
        if let PageTextStyle::Cursor { total_pages } = style {
            text.push_str("\n\n");
            let _ = writeln!(&mut text, "-- {} of {} --", page_idx + 1, total_pages);
            if i + 1 < page_indices.len() {
                text.push('\n');
            }
        }
    }
    Ok(text)
}

fn extract_pdf_plain_text(bytes: Vec<u8>, style: PageTextStyle) -> Result<String, String> {
    let (doc, page_count) = open_pdf_document(bytes)?;
    let page_indices: Vec<usize> = (0..page_count).collect();
    let style = match style {
        PageTextStyle::Grow => PageTextStyle::Grow,
        PageTextStyle::Cursor { .. } => PageTextStyle::Cursor {
            total_pages: page_count,
        },
    };
    extract_page_texts(&doc, &page_indices, style)
}

/// Extract plain text from all PDF pages (no auto-read page limit).
#[cfg(test)]
pub(crate) fn extract_pdf_plain_text_all(bytes: Vec<u8>) -> Result<String, String> {
    extract_pdf_plain_text(bytes, PageTextStyle::Grow)
}

/// Plain text from all PDF pages in the `Read` format.
pub fn extract_pdf_plain_text_cursor(bytes: Vec<u8>) -> Result<String, String> {
    extract_pdf_plain_text(bytes, PageTextStyle::Cursor { total_pages: 0 })
}

pub(crate) fn extract_pdf_text(
    bytes: Vec<u8>,
    pages_spec: Option<&str>,
) -> Result<ReadFileOutput, String> {
    let (doc, _page_count, page_indices) = open_pdf_and_resolve_pages(bytes, pages_spec)?;
    let mut text = extract_page_texts(&doc, &page_indices, PageTextStyle::Grow)?;
    append_extraction_warnings(&mut text, &doc);
    Ok(raw_text_to_file_content(text))
}

/// Markdown-style body of a single page. Page-level failures reuse the
/// text-path error marker so the output skeleton stays uniform.
fn append_page_markdown(text: &mut String, doc: &pdf_oxide::PdfDocument, page_idx: usize) {
    // Contract-frozen converter. 0.3.77 deprecates it in favour of the
    // pipeline converter; that API is out of scope for this change.
    #[allow(deprecated)]
    let converter = pdf_oxide::converters::MarkdownConverter::new();
    let options = pdf_oxide::converters::ConversionOptions {
        detect_headings: true,
        ..Default::default()
    };
    let spans = match doc.extract_spans(page_idx) {
        Ok(spans) => spans,
        Err(e) => {
            writeln!(
                text,
                "[Failed to extract text from page {}: {e}]",
                page_idx + 1
            )
            .ok();
            return;
        }
    };
    // Same /Artifact filtering as the plain-text path (ISO 32000-1 §14.8.2.2).
    let spans: Vec<_> = spans
        .into_iter()
        .filter(|span| span.artifact_type.is_none())
        .collect();
    match converter.convert_page_from_spans(&spans, &options) {
        Ok(markdown) => text.push_str(&markdown),
        Err(e) => {
            writeln!(
                text,
                "[Failed to extract text from page {}: {e}]",
                page_idx + 1
            )
            .ok();
        }
    }
}

/// Extract Markdown-style text from all requested pages.
pub(crate) fn extract_pdf_markdown(
    bytes: Vec<u8>,
    pages_spec: Option<&str>,
) -> Result<ReadFileOutput, String> {
    let (doc, _page_count, page_indices) = open_pdf_and_resolve_pages(bytes, pages_spec)?;
    let mut text = String::new();
    for (i, &page_idx) in page_indices.iter().enumerate() {
        if i > 0 {
            text.push('\n');
        }
        writeln!(&mut text, "--- Page {} ---", page_idx + 1).ok();
        append_page_markdown(&mut text, &doc, page_idx);
    }
    append_extraction_warnings(&mut text, &doc);
    Ok(raw_text_to_file_content(text))
}

/// Three-tier PDF detection: infer metadata, magic bytes, or extension.
pub fn is_pdf_file(file_bytes: &[u8], extension: &str) -> bool {
    bytes_to_metadata(file_bytes).is_ok_and(|m| m.is_pdf())
        || is_pdf_magic(file_bytes)
        || extension == "pdf"
}

/// Minimal multi-page PDF fixture for unit tests.
pub fn make_test_pdf(page_texts: &[&str]) -> Vec<u8> {
    let mut pdf = Vec::new();
    pdf.extend_from_slice(b"%PDF-1.4\n");

    let mut offsets = Vec::new();

    offsets.push(pdf.len());
    pdf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");

    let page_count = page_texts.len();
    let kids: Vec<String> = (0..page_count)
        .map(|i| format!("{} 0 R", 3 + i * 3))
        .collect();
    offsets.push(pdf.len());
    let pages_obj = format!(
        "2 0 obj\n<< /Type /Pages /Kids [{}] /Count {} >>\nendobj\n",
        kids.join(" "),
        page_count
    );
    pdf.extend_from_slice(pages_obj.as_bytes());

    for (i, text) in page_texts.iter().enumerate() {
        let page_obj = 3 + i * 3;
        let content_obj = 4 + i * 3;
        let font_obj = 5 + i * 3;

        let stream_content = format!("BT /F1 12 Tf 72 720 Td ({text}) Tj ET");
        let stream_len = stream_content.len();

        offsets.push(pdf.len());
        pdf.extend_from_slice(
            format!(
                "{page_obj} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
                 /Contents {content_obj} 0 R /Resources << /Font << /F1 {font_obj} 0 R >> >> >>\nendobj\n"
            )
            .as_bytes(),
        );

        offsets.push(pdf.len());
        pdf.extend_from_slice(
            format!(
                "{content_obj} 0 obj\n<< /Length {stream_len} >>\nstream\n{stream_content}\nendstream\nendobj\n"
            )
            .as_bytes(),
        );

        offsets.push(pdf.len());
        pdf.extend_from_slice(
            format!(
                "{font_obj} 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n"
            )
            .as_bytes(),
        );
    }

    let xref_offset = pdf.len();
    let total_objects = 2 + page_count * 3 + 1;
    pdf.extend_from_slice(format!("xref\n0 {total_objects}\n").as_bytes());
    pdf.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        pdf.extend_from_slice(format!("{:010} 00000 n \n", offset).as_bytes());
    }

    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
            total_objects, xref_offset
        )
        .as_bytes(),
    );

    pdf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_pdf_plain_text_all_reads_every_page() {
        let pdf_bytes = make_test_pdf(&["Alpha", "Beta"]);
        let text = extract_pdf_plain_text_all(pdf_bytes).unwrap();
        assert!(text.contains("--- Page 1 ---"));
        assert!(text.contains("--- Page 2 ---"));
        assert!(text.contains("Alpha"));
        assert!(text.contains("Beta"));
    }

    #[test]
    fn extract_pdf_plain_text_cursor_uses_page_of_markers() {
        let pdf_bytes = make_test_pdf(&["Alpha", "Beta"]);
        let text = extract_pdf_plain_text_cursor(pdf_bytes).unwrap();
        assert!(text.contains("Alpha"));
        assert!(text.contains("Beta"));
        assert!(text.contains("-- 1 of 2 --"));
        assert!(text.contains("-- 2 of 2 --"));
        assert!(!text.contains("--- Page"));
    }

    #[test]
    fn extract_pdf_text_returns_file_content() {
        let pdf_bytes = make_test_pdf(&["Hello World"]);
        let result = extract_pdf_text(pdf_bytes, None).unwrap();
        match result {
            ReadFileOutput::FileContent(fc) => {
                assert!(fc.raw_output.contains("Hello World"));
                assert!(fc.raw_output.contains("--- Page 1 ---"));
                assert!(fc.content.contains('\u{2192}'));
            }
            other => panic!("Expected FileContent, got {other:?}"),
        }
    }

    #[test]
    fn extract_pdf_text_multi_page() {
        let pdf_bytes = make_test_pdf(&["Page One", "Page Two"]);
        let result = extract_pdf_text(pdf_bytes, None).unwrap();
        match result {
            ReadFileOutput::FileContent(fc) => {
                assert!(fc.raw_output.contains("--- Page 1 ---"));
                assert!(fc.raw_output.contains("--- Page 2 ---"));
                assert!(fc.raw_output.contains("Page One"));
                assert!(fc.raw_output.contains("Page Two"));
            }
            other => panic!("Expected FileContent, got {other:?}"),
        }
    }

    #[test]
    fn extract_pdf_text_with_page_spec() {
        let pdf_bytes = make_test_pdf(&["First", "Second", "Third"]);
        let result = extract_pdf_text(pdf_bytes, Some("2")).unwrap();
        match result {
            ReadFileOutput::FileContent(fc) => {
                assert!(fc.raw_output.contains("--- Page 2 ---"));
                assert!(fc.raw_output.contains("Second"));
                assert!(!fc.raw_output.contains("--- Page 1 ---"));
                assert!(!fc.raw_output.contains("--- Page 3 ---"));
            }
            other => panic!("Expected FileContent, got {other:?}"),
        }
    }

    #[test]
    fn extract_pdf_text_invalid_pdf() {
        let err = extract_pdf_text(b"not a pdf".to_vec(), None).unwrap_err();
        assert!(err.contains("Failed to open PDF"), "got: {err}");
    }

    #[tokio::test]
    async fn handle_pdf_format_text() {
        let pdf_bytes = make_test_pdf(&["Test Content"]);
        let tmp = tempfile::TempDir::new().unwrap();
        let pdf_path = tmp.path().join("test.pdf");
        std::fs::write(&pdf_path, &pdf_bytes).unwrap();
        let result = handle_pdf(pdf_bytes, &pdf_path, None, Some("text"))
            .await
            .unwrap();
        match result {
            ReadFileOutput::FileContent(fc) => {
                assert!(fc.raw_output.contains("Test Content"));
                assert_eq!(fc.absolute_path, pdf_path);
            }
            other => panic!("Expected FileContent for format='text', got {other:?}"),
        }
    }

    #[tokio::test]
    async fn handle_pdf_format_image() {
        let pdf_bytes = make_test_pdf(&["Some Text"]);
        let path = std::path::Path::new("/tmp/test.pdf");
        let result = handle_pdf(pdf_bytes, path, None, Some("image"))
            .await
            .unwrap();
        assert!(matches!(result, ReadFileOutput::PdfPageImages(_)));
    }

    #[test]
    fn render_pdf_pages_rejects_invalid_pdf() {
        let err = render_pdf_pages(b"not a pdf".to_vec(), None, 10).unwrap_err();
        assert!(err.contains("Failed to open PDF"), "got: {err}");
    }

    #[test]
    fn parse_page_range_single_page() {
        assert_eq!(parse_page_range("3", 10).unwrap(), vec![2]);
    }

    #[test]
    fn parse_page_range_rejects_too_many_pages() {
        let err = parse_page_range("1-21", 30).unwrap_err();
        assert!(err.contains("maximum is"), "got: {err}");
    }

    // ── fixtures ──────────────────────────────────────────────────────────

    /// Single-page PDF whose content stream is exactly `content_stream`.
    fn make_single_page_pdf(content_stream: &str) -> Vec<u8> {
        let mut pdf = Vec::new();
        pdf.extend_from_slice(b"%PDF-1.4\n");
        let mut offsets = Vec::new();
        offsets.push(pdf.len());
        pdf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        offsets.push(pdf.len());
        pdf.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");
        offsets.push(pdf.len());
        pdf.extend_from_slice(
            b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
             /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>\nendobj\n",
        );
        let stream_len = content_stream.len();
        offsets.push(pdf.len());
        pdf.extend_from_slice(
            format!(
                "4 0 obj\n<< /Length {stream_len} >>\nstream\n{content_stream}\nendstream\nendobj\n"
            )
            .as_bytes(),
        );
        offsets.push(pdf.len());
        pdf.extend_from_slice(
            b"5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n",
        );
        let xref_offset = pdf.len();
        pdf.extend_from_slice(b"xref\n0 6\n");
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in &offsets {
            pdf.extend_from_slice(format!("{:010} 00000 n \n", offset).as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
                xref_offset
            )
            .as_bytes(),
        );
        pdf
    }

    /// Single-page PDF with an optional image XObject (`name -> jpeg_bytes`).
    fn make_pdf_with_image(content_stream: &str, image_obj: Option<(&str, Vec<u8>)>) -> Vec<u8> {
        let mut pdf = Vec::new();
        pdf.extend_from_slice(b"%PDF-1.4\n");
        let mut offsets = Vec::new();
        offsets.push(pdf.len());
        pdf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        offsets.push(pdf.len());
        pdf.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");
        let resources = match &image_obj {
            Some((name, _)) => {
                format!("/Resources << /Font << /F1 5 0 R >> /XObject << /{name} 6 0 R >> >>")
            }
            None => "/Resources << /Font << /F1 5 0 R >> >>".to_string(),
        };
        offsets.push(pdf.len());
        pdf.extend_from_slice(
            format!(
                "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
                 /Contents 4 0 R {resources} >>\nendobj\n"
            )
            .as_bytes(),
        );
        let stream_len = content_stream.len();
        offsets.push(pdf.len());
        pdf.extend_from_slice(
            format!(
                "4 0 obj\n<< /Length {stream_len} >>\nstream\n{content_stream}\nendstream\nendobj\n"
            )
            .as_bytes(),
        );
        offsets.push(pdf.len());
        pdf.extend_from_slice(
            b"5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n",
        );
        let mut total_objects = 6;
        if let Some((_, data)) = &image_obj {
            let len = data.len();
            offsets.push(pdf.len());
            pdf.extend_from_slice(
                format!(
                    "6 0 obj\n<< /Type /XObject /Subtype /Image /Width 400 /Height 400 \
                     /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {len} >>\nstream\n"
                )
                .as_bytes(),
            );
            pdf.extend_from_slice(data);
            pdf.extend_from_slice(b"\nendstream\nendobj\n");
            total_objects = 7;
        }
        let xref_offset = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {total_objects}\n").as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in &offsets {
            pdf.extend_from_slice(format!("{:010} 00000 n \n", offset).as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {total_objects} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
                xref_offset
            )
            .as_bytes(),
        );
        pdf
    }

    /// Small solid-colour JPEG, good enough for page classification.
    fn make_jpeg() -> Vec<u8> {
        let img = image::RgbImage::from_pixel(400, 400, image::Rgb([10, 20, 30]));
        let mut jpeg_bytes = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut jpeg_bytes),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        jpeg_bytes
    }

    /// `make_test_pdf` with the final object's `endobj` removed, which makes
    /// pdf_oxide raise a document-scoped `EofPremature` structured warning
    /// while still extracting fine.
    fn make_eof_truncated_pdf(page_texts: &[&str]) -> Vec<u8> {
        let mut bytes = make_test_pdf(page_texts);
        let marker = b"\nendobj\nxref".to_vec();
        if let Some(pos) = bytes.windows(marker.len()).position(|w| w == marker) {
            bytes.drain(pos..pos + marker.len());
            bytes.insert(pos, b'\n');
        }
        bytes
    }

    // ── default smart routing ─────────────────────────────────────────────

    #[tokio::test]
    async fn handle_pdf_default_routes_text_layer_to_file_content() {
        let pdf_bytes = make_test_pdf(&["Hello World"]);
        let path = std::path::Path::new("/tmp/text-layer.pdf");
        let result = handle_pdf(pdf_bytes, path, None, None).await.unwrap();
        assert!(
            matches!(result, ReadFileOutput::FileContent(_)),
            "text-layer PDF without format must route to text, got {result:?}"
        );
        if let ReadFileOutput::FileContent(fc) = result {
            assert!(fc.raw_output.contains("Hello World"));
        }
    }

    #[tokio::test]
    async fn handle_pdf_default_routes_scanned_page_to_images() {
        let pdf_bytes =
            make_pdf_with_image("q 0 0 612 792 cm /Im1 Do Q", Some(("Im1", make_jpeg())));
        let path = std::path::Path::new("/tmp/scanned.pdf");
        let result = handle_pdf(pdf_bytes, path, None, None).await.unwrap();
        assert!(
            matches!(result, ReadFileOutput::PdfPageImages(_)),
            "scanned PDF without format must route to rendering, got {result:?}"
        );
    }

    #[tokio::test]
    async fn handle_pdf_default_routes_image_text_page_to_images() {
        let pdf_bytes = make_pdf_with_image(
            "BT /F1 12 Tf 72 720 Td (This is a longer body of text with enough words) Tj ET\n\
             q 72 200 300 300 cm /Im1 Do Q",
            Some(("Im1", make_jpeg())),
        );
        let path = std::path::Path::new("/tmp/hybrid.pdf");
        let result = handle_pdf(pdf_bytes, path, None, None).await.unwrap();
        assert!(
            matches!(result, ReadFileOutput::PdfPageImages(_)),
            "image-text PDF without format must route to rendering, got {result:?}"
        );
    }

    #[tokio::test]
    async fn handle_pdf_default_routes_classify_error_to_images() {
        // /MediaBox [0 0] makes classify_page fail (InvalidPdf); the default
        // route must conservatively fall back to the historical render path
        // (which itself tolerates the bad MediaBox and renders images).
        let mut pdf_bytes = make_test_pdf(&["Hello"]);
        let mb_marker = b"/MediaBox [0 0 612 792]".to_vec();
        if let Some(pos) = pdf_bytes
            .windows(mb_marker.len())
            .position(|w| w == mb_marker)
        {
            pdf_bytes.splice(
                pos..pos + mb_marker.len(),
                b"/MediaBox [0 0]".iter().copied(),
            );
        }
        let path = std::path::Path::new("/tmp/bad-mediabox.pdf");
        let result = handle_pdf(pdf_bytes, path, None, None).await.unwrap();
        assert!(
            matches!(result, ReadFileOutput::PdfPageImages(_)),
            "classify failure must fall back to rendering, got {result:?}"
        );
    }

    #[tokio::test]
    async fn handle_pdf_rejects_unknown_format() {
        let pdf_bytes = make_test_pdf(&["Hello World"]);
        let path = std::path::Path::new("/tmp/unknown.pdf");
        let result = handle_pdf(pdf_bytes, path, None, Some("bogus"))
            .await
            .unwrap();
        match result {
            ReadFileOutput::FileReadError(msg) => {
                assert!(
                    msg.contains("'image' (default), 'text', 'markdown'"),
                    "supported-values message must list all formats, got: {msg}"
                );
            }
            other => panic!("Expected FileReadError, got {other:?}"),
        }
    }

    // ── reading order / artifact filtering (text path) ────────────────────

    #[test]
    fn extract_pdf_text_two_column_uses_top_to_bottom_order() {
        // pdf_oxide's default reading order is geometric top-to-bottom
        // (Y desc, then X asc): the two columns interleave row by row rather
        // than column-major. This test pins the actual span order.
        let pdf_bytes = make_single_page_pdf(
            "BT /F1 12 Tf 72 700 Td (Left One) Tj ET\n\
             BT /F1 12 Tf 72 680 Td (Left Two) Tj ET\n\
             BT /F1 12 Tf 360 700 Td (Right One) Tj ET\n\
             BT /F1 12 Tf 360 680 Td (Right Two) Tj ET",
        );
        let text = extract_pdf_text(pdf_bytes, None).unwrap();
        if let ReadFileOutput::FileContent(fc) = text {
            let body = fc.raw_output;
            let left_one = body.find("Left One").expect("Left One present");
            let right_one = body.find("Right One").expect("Right One present");
            let left_two = body.find("Left Two").expect("Left Two present");
            let right_two = body.find("Right Two").expect("Right Two present");
            assert!(
                left_one < right_one && right_one < left_two && left_two < right_two,
                "expected row-major (Y desc, X asc) order, got: {body}"
            );
        } else {
            panic!("Expected FileContent");
        }
    }

    #[test]
    fn extract_pdf_text_skips_artifact_marked_header() {
        let pdf_bytes = make_single_page_pdf(
            "/Artifact << /Type /Pagination /Subtype /Header >> BDC\n\
             BT /F1 12 Tf 72 750 Td (Running Header) Tj ET\n\
             EMC\n\
             BT /F1 12 Tf 72 700 Td (Body Text) Tj ET",
        );
        let result = extract_pdf_text(pdf_bytes, None).unwrap();
        match result {
            ReadFileOutput::FileContent(fc) => {
                assert!(
                    fc.raw_output.contains("Body Text"),
                    "body text must survive artifact filtering"
                );
                assert!(
                    !fc.raw_output.contains("Running Header"),
                    "/Artifact-marked header must be dropped from text output"
                );
            }
            other => panic!("Expected FileContent, got {other:?}"),
        }
    }

    // ── markdown path ──────────────────────────────────────────────────────

    #[test]
    fn extract_pdf_markdown_preserves_page_structure() {
        // Two lines at different Y coordinates render as two markdown lines;
        // the converter emits no `#` heading markers (block-level converter).
        let pdf_bytes = make_single_page_pdf(
            "BT /F1 24 Tf 72 700 Td (Big Title) Tj ET\n\
             BT /F1 12 Tf 72 660 Td (Some body text here.) Tj ET",
        );
        let result = extract_pdf_markdown(pdf_bytes, None).unwrap();
        match result {
            ReadFileOutput::FileContent(fc) => {
                assert!(fc.raw_output.contains("--- Page 1 ---"));
                assert!(fc.raw_output.contains("Big Title"));
                assert!(fc.raw_output.contains("Some body text here."));
                let title_pos = fc.raw_output.find("Big Title").unwrap();
                let body_pos = fc.raw_output.find("Some body text here.").unwrap();
                assert!(
                    title_pos < body_pos,
                    "title line must precede body line: {}",
                    fc.raw_output
                );
            }
            other => panic!("Expected FileContent, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn handle_pdf_format_markdown_returns_file_content() {
        let pdf_bytes = make_test_pdf(&["Hello World"]);
        let tmp = tempfile::TempDir::new().unwrap();
        let pdf_path = tmp.path().join("doc.pdf");
        std::fs::write(&pdf_path, &pdf_bytes).unwrap();
        let result = handle_pdf(pdf_bytes, &pdf_path, None, Some("markdown"))
            .await
            .unwrap();
        match result {
            ReadFileOutput::FileContent(fc) => {
                assert!(fc.raw_output.contains("Hello World"));
                assert!(fc.raw_output.contains("--- Page 1 ---"));
                assert_eq!(fc.absolute_path, pdf_path);
            }
            other => panic!("Expected FileContent for format='markdown', got {other:?}"),
        }
    }

    #[test]
    fn extract_pdf_markdown_with_page_spec() {
        let pdf_bytes = make_test_pdf(&["First", "Second", "Third"]);
        let result = extract_pdf_markdown(pdf_bytes, Some("2")).unwrap();
        match result {
            ReadFileOutput::FileContent(fc) => {
                assert!(fc.raw_output.contains("--- Page 2 ---"));
                assert!(fc.raw_output.contains("Second"));
                assert!(!fc.raw_output.contains("--- Page 1 ---"));
                assert!(!fc.raw_output.contains("--- Page 3 ---"));
            }
            other => panic!("Expected FileContent, got {other:?}"),
        }
    }

    #[test]
    fn extract_pdf_markdown_invalid_pdf() {
        let err = extract_pdf_markdown(b"not a pdf".to_vec(), None).unwrap_err();
        assert!(err.contains("Failed to open PDF"), "got: {err}");
    }

    #[test]
    fn extract_pdf_markdown_skips_artifact_spans() {
        let pdf_bytes = make_single_page_pdf(
            "/Artifact << /Type /Pagination /Subtype /Footer >> BDC\n\
             BT /F1 12 Tf 72 40 Td (Page 1 of 1) Tj ET\n\
             EMC\n\
             BT /F1 12 Tf 72 700 Td (Main Content) Tj ET",
        );
        let result = extract_pdf_markdown(pdf_bytes, None).unwrap();
        match result {
            ReadFileOutput::FileContent(fc) => {
                assert!(fc.raw_output.contains("Main Content"));
                assert!(
                    !fc.raw_output.contains("Page 1 of 1"),
                    "/Artifact-marked footer must be dropped from markdown output"
                );
            }
            other => panic!("Expected FileContent, got {other:?}"),
        }
    }

    // ── structured warning diagnostics ─────────────────────────────────────

    #[test]
    fn extract_pdf_text_appends_warning_diagnostics() {
        let pdf_bytes = make_eof_truncated_pdf(&["Hello World"]);
        let result = extract_pdf_text(pdf_bytes, None).unwrap();
        match result {
            ReadFileOutput::FileContent(fc) => {
                assert!(fc.raw_output.contains("Hello World"));
                assert!(
                    fc.raw_output
                        .contains("[PDF extraction warning: EofPremature]"),
                    "diagnostic line must be appended, got: {}",
                    fc.raw_output
                );
            }
            other => panic!("Expected FileContent, got {other:?}"),
        }
    }

    #[test]
    fn extract_pdf_markdown_appends_warning_diagnostics() {
        let pdf_bytes = make_eof_truncated_pdf(&["Hello World"]);
        let result = extract_pdf_markdown(pdf_bytes, None).unwrap();
        match result {
            ReadFileOutput::FileContent(fc) => {
                assert!(fc.raw_output.contains("Hello World"));
                assert!(
                    fc.raw_output
                        .contains("[PDF extraction warning: EofPremature]"),
                    "diagnostic line must be appended, got: {}",
                    fc.raw_output
                );
            }
            other => panic!("Expected FileContent, got {other:?}"),
        }
    }

    #[test]
    fn append_extraction_warnings_formats_page_scoped_warnings() {
        // Page-scoped warnings (no real-world trigger reaches the
        // document-level sink today) are injected via pdf_oxide's public
        // diagnostic API to pin the `on page N` format.
        let pdf_bytes = make_test_pdf(&["Hello"]);
        let doc = pdf_oxide::PdfDocument::from_bytes(pdf_bytes).unwrap();
        doc.push_structured_warning(pdf_oxide::extractors::Warning {
            category: pdf_oxide::extractors::WarningCategory::ToUnicodeMissing,
            page: Some(2),
            message: "type0 font has no ToUnicode entry".to_string(),
            spec_section: Some("9.10.2"),
        });
        let mut text = String::new();
        append_extraction_warnings(&mut text, &doc);
        assert_eq!(
            text,
            "[PDF extraction warning: ToUnicodeMissing on page 3]\n"
        );
    }

    // ── geometric span separators ─────────────────────────────────────────

    #[test]
    fn span_separator_same_line_gap_rules() {
        // Tight same-line spans (gap ≤ 0.5) must not get a separator.
        assert_eq!(span_separator(72.0, 700.0, 20.0, 92.3, 700.0, 12.0), None);
        // The 0.5 pt threshold itself is exclusive: exactly 0.5 stays tight.
        assert_eq!(span_separator(72.0, 700.0, 20.0, 92.5, 700.0, 12.0), None);
        // Anything beyond the word threshold gets a space.
        assert_eq!(
            span_separator(72.0, 700.0, 20.0, 92.51, 700.0, 12.0),
            Some(' ')
        );
        // A tiny same-line y jitter stays on the line (no newline).
        assert_eq!(
            span_separator(72.0, 700.0, 20.0, 300.0, 701.0, 12.0),
            Some(' ')
        );
    }

    #[test]
    fn span_separator_line_break_rules() {
        // Vertical jump beyond 0.6 × max font size breaks the line.
        assert_eq!(
            span_separator(72.0, 700.0, 20.0, 72.0, 680.0, 12.0),
            Some('\n')
        );
        // Within the 0.6 × max_font_size band (dy 7.0 < 7.2) the span stays
        // on the line; the band is sized to absorb same-line baseline jitter.
        assert_eq!(span_separator(72.0, 700.0, 20.0, 72.0, 693.0, 12.0), None);
        // Just past the threshold (dy 7.5 > 7.2) breaks.
        assert_eq!(
            span_separator(72.0, 700.0, 20.0, 72.0, 692.5, 12.0),
            Some('\n')
        );
        // Y wins over X: on a line break the X can regress to a negative gap,
        // which must not suppress the newline.
        assert_eq!(
            span_separator(360.0, 700.0, 54.0, 72.0, 680.0, 12.0),
            Some('\n')
        );
    }

    #[test]
    fn extract_pdf_text_joins_same_line_spans_with_space() {
        // Two text blocks on the same baseline separated by a large x gap
        // stay separate spans (pdf_oxide only merges gaps ≲ word width), so
        // the geometric rule must insert a space between them.
        let pdf_bytes = make_single_page_pdf(
            "BT /F1 12 Tf 72 700 Td (Left One) Tj ET\n\
             BT /F1 12 Tf 360 700 Td (Right One) Tj ET",
        );
        let result = extract_pdf_text(pdf_bytes, None).unwrap();
        match result {
            ReadFileOutput::FileContent(fc) => {
                assert!(
                    fc.raw_output.contains("Left One Right One"),
                    "same-line spans with a gap must join with a space, got: {}",
                    fc.raw_output
                );
            }
            other => panic!("Expected FileContent, got {other:?}"),
        }
    }

    #[test]
    fn extract_pdf_text_separates_lines_with_newline() {
        // Two text blocks on different baselines must be joined with a
        // newline, not glued together.
        let pdf_bytes = make_single_page_pdf(
            "BT /F1 12 Tf 72 700 Td (First Line) Tj ET\n\
             BT /F1 12 Tf 72 680 Td (Second Line) Tj ET",
        );
        let result = extract_pdf_text(pdf_bytes, None).unwrap();
        match result {
            ReadFileOutput::FileContent(fc) => {
                assert!(
                    fc.raw_output.contains("First Line\nSecond Line"),
                    "cross-line spans must be joined with a newline, got: {}",
                    fc.raw_output
                );
            }
            other => panic!("Expected FileContent, got {other:?}"),
        }
    }

    #[test]
    fn extract_pdf_text_two_column_geometry() {
        // Two rows × two columns: row-major reading order with a space
        // between the columns and a newline between the rows. This also
        // pins the Y-before-X rule (the column wrap regresses X).
        let pdf_bytes = make_single_page_pdf(
            "BT /F1 12 Tf 72 700 Td (Left One) Tj ET\n\
             BT /F1 12 Tf 72 680 Td (Left Two) Tj ET\n\
             BT /F1 12 Tf 360 700 Td (Right One) Tj ET\n\
             BT /F1 12 Tf 360 680 Td (Right Two) Tj ET",
        );
        let result = extract_pdf_text(pdf_bytes, None).unwrap();
        match result {
            ReadFileOutput::FileContent(fc) => {
                assert!(
                    fc.raw_output
                        .contains("Left One Right One\nLeft Two Right Two"),
                    "expected row-major joins, got: {}",
                    fc.raw_output
                );
            }
            other => panic!("Expected FileContent, got {other:?}"),
        }
    }
}
