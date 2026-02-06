use anyhow::Result;
use chrono::{DateTime, Local};
use ckbadger_common::{
    CyclesBackfillConfig, DotbitRebuildConfig, IndexRebuildConfig, LabelImportConfig,
    LiveCellsPopulateConfig, MemoryStatsData, MnftRebuildConfig, SecondaryIssuanceBackfillConfig,
    SporeRebuildConfig, StatisticsRebuildConfig, Task, TaskBuilder,
};
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
use crate::db::{SyncStatusRow, TaskDb};

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
            LogLevel::Info => Color::Cyan,
            LogLevel::Success => Color::Green,
            LogLevel::Warning => Color::Yellow,
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

const COLOR_BORDER: Color = Color::Rgb(88, 88, 88);
const COLOR_MUTED: Color = Color::Rgb(128, 128, 128);
const COLOR_SEPARATOR: Color = Color::Rgb(100, 100, 100);

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
    Log,
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
    status_message: Option<String>,
    rate_history: VecDeque<f64>,
    log_entries: VecDeque<LogEntry>,
    log_scroll: usize,
    focused_panel: FocusedPanel,
    prev_is_bulk_sync: Option<bool>,
    prev_is_syncing: Option<bool>,
    prev_task_ids: Vec<uuid::Uuid>,
    prev_running_task_ids: Vec<uuid::Uuid>,
    prev_indexes_deferred: Option<bool>,
    #[allow(dead_code)]
    picker: Option<Picker>,
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
            log_entries,
            log_scroll: 0,
            focused_panel: FocusedPanel::default(),
            prev_is_bulk_sync: None,
            prev_is_syncing: None,
            prev_task_ids: Vec::new(),
            prev_running_task_ids: Vec::new(),
            prev_indexes_deferred: None,
            picker,
        }
    }

    pub fn toggle_focus(&mut self) {
        self.focused_panel = match self.focused_panel {
            FocusedPanel::Tasks => FocusedPanel::Log,
            FocusedPanel::Log => FocusedPanel::Tasks,
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

    pub async fn refresh(&mut self) -> Result<()> {
        self.tasks = self.db.list_tasks(100).await?;
        self.sync_status = self.db.get_sync_status().await.ok();
        self.memory_stats = self.db.get_memory_stats().await;
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
            self.dialog_selection = (self.dialog_selection + 1) % 9;
        }
    }

    pub fn previous_dialog_option(&mut self) {
        if let Some(DialogType::NewTask) = self.dialog {
            self.dialog_selection = if self.dialog_selection == 0 {
                8
            } else {
                self.dialog_selection - 1
            };
        }
    }

    pub fn has_dialog(&self) -> bool {
        self.dialog.is_some()
    }

    pub async fn confirm_dialog(&mut self) -> Result<()> {
        match self.dialog {
            Some(DialogType::NewTask) => {
                let builder = match self.dialog_selection {
                    0 => TaskBuilder::cycles_backfill(CyclesBackfillConfig::default()),
                    1 => TaskBuilder::index_rebuild(IndexRebuildConfig::default()),
                    2 => TaskBuilder::label_import(LabelImportConfig::default()),
                    3 => TaskBuilder::statistics_rebuild(StatisticsRebuildConfig::default()),
                    4 => TaskBuilder::live_cells_populate(LiveCellsPopulateConfig::default()),
                    5 => TaskBuilder::spore_rebuild(SporeRebuildConfig::default()),
                    6 => TaskBuilder::secondary_issuance_backfill(
                        SecondaryIssuanceBackfillConfig::default(),
                    ),
                    7 => TaskBuilder::mnft_rebuild(MnftRebuildConfig::default()),
                    8 => TaskBuilder::dotbit_rebuild(DotbitRebuildConfig::default()),
                    _ => return Ok(()),
                };
                let id = self.db.create_task(&builder).await?;
                self.status_message = Some(format!("Created task: {}", id));
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
                self.status_message = Some(format!("Cancelled task: {}", id));
                self.refresh().await?;
            }
        }
        Ok(())
    }

    pub async fn pause_selected(&mut self) -> Result<()> {
        if let Some(task) = self.selected_task() {
            let id = task.id;
            if self.db.pause_task(id).await? {
                self.status_message = Some(format!("Paused task: {}", id));
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
                    self.status_message = Some(format!("Resumed task: {}", id));
                    self.refresh().await?;
                }
            } else if status == "failed" && self.db.retry_task(id).await? {
                self.status_message = Some(format!("Retrying task: {}", id));
                self.refresh().await?;
            }
        }
        Ok(())
    }

    pub async fn delete_selected(&mut self) -> Result<()> {
        if let Some(task) = self.selected_task() {
            let id = task.id;
            if self.db.delete_task(id).await? {
                self.status_message = Some(format!("Deleted task: {}", id));
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
            Constraint::Length(9),
            Constraint::Length(5),
            Constraint::Min(6),
            Constraint::Length(8),
            Constraint::Length(12),
            Constraint::Length(3),
        ])
        .split(f.area());

    draw_header(f, app, chunks[0]);
    draw_sync_and_chart(f, app, chunks[1]);
    draw_memory_stats(f, app, chunks[2]);
    draw_task_table(f, app, chunks[3]);
    draw_task_detail(f, app, chunks[4]);
    draw_log(f, app, chunks[5]);
    draw_footer(f, app, chunks[6]);

    if let Some(dialog) = &app.dialog {
        draw_dialog(f, app, dialog);
    }
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(COLOR_BORDER));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner);

    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            "ckbadger",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Task Manager", Style::default().fg(Color::White)),
    ]));
    f.render_widget(title, cols[0]);

    let now = Local::now();
    let elapsed_ms = app.last_refresh.elapsed().as_millis();
    let time_info = Line::from(vec![
        Span::styled(
            format!("{}ms ago", elapsed_ms),
            Style::default().fg(if elapsed_ms > 2000 {
                Color::Yellow
            } else {
                COLOR_MUTED
            }),
        ),
        Span::styled(" │ ", Style::default().fg(COLOR_SEPARATOR)),
        Span::styled(
            now.format("%H:%M:%S").to_string(),
            Style::default().fg(Color::White),
        ),
    ]);
    let time_widget = Paragraph::new(time_info).alignment(Alignment::Right);
    f.render_widget(time_widget, cols[1]);
}

fn draw_sync_and_chart(f: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    draw_sync_status_compact(f, app, cols[0]);
    draw_rate_chart_compact(f, app, cols[1]);
}

fn draw_sync_status_compact(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(COLOR_BORDER))
        .title(Span::styled(
            "Sync Status",
            Style::default().fg(Color::White),
        ));

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
        ("SYNCED", Color::Green)
    } else if sync.is_bulk_sync {
        ("BULK SYNC", Color::Yellow)
    } else {
        ("SYNCING", Color::Cyan)
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
        Span::styled("Block: ", Style::default().fg(Color::Gray)),
        Span::raw(format!("{}/{}", sync.tip_block, sync.chain_tip)),
    ]));

    if let Some(realtime) = sync.rate_realtime {
        if realtime > 0.0 {
            info_lines.push(Line::from(vec![
                Span::styled("Speed: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    format!("{} blk/s", format_rate(realtime)),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }
    }

    if let Some(ref eta) = sync.eta {
        info_lines.push(Line::from(vec![
            Span::styled("ETA:   ", Style::default().fg(Color::Gray)),
            Span::styled(eta.clone(), Style::default().fg(Color::Yellow)),
        ]));
    } else if let Some(ref elapsed) = sync.elapsed_time {
        info_lines.push(Line::from(vec![
            Span::styled("Time:  ", Style::default().fg(Color::Gray)),
            Span::raw(elapsed.clone()),
        ]));
    }

    if let Some(ema) = sync.rate_ema {
        if ema > 0.0 {
            info_lines.push(Line::from(vec![
                Span::styled("EMA:   ", Style::default().fg(Color::Gray)),
                Span::raw(format!("{} blk/s", format_rate(ema))),
            ]));
        }
    }

    f.render_widget(Paragraph::new(info_lines), rows[1]);

    let ratio = (sync.progress / 100.0).clamp(0.0, 1.0);
    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(Color::Cyan).bg(COLOR_BORDER))
        .ratio(ratio)
        .label(format!("{:.2}%", sync.progress))
        .use_unicode(true);
    f.render_widget(gauge, rows[2]);
}

fn draw_rate_chart_compact(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(COLOR_BORDER))
        .title(chart_title(app));

    if app.rate_history.is_empty() {
        let msg = Paragraph::new("Collecting data...")
            .style(Style::default().fg(COLOR_MUTED))
            .block(block);
        f.render_widget(msg, area);
        return;
    }

    let inner = block.inner(area);
    f.render_widget(block, area);

    let chart_result = render_bar_chart(
        &app.rate_history,
        inner.width as usize,
        inner.height as usize,
    );

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

    let samples = app.rate_history.len();
    let duration = if samples >= 3600 {
        "1h".to_string()
    } else if samples >= 60 {
        format!("{}m", samples / 60)
    } else {
        format!("{}s", samples)
    };

    let axis_label = Span::styled(format!(" {} ", duration), Style::default().fg(COLOR_MUTED));
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
        .border_style(Style::default().fg(COLOR_BORDER))
        .title(Span::styled(
            "Memory Usage",
            Style::default().fg(Color::White),
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
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner);

    let bulk_indicator = if mem.bulk_sync_mode {
        Span::styled(" [BULK]", Style::default().fg(Color::Yellow))
    } else {
        Span::raw("")
    };

    let left_lines = vec![
        Line::from(vec![
            Span::styled(
                format!("{:>14}", "RocksDB:"),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(
                format!(" {:>10}", format_bytes(mem.rocksdb_total_bytes)),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!("{:>14}", "Memtable:"),
                Style::default().fg(Color::Gray),
            ),
            Span::raw(format!(" {:>10}", format_bytes(mem.rocksdb_memtable_bytes))),
        ]),
        Line::from(vec![
            Span::styled(
                format!("{:>14}", "Block Cache:"),
                Style::default().fg(Color::Gray),
            ),
            Span::raw(format!(
                " {:>10}",
                format_bytes(mem.rocksdb_block_cache_bytes)
            )),
        ]),
    ];
    f.render_widget(Paragraph::new(left_lines), cols[0]);

    let right_lines = vec![
        Line::from(vec![
            Span::styled(
                format!("{:>16}", "Live Cells:"),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(
                format!(" {:>10}", format_count(mem.live_cells_count)),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            bulk_indicator,
        ]),
        Line::from(vec![
            Span::styled(
                format!("{:>16}", "Consumed Cache:"),
                Style::default().fg(Color::Gray),
            ),
            Span::raw(format!(
                " {:>10} ({})",
                format_count(mem.consumed_cells_count),
                format_bytes(mem.consumed_cells_bytes)
            )),
        ]),
        Line::from(vec![
            Span::styled(
                format!("{:>16}", "Headers:"),
                Style::default().fg(Color::Gray),
            ),
            Span::raw(format!(" {:>10}", format_count(mem.block_headers_count))),
        ]),
    ];
    f.render_widget(Paragraph::new(right_lines), cols[1]);
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

fn format_count(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.2}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}K", count as f64 / 1_000.0)
    } else {
        format!("{}", count)
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
    let stats = ChartStats::from_history(&app.rate_history);
    match stats {
        Some(s) => Line::from(vec![
            Span::raw("Sync Rate "),
            Span::styled(
                format!("now:{}", format_rate(s.current)),
                Style::default().fg(Color::Green),
            ),
            Span::styled(" │ ", Style::default().fg(COLOR_SEPARATOR)),
            Span::styled(
                format!("min:{}", format_rate(s.min)),
                Style::default().fg(Color::Red),
            ),
            Span::styled(" │ ", Style::default().fg(COLOR_SEPARATOR)),
            Span::styled(
                format!("avg:{}", format_rate(s.avg)),
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(" │ ", Style::default().fg(COLOR_SEPARATOR)),
            Span::styled(
                format!("max:{}", format_rate(s.max)),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        None => Line::from("Sync Rate (collecting...)"),
    }
}

fn draw_task_table(f: &mut Frame, app: &mut App, area: Rect) {
    let is_focused = app.focused_panel == FocusedPanel::Tasks;
    let border_color = if is_focused {
        Color::Cyan
    } else {
        COLOR_BORDER
    };

    let header_cells = ["ID", "Type", "Status", "Progress", "Rate", "ETA", "Created"]
        .iter()
        .map(|h| Cell::from(*h).style(Style::default().fg(Color::Yellow)));
    let header = Row::new(header_cells).style(Style::default()).height(1);

    let rows = app.tasks.iter().map(|task| {
        let id = task.id.to_string()[..8].to_string();
        let task_type = task.task_type.clone();
        let progress = format!("{:.1}%", task.progress_percent());
        let rate = task
            .rate_ema
            .map(|r| format!("{:.1}/s", r))
            .unwrap_or_default();
        let eta = task.eta_formatted().unwrap_or_default();
        let created = task.created_at.format("%Y-%m-%d %H:%M").to_string();

        let (status_icon, status_text, status_style) = match task.status.as_str() {
            "running" => ("▶ ", "running", Style::default().fg(Color::Green)),
            "pending" => ("◌ ", "pending", Style::default().fg(Color::Yellow)),
            "completed" => ("✓ ", "completed", Style::default().fg(Color::Blue)),
            "failed" => ("✗ ", "failed", Style::default().fg(Color::Red)),
            "cancelled" => ("○ ", "cancelled", Style::default().fg(COLOR_MUTED)),
            "paused" => ("⏸ ", "paused", Style::default().fg(Color::Magenta)),
            _ => ("  ", task.status.as_str(), Style::default()),
        };

        Row::new(vec![
            Cell::from(id),
            Cell::from(task_type),
            Cell::from(format!("{}{}", status_icon, status_text)).style(status_style),
            Cell::from(progress),
            Cell::from(rate),
            Cell::from(eta),
            Cell::from(created),
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
            Constraint::Length(10),
            Constraint::Length(16),
            Constraint::Length(14),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(18),
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
                    Color::Cyan
                } else {
                    Color::White
                }),
            )),
    )
    .row_highlight_style(Style::default().bg(COLOR_BORDER));

    f.render_stateful_widget(table, area, &mut app.table_state);
}

fn draw_task_detail(f: &mut Frame, app: &App, area: Rect) {
    let detail = if let Some(task) = app.selected_task() {
        let mut lines = vec![
            Line::from(vec![
                Span::styled("ID: ", Style::default().fg(COLOR_MUTED)),
                Span::raw(task.id.to_string()),
            ]),
            Line::from(vec![
                Span::styled("Type: ", Style::default().fg(COLOR_MUTED)),
                Span::styled(&task.task_type, Style::default().fg(Color::Cyan)),
            ]),
            Line::from(vec![
                Span::styled("Status: ", Style::default().fg(COLOR_MUTED)),
                Span::raw(&task.status),
            ]),
        ];

        if let Some(msg) = &task.progress_message {
            lines.push(Line::from(vec![
                Span::styled("Progress: ", Style::default().fg(COLOR_MUTED)),
                Span::raw(msg),
            ]));
        }

        if let Some(err) = &task.error_message {
            lines.push(Line::from(vec![
                Span::styled("Error: ", Style::default().fg(Color::Red)),
                Span::raw(err),
            ]));
        }

        if let Some(runner) = &task.runner_id {
            lines.push(Line::from(vec![
                Span::styled("Runner: ", Style::default().fg(COLOR_MUTED)),
                Span::raw(runner),
            ]));
        }

        if let Some(elapsed) = task.elapsed_formatted() {
            lines.push(Line::from(vec![
                Span::styled("Elapsed: ", Style::default().fg(COLOR_MUTED)),
                Span::raw(elapsed),
            ]));
        }

        Paragraph::new(lines)
    } else {
        Paragraph::new(Span::styled(
            "No task selected",
            Style::default().fg(COLOR_MUTED),
        ))
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(COLOR_BORDER))
        .title(Span::styled("Details", Style::default().fg(Color::White)));
    f.render_widget(detail.block(block).wrap(Wrap { trim: true }), area);
}

fn draw_log(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.focused_panel == FocusedPanel::Log;
    let border_color = if is_focused {
        Color::Cyan
    } else {
        COLOR_BORDER
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
                Color::Cyan
            } else {
                Color::White
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
                    Style::default().fg(COLOR_MUTED),
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
                "Tab: Log │ j/k: Navigate │ n: New │ c: Cancel │ p: Pause │ r: Retry │ d: Del │ q: Quit".to_string()
            }
            FocusedPanel::Log => {
                "Tab: Tasks │ j/k: Scroll │ g: Bottom │ G: Top │ q: Quit".to_string()
            }
        }
    };

    let status = app
        .status_message
        .as_deref()
        .map(|s| s.to_string())
        .unwrap_or(help);

    let footer = Paragraph::new(status)
        .style(Style::default().fg(COLOR_MUTED))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(COLOR_BORDER)),
        );
    f.render_widget(footer, area);
}

fn draw_dialog(f: &mut Frame, app: &App, dialog: &DialogType) {
    let area = centered_rect(50, 40, f.area());
    f.render_widget(Clear, area);

    match dialog {
        DialogType::NewTask => {
            let options = [
                ("Cycles Backfill", "Backfill transaction cycles from RPC"),
                ("Index Rebuild", "Rebuild deferred indexes and constraints"),
                ("Label Import", "Import UDT/script labels from token-labels"),
                ("Statistics Rebuild", "Rebuild all aggregate statistics"),
                ("Live Cells Populate", "Populate live_cells from RocksDB"),
                ("Spore Rebuild", "Rebuild Spore NFT data"),
                (
                    "Secondary Issuance Backfill",
                    "Backfill secondary issuance data",
                ),
                ("MNFT Rebuild", "Rebuild M-NFT issuers/classes/tokens"),
                ("DotBit Rebuild", "Rebuild DotBit accounts"),
            ];
            let items: Vec<Line> = options
                .iter()
                .enumerate()
                .map(|(i, (name, desc))| {
                    if i == app.dialog_selection {
                        Line::from(vec![
                            Span::styled(
                                format!(" ▶ {}", name),
                                Style::default()
                                    .fg(Color::Cyan)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(format!("  {}", desc), Style::default().fg(COLOR_MUTED)),
                        ])
                    } else {
                        Line::from(vec![
                            Span::styled(format!("   {}", name), Style::default().fg(Color::White)),
                            Span::styled(format!("  {}", desc), Style::default().fg(COLOR_MUTED)),
                        ])
                    }
                })
                .collect();

            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(Span::styled(
                    " New Task ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ));
            let dialog_widget = Paragraph::new(items).block(block);
            f.render_widget(dialog_widget, area);
        }
        DialogType::Confirm(msg) => {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .title(Span::styled(
                    " Confirm ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
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
