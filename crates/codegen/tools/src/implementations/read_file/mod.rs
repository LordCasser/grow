//! Shared document/image helpers for read tools (grow_build, etc.).

pub mod document;
pub mod image;
pub mod metadata;
pub mod pdf;

pub use metadata::{FileMetadata, bytes_to_metadata};
pub use pdf::{PDF_MAX_PAGES_PER_READ, parse_page_range};

pub use image::{CompressImageError, compress_image_for_conversation, image_read_output};
pub use pdf::make_test_pdf;
