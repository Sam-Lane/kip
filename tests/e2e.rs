use std::path::Path;
use std::time::Duration;

use git2::{Repository, Signature};
use kip::config::WatcherConfig;
use kip::reconciler::reconcile;
use tempfile::TempDir;
use tokio::time::{sleep, timeout};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn init_tracing() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
}

fn sig() -> Signature<'static> {
    Signature::now("kip-test", "test@kip").unwrap()
}

/// Initialise a bare "remote" repo with an initial commit + lightweight tag.
fn setup_repo(remote: &Path, tag: &str) {
    Repository::init_bare(remote).unwrap();

    let work_tmp = TempDir::new().unwrap();
    // Init a working repo (not clone — bare is empty, nothing to clone)
    let work = Repository::init(work_tmp.path()).unwrap();

    let mut index = work.index().unwrap();
    let tree_oid = index.write_tree().unwrap();
    let tree = work.find_tree(tree_oid).unwrap();
    let s = sig();
    work.commit(Some("HEAD"), &s, &s, "init", &tree, &[]).unwrap();

    let head = work.head().unwrap().peel_to_commit().unwrap();
    work.tag_lightweight(tag, head.as_object(), false).unwrap();

    work.remote("origin", &format!("file://{}", remote.display())).unwrap();
    let mut origin = work.find_remote("origin").unwrap();
    origin
        .push(
            &["refs/heads/master:refs/heads/master", &format!("refs/tags/{tag}:refs/tags/{tag}")],
            None,
        )
        .unwrap();
}

/// Push a new empty commit + tag onto an existing bare remote.
fn push_tag(remote: &Path, tag: &str) {
    let work_tmp = TempDir::new().unwrap();
    let work = Repository::clone(&format!("file://{}", remote.display()), work_tmp.path()).unwrap();

    let parent = work.head().unwrap().peel_to_commit().unwrap();
    let tree = parent.tree().unwrap();
    let s = sig();
    work.commit(Some("HEAD"), &s, &s, tag, &tree, &[&parent]).unwrap();

    let head = work.head().unwrap().peel_to_commit().unwrap();
    work.tag_lightweight(tag, head.as_object(), false).unwrap();

    let mut origin = work.find_remote("origin").unwrap();
    origin
        .push(
            &["refs/heads/master:refs/heads/master", &format!("refs/tags/{tag}:refs/tags/{tag}")],
            None,
        )
        .unwrap();
}

/// Poll a file every 100ms until it contains `needle`, or panic after `secs`.
async fn wait_for_line(path: &Path, needle: &str, secs: u64) {
    timeout(Duration::from_secs(secs), async {
        loop {
            if let Ok(contents) = std::fs::read_to_string(path) {
                if contents.contains(needle) {
                    return;
                }
            }
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {needle:?} in {}", path.display()));
}

fn watcher(repo_path: &Path, clone_dir: &Path, _sentinel: &Path, tag_pattern: &str, cmd: &str) -> WatcherConfig {
    WatcherConfig {
        name: "test".into(),
        repo: format!("file://{}", repo_path.display()),
        reference: tag_pattern.into(),
        dir: clone_dir.to_path_buf(),
        working_dir: None,
        interval: 1,
        command: cmd.to_string(),
        auth: None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn first_run_fires_command() {
    init_tracing();
    let remote = TempDir::new().unwrap();
    let clone = TempDir::new().unwrap();
    let out_dir = TempDir::new().unwrap();
    let sentinel = out_dir.path().join("out");

    setup_repo(remote.path(), "v1.0.0");

    let cmd = format!("sh -c 'echo v1.0.0 >> {}'", sentinel.display());
    let w = watcher(remote.path(), clone.path(), &sentinel, "v1.*", &cmd);
    let handle = tokio::spawn(reconcile(w));

    wait_for_line(&sentinel, "v1.0.0", 10).await;
    handle.abort();
}

#[tokio::test]
async fn tag_advance_triggers_rerun() {
    init_tracing();
    let remote = TempDir::new().unwrap();
    let clone = TempDir::new().unwrap();
    let out_dir = TempDir::new().unwrap();
    let sentinel = out_dir.path().join("out");

    setup_repo(remote.path(), "v1.0.0");

    let cmd = format!("sh -c 'echo ran >> {}'", sentinel.display());
    let w = watcher(remote.path(), clone.path(), &sentinel, "v1.*", &cmd);
    let handle = tokio::spawn(reconcile(w));

    wait_for_line(&sentinel, "ran", 10).await;

    push_tag(remote.path(), "v1.1.0");
    sleep(Duration::from_secs(3)).await;

    let lines = std::fs::read_to_string(&sentinel).unwrap();
    assert_eq!(lines.lines().count(), 2, "expected command to run twice, got:\n{lines}");

    handle.abort();
}

#[tokio::test]
async fn no_change_no_rerun() {
    init_tracing();
    let remote = TempDir::new().unwrap();
    let clone = TempDir::new().unwrap();
    let out_dir = TempDir::new().unwrap();
    let sentinel = out_dir.path().join("out");

    setup_repo(remote.path(), "v1.0.0");

    let cmd = format!("sh -c 'echo ran >> {}'", sentinel.display());
    let w = watcher(remote.path(), clone.path(), &sentinel, "v1.*", &cmd);
    let handle = tokio::spawn(reconcile(w));

    wait_for_line(&sentinel, "ran", 10).await;
    sleep(Duration::from_secs(3)).await;

    let lines = std::fs::read_to_string(&sentinel).unwrap();
    assert_eq!(lines.lines().count(), 1, "expected exactly one run, got:\n{lines}");

    handle.abort();
}

#[tokio::test]
async fn working_dir_is_cwd_for_command() {
    init_tracing();
    let remote = TempDir::new().unwrap();
    let clone = TempDir::new().unwrap();
    let work_dir = TempDir::new().unwrap();
    let out_dir = TempDir::new().unwrap();
    let sentinel = out_dir.path().join("out");

    setup_repo(remote.path(), "v1.0.0");

    let cmd = format!("sh -c 'pwd >> {}'", sentinel.display());
    let mut w = watcher(remote.path(), clone.path(), &sentinel, "v1.*", &cmd);
    w.working_dir = Some(work_dir.path().to_path_buf());

    let handle = tokio::spawn(reconcile(w));

    wait_for_line(&sentinel, work_dir.path().to_str().unwrap(), 10).await;
    handle.abort();
}

#[tokio::test]
async fn failed_command_does_not_stop_loop() {
    init_tracing();
    let remote = TempDir::new().unwrap();
    let clone = TempDir::new().unwrap();
    let out_dir = TempDir::new().unwrap();
    let sentinel = out_dir.path().join("out");
    let counter = out_dir.path().join("counter");

    setup_repo(remote.path(), "v1.0.0");

    // First run: no counter file → touch it and exit 1 (fail)
    // Second run (after tag advance): counter exists → write sentinel
    let cmd = format!(
        "sh -c 'if [ ! -f {counter} ]; then touch {counter}; exit 1; fi; echo done >> {sentinel}'",
        counter = counter.display(),
        sentinel = sentinel.display(),
    );

    let w = watcher(remote.path(), clone.path(), &sentinel, "v1.*", &cmd);
    let handle = tokio::spawn(reconcile(w));

    // Wait for the failing first run to complete
    sleep(Duration::from_secs(2)).await;

    push_tag(remote.path(), "v1.1.0");
    wait_for_line(&sentinel, "done", 10).await;
    handle.abort();
}
