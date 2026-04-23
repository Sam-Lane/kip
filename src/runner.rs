use std::path::Path;
use std::process::Stdio;

use anyhow::{bail, Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::{info, warn};

pub async fn run_command(command: &str, working_dir: &Path) -> Result<()> {
    let parts = shell_words(command);
    let (program, args) = parts
        .split_first()
        .context("command string is empty")?;

    let mut child = tokio::process::Command::new(program)
        .args(args)
        .current_dir(working_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("spawning command: {command}"))?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let mut stdout_lines = BufReader::new(stdout).lines();
    let mut stderr_lines = BufReader::new(stderr).lines();

    loop {
        tokio::select! {
            line = stdout_lines.next_line() => {
                match line? {
                    Some(l) => info!(target: "runner", "{l}"),
                    None => break,
                }
            }
            line = stderr_lines.next_line() => {
                match line? {
                    Some(l) => warn!(target: "runner", "{l}"),
                    None => {}
                }
            }
        }
    }

    let status = child.wait().await.context("waiting for command")?;
    if !status.success() {
        bail!("command exited with status {status}");
    }
    Ok(())
}

fn shell_words(s: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = ' ';

    for ch in s.chars() {
        match ch {
            '"' | '\'' if !in_quotes => {
                in_quotes = true;
                quote_char = ch;
            }
            c if in_quotes && c == quote_char => {
                in_quotes = false;
            }
            ' ' | '\t' if !in_quotes => {
                if !current.is_empty() {
                    words.push(current.clone());
                    current.clear();
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}
