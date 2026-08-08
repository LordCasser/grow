//! PDF text extraction and raster-image routing shared by read tools.

use std::io::Cursor;

use base64::Engine as _;
use image::{DynamicImage, GrayImage, ImageFormat, RgbImage};
use pdf::enc::StreamFilter;
use pdf::file::FileOptions;
use pdf::object::{ColorSpace, ImageXObject, Resources, XObject};
use pdf_inspector::{DetectionConfig, PdfOptions, ScanStrategy};

use crate::util::base64_images::ExtractedImage;

use super::document::run_bounded_document_task;
use super::image::compress_image_for_conversation;

/// Maximum pages selected explicitly in one `read_file` call.
pub const PDF_MAX_PAGES_PER_READ: usize = 20;

const MAX_IMAGES_PER_PAGE: usize = 4;
const MAX_EXTRACTED_IMAGES: usize = 20;
const PDF_AUTO_IMAGE_PAGE_LIMIT: usize = 10;
const MIN_IMAGE_PIXELS: u64 = 16_384;
const MAX_FORM_DEPTH: usize = 8;

#[derive(Debug)]
pub struct PdfExtraction {
    pub markdown: String,
    pub page_count: usize,
    pub visual_pages: Vec<usize>,
    pub images: Vec<ExtractedImage>,
}

pub async fn extract_pdf(
    file_bytes: Vec<u8>,
    path: &std::path::Path,
    pages: Option<String>,
    images_only: bool,
) -> Result<PdfExtraction, String> {
    run_bounded_document_task(file_bytes, path, "PDF", move |bytes| {
        extract_pdf_inner(bytes, pages.as_deref(), images_only)
    })
    .await
}

/// Parse a page range specification into sorted, deduplicated 0-based indices.
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
                    "page {start} out of range (document has {page_count} pages)"
                ));
            }
            if start > end {
                return Err(format!(
                    "invalid page range: {start}-{end} (start must be ≤ end)"
                ));
            }
            pages.extend((start..=end.min(page_count)).map(|page| page - 1));
        } else {
            let page: usize = part
                .parse()
                .map_err(|_| format!("invalid page number: '{part}'"))?;
            if page < 1 || page > page_count {
                return Err(format!(
                    "page {page} out of range (document has {page_count} pages)"
                ));
            }
            pages.push(page - 1);
        }
    }
    pages.sort_unstable();
    pages.dedup();
    if pages.len() > PDF_MAX_PAGES_PER_READ {
        return Err(format!(
            "requested {} pages, maximum is {PDF_MAX_PAGES_PER_READ} per call",
            pages.len()
        ));
    }
    if pages.is_empty() {
        return Err("no pages specified".to_string());
    }
    Ok(pages)
}

fn extract_pdf_inner(
    bytes: Vec<u8>,
    pages_spec: Option<&str>,
    images_only: bool,
) -> Result<PdfExtraction, String> {
    let detection = DetectionConfig {
        strategy: ScanStrategy::Full,
        ..DetectionConfig::default()
    };
    let initial = pdf_inspector::process_pdf_mem_with_options(
        &bytes,
        PdfOptions::detect_only().detection(detection),
    )
    .map_err(|error| format!("Failed to inspect PDF: {error}"))?;
    let page_count = initial.page_count as usize;
    if page_count == 0 {
        return Err("PDF has no pages".to_string());
    }
    if images_only && pages_spec.is_none() && page_count > PDF_AUTO_IMAGE_PAGE_LIMIT {
        return Err(format!(
            "PDF has {page_count} pages, which exceeds the {PDF_AUTO_IMAGE_PAGE_LIMIT}-page automatic image-extraction limit. Use `pages` to select up to {PDF_MAX_PAGES_PER_READ} pages."
        ));
    }
    let selected_pages = match pages_spec {
        Some(spec) => parse_page_range(spec, page_count)?,
        None => (0..page_count).collect(),
    };

    let mut visual_pages = if images_only {
        selected_pages.clone()
    } else {
        initial
            .pages_needing_ocr
            .iter()
            .filter_map(|page| usize::try_from(*page).ok()?.checked_sub(1))
            .filter(|page| selected_pages.binary_search(page).is_ok())
            .collect::<Vec<_>>()
    };
    let markdown = if images_only {
        String::new()
    } else {
        let selected = selected_pages
            .iter()
            .map(|page| *page as u32)
            .collect::<Vec<_>>();
        let extracted = pdf_inspector::extract_pages_markdown_mem(&bytes, Some(&selected))
            .map_err(|error| format!("Failed to extract PDF text: {error}"))?;
        visual_pages.extend(
            extracted
                .pages_needing_ocr
                .iter()
                .filter_map(|page| usize::try_from(*page).ok()?.checked_sub(1)),
        );
        extracted
            .pages
            .into_iter()
            .filter_map(|page| {
                let markdown = page.markdown.trim();
                (!markdown.is_empty()).then(|| markdown.to_owned())
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    visual_pages.sort_unstable();
    visual_pages.dedup();
    if pages_spec.is_none() && visual_pages.len() > PDF_AUTO_IMAGE_PAGE_LIMIT {
        return Err(format!(
            "PDF has {} pages requiring visual analysis, which exceeds the {PDF_AUTO_IMAGE_PAGE_LIMIT}-page automatic image-extraction limit. Use `pages` to select up to {PDF_MAX_PAGES_PER_READ} pages.",
            visual_pages.len()
        ));
    }

    let images = extract_page_images(bytes, &visual_pages)?;
    Ok(PdfExtraction {
        markdown,
        page_count,
        visual_pages,
        images,
    })
}

fn extract_page_images(
    bytes: Vec<u8>,
    page_indices: &[usize],
) -> Result<Vec<ExtractedImage>, String> {
    let file = FileOptions::cached()
        .load(bytes)
        .map_err(|error| format!("Failed to open PDF images: {error}"))?;
    let resolver = file.resolver();
    let mut output = Vec::new();

    'pages: for &page_index in page_indices {
        let page = file
            .get_page(page_index as u32)
            .map_err(|error| format!("Failed to read PDF page {}: {error}", page_index + 1))?;
        let Ok(resources) = page.resources() else {
            tracing::warn!(
                page = page_index + 1,
                "PDF page requiring visual analysis has no image resources"
            );
            continue;
        };
        let mut candidates = Vec::new();
        collect_images(resources, &resolver, 0, &mut candidates);
        candidates.sort_by_key(|(area, _, _)| std::cmp::Reverse(*area));

        for (_, bytes, mime_type) in candidates.into_iter().take(MAX_IMAGES_PER_PAGE) {
            match compress_image_for_conversation(bytes, mime_type) {
                Ok((bytes, mime_type)) => {
                    output.push(ExtractedImage {
                        data: base64::engine::general_purpose::STANDARD.encode(bytes),
                        mime_type,
                    });
                    if output.len() == MAX_EXTRACTED_IMAGES {
                        tracing::warn!(
                            limit = MAX_EXTRACTED_IMAGES,
                            "PDF image extraction reached the per-read image limit"
                        );
                        break 'pages;
                    }
                }
                Err(error) => tracing::warn!(
                    page = page_index + 1,
                    %error,
                    "skipping PDF image that cannot be prepared for vision"
                ),
            }
        }
    }
    Ok(output)
}

fn collect_images(
    resources: &Resources,
    resolver: &impl pdf::object::Resolve,
    depth: usize,
    output: &mut Vec<(u64, Vec<u8>, String)>,
) {
    if depth >= MAX_FORM_DEPTH {
        return;
    }
    for reference in resources.xobjects.values() {
        let Ok(object) = resolver.get(*reference) else {
            continue;
        };
        match &*object {
            XObject::Image(image) => {
                let area = u64::from(image.width) * u64::from(image.height);
                if !image.image_mask && area >= MIN_IMAGE_PIXELS {
                    match encode_image(image, resolver) {
                        Ok((bytes, mime)) => output.push((area, bytes, mime)),
                        Err(error) => tracing::warn!(%error, "skipping unsupported PDF image"),
                    }
                }
            }
            XObject::Form(form) => {
                if let Some(resources) = &form.dict().resources {
                    collect_images(resources, resolver, depth + 1, output);
                }
            }
            XObject::Postscript(_) => {}
        }
    }
}

fn encode_image(
    image: &ImageXObject,
    resolver: &impl pdf::object::Resolve,
) -> Result<(Vec<u8>, String), String> {
    let (raw, filter) = image
        .raw_image_data(resolver)
        .map_err(|error| error.to_string())?;
    if matches!(filter, Some(StreamFilter::DCTDecode(_))) {
        return Ok((raw.to_vec(), "image/jpeg".to_string()));
    }
    let samples = image
        .image_data(resolver)
        .map_err(|error| error.to_string())?;
    if image.bits_per_component.unwrap_or(8) != 8 {
        return Err(format!(
            "unsupported {}-bit PDF image",
            image.bits_per_component.unwrap_or(1)
        ));
    }

    let dynamic = match image.color_space.as_ref() {
        Some(ColorSpace::DeviceRGB) | Some(ColorSpace::CalRGB(_)) => DynamicImage::ImageRgb8(
            RgbImage::from_raw(image.width, image.height, samples.to_vec())
                .ok_or_else(|| "invalid RGB PDF image buffer".to_string())?,
        ),
        Some(ColorSpace::DeviceGray) | Some(ColorSpace::CalGray(_)) | None => {
            DynamicImage::ImageLuma8(
                GrayImage::from_raw(image.width, image.height, samples.to_vec())
                    .ok_or_else(|| "invalid grayscale PDF image buffer".to_string())?,
            )
        }
        Some(ColorSpace::DeviceCMYK) | Some(ColorSpace::CalCMYK(_)) => {
            let rgb = cmyk_to_rgb(&samples);
            DynamicImage::ImageRgb8(
                RgbImage::from_raw(image.width, image.height, rgb)
                    .ok_or_else(|| "invalid CMYK PDF image buffer".to_string())?,
            )
        }
        Some(ColorSpace::Indexed(base, _, lookup)) => {
            indexed_to_image(image.width, image.height, &samples, base, lookup)?
        }
        Some(other) => return Err(format!("unsupported PDF image color space: {other:?}")),
    };
    let mut encoded = Cursor::new(Vec::new());
    dynamic
        .write_to(&mut encoded, ImageFormat::Png)
        .map_err(|error| format!("failed to encode PDF image: {error}"))?;
    Ok((encoded.into_inner(), "image/png".to_string()))
}

fn indexed_to_image(
    width: u32,
    height: u32,
    samples: &[u8],
    base: &ColorSpace,
    lookup: &[u8],
) -> Result<DynamicImage, String> {
    let components = match base {
        ColorSpace::DeviceGray | ColorSpace::CalGray(_) => 1,
        ColorSpace::DeviceRGB | ColorSpace::CalRGB(_) => 3,
        _ => return Err(format!("unsupported indexed PDF color space: {base:?}")),
    };
    let mut pixels = Vec::with_capacity(samples.len() * components);
    for &index in samples {
        let start = usize::from(index) * components;
        let color = lookup
            .get(start..start + components)
            .ok_or_else(|| "invalid indexed PDF image lookup table".to_string())?;
        pixels.extend_from_slice(color);
    }
    if components == 1 {
        Ok(DynamicImage::ImageLuma8(
            GrayImage::from_raw(width, height, pixels)
                .ok_or_else(|| "invalid indexed grayscale buffer".to_string())?,
        ))
    } else {
        Ok(DynamicImage::ImageRgb8(
            RgbImage::from_raw(width, height, pixels)
                .ok_or_else(|| "invalid indexed RGB buffer".to_string())?,
        ))
    }
}

fn cmyk_to_rgb(samples: &[u8]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(samples.len() / 4 * 3);
    for pixel in samples.chunks_exact(4) {
        let c = u16::from(pixel[0]);
        let m = u16::from(pixel[1]);
        let y = u16::from(pixel[2]);
        let k = u16::from(pixel[3]);
        rgb.push((255 - (c + k).min(255)) as u8);
        rgb.push((255 - (m + k).min(255)) as u8);
        rgb.push((255 - (y + k).min(255)) as u8);
    }
    rgb
}

/// Minimal multi-page text PDF fixture for integration tests.
pub fn make_test_pdf(page_texts: &[&str]) -> Vec<u8> {
    let mut pdf = Vec::new();
    pdf.extend_from_slice(b"%PDF-1.4\n");
    let mut offsets = Vec::new();
    offsets.push(pdf.len());
    pdf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    let kids = (0..page_texts.len())
        .map(|index| format!("{} 0 R", 3 + index * 3))
        .collect::<Vec<_>>();
    offsets.push(pdf.len());
    pdf.extend_from_slice(
        format!(
            "2 0 obj\n<< /Type /Pages /Kids [{}] /Count {} >>\nendobj\n",
            kids.join(" "),
            page_texts.len()
        )
        .as_bytes(),
    );
    for (index, text) in page_texts.iter().enumerate() {
        let page_object = 3 + index * 3;
        let content_object = 4 + index * 3;
        let font_object = 5 + index * 3;
        let stream = format!("BT /F1 12 Tf 72 720 Td ({text}) Tj ET");
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{page_object} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {content_object} 0 R /Resources << /Font << /F1 {font_object} 0 R >> >> >>\nendobj\n").as_bytes());
        offsets.push(pdf.len());
        pdf.extend_from_slice(
            format!(
                "{content_object} 0 obj\n<< /Length {} >>\nstream\n{stream}\nendstream\nendobj\n",
                stream.len()
            )
            .as_bytes(),
        );
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{font_object} 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n").as_bytes());
    }
    let xref_offset = pdf.len();
    pdf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes(),
    );
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
            page_texts.len() * 3 + 3
        )
        .as_bytes(),
    );
    pdf
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_scanned_pdf() -> Vec<u8> {
        let image =
            DynamicImage::ImageRgb8(RgbImage::from_pixel(256, 256, image::Rgb([240, 240, 240])));
        let mut jpeg = Cursor::new(Vec::new());
        image.write_to(&mut jpeg, ImageFormat::Jpeg).unwrap();
        let jpeg = jpeg.into_inner();

        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        let mut object = |number: usize, body: &[u8]| {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{number} 0 obj\n").as_bytes());
            pdf.extend_from_slice(body);
            pdf.extend_from_slice(b"\nendobj\n");
        };
        object(1, b"<< /Type /Catalog /Pages 2 0 R >>");
        object(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
        object(3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 256 256] /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>");
        object(
            4,
            b"<< /Length 31 >>\nstream\nq 256 0 0 256 0 0 cm /Im0 Do Q\nendstream",
        );
        let mut image_object = format!("<< /Type /XObject /Subtype /Image /Width 256 /Height 256 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n", jpeg.len()).into_bytes();
        image_object.extend_from_slice(&jpeg);
        image_object.extend_from_slice(b"\nendstream");
        object(5, &image_object);
        let xref = pdf.len();
        pdf.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
        );
        pdf
    }

    fn make_mixed_pdf() -> Vec<u8> {
        let image =
            DynamicImage::ImageRgb8(RgbImage::from_pixel(256, 256, image::Rgb([220, 220, 220])));
        let mut jpeg = Cursor::new(Vec::new());
        image.write_to(&mut jpeg, ImageFormat::Jpeg).unwrap();
        let jpeg = jpeg.into_inner();

        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        let mut object = |number: usize, body: &[u8]| {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{number} 0 obj\n").as_bytes());
            pdf.extend_from_slice(body);
            pdf.extend_from_slice(b"\nendobj\n");
        };
        object(1, b"<< /Type /Catalog /Pages 2 0 R >>");
        object(2, b"<< /Type /Pages /Kids [3 0 R 6 0 R] /Count 2 >>");
        object(3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 256 256] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>");
        let text_stream =
            b"BT /F1 12 Tf 20 220 Td (native text) Tj 0 -20 Td (more text) Tj 0 -20 Td (final text) Tj ET";
        let mut text_object = format!("<< /Length {} >>\nstream\n", text_stream.len()).into_bytes();
        text_object.extend_from_slice(text_stream);
        text_object.extend_from_slice(b"\nendstream");
        object(4, &text_object);
        object(5, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
        object(6, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 256 256] /Resources << /XObject << /Im0 8 0 R >> >> /Contents 7 0 R >>");
        object(
            7,
            b"<< /Length 31 >>\nstream\nq 256 0 0 256 0 0 cm /Im0 Do Q\nendstream",
        );
        let mut image_object = format!("<< /Type /XObject /Subtype /Image /Width 256 /Height 256 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n", jpeg.len()).into_bytes();
        image_object.extend_from_slice(&jpeg);
        image_object.extend_from_slice(b"\nendstream");
        object(8, &image_object);
        let xref = pdf.len();
        pdf.extend_from_slice(b"xref\n0 9\n0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!("trailer\n<< /Size 9 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
        );
        pdf
    }

    #[test]
    fn parses_page_ranges() {
        assert_eq!(parse_page_range("1,3-4,3", 5).unwrap(), vec![0, 2, 3]);
        assert_eq!(parse_page_range("3-", 5).unwrap(), vec![2, 3, 4]);
        assert!(parse_page_range("0", 5).is_err());
    }

    #[test]
    fn text_pdf_uses_markdown_without_images() {
        let pdf = make_test_pdf(&["hello world", "second page"]);
        let result = extract_pdf_inner(pdf, None, false).unwrap();
        assert_eq!(result.page_count, 2);
        assert!(result.markdown.contains("hello world"));
        assert!(result.images.is_empty());
    }

    #[test]
    fn scanned_pdf_extracts_raster_for_vision() {
        let result = extract_pdf_inner(make_scanned_pdf(), None, false).unwrap();
        assert_eq!(result.visual_pages, vec![0]);
        assert_eq!(result.images.len(), 1);
        assert!(result.images[0].mime_type.starts_with("image/"));
        assert!(!result.images[0].data.is_empty());
    }

    #[test]
    fn mixed_pdf_keeps_text_and_extracts_visual_page() {
        let result = extract_pdf_inner(make_mixed_pdf(), None, false).unwrap();
        assert_eq!(result.page_count, 2);
        assert!(
            result.markdown.contains("native text"),
            "markdown was {:?}",
            result.markdown
        );
        assert_eq!(result.visual_pages, vec![1]);
        assert_eq!(result.images.len(), 1);
    }

    #[test]
    fn page_selection_limits_both_text_and_visual_extraction() {
        let text = extract_pdf_inner(make_mixed_pdf(), Some("1"), false).unwrap();
        assert!(text.markdown.contains("native text"));
        assert!(text.visual_pages.is_empty());
        assert!(text.images.is_empty());

        let image = extract_pdf_inner(make_mixed_pdf(), Some("2"), true).unwrap();
        assert!(image.markdown.is_empty());
        assert_eq!(image.visual_pages, vec![1]);
        assert_eq!(image.images.len(), 1);
    }
}
