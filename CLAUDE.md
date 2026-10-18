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
