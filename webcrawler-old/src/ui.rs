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
    widgets::{Bar, BarChart, BarGroup, Block, Borders, Paragraph, Sparkline},
    Terminal,
};
use std::collections::{HashMap, VecDeque};
use std::io;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct CrawlerStats {
    pub pages_crawled: Arc<AtomicUsize>,
    pub pages_written: Arc<AtomicUsize>,
    pub queue_size: Arc<AtomicUsize>,
    pub active_workers: Arc<AtomicUsize>,
    pub errors: Arc<Mutex<VecDeque<String>>>,
    pub rate_history: Arc<Mutex<VecDeque<u64>>>,
    pub domain_counts: Arc<Mutex<HashMap<String, usize>>>,
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
            active_workers: Arc::new(AtomicUsize::new(0)),
            errors: Arc::new(Mutex::new(VecDeque::with_capacity(10))),
            rate_history: Arc::new(Mutex::new(VecDeque::with_capacity(60))),
            domain_counts: Arc::new(Mutex::new(HashMap::new())),
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

    pub fn add_rate(&self, rate: u64) {
        let mut history = self.rate_history.lock().unwrap();
        if history.len() >= 60 {
            history.pop_front();
        }
        history.push_back(rate);
    }

    pub fn increment_domain(&self, url: &str) {
        if let Ok(parsed) = url::Url::parse(url)
            && let Some(domain) = parsed.domain() {
            let mut counts = self.domain_counts.lock().unwrap();
            *counts.entry(domain.to_string()).or_insert(0) += 1;
        }
    }

    pub fn should_stop(&self) -> bool {
        self.should_stop.load(Ordering::Relaxed)
    }

    pub fn stop(&self) {
        self.should_stop.store(true, Ordering::Relaxed);
    }
}

pub async fn run_ui(stats: Arc<CrawlerStats>, max_pages: usize) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_ui_loop(&mut terminal, stats.clone(), max_pages).await;

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
    let mut animation_frame = 0u8;
    let spinner_frames = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    let mut last_rate_update = Instant::now();
    let mut last_pages = 0usize;

    loop {
        let pages_crawled = stats.pages_crawled.load(Ordering::Relaxed);
        let pages_written = stats.pages_written.load(Ordering::Relaxed);
        let queue_size = stats.queue_size.load(Ordering::Relaxed);
        let active_workers = stats.active_workers.load(Ordering::Relaxed);
        let elapsed = stats.start_time.elapsed();

        // Update rate history every second
        if last_rate_update.elapsed() >= Duration::from_secs(1) {
            let pages_diff = pages_crawled.saturating_sub(last_pages);
            stats.add_rate(pages_diff as u64);
            last_pages = pages_crawled;
            last_rate_update = Instant::now();
        }

        animation_frame = (animation_frame + 1) % (spinner_frames.len() as u8);
        let spinner = spinner_frames[animation_frame as usize];

        terminal.draw(|f| {
            // 2x2 grid layout
            let vertical_chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(f.area());

            let top_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(vertical_chunks[0]);

            let bottom_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(vertical_chunks[1]);

            // Top-left: System info (neofetch style)
            let elapsed_secs = elapsed.as_secs();
            let hours = elapsed_secs / 3600;
            let minutes = (elapsed_secs % 3600) / 60;
            let seconds = elapsed_secs % 60;
            let rate = if elapsed.as_secs() > 0 {
                pages_crawled as f64 / elapsed.as_secs() as f64
            } else {
                0.0
            };

            let worker_status = if active_workers > 0 {
                Span::styled(
                    format!("{} Active", active_workers),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(
                    "Idle",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )
            };

            let queue_status = if queue_size == 0 {
                Span::styled(
                    "Empty",
                    Style::default()
                        .fg(Color::Red)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(
                    format!("{}", queue_size),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )
            };

            let system_info = vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(
                        format!("{} ", spinner),
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        "webcrawler",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from("  ─────────────────"),
                Line::from(vec![
                    Span::styled("  Uptime    : ", Style::default().fg(Color::Cyan)),
                    Span::styled(
                        format!("{:02}:{:02}:{:02}", hours, minutes, seconds),
                        Style::default().fg(Color::White),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("  Rate      : ", Style::default().fg(Color::Cyan)),
                    Span::styled(
                        format!("{:.2} p/s", rate),
                        Style::default().fg(Color::White),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("  Workers   : ", Style::default().fg(Color::Cyan)),
                    worker_status,
                ]),
                Line::from(vec![
                    Span::styled("  Queue     : ", Style::default().fg(Color::Cyan)),
                    queue_status,
                ]),
            ];

            let system_block = Paragraph::new(system_info)
                .block(Block::default().borders(Borders::ALL).title("System"));
            f.render_widget(system_block, top_chunks[0]);

            // Top-right: Error log
            let errors = stats.errors.lock().unwrap();
            let max_errors = 8usize;
            let mut error_lines: Vec<Line> = errors
                .iter()
                .rev()
                .take(max_errors)
                .map(|msg| {
                    Line::from(Span::styled(
                        msg.as_str(),
                        Style::default().fg(Color::Red),
                    ))
                })
                .collect();
            error_lines.reverse();
            if error_lines.is_empty() {
                error_lines.push(Line::from(Span::styled(
                    "No errors",
                    Style::default().fg(Color::Green),
                )));
            }

            let error_block = Paragraph::new(error_lines)
                .block(Block::default().borders(Borders::ALL).title("Errors"));

            f.render_widget(error_block, top_chunks[1]);

            // Bottom-left: Large progress metrics
            let progress_pct = ((pages_crawled as f64 / max_pages as f64) * 100.0).min(100.0);

            let progress_info = vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(
                        format!("{}", pages_crawled),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" / ", Style::default().fg(Color::White)),
                    Span::styled(
                        format!("{}", max_pages),
                        Style::default().fg(Color::White),
                    ),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(
                        format!("{:.1}%", progress_pct),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" Complete", Style::default().fg(Color::White)),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Written: ", Style::default().fg(Color::Cyan)),
                    Span::styled(
                        format!("{}", pages_written),
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
            ];

            let progress_block = Paragraph::new(progress_info).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Progress"),
            );
            f.render_widget(progress_block, bottom_chunks[0]);

            // Bottom-right: Sparkline (rate history)
            let rate_history = stats.rate_history.lock().unwrap();
            let sparkline_data: Vec<u64> = rate_history.iter().copied().collect();
            let max_rate = sparkline_data.iter().max().copied().unwrap_or(1);

            let sparkline = Sparkline::default()
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Rate (pages/sec, 60s)"),
                )
                .data(&sparkline_data)
                .style(Style::default().fg(Color::Cyan))
                .max(max_rate);

            f.render_widget(sparkline, bottom_chunks[1]);
        })?;

        // Check for key press
        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
            && let KeyCode::Char('q') = key.code {
            stats.stop();
            break;
        }

        // Check if done
        if pages_crawled >= max_pages {
            tokio::time::sleep(Duration::from_secs(2)).await;
            break;
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    Ok(())
}
