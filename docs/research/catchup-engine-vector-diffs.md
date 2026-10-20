# Engine vector diffs after the catch-up to upstream b456ee4fd

This is the input for the Rust re-port (catch-up Task 7). It compares every file under
`tooling/engine-conformance/vectors/` at `0044ea9b9` (the last commit before regeneration, where
the vectors were still the pre-catch-up ones) with the regenerated set committed in
`34c9a6331..31ea2feeb` and the generator fixes in `ea6b5a115..016af6547` (fix round 1). Upstream is `b456ee4fd`, merged into `research/catchup-impl` at
`5097f5547`; the merge base is `2516e2786`. Commit hashes below are upstream commits in
`2516e2786..b456ee4fd` unless they are named as ours.

Every regenerate script ran twice and gave byte-identical files, and every `:check` passes on the
committed set. The Rust tests are expected to fail against these vectors until Task 8.

## Totals

Of the 50 files that existed before, 10 are unchanged, 39 changed and 1 was removed
(`forms/effects.json`). Two files are new (`forms/apply.json`, `schema/CustomMetadataPatch.json`).
The tree holds 51 files after the change.

Unchanged: `actions/submit-flags.json`, `actions/types.json`, `pages.geometry/colors.json`,
`pages.text/colors.json`, `pages.text/non-identity-text.json`, `pages.text/report.json`,
`pages.text/reverse-and-gapped-text.json`, `render/geometry.json`, `scope/parse.json`,
`text/charmap.json`.

## Generator fixes in this round

The first regeneration left three vectors that recorded something other than what their cases were
written to test. Fix round 1 changed the generators so they do again.

`text/layout.json` (our ea6b5a115). The geometry vectors are page space, and the generator fed them
to `createPdfTextLayout`, which reads PDF rects, so the layout came out with empty quads and NaN hit
probes. It now uses `createTextLayout` (`packages/engine/core/src/pageSpace/text.ts`), the layout
`page.text.layout()` returns, which mirrors the runs with `mirroredRun`, runs the PDF layout, and
flips each answer back. The vector is page space and its envelope is `schemaVersion: 2`. A page
whose quads or probe positions are not finite now stops the generator.

`search/slice-protocol.json` (our 26887941a). The cursor cases carried a hand-written key in the
pre-a10ed2659 format. They now give the cursor's state and the query it was minted for, and the
generator derives the key with upstream's `searchQueryKey` and encodes it with
`encodeSearchCursor`. A minted cursor that stops at the key check fails the generator unless the
case is about a foreign query.

`schema/MetadataPatch.json` and `schema/CustomMetadataPatch.json` (our f08cb61cf). See the schema
section.

Our 016af6547 corrected `covers` text that upstream behaviour had made false, in `forms/apply.json`
and `scope/resolve.json`. Case names stay as they were so the Rust replays keep their keys.

## How the families map

| Rust family (`crates/epdf-engine/src/...`) | Vector files | Replaying test |
|---|---|---|
| `features/annotation/` | `annotation/*.json` (8 files) | `tests/annotation_{appearance,create,delete,move,reader,update,vocabulary,write}.rs` |
| `features/forms/` | `forms/{apply,field,snapshot,write}.json` | `tests/form_{field,reader,write}.rs`; `tests/form_effects.rs` still names the deleted `forms/effects.json` and nothing replays `forms/apply.json` yet |
| `features/files/` | `files/{attachments,piece-info,write}.json` | `tests/attachment_reader.rs`, `tests/piece_info_reader.rs`, `tests/files_write.rs` |
| `features/metadata.rs`, `features/metadata_write.rs` | `metadata/read.json`, `metadata/update.json` | `tests/metadata_corpus.rs`, `tests/metadata_write.rs` |
| `features/actions/` | `metadata/actions.json`, `actions/*.json` | `tests/action_reader.rs`, `tests/submit_flags.rs` |
| `features/pages.rs`, `features/pages_write.rs` | `metadata/pages.json`, `pages/mutate.json` | `tests/pages_corpus.rs`, `tests/pages_mutate.rs` |
| `features/extract.rs` | `pages/extract.json` | `tests/page_extract_corpus.rs` |
| `features/text/` | `pages.text/*.json`, `text/charmap.json`, `text/layout.json` | `tests/text_corpus.rs`, `tests/charmap.rs`, `tests/text_layout.rs` |
| `features/geometry.rs` | `pages.geometry/*.json` | `tests/geometry_corpus.rs` (and `tests/text_layout.rs`) |
| `features/render/` | `render/geometry.json` | `tests/render_geometry.rs` |
| `features/redaction.rs` | `redaction/apply.json` | `tests/redaction_apply.rs` |
| `features/save.rs` | `save/roundtrip.json` | `tests/document_save.rs` |
| `features/search/` | `search/*.json` | `tests/search_dialect.rs`, `tests/search_slice_protocol.rs` |
| `features/security/` | `security/state.json` | `tests/security_state.rs` |
| `auth/scope/` (not under `features/`) | `scope/*.json` | `tests/scope_conformance.rs` |
| `dto.rs` | `schema/MetadataPatch.json`, `schema/CustomMetadataPatch.json` | `tests/schema_parity.rs` (checks `MetadataPatch` only; nothing replays `CustomMetadataPatch` yet) |
| `host.rs` | `wire/requests.json` | `tests/wire_contract.rs` |

## Annotation (`features/annotation/`)

Every annotation record changed shape, so the per-file notes below list only what is specific to
the file. The shared record changes are these:

| Change | Upstream |
|---|---|
| `rect` and every point move to page space: `{x, y, width, height}` from the visible box's top-left, y down | 1a243431a |
| Colours are `"#rrggbb"` strings, not `{r, g, b}` | c053c3d33 |
| Quad corners are `upperLeft/upperRight/lowerLeft/lowerRight`, not `p1..p4` | 00d2c8760 |
| `rotation` is degrees clockwise (a stored 30 reads 330) | 2bdc74fbf |
| Box kinds (square, circle, free text, stamp, caret) carry `box`; `unrotatedRect` and `rectDifferences` are gone | 2bdc74fbf |
| `ref` is `{kind, page: PageRef, objectNumber}` or `{kind: "baseIndex", page, baseIndex}`; `pageObjectNumber`, `index`, `identityQuality` and the `nm` ref kind are gone | 595bdc6c7, bc0057f3a |
| The `flags` object is flattened to top-level booleans (`print`, `hidden`, `toggleNoView`, ...) | c78997346 |
| `created`/`modified` become `createdAt`/`modifiedAt`; a file's `creationDate` becomes `createdAt` | fd175ba50 |
| `inReplyTo`/`replyType` become `reply`; `popup`, `importedBy`, `modifiedBy` are new; `createdBy`, `userId`, `groupId` read `null` instead of `""` | c78997346 |
| `hasAppearance` and `appearanceState` are new | 25dd21f38, 17350fb4c |
| `actions` is always present; reads never leave a field out | bd4976e4e |
| Opacity reads as written: `0.30196...` is now `0.3` | bd4976e4e |
| Mutation `meta` lists changed refs with their page; `weakRefsInvalidated` and `shouldRefetch` are gone | bc0057f3a |

`annotation/appearances.json` (changed). Entries are addressed by `baseIndex` refs and carry
page-space rects. The `normal` and `scaled` cases now get rollover and down appearances too,
because the renderer returns every mode and state (17350fb4c), so 7 entries become 11.
`no-appearances` went from 0 entries to 34: annotations without `/AP` are drawn in memory now
(25dd21f38, probable; I did not isolate it further). Each case's eleventh entry is a widget,
reached through the forms renderer since widgets left the annotation verbs (1233d1349).

`annotation/create.json` (changed). 63 cases become 57. The six `widget-*` cases are gone: the
annotation verbs refuse widgets (1233d1349). `square-degenerate-cloudy` and `polygon-zero-cloud`
are now refusals with `refusedWith: "InvalidArg"` (c78997346). Drafts are written in the v3 page
space shape they are sent in (our 1227addfc), so draft coordinates changed too: a y of 200 is 592
on the 792-point page. A created annotation reads `print: true` where it read `false`, from the
defaults table (69627beb9, probable), and gets a UUID `/NM` at birth, which the generator records
as `"<stamped>"` (commit not isolated; 5f4686dc8 and bc0057f3a are the candidates). Byte inputs
moved from `resources.r0` to `resources.appearance` / `resources.file` (11a1259d9). The
`ink-advisory-rotation` and `caret-rotated` `covers` text was corrected in our 34c9a6331.

`annotation/delete.json` (changed). Same six cases, with `ref`/`target` rebuilt as `objectNumber`
or `baseIndex` refs and a `request` field holding the wire request sent. The `deleted` field is
gone. `by-object-number` and `by-index-onto-a-durable-annotation` now remove three more
annotations (`a-reply`, `a-group-member`, `stamped-by-embedpdf`), so `remaining` drops from 8 to
5 and `meta.changed` lists 4 refs: deleting an annotation deletes its thread and popups in one
change (bd4976e4e).

`annotation/move.json` (changed). Same eight cases. `movedIndices` and `movedOrder` are replaced by
`order` and `request` (the `annotations.reorder` request, 1233d1349). `afterOrder` entries that the
old engine stamped with an `/NM` are now `null` or `""`: the move no longer names annotations
(bc0057f3a).

`annotation/read.json` (changed). Same five cases, same 59 annotations, new record shape. On
`annotation_kinds.pdf` the order changed: widgets now come after the other annotations on their
page, because the generator reads them back through `forms.list` (our 0044ea9b9; widgets left
`annotations.list` in 1233d1349). Value changes beyond the shared ones: `a-popup` reads as
`popup` instead of `unsupported`, with `parent` and `open` (c78997346); free texts with no font in
`/DA` read `fontFamily: ""` instead of `"helvetica"`; `an-attachment-with-no-filespec` reads
`file: null` instead of `{name: ""}`; and on `annotation_appearances.pdf` the three rotated
annotations read `rotation: null`. That fixture stores rotation in
`/EMBD_Metadata << /Rotation 30 /UnrotatedRect [...] >>`, which upstream no longer reads as the
rotation (2bdc74fbf, 6153c181e); the turn now comes from the appearance (`appearanceTurnOf`).
Three annotations with no `/NM` cannot be matched across the change by name and were compared by
position.

`annotation/update.json` (changed). 44 cases become 41: `caret-clear-rect-differences` went with
the inset (2bdc74fbf), `widget-font-size-only` and `widget-restyle` with widgets
(1233d1349). `polygon-degenerate` is now refused with `InvalidArg` (404b79b3e). Two cases swap
their appearance verdict between `regenerated` and `preserved`. Free-text updates read
`contents: ""` (69627beb9). The `polygon-clear-rotation` `covers` text was corrected in our
34c9a6331.

`annotation/vocabulary.json` (changed). The `stampName` codec is gone: upstream deleted
`internal/stampName.ts` in 0427e3dc9 and `/Name` passes through verbatim. `freeTextIntent` decodes
`FreeTextTypeWriter` as `free-text-typewriter` (404b79b3e).

`annotation/write.json` (changed). The 8 `mutations` cases are gone: they classified weak-ref
mutation impact, and upstream removed weak refs and `computeMutationImpact` (bc0057f3a). Of the 29
`appearance` cases, four change verdict. Three move from `regenerate` to `translation`, because
the update now resolves a rect patch into the shape before classifying, so the shape moves with
the rect (9d8ebf857); those cases record `resolvedPatch`. `ink-advisory-rotation` moves from
`inert` to `regenerate`, because rotation now turns a vertex kind's upright points (6153c181e).

Upstream reference for the family:
`packages/engine/core/src/annotation/` (`base.ts`, `defaults.ts`, `appearanceTurn.ts`,
`drawnPoints.ts`, `shapeForRect.ts`, `resolve/`, `kinds/`, `color.ts`, `relationships.ts`),
`packages/engine/core/src/pageSpace/annotations.ts`, `packages/engine/core/src/pageSpace/helpers.ts`
(`resolveRectCommand`, `resolveAnnotationPatch`, `resolveAnnotationDraft`),
`packages/engine/core/src/mutation/AnnotationMutationResults.ts`,
`packages/engine/services/src/features/annotations/` (`AnnotationReader.ts`,
`AnnotationMutator.ts`, `AnnotationAppearanceReader.ts`, `AnnotationBatchApplier.ts`, `internal/`),
and `packages/engine/services/src/features/changes/` for the change queue the writes go through.

## Forms (`features/forms/`)

`forms/effects.json` (removed). `forms.applyEffects` went in 98e5c270c. Its 23 cases live on in
`forms/apply.json`. `crates/epdf-engine/tests/form_effects.rs` (line 30) still names the old
path.

`forms/apply.json` (new, generator `form-apply`, our 6017f8d90 and 6754e3263). Same 23 case names
as `effects.json`, but the cases are `doc.apply` ops (`forms.setValue`, `forms.reset`,
`forms.setDisplay`) and the answer is one change, all or none. A case that used to report a
per-effect `rejected` status is now a whole-change refusal (`refusedWith`). Two reset cases that
were rejected now apply as an item with no changed fields: `a-push-button-cannot-be-reset` and
`reset-with-no-fields-is-rejected`. `a-rejection-does-not-stop-the-batch`,
`an-all-no-op-batch-produces-no-meta` and `a-no-op-in-the-middle-does-not-stop-the-batch` now
refuse whole (`InvalidArg`, `NotFound`, `NotFound`): one op fails, so nothing in the change lands.
These five keep their old names, and their `covers` text says what upstream does now (our
016af6547).

`forms/field.json` (changed). 25 cases become 23: `attach-already-attached-refused` and
`attach-off-state-refused` went with `attachWidget` (d7bd2d277). Each case carries the `request`
it sent. `delete-merged-field-refused` now succeeds: a merged field and its widget delete in one
change (60602f579). Field records: `fieldObjectNumber` folds into `ref.objectNumber` (595bdc6c7),
`flags` flattens to `readOnly`/`required`/`noExport` (c3b90c3a4), `createdBy`/`createdAt`/
`filledBy`/`filledByName`/`filledAt`/`importedBy`/`groupId` are new (b43963d12), and widgets carry
`ref`, `page` and `objectNumber` instead of `annotObjectNumber`/`pageObjectNumber` (595bdc6c7).

`forms/snapshot.json` (changed). The field record changes above, plus a top-level `widgets` list
(1233d1349), `topIndex` and `defaultValue` where they apply (bf7d4558e), and
`calculationOrder[].objectNumber` for `fieldObjectNumber`.

`forms/write.json` (changed). 18 `cases` keep their names; 10 `documentCases` become 2, because
the eight FDF/XFDF import cases went with form bundles (7f0b73a3c). The repair report's
`needAppearancesCleared` is `needsAppearancesCleared` (7bccc4f35). Each case carries its `request`.
`direct-field-refused` is refused with `NotFound` instead of `InvalidArg`, and
`text-reset-no-default` reports no changed widget where it reported one (commits not isolated).

Upstream reference: `packages/engine/core/src/forms/` (`field.ts`, `snapshot.ts`, `widgets.ts`,
`write.ts`, `effects.ts`, `predict.ts`, `value-entry.ts`), `packages/engine/core/src/pageSpace/forms.ts`,
`packages/engine/core/src/mutation/FormMutationResults.ts` and `Change.ts`,
`packages/engine/services/src/features/forms/` (`FormReader.ts`, `FormMutator.ts`, `internal/`),
and `packages/engine/services/src/features/changes/ChangeApplier.ts` for `doc.apply`.

## Files (`features/files/`)

`files/attachments.json` (changed). Items carry a `ref` instead of `key` (a9434c0e2), and
`creationDate` becomes `createdAt` with a new `modifiedAt` (fd175ba50). The value changes format
too: the old field passed the raw PDF date through (`D:20240401090000Z`), and `createdAt` is ISO
8601 (`2024-04-01T09:00:00Z`), converted by `pdfDateToIso` (fd175ba50). Fields that were absent
are now present as `null` (bd4976e4e).

`files/piece-info.json` (changed). `lastModified` becomes `modifiedAt`, and the value loses its
milliseconds: `2024-05-01T12:00:00.000Z` is `2024-05-01T12:00:00Z` (fd175ba50).

`files/write.json` (changed). Same 16 cases. A listing is `{attachments: [...]}` rather than a bare
array (a9434c0e2); created entries carry `ref`, `createdAt`, `modifiedAt` and a `description`; a
created file with no type reads `mimeType: null` instead of `"application/octet-stream"`.

Upstream reference: `packages/engine/core/src/dto/Attachment.ts`, `PieceInfo.ts`,
`packages/engine/core/src/mutation/AttachmentMutationResults.ts`,
`packages/engine/services/src/features/attachments/` and
`packages/engine/services/src/features/pieceinfo/PieceInfoAccessor.ts`.

## Metadata (`features/metadata.rs`, `features/metadata_write.rs`)

`metadata/read.json` (changed). `created`/`modified` become `createdAt`/`modifiedAt`, and the
values keep the offset the PDF date was written with instead of normalizing to UTC with
milliseconds: `rich` reads `2024-01-15T09:30:00+01:00` for `2024-01-15T08:30:00.000Z` and
`2024-02-20T12:00:00Z` for `2024-02-20T12:00:00.000Z`, and `report` reads
`2009-04-01T16:39:25-07:00` for `2009-04-01T23:39:25.000Z` (fd175ba50, `pdfDateToIso` in
`packages/engine/services/src/shared/pdf-date.ts`). The instants are the same; only the spelling
moved, so a Rust comparison has to keep the offset rather than normalize.
Custom keys still appear under `custom` because the generator folds `metadata.custom.get()` back in
(our 1b8c92a3f); upstream reads them separately since a4b8259f7.

`metadata/update.json` (changed). The date rename with the same format change: the 13 entries
that read `2024-01-15T08:30:00.000Z` now read `2024-01-15T09:30:00+01:00`, `modifiedAt` drops
`.000`, and the dates a case writes come back without milliseconds (`2021-03-04T05:06:07Z` for
`2021-03-04T05:06:07.000Z`) (fd175ba50). Also a `request` field, and three cases now refused
with `InvalidArg`: `custom-key-shadowing-a-standard-one`, `custom-key-with-a-slash` and
`custom-key-outside-ascii` (a4b8259f7). One case reads `title: ""` where it read `null`.

Upstream reference: `packages/engine/core/src/dto/DocumentMetadata.ts`, `MetadataPatch.ts`,
`CustomMetadata.ts`, `CustomMetadataPatch.ts`, `packages/engine/core/src/wire/schemas.ts`
(`MetadataPatchSchema`, `CustomMetadataPatchSchema`), and
`packages/engine/services/src/features/metadata/`.

## Actions (`features/actions/`)

`metadata/actions.json` (changed, key order only). In the `document-actions` case the additional
actions (`willClose` through `didPrint`) now come before `nameTreeScripts`, `openAction` and
`openDestination`, because `mapDocumentActions` spreads the triggers first (60283448c). The values
are identical. A replay that compares parsed JSON passes; one that compares text does not.
Destinations inside actions are page space (f5d70b67e), but this file holds none.

`actions/submit-flags.json`, `actions/types.json` (unchanged).

Upstream reference: `packages/engine/core/src/dto/PdfAction.ts`,
`packages/engine/core/src/pageSpace/destinations.ts`,
`packages/engine/services/src/features/actions/` and
`packages/engine/services/src/features/destinations/`.

## Pages (`features/pages.rs`, `features/pages_write.rs`)

`metadata/pages.json` (changed). Each page carries `ref` instead of `pageObjectNumber`
(595bdc6c7), `tabOrder` (bf7d4558e), all five boxes in page space as `{x, y, width, height}`
(d64920820, 8b6a2d0e8), and `pdfCropBox` in PDF space (60283448c). The list gains `namedPages`
(bd4976e4e). Page space ignores `/Rotate`: the 90-degree page in `rotated` still reads 612 by 792.
A close action's destination is page space (f5d70b67e).

`pages/mutate.json` (changed). 30 cases become 28: `insert-blank-past-the-end-refused` and
`move-past-the-end-refused` cannot be written with neighbour positions (1233d1349). Each case
carries its `request` (the `document.apply` change a page verb sends now, 2445caba7). `inserted`
holds refs instead of bare object numbers, `flattened` names pages by ref, and `layout` has the
page record changes above.

Upstream reference: `packages/engine/core/src/dto/PageLayout.ts`, `PageListSnapshot.ts`,
`NamedPage.ts`, `packages/engine/core/src/geometry/pageBoxes.ts` and `pageSpace.ts`,
`packages/engine/core/src/pageSpace/pages.ts`, `packages/engine/core/src/mutation/Page*.ts`, and
`packages/engine/services/src/features/pages/`.

## Extract and save (`features/extract.rs`, `features/save.rs`)

`pages/extract.json` (changed). One page's text gained trailing spaces before line breaks
(`"loop \r\nedge"` for `"loop\r\nedge"`). This comes from the PDFium text extractor, not the
TypeScript: the runtime moved from `runtime-src` `f5ca81379` to `1461b0c51` in this range, which
brings 40 upstream PDFium commits under `core/fpdftext/`. I did not isolate which one.

`save/roundtrip.json` (changed). The same trailing-space text change, and `annotationCount` on
`annotation_kinds.pdf` drops from 39 to 37 because `annotations.list` no longer returns the two
widgets (1233d1349).

Upstream reference: `packages/engine/services/src/features/pages/PagesExtractor.ts`,
`packages/engine/services/src/features/save/`, `packages/engine/core/src/dto/PageExtract.ts`.

## Text and geometry (`features/text/`, `features/geometry.rs`)

`pages.geometry/*.json` (5 changed, `colors.json` unchanged). Runs use `start` for `charStart` and
a page-space `rect`; glyphs use `loose`/`tight` page-space boxes or quads for
`looseBox`/`looseQuad`/`tightBox`/`tightQuad`, and `space`/`empty` booleans for the `flags` bits
(a10ed2659, 00d2c8760, 3bad82c8d). Rotated runs give `rotation` in clockwise degrees instead of
radians (6153c181e: 0.5236 is now 330). The loose boxes are also shorter: PDFium now bounds
`FPDFText_GetLooseCharBox` top and bottom by `/FontBBox` (`runtime-src` 2558d5d97 and ff5e04acc).
`font-size-zero` has no runs, and `embedded-orientations` lost the two runs of its degenerate
line, which matches PDFium's `CPDF_TextPage: Ignore non-finite page objects` (`runtime-src`
06fbf69b3, probable).

`pages.text/embedded-orientations.json`, `pages.text/font-size-zero.json` (changed). The same two
PDFium changes on the text side: `" Degenerate"` and the zero-size `"A"` are no longer extracted.
The other four `pages.text` files are unchanged.

`text/layout.json` (changed, generator fixed in our ea6b5a115). Envelope `schemaVersion` 1 to 2.
Quads are page-space `PageQuad`s with named corners (`upperLeft` ... `lowerRight`) instead of
`p1..p4` in PDF space, segment rects are page boxes (`{x, y, width, height}`) instead of
`left/bottom/right/top`, and hit probes are page-space points. The glyph, run and frame counts
follow the geometry: `embedded-orientations` has 69 glyphs and 9 runs (80 and 11 before) and
`font-size-zero` has none, both from the PDFium changes above. `frameCount` is unchanged at 4,
because frames are keyed on the y-up runs through upstream's `mirroredRun`.

Some layout answers moved with the geometry, not with the layout code, so they are not layout bugs
to chase. On `report` page 0, glyph 211's line went from `[156, 253]` to `[197, 211]`, the word
range `(207, 6)` now draws as 2 segments instead of 1, and `(197, 15)` is a new line range. On
`report` page 4, glyph 3715's line went from `[3659, 3736]` to `[3661, 3736]`. The line grouping
reads the loose boxes, which PDFium now bounds by `/FontBBox` (`runtime-src` 2558d5d97), so
shorter boxes split or join lines differently; that cause is likely, not proven. On
`embedded-orientations`, the probe glyphs shifted with the two dropped runs, and the centre probe
of glyph 11 now hits glyph 12. On `non-identity-text`, the centre probe of glyph 1 hits glyph 0,
as it did before. The Rust layout has to
take page-space runs, or mirror them the way `createTextLayout` does.

`text/charmap.json` (unchanged).

Upstream reference: `packages/engine/core/src/dto/PageGeometrySnapshot.ts`, `PageTextSnapshot.ts`,
`packages/engine/core/src/text/layout.ts` (`createPdfTextLayout`, and the page-space
`createTextLayout`), `packages/engine/core/src/pageSpace/text.ts`,
`packages/engine/services/src/features/geometry/PageGeometryReader.ts` and
`packages/engine/services/src/features/text/PageTextReader.ts`. The PDFium side is the
`runtime-src` submodule at `1461b0c51`.

## Render (`features/render/`)

`render/geometry.json` (unchanged). Render targets moved to page space in 7f107fd7c and
`renderMatrix` replaced `displayRectToDeviceMatrix` (771342379), but the matrices this file
records came out the same.

## Redaction (`features/redaction.rs`)

`redaction/apply.json` (changed). 10 cases become 9: `batched-index-refs-refused` went with index
refs (bc0057f3a). Each case carries its `request` with the v3 scope, `{pages}` or `{annotations}`
of refs (a9434c0e2). Results name pages by `page` ref. `remaining` loses the two widgets
(`one-marking-by-name` 38 to 36, `two-markings-by-name` 37 to 35, `whole-page` and
`page-with-no-markings` 36 to 34), so its order shifts (1233d1349).

Upstream reference: `packages/engine/services/src/features/redaction/RedactionApplier.ts`,
`packages/engine/core/src/mutation/RedactionApplyResult.ts`.

## Search (`features/search/`)

`search/fold.json`, `search/literal.json`, `search/regex-dialect.json` (changed). A range's
`length` is `count` (a10ed2659). Nothing else moved.

`search/snippet.json` (changed). A snippet is `{before, match, after}` instead of
`{text, matchStart, matchLength}` (a10ed2659).

`search/slice-protocol.json` (changed). Keys spell the mode `snippets` or `matches` instead of
`full` or `rects` (a10ed2659). They also carry the `ignoreWhitespace` flag, but that predates the
range (f082bf3e4, before the merge base): the old vector was cut before it and was already stale
there. Each case now records `request`, the flattened `SearchRequest` actually built and checked
(query fields at top level, `snippets`, `from` as a `PageRef`, the cursor string), and `case`, the
case as written in the pre-a10ed2659 spelling (our 0963cacd3). Rust replays `request`. The six
cases that carry a well-formed cursor send one minted with the current key (our 26887941a). The
five about the cursor checks reach the check they are about again: `cursor-resumes-its-own-query` and
`cursor-beside-the-matching-start-page-is-allowed` accept, `cursor-from-another-query-is-refused`
stops at the key, `cursor-from-before-a-mutation-is-refused` at the sequence, and
`cursor-beside-a-conflicting-start-page-is-refused` at the start page, whose message is now
"`from` conflicts with the cursor" (the request field is `from`, a `PageRef`, since a10ed2659).

Upstream reference: `packages/engine/core/src/search/` (`fold.ts`, `literal.ts`, `regex.ts`,
`snippet.ts`, `matcher.ts`, `types.ts`, `epoch.ts`) and
`packages/engine/services/src/features/search/`.

## Security, scope and schema

`security/state.json` (changed). One value: the `annotate-and-modify` advisory (bits 4 and 6) now
says `canAssemble: true`. Upstream grants page assembly with bit 4 or 11, as Acrobat Pro does
(5443a5649, `materializePdfPermissions`). That is the whole reason `security:check` differed, and it is upstream behaviour, recorded as it
is; the generator and its cases are unchanged.

`scope/resolve.json` (changed). Expansions gain capabilities in three groups. `doc.text.select` and
`doc.text.search` appear wherever text copy did (a10ed2659, 5443a5649). `pdf.permissions` grants
`doc.forms.fill` and `doc.pages.assemble` on more bit sets (5443a5649). `doc.sign` and
`doc.outline.modify` join the expansions (4b2cdb395 and 67ccb76dc). Two collab probes flip to
allowed: a `group=finance` filter now matches a finance target whether or not the caller belongs
to finance, because 4b2cdb395 dropped the membership test from `filterMatches`. This is upstream
behaviour. The case keeps the name `group-filter-needs-membership`, and its `covers` text now says
the rule changed (our 016af6547).

`scope/parse.json` (unchanged).

`schema/MetadataPatch.json` (changed, cases rewritten in our f08cb61cf). The first regeneration
flipped eight cases to `valid: false` for the wrong reason: three `custom-*` cases sent a field
that left `MetadataPatch` for its own patch (a4b8259f7), and five `created-*` cases sent `created`,
which the `.strict()` schema refuses as an unknown key since the field became `createdAt`
(fd175ba50). The date cases now send `createdAt` and `modifiedAt` under their old names, and test
dates again. Against the old vector, `created-offset` flips to valid, because `IsoDateTimeSchema`
is `datetime({ offset: true, local: true })` (fd175ba50); its `covers` text says so. Three cases are
new: `created-without-offset` (valid, the `local: true` form), `created-pre-v3-spelling` (invalid,
`created` is an unknown key) and `custom-is-its-own-patch` (invalid, `custom` is an unknown key).
The four `custom-*` cases are removed from this file.

`schema/CustomMetadataPatch.json` (new). The four `custom-*` cases, sent as the patch itself
(`CustomMetadataPatchSchema`, a record of string to string-or-null, a4b8259f7). `custom-empty`,
`custom-per-key-three-state` and `custom-empty-key` are valid; `custom-null` is not. The Rust side
needs a `CustomMetadataPatch` type in `dto.rs` and a `check::<CustomMetadataPatch>` in
`tests/schema_parity.rs`.

Upstream reference: `packages/engine/core/src/auth/scope/` (`resolver.ts`, `pdf-bits.ts`,
`types.ts`), `packages/engine/core/src/wire/schemas.ts`, `packages/engine/core/src/dto/IsoDateTime.schema.ts`,
`packages/engine/services/src/features/security/`.

## Wire (`host.rs`)

`wire/requests.json` (changed). 65 kinds become 89: 38 added, 14 removed, 49 kept with changed
fields, 2 kept unchanged (`abort`, `shutdown`). The brief's example of `signatures.abort` becoming `signatures.cancel` is
not visible in this file: the old vector held no `signatures.*` kinds at all.

Removed (14): `annotations.listFullPage`, `annotations.listRawAll`, `annotations.listRawPage` (one
`annotations.list`, 83f6598d2); `annotations.move` (`annotations.reorder`, 1233d1349);
`attachments.create`, `attachments.delete` (change ops, c2d7a5194); `forms.applyEffects`
(98e5c270c); `forms.attachWidget` (d7bd2d277); `pages.delete`, `pages.insert`,
`pages.insertBlank`, `pages.move`, `pages.rotate` (page structure through `document.apply`,
119685ea4 and b69c40377); `pieceInfo.clear` (`pieceInfo.delete`, a9434c0e2).

Added (38). New in the range: `annotations.list` and `annotations.export` (83f6598d2, 843deedb4),
`annotations.import` (73f05157f), `annotations.readAppearance` (7f0b0a183), `annotations.reorder`,
`forms.reorderWidgets`, `forms.updateWidget`, `forms.deleteWidget` (1233d1349), `document.apply`,
`document.applyChanges`, `forms.addWidget` (2445caba7), `forms.importValues` (7f0b73a3c),
`forms.reorderCalculations` (98718b780), `metadata.readCustom`, `metadata.updateCustom`
(a4b8259f7), `objectNumbers.reserve` (b32fdf584), `outline.read` (67ccb76dc),
`pages.workingSet` (f550e9e5f), `pieceInfo.delete` (a9434c0e2), `signatures.cancel`
(7bccc4f35). The rest (`signatures.analyze`, `.complete`, `.contents`, `.digest`,
`.finalizeCandidate`, `.list`, `.prepare`, `.revisionBytes`, `measure.setScale`,
`measure.viewports`, `pages.setName`, `pages.removeName`, `document.version`,
`document.setFontSettings`, `fonts.authorizeEditing`, `annotations.flatten`,
`annotations.exportAppearance`, `forms.setSignatureAppearance`) already existed upstream at the
merge base but were not in the `WorkerRequest` union our branch had when the old vector was cut
(our d8946591d); they arrive with merge 5097f5547.

Kept with changed fields (49). Every job request gains an `effect` tag (`read`, `write`,
`contentWrite`, `snapshot`, `open`, `close`, `runtimeWrite`; f550e9e5f). Writes gain `opId` and
optional `objectNumberFloor` (b32fdf584, 630b77eab); several gain an `authority` (91404cfcb,
2445caba7). `pageObjectNumber` becomes `page: PageRef` and `pageObjectNumbers` becomes
`pages: PageRef[]` (595bdc6c7). Drafts and patches are typed `AnnotationDraft<C>` /
`AnnotationPatch<C>` in page space. `forms.reset` takes `refs`, `forms.import` takes a `bundle`,
`forms.export` takes a `selection`, `search.query` takes a `SearchScanRequest`, and the open kinds
take `objectNumbers`, `reserveObjectNumbers` and `signedDocumentPolicy`.

Upstream reference: `packages/engine/core/src/wire/worker-protocol.ts`,
`packages/engine/core/src/wire/schemas.ts` and `changeSchemas.ts`, and
`packages/engine/services/src/worker-host/WorkerHost.ts`.

## Cross-cutting changes

Page space. Every position the engine hands out is now page space: x right from the visible box's
left edge, y down from its top edge, in PDF units, ignoring `/Rotate`
(`packages/engine/core/src/geometry/pageSpace.ts`). The worker converts at one place,
`packages/engine/services/src/worker-host/pageSpaceBoundary.ts`, on the way out
(`resultInPageSpace`) and on the way in (`pdfAnnotationDraftOf`, `pdfAnnotationPatchOf`,
`pdfFormFieldDraftOf`, `pdfRenderTargetOf`). The vectors record page space because that is what
the API answers, and the Rust host should port that boundary rather than convert in each
feature. It touches annotation, forms, pages, geometry, text, search and actions. The commits are
60283448c (codec), 8b6a2d0e8 (boxes), f5d70b67e (destinations), 3bad82c8d (text and search),
7f107fd7c (render and appearances), 1a243431a (annotations) and c053c3d33 (forms and measure). The
cost is that the Rust readers stay in PDF space internally and every DTO needs a page-space twin
at the boundary, as the TypeScript does with `Coordinates`.

Recorded requests. Generators whose case literals keep an older spelling now record the wire
request they actually sent, in a `request` field: `annotation/delete.json`,
`annotation/move.json`, `forms/field.json`, `forms/write.json`, `metadata/update.json`,
`pages/mutate.json` and `redaction/apply.json`. Job and op ids are fixed as `"<job>"` and
`"<op>"`. Rust should replay `request`, not rebuild a request from the case fields.

`doc.apply`. Form effects, page structure edits and attachment writes are changes on one queue,
applied by `document.apply` (2445caba7, 98e5c270c, 119685ea4, c2d7a5194). A change is all or
none: one refused op refuses the whole change, which is why `forms/apply.json` records whole
refusals where `forms/effects.json` recorded per-effect statuses.

References and identity. `nm` and `index` refs are gone; an annotation is named by
`{page, objectNumber}` or `{page, baseIndex}` (bc0057f3a, 595bdc6c7). Every ref of kind
`objectNumber` names its number `objectNumber`. This hits annotation, forms, pages, redaction and
the wire.

Widgets belong to forms. `annotations.list` does not return widgets and the annotation verbs refuse
them (1233d1349). That one change moves `save/roundtrip.json`, `redaction/apply.json`,
`annotation/read.json`, `annotation/appearances.json` and the widget cases in create and update.

Dates. One `IsoDateTime` type and the `createdAt`/`modifiedAt` names everywhere (fd175ba50):
annotations, attachments, piece info, metadata, form fields and the schema vector. The value format
changed with the names. A PDF date reads through `pdfDateToIso` as ISO 8601 with the offset the
file wrote (`+01:00` stays `+01:00`, `Z` or a zero offset is `Z`, no offset stays none) and no
milliseconds. Before, metadata normalized to UTC with `.000`, piece info carried `.000`, and
attachments passed the raw `D:` string through. A write takes ISO or a `Date` and stores the
offset given. The Rust readers need the same formatter, and every replay that compares dates
needs the offset kept, not normalized.

The PDFium runtime. The `runtime-src` submodule moved from `f5ca81379` to `1461b0c51`. That
accounts for the text and geometry value changes (trailing spaces, dropped degenerate and
zero-size text, shorter loose boxes), which no TypeScript commit explains. A Rust engine that
links its own PDFium must be on the same runtime build to match these vectors.
