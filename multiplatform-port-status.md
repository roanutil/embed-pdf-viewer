# Rust port status

What is ported, what pins it, and what is left. `multiplatform-port-order.md`
is the plan; this is where the plan actually got to. Regenerate nothing from
this file: every claim below points at a test you can run.

Run everything with `cargo test --workspace`, which was 310 test binaries (doc-tests
included) with 2,875 tests passing and 1 ignored at `172f5369d`, plus `cargo clippy --workspace --all-targets -- -D warnings`. The
TypeScript half is `pnpm --filter @embedpdf/tooling-engine-conformance run
<name>:check` for each of the thirty-three check scripts, all wired into
`.github/workflows/engine-rust.yml`.

Every group from G0 through G15 now has a Rust implementation, every one of
the sixty-five wire kinds reaches it, and all three bindings call it. What is
left is listed under "Not started" below: the event DELIVERY that the port
order keeps in TypeScript, five viewer flows nobody has written, and a handful
of behaviours this corpus cannot distinguish, each documented where it lives.

Task 14.3 (`1d1ba44cc` to `577185b0b`) changed the engine in six places, each pinned by a test:
a text run that claims past its glyphs no longer panics the layout, `reorder_part` places a key
named twice twice, as the TypeScript does, the open-failure message follows the TypeScript's, an
empty password counts as none, a creation draft refuses a point count that is not a whole `u32`
instead of clamping it, and the native host's password prompt and create/import checks match the
TypeScript's. All 33 check scripts are current at `172f5369d`.

## Done, and pinned to the TypeScript by vectors

A group is listed here when a Rust implementation exists AND a committed vector
file generated from the shipping TypeScript is replayed against it. Vectors
live under `tooling/engine-conformance/vectors/`, and every generator has a
`:check` script wired into `.github/workflows/engine-rust.yml`.

| Group | What | Pinned by |
|---|---|---|
| G1 | `MetadataPatch` three-state serialization | `vectors/schema/`, `tests/schema_parity.rs` |
| G2 | `pages.geometry` | `vectors/pages.geometry/`, `tests/geometry_corpus.rs` |
| G3 | `auth/scope` resolver | `vectors/scope/`, `tests/scope_conformance.rs` |
| G3 | security derivation | `vectors/security/`, `tests/security_state.rs` |
| G4 | render geometry | `vectors/render/`, `tests/render_geometry.rs` |
| G5 | search dialect, fold, literal, snippet | `vectors/search/`, `tests/search_dialect.rs` |
| G5 | character-to-text translation | `vectors/text/charmap.json`, `tests/charmap.rs` |
| G5 | `pages.text` | `vectors/pages.text/`, `tests/text_corpus.rs` |
| G5 | text layout, hit tests, segments | `vectors/text/layout.json`, `tests/text_layout.rs` |
| G6 | `metadata.read` | `vectors/metadata/read.json`, `tests/metadata_corpus.rs` |
| G6 | `pages.list` | `vectors/metadata/pages.json`, `tests/pages_corpus.rs` |
| G6 | SubmitForm flags, action vocabulary | `vectors/actions/`, `tests/submit_flags.rs` |
| G6 | `actions.read`, page and annotation action trees | `vectors/metadata/actions.json`, `tests/action_reader.rs` |
| G7 | annotation vocabulary: subtypes, flags, enums, `/IT`, `/State` | `vectors/annotation/vocabulary.json`, `tests/annotation_vocabulary.rs` |
| G7 | `annotations.listRawAll`, `annotations.listRawPage` | `vectors/annotation/read.json`, `tests/annotation_reader.rs` |
| G7 | `annotations.renderAppearances`, placement | `vectors/annotation/appearances.json`, `tests/annotation_appearance.rs` |
| G8 | `attachments.list`, `attachments.readFile` | `vectors/files/attachments.json`, `tests/attachment_reader.rs` |
| G8 | `forms.list`, and G7's widget-to-field join | `vectors/forms/snapshot.json`, `tests/form_reader.rs` |
| G8 | `pieceInfo.read`, `pieceInfo.applications` | `vectors/files/piece-info.json`, `tests/piece_info_reader.rs` |
| G9 | `pages.extract`, read back semantically | `vectors/pages/extract.json`, `tests/page_extract_corpus.rs` |
| G10 | `document.saveBuffer`, both modes, read back semantically | `vectors/save/roundtrip.json`, `tests/document_save.rs` |
| G11 | the appearance and mutation-impact classifiers | `vectors/annotation/write.json`, `tests/annotation_write.rs` |
| G11 | `annotations.create`, every writable subtype | `vectors/annotation/create.json`, `tests/annotation_create.rs` |
| G11 | `annotations.delete`, all three reference kinds | `vectors/annotation/delete.json`, `tests/annotation_delete.rs` |
| G11 | `annotations.update`, every writable subtype, with the appearance verdict | `vectors/annotation/update.json`, `tests/annotation_update.rs` |
| G11 | `annotations.move` | `vectors/annotation/move.json`, `tests/annotation_move.rs` |
| G12 | `forms.setValue`, `forms.reset`, `forms.import`, `forms.repair` | `vectors/forms/write.json`, `tests/form_write.rs` |
| G12 | `forms.createField`, `updateField`, `deleteField`, `attachWidget`, `detachWidget` | `vectors/forms/field.json`, `tests/form_field.rs` |
| G13 | `pages.move`, `rotate`, `delete`, `insertBlank`, `insert`, `flatten` | `vectors/pages/mutate.json`, `tests/pages_mutate.rs` |
| G13 | `redaction.apply`, both scopes | `vectors/redaction/apply.json`, `tests/redaction_apply.rs` |
| G14 | `metadata.update`, three-state over `/Info` | `vectors/metadata/update.json`, `tests/metadata_write.rs` |
| G14 | `pieceInfo.update`, `clear`, `attachments.create`, `delete` | `vectors/files/write.json`, `tests/files_write.rs` |

## Done, tested against fixtures rather than vectors

These have tests but no cross-language comparison, because the behaviour is
either PDFium's own or has no TypeScript counterpart to compare with. Treat
them as verified, not as pinned.

| Group | What | Tests |
|---|---|---|
| G3 | session lifecycle, locking, leases | `tests/document_session.rs` |
| G3 | control frames, base registry | `tests/session_registry.rs` |
| G3 | `SecurityReader` over encrypted fixtures | `tests/security_reader.rs` |
| G4 | render pixels, tiles, alpha, budget | `tests/render_pixels.rs` |
| G4 | font registry, fallback order | `tests/font_registry.rs` |
| G5 | `search.query` slices and cursors | `tests/search_reader.rs` |
| G7 | appearance pixels, the clamp, the y-up flip | `tests/annotation_appearance.rs` |
| G8 | `forms.export`, FDF and XFDF envelopes | `tests/form_reader.rs` |
| G9 | `pages.extract` refusals and lifetime | `tests/page_extract.rs` |
| G10 | layer artifact round trip, encrypted save | `tests/document_save.rs` |
| G11 | a created annotation's appearance, and its survival of a save | `tests/annotation_create.rs` |
| G10 | `finishMutation`, and the event vocabulary | `tests/mutation_finalization.rs` |
| G15 | `document.renderPageFile`, and the injected image encoder | `tests/server_surfaces.rs` |
| cross-cutting | rotation normalization | `tests/rotation_normalization.rs` |
| cross-cutting | cancel traces | `tests/cancel_traces.rs` |
| B4 | signatures: `signatures.list`, `contents`, `digest`, `revisionBytes`, `analyze`, `prepare`, `complete`, `abort`, `document.version`, `forms.setSignatureAppearance` | `vectors/signature/protection.json`, `tests/signature_protection.rs`; fixture tests `tests/signature_read.rs`, `signature_analyze.rs`, `signature_sign.rs` |

## Not started, and what each gap actually is

Phase 2 is done: every read group from G2 through G9 is ported and gated. So
are G10 through G15. What follows is the residue: one layer the plan keeps in
TypeScript, five viewer flows, and a set of behaviours the corpus cannot tell
apart.

All fourteen viewer flows exist in `tooling/viewer-e2e/tests/`, but two of
them cover less than the port order asks, and the reason in both cases is a
viewer feature rather than a test.

F12 wants rotate, delete and reorder driven through the UI. The viewer has no
page-assembly UI at all: `rotate:clockwise` in the command table is
`stage.rotateView`, a camera move that writes nothing, and the sidebar
thumbnails carry no controls, no hover affordances and no context menu. The
`page-edit` capability exists and is registered, but it sits outside the
curated token list `@embedpdf/viewer` publishes. What F12 does assert is the
half the port order asks for separately: page identity, object numbers and
order alike, surviving a save and a reopen.

F14 wants undo and redo. There is no history plugin —
`packages/viewer/chrome/src/config/commands.ts` defines `history:undo` and
`history:redo` with an empty `run` and `enabled: () => false`, under the
comment "no history plugin in v3 yet". F14 asserts that the buttons are shown
and disabled, so the day a plugin lands, the assertion fails and names the
flow to write.

G10 is done. The save landed earlier, the session authority is G3's
`SessionAuthority`, and `finishMutation` with the event vocabulary is in
`session/save.rs`.

What stays TypeScript is the DELIVERY. `services/src/events/EventHub.ts` is
the publisher and the port order keeps it there; what Rust owns is the
vocabulary, the provenance envelope, and the judgement about which family a
mutation belongs to. The acknowledged stream, its deduplication and its
resync after a worker failure are the hub's, and they have no Rust half to
port.

`annotations.create` and `annotations.update` both cover all eighteen
writable subtypes. The two they do not are the two that cannot be drafted: a
popup is a child of the annotation it belongs to rather than a thing a caller
places, and `unsupported` is the reader's name for a subtype this build does
not model.

G12 is complete. `forms.applyEffects` is in `features/forms/effects.rs`,
pinned by `vectors/forms/effects.json`. What is NOT ported is the thing that
produces the effects: a form script runs in `packages/core/js-sandbox` on web
and is unsupported on mobile, and that is a capability rather than a port. The
applier takes the decisions a script already made.

`metadata.update` cannot say "the title is deliberately blank". The wire has
three states and `Clear` is one of them, but `EPDF_SetMetaText` removes the
entry for an empty string as readily as for a null, so both leave a document
with no `/Title`. The TypeScript's `writeMetaText` claims it dropped "v2's
empty-string removes quirk"; it did not, because the quirk is in the fork,
below both engines. `a_clear_and_an_empty_value_are_the_same_edit` records it
against the dictionary.

`pages.flatten` does not distinguish its two usages on this corpus, and the
corpus tried. `annotation_appearances.pdf` carries a square with `/F 36`,
which prints and never shows on screen, and that is the one annotation the
display and print flattens should disagree about. They do not: both leave the
same four annotations and render the same pixels, so a build that passed
`Display` for both passes every vector. The mapping is written the way the
fork's header names it.

`forms.deleteField` is the field-plane half only. The TypeScript's
`WorkerHost` cascades into the annotation plane to take the detached widgets
off their pages, and that cascade is the host's composition rather than a
feature: it calls two ported operations in order. `deleting_a_field_leaves_its
_widgets_on_the_page` asserts the half that IS here, and
`delete-merged-field-refused` records what the cascade does to a merged
field/widget dictionary: the annotation plane will not delete a widget, so
the whole operation is refused.

G13 is done: page assembly and redaction both.

`redaction.apply` reports a collateral count that is zero on every case, and
the corpus cannot make it anything else. The count is annotations removed
ALONGSIDE the markings, which is popups cascading and widgets detaching, and
no marking in `annotation_kinds.pdf` has either. Giving one a `/Popup` would
mean restructuring that fixture: its annotations are direct dictionaries in
`/Annots`, so there is no object number to point at.

`pages.insert` takes an OPEN source document rather than bytes, and the
reason is a lifetime. `FPDF_ImportPagesByIndex` does not fully detach what it
copies: an imported stream still reads through the source, so the source has
to outlive every future save of the destination. The TypeScript parks it on
the session and releases it at close; here it is a borrow, which makes the
requirement the caller's to satisfy and the compiler's to check.

G14 is done: all five of its operations.

G15 is done, and it is the one group with no vectors. The port order says
why: "no new Rust behavior beyond G4 raster and G10 save". What is there is
composition, and what it composes is already pinned, so
`tests/server_surfaces.rs` checks the joins instead: that a path opens and
closes, that an index resolves to the page a session would have named, and
that an engine with no encoder refuses rather than inventing one.

The encoder is injected for the reason the TypeScript injects it: a PNG
library in the engine would ship to every target to satisfy the one that
needs it. Two of the three targets refuse the encoded kinds today and this
port keeps them refusing.

## The wire, and the three bindings

`crates/epdf-engine/src/host.rs` routes all sixty-five kinds: one JSON
envelope in, one out, binary payloads positional on both sides. Each binding
is one export over it. `epdf_host_call` is the C ABI, with the payload
framing in `host::frame` because a pointer and a length cannot carry a list;
`PdfEngine::call` and `Engine::call` need no framing, since uniffi and napi
already carry a list of byte arrays.

The cost is stated because the plan asks for it. Swift, Kotlin and TypeScript
see strings and byte arrays rather than typed records, so a facade decodes the
JSON itself and a mistyped argument is a runtime refusal rather than a compile
error. The typed alternative is sixty-five signatures per language that no
compiler checks against the TypeScript they mirror, and
`vectors/wire/requests.json` checks these: the TypeScript compiler reads the
`WorkerRequest` union, records every kind with its arguments, and
`tests/wire_contract.rs` fires each one and fails on `NotImplemented`.

Forty-eight kinds have a vector pinning their BEHAVIOUR against the
TypeScript. Seventeen reach an implementation whose only gate is that routing
check, which proves the arm exists and says nothing about what it does;
`tooling/engine-conformance/OPERATIONS.md` marks which is which.

## The Swift plugins

Every Rust plugin crate is wrapped for Swift in `platforms/apple/EmbedPDF`,
per `docs/superpowers/plans/2026-10-06-swift-plugins/00-umbrella.md`. Each
row is the plugin's `ViewerConfig` field, its Swift source under
`Sources/EmbedPDF/Plugins/`, and the test file under `Tests/EmbedPDFTests/`
that runs it on the iOS simulator. `every_plugin_builds_together` in
`crates/epdf-viewer-host/tests/viewer_host.rs` installs all of them at once.

| Plugin | `ViewerConfig` | Swift | Tests |
|---|---|---|---|
| metadata | `metadata` | `Metadata.swift` | `MetadataTests.swift` |
| search | `search` | `Search.swift` | `SearchTests.swift`, `EmbedPDFUITests/SearchModelTests.swift` |
| render | `render` | `Render.swift` | `RenderTests.swift`, `RenderViewportTests.swift` |
| interaction | `interaction` | `Interaction.swift` | `InteractionTests.swift`, `PointerLatencyTests.swift` |
| feedback | `feedback` | `Feedback.swift` | `FeedbackTests.swift` |
| stage | `stage` | `Stage.swift` | `StageTests.swift`, `EmbedPDFUITests/StageReaderModelTests.swift` |
| selection | `selection` | `Selection.swift` | `SelectionTests.swift` |
| link | `link` | `Link.swift` | `LinkTests.swift` |
| annotation | `annotation` | `Annotation.swift` | `AnnotationTests.swift` |
| form | `form` | `Form.swift` | `FormTests.swift` |
| redaction | `redaction` | `Redaction.swift` | `RedactionTests.swift` |
| measurement | `measurement` | `Measurement.swift` | `MeasurementTests.swift` |
| stamp | `stamp` | `Stamp.swift` | `StampTests.swift` |
| page-edit | `page_edit` | `PageEdit.swift` | `PageEditTests.swift` |
| view-manager | `view_manager` | `ViewManager.swift` | `ViewManagerTests.swift` |
| shell | `shell` | `Shell.swift` | `ShellTests.swift` |
| i18n | `i18n` | `I18n.swift` | `I18nTests.swift` |
| commands | `commands` | `Commands.swift` | `CommandsTests.swift` |
| actions | `actions` | `Actions.swift`, `JavaScriptCoreEngine.swift` | `ActionsTests.swift`, `JavaScriptCoreEngineTests.swift` |
| signature | `signature`, `scope` | `Signature.swift`, `KeychainSigner.swift` | `SignatureTests.swift`, `SecurityTests.swift`, `KeychainSignerTests.swift` |

The cost is in the archive. `libepdf_viewer_host.a` for
`aarch64-apple-ios-sim` release was 94,029,176 bytes before the first plugin
and is 216,589,976 bytes with all twenty; B5 owns trimming it.
Re-measured on 2026-10-15 after the Swift review fixes: 218,818,352 bytes
(same slice, release profile). The `libEmbedPDFFFI.a` that
`build-xcframework.sh` packs from it is about the same size now, 218,826,528
bytes for the simulator; until `9c0e38d66` the script merged
`libembedpdf.a` into it a second time, though rustc already bundles PDFium
into the Rust archive, and it was 238,628,784.

Task 14.7 (`00cfc02bf` to `172f5369d`) migrated the Swift package to the plugin ffi shapes the
upstream catch-up landed. The package compiles for the iOS simulator against a stand-in XCFramework
(139 compile errors down to 0), but it does not link: the runtime pin `926c126b2` has no
`ios-arm64` or `ios-sim-arm64` build on disk, so the simulator run and the UI tests, which compile
only, are not done. The 327 `EmbedPDFTests` ran on macOS in a scratch harness, all passing. Building
the XCFramework is a runtime-fork payload step and is yours to run. Also left for you, and not
caused by Task 14: 10 `core-wasm` `test:wasm` failures, 8 `core-wasm` typecheck errors
(`pnpm --filter @embedpdf/core-wasm typecheck`, present at `4a0b9c126`), and 11 wasm failures in
`crates/epdf-web/tests/exports.rs` (no annotation or form plugin factories). The archive sizes above predate
the catch-up and were not re-measured.

## Where the Rust core actually runs

The published WASM payload carries it. `scripts/build-target.sh` builds
`crates/epdf-wasm` and links it, required rather than opt-in:
`EMBEDPDF_RUST_CORE=0` opts out for a build without a Rust toolchain, and
`scripts/verify-rust-core.mjs` asserts a payload has it.

That assertion is what makes the suites above it worth running. Both
`geometry-rust-parity.test.ts` and `session-rust-rehearsal.test.ts` skip
themselves on a runtime without the core, so before the payload included one
they were green and had never run: nothing in CI built a Rust-linked runtime.
The `rust-routing` job in `engine-rust.yml` builds one and runs both.

G3w is `RustSessionRehearsal`, behind an internal `rustSession` flag. It
routes a document's SESSION to Rust — the fat-memory open, the two security
calls, the two closes — and no read. A read that reaches such a document is
reported as a routing bug rather than as "document not open", which would be
true and useless. It exists so that G10, which moves session authority for
every document, is not the first time a Rust-owned session runs on web.

The viewer flows run on either backend: `VIEWER_E2E_BACKENDS=typescript,rust`
doubles the projects and the flows are written once. They are not in CI yet,
because that job fetches a pinned runtime payload and the core reaches a
payload only once one is built after the link became the default.

## Known gaps inside what is done

`open_layer_delta` and `open_layer_artifact` are tested only on their refusal
paths. Nothing produces a delta yet: `EPDFLayer_SaveDelta` is G10.

`SecurityReader::read_live` reproduces a defect rather than fixing it. It
reads `FPDF_GetDocUserPermissions` while describing it as the effective word,
so an owner-unlocked session reports itself restricted. That matches the
TypeScript, which is why it stays; `reader::LIVE_PERMISSIONS_DEFECT` and
`tests/security_reader.rs` both say so.

`openDestination` is absent from `actions.read`. It needs
`EPDFDoc_GetOpenActionDest`, which the pinned fork artifact `e4174c4f9` does
not export. The comparison drops the field rather than faking it, and
`tests/action_reader.rs::the_open_destination_form_is_not_read_by_this_build`
fails if a corpus document ever gains a destination-form `/OpenAction`.

The widget appearance-generation fallback in
`features/annotation/appearance.rs` never runs. The reader samples the
available-modes word once, before any generation could happen, and then skips
every mode whose bit is clear in that sample. Not a PDFium limitation:
`tests/annotation_appearance.rs::generating_a_form_fields_appearance_never_reaches_the_reader`
calls `EPDFAnnot_GenerateFormFieldAP` directly and it works. Reproduced from
the TypeScript, which has the same structure.

`forms.export` and `pages.extract` are compared semantically rather than
byte for byte, because the port order says the conformance suites do not
establish byte-stable PDF serialization. An export or an extract is reopened
and asked what it is; its object numbering is not compared to anything.

`document.saveLayerBuffer` can write an artifact and cannot write a bare
delta. `EPDFLayer_SaveDeltaToOwnedBuffer` refuses a layer with no promoted
objects, and nothing in this repository can promote one until a mutator runs
against a layer session.
`tests/document_save.rs::a_layer_session_writes_an_artifact_that_opens_again`
asserts both halves.

A word can extend past its own line in the text layout. The word walk follows
glyph flags and the line walk follows runs, so two text objects that abut with
no space between them give a word spanning two orientations. Reproduced from
the TypeScript and recorded in
`tests/text_layout.rs::a_word_can_cross_a_line_boundary_between_abutting_runs`.

## Fixtures we added

`tooling/engine-conformance/fixtures/generate.mjs` writes all of these, and CI
diffs its output, so they are reproducible rather than committed binaries
nobody can rebuild.

`encrypted-r2-rc4-40.pdf`, `encrypted-r3-rc4-128.pdf`,
`encrypted-r4-aes-128.pdf`, `encrypted-r6-aes-256.pdf`,
`encrypted-r4-all-allowed.pdf`, `encrypted-r4-empty-user-password.pdf` and
`unsupported-security.pdf` cover the security handler revisions and the
unsupported-handler path. `fixtures/encrypt.mjs` implements enough of the
Standard security handler to write them, RC4 included, because OpenSSL 3
dropped it.

`rotated_pages.pdf` is four pages at `/Rotate` 0, 90, 180 and 270. Nothing in
the repository set `/Rotate` before it, which is how `OwnedDocument::page`
shipped using the un-normalized loader.

`non_identity_text.pdf` is a page whose character list and text are different
sizes. Every other fixture has an identity char map.

`reverse_and_gapped_text.pdf` has a run reading right to left and a run with a
gap wide enough to split it. Mutation testing found both untested.

`rich_metadata.pdf` has an absent key, a present-but-empty key, a UTF-16 title
outside Latin-1, and dates in both the offset and Z forms.

`page_actions.pdf` and `document_actions.pdf` carry the page and catalog
action slots. Nothing in the repository had a `/AA` on either.

`annotation_base.pdf` varies the annotation shell and holds the subtype
roughly constant: an indirect annotation, a direct one with `/NM`, a direct
one without, a direct one whose `/NM` is empty, all three `/RT` cases,
`/EMBD_Metadata`, a `/Rect` in reversed corner order, and a `/Sound` no reader
claims.

`annotation_kinds.pdf` does the opposite: one annotation per subtype, paired so
each resolved default sits beside an explicit entry. A decorated square against
a bare circle, a strikeout with `/IT` against one without, a free text with a
full `/DA` against one with none.

`annotation_appearances.pdf` is the only fixture with baked `/AP` streams, and
most of it is near misses for the rotation-stripping rule: a callout and a
polygon that carry rotation metadata and must still be placed by `/Rect`, and a
square carrying the logical box with no angle. It also carries the only
`/AcroForm` widget outside `form_fields.pdf`, plus an indirect widget attached
to nothing, which is what makes the widget-to-field join a test.

`attachments.pdf` is the first `/EmbeddedFiles` tree: a key that is not the
display name, an entry with nothing optional, a zero-byte file, two entries
sharing one stream, a filespec with no `/EF`, and a tree entry whose value is
the number 42.

`form_fields.pdf` is the first form: one field of every family, a radio group
that is one field with two widgets, a `/V` that is an array, a `/V` that is a
number, a field the `/AcroForm` tree forgot, and a `/CO` slot naming nothing.

`piece_info.pdf` puts different data on the catalog and on a page under one
application name, and gives the page more applications than the catalog so a
listing that used the wrong count loses one rather than running past the end.

The write vectors have no new fixtures. Every create and update case builds
its own subject from a draft, so each says what it starts from rather than
depending on how some fixture annotation happened to be written; delete and
move run against `annotation_base.pdf`, because both are about identity and
order and that is the file whose annotations differ in both.

## Three places where the families disagree on purpose

`/DA` packs a font, a size and a colour into one string, so a patch naming one
member has to decide what the other two are. Free text READS them off the
annotation, because a text box's styling is an edit. A redaction DEFAULTS
them, because §12.5.6.14 ties the entry to `/OverlayText` rather than to the
annotation, so the entry is written whole whenever the label's styling is
touched. A widget also defaults them, because one writer serves it and
`forms.createField`'s inline placements, which makes a widget's styling a
whole description rather than an edit. `redact-label-colour-only` and
`widget-font-size-only` record the defaults that result.

`/Rect` is derived from the quads on a text-markup patch, derived on a
redaction patch only when the patch did not supply one, and never derived on
either draft. §12.5.6.10 requires a markup's box to be the smallest enclosing
one; a redaction's can carry a margin the quads do not describe, so a patch
that moves the regions and says nothing about the box is asking for the box to
follow, and one that supplies both has already decided.

`/Q` on a widget is the one write in these writers whose refusal is ignored.
PDFium declines it on a widget with no `/FT`, and an inert widget has none:
the alignment belongs to the field's text, and there is no field yet.

## How the tests were checked

Every conformance runner in this port was mutation-tested: the implementation
was deliberately broken in several ways and the suite had to fail each time.
Where a mutation survived, the corpus gained a case or a fixture rather than
the mutation being written off. The commit messages name which mutations were
tried and which ones needed new coverage.

Pass the patterns to the harness as data, not through the shell. The first
version interpolated them into a `python3 -c` string, where a multi-line
pattern could silently fail to match while the mutant was still reported as
surviving. That produced one false negative, on sorting a `pages.extract`
selection, and the four mutants recorded as equivalent were re-verified
afterwards.

Fourteen survive in total and each is documented at the line it survives on.
They fall into three groups, and the grouping is worth more than the list.

Some are guards for a case the corpus cannot produce: the redundant appearance
guard, the unexercised attachment-tree skip, the negative blend-mode cast.

Some are writes whose effect a reader normalizes away, so the difference is in
the FILE rather than in anything this project can observe. Writing a `/BE` of
intensity zero is one, because `read_cloudy_intensity` filters a zero back to
absent; so is a `/Fit` destination carrying a parameter, because PDFium pads
the array back up to the fit type's arity on the way out either way.

The largest group was writes that are inert on the create path and
load-bearing on the update path. Every clear is one of these: there is nothing
on a new annotation to remove, so clearing an interior colour, an unrotated
box or a widget's `/MK` entry passes every create vector whether it runs or
not. Extending `update` to those subtypes closed all of them, which is what
the group was there to predict.

One survivor is left that no case can reach. Clearing a link's target removes
both `/A` and `/Dest`, and nothing in this engine writes a direct `/Dest`, so
every link the vectors build carries an `/A` alone and a build that removed
only that one passes. Seeing it would take a fixture link written by something
else, which is exactly the foreign file the guard is for.
