use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use std::time::{Duration, Instant};
use url::Url;

const MIN_DELAY_SECS: u64 = 1;

pub struct RateLimiter {
    last_request: Arc<Mutex<HashMap<String, Instant>>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            last_request: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    
    pub async fn wait_if_needed(&self, url: &str) {
        let domain = match Self::extract_domain(url) {
            Some(d) => d,
            None => return,
        };
        
        let mut map = self.last_request.lock().await;
        
        if let Some(last_time) = map.get(&domain) {
            let elapsed = last_time.elapsed();
            let min_delay = Duration::from_secs(MIN_DELAY_SECS);
            
            if elapsed < min_delay {
                let sleep_duration = min_delay - elapsed;
                drop(map); // Release lock before sleeping
                tokio::time::sleep(sleep_duration).await;
                
                // Re-acquire lock to update
                let mut map = self.last_request.lock().await;
                map.insert(domain.clone(), Instant::now());
            } else {
                map.insert(domain.clone(), Instant::now());
            }
        } else {
            map.insert(domain, Instant::now());
        }
    }
    
    fn extract_domain(url: &str) -> Option<String> {
        Url::parse(url).ok().and_then(|u| u.host_str().map(|s| s.to_string()))
    }
}

impl Clone for RateLimiter {
    fn clone(&self) -> Self {
        Self {
            last_request: self.last_request.clone(),
        }
    }
}
