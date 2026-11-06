# Catch-up guide

This guide tells an agent how to continue the upstream catch-up the way the user wants it run. It records decisions as the user gives them. Update it when feedback changes an approach. Keep the ledger (`IMPLEMENTATION.md`) for progress and evidence; keep this file for method.

## Scope and where work happens

- All work runs in the devcontainer `embed-pdf`, in the checkout `/workspaces/catchup-2026-11-06` on branch `research/catchup-2026-11-06`. Do not edit the host repository for catch-up work.
- Fixed targets stay fixed: viewer `8297bad8d388eb9c70324c22cb001c59248f95f4`, runtime `4346c5131cd009c4086fdce13bde7fd43fbd4ce0`. Public baselines for final acceptance: impl `5a2f25f063054c344ea514c2f6da788ad45052f3`, plugins `8fa01cc1ec4d0739af535feec6928e6a81163fd9`.
- Read first: `scope-and-plan.md`, `PARITY-AND-PLATFORMS.md`, `IMPLEMENTATION.md`, `execution-state.json`.

## Commits and pushes

- Commit bounded work with the evidence in the message or the ledger. Push each commit to the user's fork only (`origin` = `roanutil/embed-pdf-viewer`). Never push to canonical upstream.
- Pushing needs the git credential forwarder. The user runs it on the host on port 10895. Push with:
  `GIT_CREDENTIAL_FORWARDER_SERVER=host.docker.internal:10895 git push origin research/catchup-2026-11-06`
- Do not start the host forwarder yourself. That was denied once.
- No Claude attribution in commits, PRs or releases in this repository (user rule). Use the existing git identity.
- Do not stash. Do not reset or clean a worktree that holds work.

## Conformance vectors: the rule

Vectors are the TypeScript oracle's output. Change a committed vector only when the TypeScript behavior legitimately changed.

1. When a conformance replay fails, investigate it without asking. Work through these in order:
   - Did the TypeScript source change? Check `git log` and `git diff` for the relevant files between the starting commit `2b86156d3` and `HEAD`, and check the commit that introduced each renamed key or new field. A committed vector written before that commit is stale.
   - Is it a runtime bug in Rust? Read the Rust path and find the first divergent value.
   - Is it flakiness? Regenerate the group twice and compare hashes. Run the replay twice.
2. If the TypeScript changed legitimately, regenerate the group from the oracle (`pnpm run <name>` in `tooling/engine-conformance`, names in its `package.json`) and review the semantic diff, ignoring `covers` text. Fix the Rust side to match.
3. If it is a runtime bug, fix Rust. If it is flakiness, fix the flaky path.
4. Do not regenerate a vector only to turn a replay green when the change is not a legitimate TypeScript change. Do not restore a stale key to make Rust pass.
5. Only bring the user cases you could not place. Label each one: TypeScript bug, runtime bug, flakiness, or something else, and say what you checked. Anything that needs an exception (Rust deviating from TypeScript, or changing TypeScript) needs the user's approval.
6. When you regenerate a vector, check that the generator input matches the current TypeScript API. A generator that sends a removed key (for example `family` where the draft now takes `kind`) is a stale generator. Fix the generator input, not the oracle.

## Known renames and their evidence

| Change | Evidence |
|---|---|
| Annotation mutation meta `changed` → `changedRefs` | TS `AnnotationListMutationMeta`; Rust `AnnotationMutationMeta.changed_refs` |
| Permission bits `PdfBits` → `PdfPermissionFlags` (`permissionFlags` on grants, `flags` on state), advisory removed | `a780243ba` |
| Form `family` → `kind` (fields, drafts, patches) | `c4b286a69`; `FormFieldDraftBase` and `FormFieldPatchSchema` |
| Widget `fieldFamily` → `fieldKind`, new `hasMark` | `c4b286a69`; `joinWidgetFieldNumbers` |
| Signature `contentsSize` → `contentsSizeBytes` | TS `worker-protocol.ts` |
| Attachment `size` → `sizeBytes`, `checksum` → `md5` | `26a686502` |
| Display hash `displayHash` → `displaySha256` | `26a686502` |
| Action tree drops `warningFlags` | `ActionModelReader` returns `{ root, incomplete, warnings }` |
| Markup comment `richText` on every markup kind (`null` without `/RC`) | `markupBaseFields` |
| Line `captionEnabled` is `false` without `/Cap` | `readLine` |
| Empty `nm` is not stamped or written | `writeAnnotationBase` truthiness |
| Page text is LF after cleaning, not CRLF | viewer text cleaning; ledger notes |

## Debugging patterns that worked

- Baseline first. Build a worktree at the starting commit, link the runtime build (`packages/engine/runtime/build` → the main checkout's build), and run the full suite with `LD_LIBRARY_PATH=<runtime>/lib`. Diff failing tests by panic location against the current tree. Without this you cannot separate old failures from new ones.
- Run replays with `cargo test -q --test <name>`. Set `LD_LIBRARY_PATH` to the runtime lib first, or they fail to load `libembedpdf.so`.
- Compare JSON semantically. Walk both trees, ignore `covers`, and list added, removed and changed paths. Do not diff raw text; key order and line breaks produce noise.
- Use the generator, not hand edits, to change a vector. Regenerate, then compare with `git show HEAD:<path>`.
- For shell edits in the container, write a Python script on the host with the Write tool, `docker cp` it in, and run it. Inline quoting through `docker exec bash -lc '...'` breaks on apostrophes and backticks.
- `cargo fmt -p epdf-engine` after Rust edits.

## Things that cost time

- A `docker exec` with a heredoc or nested quotes silently splits the command. Use scratch files.
- Background commands that wait on a file need `run_in_background` with an until-loop; do not sleep in the foreground.
- Do not treat the Rust test `captionEnabled` or `nm` expectations as vector truth. Check the TypeScript path first.

## Status log

Keep the current status in `IMPLEMENTATION.md`. Add a dated section per checkpoint with: the pass/fail counts from the full suite, the list of failures that remain and their category, and the commits.

## Added after the forms and transfer checkpoint

- Golden hashes in Rust tests can be stale. Check them against the TypeScript test that holds the same digest (`packages/engine/core/src/conformance/runAnnotationExportConformance.ts`). Run that test with `npx vitest run test/annotation-export.test.ts -t "same file"` from `packages/engine/main`. To get the oracle text, add a temporary write to the conformance file, run it, then `git checkout` the file. Verify with `sha256sum` before you change a Rust golden.
- Key order is part of hashed text. `serde_json::Map::remove` swaps the last entry into the removed slot when `preserve_order` is on. Use `shift_remove`.
- Compare JSON with order and without it. A byte-level diff of the first mismatch finds ordering bugs quickly.
- A replay can match a vector that disagrees with its own `covers` text. Check the covers text before you accept a regenerated answer.
- The approved reason for a vector change has to be a TypeScript source change. Check `git log` on the TypeScript files and the generator. If neither changed, the cause is in the oracle environment, so raise it before you regenerate.
