use chrono::{DateTime, Local};
use ckbadger_common::MemoryStatsData;
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

fn header_right_line(stale_secs: Option<i64>, clock_text: &str) -> Line<'static> {
    let (stale_text, stale_color) = stale_status(stale_secs);
    Line::from(vec![
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
                    Constraint::Length(10),
                    Constraint::Length(6),
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
                    Constraint::Length(10),
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

    let line = Line::from(vec![
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
        derived_status_line(
            sync.derived_tip_block,
            sync.derived_lag_blocks,
            sync.derived_sync_in_progress,
        ),
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
        sync.elapsed_time.as_deref(),
        sync.startup_phase.as_deref(),
    );
    f.render_widget(Paragraph::new(right), cols[2]);
}

fn draw_sync_charts(f: &mut Frame, app: &App, area: Rect) {
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
        let specs = sync_chart_specs(false);
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
            specs[0].title,
            specs[0].unit,
            sync_chart_data(app, specs[0].kind),
        );
        draw_chart_panel(
            f,
            cols[1],
            specs[1].title,
            specs[1].unit,
            sync_chart_data(app, specs[1].kind),
        );
        draw_chart_panel(
            f,
            cols[2],
            specs[2].title,
            specs[2].unit,
            sync_chart_data(app, specs[2].kind),
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncChartKind {
    BlockRate,
    TxRate,
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
                    adaptive_control_line(AdaptiveControlSnapshot {
                        last_batch_blocks: sync.last_batch_blocks,
                        adaptive_inflight_batches,
                        adaptive_target_batch_txs: sync.adaptive_target_batch_txs,
                        adaptive_inflight_limit: sync.adaptive_inflight_limit,
                        adaptive_min_target_batch_txs: sync.adaptive_min_target_batch_txs,
                        adaptive_cooldown_steps: sync.adaptive_cooldown_steps,
                        adaptive_last_reason: sync.adaptive_last_reason.as_deref(),
                        adaptive_adjustment_seq: sync.adaptive_adjustment_seq,
                        adaptive_last_adjusted_age_secs: sync.adaptive_last_adjusted_age_secs,
                        adaptive_backoff_streak: sync.adaptive_backoff_streak,
                    }),
                    pipeline_reset_line(
                        sync.pipeline_reset_epoch,
                        sync.pipeline_reset_reason.as_deref(),
                    ),
                ],
                dense_right_lines(
                    stage_trend_line("F", TERMINAL_DIM, &app.fetch_stage_history, spark_width),
                    stage_trend_line("P", AMBER, &app.parse_stage_history, spark_width),
                    stage_trend_line("W", TERMINAL_GREEN, &app.write_stage_history, spark_width),
                    stage_trend_line("C", CYAN, &app.db_commit_history, spark_width),
                    Line::from(vec![
                        Span::styled("Stability ", Style::default().fg(SLATE_500)),
                        Span::styled(stability, Style::default().fg(stability_color)),
                    ]),
                    io_fetch_write_jitter_line(
                        &fetch_ms_text,
                        &pipeline_write_stage_ms_text,
                        &pipeline_commit_ms_text,
                        &pipeline_gap_ms_text,
                        &rate_jitter_text,
                    ),
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
                    adaptive_control_line(AdaptiveControlSnapshot {
                        last_batch_blocks: sync.last_batch_blocks,
                        adaptive_inflight_batches,
                        adaptive_target_batch_txs: sync.adaptive_target_batch_txs,
                        adaptive_inflight_limit: sync.adaptive_inflight_limit,
                        adaptive_min_target_batch_txs: sync.adaptive_min_target_batch_txs,
                        adaptive_cooldown_steps: sync.adaptive_cooldown_steps,
                        adaptive_last_reason: sync.adaptive_last_reason.as_deref(),
                        adaptive_adjustment_seq: sync.adaptive_adjustment_seq,
                        adaptive_last_adjusted_age_secs: sync.adaptive_last_adjusted_age_secs,
                        adaptive_backoff_streak: sync.adaptive_backoff_streak,
                    }),
                    pipeline_reset_line(
                        sync.pipeline_reset_epoch,
                        sync.pipeline_reset_reason.as_deref(),
                    ),
                ],
                detail_right_lines(
                    stage_trend_line("F", TERMINAL_DIM, &app.fetch_stage_history, spark_width),
                    stage_trend_line("P", AMBER, &app.parse_stage_history, spark_width),
                    stage_trend_line("W", TERMINAL_GREEN, &app.write_stage_history, spark_width),
                    stage_trend_line("C", CYAN, &app.db_commit_history, spark_width),
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
                    io_fetch_write_jitter_line(
                        &fetch_ms_text,
                        &pipeline_write_stage_ms_text,
                        &pipeline_commit_ms_text,
                        &pipeline_gap_ms_text,
                        &rate_jitter_text,
                    ),
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
                adaptive_control_line(AdaptiveControlSnapshot {
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
                }),
                pipeline_reset_line(
                    sync.pipeline_reset_epoch,
                    sync.pipeline_reset_reason.as_deref(),
                ),
            ],
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
    commit_line: Line<'static>,
    stability_line: Line<'static>,
    io_line: Line<'static>,
) -> Vec<Line<'static>> {
    vec![
        stability_line,
        fetch_line,
        parse_line,
        write_line,
        commit_line,
        io_line,
    ]
}

fn detail_right_lines(
    fetch_line: Line<'static>,
    parse_line: Line<'static>,
    write_line: Line<'static>,
    commit_line: Line<'static>,
    stability_line: Line<'static>,
    rate_line: Line<'static>,
    io_line: Line<'static>,
) -> Vec<Line<'static>> {
    vec![
        stability_line,
        fetch_line,
        parse_line,
        write_line,
        commit_line,
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

fn derived_status_line(
    derived_tip_block: Option<i64>,
    derived_lag_blocks: Option<i64>,
    derived_sync_in_progress: bool,
) -> Line<'static> {
    let Some(derived_tip_block) = derived_tip_block else {
        return Line::from(vec![
            Span::styled("Derived: ", Style::default().fg(SLATE_500)),
            Span::styled("-", Style::default().fg(SLATE_500)),
        ]);
    };

    let lag_blocks = derived_lag_blocks.unwrap_or(0).max(0);
    let syncing = derived_sync_in_progress || lag_blocks > 0;
    let state_text = if syncing { "syncing" } else { "ready" };
    let state_color = if syncing { AMBER } else { TERMINAL_GREEN };

    Line::from(vec![
        Span::styled("Derived: ", Style::default().fg(SLATE_500)),
        Span::styled(
            format_num(derived_tip_block),
            Style::default()
                .fg(TERMINAL_GREEN)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" (lag ", Style::default().fg(SLATE_500)),
        Span::styled(format_num(lag_blocks), Style::default().fg(state_color)),
        Span::styled(", ", Style::default().fg(SLATE_500)),
        Span::styled(state_text, Style::default().fg(state_color)),
        Span::styled(")", Style::default().fg(SLATE_500)),
    ])
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
    if info.derived_syncing {
        return ("DEGRADED", CYAN);
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

fn direct_io_reads_label() -> &'static str {
    let enabled = std::env::var("CKBADGER_DIRECT_IO_READS")
        .map(|v| v != "0" && v.to_lowercase() != "false")
        .unwrap_or(true);
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
            Constraint::Length(5),
            Constraint::Min(10),
        ])
        .split(area);

    // -- Section 1: System Environment --
    draw_system_environment(f, p, mem, chunks[0]);

    // -- Section 2: Store Paths --
    draw_system_paths(
        f,
        db.domain_data_path(),
        db.append_only_data_path(),
        chunks[1],
    );

    // -- Section 3: RocksDB Parameters --
    if compact {
        draw_system_params_compact(f, p, chunks[2]);
    } else {
        draw_system_params_wide(f, p, chunks[2]);
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

fn draw_system_paths(
    f: &mut Frame,
    domain_path: &std::path::Path,
    append_path: &std::path::Path,
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

    let domain_cf_count = DOMAIN_CFS.len();
    let append_cf_count = APPEND_CFS.len();

    let lines = vec![
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
    ];
    f.render_widget(Paragraph::new(lines), inner);
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

fn system_fixed_lines(p: &ckbadger_store::MemoryProfile) -> Vec<Line<'static>> {
    vec![
        Line::from(Span::styled(
            "  Fixed Constants",
            Style::default().fg(FOREGROUND).add_modifier(Modifier::BOLD),
        )),
        system_kv_line("Compression", "LZ4 (append: None)".to_string(), SLATE_500),
        system_kv_line("Direct I/O", direct_io_reads_label().to_string(), SLATE_500),
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

fn draw_system_params_wide(f: &mut Frame, p: &ckbadger_store::MemoryProfile, area: Rect) {
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
    f.render_widget(Paragraph::new(system_fixed_lines(p)), col_chunks[2]);
}

fn draw_system_params_compact(f: &mut Frame, p: &ckbadger_store::MemoryProfile, area: Rect) {
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
    lines.extend(system_fixed_lines(p));

    f.render_widget(Paragraph::new(lines), inner);
}

#[cfg(test)]
mod tests {
    use super::{
        adaptive_control_line, adaptive_state_label, api_health_state, chart_height_warning,
        compact_overview_layout, compact_sync_layout, consumed_cells_source_color,
        consumed_cells_source_label, dense_right_lines, derived_status_line, detail_right_lines,
        diagnostics_dense_panel, direct_io_reads_label, eta_confidence_label, footer_hint_line,
        footer_status_message, format_age_secs, format_num, format_num_commas, format_rate_pair,
        format_signed_num_i128, format_stage_commit_gap_ms, header_right_line, header_title_line,
        heartbeat_is_on, io_fetch_write_jitter_line, is_rate_drop, overview_log_min_height,
        overview_services_min_height, pipeline_bottleneck, pipeline_flow_state,
        pipeline_reset_line, rate_jitter, runtime_health_state, runtime_live_delta,
        service_log_tails_line, sparkline, stack_sync_charts, stale_age_secs, stale_status,
        startup_phase_label, storage_runtime_columns, supervisor_services_line, sync_bottleneck,
        sync_chart_specs, sync_timing_lines, system_kv_line, trend_delta, trim_for_panel,
        AdaptiveControlSnapshot, App, Color, CompactOverviewLayout, CompactSyncLayout,
        DiagnosticsViewMode, SyncBottleneck, SyncChartKind, CYAN, STATUS_MESSAGE_TTL_SECS,
        TERMINAL_DIM,
    };
    use crate::db::{
        ApiServiceInfo, RuntimeDiagData, ServiceLogTailData, SupervisorServiceData, TuiDb,
    };
    use ckbadger_common::MemoryStatsData;
    use ratatui::layout::Rect;
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
        let line = header_right_line(Some(12), "10:23:45");
        let text = line_text(&line);
        assert!(text.contains("stale 12s"));
        assert!(text.contains("10:23:45"));
        assert!(!text.contains("ago"));
    }

    #[test]
    fn test_header_right_line_without_stale_data() {
        let line = header_right_line(None, "10:23:45");
        let text = line_text(&line);
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
    fn test_adaptive_control_line_format() {
        let line = adaptive_control_line(AdaptiveControlSnapshot {
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
            Line::from("C"),
            Line::from("Stability"),
            Line::from("I/O"),
        );
        let labels: Vec<String> = lines.iter().map(line_text).collect();
        assert_eq!(labels, vec!["Stability", "F", "P", "W", "C", "I/O"]);
    }

    #[test]
    fn test_detail_right_lines_order() {
        let lines = detail_right_lines(
            Line::from("F"),
            Line::from("P"),
            Line::from("W"),
            Line::from("C"),
            Line::from("Stability"),
            Line::from("Rate"),
            Line::from("I/O"),
        );
        let labels: Vec<String> = lines.iter().map(line_text).collect();
        assert_eq!(labels, vec!["Stability", "F", "P", "W", "C", "Rate", "I/O"]);
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
    fn test_derived_status_line_ready() {
        let line = derived_status_line(Some(1_000), Some(0), false);
        let text = line_text(&line);
        assert!(text.contains("Derived: 1,000"));
        assert!(text.contains("lag 0"));
        assert!(text.contains("ready"));
    }

    #[test]
    fn test_derived_status_line_syncing() {
        let line = derived_status_line(Some(2_000), Some(12), true);
        let text = line_text(&line);
        assert!(text.contains("Derived: 2,000"));
        assert!(text.contains("lag 12"));
        assert!(text.contains("syncing"));
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

        let degraded = ApiServiceInfo {
            reachable: true,
            status_code: Some(503),
            derived_syncing: true,
            ..Default::default()
        };
        assert_eq!(
            api_health_state(&degraded),
            ("DEGRADED", Color::Rgb(56, 189, 248))
        );

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
    fn test_direct_io_reads_label() {
        // Default (no env var set) should include reads
        let label = direct_io_reads_label();
        assert!(
            label == "reads + flush/compact" || label == "flush/compact only",
            "unexpected label: {label}"
        );
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
        let mut app = App::new(db);
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

        let mut app = App::new(db);
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
}
