//! `GitPort` implemented by shelling out to the `git` CLI. Offline-capable;
//! branch facts come from `rev-list`/`merge-base`/`diff` against the shared
//! object store, worktree ops from `git worktree` (§6, §7).
