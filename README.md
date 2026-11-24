# server-info

A tiny, **dependency-free** Rust CLI that reports a server's CPU, memory,
storage, RAID and network information.

```
$ server-info

Server Information

 SYSTEM
  Hostname   web-01
  OS         Debian GNU/Linux 12 (bookworm)
  Kernel     6.1.0-18-amd64
  Arch       x86_64
  Uptime     12d 7h 42m
  Load avg   0.31  0.24  0.19

 CPU
  Model      Intel(R) Xeon(R) E-2288G CPU @ 3.70GHz
  Cores      16 logical / 8 physical
  Max freq   4.90 GHz
  Usage       23.4%  ████░░░░░░░░░░░░░░░░
  Per-core   cpu0    31.2%   cpu1    18.7%   cpu2    22.9%   cpu3    19.1%
             cpu4    27.4%   cpu5    21.3%   cpu6    25.8%   cpu7    14.6%

 MEMORY
  RAM        62.7 GiB total
  Used       18.3 GiB / 62.7 GiB (29.2%)  █████░░░░░░░░░░░░░░░░
  Available  43.9 GiB
  Free       1.2 GiB  (buffers 310.4 MiB / cache 41.5 GiB)
  Swap       0 B / 8.0 GiB (0.0%)

 STORAGE
  Filesystem                 Size      Used     Avail Use%  Mounted on
  /dev/mapper/vg0-root    100.0 GiB  21.4 GiB  73.9 GiB  22%  /
  /dev/nvme1n1p1          931.5 GiB 412.7 GiB 471.2 GiB  47%  /data
  /dev/md0                465.6 GiB 101.3 GiB 340.4 GiB  23%  /srv

 RAID
  md0    raid1   active  [2/2] [UU] 465.6 GiB
        members sda1[0] sdb1[1]
  md1    raid5   active  [4/3] [UUU_] 3.6 TiB  DEGRADED
        members sdc1[0] sdd1[1] sde1[2] sdf1[3]
        progress recovery 43.7%

 NETWORK
  eno1       up  a4:bf:01:23:45:67
      192.168.10.20/24                            [private]
      2001:db8:10::20/64                          [public]
  lo         loopback
      127.0.0.1/8                                 [loopback]
      ::1/128                                     [loopback]
  Gateway    192.168.10.1
  DNS        1.1.1.1  8.8.8.8
```

## Features

- **CPU** — model, logical/physical core count, max frequency, live usage
  (overall and per core) sampled from `/proc/stat`.
- **Memory** — total, used, available, free/buffers/cache and swap from
  `/proc/meminfo`.
- **Storage** — per-filesystem size, used, available and usage percentage from
  `df`, with pseudo filesystems (`proc`, `sysfs`, `tmpfs`, …) filtered out.
- **RAID** — Linux `md` arrays from `/proc/mdstat` (level, state, members,
  size, degraded flag, rebuild progress), plus RAID controllers found via
  `lspci`. Falls back to `/sys/block/md*` when `/proc/mdstat` is missing.
- **Network** — interfaces with IPv4/IPv6 addresses, prefix length and scope
  (public / private / link-local / loopback), MAC address, default gateway and
  DNS servers.
- **JSON output** for scripts and dashboards.
- **No external crates** — only the Rust standard library. The Linux binary is
  a single static executable.

## Install

```sh
git clone https://github.com/megumisodotcom/server-info.git
cd server-info
cargo build --release
sudo install -m 0755 target/release/server-info /usr/local/bin/
```

For a statically linked musl binary (handy for old servers):

```sh
docker run --rm -v "$PWD":/app -w /app rust:alpine \
  sh -c 'cargo build --release --target x86_64-unknown-linux-musl'
# target/x86_64-unknown-linux-musl/release/server-info
```

## Usage

```
Usage: server-info [OPTIONS]

Options:
  -j, --json               Print machine-readable JSON instead of a report
      --cpu-sample-ms <MS> Sampling window used to measure CPU usage [default: 300]
      --color              Force colored output
      --no-color           Disable colored output (also honors NO_COLOR)
  -h, --help               Print help
  -V, --version            Print version
```

Examples:

```sh
server-info                    # human-readable report
server-info --json | jq .      # JSON for automation
watch -n 2 server-info         # live view
```

### JSON output

```json
{
  "system": {
    "hostname": "web-01",
    "os": "Debian GNU/Linux 12 (bookworm)",
    "kernel": "6.1.0-18-amd64",
    "arch": "x86_64",
    "uptime_seconds": 1053720,
    "load_average": [0.31, 0.24, 0.19]
  },
  "cpu": {
    "model": "Intel(R) Xeon(R) E-2288G CPU @ 3.70GHz",
    "logical_cores": 16,
    "physical_cores": 8,
    "max_mhz": 4900.0,
    "usage_percent": 23.4,
    "per_core_percent": [31.2, 18.7, 22.9]
  }
}
```

(`storage`, `raid` and `network` follow the same shape; run
`server-info --json` to see the full document.)

## Platform support

| Feature   | Linux | macOS |
| --------- | ----- | ----- |
| System    | ✅    | ✅    |
| CPU info  | ✅    | ✅ (model/cores; live usage requires `/proc`) |
| Memory    | ✅    | ✅ (via `sysctl` / `vm_stat`) |
| Storage   | ✅    | ✅    |
| RAID (md) | ✅    | —     |
| Network   | ✅    | ✅    |

The primary target is Linux servers. On non-Linux systems the tool still runs
and reports whatever the platform exposes; unavailable sections say so instead
of failing.

## How it works

`server-info` reads kernel interfaces directly instead of shelling out where
possible:

- `/proc/stat`, `/proc/cpuinfo`, `/proc/meminfo`, `/proc/mdstat`,
  `/proc/net/route`, `/proc/mounts` and `/proc/sys/kernel/*`
- `/sys/class/net/*/address`, `/sys/block/md*/md/*`
- `getifaddrs(3)` + `inet_ntop(3)` (via small FFI declarations) for interface
  addresses, so no `ip`/`ifconfig` binary is required

A few helpers (`df`, `lscpu`, `lspci`, `route`, `vm_stat`) are used when the
information is not available through `/proc`/`/sys`.

## Development

```sh
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

## License

MIT — see [LICENSE](LICENSE).
