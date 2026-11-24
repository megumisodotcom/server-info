use crate::json::Json;
use crate::output::Output;
use crate::util;

pub struct RaidArray {
    pub name: String,
    pub level: Option<String>,
    pub state: Option<String>,
    pub members: Vec<String>,
    pub size_bytes: Option<u64>,
    pub disks_up: Option<u32>,
    pub disks_total: Option<u32>,
    pub disk_map: Option<String>,
    pub sync: Option<String>,
}

impl RaidArray {
    fn new(name: &str) -> RaidArray {
        RaidArray {
            name: name.to_string(),
            level: None,
            state: None,
            members: Vec::new(),
            size_bytes: None,
            disks_up: None,
            disks_total: None,
            disk_map: None,
            sync: None,
        }
    }

    fn degraded(&self) -> bool {
        match (self.disks_up, self.disks_total) {
            (Some(up), Some(total)) => up < total,
            _ => self
                .disk_map
                .as_deref()
                .is_some_and(|map| map.contains('_')),
        }
    }
}

pub struct RaidInfo {
    pub arrays: Vec<RaidArray>,
    pub controllers: Vec<String>,
}

impl RaidInfo {
    pub fn collect() -> RaidInfo {
        let mut arrays = util::read("/proc/mdstat")
            .map(|text| parse_mdstat(&text))
            .unwrap_or_default();
        if arrays.is_empty() {
            arrays = collect_sysfs();
        }
        RaidInfo {
            arrays,
            controllers: hardware_controllers(),
        }
    }

    pub fn render(&self, out: &Output) {
        out.section("RAID");
        if self.arrays.is_empty() {
            out.note("no software RAID (md) detected");
        }
        for array in &self.arrays {
            let mut summary = String::new();
            if let Some(level) = &array.level {
                summary.push_str(&format!("{:<8}", level));
            }
            if let Some(state) = &array.state {
                summary.push_str(&format!("{:<8}", state));
            }
            if let (Some(up), Some(total)) = (array.disks_up, array.disks_total) {
                summary.push_str(&format!("[{}/{}] ", up, total));
            }
            if let Some(map) = &array.disk_map {
                summary.push_str(&format!("[{}] ", map));
            }
            if let Some(size) = array.size_bytes {
                summary.push_str(&util::human_bytes(size));
            }
            if array.degraded() {
                summary.push_str(&out.accent("  DEGRADED"));
            }
            out.kv(&array.name, summary.trim_end());
            if !array.members.is_empty() {
                println!("      {} {}", out.dim("members"), array.members.join(" "));
            }
            if let Some(sync) = &array.sync {
                println!("      {} {}", out.dim("progress"), sync);
            }
        }
        for controller in &self.controllers {
            out.note(&format!("controller: {}", controller));
        }
    }

    pub fn to_json(&self) -> Json {
        Json::object([
            (
                "arrays",
                Json::array(self.arrays.iter().map(|array| {
                    Json::object([
                        ("name", Json::str(array.name.clone())),
                        ("level", Json::opt_str(array.level.clone())),
                        ("state", Json::opt_str(array.state.clone())),
                        (
                            "members",
                            Json::array(array.members.iter().map(|m| Json::str(m.clone()))),
                        ),
                        ("size_bytes", Json::opt_num(array.size_bytes)),
                        ("disks_up", Json::opt_num(array.disks_up)),
                        ("disks_total", Json::opt_num(array.disks_total)),
                        ("disk_map", Json::opt_str(array.disk_map.clone())),
                        ("degraded", Json::Bool(array.degraded())),
                        ("sync", Json::opt_str(array.sync.clone())),
                    ])
                })),
            ),
            (
                "controllers",
                Json::array(self.controllers.iter().map(|c| Json::str(c.clone()))),
            ),
        ])
    }
}

fn parse_mdstat(text: &str) -> Vec<RaidArray> {
    let mut arrays: Vec<RaidArray> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if !line.starts_with(char::is_whitespace) {
            let Some((name, rest)) = line.split_once(':') else {
                continue;
            };
            let name = name.trim();
            if !name.starts_with("md") {
                continue;
            }
            let mut array = RaidArray::new(name);
            for (index, token) in rest.split_whitespace().enumerate() {
                if index == 0 && (token == "active" || token == "inactive") {
                    array.state = Some(token.to_string());
                    continue;
                }
                if token.starts_with('(') {
                    continue;
                }
                if is_raid_level(token) {
                    if array.level.is_none() {
                        array.level = Some(token.to_string());
                    }
                    continue;
                }
                if token.contains('[') {
                    array.members.push(token.to_string());
                }
            }
            arrays.push(array);
        } else if let Some(array) = arrays.last_mut() {
            parse_detail_line(array, line);
        }
    }
    arrays
}

fn is_raid_level(token: &str) -> bool {
    token.starts_with("raid") || matches!(token, "linear" | "multipath" | "faulty")
}

fn parse_detail_line(array: &mut RaidArray, line: &str) {
    let tokens: Vec<&str> = line.split_whitespace().collect();

    for (index, token) in tokens.iter().enumerate() {
        if *token == "blocks" && index > 0 {
            if let Ok(blocks) = tokens[index - 1].parse::<u64>() {
                array.size_bytes = Some(blocks.saturating_mul(512));
            }
        }
        if let Some(inner) = bracket_content(token) {
            if let Some((up, total)) = parse_disk_counts(inner) {
                array.disks_up = Some(up);
                array.disks_total = Some(total);
            } else if !inner.is_empty()
                && inner
                    .chars()
                    .all(|c| matches!(c, 'U' | '_' | 'S' | 'R' | 'F' | 'W'))
            {
                array.disk_map = Some(inner.to_string());
            }
        }
    }

    for window in tokens.windows(3) {
        if !matches!(window[0], "resync" | "recovery" | "check" | "reshape") {
            continue;
        }
        if window[1] != "=" {
            continue;
        }
        if let Some(value) = window[2].strip_suffix('%') {
            if let Ok(percent) = value.parse::<f64>() {
                array.sync = Some(format!("{} {:.1}%", window[0], percent));
            }
        }
    }
}

fn bracket_content(token: &str) -> Option<&str> {
    if token.len() >= 2 && token.starts_with('[') && token.ends_with(']') {
        Some(&token[1..token.len() - 1])
    } else {
        None
    }
}

/// Parses mdstat's `[total/active]` (for example `[2/1]` on a degraded mirror).
fn parse_disk_counts(inner: &str) -> Option<(u32, u32)> {
    let (total, active) = inner.split_once('/')?;
    Some((active.parse().ok()?, total.parse().ok()?))
}

fn collect_sysfs() -> Vec<RaidArray> {
    let Ok(dir) = std::fs::read_dir("/sys/block") else {
        return Vec::new();
    };
    let mut arrays = Vec::new();
    for entry in dir.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("md") {
            continue;
        }
        let base = format!("/sys/block/{}/md", name);
        let mut array = RaidArray::new(&name);
        array.level = util::read_trim(&format!("{}/level", base));
        array.state = util::read_trim(&format!("{}/array_state", base));
        array.disks_total =
            util::read_trim(&format!("{}/raid_disks", base)).and_then(|v| v.parse().ok());
        let degraded = util::read_trim(&format!("{}/degraded", base))
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(0);
        if let Some(total) = array.disks_total {
            array.disks_up = Some(total.saturating_sub(degraded));
        }
        array.size_bytes = util::read_trim(&format!("/sys/block/{}/size", name))
            .and_then(|v| v.parse::<u64>().ok())
            .map(|sectors| sectors.saturating_mul(512));
        if let Some(action) = util::read_trim(&format!("{}/sync_action", base)) {
            if action != "idle" {
                array.sync = Some(action);
            }
        }
        arrays.push(array);
    }
    arrays.sort_by(|a, b| a.name.cmp(&b.name));
    arrays
}

fn hardware_controllers() -> Vec<String> {
    let Some(text) = util::run("lspci", &[]) else {
        return Vec::new();
    };
    text.lines()
        .filter(|line| line.to_lowercase().contains("raid"))
        .map(|line| match line.split_once(": ") {
            Some((_address, description)) => description.trim().to_string(),
            None => line.trim().to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MDSTAT: &str = "Personalities : [raid1] [raid6] [raid5] [raid4]\nmd127 : active raid1 sdb1[1] sda1[0]\n      1046528 blocks super 1.2 [2/2] [UU]\n      bitmap: 0/1 pages [0KB], 65536KB chunk\n\nmd0 : active raid5 sdc1[0] sdd1[1] sde1[2]\n      2095104 blocks super 1.2 level 5, 512k chunk, algorithm 2 [3/2] [UU_]\n      [==>.................]  recovery = 12.3% (129000/1046528) finish=2.0min speed=9000K/sec\n\nunused devices: <none>\n";

    #[test]
    fn parses_arrays_members_and_size() {
        let arrays = parse_mdstat(MDSTAT);
        assert_eq!(arrays.len(), 2);

        let md127 = &arrays[0];
        assert_eq!(md127.name, "md127");
        assert_eq!(md127.level.as_deref(), Some("raid1"));
        assert_eq!(md127.state.as_deref(), Some("active"));
        assert_eq!(md127.members, vec!["sdb1[1]", "sda1[0]"]);
        assert_eq!(md127.size_bytes, Some(1_046_528 * 512));
        assert_eq!((md127.disks_up, md127.disks_total), (Some(2), Some(2)));
        assert_eq!(md127.disk_map.as_deref(), Some("UU"));
        assert!(!md127.degraded());
    }

    #[test]
    fn detects_degraded_array_and_sync_progress() {
        let arrays = parse_mdstat(MDSTAT);
        let md0 = &arrays[1];
        assert_eq!(md0.level.as_deref(), Some("raid5"));
        assert_eq!((md0.disks_up, md0.disks_total), (Some(2), Some(3)));
        assert_eq!(md0.disk_map.as_deref(), Some("UU_"));
        assert!(md0.degraded());
        assert_eq!(md0.sync.as_deref(), Some("recovery 12.3%"));
    }

    #[test]
    fn empty_mdstat_has_no_arrays() {
        assert!(parse_mdstat("Personalities :\nunused devices: <none>\n").is_empty());
    }
}
