# epdf-plugin-annotation

`packages/plugin/annotation` in Rust: the pure `epdf-annotation-core` model wired to the engine and
to the interaction hub. All 648 cases in nine vector files replay identically (`cargo test -p
epdf-plugin-annotation`):

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/annotation-repository.json` | function | 537 | `repository/`: `fromDTO` over 52 fixtures, then `toPatch`, `toScopedPatch` (geometry and 10 prop scopes over 22 restyled annotations) and `toCreateDraft`, plus the seam helpers |
| `vectors/plugins/annotation-pure.json` | function | 38 | the tool registry, `buildTextItems`, `iconPlacementDraft`, the reducer, and `engine-core`'s `buildCommentThreads` |
| `vectors/plugins/annotation-hydration.json` | scenario | 14 | whole-document hydration, the cursor-ordered replay of the hydration window, remote events, page membership, form and redaction events, `reloadPage` |
| `vectors/plugins/annotation-crud.json` | scenario | 11 | create/update/delete, the optimistic effects and their rollback, selection, props and flags, text editing, settings, projections and their memos |
| `vectors/plugins/annotation-comments.json` | scenario | 14 | comment threads and their verbs, attached links, the script commit door, attachment download, baked appearances |
| `vectors/plugins/annotation-grouping.json` | scenario | 4 | `group`/`ungroup` and their twins, the grouped Replace Text create and its rollback |
| `vectors/plugins/annotation-tools.json` | scenario | 21 | tools in the hub and the five pointer handlers as `PointerSample` sequences through `interaction.dispatch`, click placement, stamps, the file-picker port, ghosts, behaviors |
| `vectors/plugins/annotation-markup.json` | scenario | 6 | markup, caret and replace-text from the selection plugin's signals, `markupFromSelection` |
| `vectors/plugins/annotation-hover.json` | scenario | 3 | hover state, the enter/exit feed onto the actions hover pump, the permission twins |

The scenario registry runs the real interaction, selection and actions adapters beside this one.

## The API sibling plugins use

Form, link, redaction and stamp reach this plugin through the TypeScript's `/contract`,
`/contract/host`, `/authoring` and `/internal` entries. Those split one runtime object into typed
lenses; here there is one token, `ANNOTATION`, and one type, `AnnotationCapability`, whose host
members say so in their docs. The methods are the TypeScript's in snake case (`create`, `update`,
`page_items`, `hit_kind`, `set_placement_preview`, `register_behavior`, `commit_script_effects`,
...). The `comments` and `links` members are flattened to `comment_*` and `links_*` methods.

`/authoring` is `widget_appearance_from_props` and the core's `resolve_click_placement` and
`MIN_DRAG`, re-exported. `/internal` is `create_annotation_capability` and `build_tool_registry`.
The repository (`from_dto`, `to_patch`, `to_scoped_patch`, `to_create_draft`, `ref_key`,
`style_from_dto`), the reducer, the tool table (`default_tools`, `ResolvedTool`) and the five
handler constructors are public too. A sibling that implements a behavior implements the
`Behavior` trait.

Engine DTOs stay `serde_json::Value`: the model carries each one verbatim in `Annot.data`, and
every engine call sends JSON built the way the TypeScript's object literal is.

## Host inputs

`handler.ts` reads nothing from `window` or the DOM. The capability reads two ambient values, and
both come in through `AnnotationHost` on `AnnotationConfig`:

- `console_error(message, error)` receives every failure the plugin swallows (stamp and icon
  placement, the file-picker provider, attached-link sync, grouped creates, flag writes). `None`
  drops them. The vectors log them as a `console.error` callback.
- `random_uuid()` is `crypto.randomUUID()`, which `toCreateDraft` seeds `/NM` with. `None` counts up
  from `00000000-0000-4000-8000-000000000001`, which is also what the generator stubs, reset per
  case.

The other two environment seams are already explicit in the TypeScript and stay so: the file
picker is `set_file_picker_provider` (a promise of an `AttachmentFileSource` as JSON, or a falsy
value to cancel), and `appearances` takes an optional `epdf_kernel::signal::AbortSignal`. A
`Blob` has no spelling in canonical JSON, so `resolveBinarySource` handles bytes and
`{data, mimeType, name}` payloads and reproduces the TypeScript's `TypeError` for anything else.

## Turn order

The trace records how engine answers and model updates interleave, so each `await` and `.then` is
mirrored (`src/promise.rs`). A `.then` on an engine call reacts one turn after it settles; an
`await` on one adopts the `AbortablePromise` as a thenable and resumes three turns after. Every
TypeScript `async` function is a spawned `Promise`, so awaiting one costs what awaiting a native
promise does. The hydration chain `.then(...).catch(...).finally(...)` settles two turns after its
`finally` callback, because the callback's result is adopted.

## TypeScript behaviour the vectors pin

Two findings are recorded rather than fixed, since the TypeScript is the oracle:

- `armStamp` activates the `stamp` tool, and the plugin's tool-change callback disarms any tool that
  does not enable `annotation-stamp`. The built-in `stamp` tool enables `annotation-place`, so an
  armed payload is dropped the moment it is armed. Only a stamp tool configured with the legacy tag
  keeps it (`armed-stamps-need-the-stamp-tag`).
- `rehydrate()` stores whatever `runHydration` returns as the running promise. Without read
  authority that is an already-resolved promise that nothing clears, so a later `rehydrate` flags a
  re-run and returns it without asking again: the "re-check after /access" the docs describe never
  happens (`no-read-grant-means-forbidden`).

## Where the port differs in shape

The memo caches key on `Rc` identity of the model and chrome, as the TypeScript's do on object
identity. The comments index is recomputed on every read instead of memoized on `byId`/`order`
identity, which the Rust model does not carry separately; the output is the same.

`downloadAttachment` feature-detects `downloadFile` on the page service. The Rust engine contract
has no member test for page services, so the call is always made. The fake engine answers every
page member, so the TypeScript's refusal branch is unreachable in the vectors either way.

A page object number the document does not know makes `doc.page(pon)` throw. Where that throw would
escape a synchronous handler (`createStampAt`, `createIconAt`, the delete effect), the Rust skips
the call instead. The crop lookup that precedes each one comes from the same page list, so no
vector reaches it.

Prototype-named subtypes and keys (`constructor`, `__proto__`) resolve through `Object.prototype`
in the TypeScript's kind and prop tables. The Rust tables are matches over known names, so those
fall to the unsupported kind.

## Coverage

The generators reach 6,255 of the package's 6,272 statements and 1,409 of its 1,471 branches
(`vectors/coverage.json`). The 17 statements left:

- `capability.ts` 2182 to 2189, `commitScriptEffects` with no `ctx.doc`. The plugin is
  document-scoped, so its context always has a handle.
- `capability.ts` 1395 to 1397, a `createGroup` effect whose members are missing or on different
  pages. The core's group message only ever names members of one page.
- `capability.ts` 1648 and 1649, an engine page service without `downloadFile`. This one is a
  harness limit, not unreachable code: the fake engine answers every page member, so no vector can
  describe an engine that lacks one.
- `handler.ts` 454 to 456, a pointer cancel while an ink group is still inside its grouping
  window. Reachable, and not yet covered: a case would cancel between two quick ink strokes.
- `reducer.ts` 69, the `default` arm for an action outside the union, which a Rust enum can't
  express.

The 62 unreached branches are mostly the `??` and optional-chaining arms of the same guards;
`coverage.json` lists each by line and column.

## Mutation testing

Incomplete. The run went file by file (`scripts/mutants.sh --file`) and stopped partway:

| Files | Caught | Missed | Unviable |
|---|---|---|---|
| 9 complete: `effects`, `hover_feed`, `markup`, `placement`, `promise`, `reducer`, `text_item`, `threads`, `value` | 139 | 15 | 27 |
| `binary`, `capability`, `handler`, `repository/*`, `tools`, `types` | not run | | |

The 15 misses have not been triaged, and `capability.rs` and `handler.rs`, the two largest files,
are the next runs to do.
