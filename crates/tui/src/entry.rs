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
    use ckbadger_store::StoreRuntimeConfig;

    #[test]
    fn test_tui_service_config_fields() {
        let config = TuiServiceConfig {
            networks: vec![TuiNetwork {
                name: "mainnet".to_string(),
                domain_data_path: "/data/domain".to_string(),
                append_only_data_path: "/data/append".to_string(),
                ckbadger_workdir: "/workdir/ckbadger".to_string(),
                ckb_workdir: "/workdir/ckb".to_string(),
                ckb_db_path: "/workdir/ckb/data/db".to_string(),
                api_url: "http://localhost:3001/api/v1".to_string(),
                store_runtime_config: StoreRuntimeConfig::default(),
            }],
            refresh_ms: 500,
            supervisor_socket_path: Some("/run/indexer.sock".to_string()),
            service_log_dir: Some("/run/logs".to_string()),
            build_version: "0.1.0@abc123".to_string(),
        };

        assert_eq!(config.networks.len(), 1);
        assert_eq!(config.networks[0].name, "mainnet");
        assert_eq!(config.networks[0].domain_data_path, "/data/domain");
        assert_eq!(config.networks[0].api_url, "http://localhost:3001/api/v1");
        assert_eq!(config.refresh_ms, 500);
        assert_eq!(
            config.supervisor_socket_path.as_deref(),
            Some("/run/indexer.sock")
        );
        assert_eq!(config.service_log_dir.as_deref(), Some("/run/logs"));
    }
}
