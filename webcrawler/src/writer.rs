use crate::parser::ParsedHtml;
use std::io::{BufWriter, Write};
use std::fs::File;
use tokio::sync::mpsc;
use std::time::{Duration, Instant};

const BATCH_SIZE: usize = 100;
const FLUSH_INTERVAL_SECS: u64 = 5;
const MAX_BUFFER_SIZE: usize = 1_000_000;

/// Buffered writer that batches JSONL writes for better performance
pub struct BufferedWriter {
    receiver: mpsc::Receiver<ParsedHtml>,
    writer: BufWriter<File>,
    batch: Vec<String>,
    batch_bytes: usize,
    last_flush: Instant,
}

impl BufferedWriter {
    /// Create a new buffered writer and return the sender channel
    pub fn new(file_path: &str) -> Result<(Self, mpsc::Sender<ParsedHtml>), std::io::Error> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(file_path)?;
        
        let writer = BufWriter::with_capacity(8192, file);
        let (sender, receiver) = mpsc::channel(1000);
        
        Ok((
            Self {
                receiver,
                writer,
                batch: Vec::with_capacity(BATCH_SIZE),
                batch_bytes: 0,
                last_flush: Instant::now(),
            },
            sender,
        ))
    }
    
    /// Run the writer loop - call this in a tokio task
    pub async fn run(mut self) {
        while let Some(parsed) = self.receiver.recv().await {
            if let Err(e) = self.add_to_batch(parsed) {
                eprintln!("Failed to serialize data: {}", e);
                continue;
            }
            
            if self.should_flush() {
                if let Err(e) = self.flush() {
                    eprintln!("Failed to flush batch: {}", e);
                }
            }
        }
        
        let _ = self.flush();
        let _ = self.writer.flush();
    }
    
    fn add_to_batch(&mut self, parsed: ParsedHtml) -> Result<(), serde_json::Error> {
        let json_line = serde_json::to_string(&parsed)?;
        self.batch_bytes += json_line.len() + 1;
        self.batch.push(json_line);
        Ok(())
    }
    
    fn should_flush(&self) -> bool {
        self.batch.len() >= BATCH_SIZE
            || self.batch_bytes >= MAX_BUFFER_SIZE
            || self.last_flush.elapsed() >= Duration::from_secs(FLUSH_INTERVAL_SECS)
    }
    
    fn flush(&mut self) -> std::io::Result<()> {
        if self.batch.is_empty() {
            return Ok(());
        }
        
        for line in &self.batch {
            writeln!(self.writer, "{}", line)?;
        }
        
        self.writer.flush()?;
        self.batch.clear();
        self.batch_bytes = 0;
        self.last_flush = Instant::now();
        
        Ok(())
    }
}
