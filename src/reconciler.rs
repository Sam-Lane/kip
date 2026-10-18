use std::time::Duration;

use anyhow::Result;
use git2::Oid;
use tokio::time;
use tracing::{debug, error, info, warn};

use crate::config::WatcherConfig;
use crate::git::{clone_or_open, fetch_all, reset_hard};
use crate::matcher::resolve_best_ref;
use crate::runner::run_command;

pub async fn reconcile(watcher: WatcherConfig) -> Result<()> {
    let dir = &watcher.dir;
    let working_dir = watcher.working_dir.as_deref().unwrap_or(dir);

    std::fs::create_dir_all(dir)?;

    info!(watcher = %watcher.name, "starting reconciler");

    let repo = clone_or_open(&watcher.repo, watcher.auth.as_ref(), dir)?;
    let mut last_oid: Option<Oid> = None;
    let mut interval = time::interval(Duration::from_secs(watcher.interval));

    loop {
        interval.tick().await;

        if let Err(e) = fetch_all(&repo, watcher.auth.as_ref()) {
            warn!(watcher = %watcher.name, "fetch failed: {e:#}");
            continue;
        }

        let (ref_name, oid) = match resolve_best_ref(&repo, &watcher.reference) {
            Ok(r) => r,
            Err(e) => {
                warn!(watcher = %watcher.name, "ref resolution failed: {e:#}");
                continue;
            }
        };

        if Some(oid) == last_oid {
            debug!(watcher = %watcher.name, oid = %oid, "no change");
            continue;
        }

        let prev = last_oid.map(|o| o.to_string()).unwrap_or_else(|| "(none)".into());
        info!(
            watcher = %watcher.name,
            reference = %ref_name,
            from = %prev,
            to = %oid,
            "change detected, running command"
        );

        if let Err(e) = reset_hard(&repo, oid) {
            error!(watcher = %watcher.name, "reset failed: {e:#}");
            continue;
        }

        if let Err(e) = run_command(&watcher.command, working_dir).await {
            error!(watcher = %watcher.name, "command failed: {e:#}");
        } else {
            info!(watcher = %watcher.name, "command completed successfully");
            last_oid = Some(oid);
        }
    }
}
