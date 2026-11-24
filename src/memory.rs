use std::collections::BTreeMap;

use crate::json::Json;
use crate::output::Output;
use crate::util;

pub struct MemoryInfo {
    pub total: u64,
    pub available: u64,
    pub free: u64,
    pub buffers: u64,
    pub cached: u64,
    pub swap_total: u64,
    pub swap_free: u64,
}

impl MemoryInfo {
    pub fn collect() -> MemoryInfo {
        if let Some(text) = util::read("/proc/meminfo") {
            let values = parse_meminfo(&text);
            let get = |key: &str| values.get(key).copied().unwrap_or(0);
            let total = get("MemTotal");
            let free = get("MemFree");
            let buffers = get("Buffers");
            let cached = get("Cached") + get("SReclaimable");
            let available = match values.get("MemAvailable").copied() {
                Some(v) => v,
                None => free + buffers + cached,
            };
            let info = MemoryInfo {
                total,
                available,
                free,
                buffers,
                cached,
                swap_total: get("SwapTotal"),
                swap_free: get("SwapFree"),
            };
            if info.total > 0 {
                return info;
            }
        }
        memory_from_sysctl()
    }

    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.available)
    }

    pub fn used_percent(&self) -> f64 {
        percent(self.used(), self.total)
    }

    pub fn swap_used(&self) -> u64 {
        self.swap_total.saturating_sub(self.swap_free)
    }

    pub fn render(&self, out: &Output) {
        out.section("Memory");
        out.kv("RAM", &format!("{} total", util::human_bytes(self.total)));
        out.kv(
            "Used",
            &format!(
                "{} / {} ({:>4.1}%)  {}",
                util::human_bytes(self.used()),
                util::human_bytes(self.total),
                self.used_percent(),
                out.bar(self.used_percent(), 20)
            ),
        );
        out.kv("Available", &util::human_bytes(self.available));
        out.kv(
            "Free",
            &format!(
                "{}  (buffers {} / cache {})",
                util::human_bytes(self.free),
                util::human_bytes(self.buffers),
                util::human_bytes(self.cached)
            ),
        );
        if self.swap_total == 0 {
            out.kv("Swap", "none");
        } else {
            out.kv(
                "Swap",
                &format!(
                    "{} / {} ({:.1}%)",
                    util::human_bytes(self.swap_used()),
                    util::human_bytes(self.swap_total),
                    percent(self.swap_used(), self.swap_total)
                ),
            );
        }
    }

    pub fn to_json(&self) -> Json {
        Json::object([
            ("total_bytes", Json::num(self.total)),
            ("used_bytes", Json::num(self.used())),
            ("available_bytes", Json::num(self.available)),
            ("free_bytes", Json::num(self.free)),
            ("buffers_bytes", Json::num(self.buffers)),
            ("cached_bytes", Json::num(self.cached)),
            ("used_percent", Json::num(round1(self.used_percent()))),
            ("swap_total_bytes", Json::num(self.swap_total)),
            ("swap_free_bytes", Json::num(self.swap_free)),
        ])
    }
}

fn percent(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64 * 100.0
    }
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn parse_meminfo(text: &str) -> BTreeMap<String, u64> {
    let mut values = BTreeMap::new();
    for line in text.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let mut parts = rest.split_whitespace();
        let Some(number) = parts.next().and_then(|v| v.parse::<u64>().ok()) else {
            continue;
        };
        let multiplier = match parts.next() {
            Some("kB") => 1024,
            Some("MB") => 1024 * 1024,
            Some("GB") => 1024 * 1024 * 1024,
            _ => 1,
        };
        values.insert(key.trim().to_string(), number.saturating_mul(multiplier));
    }
    values
}

fn memory_from_sysctl() -> MemoryInfo {
    let total = util::run("sysctl", &["-n", "hw.memsize"])
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let page_size = util::run("sysctl", &["-n", "hw.pagesize"])
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(4096);

    let mut free_pages = 0u64;
    let mut available_pages = 0u64;
    if let Some(text) = util::run("vm_stat", &[]) {
        for line in text.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let pages = value
                .trim()
                .trim_end_matches('.')
                .parse::<u64>()
                .unwrap_or(0);
            match key.trim() {
                "Pages free" => free_pages = pages,
                "Pages inactive" | "Pages speculative" | "Pages purgeable" => {
                    available_pages += pages
                }
                _ => {}
            }
        }
    }

    let (swap_total, swap_used) = swap_usage().unwrap_or((0, 0));
    let free = free_pages.saturating_mul(page_size);
    let available = (free_pages + available_pages)
        .saturating_mul(page_size)
        .min(total);
    MemoryInfo {
        total,
        available,
        free,
        buffers: 0,
        cached: 0,
        swap_total,
        swap_free: swap_total.saturating_sub(swap_used),
    }
}

/// Parse `sysctl -n vm.swapusage` (e.g. `total = 2048.00M  used = 512.00M`).
fn swap_usage() -> Option<(u64, u64)> {
    let text = util::run("sysctl", &["-n", "vm.swapusage"])?;
    let total = value_after(&text, "total")?;
    let used = value_after(&text, "used")?;
    Some((total, used))
}

fn value_after(text: &str, key: &str) -> Option<u64> {
    let mut tokens = text.split_whitespace();
    while let Some(token) = tokens.next() {
        if token == key {
            let equals = tokens.next()?;
            if equals != "=" {
                return None;
            }
            return parse_size(tokens.next()?);
        }
    }
    None
}

fn parse_size(value: &str) -> Option<u64> {
    let (number, unit) = value.split_at(value.find(|c: char| c.is_ascii_alphabetic())?);
    let multiplier = match unit {
        "K" => 1024,
        "M" => 1024 * 1024,
        "G" => 1024 * 1024 * 1024,
        _ => 1,
    };
    let number = number.parse::<f64>().ok()?;
    Some((number * multiplier as f64) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_meminfo_values_in_bytes() {
        let text = "MemTotal:       16384000 kB\nMemFree:         1024000 kB\nMemAvailable:    8192000 kB\nBuffers:          512000 kB\nCached:          4096000 kB\nSReclaimable:     256000 kB\nSwapTotal:       2048000 kB\nSwapFree:        1024000 kB\n";
        let values = parse_meminfo(text);
        assert_eq!(values["MemTotal"], 16_384_000 * 1024);
        assert_eq!(values["MemAvailable"], 8_192_000 * 1024);
        assert_eq!(values["SwapTotal"], 2_048_000 * 1024);
    }

    #[test]
    fn computes_used_and_percent() {
        let info = MemoryInfo {
            total: 1000,
            available: 250,
            free: 100,
            buffers: 50,
            cached: 100,
            swap_total: 0,
            swap_free: 0,
        };
        assert_eq!(info.used(), 750);
        assert!((info.used_percent() - 75.0).abs() < f64::EPSILON);
    }
}
