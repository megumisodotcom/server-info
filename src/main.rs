mod cpu;
mod json;
mod memory;
mod net;
mod output;
mod raid;
mod storage;
mod system;
mod util;

use std::process::ExitCode;

use json::Json;
use output::Output;

const HELP: &str = "\
server-info - show CPU, memory, storage, RAID and network information

Usage: server-info [OPTIONS]

Options:
  -j, --json               Print machine-readable JSON instead of a report
      --cpu-sample-ms <MS> Sampling window used to measure CPU usage [default: 300]
      --color              Force colored output
      --no-color           Disable colored output (also honors NO_COLOR)
  -h, --help               Print help
  -V, --version            Print version

Examples:
  server-info              # human-readable report
  server-info --json      # JSON for scripts or dashboards
  watch -n 2 server-info  # live usage view";

struct Options {
    json: bool,
    color: Option<bool>,
    cpu_sample_ms: u64,
}

enum Action {
    Run(Options),
    Help,
    Version,
}

fn main() -> ExitCode {
    match parse_args(std::env::args().skip(1)) {
        Ok(Action::Run(options)) => {
            run(options);
            ExitCode::SUCCESS
        }
        Ok(Action::Help) => {
            println!("{}", HELP);
            ExitCode::SUCCESS
        }
        Ok(Action::Version) => {
            println!("server-info {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("server-info: {}\n", message);
            eprintln!("{}", HELP);
            ExitCode::from(2)
        }
    }
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Action, String> {
    let mut options = Options {
        json: false,
        color: None,
        cpu_sample_ms: 300,
    };
    let mut args = args;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Action::Help),
            "-V" | "--version" => return Ok(Action::Version),
            "-j" | "--json" => options.json = true,
            "--no-color" => options.color = Some(false),
            "--color" => options.color = Some(true),
            "--cpu-sample-ms" => {
                let value = args.next().ok_or("--cpu-sample-ms requires a value")?;
                let millis = value
                    .parse::<u64>()
                    .map_err(|_| format!("invalid value for --cpu-sample-ms: {}", value))?;
                if !(50..=10_000).contains(&millis) {
                    return Err("--cpu-sample-ms must be between 50 and 10000".to_string());
                }
                options.cpu_sample_ms = millis;
            }
            other => return Err(format!("unknown argument: {}", other)),
        }
    }

    Ok(Action::Run(options))
}

fn run(options: Options) {
    let system = system::SystemInfo::collect();
    let cpu = cpu::collect(options.cpu_sample_ms);
    let memory = memory::MemoryInfo::collect();
    let storage = storage::StorageInfo::collect();
    let raid = raid::RaidInfo::collect();
    let network = net::NetInfo::collect();

    if options.json {
        let json = Json::object([
            ("system", system.to_json()),
            ("cpu", cpu.to_json()),
            ("memory", memory.to_json()),
            ("storage", storage.to_json()),
            ("raid", raid.to_json()),
            ("network", network.to_json()),
        ]);
        println!("{}", json.pretty());
        return;
    }

    let out = match options.color {
        Some(color) => Output::with_color(color),
        None => Output::stdout(),
    };

    out.title("Server Information");
    system.render(&out);
    cpu.render(&out);
    memory.render(&out);
    storage.render(&out);
    raid.render(&out);
    network.render(&out);
    println!();
}
