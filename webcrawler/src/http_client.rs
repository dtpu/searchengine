use reqwest::Client;
use std::time::Duration;

const MAX_RESPONSE_SIZE: usize = 10 * 1024 * 1024;

pub struct HttpClient {
    client: Client,
}

impl HttpClient {
    pub fn new() -> Result<Self, reqwest::Error> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(30))
            .user_agent("Mozilla/5.0 (compatible; WebCrawler/1.0)")
            .pool_max_idle_per_host(10)
            .build()?;
        
        Ok(Self { client })
    }
    
    pub async fn fetch(&self, url: &str) -> Result<String, FetchError> {
        let response = self.client.get(url).send().await?;
        
        let status = response.status();
        if !status.is_success() {
            return Err(FetchError::HttpError(status.as_u16()));
        }
        
        if let Some(content_type) = response.headers().get("content-type") {
            let content_type_str = content_type.to_str().unwrap_or("");
            if !content_type_str.contains("text/html") {
                return Err(FetchError::InvalidContentType(content_type_str.to_string()));
            }
        }
        
        if let Some(content_length) = response.content_length() {
            if content_length > MAX_RESPONSE_SIZE as u64 {
                return Err(FetchError::TooLarge(content_length));
            }
        }
        
        let body = response.text().await?;
        if body.len() > MAX_RESPONSE_SIZE {
            return Err(FetchError::TooLarge(body.len() as u64));
        }
        
        Ok(body)
    }
}

#[derive(Debug)]
pub enum FetchError {
    HttpError(u16),
    InvalidContentType(String),
    TooLarge(u64),
    RequestError(reqwest::Error),
}

impl From<reqwest::Error> for FetchError {
    fn from(err: reqwest::Error) -> Self {
        FetchError::RequestError(err)
    }
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::HttpError(code) => write!(f, "HTTP error: {}", code),
            FetchError::InvalidContentType(ct) => write!(f, "Invalid content type: {}", ct),
            FetchError::TooLarge(size) => write!(f, "Response too large: {} bytes", size),
            FetchError::RequestError(e) => write!(f, "Request error: {}", e),
        }
    }
}

impl std::error::Error for FetchError {}
