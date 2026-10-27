# epdf-plugin-attachment

`packages/plugin/attachment` in Rust, at the current TypeScript. The vector
`vectors/plugins/attachment.json` (20 cases) is new with this crate: Task 13.32 wrote its generator,
`tooling/plugin-conformance/src/suites/attachment.ts`, against the shipping package. It replays with
no member-order allowance or outcome exception (`cargo test -p epdf-plugin-attachment`), except one
case that waits on a kernel repair (see "Open").

The modules follow the TypeScript files. `model.rs` is `model.ts` (the session state, the two
`AttachedFile` builders and `freeName`), `names.rs` is `names.ts`, `sync.rs` is `sync/files.ts`
(`foldFiles`, `predictFiles`, `undoOf`), `controller.rs` is `controller.ts`, and `state.rs` is
`state.ts`.

The document's attachments are one on-demand kernel mirror, `mirror/attachments`: opening a document
reads nothing, and the first `ensure_loaded` (or an `add`) reads `attachments.list`. Its value is
the engine's `Attachment` array, or `null` before the first read. The list joins that mirror's
`view()` with the annotation plugin's file attachment annotations, so it shows this session's
pending adds and removes and needs no read of its own for a page's files. Keys sort as JavaScript
compares strings, by UTF-16 code unit, so a name outside the BMP sorts before U+E000 to U+FFFF as it
does in the TypeScript.

The annotation and stage plugins are optional, as in `attachment.plugin.ts`, and are looked up when
needed. The vector runs a page's files with and without the annotation plugin, and `reveal` with and
without the stage.

## Tests

`tests/conformance.rs` replays the vector with the real interaction, annotation, stage and history
plugins beside this one. `tests/behavior.rs` pins the event-before-settlement order, a refused add's
rollback, the permission and cancellation gates asking the engine for nothing, the build with no
optional plugin, the reads' `Rc` identity, and the turns each read and write settles on, measured
on the TypeScript with `tsx`.

As in every sibling crate, `build.rs` emits the rpath only under `harness`, so run the unit tests
with `cargo test -p epdf-plugin-attachment --lib`; the pure build is checked with `cargo clippy
--no-default-features --target wasm32-unknown-unknown`.

## Rust-only divergences

None is reached by a vector.

- The TypeScript keeps one `AttachedFile` object per engine record (`controller.ts:59-71`), so a
  list row keeps its identity while its file is unchanged. Here `list()` and `get_selected()` keep
  their `Rc` until an input changes, but a row is a JSON value with no identity of its own.
- A source's bytes are `{"$bytes": base64}` (`file_source`). A `File`, `Blob` or `ArrayBuffer` has
  no spelling, so a host passes the name and type a `File` would carry. A nameless source rejects
  `invalid-input`, as bytes without a name do in the TypeScript.
- An `AttachedFile` without `home`, outside the contract type, is read as an attachment with no
  key, where the TypeScript throws reading `home.kind`.
- `reveal` hands the stage a `Rect` decoded from the record's `rect`; one that does not decode
  reveals the whole page.

## Open

Two turn counts wait on code outside this crate.

`download` of a file this session is still adding waits on `ctx.changes.whenSettled()`. The kernel's
`when_settled` (`crates/epdf-kernel/src/changes.rs:264`) settles one turn after the last result,
where the TypeScript's `await Promise.allSettled(...)` (`packages/core/main/src/changes.ts:548-550`)
settles two turns after. The `download-an-attachment` case fails on it (step 12).

`download` of a page's file awaits the annotation plugin's `download_resource`
(`crates/plugins/annotation/src/write/crud.rs:324`), which returns the engine call itself. The
TypeScript's `downloadResource` is an `async` function returning it
(`packages/plugin/annotation/src/write/crud.ts:197-208`), one turn later.
`a_page_files_download_settles_on_the_typescripts_turn` fails on it (5, not 6).

The crate has no `uniffi` feature; a host binding is 13.33's.
