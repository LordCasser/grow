//! Document detection and bounded conversion through anydoc.

use std::path::Path;
use std::time::Duration;

pub const MAX_DOCUMENT_BYTES: usize = 50 * 1024 * 1024;
pub const DOCUMENT_PROCESS_TIMEOUT: Duration = Duration::from_secs(60);

pub fn detect_format(bytes: &[u8], path: &Path) -> Option<anydoc::Format> {
    anydoc::Format::from_bytes(bytes).or_else(|| anydoc::Format::from_path(path))
}

pub fn format_label(format: anydoc::Format) -> &'static str {
    match format {
        anydoc::Format::Doc | anydoc::Format::Docx => "Word document",
        anydoc::Format::Odt => "OpenDocument text",
        anydoc::Format::Pdf => "PDF",
        anydoc::Format::Ppt | anydoc::Format::Pptx => "PowerPoint document",
        anydoc::Format::Rtf => "RTF document",
        anydoc::Format::Epub => "EPUB document",
        anydoc::Format::Excel => "Excel workbook",
        anydoc::Format::Ods => "OpenDocument spreadsheet",
        anydoc::Format::Odp => "OpenDocument presentation",
        anydoc::Format::Csv => "CSV document",
    }
}

pub async fn run_bounded_document_task<T, F>(
    file_bytes: Vec<u8>,
    path: &Path,
    format_label: &str,
    task: F,
) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(Vec<u8>) -> Result<T, String> + Send + 'static,
{
    if file_bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(format!(
            "{format_label} file is {:.1} MB, exceeds the {:.0} MB limit.",
            file_bytes.len() as f64 / 1_048_576.0,
            MAX_DOCUMENT_BYTES as f64 / 1_048_576.0,
        ));
    }

    tracing::info!(
        size_bytes = file_bytes.len(),
        format_label,
        "processing document"
    );

    let result = tokio::time::timeout(
        DOCUMENT_PROCESS_TIMEOUT,
        tokio::task::spawn_blocking(move || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| task(file_bytes)))
        }),
    )
    .await;

    match result {
        Ok(Ok(Ok(result))) => result,
        Err(_) => Err(format!(
            "{format_label} processing timed out after {}s: {}",
            DOCUMENT_PROCESS_TIMEOUT.as_secs(),
            path.display()
        )),
        Ok(Ok(Err(_))) => Err(format!(
            "{format_label} processing failed (internal error): {}",
            path.display()
        )),
        Ok(Err(error)) => Err(format!("{format_label} processing failed: {error}")),
    }
}

pub async fn convert_to_markdown(
    file_bytes: Vec<u8>,
    path: &Path,
    format: anydoc::Format,
) -> Result<String, String> {
    let label = format_label(format);
    run_bounded_document_task(
        file_bytes,
        path,
        label,
        move |bytes| match anydoc::to_markdown_bytes(&bytes, format) {
            Ok(markdown) => Ok(markdown),
            Err(error) => Err(format!(
                "Failed to convert {label} with anydoc ({}): {error}",
                error.code()
            )),
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection_prefers_content_over_extension() {
        let path = Path::new("wrong.docx");
        assert_eq!(
            detect_format(br"{\rtf1\ansi hello}", path),
            Some(anydoc::Format::Rtf)
        );
    }

    #[test]
    fn csv_uses_extension_fallback() {
        assert_eq!(
            detect_format(b"name,value\na,1\n", Path::new("data.csv")),
            Some(anydoc::Format::Csv)
        );
    }

    #[tokio::test]
    async fn converts_rtf_to_markdown() {
        let result = convert_to_markdown(
            br"{\rtf1\ansi Hello \b world\b0}".to_vec(),
            Path::new("note.rtf"),
            anydoc::Format::Rtf,
        )
        .await
        .unwrap();
        assert!(result.contains("Hello"));
        assert!(result.contains("**world**"));
    }
}
