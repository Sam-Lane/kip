use std::io::Read;

use anyhow::{bail, Context, Result};
use semver::Version;
use serde::Deserialize;

fn target() -> Result<&'static str> {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return Ok("x86_64-unknown-linux-gnu");
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    return Ok("x86_64-apple-darwin");
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return Ok("aarch64-apple-darwin");
    #[allow(unreachable_code)]
    bail!("kip upgrade is not supported on this platform — re-run install.sh")
}

const REPO: &str = "Sam-Lane/kip";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Deserialize)]
struct Release {
    tag_name: String,
}

pub async fn run() -> Result<()> {
    let client = reqwest::Client::builder()
        .user_agent(format!("kip/{CURRENT_VERSION}"))
        .build()?;

    println!("Checking for updates...");

    let release: Release = client
        .get(format!(
            "https://api.github.com/repos/{REPO}/releases/latest"
        ))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("fetching latest release")?
        .error_for_status()
        .context("GitHub API returned an error")?
        .json()
        .await
        .context("parsing release JSON")?;

    let tag = &release.tag_name;
    let latest = Version::parse(tag.trim_start_matches('v'))
        .with_context(|| format!("parsing latest version tag: {tag}"))?;
    let current = Version::parse(CURRENT_VERSION).expect("CARGO_PKG_VERSION is always valid semver");

    if latest <= current {
        println!("Already up to date (v{current})");
        return Ok(());
    }

    let target = target()?;

    println!("Upgrading v{current} → {tag}");

    let archive_name = format!("kip-{tag}-{target}.tar.gz");
    let url = format!(
        "https://github.com/{REPO}/releases/download/{tag}/{archive_name}"
    );

    let bytes = client
        .get(&url)
        .send()
        .await
        .context("downloading release archive")?
        .error_for_status()
        .with_context(|| format!("downloading {url}"))?
        .bytes()
        .await
        .context("reading archive bytes")?;

    let binary = extract_binary(&bytes).context("extracting binary from archive")?;

    let exe_path = std::env::current_exe()
        .context("finding current executable path")?
        .canonicalize()
        .context("canonicalizing executable path")?;

    let tmp_path = exe_path.with_extension("new");

    std::fs::write(&tmp_path, &binary).context("writing new binary")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755))
            .context("setting binary permissions")?;
    }

    std::fs::rename(&tmp_path, &exe_path).context("replacing binary")?;

    println!("kip upgraded to {tag}");
    Ok(())
}

fn extract_binary(bytes: &[u8]) -> Result<Vec<u8>> {
    let gz = flate2::read::GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(gz);

    for entry in archive.entries().context("reading tar entries")? {
        let mut entry = entry.context("reading tar entry")?;
        let path = entry.path().context("reading entry path")?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();

        if name == "kip" {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf).context("reading kip binary from archive")?;
            return Ok(buf);
        }
    }

    bail!("kip binary not found in archive")
}
