use chrono::{DateTime, Local};
use ckbadger_common::MemoryStatsData;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::chart::{render_bar_chart, ChartStats};
use crate::db::{ChainInfoData, SyncStatusRow, TuiDb};

const RATE_HISTORY_SIZE: usize = 3600;
const LOG_HISTORY_SIZE: usize = 200;

const TERMINAL_GREEN: Color = Color::Rgb(0, 255, 65);
const TERMINAL_DIM: Color = Color::Rgb(0, 204, 51);
const AMBER: Color = Color::Rgb(255, 176, 0);
const SLATE_800: Color = Color::Rgb(58, 71, 89);
const SLATE_700: Color = Color::Rgb(80, 95, 115);
const SLATE_500: Color = Color::Rgb(160, 174, 192);
const FOREGROUND: Color = Color::Rgb(237, 237, 237);
const ERROR_RED: Color = Color::Rgb(239, 68, 68);

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: DateTime<Local>,
    pub message: String,
    pub level: LogLevel,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LogLevel {
    Info,
    Success,
    Warning,
}

impl LogLevel {
    fn color(&self) -> Color {
        match self {
            LogLevel::Info => TERMINAL_DIM,
            LogLevel::Success => TERMINAL_GREEN,
            LogLevel::Warning => AMBER,
        }
    }

    fn prefix(&self) -> &'static str {
        match self {
            LogLevel::Info => "INFO",
            LogLevel::Success => " OK ",
            LogLevel::Warning => "WARN",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SyncTab {
    #[default]
    ChainInfo,
    SyncProgress,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ChartMode {
    #[default]
    SyncRate,
    DbWrite,
}

pub struct App {
    db: TuiDb,
    sync_status: Option<SyncStatusRow>,
    memory_stats: Option<MemoryStatsData>,
    chain_info: Option<ChainInfoData>,
    last_refresh: Instant,
    last_sample: Instant,
    status_message: Option<(String, Instant)>,
    rate_history: VecDeque<f64>,
    db_write_history: VecDeque<f64>,
    chart_mode: ChartMode,
    log_entries: VecDeque<LogEntry>,
    log_scroll: usize,
    sync_tab: SyncTab,
    prev_is_bulk_sync: Option<bool>,
    prev_is_syncing: Option<bool>,
    prev_indexes_deferred: Option<bool>,
    prev_is_direct_db_read: Option<bool>,
}

impl App {
    pub fn new(db: TuiDb) -> Self {
        let mut log_entries = VecDeque::with_capacity(LOG_HISTORY_SIZE);
        log_entries.push_back(LogEntry {
            timestamp: Local::now(),
            message: "ckbadger-tui started".to_string(),
            level: LogLevel::Info,
        });

        Self {
            db,
            sync_status: None,
            memory_stats: None,
            chain_info: None,
            last_refresh: Instant::now(),
            last_sample: Instant::now(),
            status_message: None,
            rate_history: VecDeque::with_capacity(RATE_HISTORY_SIZE),
            db_write_history: VecDeque::with_capacity(RATE_HISTORY_SIZE),
            chart_mode: ChartMode::default(),
            log_entries,
            log_scroll: 0,
            sync_tab: SyncTab::default(),
            prev_is_bulk_sync: None,
            prev_is_syncing: None,
            prev_indexes_deferred: None,
            prev_is_direct_db_read: None,
        }
    }

    pub fn toggle_sync_tab(&mut self) {
        self.sync_tab = match self.sync_tab {
            SyncTab::ChainInfo => SyncTab::SyncProgress,
            SyncTab::SyncProgress => SyncTab::ChainInfo,
        };
    }

    pub fn toggle_chart_mode(&mut self) {
        self.chart_mode = match self.chart_mode {
            ChartMode::SyncRate => ChartMode::DbWrite,
            ChartMode::DbWrite => ChartMode::SyncRate,
        };
    }

    pub fn scroll_log_up(&mut self) {
        if self.log_scroll < self.log_entries.len().saturating_sub(1) {
            self.log_scroll += 1;
        }
    }

    pub fn scroll_log_down(&mut self) {
        self.log_scroll = self.log_scroll.saturating_sub(1);
    }

    pub fn scroll_log_to_bottom(&mut self) {
        self.log_scroll = 0;
    }

    pub fn scroll_log_to_top(&mut self) {
        self.log_scroll = self.log_entries.len().saturating_sub(1);
    }

    pub async fn refresh(&mut self) {
        match self.db.get_sync_status().await {
            Ok(status) => self.sync_status = Some(status),
            Err(e) => {
                self.sync_status = None;
                self.log_warning(format!("Failed to load sync status: {e}"));
            }
        }

        self.memory_stats = self.db.get_memory_stats().await;
        self.chain_info = self.db.get_chain_info().await;
        self.last_refresh = Instant::now();

        self.detect_events();

        if self.last_sample.elapsed().as_secs() >= 1 {
            self.sample_rates();
            self.last_sample = Instant::now();
        }
    }

    pub fn should_refresh(&self, interval: Duration) -> bool {
        self.last_refresh.elapsed() >= interval
    }

    fn sample_rates(&mut self) {
        let rate = self
            .sync_status
            .as_ref()
            .and_then(|s| s.rate_realtime)
            .unwrap_or(0.0);

        if self.rate_history.len() >= RATE_HISTORY_SIZE {
            self.rate_history.pop_front();
        }
        self.rate_history.push_back(rate);

        let db_ms = self
            .sync_status
            .as_ref()
            .and_then(|s| s.db_write_ms)
            .unwrap_or(0.0);

        if self.db_write_history.len() >= RATE_HISTORY_SIZE {
            self.db_write_history.pop_front();
        }
        self.db_write_history.push_back(db_ms);
    }

    fn detect_events(&mut self) {
        let Some(sync) = self.sync_status.as_ref() else {
            return;
        };

        let is_bulk_sync = sync.is_bulk_sync;
        let is_syncing = sync.is_syncing;
        let indexes_deferred = sync.indexes_deferred;
        let is_direct_db_read = sync.is_direct_db_read;

        if let Some(prev_bulk) = self.prev_is_bulk_sync {
            if prev_bulk && !is_bulk_sync {
                self.push_log("Bulk sync completed".to_string(), LogLevel::Success);
            } else if !prev_bulk && is_bulk_sync {
                self.push_log("Bulk sync started".to_string(), LogLevel::Info);
            }
        }
        self.prev_is_bulk_sync = Some(is_bulk_sync);

        if let Some(prev_syncing) = self.prev_is_syncing {
            if prev_syncing && !is_syncing {
                self.push_log(
                    "Sync completed, now in real-time mode".to_string(),
                    LogLevel::Success,
                );
            } else if !prev_syncing && is_syncing {
                self.push_log("Syncing started".to_string(), LogLevel::Info);
            }
        }
        self.prev_is_syncing = Some(is_syncing);

        if let Some(prev_deferred) = self.prev_indexes_deferred {
            if prev_deferred && !indexes_deferred {
                self.push_log("Deferred indexes rebuilt".to_string(), LogLevel::Success);
            } else if !prev_deferred && indexes_deferred {
                self.push_log(
                    "Indexes deferred during bulk sync".to_string(),
                    LogLevel::Warning,
                );
            }
        }
        self.prev_indexes_deferred = Some(indexes_deferred);

        if let Some(prev_direct) = self.prev_is_direct_db_read {
            if prev_direct && !is_direct_db_read {
                self.push_log("Data source switched to RPC".to_string(), LogLevel::Info);
            } else if !prev_direct && is_direct_db_read {
                self.push_log(
                    "Data source switched to direct DB".to_string(),
                    LogLevel::Info,
                );
            }
        }
        self.prev_is_direct_db_read = Some(is_direct_db_read);
    }

    fn log_warning(&mut self, message: String) {
        self.status_message = Some((message.clone(), Instant::now()));
        self.push_log(message, LogLevel::Warning);
    }

    fn push_log(&mut self, message: String, level: LogLevel) {
        self.log_entries.push_back(LogEntry {
            timestamp: Local::now(),
            message,
            level,
        });
        while self.log_entries.len() > LOG_HISTORY_SIZE {
            self.log_entries.pop_front();
        }
    }
}

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(7),
            Constraint::Length(8),
            Constraint::Length(9),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(f.area());

    draw_header(f, app, chunks[0]);
    draw_sync_status_full(f, app, chunks[1]);
    draw_rate_chart_full(f, app, chunks[2]);
    draw_memory_stats(f, app, chunks[3]);
    draw_log(f, app, chunks[4]);
    draw_footer(f, app, chunks[5]);
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SLATE_800));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(inner);

    let source_text = app
        .sync_status
        .as_ref()
        .map(|s| if s.is_direct_db_read { "[DB]" } else { "[RPC]" })
        .unwrap_or("[N/A]");

    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            "CKBadger",
            Style::default()
                .fg(TERMINAL_GREEN)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Monitor ", Style::default().fg(FOREGROUND)),
        Span::styled(source_text, Style::default().fg(AMBER)),
    ]));
    f.render_widget(title, cols[0]);

    let now = Local::now();
    let elapsed_ms = app.last_refresh.elapsed().as_millis();
    let right = Paragraph::new(Line::from(vec![
        Span::styled(
            format!("{}ms ago", elapsed_ms),
            Style::default().fg(if elapsed_ms > 3000 { AMBER } else { SLATE_500 }),
        ),
        Span::styled(" │ ", Style::default().fg(SLATE_700)),
        Span::styled(
            now.format("%H:%M:%S").to_string(),
            Style::default().fg(FOREGROUND),
        ),
    ]))
    .alignment(Alignment::Right);
    f.render_widget(right, cols[1]);
}

fn draw_sync_status_full(f: &mut Frame, app: &App, area: Rect) {
    match app.sync_tab {
        SyncTab::ChainInfo => draw_chain_info(f, app, area),
        SyncTab::SyncProgress => draw_sync_progress(f, app, area),
    }
}

fn draw_chain_info(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SLATE_800))
        .title(Span::styled(
            "Chain Info [s]",
            Style::default().fg(FOREGROUND),
        ));

    let Some(info) = &app.chain_info else {
        let msg = Paragraph::new("No chain data available").block(block);
        f.render_widget(msg, area);
        return;
    };

    let inner = block.inner(area);
    f.render_widget(block, area);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ])
        .split(inner);

    let has_epoch = info.epoch_length > 0;

    let mut left_lines = vec![Line::from(vec![
        Span::styled("Latest Block: ", Style::default().fg(SLATE_500)),
        Span::styled(
            format_num_commas(info.latest_block),
            Style::default()
                .fg(TERMINAL_GREEN)
                .add_modifier(Modifier::BOLD),
        ),
    ])];

    if has_epoch {
        let epoch_ratio = (info.epoch_index as f64 / info.epoch_length as f64).clamp(0.0, 1.0);
        let bar_width = 20;
        let filled = (epoch_ratio * bar_width as f64) as usize;
        let epoch_bar = format!("[{}{}]", "█".repeat(filled), "░".repeat(bar_width - filled));

        left_lines.push(Line::from(vec![
            Span::styled("Epoch: ", Style::default().fg(SLATE_500)),
            Span::styled(
                format!(
                    "{} ({}/{})",
                    info.epoch_number, info.epoch_index, info.epoch_length
                ),
                Style::default().fg(TERMINAL_GREEN),
            ),
        ]));
        left_lines.push(Line::from(Span::styled(
            epoch_bar,
            Style::default().fg(TERMINAL_GREEN),
        )));
    } else {
        left_lines.push(Line::from(vec![
            Span::styled("Epoch: ", Style::default().fg(SLATE_500)),
            Span::styled("-", Style::default().fg(SLATE_500)),
        ]));
    }
    f.render_widget(Paragraph::new(left_lines), cols[0]);

    let mid_lines = vec![
        Line::from(vec![
            Span::styled("Difficulty: ", Style::default().fg(SLATE_500)),
            Span::styled(
                &info.difficulty,
                Style::default()
                    .fg(TERMINAL_GREEN)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Hash Rate:  ", Style::default().fg(SLATE_500)),
            Span::styled(&info.hash_rate, Style::default().fg(TERMINAL_GREEN)),
        ]),
        Line::from(vec![
            Span::styled("Avg Block:  ", Style::default().fg(SLATE_500)),
            Span::styled(&info.avg_block_time, Style::default().fg(FOREGROUND)),
        ]),
    ];
    f.render_widget(Paragraph::new(mid_lines), cols[1]);

    let right_lines = vec![
        Line::from(vec![
            Span::styled("TPS (24h): ", Style::default().fg(SLATE_500)),
            Span::styled(&info.tps, Style::default().fg(TERMINAL_GREEN)),
        ]),
        Line::from(vec![
            Span::styled("Txns (24h): ", Style::default().fg(SLATE_500)),
            Span::styled(format_num(info.tx_24h), Style::default().fg(FOREGROUND)),
        ]),
    ];
    f.render_widget(Paragraph::new(right_lines), cols[2]);
}

fn draw_sync_progress(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SLATE_800))
        .title(Span::styled(
            "Sync Status [s]",
            Style::default().fg(FOREGROUND),
        ));

    let Some(sync) = &app.sync_status else {
        let msg = Paragraph::new("No sync data available").block(block);
        f.render_widget(msg, area);
        return;
    };

    let inner = block.inner(area);
    f.render_widget(block, area);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(22),
            Constraint::Min(24),
            Constraint::Length(24),
        ])
        .split(inner);

    let (mode, mode_color) = if !sync.is_syncing {
        ("SYNCED", TERMINAL_GREEN)
    } else if sync.is_bulk_sync {
        ("BULK SYNC", AMBER)
    } else {
        ("SYNCING", TERMINAL_GREEN)
    };

    let mut tags: Vec<Span> = Vec::new();
    if sync.address_balances_deferred {
        tags.push(Span::styled("[BAL]", Style::default().fg(AMBER)));
    }
    if sync.activities_deferred {
        tags.push(Span::styled(" [ACT]", Style::default().fg(AMBER)));
    }
    if sync.token_deferred {
        tags.push(Span::styled(" [TOK]", Style::default().fg(AMBER)));
    }
    if sync.spore_deferred {
        tags.push(Span::styled(" [SPR]", Style::default().fg(AMBER)));
    }
    if sync.tx_block_map_deferred {
        tags.push(Span::styled(" [TXM]", Style::default().fg(AMBER)));
    }

    let mut left = vec![Line::from(vec![Span::styled(
        format!(" {} ", mode),
        Style::default().fg(Color::Black).bg(mode_color),
    )])];

    if !tags.is_empty() {
        left.push(Line::from(tags));
    }

    left.push(Line::from(vec![
        Span::styled("Progress: ", Style::default().fg(SLATE_500)),
        Span::styled(
            format!("{:.2}%", sync.progress),
            Style::default()
                .fg(TERMINAL_GREEN)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    let ratio = (sync.progress / 100.0).clamp(0.0, 1.0);
    let bar_width = 16;
    let filled = (ratio * bar_width as f64) as usize;
    let bar = format!("[{}{}]", "█".repeat(filled), "░".repeat(bar_width - filled));
    left.push(Line::from(Span::styled(
        bar,
        Style::default().fg(TERMINAL_GREEN),
    )));
    f.render_widget(Paragraph::new(left), cols[0]);

    let blocks_behind = sync.chain_tip - sync.tip_block;
    let mid = vec![
        Line::from(vec![
            Span::styled("Current: ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_num(sync.tip_block),
                Style::default()
                    .fg(TERMINAL_GREEN)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" / {}", format_num(sync.chain_tip)),
                Style::default().fg(SLATE_500),
            ),
        ]),
        Line::from(vec![
            Span::styled("Behind:  ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_num(blocks_behind),
                Style::default().fg(if blocks_behind > 1000 {
                    AMBER
                } else {
                    TERMINAL_GREEN
                }),
            ),
        ]),
        Line::from(vec![
            Span::styled("Now:     ", Style::default().fg(SLATE_500)),
            if let Some(rt) = sync.rate_realtime {
                Span::styled(
                    format!("{rt:.0} blk/s"),
                    Style::default()
                        .fg(TERMINAL_GREEN)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled("-", Style::default().fg(SLATE_500))
            },
        ]),
        Line::from(vec![
            Span::styled("EMA:     ", Style::default().fg(SLATE_500)),
            if let Some(ema) = sync.rate_ema {
                Span::raw(format!("{ema:.0} blk/s"))
            } else {
                Span::styled("-", Style::default().fg(SLATE_500))
            },
        ]),
    ];
    f.render_widget(Paragraph::new(mid), cols[1]);

    let mut right = Vec::new();
    if let Some(ref eta) = sync.eta {
        right.push(Line::from(vec![
            Span::styled("ETA: ", Style::default().fg(SLATE_500)),
            Span::styled(
                eta,
                Style::default()
                    .fg(TERMINAL_GREEN)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    if let Some(ref elapsed) = sync.elapsed_time {
        right.push(Line::from(vec![
            Span::styled("Elapsed: ", Style::default().fg(SLATE_500)),
            Span::styled(elapsed, Style::default().fg(FOREGROUND)),
        ]));
    }

    if let Some(ms) = sync.db_write_ms {
        right.push(Line::from(vec![
            Span::styled("DB Write: ", Style::default().fg(SLATE_500)),
            Span::styled(format!("{ms:.1} ms"), Style::default().fg(TERMINAL_DIM)),
        ]));
    }

    if let Some(ms) = sync.rpc_fetch_ms {
        right.push(Line::from(vec![
            Span::styled("RPC Fetch: ", Style::default().fg(SLATE_500)),
            Span::styled(format!("{ms:.1} ms"), Style::default().fg(TERMINAL_DIM)),
        ]));
    }

    if right.is_empty() {
        right.push(Line::from(Span::styled(
            "No timing data",
            Style::default().fg(SLATE_500),
        )));
    }

    f.render_widget(Paragraph::new(right), cols[2]);
}

fn draw_rate_chart_full(f: &mut Frame, app: &App, area: Rect) {
    let (title, unit, data) = match app.chart_mode {
        ChartMode::SyncRate => ("Sync Rate Chart [v]", "blk/s", &app.rate_history),
        ChartMode::DbWrite => ("DB Write Chart [v]", "ms", &app.db_write_history),
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SLATE_800))
        .title(Span::styled(title, Style::default().fg(FOREGROUND)));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.width < 10 || inner.height < 3 {
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(inner);

    let stats_line = if let Some(stats) = ChartStats::from_history(data) {
        Line::from(vec![
            Span::styled("cur ", Style::default().fg(SLATE_500)),
            Span::styled(
                format!("{:.1}{unit}", stats.current),
                Style::default().fg(TERMINAL_GREEN),
            ),
            Span::styled(" | avg ", Style::default().fg(SLATE_500)),
            Span::styled(
                format!("{:.1}{unit}", stats.avg),
                Style::default().fg(TERMINAL_DIM),
            ),
            Span::styled(" | min ", Style::default().fg(SLATE_500)),
            Span::styled(
                format!("{:.1}{unit}", stats.min),
                Style::default().fg(FOREGROUND),
            ),
            Span::styled(" | max ", Style::default().fg(SLATE_500)),
            Span::styled(
                format!("{:.1}{unit}", stats.max),
                Style::default().fg(AMBER),
            ),
        ])
    } else {
        Line::from(Span::styled(
            "No samples yet",
            Style::default().fg(SLATE_500),
        ))
    };
    f.render_widget(Paragraph::new(stats_line), rows[0]);

    let chart = render_bar_chart(data, rows[1].width as usize, rows[1].height as usize);
    let chart_lines: Vec<Line> = chart
        .rows
        .into_iter()
        .map(|row| Line::from(Span::styled(row.content, Style::default().fg(row.color))))
        .collect();
    f.render_widget(
        Paragraph::new(chart_lines).wrap(Wrap { trim: false }),
        rows[1],
    );
}

fn draw_memory_stats(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SLATE_800))
        .title(Span::styled(
            "Memory Stats",
            Style::default().fg(FOREGROUND),
        ));

    let Some(mem) = &app.memory_stats else {
        let msg = Paragraph::new("No memory stats (Redis unavailable)").block(block);
        f.render_widget(msg, area);
        return;
    };

    let inner = block.inner(area);
    f.render_widget(block, area);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ])
        .split(inner);

    let left = vec![
        Line::from(vec![
            Span::styled("Live Cells: ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_num_u64(mem.live_cells_count),
                Style::default().fg(TERMINAL_GREEN),
            ),
        ]),
        Line::from(vec![
            Span::styled("Consumed:   ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_num_u64(mem.consumed_cells_count),
                Style::default().fg(FOREGROUND),
            ),
        ]),
        Line::from(vec![
            Span::styled("Consumed B: ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_bytes(mem.consumed_cells_bytes),
                Style::default().fg(FOREGROUND),
            ),
        ]),
        Line::from(vec![
            Span::styled("BlockHdrs:  ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_num_u64(mem.block_headers_count),
                Style::default().fg(TERMINAL_DIM),
            ),
        ]),
    ];
    f.render_widget(Paragraph::new(left), cols[0]);

    let mid = vec![
        Line::from(vec![
            Span::styled("RocksDB Total: ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_bytes(mem.rocksdb_total_bytes),
                Style::default().fg(TERMINAL_GREEN),
            ),
        ]),
        Line::from(vec![
            Span::styled("Memtable:      ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_bytes(mem.rocksdb_memtable_bytes),
                Style::default().fg(FOREGROUND),
            ),
        ]),
        Line::from(vec![
            Span::styled("Block Cache:   ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_bytes(mem.rocksdb_block_cache_bytes),
                Style::default().fg(FOREGROUND),
            ),
        ]),
        Line::from(vec![
            Span::styled("TableReaders:  ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_bytes(mem.rocksdb_table_readers_bytes),
                Style::default().fg(TERMINAL_DIM),
            ),
        ]),
    ];
    f.render_widget(Paragraph::new(mid), cols[1]);

    let right = vec![
        Line::from(vec![
            Span::styled("Txs:  ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_num(mem.total_transactions),
                Style::default().fg(TERMINAL_GREEN),
            ),
        ]),
        Line::from(vec![
            Span::styled("Cells:", Style::default().fg(SLATE_500)),
            Span::styled(format_num(mem.total_cells), Style::default().fg(FOREGROUND)),
        ]),
        Line::from(vec![
            Span::styled("Live: ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_num(mem.total_live_cells),
                Style::default().fg(FOREGROUND),
            ),
        ]),
        Line::from(vec![
            Span::styled("Addrs:", Style::default().fg(SLATE_500)),
            Span::styled(
                format_num(mem.total_addresses),
                Style::default().fg(TERMINAL_DIM),
            ),
        ]),
    ];
    f.render_widget(Paragraph::new(right), cols[2]);
}

fn draw_log(f: &mut Frame, app: &App, area: Rect) {
    let title = if app.log_scroll > 0 {
        format!("Events [j/k g/G] (scroll +{})", app.log_scroll)
    } else {
        "Events [j/k g/G]".to_string()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SLATE_800))
        .title(Span::styled(title, Style::default().fg(FOREGROUND)));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height == 0 {
        return;
    }

    if app.log_entries.is_empty() {
        let msg = Paragraph::new("No events");
        f.render_widget(msg, inner);
        return;
    }

    let visible = inner.height as usize;
    let total = app.log_entries.len();
    let base_start = total.saturating_sub(visible);
    let start = base_start.saturating_sub(app.log_scroll);
    let end = (start + visible).min(total);

    let lines: Vec<Line> = app
        .log_entries
        .iter()
        .skip(start)
        .take(end.saturating_sub(start))
        .map(|entry| {
            Line::from(vec![
                Span::styled(
                    entry.timestamp.format("%H:%M:%S").to_string(),
                    Style::default().fg(SLATE_500),
                ),
                Span::styled(" ", Style::default().fg(SLATE_700)),
                Span::styled(
                    entry.level.prefix(),
                    Style::default().fg(entry.level.color()),
                ),
                Span::styled(" ", Style::default().fg(SLATE_700)),
                Span::styled(&entry.message, Style::default().fg(FOREGROUND)),
            ])
        })
        .collect();

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SLATE_800));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let hint = Line::from(vec![
        Span::styled("q", Style::default().fg(TERMINAL_GREEN)),
        Span::styled(" quit  ", Style::default().fg(SLATE_500)),
        Span::styled("s", Style::default().fg(TERMINAL_GREEN)),
        Span::styled(" sync-tab  ", Style::default().fg(SLATE_500)),
        Span::styled("v", Style::default().fg(TERMINAL_GREEN)),
        Span::styled(" chart-mode  ", Style::default().fg(SLATE_500)),
        Span::styled("j/k", Style::default().fg(TERMINAL_GREEN)),
        Span::styled(" log-scroll  ", Style::default().fg(SLATE_500)),
        Span::styled("R", Style::default().fg(TERMINAL_GREEN)),
        Span::styled(" refresh", Style::default().fg(SLATE_500)),
    ]);

    let mut lines = vec![hint];
    if let Some((msg, ts)) = &app.status_message {
        let color = if ts.elapsed().as_secs() < 5 {
            AMBER
        } else {
            ERROR_RED
        };
        lines.push(Line::from(Span::styled(
            msg.clone(),
            Style::default().fg(color),
        )));
    }

    let paragraph = Paragraph::new(lines).alignment(Alignment::Left);
    f.render_widget(paragraph, inner);
}

fn format_num(value: i64) -> String {
    if value < 0 {
        return format!("-{}", format_num_commas(-value));
    }
    format_num_commas(value)
}

fn format_num_u64(value: u64) -> String {
    format_num_commas(value as i64)
}

fn format_num_commas(value: i64) -> String {
    let s = value.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.2} MB", b / MB)
    } else if b >= KB {
        format!("{:.2} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::{format_num, format_num_commas};

    #[test]
    fn test_format_num_commas() {
        assert_eq!(format_num_commas(1), "1");
        assert_eq!(format_num_commas(12_345), "12,345");
        assert_eq!(format_num_commas(12_345_678), "12,345,678");
    }

    #[test]
    fn test_format_num_with_negative() {
        assert_eq!(format_num(-123_456), "-123,456");
    }
}
