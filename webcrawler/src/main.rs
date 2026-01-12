mod parser;

use reqwest;
use std::collections::HashSet;
use tokio;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Mutex;
use futures::StreamExt;
use tokio_stream::wrappers::UnboundedReceiverStream;

const MAX_PAGES: usize = 1000;
const CONCURRENCY: usize = 50;

#[tokio::main]
async fn main() {
    let seeds = vec![
        "https://student.cs.uwaterloo.ca/~cs145/".to_string()
    ];
    
    let visited = Arc::new(Mutex::new(HashSet::new()));
    let pages_count = Arc::new(AtomicUsize::new(0));
    
    // Two channels: one for discovered URLs, one for processing
    let (discovered_tx, mut discovered_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let (processing_tx, processing_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    
    let discovered_tx = Arc::new(discovered_tx);
    let processing_tx = Arc::new(processing_tx);
    
    // Task to batch and forward URLs from discovered -> processing
    let batch_task = tokio::spawn({
        let processing_tx = processing_tx.clone();
        async move {
            while let Some(link) = discovered_rx.recv().await {
                // You can add batching logic here if needed
                let _ = processing_tx.send(link);
            }
        }
    });
    
    // Add seeds to visited and queue them
    {
        let mut visited_lock = visited.lock().await;
        for seed in seeds {
            if visited_lock.insert(seed.clone()) {
                discovered_tx.send(seed).expect("Failed to enqueue seed");
            }
        }
    }
    
    println!("Starting crawl with limit of {} pages...", MAX_PAGES);
    
    // Process URLs concurrently using a stream
    UnboundedReceiverStream::new(processing_rx)
        .for_each_concurrent(CONCURRENCY, |url| {
            let visited = visited.clone();
            let pages_count = pages_count.clone();
            let discovered_tx = discovered_tx.clone();
            
            async move {
                let current_count = pages_count.fetch_add(1, Ordering::Relaxed) + 1;
                if current_count > MAX_PAGES {
                    return;
                }
                
                match process_link(url).await {
                    Some(child_links) => {
                        // Add discovered links to visited set and queue
                        for link in child_links {
                            let mut visited_lock = visited.lock().await;
                            if visited_lock.insert(link.clone()) {
                                let _ = discovered_tx.send(link);
                            }
                        }
                    }
                    None => {
                        eprintln!("Failed to process link");
                    }
                }
            }
        })
        .await;
    
    batch_task.await.unwrap();
    
    println!("Crawl complete! Processed {} pages", pages_count.load(Ordering::Relaxed));
}

async fn process_link(link: String) -> Option<Vec<String>> {
    println!("Processing: {}", link);
    
    let request = reqwest::get(&link).await;
    if let Ok(response) = request {
        let html_content = response.text().await.ok()?;
        let parsed = parser::parse_html(html_content, &link);
        
        let links: Vec<String> = parsed.links.into_iter().collect();
        println!("Found {} links", links.len());
        
        Some(links)
    } else {
        None
    }
}