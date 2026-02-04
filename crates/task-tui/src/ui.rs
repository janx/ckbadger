use anyhow::Result;
use chrono::Local;
use ckbadger_common::{
    CyclesBackfillConfig, IndexRebuildConfig, LabelImportConfig, LiveCellsPopulateConfig,
    SecondaryIssuanceBackfillConfig, SporeRebuildConfig, StatisticsRebuildConfig, Task,
    TaskBuilder,
};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Bar, BarChart, BarGroup, Block, Borders, Cell, Clear, Gauge, Paragraph, Row, Table,
        TableState, Wrap,
    },
    Frame,
};
use std::collections::VecDeque;
use std::time::Instant;

use crate::db::{SyncStatusRow, TaskDb};

const RATE_HISTORY_SIZE: usize = 3600;

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub enum DialogType {
    NewTask,
    Confirm(&'static str),
}

pub struct App {
    db: TaskDb,
    tasks: Vec<Task>,
    sync_status: Option<SyncStatusRow>,
    table_state: TableState,
    last_refresh: Instant,
    last_sample: Instant,
    dialog: Option<DialogType>,
    dialog_selection: usize,
    status_message: Option<String>,
    rate_history: VecDeque<f64>,
}

impl App {
    pub fn new(db: TaskDb) -> Self {
        Self {
            db,
            tasks: Vec::new(),
            sync_status: None,
            table_state: TableState::default(),
            last_refresh: Instant::now(),
            last_sample: Instant::now(),
            dialog: None,
            dialog_selection: 0,
            status_message: None,
            rate_history: VecDeque::with_capacity(RATE_HISTORY_SIZE),
        }
    }

    pub async fn refresh(&mut self) -> Result<()> {
        self.tasks = self.db.list_tasks(100).await?;
        self.sync_status = self.db.get_sync_status().await.ok();
        self.last_refresh = Instant::now();

        if self.last_sample.elapsed().as_secs() >= 1 {
            self.sample_rate();
            self.last_sample = Instant::now();
        }

        if self.table_state.selected().is_none() && !self.tasks.is_empty() {
            self.table_state.select(Some(0));
        }
        Ok(())
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
            self.dialog_selection = (self.dialog_selection + 1) % 7;
        }
    }

    pub fn previous_dialog_option(&mut self) {
        if let Some(DialogType::NewTask) = self.dialog {
            self.dialog_selection = if self.dialog_selection == 0 {
                6
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
            Constraint::Length(7),
            Constraint::Length(10),
            Constraint::Min(8),
            Constraint::Length(10),
            Constraint::Length(3),
        ])
        .split(f.area());

    draw_header(f, app, chunks[0]);
    draw_sync_status(f, app, chunks[1]);
    draw_rate_chart(f, app, chunks[2]);
    draw_task_table(f, app, chunks[3]);
    draw_task_detail(f, app, chunks[4]);
    draw_footer(f, app, chunks[5]);
    draw_task_table(f, app, chunks[3]);
    draw_task_detail(f, app, chunks[4]);
    draw_footer(f, app, chunks[5]);

    if let Some(dialog) = &app.dialog {
        draw_dialog(f, app, dialog);
    }
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner);

    let title = Paragraph::new("ckbadger Task Manager").style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );
    f.render_widget(title, cols[0]);

    let now = Local::now();
    let elapsed_ms = app.last_refresh.elapsed().as_millis();
    let time_info = Line::from(vec![
        Span::styled(
            format!("{}ms ago", elapsed_ms),
            Style::default().fg(if elapsed_ms > 2000 {
                Color::Yellow
            } else {
                Color::Green
            }),
        ),
        Span::styled(" | ", Style::default().fg(Color::Gray)),
        Span::styled(
            now.format("%H:%M:%S").to_string(),
            Style::default().fg(Color::White),
        ),
    ]);
    let time_widget = Paragraph::new(time_info).alignment(Alignment::Right);
    f.render_widget(time_widget, cols[1]);
}

fn draw_sync_status(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title("Sync Status");

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
            Constraint::Length(2),
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

    let idx_status = if sync.indexes_deferred {
        " [IDX DEFERRED]"
    } else {
        ""
    };

    let status_line = Line::from(vec![
        Span::styled(
            format!(" {} ", mode),
            Style::default().fg(Color::Black).bg(mode_color),
        ),
        Span::raw(format!(
            " Block {}/{} ({:.2}%){}",
            sync.tip_block, sync.chain_tip, sync.progress, idx_status
        )),
    ]);
    f.render_widget(Paragraph::new(status_line), rows[0]);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[1]);

    let format_rate = |rate: f64| -> String {
        if rate >= 1000.0 {
            format!("{:.1}K", rate / 1000.0)
        } else {
            format!("{:.0}", rate)
        }
    };

    let mut left_lines = Vec::new();
    if let Some(realtime) = sync.rate_realtime {
        if realtime > 0.0 {
            left_lines.push(Line::from(vec![
                Span::styled("Speed: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    format_rate(realtime),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" blk/s"),
            ]));
        }
    }
    if let Some(ema) = sync.rate_ema {
        if ema > 0.0 {
            left_lines.push(Line::from(vec![
                Span::styled("EMA:   ", Style::default().fg(Color::Gray)),
                Span::raw(format!("{} blk/s", format_rate(ema))),
            ]));
        }
    }
    if left_lines.is_empty() {
        left_lines.push(Line::from(Span::styled(
            "--",
            Style::default().fg(Color::Gray),
        )));
    }
    f.render_widget(Paragraph::new(left_lines), cols[0]);

    let mut right_lines = Vec::new();
    if let Some(ref eta) = sync.eta {
        right_lines.push(Line::from(vec![
            Span::styled("ETA:     ", Style::default().fg(Color::Gray)),
            Span::styled(
                eta.clone(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    if let Some(ref elapsed) = sync.elapsed_time {
        right_lines.push(Line::from(vec![
            Span::styled("Elapsed: ", Style::default().fg(Color::Gray)),
            Span::raw(elapsed.clone()),
        ]));
    }
    f.render_widget(Paragraph::new(right_lines), cols[1]);

    let ratio = (sync.progress / 100.0).clamp(0.0, 1.0);
    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(Color::Cyan).bg(Color::DarkGray))
        .ratio(ratio)
        .label(format!("{:.2}%", sync.progress))
        .use_unicode(true);
    f.render_widget(gauge, rows[2]);
}

fn draw_rate_chart(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Sync Rate (1h)");

    if app.rate_history.is_empty() {
        let msg = Paragraph::new("Collecting data...").block(block);
        f.render_widget(msg, area);
        return;
    }

    let inner = block.inner(area);
    let bar_width: u16 = 2;
    let bar_gap: u16 = 1;
    let bar_total_width = bar_width + bar_gap;
    let num_bars = (inner.width / bar_total_width).max(1) as usize;

    let bucket_size = app.rate_history.len().div_ceil(num_bars);
    let bucket_size = bucket_size.max(1);

    let max_rate = app
        .rate_history
        .iter()
        .cloned()
        .fold(0.0_f64, f64::max)
        .max(100.0);

    let bars: Vec<Bar> = app
        .rate_history
        .iter()
        .collect::<Vec<_>>()
        .chunks(bucket_size)
        .map(|chunk| {
            let avg = chunk.iter().copied().sum::<f64>() / chunk.len() as f64;
            let ratio = avg / max_rate;
            let color = if ratio > 0.8 {
                Color::Green
            } else if ratio > 0.5 {
                Color::Cyan
            } else if ratio > 0.2 {
                Color::Yellow
            } else {
                Color::Red
            };
            Bar::default()
                .value(avg as u64)
                .style(Style::default().fg(color))
        })
        .collect();

    let samples = app.rate_history.len();
    let duration_label = if samples >= 3600 {
        "1h"
    } else if samples >= 60 {
        "< 1h"
    } else {
        "< 1m"
    };

    let barchart = BarChart::default()
        .block(block)
        .data(BarGroup::default().bars(&bars))
        .bar_width(bar_width)
        .bar_gap(bar_gap)
        .value_style(Style::default().fg(Color::Reset).bg(Color::Reset))
        .max(max_rate as u64)
        .label_style(Style::default().fg(Color::Gray));

    f.render_widget(barchart, area);

    let label = format!(
        "{} | max: {} blk/s",
        duration_label,
        format_rate_short(max_rate)
    );
    let label_widget = Paragraph::new(label)
        .style(Style::default().fg(Color::Gray))
        .alignment(Alignment::Right);
    let label_area = Rect {
        x: area.x + 1,
        y: area.y,
        width: area.width.saturating_sub(2),
        height: 1,
    };
    f.render_widget(label_widget, label_area);
}

fn format_rate_short(rate: f64) -> String {
    if rate >= 1000.0 {
        format!("{:.0}K", rate / 1000.0)
    } else {
        format!("{:.0}", rate)
    }
}

fn draw_task_table(f: &mut Frame, app: &mut App, area: Rect) {
    let header_cells = ["ID", "Type", "Status", "Progress", "Rate", "ETA", "Created"]
        .iter()
        .map(|h| Cell::from(*h).style(Style::default().fg(Color::Yellow)));
    let header = Row::new(header_cells).style(Style::default()).height(1);

    let rows = app.tasks.iter().map(|task| {
        let id = task.id.to_string()[..8].to_string();
        let task_type = task.task_type.clone();
        let status = task.status.clone();
        let progress = format!("{:.1}%", task.progress_percent());
        let rate = task
            .rate_ema
            .map(|r| format!("{:.1}/s", r))
            .unwrap_or_default();
        let eta = task.eta_formatted().unwrap_or_default();
        let created = task.created_at.format("%Y-%m-%d %H:%M").to_string();

        let status_style = match task.status.as_str() {
            "running" => Style::default().fg(Color::Green),
            "pending" => Style::default().fg(Color::Yellow),
            "completed" => Style::default().fg(Color::Blue),
            "failed" => Style::default().fg(Color::Red),
            "cancelled" => Style::default().fg(Color::DarkGray),
            "paused" => Style::default().fg(Color::Magenta),
            _ => Style::default(),
        };

        Row::new(vec![
            Cell::from(id),
            Cell::from(task_type),
            Cell::from(status).style(status_style),
            Cell::from(progress),
            Cell::from(rate),
            Cell::from(eta),
            Cell::from(created),
        ])
    });

    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Length(16),
            Constraint::Length(12),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(18),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title("Tasks"))
    .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    f.render_stateful_widget(table, area, &mut app.table_state);
}

fn draw_task_detail(f: &mut Frame, app: &App, area: Rect) {
    let detail = if let Some(task) = app.selected_task() {
        let mut lines = vec![
            Line::from(vec![
                Span::styled("ID: ", Style::default().fg(Color::Yellow)),
                Span::raw(task.id.to_string()),
            ]),
            Line::from(vec![
                Span::styled("Type: ", Style::default().fg(Color::Yellow)),
                Span::raw(&task.task_type),
            ]),
            Line::from(vec![
                Span::styled("Status: ", Style::default().fg(Color::Yellow)),
                Span::raw(&task.status),
            ]),
        ];

        if let Some(msg) = &task.progress_message {
            lines.push(Line::from(vec![
                Span::styled("Progress: ", Style::default().fg(Color::Yellow)),
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
                Span::styled("Runner: ", Style::default().fg(Color::Yellow)),
                Span::raw(runner),
            ]));
        }

        if let Some(elapsed) = task.elapsed_formatted() {
            lines.push(Line::from(vec![
                Span::styled("Elapsed: ", Style::default().fg(Color::Yellow)),
                Span::raw(elapsed),
            ]));
        }

        Paragraph::new(lines)
    } else {
        Paragraph::new("No task selected")
    };

    let block = Block::default().borders(Borders::ALL).title("Details");
    f.render_widget(detail.block(block).wrap(Wrap { trim: true }), area);
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let help = if app.dialog.is_some() {
        "Enter: Confirm | Esc: Cancel | Tab: Next option"
    } else {
        "n: New | c: Cancel | p: Pause | r: Resume/Retry | d: Delete | R: Refresh | q: Quit"
    };

    let status = app.status_message.as_deref().unwrap_or(help);

    let footer = Paragraph::new(status)
        .style(Style::default().fg(Color::Cyan))
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(footer, area);
}

fn draw_dialog(f: &mut Frame, app: &App, dialog: &DialogType) {
    let area = centered_rect(50, 40, f.area());
    f.render_widget(Clear, area);

    match dialog {
        DialogType::NewTask => {
            let options = [
                "Cycles Backfill",
                "Index Rebuild",
                "Label Import",
                "Statistics Rebuild",
                "Live Cells Populate",
                "Spore Rebuild",
                "Secondary Issuance Backfill",
            ];
            let items: Vec<Line> = options
                .iter()
                .enumerate()
                .map(|(i, opt)| {
                    let style = if i == app.dialog_selection {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };
                    let prefix = if i == app.dialog_selection {
                        "> "
                    } else {
                        "  "
                    };
                    Line::from(vec![Span::styled(format!("{}{}", prefix, opt), style)])
                })
                .collect();

            let dialog_widget = Paragraph::new(items)
                .block(Block::default().borders(Borders::ALL).title("New Task"));
            f.render_widget(dialog_widget, area);
        }
        DialogType::Confirm(msg) => {
            let dialog_widget =
                Paragraph::new(*msg).block(Block::default().borders(Borders::ALL).title("Confirm"));
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
