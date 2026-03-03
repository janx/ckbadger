use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::Duration;

mod chart;
mod db;
mod ui;

use db::TuiDb;
use ui::App;

#[derive(Parser, Debug)]
#[command(name = "ckbadger-tui")]
#[command(about = "Terminal UI for ckbadger sync and memory monitoring")]
struct Args {
    #[arg(long = "domain-data-path", env = "CKBADGER_DOMAIN_DATA_PATH")]
    domain_data_path: Option<String>,

    #[arg(long = "append-only-data-path", env = "CKBADGER_APPEND_ONLY_DATA_PATH")]
    append_only_data_path: Option<String>,

    #[arg(long, env = "REDIS_URL")]
    redis_url: Option<String>,

    #[arg(long, env = "API_URL", default_value = "http://localhost:3001/api/v1")]
    api_url: String,

    #[arg(long, default_value = "1000")]
    refresh_ms: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    let args = Args::parse();
    let domain_data_path = resolve_domain_data_path(
        args.domain_data_path,
        std::env::var("CKBADGER_DOMAIN_DATA_PATH").ok(),
    );
    let append_only_data_path = resolve_append_only_data_path(
        args.append_only_data_path,
        std::env::var("CKBADGER_APPEND_ONLY_DATA_PATH").ok(),
        &domain_data_path,
    );

    let db = TuiDb::new(
        args.redis_url.as_deref(),
        &args.api_url,
        &domain_data_path,
        &append_only_data_path,
    )
    .await;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(db);
    let tick_rate = Duration::from_millis(args.refresh_ms);
    let res = run_app(&mut terminal, &mut app, tick_rate).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("Error: {err:?}");
    }

    Ok(())
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    refresh_interval: Duration,
) -> Result<()> {
    app.refresh().await;

    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if app.is_help_visible() {
                        match key.code {
                            KeyCode::Char('q') => return Ok(()),
                            KeyCode::Char('?') | KeyCode::Esc | KeyCode::Enter => {
                                app.close_help();
                            }
                            _ => {}
                        }
                        continue;
                    }

                    match key.code {
                        KeyCode::Char('q') => return Ok(()),
                        KeyCode::Char('?') => app.toggle_help(),
                        KeyCode::Char('j') | KeyCode::Down => app.scroll_log_up(),
                        KeyCode::Char('k') | KeyCode::Up => app.scroll_log_down(),
                        KeyCode::Char('g') | KeyCode::Home => app.scroll_log_to_top(),
                        KeyCode::Char('G') | KeyCode::End => app.scroll_log_to_bottom(),
                        KeyCode::Tab | KeyCode::Char('s') | KeyCode::Char('l') | KeyCode::Right => {
                            app.next_tab()
                        }
                        KeyCode::Char('h') | KeyCode::Left => app.previous_tab(),
                        KeyCode::Char('c') => app.toggle_compact_layout(),
                        KeyCode::Char('v') => app.cycle_diagnostics_view_mode(),
                        KeyCode::Char('R') => app.refresh().await,
                        _ => {}
                    }
                }
            }
        }

        if app.should_refresh(refresh_interval) {
            app.refresh().await;
        }
    }
}

fn resolve_domain_data_path(explicit: Option<String>, domain_env: Option<String>) -> String {
    explicit
        .or(domain_env)
        .unwrap_or_else(|| "./data/ckbadger-store".to_string())
}

fn resolve_append_only_data_path(
    explicit: Option<String>,
    append_env: Option<String>,
    domain_data_path: &str,
) -> String {
    explicit
        .or(append_env)
        .unwrap_or_else(|| format!("{domain_data_path}-append-only"))
}

#[cfg(test)]
mod tests {
    use super::{resolve_append_only_data_path, resolve_domain_data_path};

    #[test]
    fn test_resolve_domain_data_path() {
        assert_eq!(
            resolve_domain_data_path(
                Some("/explicit/domain".to_string()),
                Some("/env/domain".to_string()),
            ),
            "/explicit/domain"
        );
        assert_eq!(
            resolve_domain_data_path(None, Some("/env/domain".to_string())),
            "/env/domain"
        );
        assert_eq!(
            resolve_domain_data_path(None, None),
            "./data/ckbadger-store"
        );
    }

    #[test]
    fn test_resolve_append_only_data_path() {
        assert_eq!(
            resolve_append_only_data_path(
                Some("/explicit/append".to_string()),
                Some("/env/append".to_string()),
                "/domain/path",
            ),
            "/explicit/append"
        );
        assert_eq!(
            resolve_append_only_data_path(None, Some("/env/append".to_string()), "/domain/path",),
            "/env/append"
        );
        assert_eq!(
            resolve_append_only_data_path(None, None, "/domain/path"),
            "/domain/path-append-only"
        );
    }
}
