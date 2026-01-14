use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph},
    Terminal,
};
use std::collections::VecDeque;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct CrawlerStats {
    pub pages_crawled: Arc<AtomicUsize>,
    pub pages_written: Arc<AtomicUsize>,
    pub queue_size: Arc<AtomicUsize>,
    pub errors: Arc<Mutex<VecDeque<String>>>,
    pub start_time: Instant,
    pub should_stop: Arc<AtomicBool>,
}

impl CrawlerStats {
    pub fn new(
        pages_crawled: Arc<AtomicUsize>,
        pages_written: Arc<AtomicUsize>,
        queue_size: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            pages_crawled,
            pages_written,
            queue_size,
            errors: Arc::new(Mutex::new(VecDeque::with_capacity(10))),
            start_time: Instant::now(),
            should_stop: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn add_error(&self, error: String) {
        let mut errors = self.errors.lock().unwrap();
        if errors.len() >= 10 {
            errors.pop_front();
        }
        errors.push_back(error);
    }

    pub fn should_stop(&self) -> bool {
        self.should_stop.load(Ordering::Relaxed)
    }

    pub fn stop(&self) {
        self.should_stop.store(true, Ordering::Relaxed);
    }
}

pub async fn run_ui(stats: Arc<CrawlerStats>, max_pages: usize) -> io::Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_ui_loop(&mut terminal, stats.clone(), max_pages).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

async fn run_ui_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    stats: Arc<CrawlerStats>,
    max_pages: usize,
) -> io::Result<()> {
    let mut scroll_offset = 0usize;
    
    loop {
        let pages_crawled = stats.pages_crawled.load(Ordering::Relaxed);
        let pages_written = stats.pages_written.load(Ordering::Relaxed);
        let queue_size = stats.queue_size.load(Ordering::Relaxed);
        let elapsed = stats.start_time.elapsed();

        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Length(7),
                    Constraint::Min(10),
                ])
                .split(f.area());

            // Progress bar
            let progress = ((pages_crawled as f64 / max_pages as f64) * 100.0).min(100.0) as u16;
            let gauge = Gauge::default()
                .block(Block::default().borders(Borders::ALL).title("Progress"))
                .gauge_style(
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                )
                .percent(progress)
                .label(format!("{} / {} ({}%)", pages_crawled, max_pages, progress));
            f.render_widget(gauge, chunks[0]);

            // Stats
            let elapsed_secs = elapsed.as_secs();
            let hours = elapsed_secs / 3600;
            let minutes = (elapsed_secs % 3600) / 60;
            let seconds = elapsed_secs % 60;
            let rate = if elapsed.as_secs() > 0 {
                pages_crawled as f64 / elapsed.as_secs() as f64
            } else {
                0.0
            };

            let stats_text = vec![
                Line::from(vec![
                    Span::styled("Pages Crawled: ", Style::default().fg(Color::Cyan)),
                    Span::styled(
                        format!("{} / {}", pages_crawled, max_pages),
                        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Pages Written: ", Style::default().fg(Color::Cyan)),
                    Span::styled(
                        format!("{}", pages_written),
                        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Queue Size: ", Style::default().fg(Color::Cyan)),
                    Span::styled(
                        format!("{}", queue_size),
                        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Rate: ", Style::default().fg(Color::Cyan)),
                    Span::styled(
                        format!("{:.2} pages/sec", rate),
                        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Elapsed: ", Style::default().fg(Color::Cyan)),
                    Span::styled(
                        format!("{:02}:{:02}:{:02}", hours, minutes, seconds),
                        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                    ),
                ]),
            ];

            let stats_paragraph = Paragraph::new(stats_text)
                .block(Block::default().borders(Borders::ALL).title("Statistics"));
            f.render_widget(stats_paragraph, chunks[1]);

            // Scrollable errors
            let errors = stats.errors.lock().unwrap();
            let total_errors = errors.len();
            let error_items: Vec<ListItem> = errors
                .iter()
                .map(|e| {
                    ListItem::new(e.clone()).style(Style::default().fg(Color::Red))
                })
                .collect();
            
            let error_list = List::new(error_items)
                .block(Block::default()
                    .borders(Borders::ALL)
                    .title(format!("Recent Errors ({}) - ↑/↓ to scroll, q to quit", total_errors)))
                .highlight_style(Style::default().add_modifier(Modifier::BOLD));
            
            let mut list_state = ratatui::widgets::ListState::default();
            if total_errors > 0 {
                list_state.select(Some(scroll_offset.min(total_errors.saturating_sub(1))));
            }
            f.render_stateful_widget(error_list, chunks[2], &mut list_state);
        })?;

        // Check for key presses
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => {
                        stats.stop();
                        break;
                    }
                    KeyCode::Up => {
                        scroll_offset = scroll_offset.saturating_sub(1);
                    }
                    KeyCode::Down => {
                        let total_errors = stats.errors.lock().unwrap().len();
                        if total_errors > 0 {
                            scroll_offset = (scroll_offset + 1).min(total_errors - 1);
                        }
                    }
                    _ => {}
                }
            }
        }

        // Check if crawling is done
        if pages_crawled >= max_pages {
            tokio::time::sleep(Duration::from_secs(2)).await; // Show final stats
            break;
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    Ok(())
}
