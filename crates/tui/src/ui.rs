use chrono::{DateTime, Local};
use ckbadger_common::{AdaptiveLastAdjustmentData, DbMemoryStatsData, MemoryStatsData};
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
use crate::db::{
    ApiServiceInfo, ChainInfoData, RedisServiceInfo, RuntimeDiagData, SyncStatusRow, TuiDb,
};

const RATE_HISTORY_SIZE: usize = 3600;
const LOG_HISTORY_SIZE: usize = 200;
const RATE_DROP_RATIO_THRESHOLD: f64 = 0.65;

const TERMINAL_GREEN: Color = Color::Rgb(0, 255, 65);
const TERMINAL_DIM: Color = Color::Rgb(0, 204, 51);
const AMBER: Color = Color::Rgb(255, 176, 0);
const CYAN: Color = Color::Rgb(56, 189, 248);
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompactSyncLayout {
    DiagnosticsOnly,
    ChartsAndDiagnostics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompactOverviewLayout {
    MemoryOnly,
    MemoryAndStorage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncEventFilter {
    All,
    WarnOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncEventKeywordFilter {
    Any,
    Backpressure,
    Reset,
    Stall,
    Stale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LaneLagState {
    Off,
    Ok,
    Warn,
    Hot,
}

pub struct App {
    db: TuiDb,
    sync_status: Option<SyncStatusRow>,
    memory_stats: Option<MemoryStatsData>,
    chain_info: Option<ChainInfoData>,
    redis_service: RedisServiceInfo,
    api_service: ApiServiceInfo,
    runtime_diag: Option<RuntimeDiagData>,
    last_refresh: Instant,
    last_sample: Instant,
    status_message: Option<(String, Instant)>,
    rate_history: VecDeque<f64>,
    tx_rate_history: VecDeque<f64>,
    db_write_history: VecDeque<f64>,
    lane_lag_history: VecDeque<f64>,
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
    prev_pipeline_reset_epoch: Option<u64>,
    prev_bottleneck: Option<SyncBottleneck>,
    prev_adaptive_last_reason: Option<String>,
    prev_adaptive_adjustment_seq: Option<u64>,
    prev_lane_backpressure: Option<bool>,
    last_rate_drop_alert: Option<Instant>,
    last_tx_rate_drop_alert: Option<Instant>,
    stale_warning_active: bool,
    help_visible: bool,
    force_compact_layout: bool,
    diagnostics_view_mode: DiagnosticsViewMode,
    sync_focus_mode: bool,
    sync_event_filter: SyncEventFilter,
    sync_event_keyword_filter: SyncEventKeywordFilter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncBottleneck {
    WriteBound,
    FetchBound,
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
            redis_service: RedisServiceInfo::default(),
            api_service: ApiServiceInfo::default(),
            runtime_diag: None,
            last_refresh: Instant::now(),
            last_sample: Instant::now(),
            status_message: None,
            rate_history: VecDeque::with_capacity(RATE_HISTORY_SIZE),
            tx_rate_history: VecDeque::with_capacity(RATE_HISTORY_SIZE),
            db_write_history: VecDeque::with_capacity(RATE_HISTORY_SIZE),
            lane_lag_history: VecDeque::with_capacity(RATE_HISTORY_SIZE),
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
            prev_pipeline_reset_epoch: None,
            prev_bottleneck: None,
            prev_adaptive_last_reason: None,
            prev_adaptive_adjustment_seq: None,
            prev_lane_backpressure: None,
            last_rate_drop_alert: None,
            last_tx_rate_drop_alert: None,
            stale_warning_active: false,
            help_visible: false,
            force_compact_layout: false,
            diagnostics_view_mode: DiagnosticsViewMode::Auto,
            sync_focus_mode: false,
            sync_event_filter: SyncEventFilter::All,
            sync_event_keyword_filter: SyncEventKeywordFilter::Any,
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

    pub fn toggle_sync_focus_mode(&mut self) {
        self.sync_focus_mode = !self.sync_focus_mode;
        let msg = if self.sync_focus_mode {
            "Sync focus mode enabled".to_string()
        } else {
            "Sync focus mode disabled".to_string()
        };
        self.status_message = Some((msg, Instant::now()));
    }

    pub fn toggle_sync_event_filter(&mut self) {
        self.sync_event_filter = match self.sync_event_filter {
            SyncEventFilter::All => SyncEventFilter::WarnOnly,
            SyncEventFilter::WarnOnly => SyncEventFilter::All,
        };
        self.status_message = Some((
            format!(
                "Sync events filter: {}",
                sync_event_filter_label(self.sync_event_filter)
            ),
            Instant::now(),
        ));
    }

    pub fn cycle_sync_event_keyword_filter(&mut self) {
        self.sync_event_keyword_filter = match self.sync_event_keyword_filter {
            SyncEventKeywordFilter::Any => SyncEventKeywordFilter::Backpressure,
            SyncEventKeywordFilter::Backpressure => SyncEventKeywordFilter::Reset,
            SyncEventKeywordFilter::Reset => SyncEventKeywordFilter::Stall,
            SyncEventKeywordFilter::Stall => SyncEventKeywordFilter::Stale,
            SyncEventKeywordFilter::Stale => SyncEventKeywordFilter::Any,
        };
        self.status_message = Some((
            format!(
                "Sync events keyword: {}",
                sync_event_keyword_filter_label(self.sync_event_keyword_filter)
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
                if self.sync_event_scroll < self.sync_event_len_for_scroll().saturating_sub(1) {
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
                self.sync_event_scroll = self.sync_event_len_for_scroll().saturating_sub(1)
            }
        }
    }

    fn sync_event_len_for_scroll(&self) -> usize {
        self.sync_event_entries
            .iter()
            .filter(|entry| {
                sync_event_matches_filter(
                    entry,
                    self.sync_event_filter,
                    self.sync_event_keyword_filter,
                )
            })
            .count()
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
        let (chain_info, api_service) = self.db.get_chain_info_and_api_service_info().await;
        self.chain_info = chain_info;
        self.redis_service = self.db.get_redis_service_info().await;
        self.api_service = api_service;
        self.runtime_diag = self.db.get_runtime_diag();
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
        let block_rate = self
            .sync_status
            .as_ref()
            .and_then(|s| s.rate_realtime)
            .unwrap_or(0.0);
        push_history_sample(&mut self.rate_history, block_rate);
        let tx_rate = self
            .sync_status
            .as_ref()
            .and_then(|s| s.tx_rate_realtime)
            .unwrap_or(0.0);
        push_history_sample(&mut self.tx_rate_history, tx_rate);

        let db_ms = self
            .sync_status
            .as_ref()
            .and_then(|s| s.db_write_ms)
            .unwrap_or(0.0);
        push_history_sample(&mut self.db_write_history, db_ms);
        let lane_lag = self
            .sync_status
            .as_ref()
            .and_then(|s| s.heavy_lane_lag_blocks)
            .map(|v| v as f64)
            .unwrap_or(0.0);
        push_history_sample(&mut self.lane_lag_history, lane_lag);

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

        let mut block_rate_alerted = false;
        if self.rate_history.len() >= 2 {
            let prev = self.rate_history[self.rate_history.len() - 2];
            if is_rate_drop(prev, block_rate) {
                let should_alert = self
                    .last_rate_drop_alert
                    .map(|t| t.elapsed().as_secs() >= 30)
                    .unwrap_or(true);
                if should_alert {
                    self.push_sync_event_and_log(
                        format!(
                            "sync rate drop detected: {:.0} -> {:.0} blk/s",
                            prev, block_rate
                        ),
                        LogLevel::Warning,
                    );
                    self.last_rate_drop_alert = Some(Instant::now());
                    block_rate_alerted = true;
                }
            }
        }

        if self.tx_rate_history.len() >= 2 {
            let prev = self.tx_rate_history[self.tx_rate_history.len() - 2];
            if !block_rate_alerted && is_rate_drop(prev, tx_rate) {
                let should_alert = self
                    .last_tx_rate_drop_alert
                    .map(|t| t.elapsed().as_secs() >= 30)
                    .unwrap_or(true);
                if should_alert {
                    self.push_sync_event_and_log(
                        format!(
                            "sync tx rate drop detected: {:.0} -> {:.0} tx/s",
                            prev, tx_rate
                        ),
                        LogLevel::Warning,
                    );
                    self.last_tx_rate_drop_alert = Some(Instant::now());
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
        let pipeline_reset_epoch = sync.pipeline_reset_epoch;
        let pipeline_reset_reason = sync
            .pipeline_reset_reason
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let bottleneck = sync_bottleneck(sync.db_write_ms, sync.rpc_fetch_ms);
        let adaptive_last_reason = sync.adaptive_last_reason.clone();
        let adaptive_adjustment_seq = sync.adaptive_adjustment_seq;
        let adaptive_last_adjustment = sync.adaptive_last_adjustment.clone();
        let lane_backpressure = sync.heavy_lane_backpressure.unwrap_or(false);

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

        if pipeline_reset_epoch.is_some() && pipeline_reset_epoch != self.prev_pipeline_reset_epoch
        {
            self.push_sync_event_and_log(
                format!(
                    "pipeline reset #{} ({})",
                    pipeline_reset_epoch.unwrap_or(0),
                    pipeline_reset_reason
                ),
                LogLevel::Warning,
            );
        }
        self.prev_pipeline_reset_epoch = pipeline_reset_epoch;

        if let Some(prev) = self.prev_bottleneck {
            if prev != bottleneck && bottleneck != SyncBottleneck::Unknown {
                self.push_sync_event_and_log(
                    format!("bottleneck changed to {}", bottleneck_label(bottleneck)),
                    LogLevel::Info,
                );
            }
        }
        self.prev_bottleneck = Some(bottleneck);

        if adaptive_last_reason != self.prev_adaptive_last_reason {
            if let Some(reason) = adaptive_last_reason.as_deref() {
                self.push_sync_event_and_log(
                    format!("adaptive state changed: {}", reason),
                    LogLevel::Info,
                );
            }
        }
        self.prev_adaptive_last_reason = adaptive_last_reason;

        if adaptive_adjustment_seq.is_some()
            && adaptive_adjustment_seq != self.prev_adaptive_adjustment_seq
        {
            if let Some(seq) = adaptive_adjustment_seq {
                if let Some(adjustment) = adaptive_last_adjustment.as_ref() {
                    self.push_sync_event_and_log(
                        adaptive_adjustment_event_message(seq, adjustment),
                        adaptive_adjustment_event_level(&adjustment.reason),
                    );
                }
            }
        }
        self.prev_adaptive_adjustment_seq = adaptive_adjustment_seq;

        if let Some(prev_backpressure) = self.prev_lane_backpressure {
            if !prev_backpressure && lane_backpressure {
                self.push_sync_event_and_log(
                    "heavy lane backpressure ON".to_string(),
                    LogLevel::Warning,
                );
            } else if prev_backpressure && !lane_backpressure {
                self.push_sync_event_and_log(
                    "heavy lane backpressure cleared".to_string(),
                    LogLevel::Success,
                );
            }
        }
        self.prev_lane_backpressure = Some(lane_backpressure);
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

    let title = Paragraph::new(header_title_line(mode_text, mode_color));
    f.render_widget(title, cols[0]);

    let now = Local::now();
    let stale_secs = app
        .memory_stats
        .as_ref()
        .map(|m| (chrono::Utc::now().timestamp() - m.updated_at).max(0))
        .unwrap_or(0);
    let right = Paragraph::new(header_right_line(
        stale_secs,
        &now.format("%H:%M:%S").to_string(),
    ))
    .alignment(Alignment::Right);
    f.render_widget(right, cols[1]);
}

fn header_title_line(mode_text: &str, mode_color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            "CKBadger",
            Style::default()
                .fg(TERMINAL_GREEN)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Monitor", Style::default().fg(FOREGROUND)),
        Span::styled(" ", Style::default().fg(SLATE_700)),
        Span::styled(
            format!(" {} ", mode_text),
            Style::default().fg(Color::Black).bg(mode_color),
        ),
    ])
}

fn header_right_line(stale_secs: i64, clock_text: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("stale {}s", stale_secs),
            Style::default().fg(if stale_secs > 30 { AMBER } else { TERMINAL_DIM }),
        ),
        Span::styled(" │ ", Style::default().fg(SLATE_700)),
        Span::styled(clock_text.to_string(), Style::default().fg(FOREGROUND)),
    ])
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
        Span::styled("  ", Style::default().fg(SLATE_700)),
        Span::styled(
            if app.sync_focus_mode {
                "[Focus]"
            } else {
                "[Dense]"
            },
            Style::default().fg(if app.sync_focus_mode {
                AMBER
            } else {
                TERMINAL_DIM
            }),
        ),
        Span::styled("  ", Style::default().fg(SLATE_700)),
        Span::styled(
            format!(
                "[Events:{}]",
                sync_event_filter_label(app.sync_event_filter)
            ),
            Style::default().fg(if app.sync_event_filter == SyncEventFilter::WarnOnly {
                AMBER
            } else {
                SLATE_500
            }),
        ),
        Span::styled("  ", Style::default().fg(SLATE_700)),
        Span::styled(
            format!(
                "[Match:{}]",
                sync_event_keyword_filter_label(app.sync_event_keyword_filter)
            ),
            Style::default().fg(
                if app.sync_event_keyword_filter == SyncEventKeywordFilter::Any {
                    SLATE_500
                } else {
                    CYAN
                },
            ),
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
    let log_min_height = overview_log_min_height();
    match detect_layout_density(app, area) {
        LayoutDensity::Compact => match compact_overview_layout(area) {
            CompactOverviewLayout::MemoryOnly => {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(6),
                        Constraint::Length(7),
                        Constraint::Length(8),
                        Constraint::Min(log_min_height),
                    ])
                    .split(area);

                draw_overview_kpis(f, app, chunks[0]);
                draw_chain_info(f, app, chunks[1]);
                draw_memory_stats(f, app, chunks[2]);
                draw_overview_tail(f, app, chunks[3]);
            }
            CompactOverviewLayout::MemoryAndStorage => {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(6),
                        Constraint::Length(7),
                        Constraint::Length(8),
                        Constraint::Length(8),
                        Constraint::Min(log_min_height),
                    ])
                    .split(area);

                draw_overview_kpis(f, app, chunks[0]);
                draw_chain_info(f, app, chunks[1]);
                draw_memory_stats(f, app, chunks[2]);
                draw_storage_health(f, app, chunks[3]);
                draw_overview_tail(f, app, chunks[4]);
            }
        },
        LayoutDensity::Standard => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(6),
                    Constraint::Length(7),
                    Constraint::Length(8),
                    Constraint::Length(8),
                    Constraint::Min(log_min_height),
                ])
                .split(area);

            draw_overview_kpis(f, app, chunks[0]);
            draw_chain_info(f, app, chunks[1]);
            draw_memory_stats(f, app, chunks[2]);
            draw_storage_health(f, app, chunks[3]);
            draw_overview_tail(f, app, chunks[4]);
        }
        LayoutDensity::Wide => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(6),
                    Constraint::Length(7),
                    Constraint::Length(8),
                    Constraint::Length(8),
                    Constraint::Min(log_min_height),
                ])
                .split(area);

            draw_overview_kpis(f, app, chunks[0]);
            draw_chain_info(f, app, chunks[1]);
            draw_memory_stats(f, app, chunks[2]);
            draw_storage_health(f, app, chunks[3]);
            draw_overview_tail(f, app, chunks[4]);
        }
    }
}

fn overview_log_min_height() -> u16 {
    3
}

fn compact_overview_layout(area: Rect) -> CompactOverviewLayout {
    let min_height_for_storage = 6 + 7 + 8 + 8 + overview_log_min_height();
    if area.height >= min_height_for_storage {
        CompactOverviewLayout::MemoryAndStorage
    } else {
        CompactOverviewLayout::MemoryOnly
    }
}

fn overview_services_min_height() -> u16 {
    8
}

fn draw_overview_tail(f: &mut Frame, app: &App, area: Rect) {
    let min_log = overview_log_min_height();
    let min_services = overview_services_min_height();
    if area.height >= min_services + min_log {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(min_services), Constraint::Min(min_log)])
            .split(area);
        draw_service_windows(f, app, rows[0]);
        draw_log(f, app, rows[1]);
    } else {
        draw_log(f, app, area);
    }
}

fn draw_sync_content(f: &mut Frame, app: &App, area: Rect) {
    if app.sync_focus_mode {
        draw_sync_focus_content(f, app, area);
        return;
    }

    match detect_layout_density(app, area) {
        LayoutDensity::Compact => match compact_sync_layout(area) {
            CompactSyncLayout::DiagnosticsOnly => {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(4),
                        Constraint::Length(7),
                        Constraint::Min(7),
                        Constraint::Min(3),
                    ])
                    .split(area);
                draw_sync_realtime_bar(f, app, chunks[0]);
                draw_sync_progress(f, app, chunks[1]);
                draw_sync_diagnostics(f, app, chunks[2]);
                draw_sync_events(f, app, chunks[3]);
            }
            CompactSyncLayout::ChartsAndDiagnostics => {
                let chart_height = if area.width < 120 { 16 } else { 8 };
                let diagnostics_height = if area.width < 120 { 8 } else { 7 };
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(4),
                        Constraint::Length(7),
                        Constraint::Length(chart_height),
                        Constraint::Length(diagnostics_height),
                        Constraint::Min(3),
                    ])
                    .split(area);
                draw_sync_realtime_bar(f, app, chunks[0]);
                draw_sync_progress(f, app, chunks[1]);
                draw_sync_charts(f, app, chunks[2]);
                draw_sync_diagnostics(f, app, chunks[3]);
                draw_sync_events(f, app, chunks[4]);
            }
        },
        LayoutDensity::Standard => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(4),
                    Constraint::Length(7),
                    Constraint::Length(8),
                    Constraint::Length(8),
                    Constraint::Min(3),
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
                    Constraint::Length(9),
                    Constraint::Length(9),
                    Constraint::Min(3),
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

fn draw_sync_focus_content(f: &mut Frame, app: &App, area: Rect) {
    if area.height < 20 {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4),
                Constraint::Min(7),
                Constraint::Min(3),
            ])
            .split(area);
        draw_sync_realtime_bar(f, app, chunks[0]);
        draw_sync_diagnostics(f, app, chunks[1]);
        draw_sync_events(f, app, chunks[2]);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Length(9),
            Constraint::Length(8),
            Constraint::Min(3),
        ])
        .split(area);
    draw_sync_realtime_bar(f, app, chunks[0]);
    draw_sync_alert_strip(f, app, chunks[1]);
    draw_sync_diagnostics(f, app, chunks[2]);
    draw_sync_charts(f, app, chunks[3]);
    draw_sync_events(f, app, chunks[4]);
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
    let ema_rate = sync.rate_ema.unwrap_or(0.0);
    let block_rate_text = format_rate_pair(sync.rate_realtime, sync.rate_ema, "blk/s");
    let tx_rate_text = format_rate_pair(sync.tx_rate_realtime, sync.tx_rate_ema, "tx/s");
    let jitter = rate_jitter(&app.rate_history, 30).unwrap_or(0.0);
    let eta_conf = eta_confidence_label(ema_rate, jitter);
    let bottleneck = sync_bottleneck(sync.db_write_ms, sync.rpc_fetch_ms);
    let (phase_label, phase_color) = startup_phase_label(sync.startup_phase.as_deref());
    let lane_lag_blocks = sync.heavy_lane_lag_blocks.unwrap_or(0);
    let lane_lag_secs = sync.heavy_lane_lag_secs.unwrap_or(0);
    let lane_state = lane_lag_state(
        sync.heavy_lane_enabled,
        sync.heavy_lane_lag_blocks,
        sync.heavy_lane_max_lag_blocks,
        sync.heavy_lane_backpressure,
    );
    let lane_bp = sync.heavy_lane_backpressure.unwrap_or(false);
    let lane_bp_style = if lane_bp {
        Style::default().fg(Color::Black).bg(AMBER)
    } else {
        Style::default().fg(TERMINAL_GREEN)
    };
    let lane_text = if sync.heavy_lane_enabled {
        format!("{}blk {}s", format_num(lane_lag_blocks), lane_lag_secs)
    } else {
        "off".to_string()
    };
    let (lane_state_label, lane_state_color) = lane_lag_state_badge(lane_state);
    let source_text = sync
        .data_source
        .as_deref()
        .map(str::to_string)
        .unwrap_or_else(|| "-".to_string());

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

    let heartbeat_on = heartbeat_is_on(app.last_refresh.elapsed().as_millis());
    let heartbeat = if heartbeat_on { "●" } else { "○" };
    let heartbeat_color = if app.last_refresh.elapsed().as_secs() <= 2 {
        TERMINAL_GREEN
    } else {
        AMBER
    };

    let line = Line::from(vec![
        Span::styled(heartbeat, Style::default().fg(heartbeat_color)),
        Span::styled("  src ", Style::default().fg(SLATE_500)),
        Span::styled(source_text, Style::default().fg(CYAN)),
        Span::styled("  Behind ", Style::default().fg(SLATE_500)),
        Span::styled(format_num(behind), Style::default().fg(FOREGROUND)),
        Span::styled("  |  Rate ", Style::default().fg(SLATE_500)),
        Span::styled(block_rate_text, Style::default().fg(TERMINAL_GREEN)),
        Span::styled("  |  Tx ", Style::default().fg(SLATE_500)),
        Span::styled(tx_rate_text, Style::default().fg(TERMINAL_GREEN)),
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
        Span::styled(" | phase ", Style::default().fg(SLATE_500)),
        Span::styled(phase_label, Style::default().fg(phase_color)),
        Span::styled(" | Bottleneck ", Style::default().fg(SLATE_500)),
        Span::styled(
            bottleneck_label(bottleneck),
            Style::default().fg(match bottleneck {
                SyncBottleneck::WriteBound => AMBER,
                SyncBottleneck::FetchBound => TERMINAL_DIM,
                SyncBottleneck::Mixed => FOREGROUND,
                SyncBottleneck::Unknown => SLATE_500,
            }),
        ),
        Span::styled(" | lane lag ", Style::default().fg(SLATE_500)),
        Span::styled(lane_text, Style::default().fg(lane_state_color)),
        Span::styled(" ", Style::default().fg(SLATE_700)),
        Span::styled(
            if !sync.heavy_lane_enabled {
                "OFF"
            } else if lane_bp {
                "BP"
            } else {
                "FLOW"
            },
            if sync.heavy_lane_enabled {
                lane_bp_style
            } else {
                Style::default().fg(SLATE_500)
            },
        ),
        Span::styled(" ", Style::default().fg(SLATE_700)),
        Span::styled(
            format!("{} ", lane_state_label),
            Style::default().fg(Color::Black).bg(lane_state_color),
        ),
        Span::styled(" | stale ", Style::default().fg(SLATE_500)),
        Span::styled(format!("{stale_secs}s"), stale_style),
    ]);
    f.render_widget(Paragraph::new(line), inner);
}

fn heartbeat_is_on(elapsed_millis: u128) -> bool {
    (elapsed_millis / 500).is_multiple_of(2)
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
    let source_text = sync.data_source.as_deref().unwrap_or("-");
    let last_batch_text = sync
        .last_batch_blocks
        .map(format_num_u64)
        .unwrap_or_else(|| "-".to_string());
    let lane_status = lane_status_text(
        sync.heavy_lane_enabled,
        sync.heavy_lane_lag_blocks,
        sync.heavy_lane_max_lag_blocks,
        sync.heavy_lane_backpressure,
    );
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
            Span::styled("  batch ", Style::default().fg(SLATE_500)),
            Span::styled(last_batch_text, Style::default().fg(FOREGROUND)),
        ]),
        Line::from(vec![
            Span::styled("Blk N/E: ", Style::default().fg(SLATE_500)),
            if let (Some(rt), Some(ema)) = (sync.rate_realtime, sync.rate_ema) {
                Span::styled(
                    format!("{rt:.0}/{ema:.0}"),
                    Style::default()
                        .fg(TERMINAL_GREEN)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled("-", Style::default().fg(SLATE_500))
            },
            Span::styled(" blk/s", Style::default().fg(SLATE_500)),
        ]),
        Line::from(vec![
            Span::styled("Tx  N/E: ", Style::default().fg(SLATE_500)),
            if let (Some(rt), Some(ema)) = (sync.tx_rate_realtime, sync.tx_rate_ema) {
                Span::styled(
                    format!("{rt:.0}/{ema:.0}"),
                    Style::default()
                        .fg(TERMINAL_GREEN)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled("-", Style::default().fg(SLATE_500))
            },
            Span::styled(" tx/s", Style::default().fg(SLATE_500)),
        ]),
        Line::from(vec![
            Span::styled("Lane: ", Style::default().fg(SLATE_500)),
            Span::styled(lane_status, Style::default().fg(CYAN)),
        ]),
    ];
    f.render_widget(Paragraph::new(mid), cols[1]);

    let mut right = sync_timing_lines(
        sync.eta.as_deref(),
        sync.elapsed_time.as_deref(),
        sync.startup_phase.as_deref(),
    );
    right.push(Line::from(vec![
        Span::styled("Source: ", Style::default().fg(SLATE_500)),
        Span::styled(source_text.to_string(), Style::default().fg(CYAN)),
    ]));
    right.push(pipeline_reset_line(
        sync.pipeline_reset_epoch,
        sync.pipeline_reset_reason.as_deref(),
    ));
    f.render_widget(Paragraph::new(right), cols[2]);
}

fn draw_sync_charts(f: &mut Frame, app: &App, area: Rect) {
    if stack_sync_charts(area) {
        let specs = sync_chart_specs(true, false);
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);
        draw_chart_panel(
            f,
            rows[0],
            specs[0].title,
            specs[0].unit,
            sync_chart_data(app, specs[0].kind),
        );
        draw_chart_panel(
            f,
            rows[1],
            specs[1].title,
            specs[1].unit,
            sync_chart_data(app, specs[1].kind),
        );
    } else {
        let include_lane = area.width >= 160
            && app
                .sync_status
                .as_ref()
                .is_some_and(|s| s.heavy_lane_enabled);
        let specs = sync_chart_specs(false, include_lane);
        if include_lane {
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(25),
                    Constraint::Percentage(25),
                    Constraint::Percentage(25),
                    Constraint::Percentage(25),
                ])
                .split(area);
            for (idx, spec) in specs.iter().enumerate() {
                draw_chart_panel(
                    f,
                    cols[idx],
                    spec.title,
                    spec.unit,
                    sync_chart_data(app, spec.kind),
                );
            }
        } else {
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(34),
                    Constraint::Percentage(33),
                    Constraint::Percentage(33),
                ])
                .split(area);
            for (idx, spec) in specs.iter().enumerate() {
                draw_chart_panel(
                    f,
                    cols[idx],
                    spec.title,
                    spec.unit,
                    sync_chart_data(app, spec.kind),
                );
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncChartKind {
    BlockRate,
    TxRate,
    LaneLag,
    WriteLatency,
}

#[derive(Clone, Copy)]
struct SyncChartSpec {
    title: &'static str,
    unit: &'static str,
    kind: SyncChartKind,
}

const STACKED_SYNC_CHART_SPECS: [SyncChartSpec; 2] = [
    SyncChartSpec {
        title: "Sync Rate (blk/s)",
        unit: "blk/s",
        kind: SyncChartKind::BlockRate,
    },
    SyncChartSpec {
        title: "Sync Tx Rate (tx/s)",
        unit: "tx/s",
        kind: SyncChartKind::TxRate,
    },
];

const WIDE_SYNC_CHART_SPECS: [SyncChartSpec; 3] = [
    SyncChartSpec {
        title: "Sync Rate (blk/s)",
        unit: "blk/s",
        kind: SyncChartKind::BlockRate,
    },
    SyncChartSpec {
        title: "Sync Tx Rate (tx/s)",
        unit: "tx/s",
        kind: SyncChartKind::TxRate,
    },
    SyncChartSpec {
        title: "Write Latency (ms)",
        unit: "ms",
        kind: SyncChartKind::WriteLatency,
    },
];

const ULTRA_WIDE_SYNC_CHART_SPECS: [SyncChartSpec; 4] = [
    SyncChartSpec {
        title: "Sync Rate (blk/s)",
        unit: "blk/s",
        kind: SyncChartKind::BlockRate,
    },
    SyncChartSpec {
        title: "Sync Tx Rate (tx/s)",
        unit: "tx/s",
        kind: SyncChartKind::TxRate,
    },
    SyncChartSpec {
        title: "Lane Lag (blk)",
        unit: "blk",
        kind: SyncChartKind::LaneLag,
    },
    SyncChartSpec {
        title: "Write Latency (ms)",
        unit: "ms",
        kind: SyncChartKind::WriteLatency,
    },
];

fn sync_chart_specs(stacked: bool, include_lane: bool) -> &'static [SyncChartSpec] {
    if stacked {
        &STACKED_SYNC_CHART_SPECS
    } else if include_lane {
        &ULTRA_WIDE_SYNC_CHART_SPECS
    } else {
        &WIDE_SYNC_CHART_SPECS
    }
}

fn sync_chart_data(app: &App, kind: SyncChartKind) -> &VecDeque<f64> {
    match kind {
        SyncChartKind::BlockRate => &app.rate_history,
        SyncChartKind::TxRate => &app.tx_rate_history,
        SyncChartKind::LaneLag => &app.lane_lag_history,
        SyncChartKind::WriteLatency => &app.db_write_history,
    }
}

fn draw_chart_panel(f: &mut Frame, area: Rect, title: &str, unit: &str, data: &VecDeque<f64>) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SLATE_800))
        .title(Span::styled(title, Style::default().fg(FOREGROUND)));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.width < 10 || inner.height == 0 {
        return;
    }

    if let Some(message) = chart_height_warning(inner.height) {
        f.render_widget(
            Paragraph::new(message).style(Style::default().fg(SLATE_500)),
            inner,
        );
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

fn stack_sync_charts(area: Rect) -> bool {
    area.width < 120 && area.height >= 10
}

fn chart_height_warning(inner_height: u16) -> Option<&'static str> {
    if inner_height < 3 {
        Some("Insufficient height for chart")
    } else {
        None
    }
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

    let write_ms_text = sync
        .db_write_ms
        .map(|v| format!("{v:.1}ms"))
        .unwrap_or_else(|| "-".to_string());
    let fetch_ms_text = sync
        .rpc_fetch_ms
        .map(|v| format!("{v:.1}ms"))
        .unwrap_or_else(|| "-".to_string());
    let rate_jitter_value = rate_jitter(&app.rate_history, 30);
    let rate_jitter_text = rate_jitter_value
        .map(|v| format!("{v:.1} blk/s"))
        .unwrap_or_else(|| "-".to_string());
    let rate_delta = trend_delta(&app.rate_history, 10);
    let eta_conf = eta_confidence_label(
        sync.rate_ema.unwrap_or(0.0),
        rate_jitter_value.unwrap_or(0.0),
    );
    let dense_panel = diagnostics_dense_panel(app.diagnostics_view_mode, inner.width, inner.height);
    let lane_line = lane_density_line(sync);

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
        let fetch_util_ratio =
            queue_utilization(pipeline.fetch_queue_depth, pipeline.fetch_queue_capacity);
        let parse_util_ratio =
            queue_utilization(pipeline.parse_queue_depth, pipeline.parse_queue_capacity);
        let write_util_ratio =
            queue_utilization(pipeline.writer_queue_depth, pipeline.writer_queue_capacity);
        let fetch_util = format_util_pct(fetch_util_ratio);
        let parse_util = format_util_pct(parse_util_ratio);
        let write_util = format_util_pct(write_util_ratio);
        let adaptive_inflight_batches =
            match (pipeline.fetch_queue_depth, pipeline.parse_queue_depth) {
                (Some(fetch_depth), Some(parse_depth)) => Some(fetch_depth + parse_depth),
                _ => None,
            };

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
                    lane_line.clone(),
                    adaptive_control_line(AdaptiveControlSnapshot {
                        last_batch_blocks: sync.last_batch_blocks,
                        adaptive_inflight_batches,
                        adaptive_target_batch_txs: sync.adaptive_target_batch_txs,
                        adaptive_inflight_limit: sync.adaptive_inflight_limit,
                        adaptive_max_inflight_limit: sync.adaptive_max_inflight_limit,
                        adaptive_min_target_batch_txs: sync.adaptive_min_target_batch_txs,
                        adaptive_cooldown_steps: sync.adaptive_cooldown_steps,
                        adaptive_last_reason: sync.adaptive_last_reason.as_deref(),
                        adaptive_adjustment_seq: sync.adaptive_adjustment_seq,
                        adaptive_last_adjusted_age_secs: sync.adaptive_last_adjusted_age_secs,
                        adaptive_backoff_streak: sync.adaptive_backoff_streak,
                        adaptive_at_floor: sync.adaptive_at_floor,
                        adaptive_last_adjustment: sync.adaptive_last_adjustment.as_ref(),
                    }),
                    adaptive_context_line(
                        AdaptiveControlSnapshot {
                            last_batch_blocks: sync.last_batch_blocks,
                            adaptive_inflight_batches,
                            adaptive_target_batch_txs: sync.adaptive_target_batch_txs,
                            adaptive_inflight_limit: sync.adaptive_inflight_limit,
                            adaptive_max_inflight_limit: sync.adaptive_max_inflight_limit,
                            adaptive_min_target_batch_txs: sync.adaptive_min_target_batch_txs,
                            adaptive_cooldown_steps: sync.adaptive_cooldown_steps,
                            adaptive_last_reason: sync.adaptive_last_reason.as_deref(),
                            adaptive_adjustment_seq: sync.adaptive_adjustment_seq,
                            adaptive_last_adjusted_age_secs: sync.adaptive_last_adjusted_age_secs,
                            adaptive_backoff_streak: sync.adaptive_backoff_streak,
                            adaptive_at_floor: sync.adaptive_at_floor,
                            adaptive_last_adjustment: sync.adaptive_last_adjustment.as_ref(),
                        },
                        fetch_util_ratio,
                        parse_util_ratio,
                        write_util_ratio,
                        pipeline.writer_wait_ms,
                        rate_delta,
                    ),
                    pipeline_reset_line(
                        sync.pipeline_reset_epoch,
                        sync.pipeline_reset_reason.as_deref(),
                    ),
                ],
                dense_right_lines(
                    stage_trend_line("F", TERMINAL_DIM, &app.fetch_stage_history, spark_width),
                    stage_trend_line("P", AMBER, &app.parse_stage_history, spark_width),
                    stage_trend_line("W", TERMINAL_GREEN, &app.write_stage_history, spark_width),
                    lag_trend_line(&app.lane_lag_history, spark_width),
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
                    io_fetch_write_jitter_line(&fetch_ms_text, &write_ms_text, &rate_jitter_text),
                ),
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
                        Span::styled("Deferred ", Style::default().fg(SLATE_500)),
                        Span::styled(
                            format!("{deferred_count}"),
                            Style::default().fg(if deferred_count > 0 {
                                AMBER
                            } else {
                                TERMINAL_GREEN
                            }),
                        ),
                    ]),
                    lane_line.clone(),
                    adaptive_control_line(AdaptiveControlSnapshot {
                        last_batch_blocks: sync.last_batch_blocks,
                        adaptive_inflight_batches,
                        adaptive_target_batch_txs: sync.adaptive_target_batch_txs,
                        adaptive_inflight_limit: sync.adaptive_inflight_limit,
                        adaptive_max_inflight_limit: sync.adaptive_max_inflight_limit,
                        adaptive_min_target_batch_txs: sync.adaptive_min_target_batch_txs,
                        adaptive_cooldown_steps: sync.adaptive_cooldown_steps,
                        adaptive_last_reason: sync.adaptive_last_reason.as_deref(),
                        adaptive_adjustment_seq: sync.adaptive_adjustment_seq,
                        adaptive_last_adjusted_age_secs: sync.adaptive_last_adjusted_age_secs,
                        adaptive_backoff_streak: sync.adaptive_backoff_streak,
                        adaptive_at_floor: sync.adaptive_at_floor,
                        adaptive_last_adjustment: sync.adaptive_last_adjustment.as_ref(),
                    }),
                    adaptive_context_line(
                        AdaptiveControlSnapshot {
                            last_batch_blocks: sync.last_batch_blocks,
                            adaptive_inflight_batches,
                            adaptive_target_batch_txs: sync.adaptive_target_batch_txs,
                            adaptive_inflight_limit: sync.adaptive_inflight_limit,
                            adaptive_max_inflight_limit: sync.adaptive_max_inflight_limit,
                            adaptive_min_target_batch_txs: sync.adaptive_min_target_batch_txs,
                            adaptive_cooldown_steps: sync.adaptive_cooldown_steps,
                            adaptive_last_reason: sync.adaptive_last_reason.as_deref(),
                            adaptive_adjustment_seq: sync.adaptive_adjustment_seq,
                            adaptive_last_adjusted_age_secs: sync.adaptive_last_adjusted_age_secs,
                            adaptive_backoff_streak: sync.adaptive_backoff_streak,
                            adaptive_at_floor: sync.adaptive_at_floor,
                            adaptive_last_adjustment: sync.adaptive_last_adjustment.as_ref(),
                        },
                        fetch_util_ratio,
                        parse_util_ratio,
                        write_util_ratio,
                        pipeline.writer_wait_ms,
                        rate_delta,
                    ),
                    pipeline_reset_line(
                        sync.pipeline_reset_epoch,
                        sync.pipeline_reset_reason.as_deref(),
                    ),
                ],
                detail_right_lines(
                    stage_trend_line("F", TERMINAL_DIM, &app.fetch_stage_history, spark_width),
                    stage_trend_line("P", AMBER, &app.parse_stage_history, spark_width),
                    stage_trend_line("W", TERMINAL_GREEN, &app.write_stage_history, spark_width),
                    lag_trend_line(&app.lane_lag_history, spark_width),
                    Line::from(vec![
                        Span::styled("Stability ", Style::default().fg(SLATE_500)),
                        Span::styled(stability, Style::default().fg(stability_color)),
                        Span::styled("  ETA ", Style::default().fg(SLATE_500)),
                        Span::styled(eta_conf.0, Style::default().fg(eta_conf.1)),
                    ]),
                    Line::from(vec![
                        Span::styled("Rate ", Style::default().fg(SLATE_500)),
                        Span::styled(
                            format!(
                                "{}  {}",
                                format_rate_pair(sync.rate_realtime, sync.rate_ema, "blk/s"),
                                format_rate_pair(sync.tx_rate_realtime, sync.tx_rate_ema, "tx/s"),
                            ),
                            Style::default().fg(TERMINAL_GREEN),
                        ),
                        Span::styled("  samples ", Style::default().fg(SLATE_500)),
                        Span::styled(
                            format_num_u64(app.write_stage_history.len() as u64),
                            Style::default().fg(FOREGROUND),
                        ),
                    ]),
                    io_fetch_write_jitter_line(&fetch_ms_text, &write_ms_text, &rate_jitter_text),
                ),
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
                lane_line,
                adaptive_control_line(AdaptiveControlSnapshot {
                    last_batch_blocks: sync.last_batch_blocks,
                    adaptive_inflight_batches: None,
                    adaptive_target_batch_txs: sync.adaptive_target_batch_txs,
                    adaptive_inflight_limit: sync.adaptive_inflight_limit,
                    adaptive_max_inflight_limit: sync.adaptive_max_inflight_limit,
                    adaptive_min_target_batch_txs: sync.adaptive_min_target_batch_txs,
                    adaptive_cooldown_steps: sync.adaptive_cooldown_steps,
                    adaptive_last_reason: sync.adaptive_last_reason.as_deref(),
                    adaptive_adjustment_seq: sync.adaptive_adjustment_seq,
                    adaptive_last_adjusted_age_secs: sync.adaptive_last_adjusted_age_secs,
                    adaptive_backoff_streak: sync.adaptive_backoff_streak,
                    adaptive_at_floor: sync.adaptive_at_floor,
                    adaptive_last_adjustment: sync.adaptive_last_adjustment.as_ref(),
                }),
                adaptive_context_line(
                    AdaptiveControlSnapshot {
                        last_batch_blocks: sync.last_batch_blocks,
                        adaptive_inflight_batches: None,
                        adaptive_target_batch_txs: sync.adaptive_target_batch_txs,
                        adaptive_inflight_limit: sync.adaptive_inflight_limit,
                        adaptive_max_inflight_limit: sync.adaptive_max_inflight_limit,
                        adaptive_min_target_batch_txs: sync.adaptive_min_target_batch_txs,
                        adaptive_cooldown_steps: sync.adaptive_cooldown_steps,
                        adaptive_last_reason: sync.adaptive_last_reason.as_deref(),
                        adaptive_adjustment_seq: sync.adaptive_adjustment_seq,
                        adaptive_last_adjusted_age_secs: sync.adaptive_last_adjusted_age_secs,
                        adaptive_backoff_streak: sync.adaptive_backoff_streak,
                        adaptive_at_floor: sync.adaptive_at_floor,
                        adaptive_last_adjustment: sync.adaptive_last_adjustment.as_ref(),
                    },
                    None,
                    None,
                    None,
                    None,
                    rate_delta,
                ),
                pipeline_reset_line(
                    sync.pipeline_reset_epoch,
                    sync.pipeline_reset_reason.as_deref(),
                ),
            ],
            vec![
                Line::from(vec![
                    Span::styled("I/O ", Style::default().fg(SLATE_500)),
                    Span::styled(
                        format!("Fetch {} Write {}", fetch_ms_text, write_ms_text),
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

fn io_fetch_write_jitter_line(
    fetch_ms_text: &str,
    write_ms_text: &str,
    rate_jitter_text: &str,
) -> Line<'static> {
    Line::from(vec![
        Span::styled("I/O ", Style::default().fg(SLATE_500)),
        Span::styled(
            format!("Fetch {} Write {}", fetch_ms_text, write_ms_text),
            Style::default().fg(FOREGROUND),
        ),
        Span::styled("  jitter ", Style::default().fg(SLATE_500)),
        Span::styled(rate_jitter_text.to_string(), Style::default().fg(AMBER)),
    ])
}

fn lane_density_line(sync: &SyncStatusRow) -> Line<'static> {
    let state = lane_lag_state(
        sync.heavy_lane_enabled,
        sync.heavy_lane_lag_blocks,
        sync.heavy_lane_max_lag_blocks,
        sync.heavy_lane_backpressure,
    );
    let (state_label, state_color) = lane_lag_state_badge(state);
    if !sync.heavy_lane_enabled {
        return Line::from(vec![
            Span::styled("Lane ", Style::default().fg(SLATE_500)),
            Span::styled("off", Style::default().fg(SLATE_500)),
        ]);
    }
    let core_tip = sync
        .core_lane_tip
        .map(format_num)
        .unwrap_or_else(|| "-".to_string());
    let heavy_tip = sync
        .heavy_lane_tip
        .map(format_num)
        .unwrap_or_else(|| "-".to_string());
    let lag_secs = sync
        .heavy_lane_lag_secs
        .map(|v| format!("{v}s"))
        .unwrap_or_else(|| "-".to_string());
    let max_lag_secs = sync
        .heavy_lane_max_lag_seconds
        .map(|v| format!("{v}s"))
        .unwrap_or_else(|| "-".to_string());
    let lag_text = lane_status_text(
        sync.heavy_lane_enabled,
        sync.heavy_lane_lag_blocks,
        sync.heavy_lane_max_lag_blocks,
        sync.heavy_lane_backpressure,
    );
    Line::from(vec![
        Span::styled("Lane ", Style::default().fg(SLATE_500)),
        Span::styled(
            format!("{core_tip}/{heavy_tip}"),
            Style::default().fg(state_color),
        ),
        Span::styled("  lag ", Style::default().fg(SLATE_500)),
        Span::styled(lag_text, Style::default().fg(state_color)),
        Span::styled("  age ", Style::default().fg(SLATE_500)),
        Span::styled(lag_secs, Style::default().fg(FOREGROUND)),
        Span::styled("  max-age ", Style::default().fg(SLATE_500)),
        Span::styled(max_lag_secs, Style::default().fg(TERMINAL_DIM)),
        Span::styled("  ", Style::default().fg(SLATE_700)),
        Span::styled(
            state_label,
            Style::default().fg(Color::Black).bg(state_color),
        ),
    ])
}

fn format_rate_pair(now: Option<f64>, ema: Option<f64>, unit: &str) -> String {
    let now_text = now
        .map(|v| format!("{v:.0}"))
        .unwrap_or_else(|| "-".to_string());
    let ema_text = ema
        .map(|v| format!("{v:.0}"))
        .unwrap_or_else(|| "-".to_string());
    format!("{now_text}/{ema_text} {unit}")
}

fn lane_status_text(
    heavy_lane_enabled: bool,
    lag_blocks: Option<i64>,
    max_lag_blocks: Option<i64>,
    backpressure: Option<bool>,
) -> String {
    if !heavy_lane_enabled {
        return "off".to_string();
    }
    let lag = lag_blocks
        .map(format_num)
        .unwrap_or_else(|| "-".to_string());
    let max_lag = max_lag_blocks
        .map(format_num)
        .unwrap_or_else(|| "-".to_string());
    let state = if backpressure.unwrap_or(false) {
        "BP"
    } else {
        "FLOW"
    };
    format!("{lag}/{max_lag} {state}")
}

fn lane_lag_state(
    heavy_lane_enabled: bool,
    lag_blocks: Option<i64>,
    max_lag_blocks: Option<i64>,
    backpressure: Option<bool>,
) -> LaneLagState {
    if !heavy_lane_enabled {
        return LaneLagState::Off;
    }
    if backpressure.unwrap_or(false) {
        return LaneLagState::Hot;
    }
    let lag = lag_blocks.unwrap_or(0).max(0) as i128;
    if lag == 0 {
        return LaneLagState::Ok;
    }
    let max_lag = max_lag_blocks.unwrap_or(0).max(0) as i128;
    if max_lag == 0 {
        return LaneLagState::Warn;
    }
    if lag >= max_lag {
        LaneLagState::Hot
    } else if lag * 100 >= max_lag * 80 {
        LaneLagState::Warn
    } else {
        LaneLagState::Ok
    }
}

fn lane_lag_state_badge(state: LaneLagState) -> (&'static str, Color) {
    match state {
        LaneLagState::Off => ("LAG OFF", SLATE_500),
        LaneLagState::Ok => ("LAG OK", CYAN),
        LaneLagState::Warn => ("LAG WARN", AMBER),
        LaneLagState::Hot => ("LAG HOT", ERROR_RED),
    }
}

#[derive(Clone, Copy)]
struct AdaptiveControlSnapshot<'a> {
    last_batch_blocks: Option<u64>,
    adaptive_inflight_batches: Option<u64>,
    adaptive_target_batch_txs: Option<u64>,
    adaptive_inflight_limit: Option<u64>,
    adaptive_max_inflight_limit: Option<u64>,
    adaptive_min_target_batch_txs: Option<u64>,
    adaptive_cooldown_steps: Option<u64>,
    adaptive_last_reason: Option<&'a str>,
    adaptive_adjustment_seq: Option<u64>,
    adaptive_last_adjusted_age_secs: Option<i64>,
    adaptive_backoff_streak: Option<u64>,
    adaptive_at_floor: Option<bool>,
    adaptive_last_adjustment: Option<&'a AdaptiveLastAdjustmentData>,
}

fn adaptive_control_line(snapshot: AdaptiveControlSnapshot<'_>) -> Line<'static> {
    let (state, state_color) = adaptive_state_label(snapshot.adaptive_last_reason);
    let batch_blocks_text = snapshot
        .last_batch_blocks
        .map(format_num_u64)
        .unwrap_or_else(|| "-".to_string());
    let target_text = snapshot
        .adaptive_target_batch_txs
        .map(format_num_u64)
        .unwrap_or_else(|| "-".to_string());
    let min_target_text = snapshot
        .adaptive_min_target_batch_txs
        .map(format_num_u64)
        .unwrap_or_else(|| "-".to_string());
    let inflight_text = match (
        snapshot.adaptive_inflight_batches,
        snapshot.adaptive_inflight_limit,
    ) {
        (Some(current), Some(limit)) => format!("{current}/{limit}"),
        (Some(current), None) => format!("{current}/-"),
        (None, Some(limit)) => format!("-/{limit}"),
        (None, None) => "-".to_string(),
    };
    let cooldown_text = snapshot
        .adaptive_cooldown_steps
        .map(|v| v.to_string())
        .unwrap_or_else(|| "-".to_string());
    let seq_text = snapshot
        .adaptive_adjustment_seq
        .map(format_num_u64)
        .unwrap_or_else(|| "-".to_string());
    let age_text = snapshot
        .adaptive_last_adjusted_age_secs
        .map(|v| format!("{v}s"))
        .unwrap_or_else(|| "-".to_string());
    let state_text = if let Some(streak) = snapshot.adaptive_backoff_streak.filter(|v| *v > 0) {
        format!("{state}x{streak}")
    } else {
        state.to_string()
    };
    Line::from(vec![
        Span::styled("Adaptive ", Style::default().fg(SLATE_500)),
        Span::styled(
            format!(
                "batch {} blk  inflight {}  tx target/min {}/{}  cd {}  chg #{} {}",
                batch_blocks_text,
                inflight_text,
                target_text,
                min_target_text,
                cooldown_text,
                seq_text,
                age_text
            ),
            Style::default().fg(TERMINAL_DIM),
        ),
        Span::styled("  ", Style::default().fg(SLATE_500)),
        Span::styled(
            state_text,
            Style::default()
                .fg(state_color)
                .add_modifier(Modifier::BOLD),
        ),
    ])
}

fn adaptive_context_line(
    snapshot: AdaptiveControlSnapshot<'_>,
    fetch_util: Option<f64>,
    parse_util: Option<f64>,
    write_util: Option<f64>,
    writer_wait_ms: Option<f64>,
    rate_delta: Option<f64>,
) -> Line<'static> {
    let reason = snapshot.adaptive_last_reason.unwrap_or("-");
    let queue_text = format!(
        "{}/{}/{}",
        format_util_pct(fetch_util),
        format_util_pct(parse_util),
        format_util_pct(write_util)
    );
    let wait_text = writer_wait_ms
        .map(|v| format!("{v:.1}ms"))
        .unwrap_or_else(|| "-".to_string());
    let rate_delta_text = format_delta(rate_delta, "blk/s");
    let at_floor = snapshot.adaptive_at_floor.unwrap_or_else(|| {
        snapshot
            .adaptive_target_batch_txs
            .zip(snapshot.adaptive_min_target_batch_txs)
            .is_some_and(|(target, floor)| target <= floor)
    });
    let inflight_limit_text = match (
        snapshot.adaptive_inflight_limit,
        snapshot.adaptive_max_inflight_limit,
    ) {
        (Some(limit), Some(max)) => format!("{limit}/{max}"),
        (Some(limit), None) => format!("{limit}/-"),
        (None, Some(max)) => format!("-/{max}"),
        (None, None) => "-".to_string(),
    };
    let last_adjustment_text = snapshot
        .adaptive_last_adjustment
        .map(|adj| {
            format!(
                "tx {}->{} if {}->{}",
                format_num_u64(adj.previous_target_batch_txs),
                format_num_u64(adj.new_target_batch_txs),
                adj.previous_inflight_limit,
                adj.new_inflight_limit
            )
        })
        .unwrap_or_else(|| "tx - if -".to_string());
    let inflight_util = adaptive_inflight_util(snapshot);
    let (risk_label, risk_color) =
        adaptive_risk_label(at_floor, inflight_util, snapshot.adaptive_backoff_streak);

    Line::from(vec![
        Span::styled("A-ctx ", Style::default().fg(SLATE_500)),
        Span::styled("reason ", Style::default().fg(SLATE_500)),
        Span::styled(reason.to_string(), Style::default().fg(FOREGROUND)),
        Span::styled("  q ", Style::default().fg(SLATE_500)),
        Span::styled(queue_text, Style::default().fg(TERMINAL_DIM)),
        Span::styled("  lim ", Style::default().fg(SLATE_500)),
        Span::styled(inflight_limit_text, Style::default().fg(TERMINAL_DIM)),
        Span::styled("  wait ", Style::default().fg(SLATE_500)),
        Span::styled(wait_text, Style::default().fg(AMBER)),
        Span::styled("  Δrate ", Style::default().fg(SLATE_500)),
        Span::styled(
            rate_delta_text,
            Style::default().fg(adaptive_rate_delta_color(rate_delta)),
        ),
        Span::styled("  risk ", Style::default().fg(SLATE_500)),
        Span::styled(
            risk_label.to_string(),
            Style::default().fg(risk_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ", Style::default().fg(SLATE_700)),
        Span::styled(
            if at_floor { "(floor)" } else { "(open)" }.to_string(),
            Style::default().fg(if at_floor { AMBER } else { TERMINAL_GREEN }),
        ),
        Span::styled("  ", Style::default().fg(SLATE_700)),
        Span::styled(last_adjustment_text, Style::default().fg(SLATE_500)),
    ])
}

fn adaptive_inflight_util(snapshot: AdaptiveControlSnapshot<'_>) -> Option<f64> {
    match (
        snapshot.adaptive_inflight_batches,
        snapshot.adaptive_inflight_limit,
    ) {
        (Some(current), Some(limit)) if limit > 0 => {
            Some((current as f64 / limit as f64).clamp(0.0, 1.0))
        }
        _ => None,
    }
}

fn adaptive_risk_label(
    at_floor: bool,
    inflight_util: Option<f64>,
    backoff_streak: Option<u64>,
) -> (&'static str, Color) {
    let streak = backoff_streak.unwrap_or(0);
    let high_inflight = inflight_util.is_some_and(|util| util >= 0.85);
    let warn_inflight = inflight_util.is_some_and(|util| util >= 0.70);

    if streak >= 6 || (at_floor && high_inflight) {
        ("HOT", ERROR_RED)
    } else if streak >= 3 || at_floor || warn_inflight {
        ("WARN", AMBER)
    } else {
        ("OK", TERMINAL_GREEN)
    }
}

fn adaptive_rate_delta_color(delta: Option<f64>) -> Color {
    match delta {
        Some(v) if v > 20.0 => TERMINAL_GREEN,
        Some(v) if v < -20.0 => ERROR_RED,
        Some(_) => AMBER,
        None => SLATE_500,
    }
}

fn adaptive_state_label(adaptive_last_reason: Option<&str>) -> (&'static str, Color) {
    match adaptive_last_reason {
        Some("healthy_step_up")
        | Some("healthy_step_up_floor_recover")
        | Some("early_height_boost") => ("EXPAND", TERMINAL_GREEN),
        Some(reason) if reason.contains("backoff") => ("BACKOFF", AMBER),
        Some("adjusted") => ("TUNE", TERMINAL_DIM),
        Some(_) => ("TUNE", TERMINAL_DIM),
        None => ("HOLD", SLATE_500),
    }
}

fn adaptive_adjustment_event_message(seq: u64, adjustment: &AdaptiveLastAdjustmentData) -> String {
    format!(
        "adaptive adjust #{} {} tx {}->{} inflight {}->{} min {}->{}",
        format_num_u64(seq),
        adjustment.reason,
        format_num_u64(adjustment.previous_target_batch_txs),
        format_num_u64(adjustment.new_target_batch_txs),
        adjustment.previous_inflight_limit,
        adjustment.new_inflight_limit,
        format_num_u64(adjustment.previous_min_target_batch_txs),
        format_num_u64(adjustment.new_min_target_batch_txs),
    )
}

fn adaptive_adjustment_event_level(reason: &str) -> LogLevel {
    if reason.contains("backoff") {
        LogLevel::Warning
    } else if matches!(
        reason,
        "healthy_step_up" | "healthy_step_up_floor_recover" | "early_height_boost"
    ) {
        LogLevel::Success
    } else {
        LogLevel::Info
    }
}

fn pipeline_reset_line(
    pipeline_reset_epoch: Option<u64>,
    pipeline_reset_reason: Option<&str>,
) -> Line<'static> {
    let epoch_text = pipeline_reset_epoch
        .map(format_num_u64)
        .unwrap_or_else(|| "-".to_string());
    let reason_text = pipeline_reset_reason.unwrap_or("-");
    let reason_color = if pipeline_reset_epoch.is_some() {
        AMBER
    } else {
        SLATE_500
    };
    Line::from(vec![
        Span::styled("Reset ", Style::default().fg(SLATE_500)),
        Span::styled(
            format!("#{} {}", epoch_text, reason_text),
            Style::default().fg(reason_color),
        ),
    ])
}

fn dense_right_lines(
    fetch_line: Line<'static>,
    parse_line: Line<'static>,
    write_line: Line<'static>,
    lag_line: Line<'static>,
    stability_line: Line<'static>,
    io_line: Line<'static>,
) -> Vec<Line<'static>> {
    vec![
        stability_line,
        fetch_line,
        parse_line,
        write_line,
        lag_line,
        io_line,
    ]
}

fn detail_right_lines(
    fetch_line: Line<'static>,
    parse_line: Line<'static>,
    write_line: Line<'static>,
    lag_line: Line<'static>,
    stability_line: Line<'static>,
    rate_line: Line<'static>,
    io_line: Line<'static>,
) -> Vec<Line<'static>> {
    vec![
        stability_line,
        fetch_line,
        parse_line,
        write_line,
        lag_line,
        rate_line,
        io_line,
    ]
}

fn sync_timing_lines(
    eta: Option<&str>,
    elapsed: Option<&str>,
    startup_phase: Option<&str>,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    if let Some(eta) = eta {
        lines.push(Line::from(vec![
            Span::styled("ETA: ", Style::default().fg(SLATE_500)),
            Span::styled(
                eta.to_string(),
                Style::default()
                    .fg(TERMINAL_GREEN)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    if let Some(elapsed) = elapsed {
        lines.push(Line::from(vec![
            Span::styled("Elapsed: ", Style::default().fg(SLATE_500)),
            Span::styled(elapsed.to_string(), Style::default().fg(FOREGROUND)),
        ]));
    }

    if let Some(startup_phase) = startup_phase {
        let (label, color) = startup_phase_label(Some(startup_phase));
        lines.push(Line::from(vec![
            Span::styled("Phase: ", Style::default().fg(SLATE_500)),
            Span::styled(label, Style::default().fg(color)),
        ]));
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "No timing data",
            Style::default().fg(SLATE_500),
        )));
    }

    lines
}

fn draw_sync_alert_strip(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SLATE_800))
        .title(Span::styled("Alert Strip", Style::default().fg(FOREGROUND)));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height == 0 {
        return;
    }

    let counters = sync_warning_counters(&app.sync_event_entries);
    let latest_warn = app
        .sync_event_entries
        .iter()
        .rev()
        .find(|entry| entry.level == LogLevel::Warning)
        .map(|entry| trim_for_panel(&entry.message, inner.width as usize))
        .unwrap_or_else(|| "none".to_string());

    let lane_state = app
        .sync_status
        .as_ref()
        .map(|sync| {
            lane_lag_state(
                sync.heavy_lane_enabled,
                sync.heavy_lane_lag_blocks,
                sync.heavy_lane_max_lag_blocks,
                sync.heavy_lane_backpressure,
            )
        })
        .unwrap_or(LaneLagState::Off);
    let (lane_label, lane_color) = lane_lag_state_badge(lane_state);

    let lines = vec![
        Line::from(vec![
            Span::styled("Lane ", Style::default().fg(SLATE_500)),
            Span::styled(
                lane_label.to_string(),
                Style::default().fg(Color::Black).bg(lane_color),
            ),
            Span::styled("  ", Style::default().fg(SLATE_700)),
            Span::styled("BP ", Style::default().fg(SLATE_500)),
            Span::styled(
                counters.backpressure.to_string(),
                Style::default().fg(AMBER),
            ),
            Span::styled("  reset ", Style::default().fg(SLATE_500)),
            Span::styled(counters.reset.to_string(), Style::default().fg(AMBER)),
            Span::styled("  stall ", Style::default().fg(SLATE_500)),
            Span::styled(counters.stall.to_string(), Style::default().fg(AMBER)),
            Span::styled("  stale ", Style::default().fg(SLATE_500)),
            Span::styled(counters.stale.to_string(), Style::default().fg(AMBER)),
            Span::styled("  other ", Style::default().fg(SLATE_500)),
            Span::styled(counters.other.to_string(), Style::default().fg(FOREGROUND)),
        ]),
        Line::from(vec![
            Span::styled("Latest WARN ", Style::default().fg(SLATE_500)),
            Span::styled(latest_warn, Style::default().fg(AMBER)),
        ]),
    ];
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn draw_sync_events(f: &mut Frame, app: &App, area: Rect) {
    let filtered_entries = filtered_sync_event_entries(app);
    let (info_count, ok_count, warn_count) = sync_event_level_counts_slice(&filtered_entries);
    let filter_label = sync_event_filter_label(app.sync_event_filter);
    let keyword_label = sync_event_keyword_filter_label(app.sync_event_keyword_filter);
    let title = if app.sync_event_scroll > 0 {
        format!(
            "Sync Events [{}|{}][j/k g/G] I:{} OK:{} W:{} ({}/{}) (scroll +{})",
            filter_label,
            keyword_label,
            info_count,
            ok_count,
            warn_count,
            filtered_entries.len(),
            app.sync_event_entries.len(),
            app.sync_event_scroll
        )
    } else {
        format!(
            "Sync Events [{}|{}][j/k g/G] I:{} OK:{} W:{} ({}/{})",
            filter_label,
            keyword_label,
            info_count,
            ok_count,
            warn_count,
            filtered_entries.len(),
            app.sync_event_entries.len()
        )
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

    if filtered_entries.is_empty() {
        f.render_widget(Paragraph::new("No sync events"), inner);
        return;
    }

    let visible = inner.height as usize;
    let total = filtered_entries.len();
    let base_start = total.saturating_sub(visible);
    let start = base_start.saturating_sub(app.sync_event_scroll);
    let end = (start + visible).min(total);

    let lines: Vec<Line> = filtered_entries
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

fn filtered_sync_event_entries(app: &App) -> Vec<&LogEntry> {
    app.sync_event_entries
        .iter()
        .filter(|entry| {
            sync_event_matches_filter(entry, app.sync_event_filter, app.sync_event_keyword_filter)
        })
        .collect()
}

fn sync_event_level_counts_slice(entries: &[&LogEntry]) -> (usize, usize, usize) {
    let mut info = 0;
    let mut ok = 0;
    let mut warn = 0;
    for entry in entries {
        match entry.level {
            LogLevel::Info => info += 1,
            LogLevel::Success => ok += 1,
            LogLevel::Warning => warn += 1,
        }
    }
    (info, ok, warn)
}

fn sync_event_matches_filter(
    entry: &LogEntry,
    filter: SyncEventFilter,
    keyword_filter: SyncEventKeywordFilter,
) -> bool {
    let level_match = match filter {
        SyncEventFilter::All => true,
        SyncEventFilter::WarnOnly => entry.level == LogLevel::Warning,
    };
    level_match && sync_event_message_matches_keyword(&entry.message, keyword_filter)
}

fn sync_event_message_matches_keyword(
    message: &str,
    keyword_filter: SyncEventKeywordFilter,
) -> bool {
    let m = message.to_ascii_lowercase();
    match keyword_filter {
        SyncEventKeywordFilter::Any => true,
        SyncEventKeywordFilter::Backpressure => {
            m.contains("backpressure") || m.contains("lag high")
        }
        SyncEventKeywordFilter::Reset => m.contains("reset"),
        SyncEventKeywordFilter::Stall => m.contains("stall"),
        SyncEventKeywordFilter::Stale => m.contains("stale"),
    }
}

#[derive(Default)]
struct WarningCounters {
    backpressure: usize,
    reset: usize,
    stall: usize,
    stale: usize,
    other: usize,
}

fn sync_warning_counters(entries: &VecDeque<LogEntry>) -> WarningCounters {
    let mut counters = WarningCounters::default();
    for entry in entries
        .iter()
        .filter(|entry| entry.level == LogLevel::Warning)
    {
        let m = entry.message.to_ascii_lowercase();
        if m.contains("backpressure") || m.contains("lag high") {
            counters.backpressure += 1;
        } else if m.contains("reset") {
            counters.reset += 1;
        } else if m.contains("stall") {
            counters.stall += 1;
        } else if m.contains("stale") {
            counters.stale += 1;
        } else {
            counters.other += 1;
        }
    }
    counters
}

fn draw_memory_stats(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SLATE_800))
        .title(Span::styled(
            "Storage Runtime",
            Style::default().fg(FOREGROUND),
        ));

    let Some(mem) = &app.memory_stats else {
        let msg = Paragraph::new("No memory stats (Redis unavailable)").block(block);
        f.render_widget(msg, area);
        return;
    };

    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height == 0 {
        return;
    }

    let rows = if inner.height >= 12 {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(4), Constraint::Min(6)])
            .split(inner)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(4), Constraint::Length(0)])
            .split(inner)
    };

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ])
        .split(rows[0]);

    let (left, mid, right) = storage_runtime_columns(mem);
    f.render_widget(Paragraph::new(left), cols[0]);
    f.render_widget(Paragraph::new(mid), cols[1]);
    f.render_widget(Paragraph::new(right), cols[2]);

    if inner.height >= 12 {
        draw_db_lanes(f, mem, rows[1]);
    }
}

fn storage_runtime_columns(
    mem: &MemoryStatsData,
) -> (Vec<Line<'static>>, Vec<Line<'static>>, Vec<Line<'static>>) {
    let live_delta = runtime_live_delta(mem.live_cells_count, mem.total_live_cells);

    let left = vec![
        metric_line(
            "Live (cache)",
            format_num_u64(mem.live_cells_count),
            TERMINAL_GREEN,
        ),
        metric_line(
            "Consumed",
            format_num_u64(mem.consumed_cells_count),
            FOREGROUND,
        ),
        Line::from(vec![
            metric_label_span("Consumed Sz"),
            Span::styled(
                format_bytes(mem.consumed_cells_bytes),
                Style::default().fg(FOREGROUND),
            ),
            Span::styled("  src ", Style::default().fg(SLATE_500)),
            Span::styled(
                consumed_cells_source_label(&mem.consumed_cells_bytes_source),
                Style::default().fg(consumed_cells_source_color(
                    &mem.consumed_cells_bytes_source,
                )),
            ),
        ]),
        metric_line(
            "Block Hdrs",
            format_num_u64(mem.block_headers_count),
            TERMINAL_DIM,
        ),
    ];

    let mid = vec![
        metric_line(
            "RocksDB Total",
            format_bytes(mem.rocksdb_total_bytes),
            TERMINAL_GREEN,
        ),
        metric_line(
            "Memtable",
            format_bytes(mem.rocksdb_memtable_bytes),
            FOREGROUND,
        ),
        metric_line(
            "Block Cache",
            format_bytes(mem.rocksdb_block_cache_bytes),
            FOREGROUND,
        ),
        metric_line(
            "TableReaders",
            format_bytes(mem.rocksdb_table_readers_bytes),
            TERMINAL_DIM,
        ),
    ];

    let right = vec![
        metric_line(
            "Chain Txs",
            format_num(mem.total_transactions),
            TERMINAL_GREEN,
        ),
        metric_line("Chain Cells", format_num(mem.total_cells), FOREGROUND),
        Line::from(vec![
            metric_label_span("Live (sync)"),
            Span::styled(
                format_num(mem.total_live_cells),
                Style::default().fg(FOREGROUND),
            ),
            Span::styled("  Δcache ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_signed_num_i128(live_delta),
                Style::default().fg(live_delta_color(live_delta)),
            ),
        ]),
        Line::from(vec![
            metric_label_span("Chain Addrs"),
            Span::styled(
                format_num(mem.total_addresses),
                Style::default().fg(TERMINAL_DIM),
            ),
            Span::styled("  mode ", Style::default().fg(SLATE_500)),
            Span::styled(
                if mem.bulk_sync_mode { "bulk" } else { "tip" },
                Style::default().fg(if mem.bulk_sync_mode {
                    AMBER
                } else {
                    TERMINAL_GREEN
                }),
            ),
            Span::styled("  cache ", Style::default().fg(SLATE_500)),
            Span::styled(
                if mem.bulk_sync_cell_cache_enabled {
                    "on"
                } else {
                    "off"
                },
                Style::default().fg(if mem.bulk_sync_cell_cache_enabled {
                    TERMINAL_GREEN
                } else {
                    AMBER
                }),
            ),
        ]),
    ];

    (left, mid, right)
}

fn consumed_cells_source_label(source: &str) -> &'static str {
    match source {
        "live" => "live",
        "sst" => "sst",
        "mem" => "mem",
        "none" => "none",
        _ => "unknown",
    }
}

fn consumed_cells_source_color(source: &str) -> Color {
    match source {
        "live" => TERMINAL_GREEN,
        "sst" | "mem" => AMBER,
        "none" => SLATE_500,
        _ => AMBER,
    }
}

fn metric_label_span(label: &str) -> Span<'static> {
    Span::styled(format!("{label:<13}: "), Style::default().fg(SLATE_500))
}

fn metric_line(label: &str, value: String, value_color: Color) -> Line<'static> {
    Line::from(vec![
        metric_label_span(label),
        Span::styled(value, Style::default().fg(value_color)),
    ])
}

fn runtime_live_delta(cache_live_cells: u64, sync_live_cells: i64) -> i128 {
    i128::from(cache_live_cells) - i128::from(sync_live_cells)
}

fn live_delta_color(delta: i128) -> Color {
    if delta == 0 {
        TERMINAL_GREEN
    } else if delta.abs() <= 100 {
        AMBER
    } else {
        ERROR_RED
    }
}

fn format_signed_num_i128(value: i128) -> String {
    if value > 0 {
        format!("+{}", format_num_i128(value))
    } else {
        format_num_i128(value)
    }
}

fn format_num_i128(value: i128) -> String {
    if value < 0 {
        return format!("-{}", format_num_commas_u128(value.unsigned_abs()));
    }
    format_num_commas_u128(value as u128)
}

fn format_num_commas_u128(value: u128) -> String {
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

fn draw_db_lanes(f: &mut Frame, mem: &MemoryStatsData, area: Rect) {
    if area.height == 0 {
        return;
    }

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let core_lines = db_lane_lines("CORE", mem.core_db.as_ref());
    let heavy_lines = db_lane_lines("HEAVY", mem.heavy_db.as_ref());
    f.render_widget(
        Paragraph::new(core_lines).wrap(Wrap { trim: false }),
        cols[0],
    );
    f.render_widget(
        Paragraph::new(heavy_lines).wrap(Wrap { trim: false }),
        cols[1],
    );
}

fn db_lane_lines(label: &'static str, stats: Option<&DbMemoryStatsData>) -> Vec<Line<'static>> {
    let Some(stats) = stats else {
        return vec![
            Line::from(vec![
                Span::styled(format!("{label:<5}"), Style::default().fg(SLATE_500)),
                Span::styled(" ", Style::default().fg(SLATE_700)),
                Span::styled("[OFF]", Style::default().fg(SLATE_500)),
            ]),
            Line::from(vec![
                Span::styled("State ", Style::default().fg(SLATE_500)),
                Span::styled("split store disabled", Style::default().fg(SLATE_500)),
            ]),
        ];
    };

    let (state, state_color) = db_lane_health(stats);
    let top_cf = stats
        .top_cf_sizes
        .first()
        .map(|(name, _)| name.as_str())
        .unwrap_or("-");
    let top_cf_size = stats
        .top_cf_sizes
        .first()
        .map(|(_, size)| *size)
        .unwrap_or(0);

    vec![
        Line::from(vec![
            Span::styled(format!("{label:<5}"), Style::default().fg(TERMINAL_GREEN)),
            Span::styled(" ", Style::default().fg(SLATE_700)),
            Span::styled(
                format!("[{state}]"),
                Style::default()
                    .fg(Color::Black)
                    .bg(state_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" total ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_bytes(stats.rocksdb_total_bytes),
                Style::default().fg(FOREGROUND),
            ),
            Span::styled("  mem ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_bytes(stats.rocksdb_memtable_bytes),
                Style::default().fg(TERMINAL_DIM),
            ),
        ]),
        Line::from(vec![
            Span::styled("L0 ", Style::default().fg(SLATE_500)),
            Span::styled(
                format!("{}/{}", stats.l0_files_count, stats.l0_files_max),
                Style::default().fg(state_color),
            ),
            Span::styled("  comp ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_bytes(stats.compaction_pending_bytes),
                Style::default().fg(FOREGROUND),
            ),
            Span::styled("  run/imm ", Style::default().fg(SLATE_500)),
            Span::styled(
                format!(
                    "{}/{}",
                    format_num_u64(stats.num_running_compactions),
                    format_num_u64(stats.immutable_memtables)
                ),
                Style::default().fg(TERMINAL_DIM),
            ),
        ]),
        Line::from(vec![
            Span::styled("Live ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_num_u64(stats.live_cells_count),
                Style::default().fg(TERMINAL_GREEN),
            ),
            Span::styled("  Cons ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_num_u64(stats.consumed_cells_count),
                Style::default().fg(FOREGROUND),
            ),
            Span::styled("  Sz ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_bytes(stats.consumed_cells_bytes),
                Style::default().fg(FOREGROUND),
            ),
            Span::styled("  src ", Style::default().fg(SLATE_500)),
            Span::styled(
                consumed_cells_source_label(&stats.consumed_cells_bytes_source),
                Style::default().fg(consumed_cells_source_color(
                    &stats.consumed_cells_bytes_source,
                )),
            ),
        ]),
        Line::from(vec![
            Span::styled("Worst ", Style::default().fg(SLATE_500)),
            Span::styled(
                if stats.l0_worst_cf.is_empty() {
                    "-".to_string()
                } else {
                    stats.l0_worst_cf.clone()
                },
                Style::default().fg(AMBER),
            ),
            Span::styled("  top ", Style::default().fg(SLATE_500)),
            Span::styled(top_cf.to_string(), Style::default().fg(FOREGROUND)),
            Span::styled(" ", Style::default().fg(SLATE_700)),
            Span::styled(format_bytes(top_cf_size), Style::default().fg(TERMINAL_DIM)),
            Span::styled("  hdr ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_num_u64(stats.block_headers_count),
                Style::default().fg(TERMINAL_DIM),
            ),
        ]),
    ]
}

fn db_lane_health(stats: &DbMemoryStatsData) -> (&'static str, Color) {
    if stats.l0_files_max >= 16 || stats.compaction_pending_bytes >= 64 * 1024 * 1024 * 1024 {
        ("HOT", ERROR_RED)
    } else if stats.l0_files_max >= 10 || stats.compaction_pending_bytes >= 16 * 1024 * 1024 * 1024
    {
        ("WARN", AMBER)
    } else {
        ("OK", TERMINAL_GREEN)
    }
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

fn draw_service_windows(f: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ])
        .split(area);
    draw_redis_health(f, app, cols[0]);
    draw_api_health(f, app, cols[1]);
    draw_runtime_health(f, app, cols[2]);
}

fn draw_redis_health(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SLATE_800))
        .title(Span::styled(
            "Redis Health",
            Style::default().fg(FOREGROUND),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height == 0 {
        return;
    }

    let info = &app.redis_service;
    let (state, state_color) = redis_health_state(info);
    let max_age = redis_max_key_age(info);

    let mut lines = vec![
        Line::from(vec![
            Span::styled("State ", Style::default().fg(SLATE_500)),
            Span::styled(
                format!("[{}]", state),
                Style::default()
                    .fg(state_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  RTT ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_latency_ms(info.latency_ms),
                Style::default().fg(FOREGROUND),
            ),
        ]),
        Line::from(vec![
            Span::styled("Keys ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_num_u64(info.db_keys_total.unwrap_or(0)),
                Style::default().fg(TERMINAL_DIM),
            ),
            Span::styled("  exp ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_num_u64(info.db_keys_expiring.unwrap_or(0)),
                Style::default().fg(TERMINAL_DIM),
            ),
            Span::styled("  pers ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_num_u64(info.db_keys_persistent.unwrap_or(0)),
                Style::default().fg(TERMINAL_DIM),
            ),
        ]),
        Line::from(vec![
            Span::styled("Mem ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_bytes(info.used_memory_bytes.unwrap_or(0)),
                Style::default().fg(TERMINAL_DIM),
            ),
            Span::styled("  peak ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_bytes(info.used_memory_peak_bytes.unwrap_or(0)),
                Style::default().fg(TERMINAL_DIM),
            ),
            Span::styled("  frag ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_ratio(info.mem_fragmentation_ratio),
                Style::default().fg(AMBER),
            ),
        ]),
        Line::from(vec![
            Span::styled("Hit ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_hit_rate(info.keyspace_hits, info.keyspace_misses),
                Style::default().fg(TERMINAL_GREEN),
            ),
            Span::styled("  evict ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_num_u64(info.evicted_keys.unwrap_or(0)),
                Style::default().fg(AMBER),
            ),
            Span::styled("  max-age ", Style::default().fg(SLATE_500)),
            Span::styled(format_age_secs(max_age), Style::default().fg(TERMINAL_DIM)),
        ]),
    ];

    lines.push(redis_key_line(
        "sync:status",
        info.sync_status_type.as_deref(),
        info.sync_status_value_bytes,
        info.sync_status_ttl_secs,
        info.sync_status_age_secs,
    ));
    lines.push(redis_key_line(
        "sync:progress",
        info.sync_progress_type.as_deref(),
        info.sync_progress_value_bytes,
        info.sync_progress_ttl_secs,
        info.sync_progress_age_secs,
    ));
    lines.push(redis_key_line(
        "memory:stats",
        info.memory_stats_type.as_deref(),
        info.memory_stats_value_bytes,
        info.memory_stats_ttl_secs,
        info.memory_stats_age_secs,
    ));

    if let Some(err) = &info.error {
        lines.push(Line::from(vec![
            Span::styled("Err ", Style::default().fg(SLATE_500)),
            Span::styled(
                trim_for_panel(err, inner.width as usize),
                Style::default().fg(AMBER),
            ),
        ]));
    } else if !info.enabled {
        lines.push(Line::from(Span::styled(
            "Not configured (REDIS_URL)",
            Style::default().fg(SLATE_500),
        )));
    }

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn draw_api_health(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SLATE_800))
        .title(Span::styled("API Health", Style::default().fg(FOREGROUND)));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height == 0 {
        return;
    }

    let info = &app.api_service;
    let (state, state_color) = api_health_state(info);
    let status_text = info
        .status_code
        .map(|code| code.to_string())
        .unwrap_or_else(|| "-".to_string());
    let latest_block = info
        .latest_block
        .map(format_num)
        .unwrap_or_else(|| "-".to_string());

    let mut lines = vec![
        Line::from(vec![
            Span::styled("State ", Style::default().fg(SLATE_500)),
            Span::styled(
                format!("[{}]", state),
                Style::default()
                    .fg(state_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  RTT ", Style::default().fg(SLATE_500)),
            Span::styled(
                format_latency_ms(info.latency_ms),
                Style::default().fg(FOREGROUND),
            ),
            Span::styled("  HTTP ", Style::default().fg(SLATE_500)),
            Span::styled(status_text, Style::default().fg(TERMINAL_DIM)),
        ]),
        Line::from(vec![
            Span::styled("Latest ", Style::default().fg(SLATE_500)),
            Span::styled(latest_block, Style::default().fg(TERMINAL_GREEN)),
            Span::styled("  TPS ", Style::default().fg(SLATE_500)),
            Span::styled(
                info.tps.clone().unwrap_or_else(|| "-".to_string()),
                Style::default().fg(TERMINAL_DIM),
            ),
        ]),
        Line::from(vec![
            Span::styled("Avg Block ", Style::default().fg(SLATE_500)),
            Span::styled(
                info.avg_block_time
                    .clone()
                    .unwrap_or_else(|| "-".to_string()),
                Style::default().fg(FOREGROUND),
            ),
        ]),
    ];

    if let Some(err) = &info.error {
        lines.push(Line::from(vec![
            Span::styled("Err ", Style::default().fg(SLATE_500)),
            Span::styled(
                trim_for_panel(err, inner.width as usize),
                Style::default().fg(AMBER),
            ),
        ]));
    }

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn draw_runtime_health(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SLATE_800))
        .title(Span::styled("Run Health", Style::default().fg(FOREGROUND)));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height == 0 {
        return;
    }

    let (state, state_color) = runtime_health_state(app.runtime_diag.as_ref());
    let Some(diag) = app.runtime_diag.as_ref() else {
        f.render_widget(Paragraph::new("No runtime diagnostics"), inner);
        return;
    };

    let run_label = if diag.active_run_id.is_some() {
        "active"
    } else {
        "last"
    };
    let run_id = diag
        .active_run_id
        .as_ref()
        .or(diag.last_run_id.as_ref())
        .map(|s| trim_for_panel(s, inner.width as usize))
        .unwrap_or_else(|| "-".to_string());
    let stage = diag
        .heartbeat_stage
        .as_deref()
        .unwrap_or("unknown")
        .to_string();
    let age_text = format_age_secs(diag.heartbeat_age_secs);
    let incident = diag
        .last_incident_summary
        .as_ref()
        .map(|s| trim_for_panel(s, inner.width as usize))
        .unwrap_or_else(|| "-".to_string());
    let shutdown_reason = diag
        .last_shutdown_reason
        .as_ref()
        .map(|s| trim_for_panel(s, inner.width as usize))
        .unwrap_or_else(|| "-".to_string());

    let lines = vec![
        Line::from(vec![
            Span::styled("State ", Style::default().fg(SLATE_500)),
            Span::styled(
                format!("[{}]", state),
                Style::default()
                    .fg(state_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  age ", Style::default().fg(SLATE_500)),
            Span::styled(age_text, Style::default().fg(TERMINAL_DIM)),
        ]),
        Line::from(vec![
            Span::styled("Stage ", Style::default().fg(SLATE_500)),
            Span::styled(stage, Style::default().fg(FOREGROUND)),
            Span::styled("  block ", Style::default().fg(SLATE_500)),
            Span::styled(
                format!(
                    "{}/{}",
                    format_num(diag.heartbeat_block),
                    format_num(diag.heartbeat_target_block)
                ),
                Style::default().fg(TERMINAL_GREEN),
            ),
        ]),
        Line::from(vec![
            Span::styled("OOM ", Style::default().fg(SLATE_500)),
            Span::styled(
                format!(
                    "{}/{}",
                    format_num_u64(diag.heartbeat_oom_events.unwrap_or(0)),
                    format_num_u64(diag.heartbeat_oom_kill_events.unwrap_or(0))
                ),
                Style::default().fg(AMBER),
            ),
            Span::styled("  (events/kill)", Style::default().fg(SLATE_500)),
        ]),
        Line::from(vec![
            Span::styled(
                format!("Run {} ", run_label),
                Style::default().fg(SLATE_500),
            ),
            Span::styled(run_id, Style::default().fg(TERMINAL_DIM)),
        ]),
        Line::from(vec![
            Span::styled("Incident ", Style::default().fg(SLATE_500)),
            Span::styled(incident, Style::default().fg(AMBER)),
        ]),
        Line::from(vec![
            Span::styled("Shutdown ", Style::default().fg(SLATE_500)),
            Span::styled(
                format!(
                    "{}{}",
                    shutdown_reason,
                    diag.last_exit_code
                        .map(|code| format!(" (code {code})"))
                        .unwrap_or_default()
                ),
                Style::default().fg(FOREGROUND),
            ),
        ]),
    ];

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn redis_max_key_age(info: &RedisServiceInfo) -> Option<i64> {
    [
        info.sync_status_age_secs,
        info.sync_progress_age_secs,
        info.memory_stats_age_secs,
    ]
    .into_iter()
    .flatten()
    .max()
}

fn redis_health_state(info: &RedisServiceInfo) -> (&'static str, Color) {
    if !info.enabled {
        return ("OFF", SLATE_500);
    }
    if !info.reachable {
        return ("DOWN", ERROR_RED);
    }
    if redis_max_key_age(info).is_some_and(|age| age > 30) {
        ("STALE", AMBER)
    } else {
        ("OK", TERMINAL_GREEN)
    }
}

fn api_health_state(info: &ApiServiceInfo) -> (&'static str, Color) {
    if !info.reachable {
        return ("DOWN", ERROR_RED);
    }
    if info.status_code.is_some_and(|code| code >= 500)
        || info.latency_ms.is_some_and(|latency| latency >= 1500.0)
    {
        ("WARN", AMBER)
    } else {
        ("OK", TERMINAL_GREEN)
    }
}

fn runtime_health_state(info: Option<&RuntimeDiagData>) -> (&'static str, Color) {
    let Some(info) = info else {
        return ("N/A", SLATE_500);
    };
    if info
        .heartbeat_age_secs
        .is_some_and(|age| age > 60 && info.active_run_id.is_some())
    {
        return ("STALE", AMBER);
    }
    if info
        .heartbeat_oom_kill_events
        .is_some_and(|kills| kills > 0)
        || info.last_incident_summary.is_some()
    {
        return ("WARN", AMBER);
    }
    if info.active_run_id.is_none() {
        ("IDLE", SLATE_500)
    } else {
        ("OK", TERMINAL_GREEN)
    }
}

fn format_latency_ms(latency_ms: Option<f64>) -> String {
    latency_ms
        .map(|v| format!("{v:.1}ms"))
        .unwrap_or_else(|| "-".to_string())
}

fn format_age_secs(age_secs: Option<i64>) -> String {
    age_secs
        .map(|v| format!("{v}s"))
        .unwrap_or_else(|| "-".to_string())
}

fn format_ttl(ttl_secs: Option<i64>) -> String {
    match ttl_secs {
        Some(-1) => "persist".to_string(),
        Some(v) => format!("{v}s"),
        None => "-".to_string(),
    }
}

fn format_ratio(value: Option<f64>) -> String {
    value
        .map(|v| format!("{v:.2}"))
        .unwrap_or_else(|| "-".to_string())
}

fn format_hit_rate(hits: Option<u64>, misses: Option<u64>) -> String {
    match (hits, misses) {
        (Some(h), Some(m)) if h + m > 0 => format!("{:.1}%", h as f64 * 100.0 / (h + m) as f64),
        _ => "-".to_string(),
    }
}

fn redis_key_line(
    name: &str,
    key_type: Option<&str>,
    value_bytes: Option<u64>,
    ttl_secs: Option<i64>,
    age_secs: Option<i64>,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(name.to_string(), Style::default().fg(SLATE_500)),
        Span::styled(" ", Style::default().fg(SLATE_700)),
        Span::styled(
            key_type.unwrap_or("-").to_string(),
            Style::default().fg(TERMINAL_DIM),
        ),
        Span::styled(" ", Style::default().fg(SLATE_700)),
        Span::styled(
            format_bytes(value_bytes.unwrap_or(0)),
            Style::default().fg(FOREGROUND),
        ),
        Span::styled(" ttl ", Style::default().fg(SLATE_500)),
        Span::styled(format_ttl(ttl_secs), Style::default().fg(AMBER)),
        Span::styled(" age ", Style::default().fg(SLATE_500)),
        Span::styled(format_age_secs(age_secs), Style::default().fg(TERMINAL_DIM)),
    ])
}

fn trim_for_panel(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let max = width.saturating_sub(6);
    if text.chars().count() <= max {
        return text.to_string();
    }
    if max <= 3 {
        return "...".to_string();
    }
    let mut out = String::new();
    for ch in text.chars().take(max - 3) {
        out.push(ch);
    }
    out.push_str("...");
    out
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
        Span::styled("f", Style::default().fg(TERMINAL_GREEN)),
        Span::styled(" focus  ", Style::default().fg(SLATE_500)),
        Span::styled("e", Style::default().fg(TERMINAL_GREEN)),
        Span::styled(" event-filter  ", Style::default().fg(SLATE_500)),
        Span::styled("x", Style::default().fg(TERMINAL_GREEN)),
        Span::styled(" keyword-match  ", Style::default().fg(SLATE_500)),
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

fn is_rate_drop(previous: f64, current: f64) -> bool {
    previous > 0.0 && current > 0.0 && current < previous * RATE_DROP_RATIO_THRESHOLD
}

fn diagnostics_view_mode_label(mode: DiagnosticsViewMode) -> &'static str {
    match mode {
        DiagnosticsViewMode::Auto => "Auto",
        DiagnosticsViewMode::Compact => "Compact",
        DiagnosticsViewMode::Detail => "Detail",
    }
}

fn sync_event_filter_label(filter: SyncEventFilter) -> &'static str {
    match filter {
        SyncEventFilter::All => "All",
        SyncEventFilter::WarnOnly => "Warn",
    }
}

fn sync_event_keyword_filter_label(filter: SyncEventKeywordFilter) -> &'static str {
    match filter {
        SyncEventKeywordFilter::Any => "Any",
        SyncEventKeywordFilter::Backpressure => "Backpressure",
        SyncEventKeywordFilter::Reset => "Reset",
        SyncEventKeywordFilter::Stall => "Stall",
        SyncEventKeywordFilter::Stale => "Stale",
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

fn sync_bottleneck(write_ms: Option<f64>, fetch_ms: Option<f64>) -> SyncBottleneck {
    match (write_ms, fetch_ms) {
        (Some(write), Some(fetch)) if write > 0.0 && fetch > 0.0 => {
            if write > fetch * 1.2 {
                SyncBottleneck::WriteBound
            } else if fetch > write * 1.2 {
                SyncBottleneck::FetchBound
            } else {
                SyncBottleneck::Mixed
            }
        }
        (Some(write), None) if write > 0.0 => SyncBottleneck::WriteBound,
        (None, Some(fetch)) if fetch > 0.0 => SyncBottleneck::FetchBound,
        _ => SyncBottleneck::Unknown,
    }
}

fn bottleneck_label(bottleneck: SyncBottleneck) -> &'static str {
    match bottleneck {
        SyncBottleneck::WriteBound => "write-bound",
        SyncBottleneck::FetchBound => "fetch-bound",
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

fn lag_trend_line(history: &VecDeque<f64>, spark_width: usize) -> Line<'static> {
    let delta = trend_delta(history, 10);
    Line::from(vec![
        Span::styled("L", Style::default().fg(CYAN)),
        Span::styled(" ", Style::default().fg(SLATE_700)),
        Span::styled(sparkline(history, spark_width), Style::default().fg(CYAN)),
        Span::styled(" ", Style::default().fg(SLATE_700)),
        Span::styled(
            format_delta(delta, "blk"),
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

fn startup_phase_label(startup_phase: Option<&str>) -> (String, Color) {
    match startup_phase {
        Some("rollback_cleanup") => ("rollback_cleanup".to_string(), AMBER),
        Some("run_start") => ("run_start".to_string(), AMBER),
        Some("bulk_sync") => ("bulk_sync".to_string(), TERMINAL_DIM),
        Some("tip_sync") => ("tip_sync".to_string(), TERMINAL_GREEN),
        Some(custom) => (custom.to_string(), SLATE_500),
        None => ("steady".to_string(), TERMINAL_GREEN),
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

fn compact_sync_layout(area: Rect) -> CompactSyncLayout {
    let min_height_for_charts = if area.width < 120 { 38 } else { 30 };
    if area.height >= min_height_for_charts {
        CompactSyncLayout::ChartsAndDiagnostics
    } else {
        CompactSyncLayout::DiagnosticsOnly
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
        Line::from("  f        Toggle sync focus mode"),
        Line::from("  e        Toggle sync event filter (All/WarnOnly)"),
        Line::from("  x        Cycle sync event keyword match"),
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
        adaptive_adjustment_event_level, adaptive_adjustment_event_message, adaptive_context_line,
        adaptive_control_line, adaptive_rate_delta_color, adaptive_risk_label,
        adaptive_state_label, api_health_state, chart_height_warning, compact_overview_layout,
        compact_sync_layout, consumed_cells_source_color, consumed_cells_source_label,
        db_lane_health, db_lane_lines, dense_right_lines, detail_right_lines,
        diagnostics_dense_panel, eta_confidence_label, format_age_secs, format_hit_rate,
        format_num, format_num_commas, format_rate_pair, format_ratio, format_signed_num_i128,
        format_ttl, header_right_line, header_title_line, heartbeat_is_on,
        io_fetch_write_jitter_line, is_rate_drop, lane_lag_state, lane_lag_state_badge,
        lane_status_text, overview_log_min_height, overview_services_min_height,
        pipeline_bottleneck, pipeline_flow_state, pipeline_reset_line, rate_jitter,
        redis_health_state, redis_key_line, redis_max_key_age, runtime_health_state,
        runtime_live_delta, sparkline, stack_sync_charts, startup_phase_label,
        storage_runtime_columns, sync_bottleneck, sync_chart_specs, sync_event_filter_label,
        sync_event_keyword_filter_label, sync_event_level_counts_slice,
        sync_event_message_matches_keyword, sync_timing_lines, sync_warning_counters, trend_delta,
        trim_for_panel, AdaptiveControlSnapshot, Color, CompactOverviewLayout, CompactSyncLayout,
        DiagnosticsViewMode, LaneLagState, LogEntry, LogLevel, SyncBottleneck, SyncChartKind,
        SyncEventFilter, SyncEventKeywordFilter, TERMINAL_DIM,
    };
    use crate::db::{ApiServiceInfo, RedisServiceInfo, RuntimeDiagData};
    use chrono::Local;
    use ckbadger_common::{AdaptiveLastAdjustmentData, DbMemoryStatsData, MemoryStatsData};
    use ratatui::layout::Rect;
    use ratatui::text::Line;
    use std::collections::VecDeque;

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    }

    #[test]
    fn test_header_title_line_no_data_source_label() {
        let line = header_title_line("SYNCING", TERMINAL_DIM);
        let text = line_text(&line);
        assert!(text.contains("CKBadger Monitor"));
        assert!(!text.contains("[DB]"));
        assert!(!text.contains("[RPC]"));
    }

    #[test]
    fn test_header_right_line_does_not_include_elapsed_ms() {
        let line = header_right_line(12, "10:23:45");
        let text = line_text(&line);
        assert!(text.contains("stale 12s"));
        assert!(text.contains("10:23:45"));
        assert!(!text.contains("ago"));
    }

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
    fn test_is_rate_drop_threshold() {
        assert!(is_rate_drop(100.0, 64.0));
        assert!(!is_rate_drop(100.0, 65.0));
        assert!(!is_rate_drop(100.0, 70.0));
        assert!(!is_rate_drop(0.0, 10.0));
        assert!(!is_rate_drop(100.0, 0.0));
    }

    #[test]
    fn test_sync_bottleneck_detection() {
        assert_eq!(
            sync_bottleneck(Some(20.0), Some(5.0)),
            SyncBottleneck::WriteBound
        );
        assert_eq!(
            sync_bottleneck(Some(5.0), Some(20.0)),
            SyncBottleneck::FetchBound
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

    #[test]
    fn test_compact_sync_layout() {
        assert_eq!(
            compact_sync_layout(Rect::new(0, 0, 120, 20)),
            CompactSyncLayout::DiagnosticsOnly
        );
        assert_eq!(
            compact_sync_layout(Rect::new(0, 0, 120, 29)),
            CompactSyncLayout::DiagnosticsOnly
        );
        assert_eq!(
            compact_sync_layout(Rect::new(0, 0, 120, 30)),
            CompactSyncLayout::ChartsAndDiagnostics
        );
        assert_eq!(
            compact_sync_layout(Rect::new(0, 0, 100, 37)),
            CompactSyncLayout::DiagnosticsOnly
        );
        assert_eq!(
            compact_sync_layout(Rect::new(0, 0, 100, 38)),
            CompactSyncLayout::ChartsAndDiagnostics
        );
    }

    #[test]
    fn test_io_fetch_write_jitter_line_format() {
        let line = io_fetch_write_jitter_line("123.4ms", "567.8ms", "9.0 blk/s");
        let text = line_text(&line);
        assert!(text.starts_with("I/O Fetch 123.4ms Write 567.8ms"));
        assert!(text.contains("jitter 9.0 blk/s"));
    }

    #[test]
    fn test_format_rate_pair() {
        assert_eq!(
            format_rate_pair(Some(123.6), Some(100.2), "blk/s"),
            "124/100 blk/s"
        );
        assert_eq!(format_rate_pair(None, Some(5.0), "tx/s"), "-/5 tx/s");
        assert_eq!(format_rate_pair(Some(5.0), None, "tx/s"), "5/- tx/s");
    }

    #[test]
    fn test_lane_status_text() {
        assert_eq!(lane_status_text(false, None, None, None), "off");
        assert_eq!(
            lane_status_text(true, Some(120), Some(20_000), Some(false)),
            "120/20,000 FLOW"
        );
        assert_eq!(
            lane_status_text(true, Some(25_000), Some(20_000), Some(true)),
            "25,000/20,000 BP"
        );
    }

    #[test]
    fn test_lane_lag_state_and_badge() {
        assert_eq!(lane_lag_state(false, None, None, None), LaneLagState::Off);
        assert_eq!(
            lane_lag_state(true, Some(0), Some(20_000), Some(false)),
            LaneLagState::Ok
        );
        assert_eq!(
            lane_lag_state(true, Some(16_500), Some(20_000), Some(false)),
            LaneLagState::Warn
        );
        assert_eq!(
            lane_lag_state(true, Some(20_000), Some(20_000), Some(false)),
            LaneLagState::Hot
        );
        assert_eq!(
            lane_lag_state(true, Some(500), Some(20_000), Some(true)),
            LaneLagState::Hot
        );
        assert_eq!(lane_lag_state_badge(LaneLagState::Off).0, "LAG OFF");
        assert_eq!(lane_lag_state_badge(LaneLagState::Warn).0, "LAG WARN");
        assert_eq!(lane_lag_state_badge(LaneLagState::Hot).0, "LAG HOT");
    }

    #[test]
    fn test_sync_event_filter_label() {
        assert_eq!(sync_event_filter_label(SyncEventFilter::All), "All");
        assert_eq!(sync_event_filter_label(SyncEventFilter::WarnOnly), "Warn");
    }

    #[test]
    fn test_sync_event_keyword_filter_label() {
        assert_eq!(
            sync_event_keyword_filter_label(SyncEventKeywordFilter::Any),
            "Any"
        );
        assert_eq!(
            sync_event_keyword_filter_label(SyncEventKeywordFilter::Backpressure),
            "Backpressure"
        );
        assert_eq!(
            sync_event_keyword_filter_label(SyncEventKeywordFilter::Reset),
            "Reset"
        );
    }

    #[test]
    fn test_sync_event_message_matches_keyword() {
        assert!(sync_event_message_matches_keyword(
            "Heavy lane lag high; pausing core lane for backpressure",
            SyncEventKeywordFilter::Backpressure
        ));
        assert!(sync_event_message_matches_keyword(
            "pipeline reset #9 (batch mismatch)",
            SyncEventKeywordFilter::Reset
        ));
        assert!(sync_event_message_matches_keyword(
            "sync progress stalled",
            SyncEventKeywordFilter::Stall
        ));
        assert!(sync_event_message_matches_keyword(
            "sync data is stale (45s)",
            SyncEventKeywordFilter::Stale
        ));
        assert!(!sync_event_message_matches_keyword(
            "bulk sync completed",
            SyncEventKeywordFilter::Stale
        ));
    }

    #[test]
    fn test_sync_event_level_counts() {
        let mut entries = VecDeque::new();
        entries.push_back(LogEntry {
            timestamp: Local::now(),
            message: "a".to_string(),
            level: LogLevel::Info,
        });
        entries.push_back(LogEntry {
            timestamp: Local::now(),
            message: "b".to_string(),
            level: LogLevel::Warning,
        });
        entries.push_back(LogEntry {
            timestamp: Local::now(),
            message: "c".to_string(),
            level: LogLevel::Success,
        });
        entries.push_back(LogEntry {
            timestamp: Local::now(),
            message: "d".to_string(),
            level: LogLevel::Warning,
        });
        let refs: Vec<&LogEntry> = entries.iter().collect();
        assert_eq!(sync_event_level_counts_slice(&refs), (1, 1, 2));
    }

    #[test]
    fn test_sync_warning_counters() {
        let mut entries = VecDeque::new();
        entries.push_back(LogEntry {
            timestamp: Local::now(),
            message: "heavy lane lag high; pausing core lane for backpressure".to_string(),
            level: LogLevel::Warning,
        });
        entries.push_back(LogEntry {
            timestamp: Local::now(),
            message: "pipeline reset #3 (pipeline batch mismatch)".to_string(),
            level: LogLevel::Warning,
        });
        entries.push_back(LogEntry {
            timestamp: Local::now(),
            message: "sync progress stalled".to_string(),
            level: LogLevel::Warning,
        });
        entries.push_back(LogEntry {
            timestamp: Local::now(),
            message: "sync data is stale (45s)".to_string(),
            level: LogLevel::Warning,
        });
        entries.push_back(LogEntry {
            timestamp: Local::now(),
            message: "some other warning".to_string(),
            level: LogLevel::Warning,
        });

        let counters = sync_warning_counters(&entries);
        assert_eq!(counters.backpressure, 1);
        assert_eq!(counters.reset, 1);
        assert_eq!(counters.stall, 1);
        assert_eq!(counters.stale, 1);
        assert_eq!(counters.other, 1);
    }

    #[test]
    fn test_adaptive_control_line_format() {
        let line = adaptive_control_line(AdaptiveControlSnapshot {
            last_batch_blocks: Some(512),
            adaptive_inflight_batches: Some(2),
            adaptive_target_batch_txs: Some(40_000),
            adaptive_inflight_limit: Some(3),
            adaptive_max_inflight_limit: Some(8),
            adaptive_min_target_batch_txs: Some(10_000),
            adaptive_cooldown_steps: Some(2),
            adaptive_last_reason: Some("pressure_backoff"),
            adaptive_adjustment_seq: Some(12),
            adaptive_last_adjusted_age_secs: Some(7),
            adaptive_backoff_streak: Some(3),
            adaptive_at_floor: Some(false),
            adaptive_last_adjustment: None,
        });
        let text = line_text(&line);
        assert!(text.contains("Adaptive"));
        assert!(text.contains("batch 512 blk"));
        assert!(text.contains("inflight 2/3"));
        assert!(text.contains("tx target/min 40,000/10,000"));
        assert!(text.contains("cd 2"));
        assert!(text.contains("chg #12 7s"));
        assert!(text.contains("BACKOFFx3"));
    }

    #[test]
    fn test_adaptive_state_label() {
        assert_eq!(
            adaptive_state_label(Some("healthy_step_up")),
            ("EXPAND", Color::Rgb(0, 255, 65))
        );
        assert_eq!(
            adaptive_state_label(Some("pressure_backoff")),
            ("BACKOFF", Color::Rgb(255, 176, 0))
        );
        assert_eq!(
            adaptive_state_label(None),
            ("HOLD", Color::Rgb(160, 174, 192))
        );
    }

    #[test]
    fn test_adaptive_context_line_format_and_risk() {
        let line = adaptive_context_line(
            AdaptiveControlSnapshot {
                last_batch_blocks: Some(512),
                adaptive_inflight_batches: Some(8),
                adaptive_target_batch_txs: Some(10_000),
                adaptive_inflight_limit: Some(8),
                adaptive_max_inflight_limit: Some(8),
                adaptive_min_target_batch_txs: Some(10_000),
                adaptive_cooldown_steps: Some(2),
                adaptive_last_reason: Some("pressure_backoff"),
                adaptive_adjustment_seq: Some(12),
                adaptive_last_adjusted_age_secs: Some(7),
                adaptive_backoff_streak: Some(4),
                adaptive_at_floor: Some(true),
                adaptive_last_adjustment: Some(&AdaptiveLastAdjustmentData {
                    previous_target_batch_txs: 12_000,
                    new_target_batch_txs: 10_000,
                    previous_inflight_limit: 9,
                    new_inflight_limit: 8,
                    previous_min_target_batch_txs: 10_000,
                    new_min_target_batch_txs: 10_000,
                    reason: "pressure_backoff".to_string(),
                    adjusted_at: 123,
                }),
            },
            Some(0.82),
            Some(0.77),
            Some(0.88),
            Some(140.0),
            Some(-35.0),
        );
        let text = line_text(&line);
        assert!(text.contains("A-ctx"));
        assert!(text.contains("reason pressure_backoff"));
        assert!(text.contains("q 82%/77%/88%"));
        assert!(text.contains("lim 8/8"));
        assert!(text.contains("wait 140.0ms"));
        assert!(text.contains("Δrate -35.0blk/s"));
        assert!(text.contains("risk HOT"));
        assert!(text.contains("(floor)"));
        assert!(text.contains("tx 12,000->10,000 if 9->8"));
    }

    #[test]
    fn test_adaptive_risk_label_thresholds() {
        assert_eq!(
            adaptive_risk_label(false, Some(0.50), Some(0)),
            ("OK", Color::Rgb(0, 255, 65))
        );
        assert_eq!(
            adaptive_risk_label(false, Some(0.72), Some(0)),
            ("WARN", Color::Rgb(255, 176, 0))
        );
        assert_eq!(
            adaptive_risk_label(true, Some(0.90), Some(1)),
            ("HOT", Color::Rgb(239, 68, 68))
        );
    }

    #[test]
    fn test_adaptive_rate_delta_color() {
        assert_eq!(
            adaptive_rate_delta_color(Some(25.0)),
            Color::Rgb(0, 255, 65)
        );
        assert_eq!(
            adaptive_rate_delta_color(Some(-25.0)),
            Color::Rgb(239, 68, 68)
        );
        assert_eq!(
            adaptive_rate_delta_color(Some(5.0)),
            Color::Rgb(255, 176, 0)
        );
        assert_eq!(adaptive_rate_delta_color(None), Color::Rgb(160, 174, 192));
    }

    #[test]
    fn test_adaptive_adjustment_event_message() {
        let adjustment = AdaptiveLastAdjustmentData {
            previous_target_batch_txs: 50_000,
            new_target_batch_txs: 40_000,
            previous_inflight_limit: 4,
            new_inflight_limit: 3,
            previous_min_target_batch_txs: 12_000,
            new_min_target_batch_txs: 10_000,
            reason: "pressure_backoff".to_string(),
            adjusted_at: 123,
        };
        let message = adaptive_adjustment_event_message(12, &adjustment);
        assert!(message.contains("adaptive adjust #12"));
        assert!(message.contains("pressure_backoff"));
        assert!(message.contains("tx 50,000->40,000"));
        assert!(message.contains("inflight 4->3"));
        assert!(message.contains("min 12,000->10,000"));
    }

    #[test]
    fn test_adaptive_adjustment_event_level() {
        assert_eq!(
            adaptive_adjustment_event_level("pressure_backoff"),
            LogLevel::Warning
        );
        assert_eq!(
            adaptive_adjustment_event_level("healthy_step_up"),
            LogLevel::Success
        );
        assert_eq!(adaptive_adjustment_event_level("adjusted"), LogLevel::Info);
    }

    #[test]
    fn test_pipeline_reset_line_format() {
        let line = pipeline_reset_line(Some(7), Some("pipeline batch mismatch"));
        let text = line_text(&line);
        assert!(text.contains("Reset"));
        assert!(text.contains("#7"));
        assert!(text.contains("pipeline batch mismatch"));
    }

    #[test]
    fn test_dense_right_lines_order() {
        let lines = dense_right_lines(
            Line::from("F"),
            Line::from("P"),
            Line::from("W"),
            Line::from("L"),
            Line::from("Stability"),
            Line::from("I/O"),
        );
        let labels: Vec<String> = lines.iter().map(line_text).collect();
        assert_eq!(labels, vec!["Stability", "F", "P", "W", "L", "I/O"]);
    }

    #[test]
    fn test_detail_right_lines_order() {
        let lines = detail_right_lines(
            Line::from("F"),
            Line::from("P"),
            Line::from("W"),
            Line::from("L"),
            Line::from("Stability"),
            Line::from("Rate"),
            Line::from("I/O"),
        );
        let labels: Vec<String> = lines.iter().map(line_text).collect();
        assert_eq!(labels, vec!["Stability", "F", "P", "W", "L", "Rate", "I/O"]);
    }

    #[test]
    fn test_sync_timing_lines_do_not_show_data_source() {
        let lines = sync_timing_lines(Some("2m 03s"), Some("17m 12s"), Some("bulk_sync"));
        let text = lines
            .iter()
            .map(line_text)
            .collect::<Vec<String>>()
            .join(" ");
        assert!(text.contains("ETA: 2m 03s"));
        assert!(text.contains("Elapsed: 17m 12s"));
        assert!(text.contains("Phase: bulk_sync"));
        assert!(!text.contains("Source"));
        assert!(!text.contains("DB"));
        assert!(!text.contains("RPC"));
    }

    #[test]
    fn test_sync_timing_lines_empty_shows_fallback() {
        let lines = sync_timing_lines(None, None, None);
        assert_eq!(lines.len(), 1);
        assert_eq!(line_text(&lines[0]), "No timing data");
    }

    #[test]
    fn test_stack_sync_charts_rule() {
        assert!(stack_sync_charts(Rect::new(0, 0, 100, 12)));
        assert!(!stack_sync_charts(Rect::new(0, 0, 100, 8)));
        assert!(!stack_sync_charts(Rect::new(0, 0, 130, 12)));
    }

    #[test]
    fn test_sync_chart_specs_include_tx_rate() {
        let stacked = sync_chart_specs(true, false);
        assert_eq!(stacked.len(), 2);
        assert_eq!(stacked[0].kind, SyncChartKind::BlockRate);
        assert_eq!(stacked[1].kind, SyncChartKind::TxRate);

        let wide = sync_chart_specs(false, false);
        assert_eq!(wide.len(), 3);
        assert_eq!(wide[0].kind, SyncChartKind::BlockRate);
        assert_eq!(wide[1].kind, SyncChartKind::TxRate);
        assert_eq!(wide[2].kind, SyncChartKind::WriteLatency);

        let ultra = sync_chart_specs(false, true);
        assert_eq!(ultra.len(), 4);
        assert_eq!(ultra[2].kind, SyncChartKind::LaneLag);
        assert_eq!(ultra[3].kind, SyncChartKind::WriteLatency);
    }

    #[test]
    fn test_chart_height_warning() {
        assert_eq!(
            chart_height_warning(0),
            Some("Insufficient height for chart")
        );
        assert_eq!(
            chart_height_warning(2),
            Some("Insufficient height for chart")
        );
        assert_eq!(chart_height_warning(3), None);
    }

    #[test]
    fn test_startup_phase_label() {
        assert_eq!(
            startup_phase_label(Some("rollback_cleanup")),
            ("rollback_cleanup".to_string(), Color::Rgb(255, 176, 0))
        );
        assert_eq!(
            startup_phase_label(Some("tip_sync")),
            ("tip_sync".to_string(), Color::Rgb(0, 255, 65))
        );
        assert_eq!(
            startup_phase_label(Some("custom-phase")),
            ("custom-phase".to_string(), Color::Rgb(160, 174, 192))
        );
        assert_eq!(
            startup_phase_label(None),
            ("steady".to_string(), Color::Rgb(0, 255, 65))
        );
    }

    #[test]
    fn test_runtime_health_state() {
        assert_eq!(
            runtime_health_state(None),
            ("N/A", Color::Rgb(160, 174, 192))
        );

        let idle = RuntimeDiagData::default();
        assert_eq!(
            runtime_health_state(Some(&idle)),
            ("IDLE", Color::Rgb(160, 174, 192))
        );

        let ok = RuntimeDiagData {
            active_run_id: Some("run-1".to_string()),
            heartbeat_age_secs: Some(10),
            ..Default::default()
        };
        assert_eq!(
            runtime_health_state(Some(&ok)),
            ("OK", Color::Rgb(0, 255, 65))
        );

        let stale = RuntimeDiagData {
            active_run_id: Some("run-1".to_string()),
            heartbeat_age_secs: Some(61),
            ..Default::default()
        };
        assert_eq!(
            runtime_health_state(Some(&stale)),
            ("STALE", Color::Rgb(255, 176, 0))
        );

        let warn = RuntimeDiagData {
            active_run_id: Some("run-1".to_string()),
            heartbeat_age_secs: Some(10),
            heartbeat_oom_kill_events: Some(1),
            ..Default::default()
        };
        assert_eq!(
            runtime_health_state(Some(&warn)),
            ("WARN", Color::Rgb(255, 176, 0))
        );
    }

    #[test]
    fn test_consumed_cells_source_helpers() {
        assert_eq!(consumed_cells_source_label("live"), "live");
        assert_eq!(consumed_cells_source_label("sst"), "sst");
        assert_eq!(consumed_cells_source_label("foo"), "unknown");
        assert_eq!(consumed_cells_source_color("live"), Color::Rgb(0, 255, 65));
        assert_eq!(
            consumed_cells_source_color("none"),
            Color::Rgb(160, 174, 192)
        );
    }

    #[test]
    fn test_overview_log_min_height() {
        assert_eq!(overview_log_min_height(), 3);
    }

    #[test]
    fn test_overview_services_min_height() {
        assert_eq!(overview_services_min_height(), 8);
    }

    #[test]
    fn test_compact_overview_layout() {
        assert_eq!(
            compact_overview_layout(Rect::new(0, 0, 100, 31)),
            CompactOverviewLayout::MemoryOnly
        );
        assert_eq!(
            compact_overview_layout(Rect::new(0, 0, 100, 32)),
            CompactOverviewLayout::MemoryAndStorage
        );
    }

    #[test]
    fn test_redis_health_state() {
        let off = RedisServiceInfo::default();
        assert_eq!(redis_health_state(&off), ("OFF", Color::Rgb(160, 174, 192)));

        let down = RedisServiceInfo {
            enabled: true,
            reachable: false,
            ..Default::default()
        };
        assert_eq!(redis_health_state(&down), ("DOWN", Color::Rgb(239, 68, 68)));

        let stale = RedisServiceInfo {
            enabled: true,
            reachable: true,
            sync_progress_age_secs: Some(40),
            ..Default::default()
        };
        assert_eq!(
            redis_health_state(&stale),
            ("STALE", Color::Rgb(255, 176, 0))
        );

        let ok = RedisServiceInfo {
            enabled: true,
            reachable: true,
            sync_progress_age_secs: Some(10),
            ..Default::default()
        };
        assert_eq!(redis_health_state(&ok), ("OK", Color::Rgb(0, 255, 65)));
        assert_eq!(redis_max_key_age(&ok), Some(10));
    }

    #[test]
    fn test_api_health_state() {
        let down = ApiServiceInfo::default();
        assert_eq!(api_health_state(&down), ("DOWN", Color::Rgb(239, 68, 68)));

        let warn_http = ApiServiceInfo {
            reachable: true,
            status_code: Some(503),
            latency_ms: Some(30.0),
            ..Default::default()
        };
        assert_eq!(
            api_health_state(&warn_http),
            ("WARN", Color::Rgb(255, 176, 0))
        );

        let warn_latency = ApiServiceInfo {
            reachable: true,
            status_code: Some(200),
            latency_ms: Some(1800.0),
            ..Default::default()
        };
        assert_eq!(
            api_health_state(&warn_latency),
            ("WARN", Color::Rgb(255, 176, 0))
        );

        let ok = ApiServiceInfo {
            reachable: true,
            status_code: Some(200),
            latency_ms: Some(25.0),
            ..Default::default()
        };
        assert_eq!(api_health_state(&ok), ("OK", Color::Rgb(0, 255, 65)));
    }

    #[test]
    fn test_health_format_helpers() {
        assert_eq!(format_age_secs(None), "-");
        assert_eq!(format_age_secs(Some(12)), "12s");
        assert_eq!(format_ttl(None), "-");
        assert_eq!(format_ttl(Some(-1)), "persist");
        assert_eq!(format_ttl(Some(20)), "20s");
        assert_eq!(format_ratio(None), "-");
        assert_eq!(format_ratio(Some(1.236)), "1.24");
        assert_eq!(format_hit_rate(None, None), "-");
        assert_eq!(format_hit_rate(Some(95), Some(5)), "95.0%");
        assert_eq!(trim_for_panel("abcdef", 0), "");
        assert_eq!(trim_for_panel("abcdef", 6), "...");
        assert_eq!(trim_for_panel("abcdefghijkl", 10), "a...");
    }

    #[test]
    fn test_redis_key_line_format() {
        let line = redis_key_line("sync:status", Some("string"), Some(2048), Some(30), Some(2));
        let text = line_text(&line);
        assert!(text.contains("sync:status"));
        assert!(text.contains("string"));
        assert!(text.contains("2.00 KB"));
        assert!(text.contains("ttl 30s"));
        assert!(text.contains("age 2s"));
    }

    #[test]
    fn test_heartbeat_is_on_every_500ms_with_1s_cycle() {
        assert!(heartbeat_is_on(0));
        assert!(heartbeat_is_on(499));
        assert!(!heartbeat_is_on(500));
        assert!(!heartbeat_is_on(999));
        assert!(heartbeat_is_on(1000));
    }

    #[test]
    fn test_runtime_live_delta_signed_format() {
        assert_eq!(runtime_live_delta(1_005, 1_000), 5);
        assert_eq!(runtime_live_delta(995, 1_000), -5);
        assert_eq!(format_signed_num_i128(5), "+5");
        assert_eq!(format_signed_num_i128(-5), "-5");
    }

    #[test]
    fn test_storage_runtime_columns_live_sync_line() {
        let mem = MemoryStatsData {
            live_cells_count: 1_428_835,
            consumed_cells_count: 93_659_951,
            consumed_cells_bytes: 7_860_000_000,
            consumed_cells_bytes_source: "live".to_string(),
            rocksdb_memtable_bytes: 48_060_000,
            rocksdb_block_cache_bytes: 7_990_000_000,
            rocksdb_table_readers_bytes: 4_920_000,
            rocksdb_total_bytes: 8_050_000_000,
            block_headers_count: 18_663_072,
            total_transactions: 48_551_716,
            total_cells: 95_088_803,
            total_live_cells: 1_428_846,
            total_addresses: 0,
            ..Default::default()
        };

        let (_, _, right) = storage_runtime_columns(&mem);
        let live_line = line_text(&right[2]);
        assert!(live_line.contains("Live (sync)"));
        assert!(live_line.contains("1,428,846"));
        assert!(live_line.contains("Δcache -11"));
    }

    #[test]
    fn test_storage_runtime_columns_mode_and_consumed_source() {
        let mem = MemoryStatsData {
            consumed_cells_bytes: 1_024,
            consumed_cells_bytes_source: "sst".to_string(),
            bulk_sync_mode: true,
            bulk_sync_cell_cache_enabled: false,
            total_addresses: 123,
            ..Default::default()
        };

        let (left, _, right) = storage_runtime_columns(&mem);
        assert!(line_text(&left[2]).contains("src sst"));
        assert!(line_text(&right[3]).contains("mode bulk"));
        assert!(line_text(&right[3]).contains("cache off"));
    }

    #[test]
    fn test_db_lane_health_thresholds() {
        let ok = DbMemoryStatsData {
            l0_files_max: 6,
            compaction_pending_bytes: 2 * 1024 * 1024 * 1024,
            ..Default::default()
        };
        let warn = DbMemoryStatsData {
            l0_files_max: 12,
            ..Default::default()
        };
        let hot = DbMemoryStatsData {
            l0_files_max: 20,
            ..Default::default()
        };

        assert_eq!(db_lane_health(&ok), ("OK", Color::Rgb(0, 255, 65)));
        assert_eq!(db_lane_health(&warn), ("WARN", Color::Rgb(255, 176, 0)));
        assert_eq!(db_lane_health(&hot), ("HOT", Color::Rgb(239, 68, 68)));
    }

    #[test]
    fn test_db_lane_lines_off_when_split_store_not_enabled() {
        let lines = db_lane_lines("HEAVY", None);
        assert!(line_text(&lines[0]).contains("HEAVY"));
        assert!(line_text(&lines[0]).contains("[OFF]"));
        assert!(line_text(&lines[1]).contains("split store disabled"));
    }

    #[test]
    fn test_db_lane_lines_include_top_cf_and_source() {
        let stats = DbMemoryStatsData {
            rocksdb_total_bytes: 8_050_000_000,
            rocksdb_memtable_bytes: 48_000_000,
            l0_files_count: 9,
            l0_files_max: 4,
            compaction_pending_bytes: 2_000_000_000,
            num_running_compactions: 1,
            immutable_memtables: 2,
            live_cells_count: 1_428_835,
            consumed_cells_count: 93_659_951,
            consumed_cells_bytes: 7_860_000_000,
            consumed_cells_bytes_source: "live".to_string(),
            top_cf_sizes: vec![("live_cells".to_string(), 3_000_000_000)],
            ..Default::default()
        };

        let lines = db_lane_lines("CORE", Some(&stats));
        assert!(line_text(&lines[0]).contains("CORE"));
        assert!(line_text(&lines[0]).contains("[OK]"));
        assert!(line_text(&lines[2]).contains("src live"));
        assert!(line_text(&lines[3]).contains("top live_cells"));
    }
}
