use lol_html::{element, text, HtmlRewriter, Settings};
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MetaTag {
    pub name: String,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ParsedHtml {
    pub url: String,
    pub language: Option<String>,
    pub title: Option<String>,
    pub meta_tags: Vec<MetaTag>,
    pub canonical_url: Option<String>,
    pub content_text: String,
    pub links: Vec<String>,
}

/// Check if URL points to a media file that shouldn't be crawled
fn is_media_file(url: &str) -> bool {
    let media_extensions = [
        // Images
        ".jpg", ".jpeg", ".png", ".gif", ".bmp", ".svg", ".webp", ".ico", ".tiff",
        // Videos
        ".mp4", ".avi", ".mov", ".wmv", ".flv", ".webm", ".mkv", ".m4v",
        // Audio
        ".mp3", ".wav", ".ogg", ".m4a", ".flac", ".aac",
        // Documents
        ".pdf", ".doc", ".docx", ".xls", ".xlsx", ".ppt", ".pptx", ".xml",
        // Archives
        ".zip", ".rar", ".tar", ".gz", ".7z",
        // Executables
        ".exe", ".dmg", ".pkg", ".deb", ".rpm",
    ];
    
    let url_lower = url.to_lowercase();
    // Check path component for extension (ignore query params)
    let path = url_lower.split('?').next().unwrap_or(&url_lower);
    media_extensions.iter().any(|ext| path.ends_with(ext))
}

fn url_validation(url: Url) -> bool {
    let scheme = url.scheme();
    let host = url.host_str();
    let domain = url.domain();
    if is_media_file(url.as_str()) {
        return false;
    }
    if let Some(d) = domain {
        if !d.ends_with("wikipedia.org") {
            return false;
        }
    }
    return (scheme == "http" || scheme == "https") && host.is_some();
}

pub fn parse_html(input: String, base_url: &str) -> ParsedHtml {
    let base_url_parsed = Url::parse(base_url).expect("Failed to parse base URL");

    let mut links = Vec::new();
    let mut meta_tags = Vec::new();
    let mut title = None;
    let mut language = None;
    let mut canonical_url = None;
    let mut content_text = String::new();

    let mut rewriter = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![
                // Extract language from html tag
                element!("html[lang]", |el| {
                    if let Some(lang) = el.get_attribute("lang") {
                        language = Some(lang);
                    }
                    Ok(())
                }),
                // Extract title
                text!("title", |t| {
                    if title.is_none() {
                        title = Some(t.as_str().to_string());
                    } else {
                        title = Some(format!("{}{}", title.as_ref().unwrap(), t.as_str()));
                    }
                    Ok(())
                }),
                // Extract meta tags
                element!("meta[name][content]", |el| {
                    if let (Some(name), Some(content)) = (el.get_attribute("name"), el.get_attribute("content")) {
                        meta_tags.push(MetaTag { name, content });
                    }
                    Ok(())
                }),
                // Extract canonical URL
                element!("link[rel=canonical]", |el| {
                    if let Some(href) = el.get_attribute("href") {
                        canonical_url = Some(href);
                    }
                    Ok(())
                }),
                // Extract links
                element!("a[href]", |el| {
                    if let Some(attached_url) = el.get_attribute("href") {
                        if let Ok(parsed_url) = Url::parse(&attached_url) {
                            if url_validation(parsed_url.clone()) {
                                links.push(parsed_url.to_string());
                            }
                            return Ok(());
                        }
                        if let Ok(joined_url) = base_url_parsed.join(&attached_url) {
                            if url_validation(joined_url.clone()) {
                                links.push(joined_url.to_string());
                            }
                        }
                    }
                    Ok(())
                }),
                // Extract text content from body
                text!("body *", |t| {
                    if !t.as_str().trim().is_empty() {
                        if !content_text.is_empty() {
                            content_text.push(' ');
                        }
                        content_text.push_str(t.as_str().trim());
                    }
                    Ok(())
                }),
            ],
            ..Settings::new()
        },
        |_: &[u8]| {}
    );

    rewriter.write(input.as_bytes()).expect("Failed to parse HTML input");
    rewriter.end().expect("Failed to complete HTML parsing");

    ParsedHtml {
        url: base_url.to_string(),
        language,
        title,
        meta_tags,
        canonical_url,
        content_text,
        links,
    }
}
