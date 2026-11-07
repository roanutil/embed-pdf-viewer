# Brief for catch-up workers (2026-11-07)

You are one worker in an upstream catch-up of the EmbedPDF viewer's Rust port. Read this file and
`/workspaces/CATCHUP-GUIDE.md` (the method) before doing anything. Your own task prompt says which
area and which worktree are yours.

## What is going on

Upstream `embedpdf/embed-pdf-viewer` `main` at `074a190c9` is the target. Our branch
`research/catchup-2026-11-07-plugins` (tip `cc2d9490f` when you start) has upstream merged in. The
TypeScript under `packages/**` is the reference (the "oracle"). The Rust crates under `crates/` port
it and replay frozen conformance vectors generated from the TypeScript:

- `tooling/engine-conformance` generates `tooling/engine-conformance/vectors/**`, replayed by
  `crates/epdf-engine/tests/*` (and some `crates/pdfium` tests). These vectors are already
  regenerated from the new TypeScript on your starting tip; every changed vector there traces to an
  upstream commit (see the commit messages `7fe03c102` and `cc2d9490f`).
- `tooling/plugin-conformance` generates `tooling/plugin-conformance/vectors/**`, replayed by
  `crates/plugin-conformance` and `crates/plugins/*`. Its generator was already stale at the start
  (70 type errors at `8fa01cc1e`, 465 now) and has not been regenerated.

Upstream's changes in range are largely API-consistency renames (`changed` -> `changedRefs`,
`family` -> `kind`, `checksum`/`size` -> `md5`/`sizeBytes`, `contentsSize` -> `contentsSizeBytes`,
`displayHash` -> `displaySha256`, `*DTO` types -> plain names, `*State` plugin models -> `*Model`,
`PdfBits` -> `PdfPermissionFlags`, ISO 8601 dates, one spelling for "nothing") plus behaviour:
missing `/C` reads as `color: null` (no stroke), annotation kind changes (`EPDFAnnot_SetSubtype`),
ink curves, dash patterns, rich-text markup comments, `hasMark` on widgets, text lines end in `\n`
with a `charMap`, built-in fonts, snapshots, paged flow, i18n plural forms, command labelParams.
`git log --no-merges a40d63a19..074a190c9 -- packages` lists them (63 commits). Read the commit that
introduced a change before porting it.

## Where to work

Everything runs in the dev container `embed-pdf-catchup1107`. From the host:

    docker exec embed-pdf-catchup1107 bash -c 'cd /workspaces/wt/<yours> && <command>'

- Use `bash -c`, never `bash -lc` (a login shell drops emsdk from PATH).
- Prefix commands with `export PATH=/workspaces/bin:$PATH` (a clang shim the wasm build needs).
- Cargo: `export CARGO_TARGET_DIR=/workspaces/wt/<yours>/target CARGO_BUILD_JOBS=4`, and for tests
  `export LD_LIBRARY_PATH=/workspaces/wt/<yours>/packages/engine/runtime/build/libpdfium/linux-arm64/lib`.
  Other workers share the 16 cores: don't raise the job count.
- For multi-line edits write the file on the host (your scratch area) and `docker cp` it in, then
  `docker exec -u root embed-pdf-catchup1107 chown node:node <path>`; or edit with a python script
  passed through a file. Inline heredocs inside `bash -c '...'` break on quotes.
- Your worktree is already prepared (`pnpm install`, PDFium `linux-arm64`, a `wasm32` payload built
  without the Rust core, all TS packages built). Rebuild a TS package you change with
  `pnpm --filter <pkg> build` only if a generator imports its `dist`.
- `jdiff`: `python3 /workspaces/jdiff.py <rev> <vector files...>` gives a semantic diff (collapsed
  key paths, added/removed/changed). Use it to review every vector you regenerate.
- Waiting on a long command: run it detached (`docker exec -d ... > /workspaces/logs/<you>-x.log 2>&1`)
  and poll with a background shell `until` loop. Never a foreground sleep.

## Rules

1. Work only in your worktree and on your branch. Do not read from or write to another worker's
   worktree. Do not touch `/workspaces/wt/plugins`, `/workspaces/wt/impl*`,
   `/workspaces/embed-pdf-viewer`, or any other container.
2. TypeScript under `packages/**` is the reference: do not change it. If Rust cannot match it
   without a TS change, stop on that item and report it. Exporting a symbol a package does not
   export today is a TS change too: report it, don't do it.
3. Rust matches the TypeScript. Divergences the owner already accepted are listed in
   `docs/research/rust-plugins-branch-review.md` ("Status") and in
   `multiplatform-upstream-catchup-2026-11-impl.md`; keep them. Any new divergence needs the owner's
   approval: report it with the reason and leave the item undone.
4. Vectors: regenerate a group only through its generator, and only when the reference changed or
   the generator was stale. Never hand-edit a vector. Never restore an old key so Rust passes. A
   regenerated answer that contradicts its case's `covers` text is a signal: investigate (it is
   often a stale generator input, e.g. a renamed request field).
5. Removed or renamed APIs that the tooling imports: follow the guide's "Expected API that no longer
   exists" section. Record each with old name, removing commit, replacement or none, coverage lost.
6. Git: commit bounded steps by explicit path (`git add <paths>`, never `-A`/`-a`/`.`). Never
   `git stash`, never `git reset --hard`/`clean`, never `git worktree prune`/`repair`, no push, no
   merge, no rebase of shared branches. Commit messages: conventional prefix, what changed, the
   upstream commit it follows, and test evidence (counts, remaining failing names). No attribution
   trailers, no Co-Authored-By, no session links. Never mention other projects, people, products or
   ticket ids in code, comments or messages.
7. Format with `cargo fmt -p <crate>` and revert any reformatting of lines you didn't touch. Run
   `cargo clippy -p <crate> --tests` on crates you changed.
8. No `.github/**` changes.

## Report

End with a short report: commits (hash + one line), tests before/after for your area (counts and
remaining failing names with a category: port bug fixed / needs TS change / needs owner decision /
environment / other area's), every vector group you regenerated with the upstream commits its diff
traces to, and every decision you need from the owner, each with your recommendation and its cost.
