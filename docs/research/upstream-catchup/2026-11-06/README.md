# Catch up both Rust branches to frozen upstream main

**Implementation is in progress.** Read [IMPLEMENTATION.md](IMPLEMENTATION.md), [execution-state.json](execution-state.json) and [RESUME.md](RESUME.md) for the live checkpoint. Platform packages and expanded TS/Rust vectors are mandatory gates in [PARITY-AND-PLATFORMS.md](PARITY-AND-PLATFORMS.md).

Read [scope-and-plan.md](scope-and-plan.md). This replaces C7 as the current plan.

| Identity | Frozen value |
|---|---|
| Working/publication repository | https://github.com/roanutil/embed-pdf-viewer.git |
| Canonical upstream repository | https://github.com/embedpdf/embed-pdf-viewer.git |
| Upstream main target | `8297bad8d388eb9c70324c22cb001c59248f95f4` |
| Viewer-selected runtime target | `4346c5131cd009c4086fdce13bde7fd43fbd4ce0` |
| `research/multi-platform-rust-core-impl` baseline | `5a2f25f063054c344ea514c2f6da788ad45052f3` |
| `research/multi-platform-rust-core-impl-plugins` baseline | `8fa01cc1ec4d0739af535feec6928e6a81163fd9` |

The target is the upstream main observed at 2026-11-06T19:40:15.081503+00:00. Further upstream movement is a new scope. Only the user's fork may receive pushes; upstream is read-only.

The container is `embed-pdf`, its public-fork checkout is `/workspaces/embed-pdf-viewer`, and its persistent Docker volume is `embedpdf-workspace`. This directory lives inside that checkout. Planning did not merge upstream, port behavior, install project dependencies, build runtime artifacts, run product tests, or publish releases. Separately authorized devcontainer commits/merges are recorded in [environment-and-authority.md](environment-and-authority.md).

1. Read [the five dependency jobs](scope-and-plan.md#dependency-jobs).
2. Use [core-runtime-inventory.md](core-runtime-inventory.md) and [plugin-binding-inventory.md](plugin-binding-inventory.md) as the detailed worklists.
3. Check [pinned-targets.json](pinned-targets.json) before execution; compare the full SHAs, never a moving `main`.
4. Retain [historical C7](historical/c7/scope-and-plan.md) as historical evidence. Its runtime A/B choice, equal-branch assumption, path counts and estimate are superseded here.
5. Authorize implementation separately after reviewing this plan.

Next action (under two minutes): read the frozen-target table and Job 1 in [scope-and-plan.md](scope-and-plan.md).
