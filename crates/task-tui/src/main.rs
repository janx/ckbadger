use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use sqlx::postgres::PgPoolOptions;
use std::io;
use std::time::Duration;

mod db;
mod ui;

use db::TaskDb;
use ui::App;

#[derive(Parser, Debug)]
#[command(name = "ckbadger-task-tui")]
#[command(about = "Terminal UI for ckbadger task management")]
struct Args {
    #[arg(long, env = "DATABASE_URL")]
    database_url: String,

    #[arg(long, default_value = "1000")]
    refresh_ms: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    let args = Args::parse();

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&args.database_url)
        .await?;

    let db = TaskDb::new(pool);

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(db);
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
                    match key.code {
                        KeyCode::Char('q') => return Ok(()),
                        KeyCode::Char('j') | KeyCode::Down => app.next(),
                        KeyCode::Char('k') | KeyCode::Up => app.previous(),
                        KeyCode::Char('n') => app.show_new_task_dialog(),
                        KeyCode::Char('c') => app.cancel_selected().await?,
                        KeyCode::Char('p') => app.pause_selected().await?,
                        KeyCode::Char('r') => app.resume_or_retry_selected().await?,
                        KeyCode::Char('d') => app.delete_selected().await?,
                        KeyCode::Char('R') => app.refresh().await?,
                        KeyCode::Enter => app.confirm_dialog().await?,
                        KeyCode::Esc => app.cancel_dialog(),
                        KeyCode::Tab => app.next_dialog_option(),
                        _ => {}
                    }
                }
            }
        }

        if app.should_refresh() {
            app.refresh().await?;
        }
    }
}
