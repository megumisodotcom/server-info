use std::collections::BTreeMap;

use crate::json::Json;
use crate::output::Output;
use crate::util;

pub struct FsEntry {
    pub device: String,
    pub mount: String,
    pub fstype: Option<String>,
    pub total: u64,
    pub used: u64,
    pub available: u64,
}

impl FsEntry {
    pub fn use_percent(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.used as f64 / self.total as f64 * 100.0
        }
    }
}

pub struct StorageInfo {
    pub entries: Vec<FsEntry>,
}

impl StorageInfo {
    pub fn collect() -> StorageInfo {
        let fstypes = fstype_map();
        let mut entries = util::run("df", &["-P", "-k"])
            .map(|text| parse_df(&text, &fstypes))
            .unwrap_or_default();
        entries.retain(|entry| !is_file_mount(&entry.mount));
        StorageInfo { entries }
    }

    pub fn render(&self, out: &Output) {
        out.section("Storage");
        if self.entries.is_empty() {
            out.note("no filesystems reported");
            return;
        }

        let device_width = self
            .entries
            .iter()
            .map(|e| e.device.chars().count())
            .chain(std::iter::once("Filesystem".len()))
            .max()
            .unwrap_or(10)
            .min(24);
        let header = format!(
            "{:<width$} {:>9} {:>9} {:>9} {:>4}  {}",
            "Filesystem",
            "Size",
            "Used",
            "Avail",
            "Use%",
            "Mounted on",
            width = device_width
        );
        println!("  {}", out.dim(&header));
        for entry in &self.entries {
            let device = ellipsize(&entry.device, device_width);
            println!(
                "  {:<width$} {:>9} {:>9} {:>9} {}  {}",
                device,
                util::human_bytes(entry.total),
                util::human_bytes(entry.used),
                util::human_bytes(entry.available),
                out.pct_cell(entry.use_percent(), 4),
                entry.mount,
                width = device_width
            );
        }
    }

    pub fn to_json(&self) -> Json {
        Json::array(self.entries.iter().map(|entry| {
            Json::object([
                ("device", Json::str(entry.device.clone())),
                ("mount", Json::str(entry.mount.clone())),
                ("filesystem", Json::opt_str(entry.fstype.clone())),
                ("total_bytes", Json::num(entry.total)),
                ("used_bytes", Json::num(entry.used)),
                ("available_bytes", Json::num(entry.available)),
                (
                    "use_percent",
                    Json::num((entry.use_percent() * 10.0).round() / 10.0),
                ),
            ])
        }))
    }
}

fn is_file_mount(mount: &str) -> bool {
    std::fs::metadata(mount)
        .map(|meta| meta.is_file())
        .unwrap_or(false)
}

fn ellipsize(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if count <= width {
        return text.to_string();
    }
    let tail: String = text
        .chars()
        .skip(count.saturating_sub(width.saturating_sub(1)))
        .collect();
    format!("…{}", tail)
}

fn fstype_map() -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let Some(text) = util::read("/proc/mounts") else {
        return map;
    };
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let (Some(_device), Some(mount), Some(fstype)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        map.insert(mount.to_string(), fstype.to_string());
    }
    map
}

fn parse_df(text: &str, fstypes: &BTreeMap<String, String>) -> Vec<FsEntry> {
    let mut entries = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 6 {
            continue;
        }
        let (Ok(total), Ok(used), Ok(available)) = (
            fields[1].parse::<u64>(),
            fields[2].parse::<u64>(),
            fields[3].parse::<u64>(),
        ) else {
            continue;
        };
        if total == 0 {
            continue;
        }
        let device = fields[0].to_string();
        let mount = fields[5..].join(" ");
        let fstype = fstypes.get(&mount).cloned();

        let real = match &fstype {
            Some(fs) => !is_pseudo_filesystem(fs),
            None => device.starts_with("/dev/"),
        };
        if !real {
            continue;
        }

        entries.push(FsEntry {
            device,
            mount,
            fstype,
            total: total * 1024,
            used: used * 1024,
            available: available * 1024,
        });
    }
    entries.sort_by(|a, b| a.mount.cmp(&b.mount));
    entries
}

fn is_pseudo_filesystem(fstype: &str) -> bool {
    matches!(
        fstype,
        "proc"
            | "sysfs"
            | "devtmpfs"
            | "devpts"
            | "tmpfs"
            | "ramfs"
            | "cgroup"
            | "cgroup2"
            | "pstore"
            | "securityfs"
            | "debugfs"
            | "tracefs"
            | "configfs"
            | "fusectl"
            | "mqueue"
            | "hugetlbfs"
            | "binfmt_misc"
            | "autofs"
            | "efivarfs"
            | "rpc_pipefs"
            | "nsfs"
            | "bpf"
            | "squashfs"
            | "fuse.gvfsd-fuse"
            | "fuse.portal"
            | "fuse.lxcfs"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fstypes() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("/".to_string(), "ext4".to_string()),
            ("/boot".to_string(), "vfat".to_string()),
            ("/proc".to_string(), "proc".to_string()),
            ("/run".to_string(), "tmpfs".to_string()),
        ])
    }

    #[test]
    fn parses_df_and_skips_pseudo_filesystems() {
        let text = "Filesystem     1024-blocks      Used Available Capacity Mounted on\n/dev/sda2         10240000   2048000   8192000      20% /\n/dev/sda1           524288    104857    419431      20% /boot\nproc                     0         0         0        - /proc\ntmpfs               102400         0    102400       0% /run\n";
        let entries = parse_df(text, &fstypes());
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].mount, "/");
        assert_eq!(entries[0].total, 10_240_000 * 1024);
        assert_eq!(entries[1].fstype.as_deref(), Some("vfat"));
    }

    #[test]
    fn calculates_use_percent() {
        let entry = FsEntry {
            device: "/dev/sda1".into(),
            mount: "/".into(),
            fstype: Some("ext4".into()),
            total: 1000,
            used: 250,
            available: 750,
        };
        assert!((entry.use_percent() - 25.0).abs() < f64::EPSILON);
    }

    #[test]
    fn ellipsizes_long_device_paths() {
        assert_eq!(ellipsize("/dev/mapper/long-name", 8), "…ng-name");
        assert_eq!(ellipsize("/dev/sda1", 24), "/dev/sda1");
    }
}
