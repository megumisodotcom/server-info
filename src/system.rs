use crate::json::Json;
use crate::output::Output;
use crate::util;

pub struct SystemInfo {
    pub hostname: Option<String>,
    pub os: Option<String>,
    pub kernel: Option<String>,
    pub arch: &'static str,
    pub uptime_secs: Option<u64>,
    pub load: Option<(f64, f64, f64)>,
}

impl SystemInfo {
    pub fn collect() -> SystemInfo {
        SystemInfo {
            hostname: hostname(),
            os: os_name(),
            kernel: kernel(),
            arch: std::env::consts::ARCH,
            uptime_secs: uptime_secs(),
            load: load_average(),
        }
    }

    pub fn render(&self, out: &Output) {
        out.section("System");
        out.kv("Hostname", self.hostname.as_deref().unwrap_or("unknown"));
        out.kv("OS", self.os.as_deref().unwrap_or("unknown"));
        out.kv("Kernel", self.kernel.as_deref().unwrap_or("unknown"));
        out.kv("Arch", self.arch);
        out.kv(
            "Uptime",
            &self
                .uptime_secs
                .map(util::format_uptime)
                .unwrap_or_else(|| "unknown".to_string()),
        );
        out.kv(
            "Load avg",
            &self
                .load
                .map(|(a, b, c)| format!("{:.2}  {:.2}  {:.2}", a, b, c))
                .unwrap_or_else(|| "unknown".to_string()),
        );
    }

    pub fn to_json(&self) -> Json {
        Json::object([
            ("hostname", Json::opt_str(self.hostname.as_deref())),
            ("os", Json::opt_str(self.os.as_deref())),
            ("kernel", Json::opt_str(self.kernel.as_deref())),
            ("arch", Json::str(self.arch)),
            ("uptime_seconds", Json::opt_num(self.uptime_secs)),
            (
                "load_average",
                match self.load {
                    Some((a, b, c)) => Json::array([Json::num(a), Json::num(b), Json::num(c)]),
                    None => Json::Null,
                },
            ),
        ])
    }
}

fn hostname() -> Option<String> {
    util::read_trim("/proc/sys/kernel/hostname")
        .or_else(|| util::read_trim("/etc/hostname"))
        .or_else(|| util::run("hostname", &[]))
}

fn os_name() -> Option<String> {
    if let Some(text) = util::read("/etc/os-release").or_else(|| util::read("/usr/lib/os-release"))
    {
        if let Some(name) = parse_os_release(&text) {
            return Some(name);
        }
    }
    let product = util::run("sw_vers", &["-productName"])?;
    let version = util::run("sw_vers", &["-productVersion"]);
    Some(match version {
        Some(v) => format!("{} {}", product, v),
        None => product,
    })
}

fn parse_os_release(text: &str) -> Option<String> {
    let mut fallback = None;
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        match key.trim() {
            "PRETTY_NAME" if !value.is_empty() => return Some(value.to_string()),
            "NAME" if !value.is_empty() => fallback = Some(value.to_string()),
            _ => {}
        }
    }
    fallback
}

fn kernel() -> Option<String> {
    util::read_trim("/proc/sys/kernel/osrelease").or_else(|| util::run("uname", &["-r"]))
}

fn uptime_secs() -> Option<u64> {
    if let Some(text) = util::read("/proc/uptime") {
        if let Some(first) = text.split_whitespace().next() {
            if let Ok(secs) = first.parse::<f64>() {
                return Some(secs as u64);
            }
        }
    }
    let text = util::run("sysctl", &["-n", "kern.boottime"])?;
    let secs = text
        .split("sec =")
        .nth(1)?
        .split(',')
        .next()?
        .trim()
        .parse::<u64>()
        .ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    now.checked_sub(secs)
}

fn load_average() -> Option<(f64, f64, f64)> {
    if let Some(text) = util::read("/proc/loadavg") {
        let parts: Vec<f64> = text
            .split_whitespace()
            .take(3)
            .filter_map(|v| v.parse().ok())
            .collect();
        if parts.len() == 3 {
            return Some((parts[0], parts[1], parts[2]));
        }
    }
    let text = util::run("sysctl", &["-n", "vm.loadavg"])?;
    let cleaned = text.trim_matches(|c: char| c == '{' || c == '}' || c.is_whitespace());
    let parts: Vec<f64> = cleaned
        .split_whitespace()
        .filter_map(|v| v.parse().ok())
        .collect();
    if parts.len() == 3 {
        Some((parts[0], parts[1], parts[2]))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_release_prefers_pretty_name() {
        let text = "NAME=\"Debian GNU/Linux\"\nPRETTY_NAME=\"Debian GNU/Linux 12 (bookworm)\"\n";
        assert_eq!(
            parse_os_release(text).as_deref(),
            Some("Debian GNU/Linux 12 (bookworm)")
        );
    }

    #[test]
    fn os_release_falls_back_to_name() {
        let text = "NAME=\"Alpine Linux\"\nID=alpine\n";
        assert_eq!(parse_os_release(text).as_deref(), Some("Alpine Linux"));
    }
}
