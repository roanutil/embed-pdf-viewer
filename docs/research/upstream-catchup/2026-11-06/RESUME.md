# Stop/resume checkpoint

**Status:** planning refresh complete; implementation is not authorized. Authorized devcontainer changes are committed, merged into the sibling, and pushed. The dedicated planning branch carries documents, decisions, scope evidence and verification.

| Identity | Value |
|---|---|
| Public fork / only push destination | `https://github.com/roanutil/embed-pdf-viewer.git` |
| Planning branch | `planning/upstream-catchup-2026-11-06` |
| Container / checkout | `embed-pdf` / `/workspaces/embed-pdf-viewer` |
| Viewer fixed target | `8297bad8d388eb9c70324c22cb001c59248f95f4` |
| Runtime fixed target | `4346c5131cd009c4086fdce13bde7fd43fbd4ce0` |
| Impl starting tip | `5a2f25f063054c344ea514c2f6da788ad45052f3` |
| Plugins starting tip | `8fa01cc1ec4d0739af535feec6928e6a81163fd9` |

1. Open [scope-and-plan.md](scope-and-plan.md), [pinned-targets.json](pinned-targets.json), and [environment-and-authority.md](environment-and-authority.md). The target remains fixed if upstream moves.
2. Run `git status --short` and `git log -3 --oneline` in the checkout. Preserve any newer work. From another machine, clone the public fork's planning branch; no host `/private/tmp` files are required to read the plan.
3. Review [verification.json](verification.json), [self-review.md](self-review.md), and both current inventories. Historical C7 0/0/0 is not a certification of this refresh. New estimate is80–145 engineering hours plus8–24 machine hours and native/device queues.
4. Obtain separate implementation authorization before Job1. Check both public starting refs for drift; changed refs require reconciliation, not a reset. Impl is already included in plugins; do not repeat the devcontainer merge.
5. During future authorized work, commit bounded progress/decisions/evidence and update this checkpoint before stopping. Push only to the user's fork; never canonical upstream. Keep release/pin/notification actions explicitly scoped.

To open the plan in the existing container:

```sh
docker exec -it embed-pdf bash
cat docs/research/upstream-catchup/2026-11-06/README.md
```

Progress history: discovered stale C7 branch premise → published missing local impl at user direction → pinned current canonical viewer/runtime → inspected full deltas and delegated plugin review → committed/pushed infrastructure on both branches → refreshed plan/inventories → checked links/SHAs/hashes/source guards → versioned plan and checkpoint. Catch-up product work has not begun.

Next action (under two minutes): read the target table and Job1 in [scope-and-plan.md](scope-and-plan.md).
