use std::collections::BTreeMap;
use std::ffi::CStr;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::os::raw::{c_char, c_int, c_uint, c_void};

use crate::json::Json;
use crate::output::Output;
use crate::util;

#[repr(C)]
struct IfAddrs {
    ifa_next: *mut IfAddrs,
    ifa_name: *mut c_char,
    ifa_flags: c_uint,
    ifa_addr: *mut u8,
    ifa_netmask: *mut u8,
    ifa_ifu: *mut u8,
    ifa_data: *mut c_void,
}

extern "C" {
    fn getifaddrs(ifap: *mut *mut IfAddrs) -> c_int;
    fn freeifaddrs(ifa: *mut IfAddrs);
    fn inet_ntop(af: c_int, src: *const c_void, dst: *mut c_char, size: u32) -> *const c_char;
}

const IFF_UP: c_uint = 0x1;
const IFF_LOOPBACK: c_uint = 0x8;
const AF_INET: c_int = 2;

#[cfg(target_os = "macos")]
const AF_INET6: c_int = 30;
#[cfg(not(target_os = "macos"))]
const AF_INET6: c_int = 10;

pub struct Addr {
    pub family: &'static str,
    pub address: String,
    pub prefix: Option<u8>,
    pub kind: &'static str,
}

pub struct Interface {
    pub name: String,
    pub up: bool,
    pub loopback: bool,
    pub mac: Option<String>,
    pub addrs: Vec<Addr>,
}

pub struct NetInfo {
    pub interfaces: Vec<Interface>,
    pub gateway: Option<String>,
    pub dns: Vec<String>,
}

impl NetInfo {
    pub fn collect() -> NetInfo {
        let mut interfaces = collect_interfaces();
        interfaces.sort_by(|a, b| {
            a.loopback
                .cmp(&b.loopback)
                .then_with(|| a.name.cmp(&b.name))
        });
        interfaces.retain(|iface| iface.loopback || iface.up || !iface.addrs.is_empty());

        NetInfo {
            interfaces,
            gateway: default_gateway(),
            dns: dns_servers(),
        }
    }

    pub fn render(&self, out: &Output) {
        out.section("Network");
        for iface in &self.interfaces {
            let mut state = if iface.loopback {
                "loopback".to_string()
            } else if iface.up {
                "up".to_string()
            } else {
                "down".to_string()
            };
            if let Some(mac) = &iface.mac {
                state.push_str("  ");
                state.push_str(mac);
            }
            println!("  {} {}", out.dim(&format!("{:<10}", iface.name)), state);
            for addr in &iface.addrs {
                let prefix = addr.prefix.map(|p| format!("/{}", p)).unwrap_or_default();
                println!(
                    "      {:<44} {}",
                    format!("{}{}", addr.address, prefix),
                    out.dim(&format!("[{}]", addr.kind))
                );
            }
        }
        if let Some(gateway) = &self.gateway {
            out.kv("Gateway", gateway);
        }
        if !self.dns.is_empty() {
            out.kv("DNS", &self.dns.join("  "));
        }
    }

    pub fn to_json(&self) -> Json {
        Json::object([
            (
                "interfaces",
                Json::array(self.interfaces.iter().map(|iface| {
                    Json::object([
                        ("name", Json::str(iface.name.clone())),
                        ("up", Json::Bool(iface.up)),
                        ("loopback", Json::Bool(iface.loopback)),
                        ("mac", Json::opt_str(iface.mac.clone())),
                        (
                            "addresses",
                            Json::array(iface.addrs.iter().map(|addr| {
                                Json::object([
                                    ("family", Json::str(addr.family)),
                                    ("address", Json::str(addr.address.clone())),
                                    ("prefix", Json::opt_num(addr.prefix)),
                                    ("kind", Json::str(addr.kind)),
                                ])
                            })),
                        ),
                    ])
                })),
            ),
            ("gateway", Json::opt_str(self.gateway.clone())),
            (
                "dns",
                Json::array(self.dns.iter().map(|d| Json::str(d.clone()))),
            ),
        ])
    }
}

fn collect_interfaces() -> Vec<Interface> {
    let mut raw: *mut IfAddrs = std::ptr::null_mut();
    if unsafe { getifaddrs(&mut raw) } != 0 {
        return Vec::new();
    }

    let mut map: BTreeMap<String, Interface> = BTreeMap::new();
    let mut current = raw;
    while !current.is_null() {
        let item = unsafe { &*current };
        current = item.ifa_next;

        if item.ifa_name.is_null() {
            continue;
        }
        let name = unsafe { CStr::from_ptr(item.ifa_name) }
            .to_string_lossy()
            .into_owned();
        let iface = map.entry(name.clone()).or_insert_with(|| Interface {
            name: name.clone(),
            up: false,
            loopback: false,
            mac: interface_mac(&name),
            addrs: Vec::new(),
        });
        iface.up |= item.ifa_flags & IFF_UP != 0;
        iface.loopback |= item.ifa_flags & IFF_LOOPBACK != 0;

        if item.ifa_addr.is_null() {
            continue;
        }
        let family = unsafe { sock_family(item.ifa_addr) };
        if family != AF_INET && family != AF_INET6 {
            continue;
        }
        let offset = if family == AF_INET { 4 } else { 8 };
        let address = unsafe { ip_to_string(family, item.ifa_addr.add(offset)) };
        let Some(address) = address else {
            continue;
        };
        if iface.addrs.iter().any(|a| a.address == address) {
            continue;
        }
        iface.addrs.push(Addr {
            family: if family == AF_INET { "inet" } else { "inet6" },
            kind: classify(&address),
            prefix: prefix_len(family, item.ifa_netmask),
            address,
        });
    }
    unsafe { freeifaddrs(raw) };

    map.into_values().collect()
}

unsafe fn sock_family(addr: *const u8) -> c_int {
    #[cfg(target_os = "macos")]
    {
        *addr.add(1) as c_int
    }
    #[cfg(not(target_os = "macos"))]
    {
        *(addr as *const u16) as c_int
    }
}

unsafe fn ip_to_string(family: c_int, ip: *const u8) -> Option<String> {
    let mut buffer = [0u8; 64];
    let result = inet_ntop(
        family,
        ip as *const c_void,
        buffer.as_mut_ptr() as *mut c_char,
        buffer.len() as u32,
    );
    if result.is_null() {
        return None;
    }
    Some(CStr::from_ptr(result).to_string_lossy().into_owned())
}

fn prefix_len(family: c_int, netmask: *const u8) -> Option<u8> {
    if netmask.is_null() {
        return None;
    }
    let offset = if family == AF_INET { 4 } else { 8 };
    let len = if family == AF_INET { 4 } else { 16 };
    let bytes = unsafe { std::slice::from_raw_parts(netmask.add(offset), len) };
    let ones: u32 = bytes.iter().map(|b| b.count_ones()).sum();
    Some(ones as u8)
}

fn classify(address: &str) -> &'static str {
    if let Ok(v4) = address.parse::<Ipv4Addr>() {
        if v4.is_loopback() {
            "loopback"
        } else if v4.is_link_local() {
            "link-local"
        } else if v4.is_private() {
            "private"
        } else if v4.is_multicast() {
            "multicast"
        } else {
            "public"
        }
    } else if let Ok(v6) = address.parse::<Ipv6Addr>() {
        let head = v6.segments()[0];
        if v6.is_loopback() {
            "loopback"
        } else if head & 0xffc0 == 0xfe80 {
            "link-local"
        } else if head & 0xfe00 == 0xfc00 {
            "private"
        } else if head & 0xff00 == 0xff00 {
            "multicast"
        } else {
            "public"
        }
    } else {
        "unknown"
    }
}

#[cfg(target_os = "linux")]
fn interface_mac(name: &str) -> Option<String> {
    let mac = util::read_trim(&format!("/sys/class/net/{}/address", name))?;
    if mac == "00:00:00:00:00:00" {
        None
    } else {
        Some(mac)
    }
}

#[cfg(not(target_os = "linux"))]
fn interface_mac(_name: &str) -> Option<String> {
    None
}

fn default_gateway() -> Option<String> {
    if let Some(text) = util::read("/proc/net/route") {
        if let Some(gateway) = parse_proc_route(&text) {
            return Some(gateway);
        }
    }
    let text = util::run("route", &["-n", "get", "default"])?;
    text.lines()
        .find_map(|line| line.strip_prefix("gateway:"))
        .map(|value| value.trim().to_string())
}

fn parse_proc_route(text: &str) -> Option<String> {
    for line in text.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 4 || fields[1] != "00000000" {
            continue;
        }
        let Ok(raw) = u32::from_str_radix(fields[2], 16) else {
            continue;
        };
        let flags = u32::from_str_radix(fields[3], 16).unwrap_or(0);
        if flags & 0x2 == 0 {
            continue;
        }
        let [a, b, c, d] = raw.to_le_bytes();
        return Some(format!("{}.{}.{}.{}", a, b, c, d));
    }
    None
}

fn dns_servers() -> Vec<String> {
    let Some(text) = util::read("/etc/resolv.conf") else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix("nameserver")?;
            let server = rest.trim();
            if server.is_empty() {
                None
            } else {
                Some(server.to_string())
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_addresses() {
        assert_eq!(classify("127.0.0.1"), "loopback");
        assert_eq!(classify("192.168.1.10"), "private");
        assert_eq!(classify("10.0.0.5"), "private");
        assert_eq!(classify("169.254.1.2"), "link-local");
        assert_eq!(classify("8.8.8.8"), "public");
        assert_eq!(classify("::1"), "loopback");
        assert_eq!(classify("fe80::1"), "link-local");
        assert_eq!(classify("fd00::1"), "private");
    }

    #[test]
    fn parses_default_route() {
        let text = "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\neth0\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0\neth0\t0001A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0\n";
        assert_eq!(parse_proc_route(text).as_deref(), Some("192.168.1.1"));
    }

    #[test]
    fn parses_resolv_conf_style_lines() {
        let servers: Vec<String> = "nameserver 1.1.1.1\nnameserver 8.8.8.8\nsearch lan\n"
            .lines()
            .filter_map(|line| {
                line.trim()
                    .strip_prefix("nameserver")
                    .map(|s| s.trim().to_string())
            })
            .filter(|s| !s.is_empty())
            .collect();
        assert_eq!(servers, vec!["1.1.1.1", "8.8.8.8"]);
    }
}
