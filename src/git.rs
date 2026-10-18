use std::path::Path;

use anyhow::{Context, Result};
use git2::{Cred, FetchOptions, RemoteCallbacks, Repository};

use crate::config::AuthConfig;

fn make_fetch_options(auth: Option<&AuthConfig>) -> FetchOptions<'_> {
    let mut callbacks = RemoteCallbacks::new();

    if let Some(auth) = auth {
        let username = auth.username.clone().unwrap_or_default();
        let token = auth.token.clone().unwrap_or_default();
        callbacks.credentials(move |_url, _username, _allowed| {
            Cred::userpass_plaintext(&username, &token)
        });
    }

    let mut opts = FetchOptions::new();
    opts.remote_callbacks(callbacks);
    opts
}

pub fn clone_or_open(url: &str, auth: Option<&AuthConfig>, dir: &Path) -> Result<Repository> {
    if dir.join(".git").exists() {
        Repository::open(dir).with_context(|| format!("opening repo at {}", dir.display()))
    } else {
        let mut builder = git2::build::RepoBuilder::new();
        builder.fetch_options(make_fetch_options(auth));
        builder
            .clone(url, dir)
            .with_context(|| format!("cloning {} into {}", url, dir.display()))
    }
}

pub fn fetch_all(repo: &Repository, auth: Option<&AuthConfig>) -> Result<()> {
    let remote_names = repo
        .remotes()
        .context("listing remotes")?
        .iter()
        .flatten()
        .map(|s| s.to_owned())
        .collect::<Vec<_>>();

    for name in remote_names {
        let mut remote = repo.find_remote(&name)?;
        remote
            .fetch(&[] as &[&str], Some(&mut make_fetch_options(auth)), None)
            .with_context(|| format!("fetching remote {name}"))?;
    }
    Ok(())
}

pub fn list_tags(repo: &Repository) -> Result<Vec<String>> {
    let tags = repo.tag_names(None).context("listing tags")?;
    Ok(tags.iter().flatten().map(|s| s.to_owned()).collect())
}

pub fn resolve_oid(repo: &Repository, refname: &str) -> Result<git2::Oid> {
    let obj = repo
        .revparse_single(refname)
        .with_context(|| format!("resolving ref {refname}"))?;
    let commit = obj.peel_to_commit().context("peeling to commit")?;
    Ok(commit.id())
}
