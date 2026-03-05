use anyhow::Result;
use clap::Parser;

use ckbadger_tui::entry::{self, TuiServiceConfig};

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

    entry::run_tui(TuiServiceConfig {
        domain_data_path,
        append_only_data_path,
        api_url: args.api_url,
        refresh_ms: args.refresh_ms,
        redis_url: args.redis_url,
    })
    .await
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
