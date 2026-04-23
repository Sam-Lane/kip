# kip

A small GitOps daemon written in Rust. Watches git repositories for changes on a configured reference or semver tag pattern, pulls those changes, and executes a configured command.

## Build dependencies

Before building, install the following system libraries (Ubuntu/Debian):

```bash
sudo apt install pkg-config libssl-dev libgit2-dev
```

Also requires the Rust toolchain:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
```

## Build

```bash
cargo build --release
```

## Configuration

kip reads config from (in order of precedence):

1. `--config-path <path>` flag
2. `KIP_CONFIG` environment variable
3. `/etc/kip/config.toml` and all `*.toml` files under `/etc/kip/conf.d/`

### Example config

```toml
[[watcher]]
name        = "my-terraform"
repo        = "https://github.com/example/infra"
reference   = "v1.*"           # exact ref, semver wildcard, or regex
dir         = "/var/kip/infra" # where the repo is cloned and stored (required)
working_dir = "/var/kip/infra/env/prod" # cwd for the command (optional, defaults to dir)
interval    = 60               # seconds between checks
command     = "terraform apply --auto-approve"

[watcher.auth]     # optional
username = "git"
token    = "ghp_xxx"
```

## Usage

```bash
kip --config-path ./kip.toml
```
