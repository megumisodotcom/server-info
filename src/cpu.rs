use std::collections::{BTreeMap, BTreeSet};
use std::thread::sleep;
use std::time::Duration;

use crate::json::Json;
use crate::output::Output;
use crate::util;

pub struct CpuInfo {
    pub model: Option<String>,
    pub logical: usize,
    pub physical: Option<usize>,
    pub max_mhz: Option<f64>,
    pub usage: Option<CpuUsage>,
}

pub struct CpuUsage {
    pub total: f64,
    pub per_core: Vec<f64>,
}

struct StatSample {
    total: u64,
    idle: u64,
}

pub fn collect(sample_ms: u64) -> CpuInfo {
    let cpuinfo = util::read("/proc/cpuinfo").map(|t| parse_cpuinfo(&t));
    let lscpu = util::run("lscpu", &[]).map(|t| parse_lscpu(&t));

    let logical = cpuinfo
        .as_ref()
        .map(|c| c.logical)
        .filter(|n| *n > 0)
        .or_else(|| lscpu.as_ref().and_then(|l| l.logical))
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(0)
        });

    let model = cpuinfo
        .as_ref()
        .and_then(|c| c.best_model())
        .or_else(|| lscpu.as_ref().and_then(|l| l.model.clone()))
        .or_else(|| util::read_trim("/proc/device-tree/model"))
        .or_else(|| util::read_trim("/sys/firmware/devicetree/base/model"))
        .or_else(|| util::run("sysctl", &["-n", "machdep.cpu.brand_string"]));

    let physical = lscpu
        .as_ref()
        .and_then(|l| match (l.sockets, l.cores_per_socket) {
            (Some(s), Some(c)) if s > 0 && c > 0 => Some(s * c),
            _ => None,
        })
        .or_else(|| cpuinfo.as_ref().and_then(|c| c.physical_cores()));

    let max_mhz = cpuinfo
        .as_ref()
        .and_then(|c| c.max_mhz)
        .into_iter()
        .chain(lscpu.as_ref().and_then(|l| l.max_mhz))
        .reduce(f64::max);

    CpuInfo {
        model,
        logical,
        physical,
        max_mhz,
        usage: sample_usage(sample_ms),
    }
}

impl CpuInfo {
    pub fn render(&self, out: &Output) {
        out.section("CPU");
        out.kv("Model", self.model.as_deref().unwrap_or("unknown"));
        out.kv(
            "Cores",
            &match self.physical {
                Some(p) => format!("{} logical / {} physical", self.logical, p),
                None => format!("{} logical", self.logical),
            },
        );
        if let Some(mhz) = self.max_mhz {
            out.kv("Max freq", &format!("{:.2} GHz", mhz / 1000.0));
        }
        match &self.usage {
            Some(usage) => {
                out.kv(
                    "Usage",
                    &format!("{:>5.1}%  {}", usage.total, out.bar(usage.total, 20)),
                );
                if usage.per_core.len() > 1 {
                    let cells: Vec<String> = usage
                        .per_core
                        .iter()
                        .enumerate()
                        .map(|(i, pct)| format!("cpu{:<3} {:>5.1}%", i, pct))
                        .collect();
                    let rows: Vec<String> =
                        cells.chunks(4).map(|chunk| chunk.join("   ")).collect();
                    out.kv_lines("Per-core", &rows);
                }
            }
            None => out.kv("Usage", "unavailable"),
        }
    }

    pub fn to_json(&self) -> Json {
        Json::object([
            ("model", Json::opt_str(self.model.as_deref())),
            ("logical_cores", Json::num(self.logical)),
            ("physical_cores", Json::opt_num(self.physical)),
            ("max_mhz", Json::opt_num(self.max_mhz)),
            (
                "usage_percent",
                Json::opt_num(self.usage.as_ref().map(|u| round1(u.total))),
            ),
            (
                "per_core_percent",
                match &self.usage {
                    Some(u) => Json::array(u.per_core.iter().map(|p| Json::num(round1(*p)))),
                    None => Json::Null,
                },
            ),
        ])
    }
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

struct CpuFields {
    logical: usize,
    model: Option<String>,
    model_fallback: Option<String>,
    pairs: BTreeSet<(String, String)>,
    cores_by_phys: BTreeMap<String, u32>,
    max_mhz: Option<f64>,
}

impl CpuFields {
    fn best_model(&self) -> Option<String> {
        self.model.clone().or_else(|| self.model_fallback.clone())
    }
}

impl CpuFields {
    fn physical_cores(&self) -> Option<usize> {
        if !self.pairs.is_empty() {
            return Some(self.pairs.len());
        }
        let sum: u32 = self.cores_by_phys.values().sum();
        if sum > 0 {
            Some(sum as usize)
        } else {
            None
        }
    }
}

fn parse_cpuinfo(text: &str) -> CpuFields {
    let logical = text
        .lines()
        .filter(|line| {
            line.split_once(':')
                .map(|(key, _)| key.trim().eq_ignore_ascii_case("processor"))
                .unwrap_or(false)
        })
        .count();

    let mut best_model: Option<(u8, String)> = None;
    let mut fallback_model: Option<String> = None;
    let mut pairs = BTreeSet::new();
    let mut cores_by_phys: BTreeMap<String, u32> = BTreeMap::new();
    let mut max_mhz: Option<f64> = None;

    for block in text.split("\n\n") {
        let mut physical_id: Option<String> = None;
        let mut core_id: Option<String> = None;
        let mut cpu_cores: Option<u32> = None;

        for line in block.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            match key.as_str() {
                "model name" => consider_model(&mut best_model, &mut fallback_model, 3, value),
                "model" | "hardware" => {
                    consider_model(&mut best_model, &mut fallback_model, 2, value)
                }
                "processor" if value.chars().any(|c| c.is_ascii_alphabetic()) => {
                    consider_model(&mut best_model, &mut fallback_model, 1, value)
                }
                "physical id" => physical_id = Some(value.to_string()),
                "core id" => core_id = Some(value.to_string()),
                "cpu cores" => cpu_cores = value.parse::<u32>().ok(),
                "cpu mhz" => {
                    if let Ok(mhz) = value.parse::<f64>() {
                        max_mhz = Some(max_mhz.map_or(mhz, |m| m.max(mhz)));
                    }
                }
                _ => {}
            }
        }

        if let (Some(phys), Some(core)) = (&physical_id, &core_id) {
            pairs.insert((phys.clone(), core.clone()));
        }
        if let (Some(phys), Some(cores)) = (physical_id, cpu_cores) {
            let entry = cores_by_phys.entry(phys).or_insert(0);
            *entry = (*entry).max(cores);
        }
    }

    CpuFields {
        logical,
        model: best_model.map(|(_, m)| m),
        model_fallback: fallback_model,
        pairs,
        cores_by_phys,
        max_mhz,
    }
}

fn consider_model(
    best: &mut Option<(u8, String)>,
    fallback: &mut Option<String>,
    priority: u8,
    value: &str,
) {
    let lower = value.to_ascii_lowercase();
    let generic = lower.starts_with("aarch64")
        || lower.starts_with("armv")
        || lower.starts_with("arm")
        || lower == "unknown";
    if generic {
        if fallback.is_none() {
            *fallback = Some(value.to_string());
        }
        return;
    }
    if best.as_ref().map(|(p, _)| priority > *p).unwrap_or(true) {
        *best = Some((priority, value.to_string()));
    }
}

#[derive(Default)]
struct LscpuFacts {
    model: Option<String>,
    sockets: Option<usize>,
    cores_per_socket: Option<usize>,
    max_mhz: Option<f64>,
    logical: Option<usize>,
}

fn parse_lscpu(text: &str) -> LscpuFacts {
    let mut facts = LscpuFacts::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "Model name" | "BIOS Model name" => {
                if !value.is_empty() && facts.model.is_none() {
                    facts.model = Some(value.to_string());
                }
            }
            "Socket(s)" => facts.sockets = value.parse().ok(),
            "Core(s) per socket" => facts.cores_per_socket = value.parse().ok(),
            "CPU max MHz" => facts.max_mhz = value.parse().ok(),
            "CPU(s)" => facts.logical = value.parse().ok(),
            _ => {}
        }
    }
    facts
}

fn parse_stat(text: &str) -> Vec<StatSample> {
    let mut samples = Vec::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(name) = parts.next() else {
            continue;
        };
        let is_cpu_line = name == "cpu"
            || (name.starts_with("cpu")
                && name.len() > 3
                && name[3..].chars().all(|c| c.is_ascii_digit()));
        if !is_cpu_line {
            continue;
        }
        let values: Vec<u64> = parts.take(8).filter_map(|v| v.parse().ok()).collect();
        if values.len() < 4 {
            continue;
        }
        let total: u64 = values.iter().sum();
        let idle = values[3] + values.get(4).copied().unwrap_or(0);
        samples.push(StatSample { total, idle });
    }
    samples
}

fn sample_usage(sample_ms: u64) -> Option<CpuUsage> {
    let text = util::read("/proc/stat")?;
    let first = parse_stat(&text);
    if first.is_empty() {
        return None;
    }
    sleep(Duration::from_millis(sample_ms.clamp(50, 5_000)));
    let second = parse_stat(&util::read("/proc/stat")?);
    if second.len() != first.len() {
        return None;
    }
    let mut per_core = Vec::new();
    for (before, after) in first.iter().zip(second.iter()) {
        per_core.push(percent_between(before, after));
    }
    let total = per_core.first().copied().unwrap_or(0.0);
    Some(CpuUsage {
        total,
        per_core: per_core.into_iter().skip(1).collect(),
    })
}

fn percent_between(before: &StatSample, after: &StatSample) -> f64 {
    let total_delta = after.total.saturating_sub(before.total);
    if total_delta == 0 {
        return 0.0;
    }
    let idle_delta = after.idle.saturating_sub(before.idle);
    let busy = total_delta.saturating_sub(idle_delta) as f64;
    (busy / total_delta as f64 * 100.0).clamp(0.0, 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stat_aggregate_and_cores() {
        let text = "cpu  100 0 100 800 0 0 0 0 0 0\ncpu0 50 0 50 400 0 0 0 0 0 0\ncpu1 50 0 50 400 0 0 0 0 0 0\nintr 123\n";
        let samples = parse_stat(text);
        assert_eq!(samples.len(), 3);
        assert_eq!(samples[0].total, 1000);
        assert_eq!(samples[0].idle, 800);
    }

    #[test]
    fn computes_usage_between_samples() {
        let before = StatSample {
            total: 1000,
            idle: 800,
        };
        let after = StatSample {
            total: 1200,
            idle: 900,
        };
        assert!((percent_between(&before, &after) - 50.0).abs() < 0.001);
        assert_eq!(percent_between(&after, &after), 0.0);
    }

    #[test]
    fn parses_cpuinfo_model_and_cores() {
        let text = "processor\t: 0\nmodel name\t: Example CPU\nphysical id\t: 0\ncore id\t: 0\ncpu cores\t: 2\ncpu MHz\t\t: 2400.000\n\nprocessor\t: 1\nmodel name\t: Example CPU\nphysical id\t: 0\ncore id\t: 1\ncpu cores\t: 2\ncpu MHz\t\t: 2600.000\n\n";
        let fields = parse_cpuinfo(text);
        assert_eq!(fields.logical, 2);
        assert_eq!(fields.best_model().as_deref(), Some("Example CPU"));
        assert_eq!(fields.physical_cores(), Some(2));
        assert_eq!(fields.max_mhz, Some(2600.0));
    }

    #[test]
    fn parses_lscpu_topology() {
        let text = "CPU(s):              16\nModel name:          AMD EPYC 7302P\nSocket(s):           1\nCore(s) per socket:  8\nCPU max MHz:         3000.0000\n";
        let facts = parse_lscpu(text);
        assert_eq!(facts.logical, Some(16));
        assert_eq!(facts.sockets, Some(1));
        assert_eq!(facts.cores_per_socket, Some(8));
        assert_eq!(facts.max_mhz, Some(3000.0));
        assert_eq!(facts.model.as_deref(), Some("AMD EPYC 7302P"));
    }
}
