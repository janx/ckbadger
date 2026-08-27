use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::Duration;

use crate::multi::{MultiNetworkDb, TuiNetwork};
use crate::ui::{self, App};

/// Configuration for starting the TUI.
/// This is the interface the CLI binary uses to start the TUI.
#[derive(Debug)]
pub struct TuiServiceConfig {
    pub networks: Vec<TuiNetwork>,
    pub refresh_ms: u64,
    pub supervisor_socket_path: Option<String>,
    pub service_log_dir: Option<String>,
    pub build_version: String,
}

/// Run the TUI. Blocks until user exits.
pub async fn run_tui(config: TuiServiceConfig) -> Result<()> {
    let db = MultiNetworkDb::new(
        config.networks,
        config.supervisor_socket_path,
        config.service_log_dir,
    )
    .await;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(db, config.build_version);
    let tick_rate = Duration::from_millis(config.refresh_ms);
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
                        KeyCode::Char('e') => app.toggle_build_subphases(),
                        KeyCode::Char(']') => app.select_next_network().await,
                        KeyCode::Char('[') => app.select_prev_network().await,
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
