use anyhow::Result;
use ckbadger_common::{
    CyclesBackfillConfig, IndexRebuildConfig, LabelImportConfig, LiveCellsPopulateConfig,
    StatisticsRebuildConfig, Task, TaskBuilder,
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Gauge, Paragraph, Row, Table, TableState, Wrap},
    Frame,
};
use std::time::Instant;

use crate::db::{SyncStatusRow, TaskDb};

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
    dialog: Option<DialogType>,
    dialog_selection: usize,
    status_message: Option<String>,
}

impl App {
    pub fn new(db: TaskDb) -> Self {
        Self {
            db,
            tasks: Vec::new(),
            sync_status: None,
            table_state: TableState::default(),
            last_refresh: Instant::now(),
            dialog: None,
            dialog_selection: 0,
            status_message: None,
        }
    }

    pub async fn refresh(&mut self) -> Result<()> {
        self.tasks = self.db.list_tasks(100).await?;
        self.sync_status = self.db.get_sync_status().await.ok();
        self.last_refresh = Instant::now();
        if self.table_state.selected().is_none() && !self.tasks.is_empty() {
            self.table_state.select(Some(0));
        }
        Ok(())
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
            self.dialog_selection = (self.dialog_selection + 1) % 5;
        }
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
            Constraint::Length(6),
            Constraint::Min(10),
            Constraint::Length(10),
            Constraint::Length(3),
        ])
        .split(f.area());

    draw_header(f, chunks[0]);
    draw_sync_status(f, app, chunks[1]);
    draw_task_table(f, app, chunks[2]);
    draw_task_detail(f, app, chunks[3]);
    draw_footer(f, app, chunks[4]);

    if let Some(dialog) = &app.dialog {
        draw_dialog(f, app, dialog);
    }
}

fn draw_header(f: &mut Frame, area: Rect) {
    let title = Paragraph::new("ckbadger Task Manager")
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, area);
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

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
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
    f.render_widget(Paragraph::new(status_line), chunks[0]);

    let mut info_spans = Vec::new();
    if let Some(rate) = sync.rate {
        if rate > 0.0 {
            let rate_str = if rate >= 1000.0 {
                format!("{:.1}K", rate / 1000.0)
            } else {
                format!("{:.0}", rate)
            };
            info_spans.push(Span::styled("Rate: ", Style::default().fg(Color::DarkGray)));
            info_spans.push(Span::raw(format!("{} blk/s  ", rate_str)));
        }
    }
    if let Some(ref elapsed) = sync.elapsed_time {
        info_spans.push(Span::styled(
            "Elapsed: ",
            Style::default().fg(Color::DarkGray),
        ));
        info_spans.push(Span::raw(format!("{}  ", elapsed)));
    }
    if let Some(ref eta) = sync.eta {
        info_spans.push(Span::styled("ETA: ", Style::default().fg(Color::DarkGray)));
        info_spans.push(Span::raw(eta.clone()));
    }

    if !info_spans.is_empty() {
        f.render_widget(Paragraph::new(Line::from(info_spans)), chunks[1]);
    }

    let ratio = (sync.progress / 100.0).clamp(0.0, 1.0);
    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(mode_color))
        .ratio(ratio)
        .label(format!("{:.2}%", sync.progress));
    f.render_widget(gauge, chunks[2]);
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
