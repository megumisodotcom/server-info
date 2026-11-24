use std::fs;
use std::process::Command;

/// Read a file into a string. Returns `None` on any error.
pub fn read(path: &str) -> Option<String> {
    fs::read_to_string(path).ok()
}

/// Read a file and trim surrounding whitespace. Empty results become `None`.
pub fn read_trim(path: &str) -> Option<String> {
    read(path)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Run a command and return its trimmed stdout. Returns `None` when the
/// command is missing, fails, or prints nothing.
pub fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Format a byte count using binary units (KiB, MiB, ...).
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    if bytes < 1024 {
        return format!("{} B", bytes);
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{:.1} {}", value, UNITS[unit])
}

/// Format a number of seconds as `3d 4h 5m`.
pub fn format_uptime(secs: u64) -> String {
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let minutes = (secs % 3_600) / 60;
    let seconds = secs % 60;
    if days > 0 {
        format!("{}d {}h {}m", days, hours, minutes)
    } else if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, seconds)
    } else {
        format!("{}s", seconds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_bytes_units() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1024), "1.0 KiB");
        assert_eq!(human_bytes(1536), "1.5 KiB");
        assert_eq!(human_bytes(1024 * 1024), "1.0 MiB");
        assert_eq!(human_bytes(3 * 1024 * 1024 * 1024), "3.0 GiB");
    }

    #[test]
    fn uptime_formatting() {
        assert_eq!(format_uptime(5), "5s");
        assert_eq!(format_uptime(65), "1m 5s");
        assert_eq!(format_uptime(3_665), "1h 1m");
        assert_eq!(format_uptime(90_061), "1d 1h 1m");
    }
}
