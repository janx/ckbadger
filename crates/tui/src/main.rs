use anyhow::Result;
use clap::Parser;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, List, ListItem, Paragraph, Row, Table, TableState, Tabs};
use ratatui::Frame;
use std::time::Duration;

mod control_plane;

use control_plane::{ControlPlane, Instance, InstanceStatus, SyncEvent, SyncJob, SyncPhase};

#[derive(Parser)]
#[command(name = "ckbadger-tui")]
#[command(about = "CKBadger database management TUI")]
struct Args {
    #[arg(long, env = "CONTROL_DATABASE_URL")]
    control_db_url: String,
}

#[derive(Default, Clone, Copy, PartialEq)]
enum Tab {
    #[default]
    Instances,
    Jobs,
    Events,
    Integrity,
    Config,
}

impl Tab {
    const ALL: [Tab; 5] = [Tab::Instances, Tab::Jobs, Tab::Events, Tab::Integrity, Tab::Config];

    fn title(&self) -> &'static str {
        match self {
            Tab::Instances => "Instances",
            Tab::Jobs => "Jobs",
            Tab::Events => "Events",
            Tab::Integrity => "Integrity",
            Tab::Config => "Config",
        }
    }

    fn index(&self) -> usize {
        match self {
            Tab::Instances => 0,
            Tab::Jobs => 1,
            Tab::Events => 2,
            Tab::Integrity => 3,
            Tab::Config => 4,
        }
    }
}

struct App {
    control_plane: ControlPlane,
    current_tab: Tab,
    instances: Vec<Instance>,
    jobs: Vec<SyncJob>,
    events: Vec<SyncEvent>,
    active_instance_id: Option<uuid::Uuid>,
    instance_table_state: TableState,
    job_table_state: TableState,
    integrity_table_state: TableState,
    should_quit: bool,
    last_refresh: std::time::Instant,
    status_message: Option<(String, Color)>,
    db_connected: bool,
}

impl App {
    async fn new(control_db_url: &str) -> Result<Self> {
        let control_plane = ControlPlane::connect(control_db_url).await?;
        let instances = control_plane.list_instances().await?;
        let jobs = control_plane.list_running_jobs().await?;
        let events = control_plane.list_recent_events(50).await?;
        let active_instance_id = control_plane.get_active_instance_id().await?;

        let mut instance_table_state = TableState::default();
        if !instances.is_empty() {
            instance_table_state.select(Some(0));
        }

        let mut job_table_state = TableState::default();
        if !jobs.is_empty() {
            job_table_state.select(Some(0));
        }

        let mut integrity_table_state = TableState::default();
        if !instances.is_empty() {
            integrity_table_state.select(Some(0));
        }

        Ok(Self {
            control_plane,
            current_tab: Tab::Instances,
            instances,
            jobs,
            events,
            active_instance_id,
            instance_table_state,
            job_table_state,
            integrity_table_state,
            should_quit: false,
            last_refresh: std::time::Instant::now(),
            status_message: None,
            db_connected: true,
        })
    }

    async fn refresh(&mut self) -> Result<()> {
        self.instances = self.control_plane.list_instances().await?;
        self.jobs = self.control_plane.list_running_jobs().await?;
        self.events = self.control_plane.list_recent_events(50).await?;
        self.active_instance_id = self.control_plane.get_active_instance_id().await?;
        self.last_refresh = std::time::Instant::now();
        Ok(())
    }

    fn next_tab(&mut self) {
        let idx = (self.current_tab.index() + 1) % Tab::ALL.len();
        self.current_tab = Tab::ALL[idx];
    }

    fn previous_tab(&mut self) {
        let idx = if self.current_tab.index() == 0 {
            Tab::ALL.len() - 1
        } else {
            self.current_tab.index() - 1
        };
        self.current_tab = Tab::ALL[idx];
    }

    fn next_row(&mut self) {
        match self.current_tab {
            Tab::Instances => {
                if self.instances.is_empty() {
                    return;
                }
                let i = self.instance_table_state.selected().unwrap_or(0);
                self.instance_table_state
                    .select(Some((i + 1) % self.instances.len()));
            }
            Tab::Jobs => {
                if self.jobs.is_empty() {
                    return;
                }
                let i = self.job_table_state.selected().unwrap_or(0);
                self.job_table_state
                    .select(Some((i + 1) % self.jobs.len()));
            }
            Tab::Integrity => {
                if self.instances.is_empty() {
                    return;
                }
                let i = self.integrity_table_state.selected().unwrap_or(0);
                self.integrity_table_state
                    .select(Some((i + 1) % self.instances.len()));
            }
            _ => {}
        }
    }

    fn previous_row(&mut self) {
        match self.current_tab {
            Tab::Instances => {
                if self.instances.is_empty() {
                    return;
                }
                let i = self.instance_table_state.selected().unwrap_or(0);
                let prev = if i == 0 {
                    self.instances.len() - 1
                } else {
                    i - 1
                };
                self.instance_table_state.select(Some(prev));
            }
            Tab::Jobs => {
                if self.jobs.is_empty() {
                    return;
                }
                let i = self.job_table_state.selected().unwrap_or(0);
                let prev = if i == 0 { self.jobs.len() - 1 } else { i - 1 };
                self.job_table_state.select(Some(prev));
            }
            Tab::Integrity => {
                if self.instances.is_empty() {
                    return;
                }
                let i = self.integrity_table_state.selected().unwrap_or(0);
                let prev = if i == 0 {
                    self.instances.len() - 1
                } else {
                    i - 1
                };
                self.integrity_table_state.select(Some(prev));
            }
            _ => {}
        }
    }

    async fn activate_selected_instance(&mut self) -> Result<()> {
        if let Some(idx) = self.instance_table_state.selected() {
            if let Some(instance) = self.instances.get(idx) {
                if instance.status == InstanceStatus::Ready
                    || instance.status == InstanceStatus::Active
                {
                    self.control_plane
                        .set_active_instance(&instance.id, "tui")
                        .await?;
                    self.status_message = Some((
                        format!("Activated instance: {}", instance.name),
                        Color::Green,
                    ));
                    self.refresh().await?;
                } else {
                    self.status_message = Some((
                        format!(
                            "Cannot activate instance with status: {:?}",
                            instance.status
                        ),
                        Color::Red,
                    ));
                }
            }
        }
        Ok(())
    }

    async fn cancel_selected_job(&mut self) -> Result<()> {
        if let Some(idx) = self.job_table_state.selected() {
            if let Some(job) = self.jobs.get(idx) {
                self.control_plane.cancel_job(&job.id).await?;
                self.status_message = Some((
                    format!("Cancelled job: {}", job.job_type),
                    Color::Yellow,
                ));
                self.refresh().await?;
            }
        }
        Ok(())
    }

    fn get_selected_integrity_instance(&self) -> Option<&Instance> {
        self.integrity_table_state
            .selected()
            .and_then(|idx| self.instances.get(idx))
    }

    async fn trigger_integrity_job(&mut self, job_type: &str) -> Result<()> {
        let Some(instance) = self.get_selected_integrity_instance() else {
            self.status_message = Some((
                "No instance selected".to_string(),
                Color::Red,
            ));
            return Ok(());
        };

        let instance_id = instance.id;
        let instance_name = instance.name.clone();

        let job_id = self
            .control_plane
            .create_job(&instance_id, job_type, None)
            .await?;

        self.status_message = Some((
            format!(
                "Created job: {} on {} ({})",
                job_type,
                instance_name,
                &job_id.to_string()[..8]
            ),
            Color::Green,
        ));
        self.refresh().await?;
        Ok(())
    }

    fn handle_key(&mut self, key: event::KeyEvent) -> Option<AsyncAction> {
        if key.kind != KeyEventKind::Press {
            return None;
        }

        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
                None
            }
            KeyCode::Tab | KeyCode::Right => {
                self.next_tab();
                None
            }
            KeyCode::BackTab | KeyCode::Left => {
                self.previous_tab();
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.next_row();
                None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.previous_row();
                None
            }
            KeyCode::Char('a') if self.current_tab == Tab::Instances => {
                Some(AsyncAction::ActivateInstance)
            }
            KeyCode::Char('c') if self.current_tab == Tab::Jobs => Some(AsyncAction::CancelJob),
            KeyCode::Char('r') => Some(AsyncAction::Refresh),
            KeyCode::Char('1') if self.current_tab == Tab::Integrity => {
                Some(AsyncAction::TriggerCyclesFix)
            }
            KeyCode::Char('2') if self.current_tab == Tab::Integrity => {
                Some(AsyncAction::TriggerUdtLabels)
            }
            KeyCode::Char('3') if self.current_tab == Tab::Integrity => {
                Some(AsyncAction::TriggerScriptLabels)
            }
            _ => None,
        }
    }

    fn get_active_instance(&self) -> Option<&Instance> {
        self.active_instance_id.as_ref().and_then(|id| {
            self.instances.iter().find(|i| &i.id == id)
        })
    }
}

enum AsyncAction {
    ActivateInstance,
    CancelJob,
    Refresh,
    TriggerCyclesFix,
    TriggerUdtLabels,
    TriggerScriptLabels,
}

fn render(frame: &mut Frame, app: &mut App) {
    let [header_area, tabs_area, content_area, status_area, help_area] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(3),
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    render_header(frame, header_area, app);
    render_tabs(frame, tabs_area, app);
    render_content(frame, content_area, app);
    render_status(frame, status_area, app);
    render_help(frame, help_area, app);
}

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let active_info = if let Some(instance) = app.get_active_instance() {
        let phase_str = match &instance.sync_phase {
            SyncPhase::Completed => "Ready".to_string(),
            SyncPhase::CoreSync => {
                let progress = if let Some(target) = instance.target_block {
                    format!("{}/{}", format_number(instance.current_block), format_number(target))
                } else {
                    format_number(instance.current_block)
                };
                format!("Syncing: {}", progress)
            }
            other => format!("{:?}", other),
        };
        let speed_str = instance
            .sync_speed
            .map(|s| format!("{:.0}/s", s))
            .unwrap_or_default();
        let eta = calculate_phase_eta(instance, &app.jobs);
        let eta_str = if eta != "-" {
            format!(" | ETA: {}", eta)
        } else {
            String::new()
        };
        format!(
            "Active: {} ({}) | {} | {}{}",
            instance.name, instance.network, phase_str, speed_str, eta_str
        )
    } else {
        "No active instance".to_string()
    };

    let header = Paragraph::new(Line::from(vec![
        Span::styled("CKBadger Control Plane", Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(" | "),
        Span::styled(active_info, Style::new().fg(Color::White)),
    ]));

    frame.render_widget(header, area);
}

fn render_tabs(frame: &mut Frame, area: Rect, app: &App) {
    let titles: Vec<&str> = Tab::ALL.iter().map(|t| t.title()).collect();

    let tabs = Tabs::new(titles)
        .block(Block::bordered())
        .select(app.current_tab.index())
        .style(Style::new().fg(Color::White))
        .highlight_style(
            Style::new()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )
        .divider(" | ");

    frame.render_widget(tabs, area);
}

fn render_content(frame: &mut Frame, area: Rect, app: &mut App) {
    match app.current_tab {
        Tab::Instances => render_instances_table(frame, area, app),
        Tab::Jobs => render_jobs_table(frame, area, app),
        Tab::Events => render_events(frame, area, app),
        Tab::Integrity => render_integrity(frame, area, app),
        Tab::Config => render_config(frame, area, app),
    }
}

fn render_instances_table(frame: &mut Frame, area: Rect, app: &mut App) {
    let header = Row::new(vec![
        Cell::from(""),
        Cell::from("Name"),
        Cell::from("Status"),
        Cell::from("Phase"),
        Cell::from("Block"),
        Cell::from("Speed"),
        Cell::from("ETA"),
        Cell::from("Network"),
    ])
    .style(
        Style::new()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )
    .bottom_margin(1);

    let rows: Vec<Row> = app
        .instances
        .iter()
        .map(|instance| {
            let is_active = app.active_instance_id.as_ref() == Some(&instance.id);
            let active_marker = if is_active { "*" } else { " " };

            let status_style = match instance.status {
                InstanceStatus::Active => Style::new().fg(Color::Green),
                InstanceStatus::Ready => Style::new().fg(Color::Cyan),
                InstanceStatus::Syncing | InstanceStatus::Rebuilding => {
                    Style::new().fg(Color::Yellow)
                }
                InstanceStatus::Failed => Style::new().fg(Color::Red),
                _ => Style::new(),
            };

            let progress = if let Some(target) = instance.target_block {
                if target > 0 {
                    format!(
                        "{} / {} ({:.1}%)",
                        format_number(instance.current_block),
                        format_number(target),
                        instance.current_block as f64 / target as f64 * 100.0
                    )
                } else {
                    format_number(instance.current_block)
                }
            } else {
                format_number(instance.current_block)
            };

            let speed = instance
                .sync_speed
                .map(|s| format!("{:.0} blk/s", s))
                .unwrap_or_else(|| "-".to_string());

            let phase_str = match instance.sync_phase {
                SyncPhase::Pending => "Pending",
                SyncPhase::CoreSync => "Core Sync",
                SyncPhase::RebuildCellStatus => "Rebuild: cell_status",
                SyncPhase::RebuildLiveCells => "Rebuild: live_cells",
                SyncPhase::RebuildAddressBalances => "Rebuild: balances",
                SyncPhase::RebuildScriptUsageStats => "Rebuild: scripts",
                SyncPhase::RebuildDaoDeposits => "Rebuild: dao",
                SyncPhase::RebuildUdtCells => "Rebuild: udt",
                SyncPhase::RebuildDailyStatistics => "Rebuild: daily",
                SyncPhase::RebuildHourlyStatistics => "Rebuild: hourly",
                SyncPhase::RebuildEpochStatistics => "Rebuild: epoch",
                SyncPhase::RebuildMinerStatistics => "Rebuild: miner",
                SyncPhase::RebuildIndexes => "Rebuild: indexes",
                SyncPhase::Completed => "Completed",
            };

            let eta = calculate_phase_eta(instance, &app.jobs);

            Row::new(vec![
                Cell::from(active_marker).style(Style::new().fg(Color::Green)),
                Cell::from(instance.name.clone()),
                Cell::from(format!("{:?}", instance.status)).style(status_style),
                Cell::from(phase_str),
                Cell::from(progress),
                Cell::from(speed),
                Cell::from(eta),
                Cell::from(instance.network.clone()),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(1),
        Constraint::Percentage(15),
        Constraint::Percentage(10),
        Constraint::Percentage(14),
        Constraint::Percentage(20),
        Constraint::Percentage(10),
        Constraint::Percentage(10),
        Constraint::Percentage(10),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::bordered().title(format!("Database Instances ({})", app.instances.len())))
        .row_highlight_style(
            Style::new()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    frame.render_stateful_widget(table, area, &mut app.instance_table_state);
}

fn render_jobs_table(frame: &mut Frame, area: Rect, app: &mut App) {
    if app.jobs.is_empty() {
        let block = Block::bordered().title("Running Jobs (0)");
        let paragraph = Paragraph::new("No running jobs")
            .style(Style::new().fg(Color::DarkGray))
            .block(block);
        frame.render_widget(paragraph, area);
        return;
    }

    let header = Row::new(vec![
        Cell::from("Instance"),
        Cell::from("Type"),
        Cell::from("Status"),
        Cell::from("Progress"),
        Cell::from("Speed"),
        Cell::from("ETA"),
    ])
    .style(
        Style::new()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )
    .bottom_margin(1);

    let rows: Vec<Row> = app
        .jobs
        .iter()
        .map(|job| {
            let instance_name = app
                .instances
                .iter()
                .find(|i| i.id == job.instance_id)
                .map(|i| i.name.clone())
                .unwrap_or_else(|| job.instance_id.to_string()[..8].to_string());

            let progress = job
                .progress_percent
                .map(|p| format!("{:.1}%", p))
                .unwrap_or_else(|| "-".to_string());

            let speed = job
                .rows_per_second
                .map(|s| format!("{:.0}/s", s))
                .unwrap_or_else(|| "-".to_string());

            let eta = calculate_job_eta(job);

            let status_style = match job.status.as_str() {
                "running" => Style::new().fg(Color::Green),
                "pending" | "queued" => Style::new().fg(Color::Yellow),
                "failed" => Style::new().fg(Color::Red),
                _ => Style::new(),
            };

            Row::new(vec![
                Cell::from(instance_name),
                Cell::from(job.job_type.clone()),
                Cell::from(job.status.clone()).style(status_style),
                Cell::from(progress),
                Cell::from(speed),
                Cell::from(eta),
            ])
        })
        .collect();

    let widths = [
        Constraint::Percentage(20),
        Constraint::Percentage(20),
        Constraint::Percentage(12),
        Constraint::Percentage(15),
        Constraint::Percentage(13),
        Constraint::Percentage(13),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::bordered().title(format!("Running Jobs ({})", app.jobs.len())))
        .row_highlight_style(
            Style::new()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    frame.render_stateful_widget(table, area, &mut app.job_table_state);
}

fn render_events(frame: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = app
        .events
        .iter()
        .map(|event| {
            let severity_style = match event.severity.as_str() {
                "error" | "critical" => Style::new().fg(Color::Red),
                "warning" => Style::new().fg(Color::Yellow),
                "info" => Style::new().fg(Color::White),
                _ => Style::new().fg(Color::DarkGray),
            };

            let time_str = event.created_at.format("%H:%M:%S").to_string();

            ListItem::new(Line::from(vec![
                Span::styled(format!("[{}] ", time_str), Style::new().fg(Color::DarkGray)),
                Span::styled(
                    format!("{:8} ", event.severity.to_uppercase()),
                    severity_style,
                ),
                Span::styled(&event.event_type, Style::new().fg(Color::Cyan)),
                Span::raw(": "),
                Span::raw(&event.message),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::bordered().title(format!("Recent Events ({})", app.events.len())));

    frame.render_widget(list, area);
}

fn render_integrity(frame: &mut Frame, area: Rect, app: &mut App) {
    let [instance_area, jobs_area, help_area] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(7),
        Constraint::Length(4),
    ])
    .areas(area);

    let header = Row::new(vec![
        Cell::from(""),
        Cell::from("Name"),
        Cell::from("Network"),
        Cell::from("Status"),
        Cell::from("Block"),
    ])
    .style(
        Style::new()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )
    .bottom_margin(1);

    let rows: Vec<Row> = app
        .instances
        .iter()
        .map(|instance| {
            let is_active = app.active_instance_id.as_ref() == Some(&instance.id);
            let active_marker = if is_active { "*" } else { " " };

            let status_style = match instance.status {
                InstanceStatus::Active => Style::new().fg(Color::Green),
                InstanceStatus::Ready => Style::new().fg(Color::Cyan),
                InstanceStatus::Syncing | InstanceStatus::Rebuilding => {
                    Style::new().fg(Color::Yellow)
                }
                InstanceStatus::Failed => Style::new().fg(Color::Red),
                _ => Style::new(),
            };

            Row::new(vec![
                Cell::from(active_marker).style(Style::new().fg(Color::Green)),
                Cell::from(instance.name.clone()),
                Cell::from(instance.network.clone()),
                Cell::from(format!("{:?}", instance.status)).style(status_style),
                Cell::from(format_number(instance.current_block)),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(1),
        Constraint::Percentage(30),
        Constraint::Percentage(15),
        Constraint::Percentage(15),
        Constraint::Percentage(20),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::bordered().title("Select Instance for Job (* = active)"))
        .row_highlight_style(
            Style::new()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    frame.render_stateful_widget(table, instance_area, &mut app.integrity_table_state);

    // Available jobs section
    let job_items = vec![
        ListItem::new(Line::from(vec![
            Span::styled("[1] ", Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled("Fix Missing Cycles", Style::new().fg(Color::White)),
            Span::raw(" - Requires indexer running on this instance"),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("[2] ", Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled("Update UDT Labels", Style::new().fg(Color::White)),
            Span::raw(" - Works on any instance"),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("[3] ", Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled("Update Script Labels", Style::new().fg(Color::White)),
            Span::raw(" - Works on any instance"),
        ])),
    ];

    let jobs_list =
        List::new(job_items).block(Block::bordered().title("Available Jobs (press number to run)"));
    frame.render_widget(jobs_list, jobs_area);

    // Help section
    let help_line = Line::from(vec![
        Span::styled("j/k: ", Style::new().fg(Color::Cyan)),
        Span::raw("navigate  "),
        Span::styled("1-3: ", Style::new().fg(Color::Cyan)),
        Span::raw("trigger job  "),
        Span::styled("r: ", Style::new().fg(Color::Cyan)),
        Span::raw("refresh"),
    ]);

    let help_para = Paragraph::new(help_line).block(Block::bordered().title("Controls"));
    frame.render_widget(help_para, help_area);
}

fn render_config(frame: &mut Frame, area: Rect, app: &App) {
    let [config_area, progress_area, help_area] = Layout::vertical([
        Constraint::Length(14),
        Constraint::Fill(1),
        Constraint::Length(5),
    ])
    .areas(area);

    let config_text = vec![
        Line::from(vec![
            Span::styled("Sync Configuration", Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Default Batch Size: ", Style::new().fg(Color::Cyan)),
            Span::raw("3000 blocks"),
        ]),
        Line::from(vec![
            Span::styled("Parallel Fetch: ", Style::new().fg(Color::Cyan)),
            Span::raw("64 concurrent requests"),
        ]),
        Line::from(vec![
            Span::styled("Pipeline Buffer: ", Style::new().fg(Color::Cyan)),
            Span::raw("4 batches"),
        ]),
        Line::from(vec![
            Span::styled("Bulk Sync Mode: ", Style::new().fg(Color::Cyan)),
            Span::raw("Enabled (skips derived tables during initial sync)"),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Instances: ", Style::new().fg(Color::Cyan)),
            Span::raw(format!("{}", app.instances.len())),
        ]),
        Line::from(vec![
            Span::styled("Active Jobs: ", Style::new().fg(Color::Cyan)),
            Span::raw(format!("{}", app.jobs.len())),
        ]),
    ];

    let config_para = Paragraph::new(config_text)
        .block(Block::bordered().title("Configuration"));
    frame.render_widget(config_para, config_area);

    let mut progress_lines = vec![
        Line::from(vec![
            Span::styled("Active Instance Progress", Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
    ];

    if let Some(instance) = app.get_active_instance() {
        let phase_str = match &instance.sync_phase {
            SyncPhase::Pending => "Pending".to_string(),
            SyncPhase::CoreSync => "Core Sync".to_string(),
            SyncPhase::RebuildCellStatus => "Rebuild: cell_status".to_string(),
            SyncPhase::RebuildLiveCells => "Rebuild: live_cells".to_string(),
            SyncPhase::RebuildAddressBalances => "Rebuild: balances".to_string(),
            SyncPhase::RebuildScriptUsageStats => "Rebuild: scripts".to_string(),
            SyncPhase::RebuildDaoDeposits => "Rebuild: dao".to_string(),
            SyncPhase::RebuildUdtCells => "Rebuild: udt".to_string(),
            SyncPhase::RebuildDailyStatistics => "Rebuild: daily".to_string(),
            SyncPhase::RebuildHourlyStatistics => "Rebuild: hourly".to_string(),
            SyncPhase::RebuildEpochStatistics => "Rebuild: epoch".to_string(),
            SyncPhase::RebuildMinerStatistics => "Rebuild: miner".to_string(),
            SyncPhase::RebuildIndexes => "Rebuild: indexes".to_string(),
            SyncPhase::Completed => "Completed".to_string(),
        };

        progress_lines.push(Line::from(vec![
            Span::styled("Instance: ", Style::new().fg(Color::Cyan)),
            Span::raw(format!("{} ({})", instance.name, instance.network)),
        ]));
        progress_lines.push(Line::from(vec![
            Span::styled("Current Phase: ", Style::new().fg(Color::Cyan)),
            Span::raw(phase_str),
        ]));

        if instance.sync_phase != SyncPhase::Completed {
            let current_eta = calculate_phase_eta(instance, &app.jobs);
            progress_lines.push(Line::from(vec![
                Span::styled("Current Phase ETA: ", Style::new().fg(Color::Cyan)),
                Span::styled(
                    current_eta,
                    Style::new().fg(Color::Green).add_modifier(Modifier::BOLD),
                ),
            ]));

            if let Some(total_secs) = calculate_total_eta(instance, &app.jobs) {
                progress_lines.push(Line::from(vec![
                    Span::styled("Estimated Completion: ", Style::new().fg(Color::Cyan)),
                    Span::styled(
                        format_duration(total_secs),
                        Style::new().fg(Color::Green).add_modifier(Modifier::BOLD),
                    ),
                ]));
            }
        }

        progress_lines.push(Line::from(""));

        let instance_jobs: Vec<_> = app.jobs.iter().filter(|j| j.instance_id == instance.id).collect();
        if !instance_jobs.is_empty() {
            progress_lines.push(Line::from(vec![
                Span::styled("Running Jobs:", Style::new().fg(Color::Yellow)),
            ]));
            for job in instance_jobs {
                let job_eta = calculate_job_eta(job);
                let progress_str = job
                    .progress_percent
                    .map(|p| format!("{:.1}%", p))
                    .unwrap_or_else(|| "-".to_string());
                progress_lines.push(Line::from(format!(
                    "  - {} ({}): {} | ETA: {}",
                    job.job_type, job.status, progress_str, job_eta
                )));
            }
        }
    } else {
        progress_lines.push(Line::from(vec![
            Span::styled("No active instance", Style::new().fg(Color::DarkGray)),
        ]));
    }

    let progress_para = Paragraph::new(progress_lines)
        .block(Block::bordered().title("Progress & ETA"));
    frame.render_widget(progress_para, progress_area);

    let help_text = vec![
        Line::from(vec![
            Span::styled("Two-Phase Sync Architecture", Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]),
        Line::from("Phase 1: Core blockchain sync (blocks, txs, cells) - fast"),
        Line::from("Phase 2: Rebuild derived tables (balances, stats) - parallel"),
        Line::from("Expected speedup: 10-15x faster than traditional sync"),
    ];

    let help_para = Paragraph::new(help_text)
        .block(Block::bordered().title("Info"));
    frame.render_widget(help_para, help_area);
}

fn render_status(frame: &mut Frame, area: Rect, app: &App) {
    let elapsed_ms = app.last_refresh.elapsed().as_millis();
    let refresh_text = format!("Last refresh: {}ms ago", elapsed_ms);

    let db_status = if app.db_connected {
        Span::styled(" ● DB", Style::new().fg(Color::Green))
    } else {
        Span::styled(" ○ DB", Style::new().fg(Color::Red))
    };

    let status = if let Some((msg, color)) = &app.status_message {
        Line::from(vec![
            Span::styled(msg.as_str(), Style::new().fg(*color)),
            Span::raw(" | "),
            Span::raw(refresh_text),
            db_status,
        ])
    } else {
        Line::from(vec![
            Span::raw(refresh_text),
            db_status,
        ])
    };

    frame.render_widget(Paragraph::new(status), area);
}

fn render_help(frame: &mut Frame, area: Rect, app: &App) {
    let help_text = match app.current_tab {
        Tab::Instances => "q:Quit  Tab/←→:Switch tabs  j/k/↑↓:Navigate  a:Activate  r:Refresh",
        Tab::Jobs => "q:Quit  Tab/←→:Switch tabs  j/k/↑↓:Navigate  c:Cancel job  r:Refresh",
        Tab::Events => "q:Quit  Tab/←→:Switch tabs  r:Refresh",
        Tab::Integrity => "q:Quit  Tab:Switch  j/k:Select instance  1:Fix Cycles  2:UDT Labels  3:Script Labels  r:Refresh",
        Tab::Config => "q:Quit  Tab/←→:Switch tabs  r:Refresh",
    };

    let help = Paragraph::new(help_text).style(Style::new().fg(Color::DarkGray));
    frame.render_widget(help, area);
}

fn truncate_error(e: &anyhow::Error) -> String {
    let s = e.to_string();
    if s.len() > 50 {
        format!("{}...", &s[..50])
    } else {
        s
    }
}

fn format_number(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn format_eta(remaining: i64, speed: Option<f64>) -> String {
    match speed {
        Some(s) if s > 0.0 && remaining > 0 => {
            let secs = remaining as f64 / s;
            format_duration(secs)
        }
        _ if remaining <= 0 => "-".to_string(),
        _ => "-".to_string(),
    }
}

fn format_duration(secs: f64) -> String {
    if secs < 60.0 {
        format!("{:.0}s", secs)
    } else if secs < 3600.0 {
        let mins = secs / 60.0;
        format!("{:.0}m", mins)
    } else if secs < 86400.0 {
        let hours = (secs / 3600.0) as u64;
        let mins = ((secs % 3600.0) / 60.0) as u64;
        if mins > 0 {
            format!("{}h {}m", hours, mins)
        } else {
            format!("{}h", hours)
        }
    } else {
        let days = (secs / 86400.0) as u64;
        let hours = ((secs % 86400.0) / 3600.0) as u64;
        if hours > 0 {
            format!("{}d {}h", days, hours)
        } else {
            format!("{}d", days)
        }
    }
}

fn calculate_instance_eta(instance: &Instance) -> String {
    match (instance.target_block, instance.sync_speed) {
        (Some(target), Some(speed)) if speed > 0.0 && target > instance.current_block => {
            let remaining = target - instance.current_block;
            format_eta(remaining, Some(speed))
        }
        _ => "-".to_string(),
    }
}

fn calculate_job_eta(job: &SyncJob) -> String {
    match (job.progress_total, job.rows_per_second) {
        (Some(total), Some(speed)) if speed > 0.0 && total > job.progress_current => {
            let remaining = total - job.progress_current;
            format_eta(remaining, Some(speed))
        }
        _ => "-".to_string(),
    }
}

fn find_job_for_phase<'a>(jobs: &'a [SyncJob], instance_id: &uuid::Uuid, phase: &SyncPhase) -> Option<&'a SyncJob> {
    let job_type = match phase {
        SyncPhase::CoreSync => "core_sync",
        SyncPhase::RebuildCellStatus => "rebuild_cell_status",
        SyncPhase::RebuildLiveCells => "rebuild_live_cells",
        SyncPhase::RebuildAddressBalances => "rebuild_address_balances",
        SyncPhase::RebuildScriptUsageStats => "rebuild_script_usage_stats",
        SyncPhase::RebuildDaoDeposits => "rebuild_dao_deposits",
        SyncPhase::RebuildUdtCells => "rebuild_udt_cells",
        SyncPhase::RebuildDailyStatistics => "rebuild_daily_statistics",
        SyncPhase::RebuildHourlyStatistics => "rebuild_hourly_statistics",
        SyncPhase::RebuildEpochStatistics => "rebuild_epoch_statistics",
        SyncPhase::RebuildMinerStatistics => "rebuild_miner_statistics",
        SyncPhase::RebuildIndexes => "rebuild_indexes",
        _ => return None,
    };
    jobs.iter().find(|j| &j.instance_id == instance_id && j.job_type == job_type)
}

fn calculate_phase_eta(instance: &Instance, jobs: &[SyncJob]) -> String {
    match instance.sync_phase {
        SyncPhase::CoreSync => calculate_instance_eta(instance),
        SyncPhase::Completed | SyncPhase::Pending => "-".to_string(),
        _ => {
            if let Some(job) = find_job_for_phase(jobs, &instance.id, &instance.sync_phase) {
                calculate_job_eta(job)
            } else {
                "-".to_string()
            }
        }
    }
}

fn calculate_total_eta(instance: &Instance, jobs: &[SyncJob]) -> Option<f64> {
    let mut total_secs = 0.0;
    
    match instance.sync_phase {
        SyncPhase::Completed => return None,
        SyncPhase::CoreSync => {
            if let (Some(target), Some(speed)) = (instance.target_block, instance.sync_speed) {
                if speed > 0.0 && target > instance.current_block {
                    total_secs += (target - instance.current_block) as f64 / speed;
                }
            }
        }
        _ => {
            if let Some(job) = find_job_for_phase(jobs, &instance.id, &instance.sync_phase) {
                if let (Some(total), Some(speed)) = (job.progress_total, job.rows_per_second) {
                    if speed > 0.0 && total > job.progress_current {
                        total_secs += (total - job.progress_current) as f64 / speed;
                    }
                }
            }
        }
    }
    
    if total_secs > 0.0 {
        Some(total_secs)
    } else {
        None
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let args = Args::parse();

    let mut app = App::new(&args.control_db_url).await?;

    let mut terminal = ratatui::init();

    loop {
        terminal.draw(|frame| render(frame, &mut app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                let action = app.handle_key(key);

                if app.should_quit {
                    break;
                }

                if let Some(action) = action {
                    let result = match action {
                        AsyncAction::ActivateInstance => app.activate_selected_instance().await,
                        AsyncAction::CancelJob => app.cancel_selected_job().await,
                        AsyncAction::Refresh => {
                            app.refresh().await.map(|_| {
                                app.status_message = Some(("Refreshed".to_string(), Color::Green));
                            })
                        }
                        AsyncAction::TriggerCyclesFix => {
                            app.trigger_integrity_job("fix_missing_cycles").await
                        }
                        AsyncAction::TriggerUdtLabels => {
                            app.trigger_integrity_job("update_udt_labels").await
                        }
                        AsyncAction::TriggerScriptLabels => {
                            app.trigger_integrity_job("update_script_labels").await
                        }
                    };

                    if let Err(e) = result {
                        app.status_message = Some((format!("Error: {}", e), Color::Red));
                    }
                }
            }
        }

        if app.last_refresh.elapsed() > Duration::from_secs(1) {
            match tokio::time::timeout(Duration::from_secs(3), app.refresh()).await {
                Ok(Ok(())) => {
                    app.db_connected = true;
                }
                Ok(Err(e)) => {
                    app.db_connected = false;
                    app.status_message =
                        Some((format!("DB error: {}", truncate_error(&e)), Color::Red));
                    app.last_refresh = std::time::Instant::now();
                }
                Err(_) => {
                    app.db_connected = false;
                    app.status_message =
                        Some(("DB connection timeout".to_string(), Color::Red));
                    app.last_refresh = std::time::Instant::now();
                }
            }
        }
    }

    ratatui::restore();
    Ok(())
}
