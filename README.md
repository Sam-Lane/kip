# kip

A GitOps daemon written in Rust. Watches a git repository for changes on a configured branch, tag, or semver pattern — fetches, resets the working tree, and runs a command.

## Install

```sh
curl -sSfL https://raw.githubusercontent.com/Sam-Lane/kip/main/install.sh | sh
```

Override the install directory:

```sh
INSTALL_DIR=~/.local/bin curl -sSfL https://raw.githubusercontent.com/Sam-Lane/kip/main/install.sh | sh
```

## Upgrade

```sh
kip upgrade
```

Fetches the latest release from GitHub, compares it against the running version, and atomically replaces the binary. Re-run `install.sh` on Windows.

## Configuration

kip reads config from (in order of precedence):

1. `--config-path <path>` flag or `KIP_CONFIG` environment variable
2. `/etc/kip/config.toml`
3. All `*.toml` files under `/etc/kip/conf.d/`

```toml
[[watcher]]
name        = "infra"
repo        = "https://github.com/example/infra"
reference   = "v1.*"                          # branch, tag, semver wildcard, or regex
dir         = "/var/kip/infra"                # where the repo is cloned
working_dir = "/var/kip/infra/env/prod"       # cwd for the command (defaults to dir)
interval    = 60                              # seconds between polls
command     = "terraform apply --auto-approve"

[watcher.auth]          # optional — for private repos
username = "git"
token    = "ghp_xxx"
```

### Reference patterns

| Pattern | Matches |
|---|---|
| `main` | exact branch or tag name |
| `v1.2.3` | exact tag |
| `v1.*` | highest semver tag matching the wildcard |
| `release-.*` | latest tag matching the regex |

### Multiple watchers

Add multiple `[[watcher]]` blocks to watch several repos from one config file.

## How it works

On each interval tick kip:

1. Fetches all remotes
2. Resolves the configured reference against the remote tracking ref
3. If the OID changed — resets the working tree to the new commit
4. Runs the configured command in `working_dir`

The command only runs when a new commit is detected. kip logs at `INFO` by default; set `RUST_LOG=debug` to see a heartbeat on every tick.

## Usage

```sh
kip --config-path ./kip.toml   # run daemon with explicit config
kip upgrade                    # self-update to the latest release
```

## Building from source

Requires the Rust toolchain (`rustup.rs`). libgit2 is bundled — no system libraries needed.

```sh
cargo build --release
```
