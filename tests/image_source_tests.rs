//! Clipboard contents arrive in whatever shape the source application chose:
//! a bare path, a URL, an HTML fragment from a browser, or Markdown. Getting
//! the classification wrong means the app either downloads a local path or
//! tries to open a URL as a file.

use cihui_tui::image_source::{
    extract_html_img_src, extract_image_source, extract_markdown_image, is_image_path,
    is_image_url, ImageSource,
};
use pretty_assertions::assert_eq;

// ------------------------------------------------------------------ paths --

#[test]
fn recognises_absolute_and_relative_paths() {
    for path in [
        "/home/user/screenshot.png",
        "./local.jpg",
        "../up/one.jpeg",
        "images/nested/deep.webp",
        "C:\\Users\\me\\pic.bmp",
    ] {
        assert_eq!(
            extract_image_source(path),
            Some(ImageSource::Path(path.to_string())),
            "{path:?} should be recognised as a path"
        );
    }
}

#[test]
fn recognises_every_supported_extension() {
    for ext in ["png", "jpg", "jpeg", "gif", "webp", "bmp", "tiff"] {
        let path = format!("/tmp/image.{ext}");

        assert!(
            is_image_path(&path),
            "{ext} should be a supported extension"
        );
    }
}

#[test]
fn extension_matching_ignores_case() {
    for path in ["/tmp/IMAGE.PNG", "/tmp/photo.JpG", "/tmp/scan.TIFF"] {
        assert!(is_image_path(path), "{path:?} should be recognised");
    }
}

#[test]
fn rejects_non_image_files() {
    for path in [
        "/home/user/document.pdf",
        "./notes.txt",
        "archive.tar.gz",
        "/etc/hosts",
    ] {
        assert_eq!(extract_image_source(path), None, "{path:?} is not an image");
    }
}

#[test]
fn rejects_bare_words_with_no_path_or_extension() {
    // A word with no separator and no dot cannot be a path, and this is the
    // common case: the user simply typed some text.
    for text in ["hello", "你好世界", "png", ""] {
        assert_eq!(extract_image_source(text), None, "{text:?} is not a path");
    }
}

#[test]
fn a_dot_alone_is_not_enough() {
    assert!(!is_image_path("version.2"));
    assert!(!is_image_path("some.file"));
}

// ------------------------------------------------------------------- urls --

#[test]
fn recognises_http_and_https_image_urls() {
    for url in [
        "https://example.com/photo.png",
        "http://example.com/a/b/c.jpeg",
    ] {
        assert_eq!(
            extract_image_source(url),
            Some(ImageSource::Url(url.to_string())),
            "{url:?} should be recognised as a URL"
        );
    }
}

#[test]
fn file_urls_are_treated_as_paths_not_downloads() {
    let url = "file:///home/user/pic.png";

    assert_eq!(
        extract_image_source(url),
        Some(ImageSource::Path(url.to_string())),
        "file:// is local, so it must not go through the downloader"
    );
}

#[test]
fn any_file_url_counts_as_a_path() {
    // is_image_path short-circuits on the file:// prefix, before the
    // extension check.
    assert!(is_image_path("file:///home/user/no-extension"));
}

#[test]
fn rejects_non_image_urls() {
    for url in [
        "https://example.com/index.html",
        "https://example.com/",
        "https://example.com/download?file=archive.zip",
    ] {
        assert!(!is_image_url(url), "{url:?} is not an image URL");
    }
}

#[test]
fn a_query_string_hides_the_extension() {
    // Documents a real limitation: classification looks at the text after the
    // last dot, so a query string makes an image URL unrecognisable.
    let url = "https://example.com/photo.png?width=800";

    assert!(
        !is_image_url(url),
        "a query string currently defeats extension matching"
    );
    assert_eq!(extract_image_source(url), None);
}

// ------------------------------------------------------------------- html --

#[test]
fn extracts_src_from_double_quoted_img_tags() {
    let html = r#"<img src="https://example.com/a.png" alt="a">"#;

    assert_eq!(
        extract_html_img_src(html),
        Some("https://example.com/a.png".to_string())
    );
}

#[test]
fn extracts_src_from_single_quoted_img_tags() {
    let html = "<img src='https://example.com/b.png'>";

    assert_eq!(
        extract_html_img_src(html),
        Some("https://example.com/b.png".to_string())
    );
}

#[test]
fn img_tag_matching_ignores_case() {
    let html = r#"<IMG SRC="https://example.com/c.png">"#;

    assert_eq!(
        extract_html_img_src(html),
        Some("https://example.com/c.png".to_string())
    );
}

#[test]
fn finds_an_img_tag_inside_surrounding_markup() {
    let html = r#"<div class="x"><p>text</p><img src="/tmp/d.png"></div>"#;

    assert_eq!(extract_html_img_src(html), Some("/tmp/d.png".to_string()));
}

#[test]
fn html_takes_priority_over_a_bare_url() {
    // The tag is checked first, so its target wins over anything else in the
    // pasted text.
    let html = r#"<img src="/tmp/inner.png">"#;

    assert_eq!(
        extract_image_source(html),
        Some(ImageSource::PathOrUrl("/tmp/inner.png".to_string()))
    );
}

#[test]
fn an_img_tag_without_a_src_yields_nothing() {
    assert_eq!(extract_html_img_src("<img alt=\"no source\">"), None);
    assert_eq!(extract_html_img_src("<div>not an image</div>"), None);
}

#[test]
fn an_unterminated_src_attribute_yields_nothing() {
    assert_eq!(extract_html_img_src(r#"<img src="unterminated"#), None);
}

// --------------------------------------------------------------- markdown --

#[test]
fn extracts_the_target_of_a_markdown_image() {
    assert_eq!(
        extract_markdown_image("![alt text](https://example.com/e.png)"),
        Some("https://example.com/e.png".to_string())
    );
}

#[test]
fn handles_a_markdown_image_with_an_empty_alt() {
    assert_eq!(
        extract_markdown_image("![](/tmp/f.png)"),
        Some("/tmp/f.png".to_string())
    );
}

#[test]
fn markdown_must_start_at_the_beginning() {
    // The check is a starts_with, so an image later in a paragraph is missed.
    assert_eq!(extract_markdown_image("see this ![alt](/tmp/g.png)"), None);
}

#[test]
fn a_markdown_link_is_not_a_markdown_image() {
    assert_eq!(extract_markdown_image("[text](https://example.com)"), None);
}

#[test]
fn an_unterminated_markdown_image_yields_nothing() {
    assert_eq!(extract_markdown_image("![alt](/tmp/h.png"), None);
}

// ------------------------------------------------------------ whitespace --

#[test]
fn surrounding_whitespace_is_trimmed() {
    let source = extract_image_source("  \n /tmp/spaced.png \t ");

    assert_eq!(
        source,
        Some(ImageSource::Path("/tmp/spaced.png".to_string())),
        "the trimmed path is what gets opened"
    );
}

#[test]
fn markup_is_also_trimmed_before_matching() {
    let source = extract_image_source("\n  ![alt](/tmp/i.png)  \n");

    assert_eq!(
        source,
        Some(ImageSource::PathOrUrl("/tmp/i.png".to_string()))
    );
}
