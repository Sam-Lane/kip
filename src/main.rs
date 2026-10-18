mod config;
mod git;
mod matcher;
mod reconciler;
mod runner;
mod upgrade;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::info;

#[derive(Parser)]
#[command(name = "kip", about = "GitOps daemon — watches git refs and runs commands")]
struct Cli {
    /// Path to a kip config file (overrides /etc/kip defaults and KIP_CONFIG)
    #[arg(long, env = "KIP_CONFIG", value_name = "PATH")]
    config_path: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Download and install the latest kip release
    Upgrade,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "kip=info".into()),
        )
        .init();

    let cli = Cli::parse();

    if let Some(Commands::Upgrade) = cli.command {
        return upgrade::run().await;
    }

    let cfg = config::load_config(cli.config_path)?;

    if cfg.watcher.is_empty() {
        anyhow::bail!("no watchers configured");
    }

    info!("starting {} watcher(s)", cfg.watcher.len());

    let mut handles = Vec::new();
    for watcher in cfg.watcher {
        let name = watcher.name.clone();
        let handle = tokio::spawn(async move {
            if let Err(e) = reconciler::reconcile(watcher).await {
                tracing::error!(watcher = %name, "reconciler exited with error: {e:#}");
            }
        });
        handles.push(handle);
    }

    tokio::signal::ctrl_c().await?;
    info!("shutting down");
    for handle in handles {
        handle.abort();
    }

    Ok(())
}
