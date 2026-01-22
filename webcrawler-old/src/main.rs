mod parser;
mod url_store;
mod writer;
mod http_client;
mod rate_limiter;
mod ui;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::fs;
use std::time::Duration;
use futures::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use url_store::UrlStore;
use writer::BufferedWriter;
use http_client::HttpClient;
use rate_limiter::RateLimiter;
use tokio::sync::mpsc;

const MAX_PAGES: usize = 1_000_000;
const CONCURRENCY: usize = 1_000;
const CHANNEL_BUFFER: usize = 10_000;

#[tokio::main]
async fn main() {
    // Create output directory if it doesn't exist
    fs::create_dir_all("output").expect("Failed to create output directory");
    
    let seeds = vec![
        "https://en.wikipedia.org/wiki/Full-text_search".to_string()
    ];
    
    let (writer, writer_tx) = BufferedWriter::new("output/crawled_pages.jsonl")
        .expect("Failed to create buffered writer");
    tokio::spawn(writer.run());
    
    let http_client = Arc::new(HttpClient::new().expect("Failed to create HTTP client"));
    let rate_limiter = RateLimiter::new();
    let url_store = UrlStore::new("output/visited_urls.db")
        .expect("Failed to open URL store");
    
    // Load existing page count from database
    let existing_pages = url_store.get_pages_crawled();
    let pages_count = Arc::new(AtomicUsize::new(existing_pages));
    let pages_written = Arc::new(AtomicUsize::new(0));
    let queue_size = Arc::new(AtomicUsize::new(0));
    
    // Check existing frontier before adding seeds
    let existing_frontier = url_store.frontier_count();
    eprintln!("Found {} URLs in frontier from previous run", existing_frontier);
    eprintln!("Already crawled {} pages", existing_pages);
    
    // Add seeds to frontier (only if not already visited)
    let mut seeds_added = 0;
    for seed in seeds {
        if url_store.add_to_frontier(&seed) {
            seeds_added += 1;
        }
    }
    eprintln!("Added {} seed URLs to frontier", seeds_added);
    
    // Check frontier size
    let initial_frontier = url_store.frontier_count();
    eprintln!("Total URLs in frontier: {}", initial_frontier);
    if initial_frontier == 0 {
        eprintln!("No URLs in frontier. All URLs have been crawled.");
        return;
    }
    
    let stats = Arc::new(ui::CrawlerStats::new(
        pages_count.clone(),
        pages_written.clone(),
        queue_size.clone(),
    ));
    
    let ui_task = tokio::spawn({
        let stats = stats.clone();
        async move {
            if let Err(e) = ui::run_ui(stats, MAX_PAGES).await {
                eprintln!("UI error: {}", e);
            }
        }
    });
    
    let (discovered_tx, mut discovered_rx) = mpsc::channel::<String>(CHANNEL_BUFFER);
    let (processing_tx, processing_rx) = mpsc::channel::<String>(CHANNEL_BUFFER);
    
    let discovered_tx = Arc::new(discovered_tx);
    let processing_tx = Arc::new(processing_tx);
    
    // Task to add discovered URLs to frontier (workers will pull as needed)
    let frontier_task = tokio::spawn({
        let url_store = url_store.clone();
        async move {
            while let Some(link) = discovered_rx.recv().await {
                // Just persist to DB, don't send to processing channel
                url_store.add_to_frontier(&link);
            }
        }
    });
    
    // Seed the processing queue with URLs from frontier
    let url_store_clone = url_store.clone();
    let processing_tx_clone = processing_tx.clone();
    let queue_size_clone = queue_size.clone();
    let stats_clone = stats.clone();
    tokio::spawn(async move {
        loop {
            // Keep queue fed with URLs from frontier
            let current_queue = queue_size_clone.load(Ordering::Relaxed);
            if current_queue < CHANNEL_BUFFER / 2 && !stats_clone.should_stop() {
                if let Some(url) = url_store_clone.pop_from_frontier() {
                    match processing_tx_clone.try_send(url) {
                        Ok(_) => {
                            queue_size_clone.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(_) => {
                            // Channel full, wait a bit
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                    }
                } else {
                    // Frontier empty, wait for discovered URLs
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            } else {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    });
    
    ReceiverStream::new(processing_rx)
        .for_each_concurrent(CONCURRENCY, |url| {
            let url_store = url_store.clone();
            let pages_count = pages_count.clone();
            let pages_written = pages_written.clone();
            let queue_size = queue_size.clone();
            let discovered_tx = discovered_tx.clone();
            let writer_tx = writer_tx.clone();
            let http_client = http_client.clone();
            let rate_limiter = rate_limiter.clone();
            let stats = stats.clone();
            
            async move {
                queue_size.fetch_sub(1, Ordering::Relaxed);
                stats.active_workers.fetch_add(1, Ordering::Relaxed);
                
                let current_count = pages_count.fetch_add(1, Ordering::Relaxed) + 1;
                if current_count > MAX_PAGES || stats.should_stop() {
                    stats.active_workers.fetch_sub(1, Ordering::Relaxed);
                    return;
                }
                
                match process_link(url.clone(), http_client, rate_limiter, writer_tx).await {
                    Ok((parsed, child_links)) => {
                        pages_written.fetch_add(1, Ordering::Relaxed);
                        
                        // Track domain
                        stats.increment_domain(&url);
                        
                        // Persist page count every 10 pages
                        let current = pages_count.load(Ordering::Relaxed);
                        if current.is_multiple_of(10) {
                            url_store.set_pages_crawled(current);
                        }
                        
                        if let Some(canonical) = &parsed.canonical_url
                            && canonical != &parsed.url {
                            url_store.mark_visited(canonical);
                        }
                        
                        for link in child_links {
                            let _ = discovered_tx.send(link).await;
                        }
                    }
                    Err(e) => {
                        stats.add_error(format!("{}: {}", url, e));
                    }
                }
                
                stats.active_workers.fetch_sub(1, Ordering::Relaxed);
            }
        })
        .await;
    
    // Save final page count
    url_store.set_pages_crawled(pages_count.load(Ordering::Relaxed));
    
    frontier_task.await.unwrap();
    ui_task.await.unwrap();
}

async fn process_link(
    link: String,
    http_client: Arc<HttpClient>,
    rate_limiter: RateLimiter,
    writer_tx: mpsc::Sender<parser::ParsedHtml>,
) -> Result<(parser::ParsedHtml, Vec<String>), Box<dyn std::error::Error>> {
    rate_limiter.wait_if_needed(&link).await;
    let html_content = http_client.fetch(&link).await?;
    let parsed = parser::parse_html(html_content, &link);
    let links: Vec<String> = parsed.links.clone();
    
    writer_tx.send(parsed.clone()).await?;
    
    Ok((parsed, links))
}
