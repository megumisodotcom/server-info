# podman-check

A small, **dependency-free** Python CLI that installs [Podman](https://podman.io/)
on the current machine and proves that containers can actually run.

```
$ ./podman_check.py

Podman container check

 SYSTEM
  OS      Linux-6.8.0-45-generic-x86_64-with-glibc2.39
  Arch    x86_64
  Python  3.12.3

 STEPS
  [ ok ] platform         Linux-6.8.0-45-generic-x86_64-with-glibc2.39 (x86_64)
  [ ok ] podman installed podman version 5.2.3
  [ ok ] podman info      5.2.3, rootless=true
  [ ok ] container run   quay.io/podman/hello:latest: !... Hello Podman World ...!

 Result  PASS
```

## What it does

1. Detects the platform (Linux or macOS).
2. Checks whether `podman` is installed.
3. If it is missing, installs it with the native package manager
   (`apt-get`, `dnf`, `yum`, `zypper`, `pacman`, `apk`, or Homebrew).
   On macOS the Podman VM (`podman machine`) is initialized and started.
4. Verifies the daemon/VM with `podman info`.
5. Runs a real container (`quay.io/podman/hello` by default) and reports the
   result.

The exit status is `0` only when the container test succeeds, so it can be
used directly in CI or provisioning scripts.

## Usage

```
Usage: podman-check [OPTIONS]

Options:
  -j, --json            Print a JSON report
  --no-install          Do not install Podman if it is missing
  --dry-run             Show the install/test commands without running them
  --image IMAGE         Image used for the test run [default: quay.io/podman/hello:latest]
  --timeout SECONDS     Timeout for install/test commands [default: 300]
  --no-sudo             Never use sudo for installation
  --color               Force colored output
  --no-color            Disable colored output (also honors NO_COLOR)
  -h, --help            Print help
  -V, --version         Print version
```

Examples:

```sh
./podman_check.py                  # install if needed, then run a test container
./podman_check.py --no-install     # test an existing installation
./podman_check.py --dry-run        # only print what would happen
./podman_check.py --json | jq .    # machine-readable report
./podman_check.py --image docker.io/library/alpine:latest
```

## Requirements

- Python 3.9+ (standard library only)
- Root or `sudo` for installing packages on Linux
- Homebrew on macOS

## Development

```sh
python3 podman_check.py --help
python3 podman_check.py --dry-run
python3 -m py_compile podman_check.py
python3 -m unittest discover -s tests
```

## License

MIT — see [LICENSE](LICENSE).
