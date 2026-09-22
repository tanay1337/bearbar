use std::fs;
use std::path::Path;
use std::time::Duration;

use tokio::sync::watch;
use tracing::warn;

use crate::runtime;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SystemSnapshot {
    pub connected: bool,
    pub cpu_percent: u8,
    pub memory_percent: u8,
    pub memory_used_gib: f64,
    pub memory_available_gib: f64,
    pub memory_total_gib: f64,
    pub load_one: f64,
    pub logical_cpus: usize,
    pub temperature_c: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct SystemService {
    state: watch::Receiver<SystemSnapshot>,
}

impl SystemService {
    pub fn start() -> Self {
        let (state_tx, state) = watch::channel(SystemSnapshot::default());
        runtime::spawn(run(state_tx));
        Self { state }
    }

    pub fn subscribe(&self) -> watch::Receiver<SystemSnapshot> {
        self.state.clone()
    }
}

#[derive(Debug, Clone, Copy)]
struct CpuTimes {
    idle: u64,
    total: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct MemoryUsage {
    percent: u8,
    used_gib: f64,
    available_gib: f64,
    total_gib: f64,
}

async fn run(state_tx: watch::Sender<SystemSnapshot>) {
    let mut interval = tokio::time::interval(Duration::from_secs(2));
    let mut previous_cpu = None;

    loop {
        interval.tick().await;
        match read_snapshot(previous_cpu) {
            Ok((snapshot, current_cpu)) => {
                previous_cpu = Some(current_cpu);
                state_tx.send_replace(snapshot);
            }
            Err(error) => {
                warn!(%error, "failed to read system metrics");
                let mut snapshot = state_tx.borrow().clone();
                snapshot.connected = false;
                state_tx.send_replace(snapshot);
            }
        }
    }
}

fn read_snapshot(previous_cpu: Option<CpuTimes>) -> Result<(SystemSnapshot, CpuTimes), String> {
    let stat = fs::read_to_string("/proc/stat").map_err(|error| error.to_string())?;
    let memory = fs::read_to_string("/proc/meminfo").map_err(|error| error.to_string())?;
    let loadavg = fs::read_to_string("/proc/loadavg").map_err(|error| error.to_string())?;
    let current_cpu = parse_cpu_times(&stat)?;
    let memory = parse_memory_usage(&memory)?;

    Ok((
        SystemSnapshot {
            connected: true,
            cpu_percent: cpu_usage(previous_cpu, current_cpu),
            memory_percent: memory.percent,
            memory_used_gib: memory.used_gib,
            memory_available_gib: memory.available_gib,
            memory_total_gib: memory.total_gib,
            load_one: parse_load_one(&loadavg)?,
            logical_cpus: std::thread::available_parallelism()
                .map(usize::from)
                .unwrap_or(1),
            temperature_c: read_temperature(Path::new("/sys/class/thermal")),
        },
        current_cpu,
    ))
}

fn parse_cpu_times(source: &str) -> Result<CpuTimes, String> {
    let line = source
        .lines()
        .find(|line| line.starts_with("cpu "))
        .ok_or_else(|| "missing aggregate CPU line in /proc/stat".to_owned())?;
    let values = line
        .split_whitespace()
        .skip(1)
        .map(|value| value.parse::<u64>().map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    if values.len() < 5 {
        return Err("aggregate CPU line has too few values".into());
    }

    Ok(CpuTimes {
        idle: values[3].saturating_add(values[4]),
        total: values.iter().copied().sum(),
    })
}

fn cpu_usage(previous: Option<CpuTimes>, current: CpuTimes) -> u8 {
    let Some(previous) = previous else {
        return 0;
    };
    let total = current.total.saturating_sub(previous.total);
    let idle = current.idle.saturating_sub(previous.idle);
    if total == 0 {
        return 0;
    }
    (((total.saturating_sub(idle)) as f64 / total as f64) * 100.0)
        .round()
        .clamp(0.0, 100.0) as u8
}

fn parse_memory_usage(source: &str) -> Result<MemoryUsage, String> {
    let value = |key: &str| {
        source.lines().find_map(|line| {
            line.strip_prefix(key)?
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()
        })
    };
    let total = value("MemTotal:").ok_or_else(|| "missing MemTotal".to_owned())?;
    let available = value("MemAvailable:").ok_or_else(|| "missing MemAvailable".to_owned())?;
    if total == 0 {
        return Ok(MemoryUsage {
            percent: 0,
            used_gib: 0.0,
            available_gib: 0.0,
            total_gib: 0.0,
        });
    }
    let used = total.saturating_sub(available);
    const KIB_PER_GIB: f64 = 1_048_576.0;
    Ok(MemoryUsage {
        percent: (((used as f64 / total as f64) * 100.0)
            .round()
            .clamp(0.0, 100.0)) as u8,
        used_gib: used as f64 / KIB_PER_GIB,
        available_gib: available as f64 / KIB_PER_GIB,
        total_gib: total as f64 / KIB_PER_GIB,
    })
}

fn parse_load_one(source: &str) -> Result<f64, String> {
    source
        .split_whitespace()
        .next()
        .ok_or_else(|| "missing one-minute load average".to_owned())?
        .parse::<f64>()
        .map_err(|error| error.to_string())
}

fn read_temperature(root: &Path) -> Option<f64> {
    let mut values = fs::read_dir(root)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !entry
                .file_name()
                .to_string_lossy()
                .starts_with("thermal_zone")
            {
                return None;
            }
            let kind = fs::read_to_string(path.join("type")).ok()?;
            let raw = fs::read_to_string(path.join("temp")).ok()?;
            let temperature = raw.trim().parse::<f64>().ok()? / 1_000.0;
            if !(-20.0..=150.0).contains(&temperature) {
                return None;
            }
            let kind = kind.trim().to_ascii_lowercase();
            let priority = if kind.contains("x86_pkg_temp")
                || kind.contains("tcpu")
                || kind.contains("package")
            {
                0
            } else if kind.contains("cpu") {
                1
            } else if kind.contains("acpitz") {
                2
            } else {
                3
            };
            Some((priority, temperature))
        })
        .collect::<Vec<_>>();
    values.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| right.1.total_cmp(&left.1))
    });
    values.first().map(|(_, temperature)| *temperature)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculates_cpu_delta() {
        let previous = CpuTimes {
            idle: 700,
            total: 1_000,
        };
        let current = CpuTimes {
            idle: 760,
            total: 1_100,
        };
        assert_eq!(cpu_usage(Some(previous), current), 40);
    }

    #[test]
    fn parses_available_memory() {
        let source = "MemTotal: 2097152 kB\nMemAvailable: 1572864 kB\n";
        let memory = parse_memory_usage(source).unwrap();
        assert_eq!(memory.percent, 25);
        assert_eq!(memory.used_gib, 0.5);
        assert_eq!(memory.available_gib, 1.5);
        assert_eq!(memory.total_gib, 2.0);
    }

    #[test]
    fn parses_one_minute_load_average() {
        assert_eq!(parse_load_one("0.42 0.36 0.31 1/123 900").unwrap(), 0.42);
    }
}
