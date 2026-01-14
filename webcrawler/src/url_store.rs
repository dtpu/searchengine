use rocksdb::{DB, Options, BlockBasedOptions, ColumnFamilyDescriptor};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use url::Url;

/// Persistent URL deduplication store using RocksDB
/// Uses two column families:
/// - "visited": URLs that have been crawled
/// - "frontier": URLs discovered but not yet crawled
pub struct UrlStore {
    db: Arc<DB>,
}

impl UrlStore {
    /// Creates a new UrlStore with optimized RocksDB settings
    pub fn new(path: &str) -> Result<Self, rocksdb::Error> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        opts.set_write_buffer_size(64 * 1024 * 1024);
        opts.set_max_write_buffer_number(3);
        opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
        opts.increase_parallelism(num_cpus::get() as i32);
        
        let mut block_opts = BlockBasedOptions::default();
        block_opts.set_bloom_filter(10.0, false);
        block_opts.set_block_cache(&rocksdb::Cache::new_lru_cache(512 * 1024 * 1024));
        opts.set_block_based_table_factory(&block_opts);
        
        // Try to open with column families, if it fails, destroy old DB and create new one
        let db = match DB::open_cf_descriptors(&opts, path, vec![
            ColumnFamilyDescriptor::new("visited", opts.clone()),
            ColumnFamilyDescriptor::new("frontier", opts.clone())
        ]) {
            Ok(db) => db,
            Err(_) => {
                // Old database format, destroy and recreate
                eprintln!("Existing database is in old format. Creating new database...");
                DB::destroy(&opts, path)?;
                DB::open_cf_descriptors(&opts, path, vec![
                    ColumnFamilyDescriptor::new("visited", opts.clone()),
                    ColumnFamilyDescriptor::new("frontier", opts.clone())
                ])?
            }
        };
        
        Ok(Self {
            db: Arc::new(db),
        })
    }
    
    /// Add URL to frontier if not already visited or in frontier
    /// Returns true if added to frontier, false if already seen
    pub fn add_to_frontier(&self, url: &str) -> bool {
        let normalized = Self::normalize_url(url);
        let key = normalized.as_bytes();
        
        let visited_cf = self.db.cf_handle("visited").unwrap();
        let frontier_cf = self.db.cf_handle("frontier").unwrap();
        
        // Check if already visited
        if self.db.get_cf(visited_cf, key).unwrap_or(None).is_some() {
            return false;
        }
        
        // Check if already in frontier
        if self.db.get_cf(frontier_cf, key).unwrap_or(None).is_some() {
            return false;
        }
        
        // Add to frontier
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .to_le_bytes();
        self.db.put_cf(frontier_cf, key, &timestamp).unwrap_or_else(|e| {
            eprintln!("Failed to add URL to frontier: {}", e);
        });
        true
    }
    
    /// Pop a URL from the frontier and mark it as visited
    /// Returns None if frontier is empty
    pub fn pop_from_frontier(&self) -> Option<String> {
        let frontier_cf = self.db.cf_handle("frontier").unwrap();
        let visited_cf = self.db.cf_handle("visited").unwrap();
        
        let mut iter = self.db.iterator_cf(frontier_cf, rocksdb::IteratorMode::Start);
        if let Some(Ok((key, _))) = iter.next() {
            let url = String::from_utf8_lossy(&key).to_string();
            
            // Move from frontier to visited
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                .to_le_bytes();
            self.db.put_cf(visited_cf, &key, &timestamp).ok();
            self.db.delete_cf(frontier_cf, &key).ok();
            
            Some(url)
        } else {
            None
        }
    }
    
    /// Get count of URLs in frontier
    pub fn frontier_count(&self) -> usize {
        let frontier_cf = self.db.cf_handle("frontier").unwrap();
        let iter = self.db.iterator_cf(frontier_cf, rocksdb::IteratorMode::Start);
        iter.count()
    }
    
    /// Get pages crawled count from database
    pub fn get_pages_crawled(&self) -> usize {
        let visited_cf = self.db.cf_handle("visited").unwrap();
        if let Ok(Some(bytes)) = self.db.get_cf(visited_cf, b"__stats_pages_crawled__") {
            if bytes.len() == 8 {
                let array: [u8; 8] = bytes[..8].try_into().unwrap_or([0u8; 8]);
                return usize::from_le_bytes(array);
            }
        }
        0
    }
    
    /// Update pages crawled count in database
    pub fn set_pages_crawled(&self, count: usize) {
        let visited_cf = self.db.cf_handle("visited").unwrap();
        self.db.put_cf(visited_cf, b"__stats_pages_crawled__", &count.to_le_bytes()).ok();
    }
    
    /// Mark a URL as visited (for canonical URLs)
    pub fn mark_visited(&self, url: &str) {
        let normalized = Self::normalize_url(url);
        let key = normalized.as_bytes();
        let visited_cf = self.db.cf_handle("visited").unwrap();
        
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .to_le_bytes();
        self.db.put_cf(visited_cf, key, &timestamp).ok();
    }
    
    /// Normalize URL to reduce duplicates
    fn normalize_url(url: &str) -> String {
        let Ok(mut parsed) = Url::parse(url) else {
            return url.to_string();
        };
        
        parsed.set_fragment(None);
        
        let tracking_params = [
            "utm_source", "utm_medium", "utm_campaign", "utm_term", "utm_content",
            "fbclid", "gclid", "msclkid", "mc_cid", "mc_eid",
        ];
        
        let mut query_pairs: Vec<(String, String)> = parsed.query_pairs().into_owned().collect();
        query_pairs.retain(|(key, _)| !tracking_params.contains(&key.as_str()));
        
        if query_pairs.is_empty() {
            parsed.set_query(None);
        } else {
            query_pairs.sort_by(|a, b| a.0.cmp(&b.0));
            let query_string = query_pairs
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join("&");
            parsed.set_query(Some(&query_string));
        }
        
        let mut normalized = parsed.to_string();
        
        if normalized.ends_with('/') && normalized.matches('/').count() > 3 {
            normalized.pop();
        }
        
        normalized
    }
}

// Make UrlStore cloneable by cloning the Arc
impl Clone for UrlStore {
    fn clone(&self) -> Self {
        Self {
            db: self.db.clone(),
        }
    }
}
