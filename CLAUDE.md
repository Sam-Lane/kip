# kip — bug fix plan

## Bug: clone_or_open opens instead of cloning when dir pre-exists

### Root cause

`src/reconciler.rs` line 17 calls `std::fs::create_dir_all(dir)` unconditionally
before calling `clone_or_open`. This creates the target directory even when the
repo has never been cloned.

`src/git.rs` `clone_or_open` then checks `dir.exists()` — which is now `true`
because the directory was just created — and calls `Repository::open(dir)` instead
of cloning. `open` fails immediately with "could not find repository" because
the directory is empty (no `.git`). The reconciler exits, kip logs the error, and
the empty directory is left behind. Every subsequent restart hits the same path.

### Fix

Change the existence check in `clone_or_open` from `dir.exists()` to
`dir.join(".git").exists()`. This distinguishes between "directory exists" and
"a git repo exists here", which is the correct semantic.

**File:** `src/git.rs`

```rust
// before
if dir.exists() {

// after
if dir.join(".git").exists() {
```

That's the entire change. One line.

### Testing

After the fix, verify:
1. `cargo test` passes
2. Remove the local clone dir, restart kip — it should clone successfully
3. Restart kip against an existing clone dir — it should open successfully

### PR notes

- Title: `fix: check for .git dir rather than parent dir in clone_or_open`
- The companion `create_dir_all` call in `reconciler.rs` is intentional (ensures
  parent dirs exist) — do not remove it.
- No other files need changing.

---

## Bug 2: reconciler tracks local branch ref instead of remote tracking ref

### Root cause

After cloning, git2 creates two refs:
- `refs/heads/main` — local branch, points to the cloned commit
- `refs/remotes/origin/main` — remote tracking ref, updated on every `fetch_all()`

`fetch_all()` in `src/git.rs` fetches from all remotes, which correctly advances
`refs/remotes/origin/main`. However `resolve_best_ref` in `src/matcher.rs` calls
`resolve_oid(repo, "main")` which calls `repo.revparse_single("main")`. git2
resolves `"main"` to `refs/heads/main` (the local branch) first — which is never
advanced by a fetch. So `last_oid` never changes after the first run and kip stops
detecting new commits entirely.

Observed: `refs/heads/main` stays at the original clone commit; `refs/remotes/origin/main`
advances correctly but is never read.

### Fix

In `src/git.rs`, update `resolve_oid` to resolve against the remote tracking ref
when the plain refname resolves to a local branch that lags behind the remote:

```rust
pub fn resolve_oid(repo: &Repository, refname: &str) -> Result<git2::Oid> {
    // Prefer the remote tracking ref so we see fetched commits immediately
    let remote_ref = format!("refs/remotes/origin/{refname}");
    let candidate = if repo.find_reference(&remote_ref).is_ok() {
        remote_ref
    } else {
        refname.to_string()
    };

    let obj = repo
        .revparse_single(&candidate)
        .with_context(|| format!("resolving ref {refname}"))?;
    let commit = obj.peel_to_commit().context("peeling to commit")?;
    Ok(commit.id())
}
```

This tries `refs/remotes/origin/<refname>` first. If that ref exists (i.e. the user
configured a plain branch name like `"main"`), use it. Otherwise fall back to the
original refname (handles full refspecs like `refs/tags/v1.0` or already-qualified
remote refs). No changes needed to the config format or any other file.

### Testing

After the fix, verify:
1. `cargo test` passes
2. Start kip against a fresh clone, make a commit and push, confirm kip detects it
   on the next interval tick without restarting
3. Config with `reference = "main"` should work (no need for `refs/remotes/origin/main`)
4. Config with a semver pattern like `reference = "v1.*"` should still work (tags
   are not affected by this change — `refs/remotes/origin/v1.*` won't exist, so it
   falls back to the original path)

### PR notes

- Title: `fix: resolve remote tracking ref after fetch instead of stale local branch`
- These two bugs (Bug 1 and Bug 2) can ship in the same PR since they're both in
  `src/git.rs` and both found together.
- Combined PR title suggestion: `fix: correctly clone into pre-existing dirs and track remote refs after fetch`

---

## Bug 3: working tree is never updated after fetch — command runs against stale files

### Root cause

`src/reconciler.rs` detects an OID change via `resolve_oid`, logs "change detected",
and immediately runs the configured command. But it never updates the working tree
to match the new OID first.

git2's `fetch_all()` only advances `refs/remotes/origin/main`. The files in the
working directory still reflect the commit that was current when the repo was
originally cloned. The command therefore runs against stale files — it sees an old
version of every file in the repo, indefinitely.

Observed in production: kip logs "change detected … to=<new OID>" and "command
completed successfully", but `ls -la` on the working directory shows files from
the original clone commit. Manually `scp`-ing updated files to the host was used
as a workaround.

### Fix

After detecting an OID change and before running the command, perform a hard reset
to the new commit so the working tree reflects what was fetched.

**File:** `src/reconciler.rs` — in the block that handles a detected OID change,
after fetching and resolving the new OID, call into `src/git.rs` with a new helper:

```rust
// src/git.rs — add this function
pub fn reset_hard(repo: &Repository, oid: git2::Oid) -> Result<()> {
    let obj = repo.find_object(oid, None).context("finding commit object")?;
    repo.reset(&obj, git2::ResetType::Hard, None)
        .context("resetting working tree to new HEAD")?;
    Ok(())
}
```

Then in `src/reconciler.rs`, call `reset_hard` after the OID change is confirmed
and before spawning the command:

```rust
// after: let new_oid = resolve_oid(&repo, &watcher.reference)?;
// and the change-detected log line, add:
reset_hard(&repo, new_oid)?;
```

The reset must happen **after** `fetch_all` and **before** the command is spawned.
The `working_dir` in the config points into the cloned repo, so once the working
tree is correct the command (e.g. `terraform apply`) sees the right files.

### Why `git reset --hard` and not `git checkout`

`reset --hard` moves HEAD and updates the index and working tree atomically. It
handles detached HEAD state (which can occur when OID is resolved from a remote
tracking ref). `checkout` would require knowing the branch name and would not
work reliably with the arbitrary refname the user configures.

### Testing

After the fix, verify:
1. `cargo test` passes
2. Push a commit that changes a file kip's command reads; confirm the command on
   the next tick sees the updated file content (not the old version)
3. Confirm the working directory `git log -1` matches the OID kip logged as `to=`

### PR notes

- Title: `fix: reset working tree to fetched commit before running command`
- This bug is independent of Bug 1 and Bug 2 — can ship separately or bundled.
- The `reset_hard` helper belongs in `src/git.rs` alongside the other git operations.

---

## Improvement 4: no-change path is completely silent — can't tell if kip is alive

### Problem

The reconciler only emits logs when something changes or errors. During normal
operation (no new commits), the loop ticks every `interval` seconds and produces
zero output. If kip hangs after a successful run, or the fetch silently stops
working, there is no way to detect it from logs without pushing a dummy commit.

### Fix

Add a `debug!` log on the no-change path so operators can verify polling is
working by setting `RUST_LOG=debug`:

**File:** `src/reconciler.rs` — in the `if Some(oid) == last_oid { continue; }` block:

```rust
if Some(oid) == last_oid {
    debug!(watcher = %watcher.name, oid = %oid, "no change");
    continue;
}
```

Add `debug` to the `tracing` import at the top of `reconciler.rs`.

This keeps INFO output clean (no spam every 60s) while giving operators an
opt-in way to watch the heartbeat: `RUST_LOG=debug journalctl -fu kip`.

### PR notes

- Title: `feat: log no-change heartbeat at debug level`
- One-line change plus import tweak.

---

## Bug 5: command stderr routed to `warn!` — terraform output appears as warnings

### Problem

`src/runner.rs` logs the child process's stderr at `warn!` and stdout at `info!`.
For `terraform apply`, all plan/apply output goes to stderr — which means every
line of a normal, successful apply floods the journal at WARN level. This makes
it impossible to distinguish real warnings from routine output.

### Fix

Log both stdout and stderr at `info!` level. Use the stream name as a field so
log consumers can still filter by stream if needed:

**File:** `src/runner.rs`

```rust
// before
Some(l) => info!(target: "runner", "{l}"),   // stdout
Some(l) => warn!(target: "runner", "{l}"),   // stderr

// after
Some(l) => info!(target: "runner", stream = "stdout", "{l}"),
Some(l) => info!(target: "runner", stream = "stderr", "{l}"),
```

Remove the `warn` import from `runner.rs` if it is no longer used elsewhere.

### PR notes

- Title: `fix: log command stderr at info level instead of warn`
- If a future use case needs real stderr-as-warn (e.g. a command that uses stderr
  for genuine errors), this can be made configurable via the watcher config. For
  now, info is correct for tools like terraform and ansible.

---

## Bug 6: runner exits loop on stdout EOF — trailing stderr lines silently dropped

### Problem

The `run_command` loop in `src/runner.rs` uses `tokio::select!` over stdout and
stderr independently:

```rust
line = stdout_lines.next_line() => {
    match line? {
        Some(l) => info!(...),
        None => break,   // <-- exits on stdout EOF
    }
}
line = stderr_lines.next_line() => {
    match line? {
        Some(l) => warn!(...),
        None => {}       // <-- does nothing on stderr EOF
    }
}
```

When the child process closes stdout before stderr (common when a process flushes
error output after its main output), the `None => break` fires immediately and the
loop exits. Any stderr lines still buffered are never read and are silently dropped.

This means command failure messages — the most important output — can be lost.

### Fix

Track EOF for each stream separately and only break when both are exhausted:

**File:** `src/runner.rs`

```rust
let mut stdout_done = false;
let mut stderr_done = false;

loop {
    if stdout_done && stderr_done {
        break;
    }
    tokio::select! {
        line = stdout_lines.next_line(), if !stdout_done => {
            match line? {
                Some(l) => info!(target: "runner", stream = "stdout", "{l}"),
                None => stdout_done = true,
            }
        }
        line = stderr_lines.next_line(), if !stderr_done => {
            match line? {
                Some(l) => info!(target: "runner", stream = "stderr", "{l}"),
                None => stderr_done = true,
            }
        }
    }
}
```

The `if !stdout_done` / `if !stderr_done` guards on each arm prevent `select!`
from polling a closed stream (which would loop on EOF forever).

### Testing

After the fix, verify:
1. `cargo test` passes
2. Run a command that writes to stderr after stdout closes (e.g. a shell script
   that does `echo ok; sleep 0.1; echo error >&2; exit 1`) — confirm the stderr
   line appears in logs and the error exit is reported

### PR notes

- Title: `fix: drain both stdout and stderr before exiting runner loop`
- Combine with Improvement 5 (stderr at info level) in the same PR — the fix
  touches the same lines.
