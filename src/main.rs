mod cpu;
mod json;
mod memory;
mod net;
mod output;
mod raid;
mod storage;
mod system;
mod util;

use std::io::IsTerminal;
use std::os::unix::process::CommandExt;
use std::process::{Command, ExitCode};

use json::Json;
use output::Output;

const DEFAULT_SHELL: &str = "/bin/sh";

const HELP: &str = "\
server-info - show CPU, memory, storage, RAID and network information

After printing the report, server-info replaces itself with an interactive
shell (/bin/sh by default) so the session can continue on the same host.
This happens automatically only when stdin and stdout are terminals; use
--shell to force it, or --no-shell to turn it off.

Usage: server-info [OPTIONS]

Options:
  -j, --json               Print machine-readable JSON instead of a report
      --cpu-sample-ms <MS> Sampling window used to measure CPU usage [default: 300]
      --shell [PROGRAM]    Start an interactive shell after the report
                           (default: /bin/sh)
      --no-shell           Do not start a shell after the report
      --color              Force colored output
      --no-color           Disable colored output (also honors NO_COLOR)
  -h, --help               Print help
  -V, --version            Print version

Examples:
  server-info              # report, then open a shell
  server-info --no-shell   # report only
  server-info --shell bash # report, then open bash
  server-info --json       # JSON for scripts (no shell when piped)";

#[derive(Debug, PartialEq)]
enum ShellMode {
    /// Open a shell when the session is interactive.
    Default,
    /// Never open a shell.
    Disabled,
    /// Always open this shell, even for non-interactive sessions.
    Program(String),
}

struct Options {
    json: bool,
    color: Option<bool>,
    cpu_sample_ms: u64,
    shell: ShellMode,
}

enum Action {
    Run(Options),
    Help,
    Version,
}

fn main() -> ExitCode {
    match parse_args(std::env::args().skip(1)) {
        Ok(Action::Run(options)) => run(options),
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
        shell: ShellMode::Default,
    };
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Action::Help),
            "-V" | "--version" => return Ok(Action::Version),
            "-j" | "--json" => options.json = true,
            "--no-color" => options.color = Some(false),
            "--color" => options.color = Some(true),
            "--no-shell" => options.shell = ShellMode::Disabled,
            "--shell" => {
                let program = match args.peek() {
                    Some(next) if !next.starts_with('-') => args.next().unwrap(),
                    _ => DEFAULT_SHELL.to_string(),
                };
                options.shell = ShellMode::Program(program);
            }
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

fn run(options: Options) -> ExitCode {
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
    } else {
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

    start_shell(&options.shell).unwrap_or(ExitCode::SUCCESS)
}

/// Replace the current process with an interactive shell.
///
/// Returns `None` when no shell should be started, or an exit code when the
/// shell could not be executed. On success this function never returns.
fn start_shell(mode: &ShellMode) -> Option<ExitCode> {
    let program = match mode {
        ShellMode::Disabled => return None,
        ShellMode::Program(program) => program.clone(),
        ShellMode::Default => {
            let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
            if !interactive {
                return None;
            }
            DEFAULT_SHELL.to_string()
        }
    };

    println!("Starting {} ... (exit to return)", program);
    let error = Command::new(&program).exec();
    eprintln!("server-info: failed to start {}: {}", program, error);
    Some(ExitCode::from(127))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Options {
        match parse_args(args.iter().map(|s| s.to_string())) {
            Ok(Action::Run(options)) => options,
            _ => panic!("expected Run action"),
        }
    }

    #[test]
    fn shell_is_enabled_by_default() {
        assert_eq!(parse(&[]).shell, ShellMode::Default);
    }

    #[test]
    fn no_shell_disables_shell() {
        assert_eq!(parse(&["--no-shell"]).shell, ShellMode::Disabled);
    }

    #[test]
    fn shell_uses_bin_sh_without_argument() {
        assert_eq!(
            parse(&["--shell"]).shell,
            ShellMode::Program("/bin/sh".to_string())
        );
    }

    #[test]
    fn shell_accepts_program_name() {
        assert_eq!(
            parse(&["--shell", "bash"]).shell,
            ShellMode::Program("bash".to_string())
        );
    }

    #[test]
    fn shell_does_not_swallow_flags() {
        let options = parse(&["--shell", "--no-color"]);
        assert_eq!(options.shell, ShellMode::Program("/bin/sh".to_string()));
        assert_eq!(options.color, Some(false));
    }

    #[test]
    fn rejects_unknown_arguments() {
        assert!(parse_args(["--bogus".to_string()].into_iter()).is_err());
    }
}
