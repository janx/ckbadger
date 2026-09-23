//! `ckbadger verify` exit-code and report-envelope contract.
//!
//! The verify runner classifies every check as pass / fail / inconclusive /
//! skipped / not-applicable / error; this test pins how those merge into the
//! process exit code across every selected network, and that a narrowed run is
//! reported as narrowed rather than as a complete preset.
//!
//! `Fail (1) > Error/Inconclusive (2) > Pass (0)`, and every selected network
//! produces a report even when an earlier one already failed.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener};
use std::path::Path;
use std::process::Command;

/// A fixed-response HTTP server standing in for one network's ckbadger API.
struct FakeApi {
    port: u16,
}

impl FakeApi {
    /// Serve `body` (a JSON document) for every request, forever.
    fn serving(body: &'static str) -> Self {
        let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                // Drain the request head so the client sees a complete exchange.
                while reader.read_line(&mut line).unwrap_or(0) > 0 {
                    if line == "\r\n" || line == "\n" {
                        break;
                    }
                    line.clear();
                }
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        Self { port }
    }
}

/// A port nothing listens on, so the API is genuinely unreachable.
fn closed_port() -> u16 {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

fn network_stats(latest_block: i64) -> String {
    format!(
        r#"{{"latestBlock":{latest_block},"syncStatus":{{"isSyncing":false,"syncedBlock":{latest_block},"tipBlock":{latest_block}}},"deepForkStatus":{{"detected":false}}}}"#
    )
}

fn config_toml(network: &str, api_port: u16) -> String {
    format!(
        r#"[ckb]
rpc_url = "http://127.0.0.1:1"
network = "{network}"
workdir = ""

[api]
host = "127.0.0.1"
port = {api_port}
rate_limit = 100
rate_limit_burst = 200
slow_request_threshold_ms = 100

[frontend]
host = "127.0.0.1"
port = 8100

[indexer]
bulk_sync_threshold = 1000
poll_interval_ms = 1000

[store]
domain_data_path = "data/domain"
append_only_data_path = "data/append-only"
network_data_path = "data/network"
direct_io_reads = true

[crawler]
enabled = false

[log]
level = "error"
"#
    )
}

/// Scaffold an orchestrator root with one workdir per `(network, api_port)`.
fn scaffold(root: &Path, networks: &[(&str, u16)]) {
    let mut orchestrator = String::new();
    for (name, _) in networks {
        orchestrator.push_str(&format!("[[network]]\nname = \"{name}\"\n\n"));
    }
    orchestrator
        .push_str("[frontend]\nhost = \"127.0.0.1\"\nport = 8100\n\n[log]\nlevel = \"error\"\n");
    std::fs::write(root.join("ckbadger.toml"), orchestrator).unwrap();

    for (name, port) in networks {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.toml"), config_toml(name, *port)).unwrap();
    }
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn verify(root: &Path, args: &[&str]) -> Run {
    let output = Command::new(env!("CARGO_BIN_EXE_ckbadger"))
        .arg("-C")
        .arg(root)
        .arg("verify")
        .args(args)
        .output()
        .expect("running the ckbadger binary");
    Run {
        code: output.status.code().expect("verify must exit, not signal"),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn envelope(run: &Run) -> serde_json::Value {
    serde_json::from_str(&run.stdout).unwrap_or_else(|error| {
        panic!(
            "--format json stdout must be one parseable JSON document ({error}):\n{}",
            run.stdout
        )
    })
}

#[test]
fn a_failing_network_and_an_unreachable_network_merge_to_exit_1() {
    let root = tempfile::tempdir().unwrap();
    // latestBlock = 0 is exactly what `api_reachable` fails on.
    let failing = FakeApi::serving(Box::leak(network_stats(0).into_boxed_str()));
    scaffold(
        root.path(),
        &[("mainnet", failing.port), ("testnet", closed_port())],
    );

    let run = verify(
        root.path(),
        &[
            "--depth",
            "fast",
            "--checks",
            "api_reachable",
            "--no-explorer",
            "--format",
            "json",
        ],
    );

    assert_eq!(
        run.code, 1,
        "a proven inconsistency outranks an unreachable network\nstderr: {}",
        run.stderr
    );

    let json = envelope(&run);
    assert_eq!(json["schemaVersion"], 1);
    assert_eq!(json["status"], "fail");
    assert_eq!(json["scopeComplete"], false);

    let networks = json["networks"].as_array().unwrap();
    assert_eq!(
        networks.len(),
        2,
        "every selected network must produce a report, not just the first"
    );
    assert_eq!(networks[0]["network"], "mainnet");
    assert_eq!(networks[0]["status"], "fail");
    assert_eq!(networks[1]["network"], "testnet");
    assert_eq!(
        networks[1]["status"], "error",
        "an unreachable API is an error, never a data failure"
    );
}

#[test]
fn an_explicitly_narrowed_run_passes_with_exit_0_and_lists_what_it_excluded() {
    let root = tempfile::tempdir().unwrap();
    let healthy = FakeApi::serving(Box::leak(network_stats(12_345).into_boxed_str()));
    scaffold(root.path(), &[("mainnet", healthy.port)]);

    let run = verify(
        root.path(),
        &[
            "--depth",
            "fast",
            "--checks",
            "api_reachable",
            "--no-explorer",
            "--format",
            "json",
        ],
    );

    assert_eq!(run.code, 0, "stderr: {}", run.stderr);

    let json = envelope(&run);
    assert_eq!(json["status"], "pass");
    assert_eq!(
        json["scopeComplete"], false,
        "a narrowed run must not be reported as a complete preset"
    );

    let checks = json["networks"][0]["checks"].as_array().unwrap();
    let selected = checks
        .iter()
        .find(|c| c["name"] == "api_reachable")
        .expect("the selected check must be reported");
    assert_eq!(selected["status"], "pass");

    let excluded: Vec<&serde_json::Value> =
        checks.iter().filter(|c| c["status"] == "skipped").collect();
    assert!(
        !excluded.is_empty(),
        "checks excluded by --checks must still be listed: {checks:#?}"
    );
    assert!(
        excluded.iter().all(|c| c["statusReason"]
            .as_str()
            .unwrap_or_default()
            .contains("--checks")),
        "every exclusion needs an auditable reason: {excluded:#?}"
    );
}

#[test]
fn zero_samples_on_a_sampling_check_exits_2() {
    let root = tempfile::tempdir().unwrap();
    let healthy = FakeApi::serving(Box::leak(network_stats(12_345).into_boxed_str()));
    scaffold(root.path(), &[("mainnet", healthy.port)]);

    let run = verify(
        root.path(),
        &[
            "--depth",
            "sampling",
            "--checks",
            "block_hash_roundtrip",
            "--no-explorer",
            "--sample-count",
            "0",
            "--format",
            "json",
        ],
    );

    assert_eq!(
        run.code, 2,
        "sample_count=0 is a request error, not a pass\nstderr: {}",
        run.stderr
    );
    let json = envelope(&run);
    assert_eq!(json["status"], "error");
    let checks = json["networks"][0]["checks"].as_array().unwrap();
    let selected = checks
        .iter()
        .find(|c| c["name"] == "block_hash_roundtrip")
        .unwrap();
    assert_eq!(selected["status"], "error");
    assert!(selected["statusReason"]
        .as_str()
        .unwrap_or_default()
        .contains("sample-count"));
}

#[test]
fn each_network_persists_its_own_report_under_the_shared_run_id() {
    let root = tempfile::tempdir().unwrap();
    let mainnet = FakeApi::serving(Box::leak(network_stats(12_345).into_boxed_str()));
    let testnet = FakeApi::serving(Box::leak(network_stats(6_789).into_boxed_str()));
    scaffold(
        root.path(),
        &[("mainnet", mainnet.port), ("testnet", testnet.port)],
    );

    let run = verify(
        root.path(),
        &[
            "--depth",
            "fast",
            "--checks",
            "api_reachable",
            "--no-explorer",
            "--format",
            "json",
        ],
    );
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);

    let json = envelope(&run);
    let run_id = json["runId"].as_str().expect("runId").to_string();
    assert!(!run_id.is_empty());

    for network in ["mainnet", "testnet"] {
        let path = root
            .path()
            .join(network)
            .join("perf")
            .join("verify")
            .join(&run_id)
            .join("report.json");
        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| {
                panic!("{} should hold this network's report: {e}", path.display())
            }))
            .unwrap();
        assert_eq!(written["network"], network);
        assert_eq!(written["runId"], run_id);
        assert_eq!(written["schemaVersion"], 1);
    }
}
