use chrono::{DateTime, Local};
use ckbadger_common::{BulkBuildProgressData, MemoryStatsData};
use ckbadger_store::{APPEND_CFS, DOMAIN_CFS};
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
    ApiServiceInfo, ChainInfoData, RuntimeDiagData, ServiceLogTailData, SupervisorServiceData,
    SyncStatusRow, TuiDb,
};

const RATE_HISTORY_SIZE: usize = 3600;
const LOG_HISTORY_SIZE: usize = 200;
const RATE_DROP_RATIO_THRESHOLD: f64 = 0.65;
const STATUS_MESSAGE_TTL_SECS: u64 = 8;
const STATUS_MESSAGE_WARM_SECS: u64 = 5;
const WARNING_DEDUP_WINDOW_SECS: u64 = 30;

const TERMINAL_GREEN: Color = Color::Rgb(0, 255, 65);
const TERMINAL_DIM: Color = Color::Rgb(0, 204, 51);
const AMBER: Color = Color::Rgb(255, 176, 0);
const CYAN: Color = Color::Rgb(56, 189, 248);
const SLATE_800: Color = Color::Rgb(58, 71, 89);
const SLATE_700: Color = Color::Rgb(80, 95, 115);
const SLATE_500: Color = Color::Rgb(160, 174, 192);
const FOREGROUND: Color = Color::Rgb(237, 237, 237);
const ERROR_RED: Color = Color::Rgb(239, 68, 68);
const L0_GAUGE_MAX: u64 = 20;
const P95_WINDOW: usize = 300;
const P95_MIN_WIDTH: u16 = 40;
const BULK_BUILD_MIN_SPAN_K: u64 = 10;
const BULK_BUILD_MAX_SPAN_K: u64 = 100;

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
    System,
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

pub struct App {
    build_version: String,
    db: TuiDb,
    sync_status: Option<SyncStatusRow>,
    memory_stats: Option<MemoryStatsData>,
    chain_info: Option<ChainInfoData>,
    api_service: ApiServiceInfo,
    runtime_diag: Option<RuntimeDiagData>,
    supervisor_services: Option<Vec<SupervisorServiceData>>,
    service_log_tails: Option<Vec<ServiceLogTailData>>,
    last_refresh: Instant,
    last_sample: Instant,
    status_message: Option<(String, Instant)>,
    last_warning: Option<(String, Instant)>,
    rate_history: VecDeque<f64>,
    tx_rate_history: VecDeque<f64>,
    db_write_history: VecDeque<f64>,
    db_commit_history: VecDeque<f64>,
    fetch_stage_history: VecDeque<f64>,
    parse_stage_history: VecDeque<f64>,
    write_stage_history: VecDeque<f64>,
    bulk_build_ms_history: VecDeque<f64>,
    bulk_fetch_ms_history: VecDeque<f64>,
    fetch_overlap_history: VecDeque<f64>,
    flush_overlap_history: VecDeque<f64>,
    idle_ratio_history: VecDeque<f64>,
    l0_files_history: VecDeque<f64>,
    last_overlap_batch_count: u64,
    show_build_subphases: bool,
    log_entries: VecDeque<LogEntry>,
    sync_event_entries: VecDeque<LogEntry>,
    log_scroll: usize,
    sync_event_scroll: usize,
    main_tab: MainTab,
    prev_is_bulk_sync: Option<bool>,
    prev_is_syncing: Option<bool>,
    prev_pipeline_reset_epoch: Option<u64>,
    prev_bottleneck: Option<SyncBottleneck>,
    prev_adaptive_last_reason: Option<String>,
    last_rate_drop_alert: Option<Instant>,
    last_tx_rate_drop_alert: Option<Instant>,
    stale_warning_active: bool,
    help_visible: bool,
    force_compact_layout: bool,
    diagnostics_view_mode: DiagnosticsViewMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncBottleneck {
    WriteBound,
    FetchBound,
    Mixed,
    Unknown,
}

impl App {
    pub fn new(db: TuiDb, build_version: String) -> Self {
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
            build_version,
            db,
            sync_status: None,
            memory_stats: None,
            chain_info: None,
            api_service: ApiServiceInfo::default(),
            runtime_diag: None,
            supervisor_services: None,
            service_log_tails: None,
            last_refresh: Instant::now(),
            last_sample: Instant::now(),
            status_message: None,
            last_warning: None,
            rate_history: VecDeque::with_capacity(RATE_HISTORY_SIZE),
            tx_rate_history: VecDeque::with_capacity(RATE_HISTORY_SIZE),
            db_write_history: VecDeque::with_capacity(RATE_HISTORY_SIZE),
            db_commit_history: VecDeque::with_capacity(RATE_HISTORY_SIZE),
            fetch_stage_history: VecDeque::with_capacity(RATE_HISTORY_SIZE),
            parse_stage_history: VecDeque::with_capacity(RATE_HISTORY_SIZE),
            write_stage_history: VecDeque::with_capacity(RATE_HISTORY_SIZE),
            bulk_build_ms_history: VecDeque::with_capacity(RATE_HISTORY_SIZE),
            bulk_fetch_ms_history: VecDeque::with_capacity(RATE_HISTORY_SIZE),
            fetch_overlap_history: VecDeque::with_capacity(RATE_HISTORY_SIZE),
            flush_overlap_history: VecDeque::with_capacity(RATE_HISTORY_SIZE),
            idle_ratio_history: VecDeque::with_capacity(RATE_HISTORY_SIZE),
            l0_files_history: VecDeque::with_capacity(RATE_HISTORY_SIZE),
            last_overlap_batch_count: 0,
            show_build_subphases: false,
            log_entries,
            sync_event_entries,
            log_scroll: 0,
            sync_event_scroll: 0,
            main_tab: MainTab::default(),
            prev_is_bulk_sync: None,
            prev_is_syncing: None,
            prev_pipeline_reset_epoch: None,
            prev_bottleneck: None,
            prev_adaptive_last_reason: None,
            last_rate_drop_alert: None,
            last_tx_rate_drop_alert: None,
            stale_warning_active: false,
            help_visible: false,
            force_compact_layout: false,
            diagnostics_view_mode: DiagnosticsViewMode::Auto,
        }
    }

    pub fn db(&self) -> &TuiDb {
        &self.db
    }

    pub fn next_tab(&mut self) {
        self.main_tab = match self.main_tab {
            MainTab::Overview => MainTab::Sync,
            MainTab::Sync => MainTab::System,
            MainTab::System => MainTab::Overview,
        };
    }

    pub fn previous_tab(&mut self) {
        self.main_tab = match self.main_tab {
            MainTab::Overview => MainTab::System,
            MainTab::Sync => MainTab::Overview,
            MainTab::System => MainTab::Sync,
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

    pub fn toggle_build_subphases(&mut self) {
        self.show_build_subphases = !self.show_build_subphases;
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
            MainTab::System => {}
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
            MainTab::System => {}
        }
    }

    pub fn scroll_log_to_bottom(&mut self) {
        match self.main_tab {
            MainTab::Overview => self.log_scroll = 0,
            MainTab::Sync => self.sync_event_scroll = 0,
            MainTab::System => {}
        }
    }

    pub fn scroll_log_to_top(&mut self) {
        match self.main_tab {
            MainTab::Overview => self.log_scroll = self.log_entries.len().saturating_sub(1),
            MainTab::Sync => {
                self.sync_event_scroll = self.sync_event_entries.len().saturating_sub(1)
            }
            MainTab::System => {}
        }
    }

    pub async fn refresh(&mut self) {
        let (
            (sync_status_result, memory_stats, runtime_diag),
            (chain_info, api_service),
            services,
            log_tails,
        ) = tokio::join!(
            self.db.get_local_snapshot(),
            self.db.get_chain_info_and_api_service_info(),
            self.db.get_supervisor_services(),
            self.db.get_service_log_tails(),
        );

        match sync_status_result {
            Ok(status) => self.sync_status = Some(status),
            Err(e) => {
                self.sync_status = None;
                self.log_warning(format!("Failed to load sync status: {e}"));
            }
        }

        self.memory_stats = memory_stats;
        self.chain_info = chain_info;
        self.api_service = api_service;
        self.runtime_diag = runtime_diag;
        self.supervisor_services = services;
        self.service_log_tails = log_tails;
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
        let db_commit_ms = self
            .sync_status
            .as_ref()
            .and_then(|s| {
                s.pipeline
                    .as_ref()
                    .and_then(|p| p.commit_ms)
                    .or(s.db_commit_ms)
            })
            .unwrap_or(0.0);
        push_history_sample(&mut self.db_commit_history, db_commit_ms);

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

        let (bulk_build_ms, bulk_fetch_ms) = self
            .sync_status
            .as_ref()
            .and_then(|s| s.bulk_build.as_ref())
            .map(|bb| (bb.build_ms.unwrap_or(0.0), bb.fetch_ms.unwrap_or(0.0)))
            .unwrap_or((0.0, 0.0));
        push_history_sample(&mut self.bulk_build_ms_history, bulk_build_ms);
        push_history_sample(&mut self.bulk_fetch_ms_history, bulk_fetch_ms);

        // Sample overlap ratios (deduped per batch).
        if let Some(bb) = self
            .sync_status
            .as_ref()
            .and_then(|s| s.bulk_build.as_ref())
        {
            let batch_count = bb.batch_count.unwrap_or(0);
            if batch_count > 0 && batch_count != self.last_overlap_batch_count {
                let fetch_ms = bb.fetch_ms.unwrap_or(0.0);
                let prefetch_collect_ms = bb.prefetch_collect_ms.unwrap_or(0.0);
                let flush_ms = bb.flush_ms.unwrap_or(0.0);
                let flush_wait_ms = bb.flush_wait_ms.unwrap_or(0.0);
                let build_ms = bb.build_ms.unwrap_or(0.0);

                let fetch_overlap = if fetch_ms > 0.0 {
                    (1.0 - prefetch_collect_ms / fetch_ms).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                let flush_overlap = if flush_ms > 0.0 {
                    (1.0 - flush_wait_ms / flush_ms).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                let iteration_ms = build_ms + prefetch_collect_ms + flush_wait_ms;
                let idle_ratio = if iteration_ms > 0.0 {
                    (prefetch_collect_ms + flush_wait_ms) / iteration_ms
                } else {
                    0.0
                };

                push_history_sample(&mut self.fetch_overlap_history, fetch_overlap);
                push_history_sample(&mut self.flush_overlap_history, flush_overlap);
                push_history_sample(&mut self.idle_ratio_history, idle_ratio);
                self.last_overlap_batch_count = batch_count;
            }
        }

        let l0_files = self
            .memory_stats
            .as_ref()
            .map(|m| m.l0_files_count as f64)
            .unwrap_or(0.0);
        push_history_sample(&mut self.l0_files_history, l0_files);

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
        let pipeline_reset_epoch = sync.pipeline_reset_epoch;
        let pipeline_reset_reason = sync
            .pipeline_reset_reason
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let bottleneck = sync_bottleneck(sync.db_write_ms, sync.rpc_fetch_ms);
        let adaptive_last_reason = sync.adaptive_last_reason.clone();

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
    }

    fn detect_stale_state(&mut self) {
        let stale_secs = stale_age_secs(self.memory_stats.as_ref());
        let stale_now = stale_secs.is_some_and(|secs| secs > 30);
        if let Some(secs) = stale_secs {
            if stale_now && !self.stale_warning_active {
                self.push_sync_event_and_log(
                    format!("sync data is stale ({}s)", secs),
                    LogLevel::Warning,
                );
            } else if !stale_now && self.stale_warning_active {
                self.push_sync_event_and_log(
                    "sync data freshness recovered".to_string(),
                    LogLevel::Success,
                );
            }
        }
        self.stale_warning_active = stale_now;
    }

    fn log_warning(&mut self, message: String) {
        if self.last_warning.as_ref().is_some_and(|(prev, ts)| {
            prev == &message && ts.elapsed().as_secs() < WARNING_DEDUP_WINDOW_SECS
        }) {
            return;
        }
        self.last_warning = Some((message.clone(), Instant::now()));
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
    let footer_height = if footer_status_message(app.status_message.as_ref()).is_some() {
        4
    } else {
        3
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(footer_height),
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
    let stale_secs = stale_age_secs(app.memory_stats.as_ref());
    let right = Paragraph::new(header_right_line(
        &app.build_version,
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

fn header_right_line(version: &str, stale_secs: Option<i64>, clock_text: &str) -> Line<'static> {
    let (stale_text, stale_color) = stale_status(stale_secs);
    Line::from(vec![
        Span::styled(version.to_string(), Style::default().fg(SLATE_500)),
        Span::styled(" │ ", Style::default().fg(SLATE_700)),
        Span::styled(stale_text, Style::default().fg(stale_color)),
        Span::styled(" │ ", Style::default().fg(SLATE_700)),
        Span::styled(clock_text.to_string(), Style::default().fg(FOREGROUND)),
    ])
}

fn stale_age_secs(memory_stats: Option<&MemoryStatsData>) -> Option<i64> {
    let m = memory_stats?;
    if m.updated_at <= 0 {
        return None;
    }
    Some((chrono::Utc::now().timestamp() - m.updated_at).max(0))
}

fn stale_status(stale_secs: Option<i64>) -> (String, Color) {
    match stale_secs {
        Some(secs) if secs > 30 => (format!("stale {secs}s"), AMBER),
        Some(secs) => (format!("stale {secs}s"), TERMINAL_DIM),
        None => ("stale N/A".to_string(), SLATE_500),
    }
}

fn draw_tabs(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SLATE_800));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let active_style = Style::default()
        .fg(Color::Black)
        .bg(TERMINAL_GREEN)
        .add_modifier(Modifier::BOLD);
    let inactive_style = Style::default().fg(SLATE_500);

    let overview_style = if app.main_tab == MainTab::Overview {
        active_style
    } else {
        inactive_style
    };
    let sync_style = if app.main_tab == MainTab::Sync {
        active_style
    } else {
        inactive_style
    };
    let system_style = if app.main_tab == MainTab::System {
        active_style
    } else {
        inactive_style
    };

    let mut spans = vec![
        Span::styled(" Tabs: ", Style::default().fg(SLATE_500)),
        Span::styled(" Overview ", overview_style),
        Span::styled("  ", Style::default().fg(SLATE_700)),
        Span::styled(" Sync ", sync_style),
        Span::styled("  ", Style::default().fg(SLATE_700)),
        Span::styled(" System ", system_style),
    ];

    // Layout/diagnostics indicators only relevant on Overview and Sync tabs
    if app.main_tab != MainTab::System {
        spans.push(Span::styled(
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
        ));
        spans.push(Span::styled("  ", Style::default().fg(SLATE_700)));
        spans.push(Span::styled(
            format!(
                "[Diag:{}]",
                diagnostics_view_mode_label(app.diagnostics_view_mode)
            ),
            Style::default().fg(diagnostics_view_mode_color(app.diagnostics_view_mode)),
        ));
    }

    spans.push(Span::styled("  [Tab/s]", Style::default().fg(SLATE_500)));

    f.render_widget(Paragraph::new(Line::from(spans)), inner);
}

fn draw_content(f: &mut Frame, app: &App, area: Rect) {
    match app.main_tab {
        MainTab::Overview => draw_overview_content(f, app, area),
        MainTab::Sync => draw_sync_content(f, app, area),
        MainTab::System => draw_system_content(f, app, area),
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
    let is_bulk_sync = app.sync_status.as_ref().is_some_and(|s| s.is_bulk_sync);
    let is_bulk_build = app
        .sync_status
        .as_ref()
        .and_then(|s| s.bulk_build.as_ref())
        .is_some();
    let progress_height: u16 = if is_bulk_sync { 8 } else { 7 };

    match detect_layout_density(app, area) {
        LayoutDensity::Compact => match compact_sync_layout(area) {
            CompactSyncLayout::DiagnosticsOnly => {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(4),
                        Constraint::Length(progress_height),
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
                        Constraint::Length(progress_height),
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
            // Bulk-build diagnostics has 10 lines (stages + I/O + volume + adaptive);
            // allocate 12 (10 inner + 2 border) to avoid clipping.
            let diag_height: u16 = if is_bulk_build {
                if app.show_build_subphases {
                    16
                } else {
                    12
                }
            } else {
                6
            };
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(4),
                    Constraint::Length(progress_height),
                    Constraint::Length(10),
                    Constraint::Length(diag_height),
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
            let diag_height: u16 = if is_bulk_build {
                if app.show_build_subphases {
                    16
                } else {
                    12
                }
            } else {
                8
            };
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(4),
                    Constraint::Length(progress_height),
                    Constraint::Length(10),
                    Constraint::Length(diag_height),
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

    let stale_secs = stale_age_secs(app.memory_stats.as_ref());
    let (stale_text, stale_color) = stale_status(stale_secs);
    let stale_style = Style::default().fg(stale_color);

    let heartbeat_on = heartbeat_is_on(app.last_refresh.elapsed().as_millis());
    let heartbeat = if heartbeat_on { "●" } else { "○" };
    let heartbeat_color = if app.last_refresh.elapsed().as_secs() <= 2 {
        TERMINAL_GREEN
    } else {
        AMBER
    };

    let mut spans = vec![
        Span::styled(heartbeat, Style::default().fg(heartbeat_color)),
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
    ];
    if sync.is_direct_db_read {
        spans.push(Span::styled(
            " [DB]",
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        ));
    }
    spans.extend([
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
        Span::styled(" | stale ", Style::default().fg(SLATE_500)),
        Span::styled(stale_text, stale_style),
    ]);
    let line = Line::from(spans);
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
    draw_kpi_cell(f, cols[4], "Mode", &mode, TERMINAL_GREEN);

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

    let mut left = vec![Line::from(vec![Span::styled(
        format!(" {} ", mode),
        Style::default().fg(Color::Black).bg(mode_color),
    )])];

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
            Span::styled("Blk Now: ", Style::default().fg(SLATE_500)),
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
            Span::styled("Blk EMA: ", Style::default().fg(SLATE_500)),
            if let Some(ema) = sync.rate_ema {
                Span::raw(format!("{ema:.0} blk/s"))
            } else {
                Span::styled("-", Style::default().fg(SLATE_500))
            },
        ]),
        Line::from(vec![
            Span::styled("Tx Now:  ", Style::default().fg(SLATE_500)),
            if let Some(rt) = sync.tx_rate_realtime {
                Span::styled(
                    format!("{rt:.0} tx/s"),
                    Style::default()
                        .fg(TERMINAL_GREEN)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled("-", Style::default().fg(SLATE_500))
            },
        ]),
        Line::from(vec![
            Span::styled("Tx EMA:  ", Style::default().fg(SLATE_500)),
            if let Some(ema) = sync.tx_rate_ema {
                Span::raw(format!("{ema:.0} tx/s"))
            } else {
                Span::styled("-", Style::default().fg(SLATE_500))
            },
        ]),
    ];
    f.render_widget(Paragraph::new(mid), cols[1]);

    let right = sync_timing_lines(
        sync.eta.as_deref(),
        sync.eta_seconds,
        sync.elapsed_time.as_deref(),
        sync.startup_phase.as_deref(),
        sync.is_bulk_sync,
    );
    f.render_widget(Paragraph::new(right), cols[2]);

    if sync.is_bulk_sync {
        if let Some(mem) = &app.memory_stats {
            let remaining_height = inner.height.saturating_sub(5);
            if remaining_height >= 1 {
                let synced_area = Rect {
                    x: inner.x,
                    y: inner.y + inner.height.saturating_sub(1),
                    width: inner.width,
                    height: 1,
                };
                let mut spans = vec![
                    Span::styled("Synced ", Style::default().fg(SLATE_500)),
                    Span::styled(
                        format!(
                            "Txs {}  Cells {}",
                            format_num(mem.total_transactions),
                            format_num(mem.total_cells)
                        ),
                        Style::default().fg(FOREGROUND),
                    ),
                ];
                if mem.total_addresses > 0 {
                    spans.push(Span::styled(
                        format!("  Addrs {}", format_num(mem.total_addresses)),
                        Style::default().fg(FOREGROUND),
                    ));
                }
                if mem.sst_files_size > 0 {
                    spans.push(Span::styled(
                        format!("  SST {}", format_bytes(mem.sst_files_size)),
                        Style::default().fg(SLATE_500),
                    ));
                }
                f.render_widget(Paragraph::new(Line::from(spans)), synced_area);
            }
        }
    }
}

fn draw_sync_charts(f: &mut Frame, app: &App, area: Rect) {
    let is_bulk_build = app
        .sync_status
        .as_ref()
        .and_then(|s| s.bulk_build.as_ref())
        .is_some();
    if stack_sync_charts(area) {
        let specs = sync_chart_specs(true);
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
        let base_specs = sync_chart_specs(false);
        let third_spec = if is_bulk_build {
            SyncChartSpec {
                title: "Build Latency (ms)",
                unit: "ms",
                kind: SyncChartKind::BuildLatency,
            }
        } else {
            base_specs[2]
        };
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(34),
                Constraint::Percentage(33),
                Constraint::Percentage(33),
            ])
            .split(area);
        draw_chart_panel(
            f,
            cols[0],
            base_specs[0].title,
            base_specs[0].unit,
            sync_chart_data(app, base_specs[0].kind),
        );
        draw_chart_panel(
            f,
            cols[1],
            base_specs[1].title,
            base_specs[1].unit,
            sync_chart_data(app, base_specs[1].kind),
        );
        draw_chart_panel(
            f,
            cols[2],
            third_spec.title,
            third_spec.unit,
            sync_chart_data(app, third_spec.kind),
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncChartKind {
    BlockRate,
    TxRate,
    WriteLatency,
    BuildLatency,
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
        title: "Write Stage Latency (ms)",
        unit: "ms",
        kind: SyncChartKind::WriteLatency,
    },
];

fn sync_chart_specs(stacked: bool) -> &'static [SyncChartSpec] {
    if stacked {
        &STACKED_SYNC_CHART_SPECS
    } else {
        &WIDE_SYNC_CHART_SPECS
    }
}

fn sync_chart_data(app: &App, kind: SyncChartKind) -> &VecDeque<f64> {
    match kind {
        SyncChartKind::BlockRate => &app.rate_history,
        SyncChartKind::TxRate => &app.tx_rate_history,
        SyncChartKind::WriteLatency => &app.db_write_history,
        SyncChartKind::BuildLatency => &app.bulk_build_ms_history,
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

    let fetch_ms_text = sync
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

    let is_bulk_build = sync.bulk_build.is_some();

    if is_bulk_build {
        // 3-column layout for bulk build
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(38),
                Constraint::Percentage(30),
                Constraint::Percentage(32),
            ])
            .split(inner);

        if let Some(bb) = sync.bulk_build.as_ref() {
            let (left, middle) = build_bulk_build_diagnostics(
                bb,
                app,
                &cols,
                &rate_jitter_text,
                &eta_conf,
                dense_panel,
            );
            f.render_widget(Paragraph::new(left), cols[0]);
            f.render_widget(Paragraph::new(middle), cols[1]);
            draw_overlap_column(f, app, bb, cols[2]);
        }
    } else {
        // 2-column layout for pipeline and idle modes
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(52), Constraint::Percentage(48)])
            .split(inner);

        let (left, right) = if let Some(pipeline) = sync.pipeline.as_ref() {
            let pipeline_write_stage_ms = pipeline.write_ms.or(sync.db_write_ms);
            let pipeline_write_stage_ms_text = pipeline_write_stage_ms
                .map(|v| format!("{v:.1}ms"))
                .unwrap_or_else(|| "-".to_string());
            let pipeline_commit_ms = pipeline.commit_ms.or(sync.db_commit_ms);
            let pipeline_commit_ms_text = pipeline_commit_ms
                .map(|v| format!("{v:.1}ms"))
                .unwrap_or_else(|| "-".to_string());
            let pipeline_gap_ms_text =
                format_stage_commit_gap_ms(pipeline_write_stage_ms, pipeline_commit_ms);
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
            let adaptive_inflight_batches =
                match (pipeline.fetch_queue_depth, pipeline.parse_queue_depth) {
                    (Some(fetch_depth), Some(parse_depth)) => Some(fetch_depth + parse_depth),
                    _ => None,
                };

            let spark_width = cols[1].width.saturating_sub(14).clamp(8, 24) as usize;
            let (left, right) = if dense_panel {
                (
                    {
                        let mut left = vec![
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
                                Span::styled("Commit ", Style::default().fg(SLATE_500)),
                                Span::styled(
                                    pipeline_commit_ms_text.clone(),
                                    Style::default().fg(FOREGROUND),
                                ),
                                Span::styled("  ", Style::default().fg(SLATE_700)),
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
                        ];
                        let adaptive = adaptive_control_lines(
                            AdaptiveControlSnapshot {
                                last_batch_blocks: sync.last_batch_blocks,
                                adaptive_inflight_batches,
                                adaptive_target_batch_txs: sync.adaptive_target_batch_txs,
                                adaptive_inflight_limit: sync.adaptive_inflight_limit,
                                adaptive_min_target_batch_txs: sync.adaptive_min_target_batch_txs,
                                adaptive_cooldown_steps: sync.adaptive_cooldown_steps,
                                adaptive_last_reason: sync.adaptive_last_reason.as_deref(),
                                adaptive_adjustment_seq: sync.adaptive_adjustment_seq,
                                adaptive_last_adjusted_age_secs: sync
                                    .adaptive_last_adjusted_age_secs,
                                adaptive_backoff_streak: sync.adaptive_backoff_streak,
                            },
                            dense_panel,
                            sync.pipeline_reset_epoch,
                            sync.pipeline_reset_reason.as_deref(),
                        );
                        left.extend(adaptive);
                        left
                    },
                    {
                        let show_p95 = cols[1].width >= P95_MIN_WIDTH;
                        let gauge_width = (cols[1].width / 4).clamp(6, 12) as usize;
                        let spark_fp = merged_sparkline_p95_line(
                            "F",
                            TERMINAL_DIM,
                            &app.fetch_stage_history,
                            "P",
                            AMBER,
                            &app.parse_stage_history,
                            spark_width,
                            show_p95,
                        );
                        let spark_wc = merged_sparkline_p95_line(
                            "W",
                            TERMINAL_GREEN,
                            &app.write_stage_history,
                            "C",
                            CYAN,
                            &app.db_commit_history,
                            spark_width,
                            show_p95,
                        );
                        let (l0_line, wbm_line, pressure_line) = if let Some(mem) =
                            &app.memory_stats
                        {
                            (
                                storage_pressure_l0_line(mem, gauge_width),
                                storage_pressure_wbm_line(mem, gauge_width),
                                storage_pressure_summary_line(mem),
                            )
                        } else {
                            (
                                Line::from(Span::styled("L0 -", Style::default().fg(SLATE_500))),
                                Line::from(Span::styled("WBM -", Style::default().fg(SLATE_500))),
                                Line::from(Span::styled(
                                    "Compact -",
                                    Style::default().fg(SLATE_500),
                                )),
                            )
                        };
                        dense_right_lines(
                            spark_fp,
                            spark_wc,
                            l0_line,
                            wbm_line,
                            pressure_line,
                            Line::from(vec![
                                Span::styled("Stability ", Style::default().fg(SLATE_500)),
                                Span::styled(stability, Style::default().fg(stability_color)),
                                Span::styled("  jitter ", Style::default().fg(SLATE_500)),
                                Span::styled(
                                    rate_jitter_text.to_string(),
                                    Style::default().fg(AMBER),
                                ),
                            ]),
                        )
                    },
                )
            } else {
                (
                    {
                        let mut left = vec![
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
                                Span::styled("  Commit ", Style::default().fg(SLATE_500)),
                                Span::styled(
                                    pipeline_commit_ms_text.clone(),
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
                        ];
                        let adaptive = adaptive_control_lines(
                            AdaptiveControlSnapshot {
                                last_batch_blocks: sync.last_batch_blocks,
                                adaptive_inflight_batches,
                                adaptive_target_batch_txs: sync.adaptive_target_batch_txs,
                                adaptive_inflight_limit: sync.adaptive_inflight_limit,
                                adaptive_min_target_batch_txs: sync.adaptive_min_target_batch_txs,
                                adaptive_cooldown_steps: sync.adaptive_cooldown_steps,
                                adaptive_last_reason: sync.adaptive_last_reason.as_deref(),
                                adaptive_adjustment_seq: sync.adaptive_adjustment_seq,
                                adaptive_last_adjusted_age_secs: sync
                                    .adaptive_last_adjusted_age_secs,
                                adaptive_backoff_streak: sync.adaptive_backoff_streak,
                            },
                            false,
                            sync.pipeline_reset_epoch,
                            sync.pipeline_reset_reason.as_deref(),
                        );
                        left.extend(adaptive);
                        left
                    },
                    {
                        let show_p95 = cols[1].width >= P95_MIN_WIDTH;
                        let gauge_width = (cols[1].width / 4).clamp(6, 12) as usize;
                        let spark_fp = merged_sparkline_p95_line(
                            "F",
                            TERMINAL_DIM,
                            &app.fetch_stage_history,
                            "P",
                            AMBER,
                            &app.parse_stage_history,
                            spark_width,
                            show_p95,
                        );
                        let spark_wc = merged_sparkline_p95_line(
                            "W",
                            TERMINAL_GREEN,
                            &app.write_stage_history,
                            "C",
                            CYAN,
                            &app.db_commit_history,
                            spark_width,
                            show_p95,
                        );
                        let (l0_line, wbm_line, pressure_line) = if let Some(mem) =
                            &app.memory_stats
                        {
                            (
                                storage_pressure_l0_line(mem, gauge_width),
                                storage_pressure_wbm_line(mem, gauge_width),
                                storage_pressure_summary_line(mem),
                            )
                        } else {
                            (
                                Line::from(Span::styled("L0 -", Style::default().fg(SLATE_500))),
                                Line::from(Span::styled("WBM -", Style::default().fg(SLATE_500))),
                                Line::from(Span::styled(
                                    "Compact -",
                                    Style::default().fg(SLATE_500),
                                )),
                            )
                        };
                        detail_right_lines(
                            spark_fp,
                            spark_wc,
                            l0_line,
                            wbm_line,
                            pressure_line,
                            Line::from(vec![
                                Span::styled("Stability ", Style::default().fg(SLATE_500)),
                                Span::styled(stability, Style::default().fg(stability_color)),
                                Span::styled("  ETA ", Style::default().fg(SLATE_500)),
                                Span::styled(eta_conf.0, Style::default().fg(eta_conf.1)),
                            ]),
                            io_fetch_write_jitter_line(
                                &fetch_ms_text,
                                &pipeline_write_stage_ms_text,
                                &pipeline_commit_ms_text,
                                &pipeline_gap_ms_text,
                                &rate_jitter_text,
                            ),
                        )
                    },
                )
            };

            (left, right)
        } else {
            (
                {
                    let mut left = vec![
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
                    ];
                    let adaptive = adaptive_control_lines(
                        AdaptiveControlSnapshot {
                            last_batch_blocks: sync.last_batch_blocks,
                            adaptive_inflight_batches: None,
                            adaptive_target_batch_txs: sync.adaptive_target_batch_txs,
                            adaptive_inflight_limit: sync.adaptive_inflight_limit,
                            adaptive_min_target_batch_txs: sync.adaptive_min_target_batch_txs,
                            adaptive_cooldown_steps: sync.adaptive_cooldown_steps,
                            adaptive_last_reason: sync.adaptive_last_reason.as_deref(),
                            adaptive_adjustment_seq: sync.adaptive_adjustment_seq,
                            adaptive_last_adjusted_age_secs: sync.adaptive_last_adjusted_age_secs,
                            adaptive_backoff_streak: sync.adaptive_backoff_streak,
                        },
                        dense_panel,
                        sync.pipeline_reset_epoch,
                        sync.pipeline_reset_reason.as_deref(),
                    );
                    left.extend(adaptive);
                    left
                },
                vec![
                    {
                        let write_stage_ms_text = sync
                            .db_write_ms
                            .map(|v| format!("{v:.1}ms"))
                            .unwrap_or_else(|| "-".to_string());
                        let write_commit_ms_text = sync
                            .db_commit_ms
                            .map(|v| format!("{v:.1}ms"))
                            .unwrap_or_else(|| "-".to_string());
                        let write_gap_ms_text =
                            format_stage_commit_gap_ms(sync.db_write_ms, sync.db_commit_ms);
                        Line::from(vec![
                            Span::styled("I/O ", Style::default().fg(SLATE_500)),
                            Span::styled(
                                format!(
                                    "Fetch {} Write(stage) {} Commit {} Gap {}",
                                    fetch_ms_text,
                                    write_stage_ms_text,
                                    write_commit_ms_text,
                                    write_gap_ms_text
                                ),
                                Style::default().fg(FOREGROUND),
                            ),
                        ])
                    },
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
}

fn io_fetch_write_jitter_line(
    fetch_ms_text: &str,
    write_stage_ms_text: &str,
    write_commit_ms_text: &str,
    write_gap_ms_text: &str,
    rate_jitter_text: &str,
) -> Line<'static> {
    Line::from(vec![
        Span::styled("I/O ", Style::default().fg(SLATE_500)),
        Span::styled(
            format!(
                "Fetch {} Write(stage) {} Commit {} Gap {}",
                fetch_ms_text, write_stage_ms_text, write_commit_ms_text, write_gap_ms_text
            ),
            Style::default().fg(FOREGROUND),
        ),
        Span::styled("  jitter ", Style::default().fg(SLATE_500)),
        Span::styled(rate_jitter_text.to_string(), Style::default().fg(AMBER)),
    ])
}

/// Build a single overlap sparkline line for the diagnostics right column.
fn overlap_sparkline_line(
    label: &str,
    history: &VecDeque<f64>,
    spark_width: usize,
    inverted: bool,
) -> Line<'static> {
    let sparkline_chars: [char; 8] = [
        '\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}',
        '\u{2588}',
    ];

    let visible: Vec<f64> = if history.len() > spark_width {
        history
            .iter()
            .skip(history.len() - spark_width)
            .copied()
            .collect()
    } else {
        history.iter().copied().collect()
    };

    let mut spans: Vec<Span<'static>> = Vec::new();

    // Label
    spans.push(Span::styled(
        format!("{:<6}", label),
        Style::default().fg(SLATE_500),
    ));

    // Pad
    let pad = spark_width.saturating_sub(visible.len());
    if pad > 0 {
        spans.push(Span::raw(" ".repeat(pad)));
    }

    // Sparkline chars
    for &val in &visible {
        let idx = ((val * 7.0).round() as usize).min(7);
        let ch = sparkline_chars[idx];
        let color = if inverted {
            if val < 0.2 {
                TERMINAL_GREEN
            } else if val < 0.5 {
                AMBER
            } else {
                ERROR_RED
            }
        } else if val > 0.8 {
            TERMINAL_GREEN
        } else if val > 0.5 {
            AMBER
        } else {
            ERROR_RED
        };
        spans.push(Span::styled(ch.to_string(), Style::default().fg(color)));
    }

    // Stats
    let current = visible
        .last()
        .copied()
        .unwrap_or(if inverted { 0.0 } else { 1.0 });
    let avg = if visible.is_empty() {
        0.0
    } else {
        visible.iter().sum::<f64>() / visible.len() as f64
    };
    let stats_color = if inverted {
        if current < 0.2 {
            TERMINAL_GREEN
        } else {
            AMBER
        }
    } else if current > 0.8 {
        TERMINAL_GREEN
    } else {
        AMBER
    };
    spans.push(Span::styled(
        format!(" {:>3.0}% {:>3.0}%", current * 100.0, avg * 100.0),
        Style::default().fg(stats_color),
    ));

    Line::from(spans)
}

/// Renders the pipeline overlap column: Gantt timeline + overlap sparklines.
fn draw_overlap_column(f: &mut Frame, app: &App, bb: &BulkBuildProgressData, area: Rect) {
    let build_ms = bb.build_ms.unwrap_or(0.0);
    let prefetch_collect_ms = bb.prefetch_collect_ms.unwrap_or(0.0);
    let flush_wait_ms = bb.flush_wait_ms.unwrap_or(0.0);
    let fetch_ms = bb.fetch_ms.unwrap_or(0.0);
    let flush_ms = bb.flush_ms.unwrap_or(0.0);
    let batch_count = bb.batch_count.unwrap_or(0);
    let iteration_ms = build_ms + prefetch_collect_ms + flush_wait_ms;

    let mut lines: Vec<Line<'static>> = Vec::new();

    // -- Gantt timeline --
    if iteration_ms > 0.0 && build_ms > 0.0 {
        let visible_ms = iteration_ms + flush_ms;
        if visible_ms > 0.0 {
            let label_width = 7usize; // "ACTVTY " = 6 chars + space
            let bar_width = (area.width as usize).saturating_sub(label_width + 8); // 8 for "  NNNms"

            // Header
            lines.push(Line::from(Span::styled(
                format!("Batch #{:<4} ({:.0}ms)", batch_count, iteration_ms),
                Style::default().fg(FOREGROUND),
            )));

            // Helper: map ms to column count
            let col = |ms: f64| -> usize {
                ((ms / visible_ms * bar_width as f64).round() as usize).min(bar_width)
            };

            if app.show_build_subphases {
                let facts_ms = bb.facts_ms.unwrap_or(0.0);
                let resolve_ms = bb.resolve_ms.unwrap_or(0.0);
                let reduce_ms = bb.reduce_ms.unwrap_or(0.0);
                let history_ms = bb.history_ms.unwrap_or(0.0);
                let addr_ms = bb.address_reduce_ms.unwrap_or(0.0);
                let actvty_ms = bb.activity_stats_ms.unwrap_or(0.0);

                let sub_phases: &[(&str, f64, Color)] = &[
                    ("FACTS", facts_ms, AMBER),
                    ("RESOLV", resolve_ms, Color::Magenta),
                    ("REDUCE", reduce_ms, Color::Blue),
                    ("HIST", history_ms, TERMINAL_GREEN),
                    ("ADDR", addr_ms, CYAN),
                    ("ACTVTY", actvty_ms, FOREGROUND),
                ];

                // FETCH bar
                let fetch_end = col(build_ms + prefetch_collect_ms);
                let fetch_start = col((build_ms + prefetch_collect_ms - fetch_ms).max(0.0));
                lines.push(gantt_bar_line(
                    "FETCH",
                    TERMINAL_GREEN,
                    fetch_start,
                    fetch_end,
                    bar_width,
                    fetch_ms,
                ));

                // Sub-phase bars
                let mut offset = 0.0;
                for (label, dur, color) in sub_phases {
                    let s = col(offset);
                    let e = col(offset + dur);
                    lines.push(gantt_bar_line(label, *color, s, e, bar_width, *dur));
                    offset += dur;
                }

                // FLUSH bar
                if flush_ms > 0.0 {
                    let s = col(iteration_ms);
                    let e = col(iteration_ms + flush_ms);
                    lines.push(gantt_bar_line("FLUSH", CYAN, s, e, bar_width, flush_ms));
                }
            } else {
                // Collapsed: FETCH, BUILD, FLUSH
                let fetch_end = col(build_ms + prefetch_collect_ms);
                let fetch_start = col((build_ms + prefetch_collect_ms - fetch_ms).max(0.0));
                lines.push(gantt_bar_line(
                    "FETCH",
                    TERMINAL_GREEN,
                    fetch_start,
                    fetch_end,
                    bar_width,
                    fetch_ms,
                ));
                lines.push(gantt_bar_line(
                    "BUILD",
                    AMBER,
                    col(0.0),
                    col(build_ms),
                    bar_width,
                    build_ms,
                ));
                if flush_ms > 0.0 {
                    lines.push(gantt_bar_line(
                        "FLUSH",
                        CYAN,
                        col(iteration_ms),
                        col(iteration_ms + flush_ms),
                        bar_width,
                        flush_ms,
                    ));
                }
            }
        }
    }

    // -- Overlap sparklines --
    let spark_width = (area.width as usize).saturating_sub(20).clamp(4, 30);
    lines.push(overlap_sparkline_line(
        "Fetch",
        &app.fetch_overlap_history,
        spark_width,
        false,
    ));
    lines.push(overlap_sparkline_line(
        "Flush",
        &app.flush_overlap_history,
        spark_width,
        false,
    ));
    lines.push(overlap_sparkline_line(
        "Idle",
        &app.idle_ratio_history,
        spark_width,
        true,
    ));

    f.render_widget(Paragraph::new(lines), area);
}

/// Build a single Gantt bar as a Line (for use in Paragraph-based rendering).
fn gantt_bar_line(
    label: &str,
    color: Color,
    start: usize,
    end: usize,
    max_width: usize,
    duration_ms: f64,
) -> Line<'static> {
    let label_text = format!("{:<6} ", label);
    let bar_start = start.min(max_width);
    let bar_len = end.saturating_sub(start).max(1).min(max_width - bar_start);

    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::styled(label_text, Style::default().fg(color)));

    // Pad before bar
    if bar_start > 0 {
        spans.push(Span::raw(" ".repeat(bar_start)));
    }

    // Bar
    spans.push(Span::styled(
        "\u{2588}".repeat(bar_len),
        Style::default().fg(color),
    ));

    // Duration annotation
    spans.push(Span::styled(
        format!(" {:.0}ms", duration_ms),
        Style::default().fg(SLATE_500),
    ));

    Line::from(spans)
}

fn build_bulk_build_diagnostics(
    bb: &BulkBuildProgressData,
    app: &App,
    cols: &[Rect],
    rate_jitter_text: &str,
    eta_conf: &(&'static str, Color),
    dense_panel: bool,
) -> (Vec<Line<'static>>, Vec<Line<'static>>) {
    // ── Left column: finalize checklist OR engine header + stage breakdown ──
    let left = if bb.finalize_phase.is_some() {
        build_finalize_left_column(bb, cols[0].height as usize)
    } else {
        build_batch_left_column(bb, cols, dense_panel)
    };

    // ── Right column: memory, materialization, pressure gauges, sparklines ──
    let owner_mem_text = bb
        .owner_memory_bytes
        .map(format_bytes)
        .unwrap_or_else(|| "-".to_string());
    let live_cells_text = bb
        .live_cell_count
        .map(format_num_u64)
        .unwrap_or_else(|| "-".to_string());
    let hist_rows = bb
        .cumulative_history_rows
        .map(format_num_u64)
        .unwrap_or_else(|| "-".to_string());
    let sealed_rows = bb
        .cumulative_sealed_rows
        .map(format_num_u64)
        .unwrap_or_else(|| "-".to_string());

    let spark_width = cols[1].width.saturating_sub(14).clamp(8, 24) as usize;
    let show_p95 = cols[1].width >= P95_MIN_WIDTH;
    let gauge_width = (cols[1].width / 4).clamp(6, 12) as usize;

    let spark_build_fetch = merged_sparkline_p95_line(
        "B",
        AMBER,
        &app.bulk_build_ms_history,
        "F",
        TERMINAL_DIM,
        &app.bulk_fetch_ms_history,
        spark_width,
        show_p95,
    );

    let (l0_line, wbm_line, pressure_line) = if let Some(mem) = &app.memory_stats {
        (
            storage_pressure_l0_line(mem, gauge_width),
            storage_pressure_wbm_line(mem, gauge_width),
            storage_pressure_summary_line(mem),
        )
    } else {
        (
            Line::from(Span::styled("L0 -", Style::default().fg(SLATE_500))),
            Line::from(Span::styled("WBM -", Style::default().fg(SLATE_500))),
            Line::from(Span::styled("Compact -", Style::default().fg(SLATE_500))),
        )
    };

    let right = vec![
        Line::from(vec![
            Span::styled("Owner mem ", Style::default().fg(SLATE_500)),
            Span::styled(owner_mem_text, Style::default().fg(FOREGROUND)),
            Span::styled("  Live cells ", Style::default().fg(SLATE_500)),
            Span::styled(live_cells_text, Style::default().fg(FOREGROUND)),
        ]),
        Line::from(vec![
            Span::styled("Materialized ", Style::default().fg(SLATE_500)),
            Span::styled("hist ", Style::default().fg(SLATE_500)),
            Span::styled(hist_rows, Style::default().fg(FOREGROUND)),
            Span::styled("  sealed ", Style::default().fg(SLATE_500)),
            Span::styled(sealed_rows, Style::default().fg(FOREGROUND)),
        ]),
        spark_build_fetch,
        l0_line,
        wbm_line,
        pressure_line,
        Line::from(vec![
            Span::styled("ETA ", Style::default().fg(SLATE_500)),
            Span::styled(eta_conf.0, Style::default().fg(eta_conf.1)),
            Span::styled("  jitter ", Style::default().fg(SLATE_500)),
            Span::styled(rate_jitter_text.to_string(), Style::default().fg(AMBER)),
        ]),
    ];

    (left, right)
}

/// Build the left column for normal per-batch bulk-build diagnostics.
fn build_batch_left_column(
    bb: &BulkBuildProgressData,
    cols: &[Rect],
    dense_panel: bool,
) -> Vec<Line<'static>> {
    let batch_count_text = bb
        .batch_count
        .map(format_num_u64)
        .unwrap_or_else(|| "-".to_string());
    let span_text = bb
        .batch_block_span
        .map(|v| format!("{}k", v / 1000))
        .unwrap_or_else(|| "-".to_string());

    let stages: [(&str, Option<f64>, Color); 6] = [
        ("Facts", bb.facts_ms, TERMINAL_GREEN),
        ("Resolve", bb.resolve_ms, TERMINAL_DIM),
        ("Reduce", bb.reduce_ms, AMBER),
        ("History", bb.history_ms, CYAN),
        ("Addr", bb.address_reduce_ms, SLATE_500),
        ("Activity", bb.activity_stats_ms, SLATE_500),
    ];
    let max_stage_ms = stages
        .iter()
        .filter_map(|(_, ms, _)| *ms)
        .fold(0.0_f64, f64::max)
        .max(1.0);

    let bar_width = (cols[0].width as usize).saturating_sub(22).clamp(8, 30);
    let mut left = vec![Line::from(vec![
        Span::styled("Engine ", Style::default().fg(SLATE_500)),
        Span::styled(
            "[BULK BUILD]",
            Style::default()
                .fg(Color::Black)
                .bg(AMBER)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  Batch #", Style::default().fg(SLATE_500)),
        Span::styled(batch_count_text, Style::default().fg(FOREGROUND)),
        Span::styled("  Span ", Style::default().fg(SLATE_500)),
        Span::styled(span_text, Style::default().fg(FOREGROUND)),
    ])];

    for (name, ms_opt, color) in &stages {
        let ms = ms_opt.unwrap_or(0.0);
        let filled = ((ms / max_stage_ms) * bar_width as f64).round() as usize;
        let filled = filled.min(bar_width);
        let bar: String = "\u{2588}".repeat(filled);
        let empty: String = "\u{2591}".repeat(bar_width - filled);
        let pct = if bb.build_ms.unwrap_or(0.0) > 0.0 {
            format!("{:3.0}%", ms / bb.build_ms.unwrap() * 100.0)
        } else {
            "  -".to_string()
        };
        left.push(Line::from(vec![
            Span::styled(format!("{:<8}", name), Style::default().fg(SLATE_500)),
            Span::styled(bar, Style::default().fg(*color)),
            Span::styled(empty, Style::default().fg(SLATE_800)),
            Span::styled(format!(" {:>6.1}ms", ms), Style::default().fg(FOREGROUND)),
            Span::styled(format!(" {}", pct), Style::default().fg(SLATE_500)),
        ]));
    }

    // Facts parallel breakdown detail line (after stage bars)
    if let (Some(par_ms), Some(merge_ms), Some(serial_ms)) = (
        bb.facts_par_iter_ms,
        bb.facts_merge_ms,
        bb.facts_serial_equivalent_ms,
    ) {
        let speedup = if par_ms > 0.0 {
            serial_ms / par_ms
        } else {
            0.0
        };
        let miss_rate_text = match (bb.facts_intern_slow_path_count, bb.facts_intern_total_count) {
            (Some(slow), Some(total)) if total > 0 => {
                format!("{:.1}%", slow as f64 / total as f64 * 100.0)
            }
            _ => "-".to_string(),
        };
        if dense_panel {
            // Compact: one line
            left.push(Line::from(vec![
                Span::styled("  par ", Style::default().fg(SLATE_500)),
                Span::styled(format!("{par_ms:.0}ms"), Style::default().fg(FOREGROUND)),
                Span::styled("  merge ", Style::default().fg(SLATE_500)),
                Span::styled(format!("{merge_ms:.0}ms"), Style::default().fg(FOREGROUND)),
                Span::styled(
                    format!("  {speedup:.1}"),
                    Style::default().fg(TERMINAL_GREEN),
                ),
                Span::styled("\u{00d7} speedup", Style::default().fg(SLATE_500)),
                Span::styled("  miss ", Style::default().fg(SLATE_500)),
                Span::styled(miss_rate_text, Style::default().fg(FOREGROUND)),
            ]));
        } else {
            // Detail: multi-line
            left.push(Line::from(vec![
                Span::styled("  par_iter ", Style::default().fg(SLATE_500)),
                Span::styled(format!("{par_ms:>7.1}ms"), Style::default().fg(FOREGROUND)),
                Span::styled(
                    format!("  (serial equiv {serial_ms:.0}ms \u{2192} {speedup:.1}\u{00d7})"),
                    Style::default().fg(SLATE_500),
                ),
            ]));
            left.push(Line::from(vec![
                Span::styled("  merge    ", Style::default().fg(SLATE_500)),
                Span::styled(
                    format!("{merge_ms:>7.1}ms"),
                    Style::default().fg(FOREGROUND),
                ),
                Span::styled(
                    format!(
                        "  ({:.1}%)",
                        if (par_ms + merge_ms) > 0.0 {
                            merge_ms / (par_ms + merge_ms) * 100.0
                        } else {
                            0.0
                        }
                    ),
                    Style::default().fg(SLATE_500),
                ),
            ]));
            let intern_text = match (bb.facts_intern_total_count, bb.facts_intern_slow_path_count) {
                (Some(total), Some(slow)) => {
                    format!(
                        "  intern   {}k calls  {} miss ({})",
                        total / 1000,
                        format_num_u64(slow),
                        miss_rate_text
                    )
                }
                _ => "  intern   -".to_string(),
            };
            left.push(Line::from(Span::styled(
                intern_text,
                Style::default().fg(SLATE_500),
            )));
            // Volume line
            let cells_text = bb
                .facts_cell_count
                .map(|c| format!("{}k", c / 1000))
                .unwrap_or_else(|| "-".to_string());
            let blocks_text = bb
                .batch_block_span
                .map(|b| format!("{}k", b / 1000))
                .unwrap_or_else(|| "-".to_string());
            left.push(Line::from(vec![
                Span::styled("  volume   ", Style::default().fg(SLATE_500)),
                Span::styled(
                    format!("{cells_text} cells"),
                    Style::default().fg(FOREGROUND),
                ),
                Span::styled("  ", Style::default().fg(SLATE_500)),
                Span::styled(
                    format!("{blocks_text} blocks"),
                    Style::default().fg(FOREGROUND),
                ),
            ]));
        }
    }

    let flush_text = bb
        .flush_ms
        .map(|v| format!("{v:.1}ms"))
        .unwrap_or_else(|| "-".to_string());
    let fetch_text = bb
        .fetch_ms
        .map(|v| format!("{v:.1}ms"))
        .unwrap_or_else(|| "-".to_string());
    let build_text = bb
        .build_ms
        .map(|v| format!("{v:.1}ms"))
        .unwrap_or_else(|| "-".to_string());
    left.push(Line::from(vec![
        Span::styled("I/O ", Style::default().fg(SLATE_500)),
        Span::styled("Fetch ", Style::default().fg(SLATE_500)),
        Span::styled(fetch_text, Style::default().fg(FOREGROUND)),
        Span::styled("  Build ", Style::default().fg(SLATE_500)),
        Span::styled(build_text, Style::default().fg(FOREGROUND)),
        Span::styled("  Flush ", Style::default().fg(SLATE_500)),
        Span::styled(flush_text, Style::default().fg(TERMINAL_DIM)),
    ]));

    let cells_created = bb
        .cells_created
        .map(|v| format!("+{}", format_num_u64(v)))
        .unwrap_or_else(|| "-".to_string());
    let cells_consumed = bb
        .cells_consumed
        .map(|v| format!("-{}", format_num_u64(v)))
        .unwrap_or_else(|| "-".to_string());
    let density_text = bb
        .tx_density
        .map(|v| format!("{v:.1}"))
        .unwrap_or_else(|| "-".to_string());
    left.push(Line::from(vec![
        Span::styled("Volume ", Style::default().fg(SLATE_500)),
        Span::styled("Cells ", Style::default().fg(SLATE_500)),
        Span::styled(cells_created, Style::default().fg(TERMINAL_GREEN)),
        Span::styled(" ", Style::default()),
        Span::styled(cells_consumed, Style::default().fg(AMBER)),
        Span::styled("  Density ", Style::default().fg(SLATE_500)),
        Span::styled(
            format!("{} tx/blk", density_text),
            Style::default().fg(FOREGROUND),
        ),
    ]));

    // Adaptive EMA controller: cost model and budget utilization
    let ema_text = bb
        .ms_per_block_ema
        .filter(|v| *v > 0.0 && v.is_finite())
        .map(|v| format!("{v:.3}"))
        .unwrap_or_else(|| "-".to_string());
    let ctrl_ms = bb.controllable_ms.unwrap_or(0.0);
    let target_ms = bb.target_iteration_ms.unwrap_or(0.0);
    let ctrl_text = if ctrl_ms > 0.0 {
        format!("{ctrl_ms:.0}")
    } else {
        "-".to_string()
    };
    let target_text = if target_ms > 0.0 {
        format!("{target_ms:.0}")
    } else {
        "-".to_string()
    };
    let budget_color = if ctrl_ms > 0.0 && target_ms > 0.0 {
        let ratio = ctrl_ms / target_ms;
        if ratio <= 1.1 {
            TERMINAL_GREEN
        } else if ratio <= 1.5 {
            AMBER
        } else {
            ERROR_RED
        }
    } else {
        FOREGROUND
    };
    left.push(Line::from(vec![
        Span::styled("Adaptive ", Style::default().fg(SLATE_500)),
        Span::styled("EMA ", Style::default().fg(SLATE_500)),
        Span::styled(
            format!("{} ms/blk", ema_text),
            Style::default().fg(TERMINAL_DIM),
        ),
        Span::styled("  Budget ", Style::default().fg(SLATE_500)),
        Span::styled(
            format!("{}/{} ms", ctrl_text, target_text),
            Style::default().fg(budget_color),
        ),
        Span::styled(
            format!("  [{}-{}k]", BULK_BUILD_MIN_SPAN_K, BULK_BUILD_MAX_SPAN_K),
            Style::default().fg(SLATE_500),
        ),
    ]));

    left
}

/// Finalize phase labels in execution order.
const FINALIZE_PHASES: &[&str] = &[
    "Drain flush",
    "Activity stats",
    "Chain stats",
    "Final snapshot",
    "Owner: address",
    "Owner: script",
    "Owner: token",
    "Owner: dao",
    "Owner: fiber",
    "Owner: object",
    "Metadata",
    "Memtable flush",
    "Sync status",
];

/// Build the left column for finalize-mode diagnostics (checklist).
fn build_finalize_left_column(
    bb: &BulkBuildProgressData,
    available_height: usize,
) -> Vec<Line<'static>> {
    let step = bb.finalize_step.unwrap_or(0);
    let total = bb
        .finalize_steps_total
        .unwrap_or(FINALIZE_PHASES.len() as u8);
    let elapsed_s = bb.finalize_elapsed_ms.unwrap_or(0.0) / 1000.0;

    let mut lines = vec![Line::from(vec![
        Span::styled("Engine ", Style::default().fg(SLATE_500)),
        Span::styled(
            "[FINALIZING]",
            Style::default()
                .fg(Color::Black)
                .bg(AMBER)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  step {}/{}", step + 1, total),
            Style::default().fg(FOREGROUND),
        ),
        Span::styled(
            format!("  {:.1}s", elapsed_s),
            Style::default().fg(SLATE_500),
        ),
    ])];

    // Compact: if very little space, show only header + current phase
    if available_height < 6 {
        let label = FINALIZE_PHASES.get(step as usize).unwrap_or(&"...");
        lines.push(Line::from(vec![
            Span::styled("\u{25b8} ", Style::default().fg(AMBER)),
            Span::styled(label.to_string(), Style::default().fg(AMBER)),
        ]));
        return lines;
    }

    // Medium: group small trailing phases to fit
    // Full: show all 13 individually
    let group_tail = available_height < 14;

    // Main phases to show individually (first 7: drain through owner:token)
    let individual_end = if group_tail { 7 } else { FINALIZE_PHASES.len() };

    for (i, label) in FINALIZE_PHASES.iter().enumerate().take(individual_end) {
        let i = i as u8;
        let (marker, color) = if i < step {
            ("\u{2713}", TERMINAL_GREEN) // ✓
        } else if i == step {
            ("\u{25b8}", AMBER) // ►
        } else {
            (" ", SLATE_500)
        };
        lines.push(Line::from(vec![
            Span::styled(format!("  {} ", marker), Style::default().fg(color)),
            Span::styled(label.to_string(), Style::default().fg(color)),
        ]));
    }

    if group_tail {
        // Group remaining owners (dao/fiber/object) on one line
        let group1_phases: &[usize] = &[7, 8, 9]; // dao, fiber, object
        let group1_label = "Owner: dao / fiber / object";
        let group1_color = if group1_phases.iter().all(|&i| (i as u8) < step) {
            TERMINAL_GREEN
        } else if group1_phases.iter().any(|&i| (i as u8) == step) {
            AMBER
        } else {
            SLATE_500
        };
        let group1_marker = if group1_phases.iter().all(|&i| (i as u8) < step) {
            "\u{2713}"
        } else if group1_phases.iter().any(|&i| (i as u8) == step) {
            "\u{25b8}"
        } else {
            " "
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {} ", group1_marker),
                Style::default().fg(group1_color),
            ),
            Span::styled(group1_label.to_string(), Style::default().fg(group1_color)),
        ]));

        // Group tail phases (metadata/memtable/sync) on one line
        let group2_phases: &[usize] = &[10, 11, 12]; // metadata, memtable, sync
        let group2_label = "Metadata / Flush / Status";
        let group2_color = if group2_phases.iter().all(|&i| (i as u8) < step) {
            TERMINAL_GREEN
        } else if group2_phases.iter().any(|&i| (i as u8) == step) {
            AMBER
        } else {
            SLATE_500
        };
        let group2_marker = if group2_phases.iter().all(|&i| (i as u8) < step) {
            "\u{2713}"
        } else if group2_phases.iter().any(|&i| (i as u8) == step) {
            "\u{25b8}"
        } else {
            " "
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {} ", group2_marker),
                Style::default().fg(group2_color),
            ),
            Span::styled(group2_label.to_string(), Style::default().fg(group2_color)),
        ]));
    }

    lines
}

fn format_stage_commit_gap_ms(stage_ms: Option<f64>, commit_ms: Option<f64>) -> String {
    match (stage_ms, commit_ms) {
        (Some(stage), Some(commit)) => format!("{:+.1}ms", stage - commit),
        _ => "-".to_string(),
    }
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

#[derive(Clone, Copy)]
struct AdaptiveControlSnapshot<'a> {
    last_batch_blocks: Option<u64>,
    adaptive_inflight_batches: Option<u64>,
    adaptive_target_batch_txs: Option<u64>,
    adaptive_inflight_limit: Option<u64>,
    adaptive_min_target_batch_txs: Option<u64>,
    adaptive_cooldown_steps: Option<u64>,
    adaptive_last_reason: Option<&'a str>,
    adaptive_adjustment_seq: Option<u64>,
    adaptive_last_adjusted_age_secs: Option<i64>,
    adaptive_backoff_streak: Option<u64>,
}

fn adaptive_control_lines(
    snapshot: AdaptiveControlSnapshot<'_>,
    dense: bool,
    pipeline_reset_epoch: Option<u64>,
    pipeline_reset_reason: Option<&str>,
) -> Vec<Line<'static>> {
    let (state, state_color) = adaptive_state_label(snapshot.adaptive_last_reason);
    let batch_blocks_text = snapshot
        .last_batch_blocks
        .map(format_num_u64)
        .unwrap_or_else(|| "-".to_string());
    let target_text = snapshot
        .adaptive_target_batch_txs
        .map(format_num_compact)
        .unwrap_or_else(|| "-".to_string());
    let floor_text = snapshot
        .adaptive_min_target_batch_txs
        .map(format_num_compact)
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

    let state_text = if let Some(streak) = snapshot.adaptive_backoff_streak.filter(|v| *v > 0) {
        format!("{state} x{streak}")
    } else {
        state.to_string()
    };
    let state_text_color = if snapshot.adaptive_backoff_streak.unwrap_or(0) >= 5 {
        ERROR_RED
    } else {
        state_color
    };

    let line1 = Line::from(vec![
        Span::styled("Adaptive ", Style::default().fg(SLATE_500)),
        Span::styled(
            format!(
                "batch {} blk  inflight {}  target {} tx  floor {}",
                batch_blocks_text, inflight_text, target_text, floor_text,
            ),
            Style::default().fg(TERMINAL_DIM),
        ),
        Span::styled("  ", Style::default().fg(SLATE_700)),
        Span::styled(
            state_text,
            Style::default()
                .fg(state_text_color)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    if dense {
        return vec![line1];
    }

    // Line 2: adjustment history + pipeline reset
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
        .map(|v| format!("{v}s ago"))
        .unwrap_or_else(|| "-".to_string());
    let backoff_text = format!("x{}", snapshot.adaptive_backoff_streak.unwrap_or(0));

    let mut line2_spans = vec![
        Span::styled("          ", Style::default()), // indent to align under "Adaptive "
        Span::styled(
            format!(
                "cooldown {}  adj #{} ({})  backoff {}",
                cooldown_text, seq_text, age_text, backoff_text,
            ),
            Style::default().fg(TERMINAL_DIM),
        ),
    ];

    // Merge pipeline reset info if epoch > 0
    if let Some(epoch) = pipeline_reset_epoch.filter(|e| *e > 0) {
        let reason = pipeline_reset_reason.unwrap_or("-");
        line2_spans.push(Span::styled(
            format!("  reset #{} {}", epoch, reason),
            Style::default().fg(AMBER),
        ));
    }

    vec![line1, Line::from(line2_spans)]
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

fn dense_right_lines(
    spark_fp_line: Line<'static>,
    spark_wc_line: Line<'static>,
    l0_line: Line<'static>,
    wbm_line: Line<'static>,
    pressure_line: Line<'static>,
    stability_line: Line<'static>,
) -> Vec<Line<'static>> {
    vec![
        spark_fp_line,
        spark_wc_line,
        l0_line,
        wbm_line,
        pressure_line,
        stability_line,
    ]
}

fn detail_right_lines(
    spark_fp_line: Line<'static>,
    spark_wc_line: Line<'static>,
    l0_line: Line<'static>,
    wbm_line: Line<'static>,
    pressure_line: Line<'static>,
    stability_line: Line<'static>,
    io_line: Line<'static>,
) -> Vec<Line<'static>> {
    vec![
        spark_fp_line,
        spark_wc_line,
        l0_line,
        wbm_line,
        pressure_line,
        stability_line,
        io_line,
    ]
}

fn sync_timing_lines(
    eta: Option<&str>,
    eta_seconds: Option<f64>,
    elapsed: Option<&str>,
    startup_phase: Option<&str>,
    is_bulk_sync: bool,
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

    if is_bulk_sync {
        if let Some(secs) = eta_seconds {
            let done_at = Local::now() + chrono::TimeDelta::seconds(secs as i64);
            lines.push(Line::from(vec![
                Span::styled("Est. done: ", Style::default().fg(SLATE_500)),
                Span::styled(
                    done_at.format("%H:%M").to_string(),
                    Style::default().fg(TERMINAL_GREEN),
                ),
            ]));
        }
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
            "Storage Runtime",
            Style::default().fg(FOREGROUND),
        ));

    let Some(mem) = &app.memory_stats else {
        let msg = Paragraph::new("No memory stats (store unavailable)").block(block);
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

    let (left, mid, right) = storage_runtime_columns(mem);
    f.render_widget(Paragraph::new(left), cols[0]);
    f.render_widget(Paragraph::new(mid), cols[1]);
    f.render_widget(Paragraph::new(right), cols[2]);
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
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    draw_api_health(f, app, cols[0]);
    draw_runtime_health(f, app, cols[1]);
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

    let services = app.supervisor_services.as_deref();
    let log_tails = app.service_log_tails.as_deref();
    let (state, state_color) = runtime_health_state(app.runtime_diag.as_ref(), services);
    let Some(diag) = app.runtime_diag.as_ref() else {
        let mut lines = vec![
            Line::from(vec![
                Span::styled("State ", Style::default().fg(SLATE_500)),
                Span::styled(
                    format!("[{}]", state),
                    Style::default()
                        .fg(state_color)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(Span::styled(
                "No runtime diagnostics",
                Style::default().fg(SLATE_500),
            )),
        ];
        if let Some(service_line) = supervisor_services_line(services) {
            lines.push(service_line);
        }
        if let Some(tail_line) = service_log_tails_line(log_tails, inner.width as usize) {
            lines.push(tail_line);
        }
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
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

    let mut lines = vec![
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
    if let Some(service_line) = supervisor_services_line(services) {
        lines.push(service_line);
    }
    if let Some(tail_line) = service_log_tails_line(log_tails, inner.width as usize) {
        lines.push(tail_line);
    }

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
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

fn runtime_health_state(
    info: Option<&RuntimeDiagData>,
    services: Option<&[SupervisorServiceData]>,
) -> (&'static str, Color) {
    let Some(info) = info else {
        if services.is_some() {
            return ("SUP", CYAN);
        }
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

fn supervisor_services_line(services: Option<&[SupervisorServiceData]>) -> Option<Line<'static>> {
    let services = services?;
    if services.is_empty() {
        return None;
    }

    let mut statuses = Vec::new();
    for service in services {
        let short_name = match service.name.as_str() {
            "frontend-server" => "frontend",
            other => other,
        };
        statuses.push(format!(
            "{}:{}#{}({}s)",
            short_name, service.status, service.pid, service.uptime_secs
        ));
    }

    Some(Line::from(vec![
        Span::styled("Svc ", Style::default().fg(SLATE_500)),
        Span::styled(statuses.join("  "), Style::default().fg(CYAN)),
    ]))
}

fn service_log_tails_line(
    tails: Option<&[ServiceLogTailData]>,
    panel_width: usize,
) -> Option<Line<'static>> {
    let tails = tails?;
    if tails.is_empty() {
        return None;
    }

    let max_items = 2usize;
    let per_item_limit = panel_width.saturating_div(max_items).max(24);
    let mut parts = Vec::new();
    for tail in tails.iter().take(max_items) {
        let item = format!("{}: {}", tail.service, tail.last_line);
        parts.push(trim_for_panel(&item, per_item_limit));
    }
    if tails.len() > max_items {
        parts.push(format!("+{}", tails.len() - max_items));
    }

    let joined = trim_for_panel(&parts.join(" | "), panel_width.saturating_sub(8));
    Some(Line::from(vec![
        Span::styled("Tail ", Style::default().fg(SLATE_500)),
        Span::styled(joined, Style::default().fg(AMBER)),
    ]))
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

    let hint = footer_hint_line(inner.width);

    let mut lines = vec![hint];
    if let Some((msg, color)) = footer_status_message(app.status_message.as_ref()) {
        lines.push(Line::from(Span::styled(msg, Style::default().fg(color))));
    }

    f.render_widget(Paragraph::new(lines).alignment(Alignment::Left), inner);
}

fn footer_hint_line(width: u16) -> Line<'static> {
    if width < 90 {
        Line::from(vec![
            Span::styled("q", Style::default().fg(TERMINAL_GREEN)),
            Span::styled(" quit  ", Style::default().fg(SLATE_500)),
            Span::styled("Tab/h/l", Style::default().fg(TERMINAL_GREEN)),
            Span::styled(" tabs  ", Style::default().fg(SLATE_500)),
            Span::styled("?", Style::default().fg(TERMINAL_GREEN)),
            Span::styled(" help", Style::default().fg(SLATE_500)),
        ])
    } else if width < 120 {
        Line::from(vec![
            Span::styled("q", Style::default().fg(TERMINAL_GREEN)),
            Span::styled(" quit  ", Style::default().fg(SLATE_500)),
            Span::styled("Tab/h/l", Style::default().fg(TERMINAL_GREEN)),
            Span::styled(" tabs  ", Style::default().fg(SLATE_500)),
            Span::styled("j/k", Style::default().fg(TERMINAL_GREEN)),
            Span::styled(" scroll  ", Style::default().fg(SLATE_500)),
            Span::styled("R", Style::default().fg(TERMINAL_GREEN)),
            Span::styled(" refresh  ", Style::default().fg(SLATE_500)),
            Span::styled("?", Style::default().fg(TERMINAL_GREEN)),
            Span::styled(" help", Style::default().fg(SLATE_500)),
        ])
    } else {
        Line::from(vec![
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
        ])
    }
}

fn footer_status_message(status_message: Option<&(String, Instant)>) -> Option<(String, Color)> {
    let (msg, ts) = status_message?;
    let age_secs = ts.elapsed().as_secs();
    if age_secs >= STATUS_MESSAGE_TTL_SECS {
        return None;
    }

    let color = if age_secs < STATUS_MESSAGE_WARM_SECS {
        AMBER
    } else {
        SLATE_500
    };
    Some((msg.clone(), color))
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
        SyncBottleneck::WriteBound => "write-stage-bound",
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

fn storage_pressure_l0_line(mem: &MemoryStatsData, gauge_width: usize) -> Line<'static> {
    let ratio = if L0_GAUGE_MAX > 0 {
        (mem.l0_files_max as f64 / L0_GAUGE_MAX as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let filled = (ratio * gauge_width as f64).round() as usize;
    let gauge = render_gauge(filled, gauge_width);
    let (badge, badge_color) = if mem.l0_files_max >= 16 {
        ("HOT", ERROR_RED)
    } else if mem.l0_files_max >= 10 {
        ("WARN", AMBER)
    } else {
        ("OK", TERMINAL_GREEN)
    };
    Line::from(vec![
        Span::styled("L0 ", Style::default().fg(SLATE_500)),
        Span::styled(
            format!("{}/{} ", mem.l0_files_max, L0_GAUGE_MAX),
            Style::default().fg(FOREGROUND),
        ),
        Span::styled(gauge, Style::default().fg(badge_color)),
        Span::styled(format!(" [{}]", badge), Style::default().fg(badge_color)),
    ])
}

fn storage_pressure_wbm_line(mem: &MemoryStatsData, gauge_width: usize) -> Line<'static> {
    if mem.wbm_budget_bytes == 0 {
        return Line::from(vec![
            Span::styled("WBM ", Style::default().fg(SLATE_500)),
            Span::styled("-", Style::default().fg(SLATE_500)),
        ]);
    }
    let ratio = (mem.wbm_usage_bytes as f64 / mem.wbm_budget_bytes as f64).clamp(0.0, 1.0);
    let pct = (ratio * 100.0) as u64;
    let filled = (ratio * gauge_width as f64).round() as usize;
    let gauge = render_gauge(filled, gauge_width);
    let color = if pct > 90 {
        ERROR_RED
    } else if pct > 70 {
        AMBER
    } else {
        TERMINAL_GREEN
    };
    Line::from(vec![
        Span::styled("WBM ", Style::default().fg(SLATE_500)),
        Span::styled(
            format!(
                "{}/{} ",
                format_bytes(mem.wbm_usage_bytes),
                format_bytes(mem.wbm_budget_bytes)
            ),
            Style::default().fg(FOREGROUND),
        ),
        Span::styled(gauge, Style::default().fg(color)),
        Span::styled(format!(" {}%", pct), Style::default().fg(color)),
    ])
}

fn storage_pressure_summary_line(mem: &MemoryStatsData) -> Line<'static> {
    let worst = if mem.l0_worst_cf.is_empty() {
        "-".to_string()
    } else {
        mem.l0_worst_cf.clone()
    };
    Line::from(vec![
        Span::styled("Compact ", Style::default().fg(SLATE_500)),
        Span::styled(
            format_bytes(mem.compaction_pending_bytes),
            Style::default().fg(FOREGROUND),
        ),
        Span::styled("  Imm ", Style::default().fg(SLATE_500)),
        Span::styled(
            format_num_u64(mem.immutable_memtables),
            Style::default().fg(FOREGROUND),
        ),
        Span::styled("  worst ", Style::default().fg(SLATE_500)),
        Span::styled(worst, Style::default().fg(AMBER)),
    ])
}

#[allow(clippy::too_many_arguments)]
fn merged_sparkline_p95_line(
    label_a: &'static str,
    color_a: Color,
    history_a: &VecDeque<f64>,
    label_b: &'static str,
    color_b: Color,
    history_b: &VecDeque<f64>,
    spark_width: usize,
    show_p95: bool,
) -> Line<'static> {
    let half_spark = spark_width / 2;
    let mut spans = vec![
        Span::styled(label_a, Style::default().fg(color_a)),
        Span::styled(" ", Style::default().fg(SLATE_700)),
        Span::styled(
            sparkline(history_a, half_spark),
            Style::default().fg(color_a),
        ),
        Span::styled("  ", Style::default().fg(SLATE_700)),
        Span::styled(label_b, Style::default().fg(color_b)),
        Span::styled(" ", Style::default().fg(SLATE_700)),
        Span::styled(
            sparkline(history_b, half_spark),
            Style::default().fg(color_b),
        ),
    ];
    if show_p95 {
        let p95_a = percentile_from_history(history_a, P95_WINDOW, 95.0);
        let p95_b = percentile_from_history(history_b, P95_WINDOW, 95.0);
        let p95_text = match (p95_a, p95_b) {
            (Some(a), Some(b)) => format!("  p95 {:.0}/{:.0}ms", a, b),
            (Some(a), None) => format!("  p95 {:.0}/-ms", a),
            (None, Some(b)) => format!("  p95 -/{:.0}ms", b),
            (None, None) => String::new(),
        };
        if !p95_text.is_empty() {
            spans.push(Span::styled(p95_text, Style::default().fg(SLATE_500)));
        }
    }
    Line::from(spans)
}

fn percentile_from_history(history: &VecDeque<f64>, window: usize, pct: f64) -> Option<f64> {
    if history.is_empty() {
        return None;
    }
    let mut values: Vec<f64> = history.iter().rev().take(window).copied().collect();
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((values.len() as f64 * pct / 100.0) as usize).min(values.len() - 1);
    Some(values[idx])
}

fn render_gauge(filled: usize, total: usize) -> String {
    if total == 0 {
        return String::new();
    }
    let filled = filled.min(total);
    format!("{}{}", "█".repeat(filled), "░".repeat(total - filled))
}

fn format_num_compact(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{}K", value / 1_000)
    } else {
        value.to_string()
    }
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
        Line::from("  Tab/s/l  Next tab (Overview / Sync / System)"),
        Line::from("  h        Previous tab"),
        Line::from("  c        Toggle compact layout override"),
        Line::from("  v        Cycle diagnostics view (Auto/Compact/Detail)"),
        Line::from("  e        Toggle build sub-phase expansion"),
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

// ---------------------------------------------------------------------------
// System tab
// ---------------------------------------------------------------------------

fn system_kv_line(label: &str, value: String, value_color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {:<22}", label), Style::default().fg(SLATE_500)),
        Span::styled(value, Style::default().fg(value_color)),
    ])
}

fn direct_io_reads_label(enabled: bool) -> &'static str {
    if enabled {
        "reads + flush/compact"
    } else {
        "flush/compact only"
    }
}

fn draw_system_content(f: &mut Frame, app: &App, area: Rect) {
    let db = app.db();
    let p = db.memory_profile();
    let mem = &app.memory_stats;
    let compact = app.force_compact_layout || area.width < 130;

    // Section 1 grows by 2 rows when live indexer state is available.
    let env_height = if mem.is_some() { 8 } else { 6 };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(env_height),
            Constraint::Length(4),
            Constraint::Length(6),
            Constraint::Min(10),
        ])
        .split(area);

    // -- Section 1: System Environment --
    draw_system_environment(f, p, mem, chunks[0]);

    // -- Section 2: Workdir --
    draw_system_workdirs(f, db.ckbadger_workdir(), db.ckb_workdir(), chunks[1]);

    // -- Section 3: Store Paths --
    draw_system_paths(
        f,
        db.domain_data_path(),
        db.append_only_data_path(),
        db.ckb_db_path(),
        chunks[2],
    );

    // -- Section 4: RocksDB Parameters --
    if compact {
        draw_system_params_compact(f, p, db.direct_io_reads_enabled(), chunks[3]);
    } else {
        draw_system_params_wide(f, p, db.direct_io_reads_enabled(), chunks[3]);
    }
}

fn draw_system_environment(
    f: &mut Frame,
    p: &ckbadger_store::MemoryProfile,
    mem: &Option<MemoryStatsData>,
    area: Rect,
) {
    let block = Block::default()
        .title(Span::styled(
            " System Environment ",
            Style::default().fg(FOREGROUND).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SLATE_800));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let ram_gb = p.system_ram_bytes as f64 / 1_073_741_824.0;
    let budget_pct = if p.system_ram_bytes > 0 {
        (p.rocksdb_budget_bytes as f64 / p.system_ram_bytes as f64 * 100.0) as u32
    } else {
        0
    };
    let mode_label = if p.is_secondary {
        "Secondary (read-only)"
    } else {
        "Primary (read-write)"
    };

    let mut lines = vec![
        system_kv_line("System RAM", format!("{:.1} GB", ram_gb), CYAN),
        system_kv_line("CPU count", format!("{}", p.cpu_count), CYAN),
        system_kv_line("Store mode", mode_label.to_string(), CYAN),
        system_kv_line(
            "RocksDB budget",
            format!(
                "{} ({}% of RAM)",
                format_bytes(p.rocksdb_budget_bytes as u64),
                budget_pct
            ),
            CYAN,
        ),
    ];

    // Live indexer state from memory stats
    if let Some(m) = mem {
        let mode_str = if m.bulk_sync_mode {
            "Bulk Sync"
        } else {
            "Normal"
        };
        let mode_color = if m.bulk_sync_mode {
            AMBER
        } else {
            TERMINAL_GREEN
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {:<22}", "Indexer mode"),
                Style::default().fg(SLATE_500),
            ),
            Span::styled(mode_str.to_string(), Style::default().fg(mode_color)),
        ]));

        if m.wbm_budget_bytes > 0 {
            let pct = (m.wbm_usage_bytes as f64 / m.wbm_budget_bytes as f64 * 100.0) as u32;
            let color = if pct > 90 { AMBER } else { CYAN };
            lines.push(system_kv_line(
                "WBM usage",
                format!(
                    "{} / {} ({}%)",
                    format_bytes(m.wbm_usage_bytes),
                    format_bytes(m.wbm_budget_bytes),
                    pct
                ),
                color,
            ));
        }
    }

    f.render_widget(Paragraph::new(lines), inner);
}

fn system_workdir_lines(
    ckbadger_workdir: &std::path::Path,
    ckb_workdir: &std::path::Path,
) -> Vec<Line<'static>> {
    vec![
        system_kv_line("CKB workdir", ckb_workdir.display().to_string(), FOREGROUND),
        system_kv_line(
            "ckbadger workdir",
            ckbadger_workdir.display().to_string(),
            FOREGROUND,
        ),
    ]
}

fn draw_system_workdirs(
    f: &mut Frame,
    ckbadger_workdir: &std::path::Path,
    ckb_workdir: &std::path::Path,
    area: Rect,
) {
    let block = Block::default()
        .title(Span::styled(
            " Workdir ",
            Style::default().fg(FOREGROUND).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SLATE_800));
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(
        Paragraph::new(system_workdir_lines(ckbadger_workdir, ckb_workdir)),
        inner,
    );
}

fn system_store_path_lines(
    domain_path: &std::path::Path,
    append_path: &std::path::Path,
    ckb_db_path: &std::path::Path,
) -> Vec<Line<'static>> {
    let domain_cf_count = DOMAIN_CFS.len();
    let append_cf_count = APPEND_CFS.len();

    vec![
        system_kv_line("CKB RocksDB", ckb_db_path.display().to_string(), FOREGROUND),
        system_kv_line(
            "Domain store",
            domain_path.display().to_string(),
            FOREGROUND,
        ),
        system_kv_line(
            "Append-only store",
            append_path.display().to_string(),
            FOREGROUND,
        ),
        system_kv_line(
            "Column families",
            format!(
                "{} domain + {} append-only = {}",
                domain_cf_count,
                append_cf_count,
                domain_cf_count + append_cf_count
            ),
            SLATE_500,
        ),
    ]
}

fn draw_system_paths(
    f: &mut Frame,
    domain_path: &std::path::Path,
    append_path: &std::path::Path,
    ckb_db_path: &std::path::Path,
    area: Rect,
) {
    let block = Block::default()
        .title(Span::styled(
            " Store Paths ",
            Style::default().fg(FOREGROUND).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SLATE_800));
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(
        Paragraph::new(system_store_path_lines(
            domain_path,
            append_path,
            ckb_db_path,
        )),
        inner,
    );
}

fn system_normal_mode_lines(p: &ckbadger_store::MemoryProfile) -> Vec<Line<'static>> {
    vec![
        Line::from(Span::styled(
            "  Normal Mode",
            Style::default().fg(FOREGROUND).add_modifier(Modifier::BOLD),
        )),
        system_kv_line("WBM", format_bytes(p.wbm_normal_bytes as u64), CYAN),
        system_kv_line(
            "Block cache",
            format_bytes(p.block_cache_normal_bytes as u64),
            CYAN,
        ),
        system_kv_line(
            "Level base",
            format_bytes(p.normal_max_bytes_for_level_base),
            CYAN,
        ),
        system_kv_line(
            "File base",
            format_bytes(p.normal_target_file_size_base),
            CYAN,
        ),
        system_kv_line(
            "WB mega",
            format_bytes(p.write_buffer_mega_bytes as u64),
            CYAN,
        ),
        system_kv_line(
            "WB high",
            format_bytes(p.write_buffer_high_bytes as u64),
            CYAN,
        ),
        system_kv_line(
            "WB low",
            format_bytes(p.write_buffer_low_bytes as u64),
            CYAN,
        ),
        system_kv_line(
            "Background jobs",
            format!("{}", p.max_background_jobs),
            CYAN,
        ),
        system_kv_line("Subcompactions", format!("{}", p.max_subcompactions), CYAN),
        system_kv_line("L0 trigger", "4".to_string(), SLATE_500),
        system_kv_line("L0 slowdown", "12".to_string(), SLATE_500),
        system_kv_line("L0 stop", "24".to_string(), SLATE_500),
    ]
}

fn system_bulk_sync_lines(p: &ckbadger_store::MemoryProfile) -> Vec<Line<'static>> {
    vec![
        Line::from(Span::styled(
            "  Bulk Sync Mode",
            Style::default().fg(FOREGROUND).add_modifier(Modifier::BOLD),
        )),
        system_kv_line("WBM", format_bytes(p.wbm_bulk_sync_bytes as u64), CYAN),
        system_kv_line(
            "Block cache",
            format_bytes(p.block_cache_bulk_sync_bytes as u64),
            CYAN,
        ),
        system_kv_line(
            "Level base",
            format_bytes(p.bulk_max_bytes_for_level_base),
            CYAN,
        ),
        system_kv_line(
            "File base",
            format_bytes(p.bulk_target_file_size_base),
            CYAN,
        ),
        system_kv_line(
            "Hot CF WB",
            format_bytes(p.write_buffer_hot_cf_bytes as u64),
            CYAN,
        ),
        system_kv_line("L0 slowdown", "64".to_string(), SLATE_500),
        system_kv_line("L0 stop", "128".to_string(), SLATE_500),
    ]
}

fn system_fixed_lines(
    p: &ckbadger_store::MemoryProfile,
    direct_io_reads: bool,
) -> Vec<Line<'static>> {
    vec![
        Line::from(Span::styled(
            "  Fixed Constants",
            Style::default().fg(FOREGROUND).add_modifier(Modifier::BOLD),
        )),
        system_kv_line("Compression", "LZ4 (append: None)".to_string(), SLATE_500),
        system_kv_line(
            "Direct I/O",
            direct_io_reads_label(direct_io_reads).to_string(),
            SLATE_500,
        ),
        system_kv_line("Unordered write", "true".to_string(), SLATE_500),
        system_kv_line("Block size", "16 KB".to_string(), SLATE_500),
        system_kv_line("Bloom filter", "10 bits".to_string(), SLATE_500),
        Line::from(""),
        Line::from(Span::styled(
            "  Pressure Thresholds",
            Style::default().fg(FOREGROUND).add_modifier(Modifier::BOLD),
        )),
        system_kv_line(
            "Severe pending",
            format_bytes(p.severe_compaction_pending_bytes),
            CYAN,
        ),
        system_kv_line(
            "Moderate pending",
            format_bytes(p.moderate_compaction_pending_bytes),
            CYAN,
        ),
        system_kv_line(
            "Severe imm tables",
            format!("{}", p.severe_immutable_memtables),
            CYAN,
        ),
        system_kv_line(
            "Moderate imm tables",
            format!("{}", p.moderate_immutable_memtables),
            CYAN,
        ),
        system_kv_line(
            "Drain pending",
            format_bytes(p.drain_pending_bytes_threshold),
            CYAN,
        ),
    ]
}

fn draw_system_params_wide(
    f: &mut Frame,
    p: &ckbadger_store::MemoryProfile,
    direct_io_reads: bool,
    area: Rect,
) {
    let block = Block::default()
        .title(Span::styled(
            " RocksDB Parameters (TUI Secondary) ",
            Style::default().fg(FOREGROUND).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SLATE_800));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let col_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ])
        .split(inner);

    f.render_widget(Paragraph::new(system_normal_mode_lines(p)), col_chunks[0]);
    f.render_widget(Paragraph::new(system_bulk_sync_lines(p)), col_chunks[1]);
    f.render_widget(
        Paragraph::new(system_fixed_lines(p, direct_io_reads)),
        col_chunks[2],
    );
}

fn draw_system_params_compact(
    f: &mut Frame,
    p: &ckbadger_store::MemoryProfile,
    direct_io_reads: bool,
    area: Rect,
) {
    let block = Block::default()
        .title(Span::styled(
            " RocksDB Parameters (TUI Secondary) ",
            Style::default().fg(FOREGROUND).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SLATE_800));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines = system_normal_mode_lines(p);
    lines.push(Line::from(""));
    lines.extend(system_bulk_sync_lines(p));
    lines.push(Line::from(""));
    lines.extend(system_fixed_lines(p, direct_io_reads));

    f.render_widget(Paragraph::new(lines), inner);
}

#[cfg(test)]
mod tests {
    use super::{
        adaptive_control_lines, adaptive_state_label, api_health_state, build_batch_left_column,
        build_finalize_left_column, chart_height_warning, compact_overview_layout,
        compact_sync_layout, consumed_cells_source_color, consumed_cells_source_label,
        dense_right_lines, detail_right_lines, diagnostics_dense_panel, direct_io_reads_label,
        eta_confidence_label, footer_hint_line, footer_status_message, format_age_secs, format_num,
        format_num_commas, format_num_compact, format_rate_pair, format_signed_num_i128,
        format_stage_commit_gap_ms, header_right_line, header_title_line, heartbeat_is_on,
        io_fetch_write_jitter_line, is_rate_drop, merged_sparkline_p95_line,
        overview_log_min_height, overview_services_min_height, percentile_from_history,
        pipeline_bottleneck, pipeline_flow_state, rate_jitter, render_gauge, runtime_health_state,
        runtime_live_delta, service_log_tails_line, sparkline, stack_sync_charts, stale_age_secs,
        stale_status, startup_phase_label, storage_pressure_l0_line, storage_pressure_wbm_line,
        storage_runtime_columns, supervisor_services_line, sync_bottleneck, sync_chart_specs,
        sync_timing_lines, system_kv_line, system_store_path_lines, system_workdir_lines,
        trend_delta, trim_for_panel, AdaptiveControlSnapshot, App, Color, CompactOverviewLayout,
        CompactSyncLayout, DiagnosticsViewMode, SyncBottleneck, SyncChartKind, AMBER, CYAN,
        STATUS_MESSAGE_TTL_SECS, TERMINAL_DIM,
    };
    use crate::db::{
        ApiServiceInfo, RuntimeDiagData, ServiceLogTailData, SupervisorServiceData, TuiDb,
    };
    use ckbadger_common::{BulkBuildProgressData, MemoryStatsData};
    use ratatui::layout::{Constraint, Direction, Layout, Rect};
    use ratatui::text::Line;
    use std::collections::VecDeque;
    use std::time::Instant;

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
        let line = header_right_line("0.1.0@abc123", Some(12), "10:23:45");
        let text = line_text(&line);
        assert!(text.contains("0.1.0@abc123"));
        assert!(text.contains("stale 12s"));
        assert!(text.contains("10:23:45"));
        assert!(!text.contains("ago"));
    }

    #[test]
    fn test_header_right_line_without_stale_data() {
        let line = header_right_line("0.1.0@abc123", None, "10:23:45");
        let text = line_text(&line);
        assert!(text.contains("0.1.0@abc123"));
        assert!(text.contains("stale N/A"));
    }

    #[test]
    fn test_stale_age_secs_handles_missing_or_zero_timestamp() {
        assert_eq!(stale_age_secs(None), None);
        let zero_ts = MemoryStatsData {
            updated_at: 0,
            ..Default::default()
        };
        assert_eq!(stale_age_secs(Some(&zero_ts)), None);
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
        let line =
            io_fetch_write_jitter_line("123.4ms", "567.8ms", "34.5ms", "+533.3ms", "9.0 blk/s");
        let text = line_text(&line);
        assert!(
            text.starts_with("I/O Fetch 123.4ms Write(stage) 567.8ms Commit 34.5ms Gap +533.3ms")
        );
        assert!(text.contains("jitter 9.0 blk/s"));
    }

    #[test]
    fn test_format_stage_commit_gap_ms() {
        assert_eq!(
            format_stage_commit_gap_ms(Some(120.0), Some(45.0)),
            "+75.0ms"
        );
        assert_eq!(
            format_stage_commit_gap_ms(Some(45.0), Some(120.0)),
            "-75.0ms"
        );
        assert_eq!(format_stage_commit_gap_ms(None, Some(120.0)), "-");
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
    fn test_adaptive_control_lines_dense() {
        let lines = adaptive_control_lines(
            AdaptiveControlSnapshot {
                last_batch_blocks: Some(512),
                adaptive_inflight_batches: Some(2),
                adaptive_target_batch_txs: Some(40_000),
                adaptive_inflight_limit: Some(3),
                adaptive_min_target_batch_txs: Some(10_000),
                adaptive_cooldown_steps: Some(2),
                adaptive_last_reason: Some("pressure_backoff"),
                adaptive_adjustment_seq: Some(12),
                adaptive_last_adjusted_age_secs: Some(7),
                adaptive_backoff_streak: Some(3),
            },
            true,
            Some(5),
            Some("batch mismatch"),
        );
        assert_eq!(lines.len(), 1, "dense mode should return 1 line");
        let text = line_text(&lines[0]);
        assert!(text.contains("Adaptive"));
        assert!(text.contains("batch 512 blk"));
        assert!(text.contains("inflight 2/3"));
        assert!(text.contains("target 40K tx"));
        assert!(text.contains("floor 10K"));
        assert!(text.contains("BACKOFF x3"));
    }

    #[test]
    fn test_adaptive_control_lines_detail_with_reset() {
        let lines = adaptive_control_lines(
            AdaptiveControlSnapshot {
                last_batch_blocks: Some(512),
                adaptive_inflight_batches: Some(2),
                adaptive_target_batch_txs: Some(40_000),
                adaptive_inflight_limit: Some(3),
                adaptive_min_target_batch_txs: Some(10_000),
                adaptive_cooldown_steps: Some(2),
                adaptive_last_reason: Some("pressure_backoff"),
                adaptive_adjustment_seq: Some(12),
                adaptive_last_adjusted_age_secs: Some(7),
                adaptive_backoff_streak: Some(3),
            },
            false,
            Some(5),
            Some("batch mismatch"),
        );
        assert_eq!(lines.len(), 2, "detail mode should return 2 lines");
        let text1 = line_text(&lines[0]);
        assert!(text1.contains("Adaptive"));
        assert!(text1.contains("batch 512 blk"));
        assert!(text1.contains("target 40K tx"));
        let text2 = line_text(&lines[1]);
        assert!(text2.contains("cooldown 2"));
        assert!(text2.contains("adj #12"));
        assert!(text2.contains("7s ago"));
        assert!(text2.contains("backoff x3"));
        assert!(text2.contains("reset #5 batch mismatch"));
    }

    #[test]
    fn test_adaptive_control_lines_detail_no_reset() {
        let lines = adaptive_control_lines(
            AdaptiveControlSnapshot {
                last_batch_blocks: Some(100),
                adaptive_inflight_batches: None,
                adaptive_target_batch_txs: Some(5_000),
                adaptive_inflight_limit: None,
                adaptive_min_target_batch_txs: Some(1_000),
                adaptive_cooldown_steps: None,
                adaptive_last_reason: None,
                adaptive_adjustment_seq: None,
                adaptive_last_adjusted_age_secs: None,
                adaptive_backoff_streak: None,
            },
            false,
            Some(0),
            None,
        );
        assert_eq!(lines.len(), 2);
        let text2 = line_text(&lines[1]);
        assert!(!text2.contains("reset"), "epoch 0 should not show reset");
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
    fn test_dense_right_lines_order() {
        let lines = dense_right_lines(
            Line::from("FP"),
            Line::from("WC"),
            Line::from("L0"),
            Line::from("WBM"),
            Line::from("Compact"),
            Line::from("Stability"),
        );
        let labels: Vec<String> = lines.iter().map(line_text).collect();
        assert_eq!(
            labels,
            vec!["FP", "WC", "L0", "WBM", "Compact", "Stability"]
        );
    }

    #[test]
    fn test_detail_right_lines_order() {
        let lines = detail_right_lines(
            Line::from("FP"),
            Line::from("WC"),
            Line::from("L0"),
            Line::from("WBM"),
            Line::from("Compact"),
            Line::from("Stability"),
            Line::from("I/O"),
        );
        let labels: Vec<String> = lines.iter().map(line_text).collect();
        assert_eq!(
            labels,
            vec!["FP", "WC", "L0", "WBM", "Compact", "Stability", "I/O"]
        );
    }

    #[test]
    fn test_sync_timing_lines_do_not_show_data_source() {
        let lines = sync_timing_lines(
            Some("2m 03s"),
            None,
            Some("17m 12s"),
            Some("bulk_sync"),
            false,
        );
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
        let lines = sync_timing_lines(None, None, None, None, false);
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
        let stacked = sync_chart_specs(true);
        assert_eq!(stacked.len(), 2);
        assert_eq!(stacked[0].kind, SyncChartKind::BlockRate);
        assert_eq!(stacked[1].kind, SyncChartKind::TxRate);

        let wide = sync_chart_specs(false);
        assert_eq!(wide.len(), 3);
        assert_eq!(wide[0].kind, SyncChartKind::BlockRate);
        assert_eq!(wide[1].kind, SyncChartKind::TxRate);
        assert_eq!(wide[2].kind, SyncChartKind::WriteLatency);
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
            runtime_health_state(None, None),
            ("N/A", Color::Rgb(160, 174, 192))
        );

        let idle = RuntimeDiagData::default();
        assert_eq!(
            runtime_health_state(Some(&idle), None),
            ("IDLE", Color::Rgb(160, 174, 192))
        );

        let ok = RuntimeDiagData {
            active_run_id: Some("run-1".to_string()),
            heartbeat_age_secs: Some(10),
            ..Default::default()
        };
        assert_eq!(
            runtime_health_state(Some(&ok), None),
            ("OK", Color::Rgb(0, 255, 65))
        );

        let stale = RuntimeDiagData {
            active_run_id: Some("run-1".to_string()),
            heartbeat_age_secs: Some(61),
            ..Default::default()
        };
        assert_eq!(
            runtime_health_state(Some(&stale), None),
            ("STALE", Color::Rgb(255, 176, 0))
        );

        let warn = RuntimeDiagData {
            active_run_id: Some("run-1".to_string()),
            heartbeat_age_secs: Some(10),
            heartbeat_oom_kill_events: Some(1),
            ..Default::default()
        };
        assert_eq!(
            runtime_health_state(Some(&warn), None),
            ("WARN", Color::Rgb(255, 176, 0))
        );
    }

    #[test]
    fn test_runtime_health_state_with_supervisor_only() {
        let services = vec![SupervisorServiceData {
            name: "indexer".to_string(),
            pid: 123,
            status: "running".to_string(),
            uptime_secs: 9,
        }];
        assert_eq!(
            runtime_health_state(None, Some(&services)),
            ("SUP", Color::Rgb(56, 189, 248))
        );
    }

    #[test]
    fn test_supervisor_services_line_format() {
        let services = vec![
            SupervisorServiceData {
                name: "indexer".to_string(),
                pid: 123,
                status: "running".to_string(),
                uptime_secs: 9,
            },
            SupervisorServiceData {
                name: "frontend-server".to_string(),
                pid: 456,
                status: "restarting".to_string(),
                uptime_secs: 2,
            },
        ];

        let line = supervisor_services_line(Some(&services)).expect("service line");
        let text = line_text(&line);
        assert!(text.contains("Svc"));
        assert!(text.contains("indexer:running#123(9s)"));
        assert!(text.contains("frontend:restarting#456(2s)"));
    }

    #[test]
    fn test_service_log_tails_line_format() {
        let tails = vec![
            ServiceLogTailData {
                service: "api".to_string(),
                last_line: "bind failed: addr in use".to_string(),
            },
            ServiceLogTailData {
                service: "indexer".to_string(),
                last_line: "pipeline batch mismatch at block 123".to_string(),
            },
        ];

        let line = service_log_tails_line(Some(&tails), 120).expect("tail line");
        let text = line_text(&line);
        assert!(text.contains("Tail"));
        assert!(text.contains("api: bind failed: addr in use"));
        assert!(text.contains("indexer: pipeline batch mismatch at block 123"));
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
        assert_eq!(trim_for_panel("abcdef", 0), "");
        assert_eq!(trim_for_panel("abcdef", 6), "...");
        assert_eq!(trim_for_panel("abcdefghijkl", 10), "a...");
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
    fn test_system_kv_line_format() {
        let line = system_kv_line("Test label", "value".to_string(), CYAN);
        let text = line_text(&line);
        assert!(text.contains("Test label"));
        assert!(text.contains("value"));
        // Label is left-padded to 22 chars + 2-char indent = 24 chars before value
        assert!(text.starts_with("  Test label"));
    }

    #[test]
    fn test_system_workdir_lines_include_both_roots() {
        let lines = system_workdir_lines(
            std::path::Path::new("/workdir/ckbadger"),
            std::path::Path::new("/workdir/ckb"),
        );
        assert!(line_text(&lines[0]).contains("CKB workdir"));
        assert!(line_text(&lines[1]).contains("ckbadger workdir"));
        let text = lines
            .iter()
            .map(line_text)
            .collect::<Vec<String>>()
            .join(" ");
        assert!(text.contains("CKB workdir"));
        assert!(text.contains("/workdir/ckb"));
        assert!(text.contains("ckbadger workdir"));
        assert!(text.contains("/workdir/ckbadger"));
    }

    #[test]
    fn test_system_store_path_lines_include_ckb_rocksdb() {
        let lines = system_store_path_lines(
            std::path::Path::new("/data/domain"),
            std::path::Path::new("/data/append-only"),
            std::path::Path::new("/ckb/data/db"),
        );
        assert!(line_text(&lines[0]).contains("CKB RocksDB"));
        let text = lines
            .iter()
            .map(line_text)
            .collect::<Vec<String>>()
            .join(" ");
        assert!(text.contains("Domain store"));
        assert!(text.contains("/data/domain"));
        assert!(text.contains("Append-only store"));
        assert!(text.contains("/data/append-only"));
        assert!(text.contains("CKB RocksDB"));
        assert!(text.contains("/ckb/data/db"));
    }

    #[test]
    fn test_direct_io_reads_label() {
        assert_eq!(direct_io_reads_label(true), "reads + flush/compact");
        assert_eq!(direct_io_reads_label(false), "flush/compact only");
    }

    #[test]
    fn test_stale_status() {
        assert_eq!(
            stale_status(None),
            ("stale N/A".to_string(), Color::Rgb(160, 174, 192))
        );
        assert_eq!(
            stale_status(Some(12)),
            ("stale 12s".to_string(), Color::Rgb(0, 204, 51))
        );
        assert_eq!(
            stale_status(Some(31)),
            ("stale 31s".to_string(), Color::Rgb(255, 176, 0))
        );
    }

    #[test]
    fn test_footer_hint_line_adapts_to_width() {
        let compact = line_text(&footer_hint_line(80));
        assert!(compact.contains("q quit"));
        assert!(compact.contains("Tab/h/l tabs"));
        assert!(!compact.contains("diag-view"));

        let medium = line_text(&footer_hint_line(100));
        assert!(medium.contains("j/k scroll"));
        assert!(!medium.contains("diag-view"));

        let wide = line_text(&footer_hint_line(140));
        assert!(wide.contains("diag-view"));
        assert!(wide.contains("log-scroll"));
    }

    #[test]
    fn test_footer_status_message_ttl() {
        let fresh = ("fresh".to_string(), Instant::now());
        let shown = footer_status_message(Some(&fresh));
        assert_eq!(shown.as_ref().map(|(msg, _)| msg.as_str()), Some("fresh"));

        let expired = (
            "expired".to_string(),
            Instant::now() - std::time::Duration::from_secs(STATUS_MESSAGE_TTL_SECS + 1),
        );
        assert!(footer_status_message(Some(&expired)).is_none());
    }

    #[tokio::test]
    async fn test_app_refresh_without_store_dependency() {
        let db = TuiDb::new(
            "http://127.0.0.1:9/api/v1",
            "/tmp/ckbadger-store",
            "/tmp/ckbadger-store-append-only",
        )
        .await;
        let mut app = App::new(db, "test".to_string());
        app.refresh().await;

        assert!(app.sync_status.is_none());
        assert!(app.memory_stats.is_none());
    }

    #[tokio::test]
    async fn test_log_warning_deduplicates_recent_same_message() {
        let db = TuiDb::new(
            "http://127.0.0.1:9/api/v1",
            "/tmp/ckbadger-store",
            "/tmp/ckbadger-store-append-only",
        )
        .await;

        let mut app = App::new(db, "test".to_string());
        let initial_logs = app.log_entries.len();
        app.log_warning("repeat warning".to_string());
        let after_first = app.log_entries.len();
        app.log_warning("repeat warning".to_string());
        let after_second = app.log_entries.len();
        app.log_warning("different warning".to_string());
        let after_third = app.log_entries.len();

        assert_eq!(after_first, initial_logs + 1);
        assert_eq!(after_second, after_first);
        assert_eq!(after_third, after_second + 1);

        drop(app);
    }

    #[test]
    fn test_percentile_from_history_empty() {
        let history: VecDeque<f64> = VecDeque::new();
        assert!(percentile_from_history(&history, 300, 95.0).is_none());
    }

    #[test]
    fn test_percentile_from_history_single_element() {
        let mut history: VecDeque<f64> = VecDeque::new();
        history.push_back(42.0);
        assert_eq!(percentile_from_history(&history, 300, 95.0), Some(42.0));
    }

    #[test]
    fn test_percentile_from_history_identical_values() {
        let history: VecDeque<f64> = vec![10.0; 100].into_iter().collect();
        assert_eq!(percentile_from_history(&history, 300, 95.0), Some(10.0));
    }

    #[test]
    fn test_percentile_from_history_normal_case() {
        let history: VecDeque<f64> = (0..100).map(|i| i as f64).collect();
        let p95 = percentile_from_history(&history, 300, 95.0).unwrap();
        assert!((p95 - 95.0).abs() < 1.0);
    }

    #[test]
    fn test_percentile_from_history_window_smaller_than_history() {
        let history: VecDeque<f64> = (0..300).map(|i| i as f64).collect();
        let p95 = percentile_from_history(&history, 50, 95.0).unwrap();
        assert!((295.0..=300.0).contains(&p95));
    }

    #[test]
    fn test_render_gauge_zero_total() {
        assert_eq!(render_gauge(0, 0), "");
    }

    #[test]
    fn test_render_gauge_empty() {
        assert_eq!(render_gauge(0, 10), "░░░░░░░░░░");
    }

    #[test]
    fn test_render_gauge_full() {
        assert_eq!(render_gauge(10, 10), "██████████");
    }

    #[test]
    fn test_render_gauge_half() {
        let g = render_gauge(5, 10);
        assert_eq!(g, "█████░░░░░");
    }

    #[test]
    fn test_format_num_compact_small() {
        assert_eq!(format_num_compact(0), "0");
        assert_eq!(format_num_compact(999), "999");
    }

    #[test]
    fn test_format_num_compact_thousands() {
        assert_eq!(format_num_compact(1_000), "1K");
        assert_eq!(format_num_compact(50_000), "50K");
        assert_eq!(format_num_compact(180_000), "180K");
        assert_eq!(format_num_compact(999_999), "999K");
    }

    #[test]
    fn test_format_num_compact_millions() {
        assert_eq!(format_num_compact(1_000_000), "1.0M");
        assert_eq!(format_num_compact(1_500_000), "1.5M");
        assert_eq!(format_num_compact(12_300_000), "12.3M");
    }

    #[test]
    fn test_storage_pressure_wbm_line_zero_budget() {
        let mem = MemoryStatsData {
            wbm_budget_bytes: 0,
            wbm_usage_bytes: 0,
            ..MemoryStatsData::new()
        };
        let line = storage_pressure_wbm_line(&mem, 10);
        let text = line_text(&line);
        assert!(text.contains("WBM"));
        assert!(text.contains("-"));
    }

    #[test]
    fn test_storage_pressure_l0_thresholds() {
        let mut mem = MemoryStatsData::new();
        mem.l0_files_max = 5;
        let line = storage_pressure_l0_line(&mem, 10);
        assert!(line_text(&line).contains("[OK]"));

        mem.l0_files_max = 12;
        let line = storage_pressure_l0_line(&mem, 10);
        assert!(line_text(&line).contains("[WARN]"));

        mem.l0_files_max = 18;
        let line = storage_pressure_l0_line(&mem, 10);
        assert!(line_text(&line).contains("[HOT]"));
    }

    #[test]
    fn test_merged_sparkline_no_p95() {
        let history: VecDeque<f64> = vec![1.0, 2.0, 3.0].into_iter().collect();
        let line =
            merged_sparkline_p95_line("F", TERMINAL_DIM, &history, "P", AMBER, &history, 8, false);
        let text = line_text(&line);
        assert!(text.contains("F"));
        assert!(text.contains("P"));
        assert!(!text.contains("p95"));
    }

    #[test]
    fn test_finalize_checklist_first_phase() {
        let bb = BulkBuildProgressData {
            finalize_phase: Some("drain_flush".to_string()),
            finalize_step: Some(0),
            finalize_steps_total: Some(13),
            finalize_elapsed_ms: Some(500.0),
            ..Default::default()
        };
        let lines = build_finalize_left_column(&bb, 20);
        let header = line_text(&lines[0]);
        assert!(header.contains("FINALIZING"));
        assert!(header.contains("step 1/13"));
        // First phase should have the active marker (►)
        let first_phase = line_text(&lines[1]);
        assert!(first_phase.contains("Drain flush"));
        // Second phase should not have the check marker
        let second_phase = line_text(&lines[2]);
        assert!(second_phase.contains("Activity stats"));
    }

    #[test]
    fn test_finalize_checklist_mid_owner() {
        let bb = BulkBuildProgressData {
            finalize_phase: Some("owner:script".to_string()),
            finalize_step: Some(5),
            finalize_steps_total: Some(13),
            finalize_elapsed_ms: Some(12400.0),
            ..Default::default()
        };
        let lines = build_finalize_left_column(&bb, 20);
        let header = line_text(&lines[0]);
        assert!(header.contains("step 6/13"));
        // Phases 0-4 (indices 1-5) should be completed (✓)
        for (idx, line) in lines[1..=5].iter().enumerate() {
            let text = line_text(line);
            assert!(
                text.contains('\u{2713}'),
                "phase {} should be ✓: {}",
                idx + 1,
                text
            );
        }
        // Phase 5 (index 6) should be active (►)
        let active = line_text(&lines[6]);
        assert!(
            active.contains('\u{25b8}'),
            "phase 5 should be ►: {}",
            active
        );
        assert!(active.contains("Owner: script"));
    }

    #[test]
    fn test_finalize_checklist_compact_mode() {
        let bb = BulkBuildProgressData {
            finalize_phase: Some("owner:address".to_string()),
            finalize_step: Some(4),
            finalize_steps_total: Some(13),
            finalize_elapsed_ms: Some(8000.0),
            ..Default::default()
        };
        // Compact: available_height < 6 → only header + current phase
        let lines = build_finalize_left_column(&bb, 4);
        assert_eq!(lines.len(), 2);
        let current = line_text(&lines[1]);
        assert!(current.contains("Owner: address"));
    }

    #[test]
    fn test_finalize_checklist_grouped_mode() {
        let bb = BulkBuildProgressData {
            finalize_phase: Some("owner:dao".to_string()),
            finalize_step: Some(7),
            finalize_steps_total: Some(13),
            finalize_elapsed_ms: Some(20000.0),
            ..Default::default()
        };
        // Medium height: groups tail phases
        let lines = build_finalize_left_column(&bb, 12);
        let all_text: String = lines
            .iter()
            .map(|l| line_text(l))
            .collect::<Vec<_>>()
            .join("|");
        assert!(
            all_text.contains("dao / fiber / object"),
            "should group tail owners: {}",
            all_text
        );
        assert!(
            all_text.contains("Metadata / Flush / Status"),
            "should group tail phases: {}",
            all_text
        );
    }

    #[test]
    fn test_batch_left_column_shows_adaptive_ema() {
        let bb = BulkBuildProgressData {
            facts_ms: Some(10.0),
            resolve_ms: Some(8.0),
            reduce_ms: Some(6.0),
            history_ms: Some(4.0),
            address_reduce_ms: Some(2.0),
            activity_stats_ms: Some(1.0),
            flush_ms: Some(50.0),
            fetch_ms: Some(100.0),
            build_ms: Some(31.0),
            batch_block_span: Some(30_000),
            batch_count: Some(5),
            tx_density: Some(3.5),
            ms_per_block_ema: Some(0.042),
            controllable_ms: Some(1380.0),
            target_iteration_ms: Some(1500.0),
            ..Default::default()
        };
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(60), Constraint::Length(40)])
            .split(Rect::new(0, 0, 100, 20));
        let lines = build_batch_left_column(&bb, &cols, false);
        let all_text: String = lines
            .iter()
            .map(|l| line_text(l))
            .collect::<Vec<_>>()
            .join("|");
        assert!(
            all_text.contains("Adaptive"),
            "should show Adaptive line: {}",
            all_text
        );
        assert!(
            all_text.contains("EMA"),
            "should show EMA label: {}",
            all_text
        );
        assert!(
            all_text.contains("ms/blk"),
            "should show ms/blk unit: {}",
            all_text
        );
        assert!(
            all_text.contains("Budget"),
            "should show Budget label: {}",
            all_text
        );
        assert!(
            all_text.contains("1380/1500 ms"),
            "should show controllable/target: {}",
            all_text
        );
        assert!(
            all_text.contains("[10-100k]"),
            "should show span bounds: {}",
            all_text
        );
    }

    #[test]
    fn test_overlap_ratio_fully_hidden() {
        let fetch_ms = 200.0_f64;
        let prefetch_collect_ms = 0.0_f64;
        let fetch_overlap = (1.0 - prefetch_collect_ms / fetch_ms).clamp(0.0, 1.0);
        assert!((fetch_overlap - 1.0).abs() < f64::EPSILON);

        let flush_ms = 50.0_f64;
        let flush_wait_ms = 0.0_f64;
        let flush_overlap = (1.0 - flush_wait_ms / flush_ms).clamp(0.0, 1.0);
        assert!((flush_overlap - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_overlap_ratio_partially_hidden() {
        let fetch_ms = 200.0_f64;
        let prefetch_collect_ms = 50.0_f64;
        let fetch_overlap = (1.0 - prefetch_collect_ms / fetch_ms).clamp(0.0, 1.0);
        assert!((fetch_overlap - 0.75).abs() < 0.001);
    }

    #[test]
    fn test_overlap_ratio_zero_duration() {
        let fetch_ms = 0.0_f64;
        let fetch_overlap = if fetch_ms > 0.0 {
            (1.0 - 0.0 / fetch_ms).clamp(0.0, 1.0)
        } else {
            1.0
        };
        assert!((fetch_overlap - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_idle_ratio() {
        let build_ms = 3000.0_f64;
        let prefetch_collect_ms = 50.0_f64;
        let flush_wait_ms = 100.0_f64;
        let iteration_ms = build_ms + prefetch_collect_ms + flush_wait_ms;
        let idle_ratio = (prefetch_collect_ms + flush_wait_ms) / iteration_ms;
        assert!((idle_ratio - 150.0 / 3150.0).abs() < 0.001);
    }

    #[test]
    fn test_gantt_bar_positions_collapsed() {
        let build_ms = 3000.0_f64;
        let prefetch_collect_ms = 50.0_f64;
        let flush_wait_ms = 100.0_f64;
        let fetch_ms = 200.0_f64;
        let flush_ms = 80.0_f64;
        let iteration_ms = build_ms + prefetch_collect_ms + flush_wait_ms;

        // BUILD: 0 → build_ms
        assert!((0.0_f64).abs() < f64::EPSILON);
        assert!((build_ms - 3000.0).abs() < f64::EPSILON);

        // FETCH: ends at build_ms + prefetch_collect_ms, extends left by fetch_ms
        let fetch_end = build_ms + prefetch_collect_ms;
        let fetch_start = (fetch_end - fetch_ms).max(0.0);
        assert!((fetch_start - 2850.0).abs() < f64::EPSILON);
        assert!((fetch_end - 3050.0).abs() < f64::EPSILON);

        // FLUSH: starts at iteration_ms
        let flush_start = iteration_ms;
        let flush_end = iteration_ms + flush_ms;
        assert!((flush_start - 3150.0).abs() < f64::EPSILON);
        assert!((flush_end - 3230.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_subphase_invariant() {
        let facts_ms = 10.0_f64;
        let resolve_ms = 5.0_f64;
        let reduce_ms = 8.0_f64;
        let history_ms = 3.0_f64;
        let addr_ms = 2.0_f64;
        let actvty_ms = 1.0_f64;
        let build_ms = 29.0_f64;

        let sub_phase_sum = facts_ms + resolve_ms + reduce_ms + history_ms + addr_ms + actvty_ms;
        assert!(
            (sub_phase_sum - build_ms).abs() < 0.5,
            "sub-phase sum {sub_phase_sum} should be within 0.5ms of build_ms {build_ms}"
        );
    }
}
