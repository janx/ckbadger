use anyhow::Result;
use chrono::{DateTime, Local};
use ckbadger_common::{LabelImportConfig, MemoryStatsData, Task, TaskBuilder};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Gauge, Paragraph, Row, Table, TableState, Wrap},
    Frame,
};
use ratatui_image::picker::Picker;
use std::collections::VecDeque;
use std::time::Instant;

use crate::chart::{render_bar_chart, ChartStats};
use crate::db::{ChainInfoData, SyncStatusRow, TaskDb};

const RATE_HISTORY_SIZE: usize = 3600;
const LOG_HISTORY_SIZE: usize = 100;

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

// Terminal green (primary) - matches frontend terminal-*
const TERMINAL_GREEN: Color = Color::Rgb(0, 255, 65); // #00ff41
const TERMINAL_DIM: Color = Color::Rgb(0, 204, 51); // #00cc33
#[allow(dead_code)]
const TERMINAL_DARK: Color = Color::Rgb(0, 128, 31); // #00801f

// Amber accent - matches frontend amber-*
const AMBER: Color = Color::Rgb(255, 176, 0); // #ffb000
#[allow(dead_code)]
const AMBER_BRIGHT: Color = Color::Rgb(255, 200, 50); // #ffc832
const AMBER_DIM: Color = Color::Rgb(204, 140, 0); // #cc8c00

// Slate (brightened for terminal readability)
const SLATE_800: Color = Color::Rgb(58, 71, 89); // borders
const SLATE_700: Color = Color::Rgb(80, 95, 115); // separators
const SLATE_600: Color = Color::Rgb(135, 150, 170); // labels
const SLATE_500: Color = Color::Rgb(160, 174, 192); // muted text

// CKB brand
const CKB_PRIMARY: Color = Color::Rgb(0, 195, 137); // #00c389

// Text
const FOREGROUND: Color = Color::Rgb(237, 237, 237); // #ededed

// Error
const ERROR_RED: Color = Color::Rgb(239, 68, 68); // #ef4444

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub enum DialogType {
    NewTask,
    Confirm(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum FocusedPanel {
    #[default]
    Tasks,
    Details,
    Log,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SyncTab {
    #[default]
    ChainInfo,
    SyncProgress,
}

pub struct App {
    db: TaskDb,
    tasks: Vec<Task>,
    sync_status: Option<SyncStatusRow>,
    memory_stats: Option<MemoryStatsData>,
    table_state: TableState,
    last_refresh: Instant,
    last_sample: Instant,
    dialog: Option<DialogType>,
    dialog_selection: usize,
    status_message: Option<(String, Instant)>,
    rate_history: VecDeque<f64>,
    db_write_history: VecDeque<f64>,
    chart_mode: ChartMode,
    log_entries: VecDeque<LogEntry>,
    log_scroll: usize,
    detail_scroll: usize,
    focused_panel: FocusedPanel,
    prev_is_bulk_sync: Option<bool>,
    prev_is_syncing: Option<bool>,
    prev_task_ids: Vec<uuid::Uuid>,
    prev_running_task_ids: Vec<uuid::Uuid>,
    prev_indexes_deferred: Option<bool>,
    sync_tab: SyncTab,
    chain_info: Option<ChainInfoData>,
    #[allow(dead_code)]
    picker: Option<Picker>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ChartMode {
    #[default]
    SyncRate,
    DbWrite,
}

impl App {
    pub fn new(db: TaskDb, picker: Option<Picker>) -> Self {
        let mut log_entries = VecDeque::with_capacity(LOG_HISTORY_SIZE);
        log_entries.push_back(LogEntry {
            timestamp: Local::now(),
            message: "Task Manager started".to_string(),
            level: LogLevel::Info,
        });

        Self {
            db,
            tasks: Vec::new(),
            sync_status: None,
            memory_stats: None,
            table_state: TableState::default(),
            last_refresh: Instant::now(),
            last_sample: Instant::now(),
            dialog: None,
            dialog_selection: 0,
            status_message: None,
            rate_history: VecDeque::with_capacity(RATE_HISTORY_SIZE),
            db_write_history: VecDeque::with_capacity(RATE_HISTORY_SIZE),
            chart_mode: ChartMode::default(),
            log_entries,
            log_scroll: 0,
            detail_scroll: 0,
            focused_panel: FocusedPanel::default(),
            prev_is_bulk_sync: None,
            prev_is_syncing: None,
            prev_task_ids: Vec::new(),
            prev_running_task_ids: Vec::new(),
            prev_indexes_deferred: None,
            sync_tab: SyncTab::default(),
            chain_info: None,
            picker,
        }
    }

    pub fn toggle_focus(&mut self) {
        self.focused_panel = match self.focused_panel {
            FocusedPanel::Tasks => FocusedPanel::Details,
            FocusedPanel::Details => FocusedPanel::Log,
            FocusedPanel::Log => FocusedPanel::Tasks,
        };
    }

    pub fn toggle_sync_tab(&mut self) {
        self.sync_tab = match self.sync_tab {
            SyncTab::ChainInfo => SyncTab::SyncProgress,
            SyncTab::SyncProgress => SyncTab::ChainInfo,
        };
    }

    pub fn focused_panel(&self) -> FocusedPanel {
        self.focused_panel
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

    pub fn scroll_detail_up(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_sub(1);
    }

    pub fn scroll_detail_down(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_add(1);
    }

    #[allow(dead_code)]
    pub fn reset_detail_scroll(&mut self) {
        self.detail_scroll = 0;
    }

    pub async fn refresh(&mut self) -> Result<()> {
        self.tasks = self.db.list_tasks(100).await?;
        self.sync_status = self.db.get_sync_status().await.ok();
        self.memory_stats = self.db.get_memory_stats().await;
        self.chain_info = self.db.get_chain_info().await;
        self.last_refresh = Instant::now();

        self.detect_events();

        if self.last_sample.elapsed().as_secs() >= 1 {
            self.sample_rate();
            self.last_sample = Instant::now();
        }

        if self.table_state.selected().is_none() && !self.tasks.is_empty() {
            self.table_state.select(Some(0));
        }
        Ok(())
    }

    fn detect_events(&mut self) {
        if let Some(sync) = &self.sync_status {
            if let Some(prev_bulk) = self.prev_is_bulk_sync {
                if prev_bulk && !sync.is_bulk_sync {
                    self.log_entries.push_back(LogEntry {
                        timestamp: Local::now(),
                        message: "Bulk sync completed".to_string(),
                        level: LogLevel::Success,
                    });
                } else if !prev_bulk && sync.is_bulk_sync {
                    self.log_entries.push_back(LogEntry {
                        timestamp: Local::now(),
                        message: "Bulk sync started".to_string(),
                        level: LogLevel::Info,
                    });
                }
            }
            self.prev_is_bulk_sync = Some(sync.is_bulk_sync);

            if let Some(prev_syncing) = self.prev_is_syncing {
                if prev_syncing && !sync.is_syncing {
                    self.log_entries.push_back(LogEntry {
                        timestamp: Local::now(),
                        message: "Sync completed - now in real-time mode".to_string(),
                        level: LogLevel::Success,
                    });
                } else if !prev_syncing && sync.is_syncing {
                    self.log_entries.push_back(LogEntry {
                        timestamp: Local::now(),
                        message: "Syncing started".to_string(),
                        level: LogLevel::Info,
                    });
                }
            }
            self.prev_is_syncing = Some(sync.is_syncing);

            if let Some(prev_deferred) = self.prev_indexes_deferred {
                if prev_deferred && !sync.indexes_deferred {
                    self.log_entries.push_back(LogEntry {
                        timestamp: Local::now(),
                        message: "Index rebuild completed".to_string(),
                        level: LogLevel::Success,
                    });
                } else if !prev_deferred && sync.indexes_deferred {
                    self.log_entries.push_back(LogEntry {
                        timestamp: Local::now(),
                        message: "Indexes deferred for bulk sync".to_string(),
                        level: LogLevel::Warning,
                    });
                }
            }
            self.prev_indexes_deferred = Some(sync.indexes_deferred);
        }

        let current_task_ids: Vec<uuid::Uuid> = self.tasks.iter().map(|t| t.id).collect();
        for task in &self.tasks {
            if !self.prev_task_ids.contains(&task.id) {
                self.log_entries.push_back(LogEntry {
                    timestamp: Local::now(),
                    message: format!("New task created: {}", task.task_type),
                    level: LogLevel::Info,
                });
            }
        }

        let current_running: Vec<uuid::Uuid> = self
            .tasks
            .iter()
            .filter(|t| t.status == "running")
            .map(|t| t.id)
            .collect();
        for task in &self.tasks {
            if task.status == "running" && !self.prev_running_task_ids.contains(&task.id) {
                self.log_entries.push_back(LogEntry {
                    timestamp: Local::now(),
                    message: format!("Task started: {}", task.task_type),
                    level: LogLevel::Info,
                });
            }
        }
        for prev_id in &self.prev_running_task_ids {
            if !current_running.contains(prev_id) {
                if let Some(task) = self.tasks.iter().find(|t| &t.id == prev_id) {
                    let msg = match task.status.as_str() {
                        "completed" => format!("Task completed: {}", task.task_type),
                        "failed" => format!("Task failed: {}", task.task_type),
                        "cancelled" => format!("Task cancelled: {}", task.task_type),
                        "paused" => format!("Task paused: {}", task.task_type),
                        _ => format!("Task stopped: {}", task.task_type),
                    };
                    let level = match task.status.as_str() {
                        "completed" => LogLevel::Success,
                        "failed" | "cancelled" => LogLevel::Warning,
                        _ => LogLevel::Info,
                    };
                    self.log_entries.push_back(LogEntry {
                        timestamp: Local::now(),
                        message: msg,
                        level,
                    });
                }
            }
        }

        self.prev_task_ids = current_task_ids;
        self.prev_running_task_ids = current_running;

        while self.log_entries.len() > LOG_HISTORY_SIZE {
            self.log_entries.pop_front();
        }
    }

    fn sample_rate(&mut self) {
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

    pub fn toggle_chart_mode(&mut self) {
        self.chart_mode = match self.chart_mode {
            ChartMode::SyncRate => ChartMode::DbWrite,
            ChartMode::DbWrite => ChartMode::SyncRate,
        };
    }

    pub fn should_refresh(&self) -> bool {
        self.last_refresh.elapsed().as_secs() >= 2
    }

    pub fn next(&mut self) {
        if self.dialog.is_some() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i >= self.tasks.len().saturating_sub(1) {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    pub fn previous(&mut self) {
        if self.dialog.is_some() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.tasks.len().saturating_sub(1)
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    pub fn selected_task(&self) -> Option<&Task> {
        self.table_state.selected().and_then(|i| self.tasks.get(i))
    }

    pub fn show_new_task_dialog(&mut self) {
        self.dialog = Some(DialogType::NewTask);
        self.dialog_selection = 0;
    }

    pub fn cancel_dialog(&mut self) {
        self.dialog = None;
        self.dialog_selection = 0;
    }

    pub fn next_dialog_option(&mut self) {
        if let Some(DialogType::NewTask) = self.dialog {
            self.dialog_selection = 0;
        }
    }

    pub fn previous_dialog_option(&mut self) {
        if let Some(DialogType::NewTask) = self.dialog {
            self.dialog_selection = 0;
        }
    }

    pub fn has_dialog(&self) -> bool {
        self.dialog.is_some()
    }

    pub async fn confirm_dialog(&mut self) -> Result<()> {
        match self.dialog {
            Some(DialogType::NewTask) => {
                let builder = match self.dialog_selection {
                    0 => TaskBuilder::label_import(LabelImportConfig::default()),
                    _ => return Ok(()),
                };
                let id = self.db.create_task(&builder).await?;
                self.status_message = Some((format!("Created task: {}", id), Instant::now()));
                self.dialog = None;
                self.refresh().await?;
            }
            Some(DialogType::Confirm(_)) => {
                self.dialog = None;
            }
            None => {}
        }
        Ok(())
    }

    pub async fn cancel_selected(&mut self) -> Result<()> {
        if let Some(task) = self.selected_task() {
            let id = task.id;
            if self.db.cancel_task(id).await? {
                self.status_message = Some((format!("Cancelled task: {}", id), Instant::now()));
                self.refresh().await?;
            }
        }
        Ok(())
    }

    pub async fn pause_selected(&mut self) -> Result<()> {
        if let Some(task) = self.selected_task() {
            let id = task.id;
            if self.db.pause_task(id).await? {
                self.status_message = Some((format!("Paused task: {}", id), Instant::now()));
                self.refresh().await?;
            }
        }
        Ok(())
    }

    pub async fn resume_or_retry_selected(&mut self) -> Result<()> {
        if let Some(task) = self.selected_task() {
            let id = task.id;
            let status = task.status.as_str();
            if status == "paused" {
                if self.db.resume_task(id).await? {
                    self.status_message = Some((format!("Resumed task: {}", id), Instant::now()));
                    self.refresh().await?;
                }
            } else if status == "failed" && self.db.retry_task(id).await? {
                self.status_message = Some((format!("Retrying task: {}", id), Instant::now()));
                self.refresh().await?;
            }
        }
        Ok(())
    }

    pub async fn delete_selected(&mut self) -> Result<()> {
        if let Some(task) = self.selected_task() {
            let id = task.id;
            if self.db.delete_task(id).await? {
                self.status_message = Some((format!("Deleted task: {}", id), Instant::now()));
                self.refresh().await?;
            }
        }
        Ok(())
    }
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(7),
            Constraint::Length(8),
            Constraint::Length(9),
            Constraint::Min(8),
            Constraint::Length(10),
            Constraint::Length(3),
        ])
        .split(f.area());

    draw_header(f, app, chunks[0]);
    draw_sync_status_full(f, app, chunks[1]);
    draw_rate_chart_full(f, app, chunks[2]);
    draw_memory_stats(f, app, chunks[3]);
    draw_tasks_and_details(f, app, chunks[4]);
    draw_log(f, app, chunks[5]);
    draw_footer(f, app, chunks[6]);

    if let Some(dialog) = &app.dialog {
        draw_dialog(f, app, dialog);
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
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner);

    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            "CKBadger",
            Style::default()
                .fg(TERMINAL_GREEN)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Dashboard", Style::default().fg(FOREGROUND)),
    ]));
    f.render_widget(title, cols[0]);

    let now = Local::now();
    let elapsed_ms = app.last_refresh.elapsed().as_millis();
    let time_info = Line::from(vec![
        Span::styled(
            format!("{}ms ago", elapsed_ms),
            Style::default().fg(if elapsed_ms > 2000 { AMBER } else { SLATE_500 }),
        ),
        Span::styled(" │ ", Style::default().fg(SLATE_700)),
        Span::styled(
            now.format("%H:%M:%S").to_string(),
            Style::default().fg(FOREGROUND),
        ),
    ]);
    let time_widget = Paragraph::new(time_info).alignment(Alignment::Right);
    f.render_widget(time_widget, cols[1]);
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

    // Left column: block/epoch
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
            Span::styled("Epoch:        ", Style::default().fg(SLATE_500)),
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
            Span::styled("Epoch:        ", Style::default().fg(SLATE_500)),
            Span::styled("-", Style::default().fg(SLATE_500)),
        ]));
    }
    f.render_widget(Paragraph::new(left_lines), cols[0]);

    // Middle column: difficulty/hashrate/block time
    let mid_lines = vec![
        Line::from(vec![
            Span::styled("Difficulty:     ", Style::default().fg(SLATE_500)),
            Span::styled(
                &info.difficulty,
                Style::default()
                    .fg(TERMINAL_GREEN)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Hash Rate:      ", Style::default().fg(SLATE_500)),
            Span::styled(&info.hash_rate, Style::default().fg(TERMINAL_GREEN)),
        ]),
        Line::from(vec![
            Span::styled("Avg Block Time: ", Style::default().fg(SLATE_500)),
            Span::styled(&info.avg_block_time, Style::default().fg(FOREGROUND)),
        ]),
    ];
    f.render_widget(Paragraph::new(mid_lines), cols[1]);

    // Right column: TPS/txns
    let right_lines = vec![
        Line::from(vec![
            Span::styled("TPS (24h):  ", Style::default().fg(SLATE_500)),
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
            Constraint::Length(20),
            Constraint::Min(30),
            Constraint::Length(25),
        ])
        .split(inner);

    let (mode, mode_color) = if !sync.is_syncing {
        ("SYNCED", TERMINAL_GREEN)
    } else if sync.is_bulk_sync {
        ("BULK SYNC", AMBER)
    } else {
        ("SYNCING", TERMINAL_GREEN)
    };

    // Build deferred index tags (only shown when indexes are deferred)
    let mut deferred_tags: Vec<Span> = Vec::new();
    if sync.address_balances_deferred {
        deferred_tags.push(Span::styled("[BAL]", Style::default().fg(AMBER)));
    }
    if sync.token_deferred {
        deferred_tags.push(Span::styled(" [TOK]", Style::default().fg(AMBER)));
    }
    if sync.spore_deferred {
        deferred_tags.push(Span::styled(" [SPR]", Style::default().fg(AMBER)));
    }
    if sync.tx_block_map_deferred {
        deferred_tags.push(Span::styled(" [TXM]", Style::default().fg(AMBER)));
    }

    let mut left_lines = vec![Line::from(vec![Span::styled(
        format!(" {} ", mode),
        Style::default().fg(Color::Black).bg(mode_color),
    )])];
    if !deferred_tags.is_empty() {
        left_lines.push(Line::from(deferred_tags));
    }
    left_lines.push(Line::from(vec![
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
    left_lines.push(Line::from(Span::styled(
        bar,
        Style::default().fg(TERMINAL_GREEN),
    )));

    f.render_widget(Paragraph::new(left_lines), cols[0]);

    let blocks_behind = sync.chain_tip - sync.tip_block;
    let mid_lines = vec![
        Line::from(vec![
            Span::styled("Current Block: ", Style::default().fg(SLATE_500)),
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
            Span::styled("Blocks Behind: ", Style::default().fg(SLATE_500)),
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
            Span::styled("Speed (now):   ", Style::default().fg(SLATE_500)),
            if let Some(rt) = sync.rate_realtime {
                Span::styled(
                    format!("{:.0} blk/s", rt),
                    Style::default()
                        .fg(TERMINAL_GREEN)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled("-", Style::default().fg(SLATE_500))
            },
        ]),
        Line::from(vec![
            Span::styled("Speed (EMA):   ", Style::default().fg(SLATE_500)),
            if let Some(ema) = sync.rate_ema {
                Span::raw(format!("{:.0} blk/s", ema))
            } else {
                Span::styled("-", Style::default().fg(SLATE_500))
            },
        ]),
    ];
    f.render_widget(Paragraph::new(mid_lines), cols[1]);

    let mut right_lines = Vec::new();
    if let Some(ref eta) = sync.eta {
        right_lines.push(Line::from(vec![
            Span::styled("ETA: ", Style::default().fg(SLATE_500)),
            Span::styled(
                eta.clone(),
                Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    if let Some(ref elapsed) = sync.elapsed_time {
        right_lines.push(Line::from(vec![
            Span::styled("Elapsed: ", Style::default().fg(SLATE_500)),
            Span::raw(elapsed.clone()),
        ]));
    }
    // DB/RPC batch performance
    if let Some(db_ms) = sync.db_write_ms {
        let db_color = if db_ms > 2000.0 {
            ERROR_RED
        } else if db_ms > 1000.0 {
            AMBER
        } else {
            TERMINAL_GREEN
        };
        right_lines.push(Line::from(vec![
            Span::styled("DB Write: ", Style::default().fg(SLATE_500)),
            Span::styled(format!("{:.0}ms", db_ms), Style::default().fg(db_color)),
            if let Some(rpc_ms) = sync.rpc_fetch_ms {
                Span::styled(
                    format!("  RPC: {:.0}ms", rpc_ms),
                    Style::default().fg(SLATE_500),
                )
            } else {
                Span::raw("")
            },
        ]));
    }

    if right_lines.is_empty() {
        right_lines.push(Line::from(Span::styled(
            "Real-time sync",
            Style::default().fg(TERMINAL_GREEN),
        )));
    }
    f.render_widget(Paragraph::new(right_lines), cols[2]);
}

fn draw_rate_chart_full(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SLATE_800))
        .title(chart_title(app));

    let history = match app.chart_mode {
        ChartMode::SyncRate => &app.rate_history,
        ChartMode::DbWrite => &app.db_write_history,
    };

    if history.is_empty() {
        let msg = Paragraph::new("Collecting data...")
            .style(Style::default().fg(SLATE_500))
            .block(block);
        f.render_widget(msg, area);
        return;
    }

    let inner = block.inner(area);
    f.render_widget(block, area);

    let chart_result = render_bar_chart(history, inner.width as usize, inner.height as usize);

    for (i, row) in chart_result.rows.iter().enumerate() {
        if i < inner.height as usize {
            let y = inner.y + i as u16;
            let span = Span::styled(row.content.clone(), Style::default().fg(row.color));
            let paragraph = Paragraph::new(Line::from(span));
            let line_area = Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: 1,
            };
            f.render_widget(paragraph, line_area);
        }
    }

    let samples = history.len();
    let duration = if samples >= 3600 {
        "1h".to_string()
    } else if samples >= 60 {
        format!("{}m", samples / 60)
    } else {
        format!("{}s", samples)
    };

    let axis_label = Span::styled(format!(" {} ", duration), Style::default().fg(SLATE_500));
    let label_para = Paragraph::new(Line::from(axis_label)).alignment(Alignment::Right);
    let label_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: 1,
    };
    f.render_widget(label_para, label_area);
}

fn draw_tasks_and_details(f: &mut Frame, app: &mut App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    draw_task_table(f, app, cols[0]);
    draw_task_detail(f, app, cols[1]);
}

#[allow(dead_code)]
fn draw_sync_and_chart(f: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    draw_sync_status_compact(f, app, cols[0]);
    draw_rate_chart_compact(f, app, cols[1]);
}

#[allow(dead_code)]
fn draw_sync_status_compact(f: &mut Frame, app: &App, area: Rect) {
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

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(4),
            Constraint::Length(1),
        ])
        .split(inner);

    let (mode, mode_color) = if !sync.is_syncing {
        ("SYNCED", TERMINAL_GREEN)
    } else if sync.is_bulk_sync {
        ("BULK SYNC", AMBER)
    } else {
        ("SYNCING", TERMINAL_GREEN)
    };

    let idx_status = if sync.indexes_deferred { " [IDX]" } else { "" };

    let status_line = Line::from(vec![
        Span::styled(
            format!(" {} ", mode),
            Style::default().fg(Color::Black).bg(mode_color),
        ),
        Span::raw(format!(" {:.2}%{}", sync.progress, idx_status)),
    ]);
    f.render_widget(Paragraph::new(status_line), rows[0]);

    let format_rate = |rate: f64| -> String {
        if rate >= 1000.0 {
            format!("{:.1}K", rate / 1000.0)
        } else {
            format!("{:.0}", rate)
        }
    };

    let mut info_lines = Vec::new();
    info_lines.push(Line::from(vec![
        Span::styled("Block: ", Style::default().fg(SLATE_600)),
        Span::raw(format!("{}/{}", sync.tip_block, sync.chain_tip)),
    ]));

    if let Some(realtime) = sync.rate_realtime {
        if realtime > 0.0 {
            info_lines.push(Line::from(vec![
                Span::styled("Speed: ", Style::default().fg(SLATE_600)),
                Span::styled(
                    format!("{} blk/s", format_rate(realtime)),
                    Style::default()
                        .fg(TERMINAL_GREEN)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }
    }

    if let Some(ref eta) = sync.eta {
        info_lines.push(Line::from(vec![
            Span::styled("ETA:   ", Style::default().fg(SLATE_600)),
            Span::styled(eta.clone(), Style::default().fg(AMBER)),
        ]));
    } else if let Some(ref elapsed) = sync.elapsed_time {
        info_lines.push(Line::from(vec![
            Span::styled("Time:  ", Style::default().fg(SLATE_600)),
            Span::raw(elapsed.clone()),
        ]));
    }

    if let Some(ema) = sync.rate_ema {
        if ema > 0.0 {
            info_lines.push(Line::from(vec![
                Span::styled("EMA:   ", Style::default().fg(SLATE_600)),
                Span::raw(format!("{} blk/s", format_rate(ema))),
            ]));
        }
    }

    f.render_widget(Paragraph::new(info_lines), rows[1]);

    let ratio = (sync.progress / 100.0).clamp(0.0, 1.0);
    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(TERMINAL_GREEN).bg(SLATE_800))
        .ratio(ratio)
        .label(format!("{:.2}%", sync.progress))
        .use_unicode(true);
    f.render_widget(gauge, rows[2]);
}

#[allow(dead_code)]
fn draw_rate_chart_compact(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SLATE_800))
        .title(chart_title(app));

    let history = match app.chart_mode {
        ChartMode::SyncRate => &app.rate_history,
        ChartMode::DbWrite => &app.db_write_history,
    };

    if history.is_empty() {
        let msg = Paragraph::new("Collecting data...")
            .style(Style::default().fg(SLATE_500))
            .block(block);
        f.render_widget(msg, area);
        return;
    }

    let inner = block.inner(area);
    f.render_widget(block, area);

    let chart_result = render_bar_chart(history, inner.width as usize, inner.height as usize);

    for (i, row) in chart_result.rows.iter().enumerate() {
        if i < inner.height as usize {
            let y = inner.y + i as u16;
            let span = Span::styled(row.content.clone(), Style::default().fg(row.color));
            let paragraph = Paragraph::new(Line::from(span));
            let line_area = Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: 1,
            };
            f.render_widget(paragraph, line_area);
        }
    }

    let samples = history.len();
    let duration = if samples >= 3600 {
        "1h".to_string()
    } else if samples >= 60 {
        format!("{}m", samples / 60)
    } else {
        format!("{}s", samples)
    };

    let axis_label = Span::styled(format!(" {} ", duration), Style::default().fg(SLATE_500));
    let label_para = Paragraph::new(Line::from(axis_label)).alignment(Alignment::Right);
    let label_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: 1,
    };
    f.render_widget(label_para, label_area);
}

fn draw_memory_stats(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SLATE_800))
        .title(Span::styled(
            "Database & Storage",
            Style::default().fg(FOREGROUND),
        ));

    let Some(mem) = &app.memory_stats else {
        let msg = Paragraph::new("No memory data available").block(block);
        f.render_widget(msg, area);
        return;
    };

    let inner = block.inner(area);
    f.render_widget(block, area);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(34),
            Constraint::Percentage(33),
        ])
        .split(inner);

    let bulk_indicator = if mem.bulk_sync_mode {
        Span::styled(" [BULK]", Style::default().fg(AMBER))
    } else {
        Span::raw("")
    };

    // Left column: RocksDB memory breakdown
    let mut left_lines = vec![
        Line::from(vec![
            Span::styled(
                format!("{:>14}", "RocksDB:"),
                Style::default().fg(SLATE_600),
            ),
            Span::styled(
                format!(" {:>10}", format_bytes(mem.rocksdb_total_bytes)),
                Style::default()
                    .fg(TERMINAL_GREEN)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!("{:>14}", "Memtable:"),
                Style::default().fg(SLATE_600),
            ),
            Span::raw(format!(" {:>10}", format_bytes(mem.rocksdb_memtable_bytes))),
        ]),
        Line::from(vec![
            Span::styled(
                format!("{:>14}", "Block Cache:"),
                Style::default().fg(SLATE_600),
            ),
            Span::raw(format!(
                " {:>10}",
                format_bytes(mem.rocksdb_block_cache_bytes)
            )),
        ]),
        Line::from(vec![
            Span::styled(
                format!("{:>14}", "Table Read:"),
                Style::default().fg(SLATE_600),
            ),
            Span::raw(format!(
                " {:>10}",
                format_bytes(mem.rocksdb_table_readers_bytes)
            )),
        ]),
    ];
    // Disk usage
    if mem.sst_files_size > 0 {
        left_lines.push(Line::from(vec![
            Span::styled(
                format!("{:>14}", "Disk (SST):"),
                Style::default().fg(SLATE_600),
            ),
            Span::styled(
                format!(" {:>10}", format_bytes(mem.sst_files_size)),
                Style::default().fg(AMBER_DIM),
            ),
        ]));
    }
    // Compaction status
    if mem.compaction_pending_bytes > 0 || mem.num_running_compactions > 0 {
        left_lines.push(Line::from(vec![
            Span::styled(
                format!("{:>14}", "Compaction:"),
                Style::default().fg(SLATE_600),
            ),
            Span::styled(
                format!(
                    " {} run, {} pend",
                    mem.num_running_compactions,
                    format_bytes(mem.compaction_pending_bytes)
                ),
                Style::default().fg(AMBER),
            ),
        ]));
    }
    f.render_widget(Paragraph::new(left_lines), cols[0]);

    // Middle column: Chain statistics
    let mut mid_lines = vec![
        Line::from(vec![
            Span::styled(
                format!("{:>14}", "Transactions:"),
                Style::default().fg(SLATE_600),
            ),
            Span::styled(
                format!(" {:>10}", format_count_i64(mem.total_transactions)),
                Style::default()
                    .fg(TERMINAL_GREEN)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!("{:>14}", "Total Cells:"),
                Style::default().fg(SLATE_600),
            ),
            Span::raw(format!(" {:>10}", format_count_i64(mem.total_cells))),
        ]),
        Line::from(vec![
            Span::styled(
                format!("{:>14}", "Live Cells:"),
                Style::default().fg(SLATE_600),
            ),
            Span::styled(
                format!(" {:>10}", format_count_i64(mem.total_live_cells)),
                Style::default()
                    .fg(TERMINAL_GREEN)
                    .add_modifier(Modifier::BOLD),
            ),
            bulk_indicator,
        ]),
    ];
    if mem.total_addresses > 0 {
        mid_lines.push(Line::from(vec![
            Span::styled(
                format!("{:>14}", "Addresses:"),
                Style::default().fg(SLATE_600),
            ),
            Span::raw(format!(" {:>10}", format_count_i64(mem.total_addresses))),
        ]));
    }
    f.render_widget(Paragraph::new(mid_lines), cols[1]);

    // Right column: Top column families by size
    let mut right_lines = vec![Line::from(Span::styled(
        " Top Column Families",
        Style::default().fg(FOREGROUND).add_modifier(Modifier::BOLD),
    ))];
    if mem.top_cf_sizes.is_empty() {
        right_lines.push(Line::from(Span::styled(
            "   (no data)",
            Style::default().fg(SLATE_500),
        )));
    } else {
        for (name, size) in &mem.top_cf_sizes {
            right_lines.push(Line::from(vec![
                Span::styled(format!("  {:<18}", name), Style::default().fg(SLATE_600)),
                Span::styled(
                    format!("{:>10}", format_bytes(*size)),
                    Style::default().fg(TERMINAL_GREEN),
                ),
            ]));
        }
    }
    f.render_widget(Paragraph::new(right_lines), cols[2]);
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GB", bytes as f64 / 1024.0 / 1024.0 / 1024.0)
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / 1024.0 / 1024.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

#[allow(dead_code)]
fn format_count(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.2}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}K", count as f64 / 1_000.0)
    } else {
        format!("{}", count)
    }
}

fn format_num_commas(n: i64) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, &b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(b as char);
    }
    result
}

fn format_num(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.2}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        format!("{}", n)
    }
}

fn format_count_i64(count: i64) -> String {
    let abs = count.unsigned_abs();
    let prefix = if count < 0 { "-" } else { "" };
    if abs >= 1_000_000 {
        format!("{}{:.2}M", prefix, abs as f64 / 1_000_000.0)
    } else if abs >= 1_000 {
        format!("{}{:.1}K", prefix, abs as f64 / 1_000.0)
    } else {
        format!("{}{}", prefix, abs)
    }
}

fn format_rate(rate: f64) -> String {
    if rate >= 1000.0 {
        format!("{:.1}K", rate / 1000.0)
    } else {
        format!("{:.0}", rate)
    }
}

fn chart_title(app: &App) -> Line<'static> {
    let (history, label, unit) = match app.chart_mode {
        ChartMode::SyncRate => (&app.rate_history, "Sync Rate", "blk/s"),
        ChartMode::DbWrite => (&app.db_write_history, "DB Write", "ms"),
    };
    let stats = ChartStats::from_history(history);
    let mode_hint = Span::styled(" [v] ", Style::default().fg(SLATE_700));
    match stats {
        Some(s) => {
            let fmt = |v: f64| -> String {
                if unit == "ms" {
                    format!("{:.0}{}", v, unit)
                } else {
                    format!("{} {}", format_rate(v), unit)
                }
            };
            Line::from(vec![
                Span::raw(format!("{} ", label)),
                mode_hint,
                Span::styled(
                    format!("now:{}", fmt(s.current)),
                    Style::default().fg(TERMINAL_GREEN),
                ),
                Span::styled(" │ ", Style::default().fg(SLATE_700)),
                Span::styled(
                    format!("min:{}", fmt(s.min)),
                    Style::default().fg(ERROR_RED),
                ),
                Span::styled(" │ ", Style::default().fg(SLATE_700)),
                Span::styled(
                    format!("avg:{}", fmt(s.avg)),
                    Style::default().fg(TERMINAL_GREEN),
                ),
                Span::styled(" │ ", Style::default().fg(SLATE_700)),
                Span::styled(format!("max:{}", fmt(s.max)), Style::default().fg(AMBER)),
            ])
        }
        None => Line::from(format!("{} (collecting...)", label)),
    }
}

fn draw_task_table(f: &mut Frame, app: &mut App, area: Rect) {
    let is_focused = app.focused_panel == FocusedPanel::Tasks;
    let border_color = if is_focused {
        TERMINAL_GREEN
    } else {
        SLATE_800
    };

    let header_cells = ["Type", "Status", "Progress", "ETA"]
        .iter()
        .map(|h| Cell::from(*h).style(Style::default().fg(AMBER)));
    let header = Row::new(header_cells).style(Style::default()).height(1);

    let rows = app.tasks.iter().map(|task| {
        let task_type = task.task_type.clone();
        let progress = format!("{:.1}%", task.progress_percent().min(100.0));
        let eta = task.eta_formatted().unwrap_or_default();

        let (status_icon, status_text, status_style) = match task.status.as_str() {
            "running" => ("▶ ", "running", Style::default().fg(TERMINAL_GREEN)),
            "pending" => ("◌ ", "pending", Style::default().fg(AMBER)),
            "completed" => ("✓ ", "done", Style::default().fg(CKB_PRIMARY)),
            "failed" => ("✗ ", "failed", Style::default().fg(ERROR_RED)),
            "cancelled" => ("○ ", "cancel", Style::default().fg(SLATE_500)),
            "paused" => ("⏸ ", "paused", Style::default().fg(AMBER_DIM)),
            _ => ("  ", task.status.as_str(), Style::default()),
        };

        Row::new(vec![
            Cell::from(task_type),
            Cell::from(format!("{}{}", status_icon, status_text)).style(status_style),
            Cell::from(progress),
            Cell::from(eta),
        ])
    });

    let task_count = app.tasks.len();
    let running_count = app.tasks.iter().filter(|t| t.status == "running").count();
    let title = if running_count > 0 {
        format!("Tasks ({} total, {} running)", task_count, running_count)
    } else {
        format!("Tasks ({})", task_count)
    };

    let table = Table::new(
        rows,
        [
            Constraint::Min(16),
            Constraint::Length(12),
            Constraint::Length(10),
            Constraint::Length(10),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(
                title,
                Style::default().fg(if is_focused {
                    TERMINAL_GREEN
                } else {
                    FOREGROUND
                }),
            )),
    )
    .row_highlight_style(Style::default().bg(SLATE_800));

    f.render_stateful_widget(table, area, &mut app.table_state);
}

fn draw_task_detail(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.focused_panel == FocusedPanel::Details;
    let border_color = if is_focused {
        TERMINAL_GREEN
    } else {
        SLATE_800
    };

    let (lines, total_lines) = if let Some(task) = app.selected_task() {
        let (status_icon, status_color) = match task.status.as_str() {
            "running" => ("▶", TERMINAL_GREEN),
            "pending" => ("◌", AMBER),
            "completed" => ("✓", CKB_PRIMARY),
            "failed" => ("✗", ERROR_RED),
            "cancelled" => ("○", SLATE_500),
            "paused" => ("⏸", AMBER_DIM),
            _ => (" ", FOREGROUND),
        };

        let mut lines = vec![
            Line::from(vec![
                Span::styled("ID:      ", Style::default().fg(SLATE_500)),
                Span::styled(task.id.to_string(), Style::default().fg(TERMINAL_GREEN)),
            ]),
            Line::from(vec![
                Span::styled("Status:  ", Style::default().fg(SLATE_500)),
                Span::styled(
                    format!("{} {}", status_icon, task.status),
                    Style::default().fg(status_color),
                ),
            ]),
            Line::from(vec![
                Span::styled("Created: ", Style::default().fg(SLATE_500)),
                Span::raw(task.created_at.format("%Y-%m-%d %H:%M:%S").to_string()),
            ]),
        ];

        if let Some(rate) = task.rate_ema {
            lines.push(Line::from(vec![
                Span::styled("Rate:    ", Style::default().fg(SLATE_500)),
                Span::styled(
                    format!("{:.1}/s", rate),
                    Style::default()
                        .fg(TERMINAL_GREEN)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }

        if let Some(elapsed) = task.elapsed_formatted() {
            lines.push(Line::from(vec![
                Span::styled("Elapsed: ", Style::default().fg(SLATE_500)),
                Span::raw(elapsed),
            ]));
        }

        if let Some(eta) = task.eta_formatted() {
            lines.push(Line::from(vec![
                Span::styled("ETA:     ", Style::default().fg(SLATE_500)),
                Span::styled(eta, Style::default().fg(AMBER)),
            ]));
        }

        lines.push(Line::from(""));

        if let Some(msg) = &task.progress_message {
            lines.push(Line::from(vec![
                Span::styled("Progress: ", Style::default().fg(SLATE_500)),
                Span::raw(msg),
            ]));
        }

        if let Some(err) = &task.error_message {
            lines.push(Line::from(vec![
                Span::styled("Error: ", Style::default().fg(ERROR_RED)),
                Span::raw(err),
            ]));
        }

        if let Some(runner) = &task.runner_id {
            lines.push(Line::from(vec![
                Span::styled("Runner:  ", Style::default().fg(SLATE_500)),
                Span::raw(runner),
            ]));
        }

        let total = lines.len();
        (lines, total)
    } else {
        let lines = vec![Line::from(Span::styled(
            "No task selected",
            Style::default().fg(SLATE_500),
        ))];
        (lines, 1)
    };

    let scroll_indicator = if app.detail_scroll > 0 {
        format!(" ↑{}", app.detail_scroll)
    } else {
        String::new()
    };

    let title = format!("Details{}", scroll_indicator);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(
            title,
            Style::default().fg(if is_focused {
                TERMINAL_GREEN
            } else {
                FOREGROUND
            }),
        ));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let visible_height = inner.height as usize;
    let scroll = app
        .detail_scroll
        .min(total_lines.saturating_sub(visible_height));

    let visible_lines: Vec<Line> = lines
        .into_iter()
        .skip(scroll)
        .take(visible_height)
        .collect();

    let paragraph = Paragraph::new(visible_lines).wrap(Wrap { trim: true });
    f.render_widget(paragraph, inner);
}

fn draw_log(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.focused_panel == FocusedPanel::Log;
    let border_color = if is_focused {
        TERMINAL_GREEN
    } else {
        SLATE_800
    };

    let scroll_indicator = if app.log_scroll > 0 {
        format!(" ↑{}", app.log_scroll)
    } else {
        String::new()
    };

    let title = format!("Event Log ({}){}", app.log_entries.len(), scroll_indicator);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(
            title,
            Style::default().fg(if is_focused {
                TERMINAL_GREEN
            } else {
                FOREGROUND
            }),
        ));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let visible_lines = inner.height as usize;
    let total_entries = app.log_entries.len();

    let end_idx = total_entries.saturating_sub(app.log_scroll);
    let start_idx = end_idx.saturating_sub(visible_lines);

    let entries: Vec<&LogEntry> = app
        .log_entries
        .iter()
        .skip(start_idx)
        .take(end_idx - start_idx)
        .collect();

    let lines: Vec<Line> = entries
        .iter()
        .map(|entry| {
            Line::from(vec![
                Span::styled(
                    entry.timestamp.format("%H:%M:%S").to_string(),
                    Style::default().fg(SLATE_500),
                ),
                Span::styled(
                    format!(" [{}] ", entry.level.prefix()),
                    Style::default().fg(entry.level.color()),
                ),
                Span::raw(&entry.message),
            ])
        })
        .collect();

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, inner);
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let help = if app.dialog.is_some() {
        "Enter: Confirm │ Esc: Cancel │ Tab: Next option".to_string()
    } else {
        match app.focused_panel {
            FocusedPanel::Tasks => {
                "Tab: Details │ j/k: Navigate │ n: New │ c: Cancel │ p: Pause │ r: Retry │ d: Del │ s: Sync │ v: Chart │ R: Refresh │ q: Quit".to_string()
            }
            FocusedPanel::Details => {
                "Tab: Log │ j/k: Scroll │ v: Chart │ R: Refresh │ q: Quit".to_string()
            }
            FocusedPanel::Log => {
                "Tab: Tasks │ j/k: Scroll │ g: Top │ G: Bottom │ v: Chart │ R: Refresh │ q: Quit".to_string()
            }
        }
    };

    let status = app
        .status_message
        .as_ref()
        .filter(|(_, t)| t.elapsed().as_secs() < 5)
        .map(|(s, _)| s.clone())
        .unwrap_or(help);

    let footer = Paragraph::new(status)
        .style(Style::default().fg(SLATE_500))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(SLATE_800)),
        );
    f.render_widget(footer, area);
}

fn draw_dialog(f: &mut Frame, app: &App, dialog: &DialogType) {
    let area = centered_rect(50, 40, f.area());
    f.render_widget(Clear, area);

    match dialog {
        DialogType::NewTask => {
            let options = [("Label Import", "Import UDT/script labels from token-labels")];
            let items: Vec<Line> = options
                .iter()
                .enumerate()
                .map(|(i, (name, desc))| {
                    if i == app.dialog_selection {
                        Line::from(vec![
                            Span::styled(
                                format!(" ▶ {}", name),
                                Style::default()
                                    .fg(TERMINAL_GREEN)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(format!("  {}", desc), Style::default().fg(SLATE_500)),
                        ])
                    } else {
                        Line::from(vec![
                            Span::styled(format!("   {}", name), Style::default().fg(FOREGROUND)),
                            Span::styled(format!("  {}", desc), Style::default().fg(SLATE_500)),
                        ])
                    }
                })
                .collect();

            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(TERMINAL_GREEN))
                .title(Span::styled(
                    " New Task ",
                    Style::default()
                        .fg(TERMINAL_GREEN)
                        .add_modifier(Modifier::BOLD),
                ));
            let dialog_widget = Paragraph::new(items).block(block);
            f.render_widget(dialog_widget, area);
        }
        DialogType::Confirm(msg) => {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(AMBER))
                .title(Span::styled(
                    " Confirm ",
                    Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
                ));
            let dialog_widget = Paragraph::new(*msg).block(block);
            f.render_widget(dialog_widget, area);
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes_gigabytes() {
        assert_eq!(format_bytes(1_073_741_824), "1.00 GB"); // 1 GB
        assert_eq!(format_bytes(1_610_612_736), "1.50 GB"); // 1.5 GB
        assert_eq!(format_bytes(10_737_418_240), "10.00 GB"); // 10 GB
    }

    #[test]
    fn test_format_bytes_megabytes() {
        assert_eq!(format_bytes(1_048_576), "1.0 MB"); // 1 MB
        assert_eq!(format_bytes(524_288_000), "500.0 MB"); // 500 MB
        assert_eq!(format_bytes(104_857_600), "100.0 MB"); // 100 MB
    }

    #[test]
    fn test_format_bytes_kilobytes() {
        assert_eq!(format_bytes(1_024), "1.0 KB"); // 1 KB
        assert_eq!(format_bytes(512_000), "500.0 KB"); // ~500 KB
        assert_eq!(format_bytes(102_400), "100.0 KB"); // 100 KB
    }

    #[test]
    fn test_format_bytes_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1), "1 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1023), "1023 B");
    }

    #[test]
    fn test_format_count_millions() {
        assert_eq!(format_count(1_000_000), "1.00M");
        assert_eq!(format_count(45_000_000), "45.00M");
        assert_eq!(format_count(1_234_567), "1.23M");
    }

    #[test]
    fn test_format_count_thousands() {
        assert_eq!(format_count(1_000), "1.0K");
        assert_eq!(format_count(45_500), "45.5K");
        assert_eq!(format_count(999_999), "1000.0K"); // Just under 1M
    }

    #[test]
    fn test_format_count_small() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(1), "1");
        assert_eq!(format_count(999), "999");
    }

    #[test]
    fn test_format_rate() {
        assert_eq!(format_rate(0.0), "0");
        assert_eq!(format_rate(100.0), "100");
        assert_eq!(format_rate(999.0), "999");
        assert_eq!(format_rate(1000.0), "1.0K");
        assert_eq!(format_rate(3465.0), "3.5K");
        assert_eq!(format_rate(10000.0), "10.0K");
    }
}
