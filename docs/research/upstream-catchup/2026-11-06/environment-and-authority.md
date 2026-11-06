# Environment, authorization and decisions

## Authorized and completed infrastructure work

| Action | Evidence / result |
|---|---|
| Fresh public-fork devcontainer | `embed-pdf`, image `sha256:30b97dec3bd8d007b9197ab8fc0027f3b62ea3bd25390a5f516b4cbab085df84`, checkout `/workspaces/embed-pdf-viewer`, persistent volume `embedpdf-workspace` |
| Publish missing impl branch | User explicitly allowed local/private branch publication to public fork. Initially published local `dda1cfcd868d2d7fee0c51b90831947ac752e398`; original private branch f28ed remains untouched. |
| Commit/push pending devcontainer edits | `5a2f25f063054c344ea514c2f6da788ad45052f3` on public `research/multi-platform-rust-core-impl`; only `.devcontainer` paths changed. |
| Merge infrastructure into sibling / push | `8fa01cc1ec4d0739af535feec6928e6a81163fd9` on public `research/multi-platform-rust-core-impl-plugins`; parents are `57a125dd7…` and `5a2f25f06…`. Tree delta versus prior plugins affects only `.devcontainer`. Preserves .NET10.0.403, Binaryen133, Rust-eval bootstrap/5250/5251, Apple ignore rules and runtime82f2. |
| Versioned planning checkpoints | User requested version-control progress/decisions for easy stop/resume. Planning documents and resume notes are committed on `planning/upstream-catchup-2026-11-06` and pushed exclusively to `https://github.com/roanutil/embed-pdf-viewer.git`. |

## Decisions and boundaries

1. The working repository is the public fork `roanutil/embed-pdf-viewer`. Canonical `embedpdf/embed-pdf-viewer` is read-only. User explicitly forbids every upstream push; all pushes use the explicit public fork URL.
2. Viewer target is frozen at `8297bad8d388eb9c70324c22cb001c59248f95f4`, observed main and reconfirmed during final baseline capture. Viewer runtime and observed canonical runtime HEAD both equal `4346c5131cd009c4086fdce13bde7fd43fbd4ce0`; include that target in the retained runtime fork. No C7 supplemental A/B choice remains.
3. Both research branch histories are required; impl is already an ancestor of plugins after the infrastructure merge. One shared future catch-up integration is a recommendation, not approved product implementation. Neither branch has received the new upstream target in this planning task.
4. Existing C7 documents/evidence are copied byte-exact under `historical/c7` and `historical/c5g`. Original host records are preserved. Historical bare repositories are excluded from the archive; new exact target objects/refs live in this checkout's Git metadata. Missing old host resources are historical evidence limitations, not new acceptance.
5. Product catch-up implementation remains unauthorized. No upstream viewer/runtime merge, Rust behavior port, project dependency install, runtime build, product test, release, artifact pin, NuGet publication, workflow dispatch or upstream push occurred. Devcontainer mock/syntax checks are infrastructure verification only.

## Current environment and resumability

The image is Linux ARM64. Observed tools: Node24.20.0, Python3.13.5, Cargo1.98.0 and Swift6.3.1. `dotnet` is not installed in this image. Sibling source Dockerfile preserves .NET10.0.403, but that does not install it in this existing container. Before a later .NET gate, provision a matching image/toolchain from the selected execution source. Android NDK is absent on ARM64; Apple consumers require macOS/Xcode. Project dependencies/runtime artifacts were intentionally not installed or built for planning.

The agent manages the Git credential forwarder at host127.0.0.1:18081 and OAuth browser forwarder at host127.0.0.1:18080. Both listeners were checked running. Host credentials are forwarded, not copied into the container or documents. Processes are session resources; after a host restart, run the documented `mise exec -- gcf-server` and `mise exec -- o2f-server` with their port environment variables. Authentication listeners are not Git artifacts.

An initial private clone was retained at `/tmp/private-source-reference` inside the container when switching to the explicitly requested public checkout. It is not the workspace or a push destination. Host checkout remains on impl with only its preexisting dirty runtime submodule; that submodule state was not committed or modified.

The runtime metadata repository is `.git/catchup-runtime.git`; a public-only fetch recreates it if needed. Viewer fixed target has local ref `refs/catchup/2026-11-06/viewer-target`. These local objects are conveniences; the committed full SHAs and source inventories are the portable authority. Raw inventory generation may warn that exhaustive endpoint rename detection was skipped; path-set overlap uses direct names, and no conflict-free inference is made.

Next action (under two minutes): read [RESUME.md](RESUME.md) before continuing this task.
