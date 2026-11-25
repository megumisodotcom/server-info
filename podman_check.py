#!/usr/bin/env python3
"""podman-check - install Podman and verify that containers can actually run.

The tool is dependency-free (Python standard library only). It detects the
host platform, installs Podman with the native package manager when it is
missing, and then proves the installation works by running a real container.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import shutil
import subprocess
import sys
from dataclasses import asdict, dataclass, field
from typing import Iterable, Sequence

VERSION = "0.1.0"
DEFAULT_IMAGE = "quay.io/podman/hello:latest"
DEFAULT_TIMEOUT = 300
DEFAULT_MACHINE = "podman-machine-default"

INSTALLERS = (
    ("apt-get", [["apt-get", "update"], ["apt-get", "install", "-y", "podman"]]),
    ("dnf", [["dnf", "install", "-y", "podman"]]),
    ("yum", [["yum", "install", "-y", "podman"]]),
    ("zypper", [["zypper", "--non-interactive", "install", "podman"]]),
    ("pacman", [["pacman", "-Sy", "--noconfirm", "podman"]]),
    ("apk", [["apk", "add", "podman"]]),
    ("brew", [["brew", "install", "podman"]]),
)

HELP_EPILOG = """\
examples:
  ./podman_check.py                 # install Podman if needed, then run a test container
  ./podman_check.py --no-install    # only test the existing installation
  ./podman_check.py --dry-run       # show what would be installed/executed
  ./podman_check.py --json          # machine-readable result
"""


@dataclass
class Step:
    name: str
    status: str = "pending"  # ok | fail | skip
    detail: str = ""
    command: list[str] = field(default_factory=list)


class Checker:
    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args
        self.steps: list[Step] = []
        self.installer: str | None = None
        self.install_commands: list[list[str]] = []

    # ------------------------------------------------------------------ utils

    def run(self, cmd: Sequence[str], *, timeout: int | None = None) -> tuple[int, str]:
        """Run a command, returning (exit_code, combined_output)."""
        timeout = timeout or self.args.timeout
        try:
            proc = subprocess.run(
                list(cmd),
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
                timeout=timeout,
            )
            return proc.returncode, (proc.stdout or "").strip()
        except FileNotFoundError:
            return 127, f"command not found: {cmd[0]}"
        except subprocess.TimeoutExpired as exc:
            output = exc.output or ""
            if isinstance(output, bytes):
                output = output.decode(errors="replace")
            return 124, f"timed out after {timeout}s\n{output}".strip()
        except OSError as exc:
            return 1, str(exc)

    def sudo_prefix(self) -> list[str]:
        if self.args.no_sudo or (hasattr(os, "geteuid") and os.geteuid() == 0):
            return []
        prefix = ["sudo"]
        if not sys.stdin.isatty():
            prefix.append("-n")
        return prefix

    def record(self, step: Step) -> Step:
        self.steps.append(step)
        return step

    def fail(self, name: str, detail: str, command: Iterable[str] = ()) -> Step:
        return self.record(Step(name, "fail", detail, list(command)))

    def ok(self, name: str, detail: str = "", command: Iterable[str] = ()) -> Step:
        return self.record(Step(name, "ok", detail, list(command)))

    def skip(self, name: str, detail: str, command: Iterable[str] = ()) -> Step:
        return self.record(Step(name, "skip", detail, list(command)))

    # ------------------------------------------------------------------ steps

    def detect_installer(self) -> None:
        for name, commands in INSTALLERS:
            if shutil.which(name):
                self.installer = name
                prefix = [] if name == "brew" else self.sudo_prefix()
                self.install_commands = [prefix + cmd for cmd in commands]
                return

    def check_platform(self) -> bool:
        system = platform.system()
        if system not in ("Linux", "Darwin"):
            self.fail("platform", f"unsupported operating system: {system}")
            return False
        os_name = platform.platform(aliased=True)
        self.ok("platform", f"{os_name} ({platform.machine()})")
        return True

    def podman_version(self) -> str | None:
        code, out = self.run(["podman", "--version"])
        if code == 0:
            return out.splitlines()[0] if out else "podman"
        return None

    def ensure_podman(self) -> bool:
        version = self.podman_version()
        if version:
            self.ok("podman installed", version)
            return True

        if self.args.no_install:
            self.fail("podman installed", "podman not found (--no-install given)")
            return False

        if shutil.which("podman") is None:
            self.detect_installer()
            if self.installer is None:
                self.fail(
                    "podman install",
                    "no supported package manager found "
                    "(apt-get, dnf, yum, zypper, pacman, apk, brew)",
                )
                return False

        for cmd in self.install_commands:
            code, out = self.run(cmd)
            if code != 0:
                self.fail("podman install", tail(out), cmd)
                return False

        version = self.podman_version()
        if not version:
            self.fail("podman install", "installation finished but podman is still missing")
            return False
        self.ok("podman install", version, self.install_commands[-1])
        return True

    def start_machine_if_needed(self) -> None:
        """On macOS Podman runs inside a VM; make sure it is up."""
        if platform.system() != "Darwin":
            return
        if self.args.dry_run:
            self.skip("podman machine", f"would run: podman machine start {DEFAULT_MACHINE}")
            return
        code, out = self.run(["podman", "info"], timeout=min(self.args.timeout, 60))
        if code == 0:
            self.ok("podman machine", "running")
            return
        self.run(["podman", "machine", "init"], timeout=self.args.timeout)
        code, out = self.run(["podman", "machine", "start", DEFAULT_MACHINE])
        if code != 0:
            self.fail("podman machine", tail(out))
            return
        self.ok("podman machine", "started")

    def check_info(self) -> bool:
        code, out = self.run(["podman", "info", "--format", "json"])
        if code != 0:
            self.fail("podman info", tail(out))
            return False
        try:
            info = json.loads(out)
        except json.JSONDecodeError:
            self.ok("podman info", "responded")
            return True
        version = info.get("version", {}) if isinstance(info, dict) else {}
        runtime = info.get("host", {}).get("security", {}).get("rootless")
        detail = str(version.get("version", "unknown"))
        if runtime is not None:
            detail += f", rootless={str(runtime).lower()}"
        self.ok("podman info", detail)
        return True

    def run_container(self) -> bool:
        cmd = ["podman", "run", "--rm", self.args.image]
        if self.args.dry_run:
            self.skip("container run", f"would run: {' '.join(cmd)}", cmd)
            return False
        code, out = self.run(cmd)
        if code != 0:
            self.fail("container run", f"{' '.join(cmd)}\n{tail(out)}", cmd)
            return False
        self.ok("container run", f"{self.args.image}: {first_line(out) or 'ok'}", cmd)
        return True

    # ------------------------------------------------------------------- main

    def plan(self) -> None:
        """Record the steps that a real run would perform."""
        if shutil.which("podman"):
            self.skip("podman installed", "podman already present (dry run)")
        else:
            self.detect_installer()
            if self.installer is None:
                self.fail(
                    "podman install",
                    "no supported package manager found "
                    "(apt-get, dnf, yum, zypper, pacman, apk, brew)",
                )
                return
            for cmd in self.install_commands:
                self.skip("podman install", f"would run: {' '.join(cmd)}", cmd)
        self.start_machine_if_needed()
        self.skip(
            "podman info",
            "would run: podman info --format json",
            ["podman", "info", "--format", "json"],
        )
        self.run_container()

    def execute(self) -> bool:
        if not self.check_platform():
            return False
        if self.args.dry_run:
            self.plan()
            return False
        if not self.ensure_podman():
            return False
        self.start_machine_if_needed()
        if not self.check_info():
            return False
        return self.run_container()


def tail(text: str, limit: int = 1200) -> str:
    text = text.strip()
    return text if len(text) <= limit else "…" + text[-limit:]


def first_line(text: str) -> str:
    for line in text.splitlines():
        line = line.strip()
        if line:
            return line
    return ""


def use_color(args: argparse.Namespace) -> bool:
    if args.color:
        return True
    if args.no_color or os.environ.get("NO_COLOR"):
        return False
    return sys.stdout.isatty()


def render(checker: Checker, args: argparse.Namespace) -> None:
    color = use_color(args)
    marks = {
        "ok": ("\033[32m[ ok ]\033[0m", "[ ok ]"),
        "fail": ("\033[31m[fail]\033[0m", "[fail]"),
        "skip": ("\033[33m[skip]\033[0m", "[skip]"),
    }
    failed = any(step.status == "fail" for step in checker.steps)

    print("Podman container check\n")
    print(" SYSTEM")
    print(f"  OS      {platform.platform(aliased=True)}")
    print(f"  Arch    {platform.machine()}")
    print(f"  Python  {platform.python_version()}")
    print()
    print(" STEPS")
    width = max(len(step.name) for step in checker.steps) if checker.steps else 10
    for step in checker.steps:
        mark = marks[step.status][0 if color else 1]
        line = f"  {mark} {step.name.ljust(width)}"
        if step.detail:
            line += f"  {step.detail}"
        print(line)
    print()
    if args.dry_run:
        print(" Result  DRY RUN (nothing was executed)")
    else:
        print(" Result  " + ("FAIL" if failed else "PASS"))
    print()


def emit_json(checker: Checker, args: argparse.Namespace) -> None:
    failed = any(step.status == "fail" for step in checker.steps)
    document = {
        "tool": "podman-check",
        "version": VERSION,
        "platform": {
            "system": platform.system(),
            "release": platform.release(),
            "machine": platform.machine(),
        },
        "image": args.image,
        "dry_run": args.dry_run,
        "result": "dry-run" if args.dry_run else ("fail" if failed else "pass"),
        "steps": [asdict(step) for step in checker.steps],
    }
    print(json.dumps(document, indent=2, ensure_ascii=False))


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="podman-check",
        description="Install Podman and verify that containers can run.",
        epilog=HELP_EPILOG,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("-V", "--version", action="version", version=f"podman-check {VERSION}")
    parser.add_argument("-j", "--json", action="store_true", help="print a JSON report")
    parser.add_argument("--no-install", action="store_true", help="do not install Podman if missing")
    parser.add_argument("--dry-run", action="store_true", help="show commands without running them")
    parser.add_argument(
        "--image",
        default=DEFAULT_IMAGE,
        help=f"container image used for the test run [default: {DEFAULT_IMAGE}]",
    )
    parser.add_argument(
        "--timeout",
        type=int,
        default=DEFAULT_TIMEOUT,
        metavar="SECONDS",
        help=f"timeout for install/test commands [default: {DEFAULT_TIMEOUT}]",
    )
    parser.add_argument("--no-sudo", action="store_true", help="never use sudo for installation")
    parser.add_argument("--color", action="store_true", help="force colored output")
    parser.add_argument("--no-color", action="store_true", help="disable colored output")
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    checker = Checker(args)
    passed = checker.execute()

    if args.json:
        emit_json(checker, args)
    else:
        render(checker, args)

    if args.dry_run:
        failed = any(step.status == "fail" for step in checker.steps)
        return 1 if failed else 0
    return 0 if passed else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        print("\ninterrupted", file=sys.stderr)
        sys.exit(130)
