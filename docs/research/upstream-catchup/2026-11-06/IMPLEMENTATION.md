# Catch-up execution ledger

Plan: docs/research/upstream-catchup/2026-11-06/scope-and-plan.md

## Authorization and inputs

User authorized implementation after planning commit4289d3f86, allowed subagents, requires version-controlled progress and permits pushes only to their fork. Viewer target8297bad8d388eb9c70324c22cb001c59248f95f4 and runtime4346c5131cd009c4086fdce13bde7fd43fbd4ce0 remain fixed. Public baseline refs remain impl5a2f25f06 and plugins8fa01cc1e until final acceptance.

## Current jobs

| Job | State | Evidence / next operation |
|---|---|---|
| 1 Runtime | In progress, runtime agent | Worktree /workspaces/catchup-runtime-2026-11-06, branch catchup/2026-11-06-runtime; baseline target-args and emsdk tests passed; source merge and toolchain bootstrap underway. |
| 2 Viewer integration | In progress, root | Worktree /workspaces/catchup-2026-11-06, branch research/catchup-2026-11-06 starts at frozen plugins and merges planning checkpoints; upstream merge next. |
| 3 Engine and shared contracts | Pending | Final merged TS oracle and matching runtime required for acceptance. |
| 4 Plugins and bindings | Pending | Shared contracts and geometry first; maintain all platform packages and parity vectors. |
| 5 Artifacts and both refs | Pending | Final supported consumers and ancestry proof required; no release publication authorized. |

## Preflight interfaces and rulings

1. Job1→2/3: runtime headers/symbols/build identity must match the fork. Ruling: source integration and dependency provisioning may overlap runtime builds; no dependent behavior acceptance until matching runtime exists. Cost if wrong: duplicated source work, caught by final graph checks.
2. Job2→3/4: merged TS schemas define final keys/tags/nulls and generated vector outputs. Ruling: preserve generated baseline evidence and regenerate with explicit reviewed diffs, never restore stale fixtures to turn Rust green. Cost if wrong: missed feature cases; paired replay/coverage gates catch them.
3. Job3→4/5: engine DTO vocabulary is shared with router/native bindings. One owner changes each vocabulary; platform packages follow the accepted records and artifacts.
4. Ruling: use manual Git worktrees inside Docker because native app worktree tools operate on the host checkout; retain a versioned ledger here per user request instead of relying on disposable skill scratch. The five plan headings remain Jobs rather than rewriting them for skill script parsing.
5. Platform/parity requirements were reaffirmed by the user: see PARITY-AND-PLATFORMS.md. Both are completion gates, not optional follow-up work.

## Stop/resume

Inspect this ledger, git status, git log and agent worktrees before repeating a task. Logs go under .superpowers/sdd/upstream-catchup-2026-11-06; summaries, source identities, tests/results and decisions are committed here. Preserve running builds and all prior artifacts. Do not reset/clean a worktree to resume.
