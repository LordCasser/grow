//! Magic-byte metadata inspection shared by read tools.

/// Metadata extracted from file bytes via magic-byte inspection.
#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub mime_type: String,
}

impl FileMetadata {
    pub fn is_image(&self) -> bool {
        self.mime_type.starts_with("image/")
    }
}

/// Infer file metadata (MIME type, extension) from raw bytes using magic-byte inspection.
pub fn bytes_to_metadata(file_bytes: &[u8]) -> Result<FileMetadata, tool_runtime::ToolError> {
    let data = infer::get(file_bytes).ok_or_else(|| {
        tool_runtime::ToolError::invalid_arguments("failed to infer file type from magic bytes")
    })?;

    Ok(FileMetadata {
        mime_type: data.mime_type().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_metadata_identifies_images() {
        let meta = FileMetadata {
            mime_type: "image/png".to_string(),
        };
        assert!(meta.is_image());
    }
}
