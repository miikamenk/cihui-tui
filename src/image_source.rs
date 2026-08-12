//! Recognising image references in arbitrary pasted text.
//!
//! Clipboard contents arrive in many shapes: a bare filesystem path, a URL, an
//! HTML `<img>` fragment copied from a browser, or a Markdown image. These
//! helpers classify such text so the caller knows whether to read a file or
//! download a URL.

/// An image reference recognised in pasted text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageSource {
    /// A local filesystem path (possibly `file://`-prefixed).
    Path(String),
    /// A remote URL.
    Url(String),
    /// Extracted from markup, where the target may be either.
    PathOrUrl(String),
}

/// Image file extensions recognised everywhere in this module.
const IMAGE_EXTENSIONS: [&str; 7] = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "tiff"];

/// Classify `text` as an image reference, if it looks like one.
///
/// Markup forms are checked first, since an `<img>` tag or a Markdown image
/// contains a path or URL that would otherwise be missed.
pub fn extract_image_source(text: &str) -> Option<ImageSource> {
    let trimmed = text.trim();

    if let Some(src) = extract_html_img_src(trimmed) {
        return Some(ImageSource::PathOrUrl(src));
    }

    if let Some(src) = extract_markdown_image(trimmed) {
        return Some(ImageSource::PathOrUrl(src));
    }

    if trimmed.starts_with("file://") && is_image_path(trimmed) {
        return Some(ImageSource::Path(trimmed.to_string()));
    }

    if is_image_url(trimmed) {
        return Some(ImageSource::Url(trimmed.to_string()));
    }

    if is_image_path(trimmed) {
        return Some(ImageSource::Path(trimmed.to_string()));
    }

    None
}

/// Pull the `src` attribute out of the first `<img>` tag in `text`.
///
/// Handles both quote styles. Not a real HTML parser: it scans for the first
/// `<img`, then the first `src="` or `src='` after it.
pub fn extract_html_img_src(text: &str) -> Option<String> {
    let text_lower = text.to_lowercase();
    if let Some(img_idx) = text_lower.find("<img") {
        let after_img = &text[img_idx..];
        if let Some(src_start) = after_img.to_lowercase().find("src=\"") {
            let after_src = &after_img[src_start + 5..];
            if let Some(src_end) = after_src.find('"') {
                return Some(after_src[..src_end].to_string());
            }
        }
        if let Some(src_start) = after_img.to_lowercase().find("src='") {
            let after_src = &after_img[src_start + 5..];
            if let Some(src_end) = after_src.find('\'') {
                return Some(after_src[..src_end].to_string());
            }
        }
    }
    None
}

/// Pull the target out of a Markdown image, `![alt](target)`.
pub fn extract_markdown_image(text: &str) -> Option<String> {
    if text.starts_with("![") {
        if let Some(start) = text.find("](") {
            let after_paren = &text[start + 2..];
            if let Some(end) = after_paren.find(')') {
                return Some(after_paren[..end].to_string());
            }
        }
    }
    None
}

/// Whether `text` is a URL pointing at an image file.
pub fn is_image_url(text: &str) -> bool {
    (text.starts_with("http://") || text.starts_with("https://") || text.starts_with("file://"))
        && text.chars().any(|c| c == '.')
        && has_image_extension(text)
}

/// Whether `text` looks like a filesystem path to an image file.
pub fn is_image_path(text: &str) -> bool {
    let trimmed = text.trim();

    if !trimmed.chars().any(|c| c == '/' || c == '\\' || c == '.') {
        return false;
    }

    if trimmed.starts_with("file://") {
        return true;
    }

    has_image_extension(trimmed)
}

/// Whether the text after the final `.` is a known image extension.
fn has_image_extension(text: &str) -> bool {
    text.split('.')
        .next_back()
        .is_some_and(|ext| IMAGE_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
}
