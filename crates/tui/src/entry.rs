use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::Duration;

use crate::db::TuiDb;
use crate::ui::{self, App};

/// Configuration for starting the TUI.
/// This is the interface the CLI binary uses to start the TUI.
pub struct TuiServiceConfig {
    pub domain_data_path: String,
    pub append_only_data_path: String,
    pub api_url: String,
    pub refresh_ms: u64,
    pub supervisor_socket_path: Option<String>,
    pub service_log_dir: Option<String>,
}

/// Run the TUI. Blocks until user exits.
pub async fn run_tui(config: TuiServiceConfig) -> Result<()> {
    let db = TuiDb::new_with_monitoring(
        &config.api_url,
        &config.domain_data_path,
        &config.append_only_data_path,
        config.supervisor_socket_path.as_deref(),
        config.service_log_dir.as_deref(),
    )
    .await;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(db);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tui_service_config_fields() {
        let config = TuiServiceConfig {
            domain_data_path: "/data/domain".to_string(),
            append_only_data_path: "/data/append".to_string(),
            api_url: "http://localhost:3001/api/v1".to_string(),
            refresh_ms: 500,
            supervisor_socket_path: Some("/run/indexer.sock".to_string()),
            service_log_dir: Some("/run/logs".to_string()),
        };

        assert_eq!(config.domain_data_path, "/data/domain");
        assert_eq!(config.append_only_data_path, "/data/append");
        assert_eq!(config.api_url, "http://localhost:3001/api/v1");
        assert_eq!(config.refresh_ms, 500);
        assert_eq!(
            config.supervisor_socket_path.as_deref(),
            Some("/run/indexer.sock")
        );
        assert_eq!(config.service_log_dir.as_deref(), Some("/run/logs"));
    }
}
