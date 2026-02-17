use chrono::{DateTime, Local};
use ckbadger_common::MemoryStatsData;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
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
pub enum MainTab {
    #[default]
    Overview,
    Sync,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum LayoutDensity {
    Compact,
    Standard,
    Wide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiagnosticsViewMode {
    Auto,
    Compact,
    Detail,
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
    fetch_stage_history: VecDeque<f64>,
    parse_stage_history: VecDeque<f64>,
    write_stage_history: VecDeque<f64>,
    log_entries: VecDeque<LogEntry>,
    sync_event_entries: VecDeque<LogEntry>,
    log_scroll: usize,
    sync_event_scroll: usize,
    main_tab: MainTab,
    prev_is_bulk_sync: Option<bool>,
    prev_is_syncing: Option<bool>,
    prev_indexes_deferred: Option<bool>,
    prev_is_direct_db_read: Option<bool>,
    prev_bottleneck: Option<SyncBottleneck>,
    last_rate_drop_alert: Option<Instant>,
    stale_warning_active: bool,
    help_visible: bool,
    force_compact_layout: bool,
    diagnostics_view_mode: DiagnosticsViewMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncBottleneck {
    DbBound,
    RpcBound,
    Mixed,
    Unknown,
}

impl App {
    pub fn new(db: TuiDb) -> Self {
        let mut log_entries = VecDeque::with_capacity(LOG_HISTORY_SIZE);
        log_entries.push_back(LogEntry {
            timestamp: Local::now(),
            message: "ckbadger-tui started".to_string(),
            level: LogLevel::Info,
        });
        let mut sync_event_entries = VecDeque::with_capacity(LOG_HISTORY_SIZE);
        sync_event_entries.push_back(LogEntry {
            timestamp: Local::now(),
            message: "sync monitor initialized".to_string(),
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
            fetch_stage_history: VecDeque::with_capacity(RATE_HISTORY_SIZE),
            parse_stage_history: VecDeque::with_capacity(RATE_HISTORY_SIZE),
            write_stage_history: VecDeque::with_capacity(RATE_HISTORY_SIZE),
            log_entries,
            sync_event_entries,
            log_scroll: 0,
            sync_event_scroll: 0,
            main_tab: MainTab::default(),
            prev_is_bulk_sync: None,
            prev_is_syncing: None,
            prev_indexes_deferred: None,
            prev_is_direct_db_read: None,
            prev_bottleneck: None,
            last_rate_drop_alert: None,
            stale_warning_active: false,
            help_visible: false,
            force_compact_layout: false,
            diagnostics_view_mode: DiagnosticsViewMode::Auto,
        }
    }

    pub fn next_tab(&mut self) {
        self.main_tab = match self.main_tab {
            MainTab::Overview => MainTab::Sync,
            MainTab::Sync => MainTab::Overview,
        };
    }

    pub fn previous_tab(&mut self) {
        self.main_tab = match self.main_tab {
            MainTab::Overview => MainTab::Sync,
            MainTab::Sync => MainTab::Overview,
        };
    }

    pub fn toggle_help(&mut self) {
        self.help_visible = !self.help_visible;
    }

    pub fn close_help(&mut self) {
        self.help_visible = false;
    }

    pub fn is_help_visible(&self) -> bool {
        self.help_visible
    }

    pub fn toggle_compact_layout(&mut self) {
        self.force_compact_layout = !self.force_compact_layout;
        let msg = if self.force_compact_layout {
            "Compact layout enabled".to_string()
        } else {
            "Compact layout disabled (auto mode)".to_string()
        };
        self.status_message = Some((msg, Instant::now()));
    }

    pub fn cycle_diagnostics_view_mode(&mut self) {
        self.diagnostics_view_mode = match self.diagnostics_view_mode {
            DiagnosticsViewMode::Auto => DiagnosticsViewMode::Compact,
            DiagnosticsViewMode::Compact => DiagnosticsViewMode::Detail,
            DiagnosticsViewMode::Detail => DiagnosticsViewMode::Auto,
        };

        self.status_message = Some((
            format!(
                "Diagnostics view: {}",
                diagnostics_view_mode_label(self.diagnostics_view_mode)
            ),
            Instant::now(),
        ));
    }

    pub fn scroll_log_up(&mut self) {
        match self.main_tab {
            MainTab::Overview => {
                if self.log_scroll < self.log_entries.len().saturating_sub(1) {
                    self.log_scroll += 1;
                }
            }
            MainTab::Sync => {
                if self.sync_event_scroll < self.sync_event_entries.len().saturating_sub(1) {
                    self.sync_event_scroll += 1;
                }
            }
        }
    }

    pub fn scroll_log_down(&mut self) {
        match self.main_tab {
            MainTab::Overview => {
                self.log_scroll = self.log_scroll.saturating_sub(1);
            }
            MainTab::Sync => {
                self.sync_event_scroll = self.sync_event_scroll.saturating_sub(1);
            }
        }
    }

    pub fn scroll_log_to_bottom(&mut self) {
        match self.main_tab {
            MainTab::Overview => self.log_scroll = 0,
            MainTab::Sync => self.sync_event_scroll = 0,
        }
    }

    pub fn scroll_log_to_top(&mut self) {
        match self.main_tab {
            MainTab::Overview => self.log_scroll = self.log_entries.len().saturating_sub(1),
            MainTab::Sync => {
                self.sync_event_scroll = self.sync_event_entries.len().saturating_sub(1)
            }
        }
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
        self.detect_stale_state();

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

        let (fetch_ms, parse_ms, write_ms) = self
            .sync_status
            .as_ref()
            .and_then(|s| s.pipeline.as_ref())
            .map(|p| {
                (
                    p.fetch_ms.unwrap_or(0.0),
                    p.parse_ms.unwrap_or(0.0),
                    p.write_ms.unwrap_or(0.0),
                )
            })
            .unwrap_or((0.0, 0.0, 0.0));
        push_history_sample(&mut self.fetch_stage_history, fetch_ms);
        push_history_sample(&mut self.parse_stage_history, parse_ms);
        push_history_sample(&mut self.write_stage_history, write_ms);

        if self.rate_history.len() >= 2 {
            let prev = self.rate_history[self.rate_history.len() - 2];
            if prev > 0.0 && rate > 0.0 && rate < prev * 0.65 {
                let should_alert = self
                    .last_rate_drop_alert
                    .map(|t| t.elapsed().as_secs() >= 30)
                    .unwrap_or(true);
                if should_alert {
                    self.push_sync_event_and_log(
                        format!("sync rate drop detected: {:.0} -> {:.0} blk/s", prev, rate),
                        LogLevel::Warning,
                    );
                    self.last_rate_drop_alert = Some(Instant::now());
                }
            }
        }
    }

    fn detect_events(&mut self) {
        let Some(sync) = self.sync_status.as_ref() else {
            return;
        };

        let is_bulk_sync = sync.is_bulk_sync;
        let is_syncing = sync.is_syncing;
        let indexes_deferred = sync.indexes_deferred;
        let is_direct_db_read = sync.is_direct_db_read;
        let bottleneck = sync_bottleneck(sync.db_write_ms, sync.rpc_fetch_ms);

        if let Some(prev_bulk) = self.prev_is_bulk_sync {
            if prev_bulk && !is_bulk_sync {
                self.push_sync_event_and_log("bulk sync completed".to_string(), LogLevel::Success);
            } else if !prev_bulk && is_bulk_sync {
                self.push_sync_event_and_log("bulk sync started".to_string(), LogLevel::Info);
            }
        }
        self.prev_is_bulk_sync = Some(is_bulk_sync);

        if let Some(prev_syncing) = self.prev_is_syncing {
            if prev_syncing && !is_syncing {
                self.push_sync_event_and_log(
                    "sync completed, now in real-time mode".to_string(),
                    LogLevel::Success,
                );
            } else if !prev_syncing && is_syncing {
                self.push_sync_event_and_log("syncing started".to_string(), LogLevel::Info);
            }
        }
        self.prev_is_syncing = Some(is_syncing);

        if let Some(prev_deferred) = self.prev_indexes_deferred {
            if prev_deferred && !indexes_deferred {
                self.push_sync_event_and_log(
                    "deferred indexes rebuilt".to_string(),
                    LogLevel::Success,
                );
            } else if !prev_deferred && indexes_deferred {
                self.push_sync_event_and_log(
                    "indexes deferred during bulk sync".to_string(),
                    LogLevel::Warning,
                );
            }
        }
        self.prev_indexes_deferred = Some(indexes_deferred);

        if let Some(prev_direct) = self.prev_is_direct_db_read {
            if prev_direct && !is_direct_db_read {
                self.push_sync_event_and_log(
                    "data source switched to RPC".to_string(),
                    LogLevel::Info,
                );
            } else if !prev_direct && is_direct_db_read {
                self.push_sync_event_and_log(
                    "data source switched to direct DB".to_string(),
                    LogLevel::Info,
                );
            }
        }
        self.prev_is_direct_db_read = Some(is_direct_db_read);

        if let Some(prev) = self.prev_bottleneck {
            if prev != bottleneck && bottleneck != SyncBottleneck::Unknown {
                self.push_sync_event_and_log(
                    format!("bottleneck changed to {}", bottleneck_label(bottleneck)),
                    LogLevel::Info,
                );
            }
        }
        self.prev_bottleneck = Some(bottleneck);
    }

    fn detect_stale_state(&mut self) {
        let stale_secs = self
            .memory_stats
            .as_ref()
            .map(|m| (chrono::Utc::now().timestamp() - m.updated_at).max(0))
            .unwrap_or(0);
        let stale_now = stale_secs > 30;
        if stale_now && !self.stale_warning_active {
            self.push_sync_event_and_log(
                format!("sync data is stale ({}s)", stale_secs),
                LogLevel::Warning,
            );
        } else if !stale_now && self.stale_warning_active {
            self.push_sync_event_and_log(
                "sync data freshness recovered".to_string(),
                LogLevel::Success,
            );
        }
        self.stale_warning_active = stale_now;
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

    fn push_sync_event_and_log(&mut self, message: String, level: LogLevel) {
        self.push_log(message.clone(), level);
        self.sync_event_entries.push_back(LogEntry {
            timestamp: Local::now(),
            message,
            level,
        });
        while self.sync_event_entries.len() > LOG_HISTORY_SIZE {
            self.sync_event_entries.pop_front();
        }
    }
}

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(f.area());

    draw_header(f, app, chunks[0]);
    draw_tabs(f, app, chunks[1]);
    draw_content(f, app, chunks[2]);
    draw_footer(f, app, chunks[3]);

    if app.help_visible {
        draw_help_popup(f);
    }
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
    let mode_text = app
        .sync_status
        .as_ref()
        .map(|s| {
            if !s.is_syncing {
                "SYNCED"
            } else if s.is_bulk_sync {
                "BULK"
            } else {
                "SYNCING"
            }
        })
        .unwrap_or("UNKNOWN");
    let mode_color = match mode_text {
        "SYNCED" => TERMINAL_GREEN,
        "BULK" => AMBER,
        _ => TERMINAL_DIM,
    };

    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            "CKBadger",
            Style::default()
                .fg(TERMINAL_GREEN)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Monitor ", Style::default().fg(FOREGROUND)),
        Span::styled(source_text, Style::default().fg(AMBER)),
        Span::styled(" ", Style::default().fg(SLATE_700)),
        Span::styled(
            format!(" {} ", mode_text),
            Style::default().fg(Color::Black).bg(mode_color),
        ),
    ]));
    f.render_widget(title, cols[0]);

    let now = Local::now();
    let elapsed_ms = app.last_refresh.elapsed().as_millis();
    let stale_secs = app
        .memory_stats
        .as_ref()
        .map(|m| (chrono::Utc::now().timestamp() - m.updated_at).max(0))
        .unwrap_or(0);
    let right = Paragraph::new(Line::from(vec![
        Span::styled(
            format!("{}ms ago", elapsed_ms),
            Style::default().fg(if elapsed_ms > 3000 { AMBER } else { SLATE_500 }),
        ),
        Span::styled(" │ ", Style::default().fg(SLATE_700)),
        Span::styled(
            format!("stale {}s", stale_secs),
            Style::default().fg(if stale_secs > 30 { AMBER } else { TERMINAL_DIM }),
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

fn draw_tabs(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SLATE_800));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let (overview_style, sync_style) = match app.main_tab {
        MainTab::Overview => (
            Style::default()
                .fg(Color::Black)
                .bg(TERMINAL_GREEN)
                .add_modifier(Modifier::BOLD),
            Style::default().fg(SLATE_500),
        ),
        MainTab::Sync => (
            Style::default().fg(SLATE_500),
            Style::default()
                .fg(Color::Black)
                .bg(TERMINAL_GREEN)
                .add_modifier(Modifier::BOLD),
        ),
    };

    let line = Line::from(vec![
        Span::styled(" Tabs: ", Style::default().fg(SLATE_500)),
        Span::styled(" Overview ", overview_style),
        Span::styled("  ", Style::default().fg(SLATE_700)),
        Span::styled(" Sync ", sync_style),
        Span::styled(
            if app.force_compact_layout {
                "  [Compact]"
            } else {
                "  [Auto]"
            },
            Style::default().fg(if app.force_compact_layout {
                AMBER
            } else {
                SLATE_500
            }),
        ),
        Span::styled("  ", Style::default().fg(SLATE_700)),
        Span::styled(
            format!(
                "[Diag:{}]",
                diagnostics_view_mode_label(app.diagnostics_view_mode)
            ),
            Style::default().fg(diagnostics_view_mode_color(app.diagnostics_view_mode)),
        ),
        Span::styled("  [Tab/s]", Style::default().fg(SLATE_500)),
    ]);
    f.render_widget(Paragraph::new(line), inner);
}

fn draw_content(f: &mut Frame, app: &App, area: Rect) {
    match app.main_tab {
        MainTab::Overview => draw_overview_content(f, app, area),
        MainTab::Sync => draw_sync_content(f, app, area),
    }
}

fn draw_overview_content(f: &mut Frame, app: &App, area: Rect) {
    match detect_layout_density(app, area) {
        LayoutDensity::Compact => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(6),
                    Constraint::Length(7),
                    Constraint::Length(8),
                    Constraint::Min(4),
                ])
                .split(area);

            draw_overview_kpis(f, app, chunks[0]);
            draw_chain_info(f, app, chunks[1]);
            draw_storage_health(f, app, chunks[2]);
            draw_log(f, app, chunks[3]);
        }
        LayoutDensity::Standard => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(6),
                    Constraint::Length(7),
                    Constraint::Length(8),
                    Constraint::Length(8),
                    Constraint::Min(5),
                ])
                .split(area);

            draw_overview_kpis(f, app, chunks[0]);
            draw_chain_info(f, app, chunks[1]);
            draw_memory_stats(f, app, chunks[2]);
            draw_storage_health(f, app, chunks[3]);
            draw_log(f, app, chunks[4]);
        }
        LayoutDensity::Wide => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(6),
                    Constraint::Length(7),
                    Constraint::Length(8),
                    Constraint::Length(8),
                    Constraint::Min(7),
                ])
                .split(area);

            draw_overview_kpis(f, app, chunks[0]);
            draw_chain_info(f, app, chunks[1]);
            draw_memory_stats(f, app, chunks[2]);
            draw_storage_health(f, app, chunks[3]);
            draw_log(f, app, chunks[4]);
        }
    }
}

fn draw_sync_content(f: &mut Frame, app: &App, area: Rect) {
    match detect_layout_density(app, area) {
        LayoutDensity::Compact => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(4),
                    Constraint::Length(7),
                    Constraint::Min(10),
                    Constraint::Min(4),
                ])
                .split(area);
            draw_sync_realtime_bar(f, app, chunks[0]);
            draw_sync_progress(f, app, chunks[1]);
            draw_sync_charts(f, app, chunks[2]);
            draw_sync_events(f, app, chunks[3]);
        }
        LayoutDensity::Standard => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(4),
                    Constraint::Length(7),
                    Constraint::Length(10),
                    Constraint::Length(7),
                    Constraint::Min(5),
                ])
                .split(area);
            draw_sync_realtime_bar(f, app, chunks[0]);
            draw_sync_progress(f, app, chunks[1]);
            draw_sync_charts(f, app, chunks[2]);
            draw_sync_diagnostics(f, app, chunks[3]);
            draw_sync_events(f, app, chunks[4]);
        }
        LayoutDensity::Wide => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(4),
                    Constraint::Length(7),
                    Constraint::Length(10),
                    Constraint::Length(9),
                    Constraint::Min(5),
                ])
                .split(area);
            draw_sync_realtime_bar(f, app, chunks[0]);
            draw_sync_progress(f, app, chunks[1]);
            draw_sync_charts(f, app, chunks[2]);
            draw_sync_diagnostics(f, app, chunks[3]);
            draw_sync_events(f, app, chunks[4]);
        }
    }
}

fn draw_sync_realtime_bar(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SLATE_800))
        .title(Span::styled("Realtime", Style::default().fg(FOREGROUND)));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(sync) = &app.sync_status else {
        f.render_widget(Paragraph::new("No realtime sync data"), inner);
        return;
    };

    let behind = sync.chain_tip - sync.tip_block;
    let now_rate = sync.rate_realtime.unwrap_or(0.0);
    let ema_rate = sync.rate_ema.unwrap_or(0.0);
    let jitter = rate_jitter(&app.rate_history, 30).unwrap_or(0.0);
    let eta_conf = eta_confidence_label(ema_rate, jitter);
    let bottleneck = sync_bottleneck(sync.db_write_ms, sync.rpc_fetch_ms);

    let stale_secs = app
        .memory_stats
        .as_ref()
        .map(|m| (chrono::Utc::now().timestamp() - m.updated_at).max(0))
        .unwrap_or(0);
    let stale_style = if stale_secs > 30 {
        Style::default().fg(AMBER)
    } else {
        Style::default().fg(TERMINAL_DIM)
    };

    let heartbeat_on = (app.last_refresh.elapsed().as_millis() / 500).is_multiple_of(2);
    let heartbeat = if heartbeat_on { "●" } else { "○" };
    let heartbeat_color = if app.last_refresh.elapsed().as_secs() <= 2 {
        TERMINAL_GREEN
    } else {
        AMBER
    };

    let line = Line::from(vec![
        Span::styled(heartbeat, Style::default().fg(heartbeat_color)),
        Span::styled("  Behind ", Style::default().fg(SLATE_500)),
        Span::styled(format_num(behind), Style::default().fg(FOREGROUND)),
        Span::styled("  |  Rate ", Style::default().fg(SLATE_500)),
        Span::styled(
            format!("{:.0}/{:.0} blk/s", now_rate, ema_rate),
            Style::default().fg(TERMINAL_GREEN),
        ),
        Span::styled("  |  ETA ", Style::default().fg(SLATE_500)),
        Span::styled(
            sync.eta.clone().unwrap_or_else(|| "-".to_string()),
            Style::default().fg(FOREGROUND),
        ),
        Span::styled(" ", Style::default().fg(SLATE_700)),
        Span::styled(
            format!("{} ", eta_conf.0),
            Style::default().fg(Color::Black).bg(eta_conf.1),
        ),
        Span::styled(" | Bottleneck ", Style::default().fg(SLATE_500)),
        Span::styled(
            bottleneck_label(bottleneck),
            Style::default().fg(match bottleneck {
                SyncBottleneck::DbBound => AMBER,
                SyncBottleneck::RpcBound => TERMINAL_DIM,
                SyncBottleneck::Mixed => FOREGROUND,
                SyncBottleneck::Unknown => SLATE_500,
            }),
        ),
        Span::styled(" | stale ", Style::default().fg(SLATE_500)),
        Span::styled(format!("{stale_secs}s"), stale_style),
    ]);
    f.render_widget(Paragraph::new(line), inner);
}

fn draw_overview_kpis(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SLATE_800))
        .title(Span::styled("Overview", Style::default().fg(FOREGROUND)));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(sync) = &app.sync_status else {
        f.render_widget(
            Paragraph::new("No sync status available").alignment(Alignment::Center),
            inner,
        );
        return;
    };

    let behind = sync.chain_tip - sync.tip_block;
    let rate_now = sync
        .rate_realtime
        .map(|v| format!("{v:.0} blk/s"))
        .unwrap_or_else(|| "-".to_string());
    let rate_ema = sync
        .rate_ema
        .map(|v| format!("{v:.0} blk/s"))
        .unwrap_or_else(|| "-".to_string());
    let eta = sync.eta.clone().unwrap_or_else(|| "-".to_string());
    let mode = if !sync.is_syncing {
        "SYNCED".to_string()
    } else if sync.is_bulk_sync {
        "BULK".to_string()
    } else {
        "SYNCING".to_string()
    };
    let deferred_count = [
        sync.address_balances_deferred,
        sync.activities_deferred,
        sync.token_deferred,
        sync.spore_deferred,
        sync.tx_block_map_deferred,
    ]
    .into_iter()
    .filter(|v| *v)
    .count();
    let mode_with_deferred = if deferred_count > 0 {
        format!("{mode} ({deferred_count}D)")
    } else {
        mode
    };

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(inner);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
        ])
        .split(rows[0]);

    draw_kpi_cell(
        f,
        cols[0],
        "Tip",
        &format_num(sync.tip_block),
        TERMINAL_GREEN,
    );
    draw_kpi_cell(f, cols[1], "Behind", &format_num(behind), AMBER);
    draw_kpi_cell(
        f,
        cols[2],
        "Rate",
        &format!("{rate_now} / {rate_ema}"),
        TERMINAL_DIM,
    );
    draw_kpi_cell(f, cols[3], "ETA", &eta, FOREGROUND);
    draw_kpi_cell(f, cols[4], "Mode", &mode_with_deferred, TERMINAL_GREEN);

    let progress_ratio = if sync.chain_tip > 0 {
        (sync.tip_block as f64 / sync.chain_tip as f64).clamp(0.0, 1.0)
    } else {
        1.0
    };
    let progress_line = format!(
        "Progress {} {:.2}% ({}/{})",
        draw_bar(progress_ratio, 24),
        sync.progress,
        format_num(sync.tip_block),
        format_num(sync.chain_tip)
    );
    f.render_widget(
        Paragraph::new(Span::styled(
            progress_line,
            Style::default().fg(TERMINAL_DIM),
        )),
        rows[1],
    );
}

fn draw_kpi_cell(f: &mut Frame, area: Rect, label: &str, value: &str, color: Color) {
    let lines = vec![
        Line::from(Span::styled(label, Style::default().fg(SLATE_500))),
        Line::from(Span::styled(
            value,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )),
    ];
    f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), area);
}

fn draw_chain_info(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SLATE_800))
        .title(Span::styled("Chain Info", Style::default().fg(FOREGROUND)));

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
        .title(Span::styled("Sync Status", Style::default().fg(FOREGROUND)));

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

    right.push(Line::from(vec![
        Span::styled("Source: ", Style::default().fg(SLATE_500)),
        Span::styled(
            if sync.is_direct_db_read { "DB" } else { "RPC" },
            Style::default().fg(TERMINAL_DIM),
        ),
    ]));

    if right.is_empty() {
        right.push(Line::from(Span::styled(
            "No timing data",
            Style::default().fg(SLATE_500),
        )));
    }

    f.render_widget(Paragraph::new(right), cols[2]);
}

fn draw_sync_charts(f: &mut Frame, app: &App, area: Rect) {
    if area.width < 120 {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);
        draw_chart_panel(f, rows[0], "Sync Rate (blk/s)", "blk/s", &app.rate_history);
        draw_chart_panel(f, rows[1], "DB Write (ms)", "ms", &app.db_write_history);
    } else {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);
        draw_chart_panel(f, cols[0], "Sync Rate (blk/s)", "blk/s", &app.rate_history);
        draw_chart_panel(f, cols[1], "DB Write (ms)", "ms", &app.db_write_history);
    }
}

fn draw_chart_panel(f: &mut Frame, area: Rect, title: &str, unit: &str, data: &VecDeque<f64>) {
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

fn draw_sync_diagnostics(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SLATE_800))
        .title(Span::styled(
            "Sync Diagnostics",
            Style::default().fg(FOREGROUND),
        ));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(sync) = &app.sync_status else {
        f.render_widget(Paragraph::new("No diagnostics available"), inner);
        return;
    };

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(52), Constraint::Percentage(48)])
        .split(inner);

    let deferred_count = [
        sync.address_balances_deferred,
        sync.activities_deferred,
        sync.token_deferred,
        sync.spore_deferred,
        sync.tx_block_map_deferred,
    ]
    .into_iter()
    .filter(|v| *v)
    .count();

    let db_write = sync
        .db_write_ms
        .map(|v| format!("{v:.1}ms"))
        .unwrap_or_else(|| "-".to_string());
    let rpc_fetch = sync
        .rpc_fetch_ms
        .map(|v| format!("{v:.1}ms"))
        .unwrap_or_else(|| "-".to_string());
    let rate_jitter_value = rate_jitter(&app.rate_history, 30);
    let rate_jitter_text = rate_jitter_value
        .map(|v| format!("{v:.1} blk/s"))
        .unwrap_or_else(|| "-".to_string());
    let eta_conf = eta_confidence_label(
        sync.rate_ema.unwrap_or(0.0),
        rate_jitter_value.unwrap_or(0.0),
    );
    let dense_panel = diagnostics_dense_panel(app.diagnostics_view_mode, inner.width, inner.height);

    let (left, right) = if let Some(pipeline) = sync.pipeline.as_ref() {
        let (state, state_color) = pipeline_flow_state(
            sync.is_syncing,
            pipeline.fetch_queue_depth,
            pipeline.fetch_queue_capacity,
            pipeline.parse_queue_depth,
            pipeline.parse_queue_capacity,
            pipeline.writer_queue_depth,
            pipeline.writer_queue_capacity,
        );
        let (stage, stage_color) =
            pipeline_bottleneck(pipeline.fetch_ms, pipeline.parse_ms, pipeline.write_ms);
        let bottleneck_delta = match stage {
            "FETCH" => trend_delta(&app.fetch_stage_history, 10),
            "PARSE" => trend_delta(&app.parse_stage_history, 10),
            "WRITE" => trend_delta(&app.write_stage_history, 10),
            _ => None,
        };
        let bottleneck_delta_text = format_delta(bottleneck_delta, "ms/10s");
        let (stability, stability_color) = pipeline_stability_label(&app.write_stage_history);
        let fetch_util = format_util_pct(queue_utilization(
            pipeline.fetch_queue_depth,
            pipeline.fetch_queue_capacity,
        ));
        let parse_util = format_util_pct(queue_utilization(
            pipeline.parse_queue_depth,
            pipeline.parse_queue_capacity,
        ));
        let write_util = format_util_pct(queue_utilization(
            pipeline.writer_queue_depth,
            pipeline.writer_queue_capacity,
        ));

        let spark_width = cols[1].width.saturating_sub(14).clamp(8, 24) as usize;
        let (left, right) = if dense_panel {
            (
                vec![
                    Line::from(vec![
                        Span::styled("State ", Style::default().fg(SLATE_500)),
                        Span::styled(
                            format!("[{}]", state),
                            Style::default()
                                .fg(Color::Black)
                                .bg(state_color)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("  Bottleneck ", Style::default().fg(SLATE_500)),
                        Span::styled(stage, Style::default().fg(stage_color)),
                    ]),
                    pipeline_stage_line(
                        "FETCH",
                        pipeline.fetch_ms,
                        pipeline.fetch_queue_depth,
                        pipeline.fetch_queue_capacity,
                        TERMINAL_DIM,
                    ),
                    pipeline_stage_line(
                        "PARSE",
                        pipeline.parse_ms,
                        pipeline.parse_queue_depth,
                        pipeline.parse_queue_capacity,
                        AMBER,
                    ),
                    pipeline_stage_line(
                        "WRITE",
                        pipeline.write_ms,
                        pipeline.writer_queue_depth,
                        pipeline.writer_queue_capacity,
                        TERMINAL_GREEN,
                    ),
                    Line::from(vec![
                        Span::styled("Wait ", Style::default().fg(SLATE_500)),
                        Span::styled(
                            pipeline
                                .writer_wait_ms
                                .map(|v| format!("{v:.1}ms"))
                                .unwrap_or_else(|| "-".to_string()),
                            Style::default().fg(FOREGROUND),
                        ),
                        Span::styled("  Trend ", Style::default().fg(SLATE_500)),
                        Span::styled(
                            bottleneck_delta_text,
                            Style::default().fg(delta_color(bottleneck_delta)),
                        ),
                    ]),
                ],
                vec![
                    stage_trend_line("F", TERMINAL_DIM, &app.fetch_stage_history, spark_width),
                    stage_trend_line("P", AMBER, &app.parse_stage_history, spark_width),
                    stage_trend_line("W", TERMINAL_GREEN, &app.write_stage_history, spark_width),
                    Line::from(vec![
                        Span::styled("Stability ", Style::default().fg(SLATE_500)),
                        Span::styled(stability, Style::default().fg(stability_color)),
                        Span::styled("  Deferred ", Style::default().fg(SLATE_500)),
                        Span::styled(
                            format!("{deferred_count}"),
                            Style::default().fg(if deferred_count > 0 {
                                AMBER
                            } else {
                                TERMINAL_GREEN
                            }),
                        ),
                    ]),
                    Line::from(vec![
                        Span::styled("I/O ", Style::default().fg(SLATE_500)),
                        Span::styled(
                            format!("RPC {} DB {}", rpc_fetch, db_write),
                            Style::default().fg(FOREGROUND),
                        ),
                        Span::styled("  jitter ", Style::default().fg(SLATE_500)),
                        Span::styled(rate_jitter_text.clone(), Style::default().fg(AMBER)),
                    ]),
                ],
            )
        } else {
            (
                vec![
                    Line::from(vec![
                        Span::styled("State ", Style::default().fg(SLATE_500)),
                        Span::styled(
                            format!("[{}]", state),
                            Style::default()
                                .fg(Color::Black)
                                .bg(state_color)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("  Bottleneck ", Style::default().fg(SLATE_500)),
                        Span::styled(stage, Style::default().fg(stage_color)),
                        Span::styled("  Δ ", Style::default().fg(SLATE_500)),
                        Span::styled(
                            bottleneck_delta_text,
                            Style::default().fg(delta_color(bottleneck_delta)),
                        ),
                    ]),
                    pipeline_stage_line(
                        "FETCH",
                        pipeline.fetch_ms,
                        pipeline.fetch_queue_depth,
                        pipeline.fetch_queue_capacity,
                        TERMINAL_DIM,
                    ),
                    pipeline_stage_line(
                        "PARSE",
                        pipeline.parse_ms,
                        pipeline.parse_queue_depth,
                        pipeline.parse_queue_capacity,
                        AMBER,
                    ),
                    pipeline_stage_line(
                        "WRITE",
                        pipeline.write_ms,
                        pipeline.writer_queue_depth,
                        pipeline.writer_queue_capacity,
                        TERMINAL_GREEN,
                    ),
                    Line::from(vec![
                        Span::styled("Util F/P/W ", Style::default().fg(SLATE_500)),
                        Span::styled(
                            format!("{}/{}/{}", fetch_util, parse_util, write_util),
                            Style::default().fg(FOREGROUND),
                        ),
                        Span::styled("  Wait ", Style::default().fg(SLATE_500)),
                        Span::styled(
                            pipeline
                                .writer_wait_ms
                                .map(|v| format!("{v:.1}ms"))
                                .unwrap_or_else(|| "-".to_string()),
                            Style::default().fg(AMBER),
                        ),
                    ]),
                    Line::from(vec![
                        Span::styled("Source ", Style::default().fg(SLATE_500)),
                        Span::styled(
                            if sync.is_direct_db_read { "DB" } else { "RPC" },
                            Style::default().fg(TERMINAL_DIM),
                        ),
                        Span::styled("  Deferred ", Style::default().fg(SLATE_500)),
                        Span::styled(
                            format!("{deferred_count}"),
                            Style::default().fg(if deferred_count > 0 {
                                AMBER
                            } else {
                                TERMINAL_GREEN
                            }),
                        ),
                    ]),
                ],
                vec![
                    stage_trend_line("F", TERMINAL_DIM, &app.fetch_stage_history, spark_width),
                    stage_trend_line("P", AMBER, &app.parse_stage_history, spark_width),
                    stage_trend_line("W", TERMINAL_GREEN, &app.write_stage_history, spark_width),
                    Line::from(vec![
                        Span::styled("Stability ", Style::default().fg(SLATE_500)),
                        Span::styled(stability, Style::default().fg(stability_color)),
                        Span::styled("  ETA ", Style::default().fg(SLATE_500)),
                        Span::styled(eta_conf.0, Style::default().fg(eta_conf.1)),
                    ]),
                    Line::from(vec![
                        Span::styled("I/O ", Style::default().fg(SLATE_500)),
                        Span::styled(
                            format!("RPC {} DB {}", rpc_fetch, db_write),
                            Style::default().fg(FOREGROUND),
                        ),
                        Span::styled("  jitter ", Style::default().fg(SLATE_500)),
                        Span::styled(rate_jitter_text.clone(), Style::default().fg(AMBER)),
                    ]),
                    Line::from(vec![
                        Span::styled("Rate ", Style::default().fg(SLATE_500)),
                        Span::styled(
                            format!(
                                "{:.0}/{:.0} blk/s",
                                sync.rate_realtime.unwrap_or(0.0),
                                sync.rate_ema.unwrap_or(0.0)
                            ),
                            Style::default().fg(TERMINAL_GREEN),
                        ),
                        Span::styled("  samples ", Style::default().fg(SLATE_500)),
                        Span::styled(
                            format_num_u64(app.write_stage_history.len() as u64),
                            Style::default().fg(FOREGROUND),
                        ),
                    ]),
                ],
            )
        };

        (left, right)
    } else {
        (
            vec![
                Line::from(vec![
                    Span::styled("State ", Style::default().fg(SLATE_500)),
                    Span::styled("N/A", Style::default().fg(SLATE_500)),
                ]),
                Line::from(vec![
                    Span::styled("Pipeline ", Style::default().fg(SLATE_500)),
                    Span::styled("not available", Style::default().fg(SLATE_500)),
                ]),
                Line::from(vec![
                    Span::styled("Samples ", Style::default().fg(SLATE_500)),
                    Span::styled(
                        format_num_u64(app.rate_history.len() as u64),
                        Style::default().fg(FOREGROUND),
                    ),
                ]),
            ],
            vec![
                Line::from(vec![
                    Span::styled("I/O ", Style::default().fg(SLATE_500)),
                    Span::styled(
                        format!("RPC {} DB {}", rpc_fetch, db_write),
                        Style::default().fg(FOREGROUND),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Rate jitter ", Style::default().fg(SLATE_500)),
                    Span::styled(rate_jitter_text, Style::default().fg(AMBER)),
                ]),
            ],
        )
    };

    f.render_widget(Paragraph::new(left), cols[0]);
    f.render_widget(Paragraph::new(right), cols[1]);
}

fn draw_sync_events(f: &mut Frame, app: &App, area: Rect) {
    let title = if app.sync_event_scroll > 0 {
        format!("Sync Events [j/k g/G] (scroll +{})", app.sync_event_scroll)
    } else {
        "Sync Events [j/k g/G]".to_string()
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

    if app.sync_event_entries.is_empty() {
        f.render_widget(Paragraph::new("No sync events"), inner);
        return;
    }

    let visible = inner.height as usize;
    let total = app.sync_event_entries.len();
    let base_start = total.saturating_sub(visible);
    let start = base_start.saturating_sub(app.sync_event_scroll);
    let end = (start + visible).min(total);

    let lines: Vec<Line> = app
        .sync_event_entries
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

fn draw_storage_health(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SLATE_800))
        .title(Span::styled(
            "RocksDB Health",
            Style::default().fg(FOREGROUND),
        ));

    let Some(mem) = &app.memory_stats else {
        let msg = Paragraph::new("No RocksDB health data").block(block);
        f.render_widget(msg, area);
        return;
    };

    let inner = block.inner(area);
    f.render_widget(block, area);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(inner);

    let l0_state = if mem.l0_files_max >= 16 {
        ("HOT", ERROR_RED)
    } else if mem.l0_files_max >= 10 {
        ("WARN", AMBER)
    } else {
        ("OK", TERMINAL_GREEN)
    };
    let health_state = if mem.l0_files_max >= 16
        || mem.compaction_pending_bytes >= 64 * 1024 * 1024 * 1024
    {
        ("HOT", ERROR_RED)
    } else if mem.l0_files_max >= 10 || mem.compaction_pending_bytes >= 16 * 1024 * 1024 * 1024 {
        ("WARN", AMBER)
    } else {
        ("OK", TERMINAL_GREEN)
    };

    let left = vec![
        Line::from(vec![
            Span::styled("Health: ", Style::default().fg(SLATE_500)),
            Span::styled(
                format!(" {} ", health_state.0),
                Style::default()
                    .fg(Color::Black)
                    .bg(health_state.1)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Compaction Pending: ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_bytes(mem.compaction_pending_bytes),
                Style::default().fg(FOREGROUND),
            ),
        ]),
        Line::from(vec![
            Span::styled("Run/Immutable: ", Style::default().fg(SLATE_500)),
            Span::styled(
                format!(
                    "{}/{}",
                    format_num_u64(mem.num_running_compactions),
                    format_num_u64(mem.immutable_memtables)
                ),
                Style::default().fg(TERMINAL_DIM),
            ),
            Span::styled("  (comp/imm)", Style::default().fg(SLATE_500)),
        ]),
        Line::from(vec![
            Span::styled("SST Files Size: ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_bytes(mem.sst_files_size),
                Style::default().fg(FOREGROUND),
            ),
        ]),
        Line::from(vec![
            Span::styled("L0 Total/Max: ", Style::default().fg(SLATE_500)),
            Span::styled(
                format!("{}/{}", mem.l0_files_count, mem.l0_files_max),
                Style::default().fg(l0_state.1),
            ),
            Span::styled("  ", Style::default().fg(SLATE_700)),
            Span::styled(format!("[{}]", l0_state.0), Style::default().fg(l0_state.1)),
        ]),
        Line::from(vec![
            Span::styled("Worst CF: ", Style::default().fg(SLATE_500)),
            Span::styled(
                if mem.l0_worst_cf.is_empty() {
                    "-"
                } else {
                    mem.l0_worst_cf.as_str()
                },
                Style::default().fg(AMBER),
            ),
        ]),
    ];
    f.render_widget(Paragraph::new(left), cols[0]);

    let mut right_lines = vec![Line::from(Span::styled(
        "Top CF Sizes",
        Style::default().fg(TERMINAL_GREEN),
    ))];

    if mem.top_cf_sizes.is_empty() {
        right_lines.push(Line::from(Span::styled(
            "- no data",
            Style::default().fg(SLATE_500),
        )));
    } else {
        let max_size = mem
            .top_cf_sizes
            .iter()
            .take(3)
            .map(|(_, size)| *size)
            .max()
            .unwrap_or(1);
        for (idx, (name, size)) in mem.top_cf_sizes.iter().take(3).enumerate() {
            let ratio = if max_size == 0 {
                0.0
            } else {
                *size as f64 / max_size as f64
            };
            let bar = draw_bar(ratio, 8);
            right_lines.push(Line::from(vec![
                Span::styled(format!("{}. ", idx + 1), Style::default().fg(SLATE_500)),
                Span::styled(name, Style::default().fg(FOREGROUND)),
                Span::styled(" ", Style::default().fg(SLATE_700)),
                Span::styled(format_bytes(*size), Style::default().fg(AMBER)),
                Span::styled(" ", Style::default().fg(SLATE_700)),
                Span::styled(bar, Style::default().fg(TERMINAL_DIM)),
            ]));
        }
    }
    f.render_widget(Paragraph::new(right_lines), cols[1]);
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
        f.render_widget(Paragraph::new("No events"), inner);
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
        Span::styled("h/l Tab/s", Style::default().fg(TERMINAL_GREEN)),
        Span::styled(" switch-tab  ", Style::default().fg(SLATE_500)),
        Span::styled("c", Style::default().fg(TERMINAL_GREEN)),
        Span::styled(" compact  ", Style::default().fg(SLATE_500)),
        Span::styled("v", Style::default().fg(TERMINAL_GREEN)),
        Span::styled(" diag-view  ", Style::default().fg(SLATE_500)),
        Span::styled("?", Style::default().fg(TERMINAL_GREEN)),
        Span::styled(" help  ", Style::default().fg(SLATE_500)),
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

    f.render_widget(Paragraph::new(lines).alignment(Alignment::Left), inner);
}

fn push_history_sample(history: &mut VecDeque<f64>, sample: f64) {
    if history.len() >= RATE_HISTORY_SIZE {
        history.pop_front();
    }
    history.push_back(sample);
}

fn diagnostics_view_mode_label(mode: DiagnosticsViewMode) -> &'static str {
    match mode {
        DiagnosticsViewMode::Auto => "Auto",
        DiagnosticsViewMode::Compact => "Compact",
        DiagnosticsViewMode::Detail => "Detail",
    }
}

fn diagnostics_view_mode_color(mode: DiagnosticsViewMode) -> Color {
    match mode {
        DiagnosticsViewMode::Auto => SLATE_500,
        DiagnosticsViewMode::Compact => AMBER,
        DiagnosticsViewMode::Detail => TERMINAL_GREEN,
    }
}

fn diagnostics_dense_panel(mode: DiagnosticsViewMode, width: u16, height: u16) -> bool {
    match mode {
        DiagnosticsViewMode::Compact => true,
        DiagnosticsViewMode::Detail => false,
        DiagnosticsViewMode::Auto => width < 145 || height < 7,
    }
}

fn rate_jitter(history: &VecDeque<f64>, sample_window: usize) -> Option<f64> {
    if history.is_empty() {
        return None;
    }

    let mut values: Vec<f64> = history
        .iter()
        .rev()
        .take(sample_window)
        .copied()
        .filter(|v| *v > 0.0)
        .collect();

    if values.len() < 2 {
        return None;
    }

    values.reverse();
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let variance = values
        .iter()
        .map(|v| {
            let d = *v - mean;
            d * d
        })
        .sum::<f64>()
        / values.len() as f64;

    Some(variance.sqrt())
}

fn sync_bottleneck(db_write_ms: Option<f64>, rpc_fetch_ms: Option<f64>) -> SyncBottleneck {
    match (db_write_ms, rpc_fetch_ms) {
        (Some(db), Some(rpc)) if db > 0.0 && rpc > 0.0 => {
            if db > rpc * 1.2 {
                SyncBottleneck::DbBound
            } else if rpc > db * 1.2 {
                SyncBottleneck::RpcBound
            } else {
                SyncBottleneck::Mixed
            }
        }
        (Some(db), None) if db > 0.0 => SyncBottleneck::DbBound,
        (None, Some(rpc)) if rpc > 0.0 => SyncBottleneck::RpcBound,
        _ => SyncBottleneck::Unknown,
    }
}

fn bottleneck_label(bottleneck: SyncBottleneck) -> &'static str {
    match bottleneck {
        SyncBottleneck::DbBound => "DB-bound",
        SyncBottleneck::RpcBound => "RPC-bound",
        SyncBottleneck::Mixed => "mixed",
        SyncBottleneck::Unknown => "unknown",
    }
}

fn pipeline_stage_line(
    stage: &'static str,
    stage_ms: Option<f64>,
    queue_depth: Option<u64>,
    queue_capacity: Option<u64>,
    stage_color: Color,
) -> Line<'static> {
    let ms_text = stage_ms
        .map(|v| format!("{v:.1}ms"))
        .unwrap_or_else(|| "-".to_string());
    let queue_text = match (queue_depth, queue_capacity) {
        (Some(depth), Some(capacity)) if capacity > 0 => format!("{depth}/{capacity}"),
        (Some(depth), None) => depth.to_string(),
        _ => "-".to_string(),
    };
    let pressure_bar = queue_utilization(queue_depth, queue_capacity)
        .map(|u| draw_bar(u, 8))
        .unwrap_or_else(|| "[--------]".to_string());

    Line::from(vec![
        Span::styled(format!("{stage:<5}"), Style::default().fg(stage_color)),
        Span::styled(" ", Style::default().fg(SLATE_700)),
        Span::styled(ms_text, Style::default().fg(FOREGROUND)),
        Span::styled("  q ", Style::default().fg(SLATE_500)),
        Span::styled(queue_text, Style::default().fg(TERMINAL_DIM)),
        Span::styled(" ", Style::default().fg(SLATE_700)),
        Span::styled(pressure_bar, Style::default().fg(stage_color)),
    ])
}

fn queue_utilization(queue_depth: Option<u64>, queue_capacity: Option<u64>) -> Option<f64> {
    match (queue_depth, queue_capacity) {
        (Some(depth), Some(capacity)) if capacity > 0 => {
            Some((depth as f64 / capacity as f64).clamp(0.0, 1.0))
        }
        _ => None,
    }
}

fn format_util_pct(util: Option<f64>) -> String {
    util.map(|u| format!("{:.0}%", u * 100.0))
        .unwrap_or_else(|| "-".to_string())
}

fn pipeline_flow_state(
    is_syncing: bool,
    fetch_depth: Option<u64>,
    fetch_capacity: Option<u64>,
    parse_depth: Option<u64>,
    parse_capacity: Option<u64>,
    writer_depth: Option<u64>,
    writer_capacity: Option<u64>,
) -> (&'static str, Color) {
    if !is_syncing {
        return ("IDLE", SLATE_500);
    }

    let max_util = [
        queue_utilization(fetch_depth, fetch_capacity),
        queue_utilization(parse_depth, parse_capacity),
        queue_utilization(writer_depth, writer_capacity),
    ]
    .into_iter()
    .flatten()
    .fold(0.0, f64::max);

    if max_util >= 0.9 {
        ("STALL", ERROR_RED)
    } else if max_util >= 0.75 {
        ("BACKPRESSURE", AMBER)
    } else {
        ("FLOW", TERMINAL_GREEN)
    }
}

fn pipeline_bottleneck(
    fetch_ms: Option<f64>,
    parse_ms: Option<f64>,
    write_ms: Option<f64>,
) -> (&'static str, Color) {
    let mut best: Option<(&'static str, f64, Color)> = None;
    for (name, value, color) in [
        ("FETCH", fetch_ms, TERMINAL_DIM),
        ("PARSE", parse_ms, AMBER),
        ("WRITE", write_ms, TERMINAL_GREEN),
    ] {
        if let Some(ms) = value {
            match best {
                Some((_, best_ms, _)) if ms <= best_ms => {}
                _ => best = Some((name, ms, color)),
            }
        }
    }

    if let Some((name, _, color)) = best {
        (name, color)
    } else {
        ("N/A", SLATE_500)
    }
}

fn sparkline(history: &VecDeque<f64>, width: usize) -> String {
    const CHARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

    if width == 0 {
        return String::new();
    }

    let mut values: Vec<f64> = history.iter().rev().take(width).copied().collect();
    if values.is_empty() {
        return "-".to_string();
    }
    values.reverse();

    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);

    if (max - min).abs() < f64::EPSILON {
        return CHARS[0].to_string().repeat(values.len());
    }

    values
        .into_iter()
        .map(|v| {
            let idx = (((v - min) / (max - min)) * (CHARS.len() - 1) as f64).round() as usize;
            CHARS[idx.min(CHARS.len() - 1)]
        })
        .collect()
}

fn trend_delta(history: &VecDeque<f64>, window: usize) -> Option<f64> {
    if window == 0 {
        return None;
    }

    let mut values: Vec<f64> = history.iter().rev().take(window * 2).copied().collect();
    if values.len() < 4 {
        return None;
    }
    values.reverse();
    let split = values.len() / 2;
    if split == 0 || split >= values.len() {
        return None;
    }

    let (prev, latest) = values.split_at(split);
    let prev_vals: Vec<f64> = prev.iter().copied().filter(|v| *v > 0.0).collect();
    let latest_vals: Vec<f64> = latest.iter().copied().filter(|v| *v > 0.0).collect();
    if prev_vals.is_empty() || latest_vals.is_empty() {
        return None;
    }

    let prev_avg = prev_vals.iter().sum::<f64>() / prev_vals.len() as f64;
    let latest_avg = latest_vals.iter().sum::<f64>() / latest_vals.len() as f64;
    Some(latest_avg - prev_avg)
}

fn format_delta(delta: Option<f64>, unit: &str) -> String {
    match delta {
        Some(v) if v >= 0.0 => format!("+{v:.1}{unit}"),
        Some(v) => format!("{v:.1}{unit}"),
        None => "-".to_string(),
    }
}

fn delta_color(delta: Option<f64>) -> Color {
    match delta {
        Some(v) if v > 5.0 => ERROR_RED,
        Some(v) if v < -5.0 => TERMINAL_GREEN,
        Some(_) => AMBER,
        None => SLATE_500,
    }
}

fn stage_trend_line(
    label: &'static str,
    color: Color,
    history: &VecDeque<f64>,
    spark_width: usize,
) -> Line<'static> {
    let delta = trend_delta(history, 10);
    Line::from(vec![
        Span::styled(label, Style::default().fg(color)),
        Span::styled(" ", Style::default().fg(SLATE_700)),
        Span::styled(sparkline(history, spark_width), Style::default().fg(color)),
        Span::styled(" ", Style::default().fg(SLATE_700)),
        Span::styled(
            format_delta(delta, "ms"),
            Style::default().fg(delta_color(delta)),
        ),
    ])
}

fn pipeline_stability_label(history: &VecDeque<f64>) -> (&'static str, Color) {
    let jitter = rate_jitter(history, 30);
    let mean = history
        .iter()
        .rev()
        .take(30)
        .copied()
        .filter(|v| *v > 0.0)
        .collect::<Vec<f64>>();
    if mean.is_empty() {
        return ("N/A", SLATE_500);
    }
    let mean = mean.iter().sum::<f64>() / mean.len() as f64;
    if mean <= 0.0 {
        return ("N/A", SLATE_500);
    }

    let ratio = jitter.unwrap_or(0.0) / mean;
    if ratio < 0.2 {
        ("HIGH", TERMINAL_GREEN)
    } else if ratio < 0.4 {
        ("MED", AMBER)
    } else {
        ("LOW", ERROR_RED)
    }
}

fn eta_confidence_label(ema_rate: f64, jitter: f64) -> (&'static str, Color) {
    if ema_rate <= 0.0 {
        return ("ETA ?", SLATE_500);
    }

    let ratio = (jitter / ema_rate).max(0.0);
    if ratio < 0.15 {
        ("ETA HIGH", TERMINAL_GREEN)
    } else if ratio < 0.35 {
        ("ETA MED", AMBER)
    } else {
        ("ETA LOW", ERROR_RED)
    }
}

fn detect_layout_density(app: &App, area: Rect) -> LayoutDensity {
    if app.force_compact_layout {
        return LayoutDensity::Compact;
    }
    if area.width >= 165 && area.height >= 34 {
        LayoutDensity::Wide
    } else if area.width < 130 || area.height < 28 {
        LayoutDensity::Compact
    } else {
        LayoutDensity::Standard
    }
}

fn draw_help_popup(f: &mut Frame) {
    let outer = f.area();
    let popup_area = centered_rect(74, 62, outer);
    f.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SLATE_800))
        .title(Span::styled("Help", Style::default().fg(FOREGROUND)));
    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);

    let lines = vec![
        Line::from(Span::styled(
            "Navigation",
            Style::default()
                .fg(TERMINAL_GREEN)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("  q        Quit"),
        Line::from("  Tab/s/l  Next tab"),
        Line::from("  h        Previous tab"),
        Line::from("  c        Toggle compact layout override"),
        Line::from("  v        Cycle diagnostics view (Auto/Compact/Detail)"),
        Line::from("  R        Force refresh"),
        Line::from(""),
        Line::from(Span::styled(
            "Events",
            Style::default()
                .fg(TERMINAL_GREEN)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("  j / Down  Scroll older"),
        Line::from("  k / Up    Scroll newer"),
        Line::from("  g / Home  Jump top"),
        Line::from("  G / End   Jump bottom"),
        Line::from(""),
        Line::from(Span::styled(
            "Help",
            Style::default()
                .fg(TERMINAL_GREEN)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("  ? / Esc / Enter  Close this panel"),
    ];
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn draw_bar(ratio: f64, width: usize) -> String {
    let clamped = ratio.clamp(0.0, 1.0);
    let filled = (clamped * width as f64).round() as usize;
    format!(
        "[{}{}]",
        "█".repeat(filled.min(width)),
        "░".repeat(width.saturating_sub(filled.min(width)))
    )
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
    use super::{
        diagnostics_dense_panel, eta_confidence_label, format_num, format_num_commas,
        pipeline_bottleneck, pipeline_flow_state, rate_jitter, sparkline, sync_bottleneck,
        trend_delta, Color, DiagnosticsViewMode, SyncBottleneck,
    };
    use std::collections::VecDeque;

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

    #[test]
    fn test_rate_jitter() {
        let mut history = VecDeque::new();
        history.push_back(100.0);
        history.push_back(120.0);
        history.push_back(80.0);
        let jitter = rate_jitter(&history, 10).unwrap();
        assert!(jitter > 0.0);
    }

    #[test]
    fn test_sync_bottleneck_detection() {
        assert_eq!(
            sync_bottleneck(Some(20.0), Some(5.0)),
            SyncBottleneck::DbBound
        );
        assert_eq!(
            sync_bottleneck(Some(5.0), Some(20.0)),
            SyncBottleneck::RpcBound
        );
        assert_eq!(
            sync_bottleneck(Some(10.0), Some(11.0)),
            SyncBottleneck::Mixed
        );
        assert_eq!(sync_bottleneck(None, None), SyncBottleneck::Unknown);
    }

    #[test]
    fn test_eta_confidence_label() {
        assert_eq!(
            eta_confidence_label(0.0, 0.0),
            ("ETA ?", Color::Rgb(160, 174, 192))
        );
        assert_eq!(
            eta_confidence_label(100.0, 5.0),
            ("ETA HIGH", Color::Rgb(0, 255, 65))
        );
        assert_eq!(
            eta_confidence_label(100.0, 25.0),
            ("ETA MED", Color::Rgb(255, 176, 0))
        );
        assert_eq!(
            eta_confidence_label(100.0, 60.0),
            ("ETA LOW", Color::Rgb(239, 68, 68))
        );
    }

    #[test]
    fn test_pipeline_bottleneck_detection() {
        assert_eq!(
            pipeline_bottleneck(Some(10.0), Some(20.0), Some(30.0)),
            ("WRITE", Color::Rgb(0, 255, 65))
        );
        assert_eq!(
            pipeline_bottleneck(Some(40.0), Some(20.0), Some(30.0)),
            ("FETCH", Color::Rgb(0, 204, 51))
        );
        assert_eq!(
            pipeline_bottleneck(None, Some(25.0), Some(10.0)),
            ("PARSE", Color::Rgb(255, 176, 0))
        );
        assert_eq!(
            pipeline_bottleneck(None, None, None),
            ("N/A", Color::Rgb(160, 174, 192))
        );
    }

    #[test]
    fn test_pipeline_flow_state() {
        assert_eq!(
            pipeline_flow_state(
                false,
                Some(0),
                Some(16),
                Some(0),
                Some(16),
                Some(0),
                Some(16)
            ),
            ("IDLE", Color::Rgb(160, 174, 192))
        );
        assert_eq!(
            pipeline_flow_state(
                true,
                Some(2),
                Some(16),
                Some(3),
                Some(16),
                Some(4),
                Some(16)
            ),
            ("FLOW", Color::Rgb(0, 255, 65))
        );
        assert_eq!(
            pipeline_flow_state(
                true,
                Some(13),
                Some(16),
                Some(12),
                Some(16),
                Some(14),
                Some(16)
            ),
            ("BACKPRESSURE", Color::Rgb(255, 176, 0))
        );
        assert_eq!(
            pipeline_flow_state(
                true,
                Some(15),
                Some(16),
                Some(10),
                Some(16),
                Some(12),
                Some(16)
            ),
            ("STALL", Color::Rgb(239, 68, 68))
        );
    }

    #[test]
    fn test_trend_delta() {
        let mut rising = VecDeque::new();
        for v in [10.0, 12.0, 14.0, 18.0, 22.0, 26.0] {
            rising.push_back(v);
        }
        let delta = trend_delta(&rising, 3).expect("delta should be present");
        assert!(delta > 0.0);

        let mut falling = VecDeque::new();
        for v in [30.0, 24.0, 20.0, 18.0, 15.0, 10.0] {
            falling.push_back(v);
        }
        let delta = trend_delta(&falling, 3).expect("delta should be present");
        assert!(delta < 0.0);
    }

    #[test]
    fn test_sparkline_output() {
        let mut history = VecDeque::new();
        for v in [10.0, 20.0, 15.0, 35.0, 40.0, 25.0] {
            history.push_back(v);
        }
        let s = sparkline(&history, 6);
        assert_eq!(s.chars().count(), 6);
    }

    #[test]
    fn test_diagnostics_dense_panel_mode() {
        assert!(diagnostics_dense_panel(
            DiagnosticsViewMode::Compact,
            200,
            20
        ));
        assert!(!diagnostics_dense_panel(DiagnosticsViewMode::Detail, 90, 5));
        assert!(diagnostics_dense_panel(DiagnosticsViewMode::Auto, 120, 8));
        assert!(diagnostics_dense_panel(DiagnosticsViewMode::Auto, 180, 6));
        assert!(!diagnostics_dense_panel(DiagnosticsViewMode::Auto, 180, 10));
    }
}
