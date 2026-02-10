use anyhow::Result;
use ckbadger_store::CkbadgerStore;
use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use ratatui_image::picker::Picker;
use std::io;
use std::sync::Arc;
use std::time::Duration;

mod chart;
mod db;
mod ui;

use db::TaskDb;
use ui::{App, FocusedPanel};

#[derive(Parser, Debug)]
#[command(name = "ckbadger-task-tui")]
#[command(about = "Terminal UI for ckbadger task management")]
struct Args {
    #[arg(
        long,
        env = "CKBADGER_DATA_PATH",
        default_value = "./data/ckbadger-store"
    )]
    data_path: String,

    #[arg(long, env = "REDIS_URL")]
    redis_url: Option<String>,

    #[arg(long, default_value = "1000")]
    refresh_ms: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    let args = Args::parse();

    let store = Arc::new(CkbadgerStore::open(&args.data_path)?);

    let db = TaskDb::new(store, args.redis_url.as_deref()).await;

    let picker = Picker::from_query_stdio().ok();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(db, picker);
    let res = run_app(
        &mut terminal,
        &mut app,
        Duration::from_millis(args.refresh_ms),
    )
    .await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("Error: {:?}", err);
    }

    Ok(())
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    tick_rate: Duration,
) -> Result<()> {
    app.refresh().await?;

    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        if crossterm::event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if app.has_dialog() {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => app.cancel_dialog(),
                            KeyCode::Char('j') | KeyCode::Down | KeyCode::Tab => {
                                app.next_dialog_option()
                            }
                            KeyCode::Char('k') | KeyCode::Up => app.previous_dialog_option(),
                            KeyCode::Enter => app.confirm_dialog().await?,
                            _ => {}
                        }
                    } else {
                        match key.code {
                            KeyCode::Char('q') => return Ok(()),
                            KeyCode::Tab => app.toggle_focus(),
                            KeyCode::Char('j') | KeyCode::Down => match app.focused_panel() {
                                FocusedPanel::Tasks => app.next(),
                                FocusedPanel::Details => app.scroll_detail_down(),
                                FocusedPanel::Log => app.scroll_log_down(),
                            },
                            KeyCode::Char('k') | KeyCode::Up => match app.focused_panel() {
                                FocusedPanel::Tasks => app.previous(),
                                FocusedPanel::Details => app.scroll_detail_up(),
                                FocusedPanel::Log => app.scroll_log_up(),
                            },
                            KeyCode::Char('g') | KeyCode::End => app.scroll_log_to_bottom(),
                            KeyCode::Char('G') | KeyCode::Home => app.scroll_log_to_top(),
                            KeyCode::Char('n') => app.show_new_task_dialog(),
                            KeyCode::Char('c') => app.cancel_selected().await?,
                            KeyCode::Char('p') => app.pause_selected().await?,
                            KeyCode::Char('r') => app.resume_or_retry_selected().await?,
                            KeyCode::Char('d') => app.delete_selected().await?,
                            KeyCode::Char('v') => app.toggle_chart_mode(),
                            KeyCode::Char('R') => app.refresh().await?,
                            KeyCode::Enter => app.confirm_dialog().await?,
                            KeyCode::Esc => app.cancel_dialog(),
                            _ => {}
                        }
                    }
                }
            }
        }

        if app.should_refresh() {
            app.refresh().await?;
        }
    }
}
