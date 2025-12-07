use lol_html::{element, HtmlRewriter, Settings};
use url::Url;

pub struct ParsedHtml {
    pub links: Vec<String>
}

pub fn parse_html(input: String, base_url: &str) -> ParsedHtml {

    let base_url = Url::parse(base_url).expect("Failed to parse base URL");
    let mut links = Vec::new();
    

    let mut rewriter = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![
                element!("a[href]", |el| {
                    if let Some(attached_url) = el.get_attribute("href") {
                        if let Ok(attached_url) = Url::parse(&attached_url) {
                            if attached_url.scheme() == "http" || attached_url.scheme() == "https" {
                                links.push(attached_url.to_string());
                                return Ok(());
                            }
                        }
                        if let Ok(joined_url) = base_url.join(&attached_url) {
                            links.push(joined_url.to_string());
                        }
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

    return ParsedHtml { 
        links: links.clone()
    };

}