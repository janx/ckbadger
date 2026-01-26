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
    Config,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::Instances, Tab::Jobs, Tab::Events, Tab::Config];

    fn title(&self) -> &'static str {
        match self {
            Tab::Instances => "Instances",
            Tab::Jobs => "Jobs",
            Tab::Events => "Events",
            Tab::Config => "Config",
        }
    }

    fn index(&self) -> usize {
        match self {
            Tab::Instances => 0,
            Tab::Jobs => 1,
            Tab::Events => 2,
            Tab::Config => 3,
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
    should_quit: bool,
    last_refresh: std::time::Instant,
    status_message: Option<(String, Color)>,
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

        Ok(Self {
            control_plane,
            current_tab: Tab::Instances,
            instances,
            jobs,
            events,
            active_instance_id,
            instance_table_state,
            job_table_state,
            should_quit: false,
            last_refresh: std::time::Instant::now(),
            status_message: None,
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
            SyncPhase::CoreSync => format!("Syncing: {} blocks", instance.current_block),
            other => format!("{:?}", other),
        };
        format!(
            "Active: {} ({}) | {} | {}",
            instance.name, instance.network, phase_str,
            instance.sync_speed.map(|s| format!("{:.0} blk/s", s)).unwrap_or_default()
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
                SyncPhase::RebuildLiveCells => "Rebuild: live_cells",
                SyncPhase::RebuildBalances => "Rebuild: balances",
                SyncPhase::RebuildScriptUsage => "Rebuild: scripts",
                SyncPhase::RebuildStatistics => "Rebuild: stats",
                SyncPhase::RebuildIndexes => "Rebuild: indexes",
                SyncPhase::RebuildAddressTx => "Rebuild: addr_tx",
                SyncPhase::Completed => "Completed",
            };

            Row::new(vec![
                Cell::from(active_marker).style(Style::new().fg(Color::Green)),
                Cell::from(instance.name.clone()),
                Cell::from(format!("{:?}", instance.status)).style(status_style),
                Cell::from(phase_str),
                Cell::from(progress),
                Cell::from(speed),
                Cell::from(instance.network.clone()),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(1),
        Constraint::Percentage(18),
        Constraint::Percentage(12),
        Constraint::Percentage(16),
        Constraint::Percentage(23),
        Constraint::Percentage(12),
        Constraint::Percentage(12),
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
            ])
        })
        .collect();

    let widths = [
        Constraint::Percentage(25),
        Constraint::Percentage(25),
        Constraint::Percentage(15),
        Constraint::Percentage(20),
        Constraint::Percentage(15),
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

fn render_config(frame: &mut Frame, area: Rect, app: &App) {
    let [info_area, help_area] = Layout::vertical([
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
            Span::styled("Tables skipped during bulk sync:", Style::new().fg(Color::Yellow)),
        ]),
        Line::from("  - live_cells"),
        Line::from("  - address_balances"),
        Line::from("  - address_transactions"),
        Line::from("  - script_usage_stats"),
        Line::from("  - hourly_statistics, daily_statistics"),
        Line::from("  - miner_statistics"),
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
    frame.render_widget(config_para, info_area);

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
    let elapsed = app.last_refresh.elapsed();
    let refresh_text = format!("Last refresh: {:.0}s ago", elapsed.as_secs_f64());

    let status = if let Some((msg, color)) = &app.status_message {
        Line::from(vec![
            Span::styled(msg.as_str(), Style::new().fg(*color)),
            Span::raw(" | "),
            Span::raw(refresh_text),
        ])
    } else {
        Line::from(refresh_text)
    };

    frame.render_widget(Paragraph::new(status), area);
}

fn render_help(frame: &mut Frame, area: Rect, app: &App) {
    let help_text = match app.current_tab {
        Tab::Instances => "q:Quit  Tab/←→:Switch tabs  j/k/↑↓:Navigate  a:Activate  r:Refresh",
        Tab::Jobs => "q:Quit  Tab/←→:Switch tabs  j/k/↑↓:Navigate  c:Cancel job  r:Refresh",
        Tab::Events => "q:Quit  Tab/←→:Switch tabs  r:Refresh",
        Tab::Config => "q:Quit  Tab/←→:Switch tabs  r:Refresh",
    };

    let help = Paragraph::new(help_text).style(Style::new().fg(Color::DarkGray));
    frame.render_widget(help, area);
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
                    };

                    if let Err(e) = result {
                        app.status_message = Some((format!("Error: {}", e), Color::Red));
                    }
                }
            }
        }

        if app.last_refresh.elapsed() > Duration::from_secs(5) {
            let _ = app.refresh().await;
        }
    }

    ratatui::restore();
    Ok(())
}
