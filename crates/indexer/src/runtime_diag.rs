use chrono::Utc;
use serde::Serialize;
use std::collections::VecDeque;
use std::fs;
use std::path::Path;
use std::sync::Mutex;

const CGROUP_ROOT: &str = "/sys/fs/cgroup";
const PROC_SELF_STATUS: &str = "/proc/self/status";

#[derive(Debug, Clone, Default, Serialize)]
pub struct CgroupMemorySnapshot {
    pub memory_current_bytes: Option<u64>,
    pub memory_max_bytes: Option<u64>,
    pub memory_max_raw: Option<String>,
    pub oom_events: Option<u64>,
    pub oom_kill_events: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ProcessMemorySnapshot {
    pub rss_bytes: u64,
    pub rss_anon_bytes: u64,
    pub swap_bytes: u64,
    pub high_water_rss_bytes: u64,
}

impl ProcessMemorySnapshot {
    pub fn committed_bytes(self) -> anyhow::Result<u64> {
        self.rss_bytes.checked_add(self.swap_bytes).ok_or_else(|| {
            anyhow::anyhow!(
                "process committed memory overflow: rss_bytes={} swap_bytes={}",
                self.rss_bytes,
                self.swap_bytes
            )
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FlightEvent {
    pub ts: i64,
    pub event: String,
    pub detail: String,
}

#[derive(Debug)]
pub struct FlightRecorder {
    capacity: usize,
    events: Mutex<VecDeque<FlightEvent>>,
}

impl FlightRecorder {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            events: Mutex::new(VecDeque::with_capacity(capacity.max(1))),
        }
    }

    pub fn record(&self, event: &str, detail: impl Into<String>) {
        let mut guard = self.events.lock().unwrap();
        guard.push_back(FlightEvent {
            ts: Utc::now().timestamp(),
            event: event.to_string(),
            detail: detail.into(),
        });
        while guard.len() > self.capacity {
            guard.pop_front();
        }
    }

    pub fn snapshot(&self) -> Vec<FlightEvent> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .cloned()
            .collect::<Vec<_>>()
    }
}

pub fn generate_run_id() -> String {
    format!(
        "run-{}-pid{}",
        Utc::now().format("%Y%m%dT%H%M%S%.3fZ"),
        std::process::id()
    )
}

pub fn generate_incident_id(run_id: &str, sequence: u64) -> String {
    format!("{}-inc-{:06}", run_id, sequence)
}

pub fn read_cgroup_memory_snapshot() -> CgroupMemorySnapshot {
    read_cgroup_memory_snapshot_from(Path::new(CGROUP_ROOT))
}

pub fn read_process_memory_snapshot() -> anyhow::Result<ProcessMemorySnapshot> {
    let content = fs::read_to_string(PROC_SELF_STATUS).map_err(|err| {
        anyhow::anyhow!(
            "failed to read process memory status {}: {}",
            PROC_SELF_STATUS,
            err
        )
    })?;
    parse_process_memory_status(&content)
}

fn parse_process_memory_status(content: &str) -> anyhow::Result<ProcessMemorySnapshot> {
    let read_field = |name: &str| -> anyhow::Result<u64> {
        let line = content
            .lines()
            .find(|line| line.starts_with(name))
            .ok_or_else(|| anyhow::anyhow!("process memory status missing field {}", name))?;
        let (_, raw) = line
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("malformed process memory field: {}", line))?;
        let mut parts = raw.split_whitespace();
        let kib = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("process memory field has no value: {}", line))?
            .parse::<u64>()
            .map_err(|err| anyhow::anyhow!("invalid process memory field {}: {}", line, err))?;
        let unit = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("process memory field has no unit: {}", line))?;
        if unit != "kB" {
            return Err(anyhow::anyhow!(
                "unsupported process memory field unit: field={} unit={}",
                name,
                unit
            ));
        }
        kib.checked_mul(1024).ok_or_else(|| {
            anyhow::anyhow!(
                "process memory field byte conversion overflow: field={} kib={}",
                name,
                kib
            )
        })
    };

    Ok(ProcessMemorySnapshot {
        rss_bytes: read_field("VmRSS")?,
        rss_anon_bytes: read_field("RssAnon")?,
        swap_bytes: read_field("VmSwap")?,
        high_water_rss_bytes: read_field("VmHWM")?,
    })
}

fn read_cgroup_memory_snapshot_from(root: &Path) -> CgroupMemorySnapshot {
    let memory_current_bytes = read_u64_file(&root.join("memory.current"));

    let memory_max_raw = read_trimmed(&root.join("memory.max"));
    let memory_max_bytes = memory_max_raw.as_deref().and_then(|value| {
        if value == "max" {
            None
        } else {
            value.parse().ok()
        }
    });

    let (oom_events, oom_kill_events) = read_memory_events(&root.join("memory.events"));

    CgroupMemorySnapshot {
        memory_current_bytes,
        memory_max_bytes,
        memory_max_raw,
        oom_events,
        oom_kill_events,
    }
}

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn read_u64_file(path: &Path) -> Option<u64> {
    read_trimmed(path).and_then(|value| value.parse::<u64>().ok())
}

fn read_memory_events(path: &Path) -> (Option<u64>, Option<u64>) {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) => return (None, None),
    };

    let mut oom_events = None;
    let mut oom_kill_events = None;

    for line in content.lines() {
        let mut parts = line.split_whitespace();
        let Some(key) = parts.next() else {
            continue;
        };
        let Some(value) = parts.next() else {
            continue;
        };
        let parsed = value.parse::<u64>().ok();
        match key {
            "oom" => oom_events = parsed,
            "oom_kill" => oom_kill_events = parsed,
            _ => {}
        }
    }

    (oom_events, oom_kill_events)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_run_and_incident_ids() {
        let run_id = generate_run_id();
        assert!(run_id.starts_with("run-"));
        assert!(run_id.contains("-pid"));

        let incident_id = generate_incident_id("run-abc", 42);
        assert_eq!(incident_id, "run-abc-inc-000042");
    }

    #[test]
    fn test_flight_recorder_eviction() {
        let recorder = FlightRecorder::new(2);
        recorder.record("event-1", "a");
        recorder.record("event-2", "b");
        recorder.record("event-3", "c");

        let snapshot = recorder.snapshot();
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[0].event, "event-2");
        assert_eq!(snapshot[1].event, "event-3");
    }

    #[test]
    fn test_read_cgroup_snapshot_from_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("memory.current"), "123\n").unwrap();
        fs::write(dir.path().join("memory.max"), "max\n").unwrap();
        fs::write(
            dir.path().join("memory.events"),
            "low 0\noom 7\noom_kill 2\n",
        )
        .unwrap();

        let snapshot = read_cgroup_memory_snapshot_from(dir.path());
        assert_eq!(snapshot.memory_current_bytes, Some(123));
        assert_eq!(snapshot.memory_max_bytes, None);
        assert_eq!(snapshot.memory_max_raw.as_deref(), Some("max"));
        assert_eq!(snapshot.oom_events, Some(7));
        assert_eq!(snapshot.oom_kill_events, Some(2));
    }

    #[test]
    fn test_parse_process_memory_status_uses_rss_plus_swap_as_committed() {
        let status = r#"Name:\tckbadger
VmHWM:   4096 kB
VmRSS:   3072 kB
RssAnon: 2048 kB
VmSwap:  1024 kB
"#;
        let snapshot = parse_process_memory_status(status).unwrap();
        assert_eq!(snapshot.rss_bytes, 3 * 1024 * 1024);
        assert_eq!(snapshot.rss_anon_bytes, 2 * 1024 * 1024);
        assert_eq!(snapshot.swap_bytes, 1024 * 1024);
        assert_eq!(snapshot.high_water_rss_bytes, 4 * 1024 * 1024);
        assert_eq!(snapshot.committed_bytes().unwrap(), 4 * 1024 * 1024);
    }
}
