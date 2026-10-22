# Operation matrix

Generated from `src/operations.ts`. Do not edit by hand; run
`pnpm --filter @embedpdf/tooling-engine-conformance run operations`.

Every one of the 89 `WorkerRequest` kinds, the port group that owns it,
the ownership stage it moves under, the phase that moves it, and the
cancellation case it has to document. Group assignments come from
`multiplatform-port-order.md` Section 3; cancellation cases from
`multiplatform-plan.md` Section 7.

## G2

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `pages.geometry` | A | 0 | cooperative | ported | Answers PageGeometrySnapshot in page space: the reader answers the file's coordinates (runs with start, loose/tight boxes or quads, space/empty, rotation in degrees clockwise) and the host boundary converts them (pageGeometryOf). The C ABI export epdf_page_geometry answers the file's coordinates, which RustPageGeometryReader.ts reads as PageGeometrySnapshot<PdfCoordinates>. Gated by tooling/engine-conformance/vectors/pages.geometry through Host::call in crates/epdf-engine/tests/geometry_corpus.rs, and crates/epdf-wasm/tests/borrowed_page.rs. |

## G3

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `open.fatMem` | A | 1 | queued-only | routed | No throwIfAborted reaches PdfDocumentOpener: once an open starts it runs to completion. |
| `open.layerMemBase` | A | 1 | queued-only | routed |  |
| `open.layerFileBase` | A | 1 | queued-only | routed |  |
| `document.checkPasswordPermissions` | A | 1 | queued-only | ported | Answers { security, protection } as WorkerHost.handleDocumentCheckPasswordPermissions does; on a locked session it loads the parked document with the password. |
| `document.probeSecurityFile` | A | 1 | queued-only | ported | Answers { security } as WorkerHost.handleDocumentProbeSecurityFile does. Needs no session. |
| `close` | control | 1 | control | routed | Idempotent: rejects new work, invalidates handles, releases on the owner thread. Closing an engine closes its documents. |
| `layer.close` | control | 1 | control | routed |  |
| `abort` | control | 1 | control | routed | Handled in WorkerHost.receive before anything is queued (WorkerHost.ts:413), as pages.workingSet is (WorkerHost.ts:418). The other 87 of the 89 kinds reach a switch: 76 arms in run() (WorkerHost.ts:554) and 11 sliced reads in slicedWork() (WorkerHost.ts:1896). |
| `shutdown` | control | 1 | control | routed |  |
| `document.version` | A | 1 | queued-only | routed | SignatureReader.version (WorkerHost.ts:1018-1024): the hash and length of the base the session is on. |
| `objectNumbers.reserve` | A | 1 | queued-only | routed | A session effect: moves the layer's last object number up. Only a session that hands out object numbers itself takes it. Every session is a layer: open.fatMem opens its bytes as a fresh layer over a private base, as WorkerHost.openBytesAsLayer does. |
| `pages.workingSet` | A | 1 | non-preemptible | routed | Never answers and never queues: WorkerHost.receive takes it on arrival, even mid-render (WorkerHost.ts:418), and jobId only names the message. So it is not queued-only, since there is no queue to drop it from; an abort finds no job and the bookkeeping always completes, which is non-preemptible with no wait. Not one of the four control frames (worker-kinds.ts), so it cannot carry the control case. Rust records the sets on the page pool of an open session and answers null, since Host::call answers every call; input it cannot read is dropped and answered null too, as upstream drops it unanswered; the pool keeps no page past its last holder, as an upstream pool without a residency keeps none. |

## G4

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `pages.render` | A | 1 | non-preemptible | ported | Cooperative up to FPDF_RenderPageBitmapWithMatrix (PageRenderReader.ts:51), which cannot be interrupted. Rust takes the target in page space and answers page and area, converting both at the host boundary (page_space::boundary, pdfRenderTargetOf); usage, the dpi viewport and includeFormFields are not read yet. |
| `fonts.register` | A | 1 | queued-only | routed | Engine-level, not per document. One Rust FontRegistry per combined worker owns the key-to-ID map and the retained handles. |
| `fonts.addFallback` | A | 1 | queued-only | routed |  |
| `fonts.clearFallbacks` | A | 1 | queued-only | routed |  |
| `fonts.clear` | A | 1 | queued-only | routed |  |
| `fonts.authorizeEditing` | A | 1 | queued-only | routed |  |
| `document.setFontSettings` | A | 1 | queued-only | routed | Per-document session state on the handle (EPDFDoc_SetFontEmbeddingPolicy, EPDFDoc_SetTypographicFeatures, WorkerHost.ts:2285-2290), never written to the file: a session effect. Filed with the fonts it configures. |

## G5

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `pages.text` | A | 2 | cooperative | ported | Already coarse (PageTextReader.ts:93). Gate on correctness, not on the boundary. Answers PageTextSnapshot, which holds no places, so the boundary passes it through. Gated by vectors/pages.text in crates/epdf-engine/tests/text_corpus.rs. |
| `search.query` | A | 2 | cooperative | ported | Takes the flattened SearchScanRequest (query fields at the top level, snippets, from as a PageRef, cursor, limit) and answers SearchSlice: matches as page ranges (start, count) with a snippet of before, match and after, pagesSearched and pageCount. The cursor key is searchQueryKey, mode spelled snippets or matches. The reader answers segments in the file's coordinates and the host boundary converts them (pageSearchSliceOf). Gated by vectors/search through crates/epdf-engine/tests/search_dialect.rs, search_slice_protocol.rs, search_reader.rs and search_ignore_whitespace.rs. |

## G6

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `metadata.read` | A | 2 | cooperative | ported | Gated by vectors/metadata/read.json through crates/epdf-engine/tests/metadata_corpus.rs. The standard fields only: createdAt and modifiedAt keep the offset the file wrote (pdfDateToIso); the custom keys are metadata.readCustom. |
| `metadata.readCustom` | A | 2 | cooperative | ported | Gated with metadata.read: the generator folds metadata.custom.get() into the read it records, and metadata_corpus.rs does the same. |
| `pages.list` | A | 2 | cooperative | ported | Page identity must agree between lazy per-page lookup and full enumeration (DocumentSession.ts:41-44). Rust reads the boxes by pageBoxesOf and /Rotate by pageRotationOf, and answers them in page space at the host boundary (pageListOf). |
| `actions.read` | A | 2 | cooperative | ported | Rust reads openDestination and answers every destination in page space at the host boundary (mapDocumentActions), triggers first. |
| `outline.read` | A | 2 | cooperative | routed | New in 67ccb76dc. Answers the DocumentOutline, every item depth first, each destination of its target and its action in page space at the host boundary (pageOutlineOf); no page is loaded. The outline's writes are the document.apply ops outline.create, outline.update, outline.move and outline.delete. No vector covers it: crates/epdf-engine/tests/outline.rs ports runOutlineConformance. |
| `measure.viewports` | A | 2 | cooperative | routed |  |

## G7

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `annotations.renderAppearances` | A | 2 | non-preemptible | ported | One family per request (annotations, or widgets for page.forms). Every mode and every state an annotation stores, each labelled (17350fb4c); an annotation with no normal appearance is drawn in memory into EPDFAnnot_GetDrawingRect's box, never written (25dd21f38). A box kind drawn turned renders upright into its box, the turn passed to EPDF_RenderAnnotBitmapUnrotated (runtime 5e9241945). Refs by baseIndex or object number; rects converted to page space at the host boundary (pageAppearancesOf). Gated by vectors/annotation/appearances.json through crates/epdf-engine/tests/annotation_appearance.rs. |
| `annotations.list` | A | 2 | cooperative | ported | Replaces annotations.listRawAll, listRawPage and listFullPage. Unlike listFullPage it builds no widget rows (RawAnnotationReader.list with the 'annotations' family), so it never reaches the cached form model; the widget-to-field join and its repeated-read benchmark moved to forms.list. Answers AnnotationList in upstream's record shape, read in the file's coordinates and converted at the host boundary by field space (pageAnnotationOf); a promoted inline annotation reads by its birth name (EPDFLayer_GetBirthName). Gated by vectors/annotation/read.json through crates/epdf-engine/tests/annotation_reader.rs. |
| `annotations.readAppearance` | A | 2 | cooperative | ported | An annotation's appearance as a one-page PDF (bytes): EPDFAnnot_ExportAppearance with the turn the annotation reads taken out (exportUnturnedAppearance). Only a kind with an appearance resource (the stamp) has one. |

## G8

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `attachments.list` | A | 2 | cooperative | ported | Answers { attachments } (AttachmentList, a9434c0e2): each entry carries its ref, createdAt and modifiedAt as ISO dates, and null for every field the PDF has no value for. Gated by vectors/files/attachments.json through Host::call. |
| `attachments.readFile` | A | 2 | cooperative | ported |  |
| `annotations.readFile` | A | 2 | cooperative | ported | Byte equality with the TypeScript path, not a semantic comparison. Its ref resolves as upstream resolveAnnotIndexRaw does, a baseIndex on a promoted page through the layer's births. |
| `pieceInfo.read` | A | 2 | cooperative | ported |  |
| `pieceInfo.applications` | A | 2 | cooperative | ported |  |
| `forms.list` | A | 2 | cooperative | ported | Filed by behaviour: a read despite the forms. prefix. Answers FormSnapshot: the fields in upstream's shape (ref by objectNumber, the /Ff bits as readOnly, required and noExport, groupId and attribution from the field's /EMBD_Metadata) and every widget row the pages' /Annots hold, each joined to its field (widgetRows.ts, joinWidgetField.ts). The reader answers the file's coordinates and the host boundary converts them (pageFormSnapshotOf). Upstream joins through its cached form model (acquireFormModel) and carries the repeated-read benchmark annotations.listFullPage carried; the Rust host has no form model cache and builds one model per read. Gated by vectors/forms/snapshot.json through crates/epdf-engine/tests/form_reader.rs. |
| `forms.export` | A | 2 | cooperative | routed | The fields a selection takes, each whole with its widgets, as a form bundle in page space (FormExporter, 7f0b73a3c, which removed FDF and XFDF); a snapshot. No vector: gated by crates/epdf-engine/tests/form_transfer.rs and the bundle unit tests (features/transfer). |
| `annotations.export` | A | 2 | cooperative | routed | Annotations and their resources as one bundle, in page space (AnnotationExporter); a snapshot, like forms.export. Each resource once, by its SHA-256. No vector: gated by crates/epdf-engine/tests/annotation_transfer.rs, whose export of the two Acrobat stamp fixtures hashes to the TypeScript engine's file. |

## G9

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `pages.extract` | A | 2 | cooperative | ported | Creates and closes its own scratch document (PagesExtractor.ts:73-112), a named exception to Stage A "retains nothing, closes nothing": FPDF_CreateNewDocument with FPDF_ImportPagesByIndex for a plain copy, EPDFDoc_ExportPagesFlattened with flatten (what it paints arrives already checked against the caller's rights, resolvePageExtract), either keeping the title. Takes PageRefs in the caller's order. Gated by vectors/pages/extract.json, which records the sources sent, through crates/epdf-engine/tests/page_extract_corpus.rs; page_extract.rs ports the extract conformance suite. |
| `annotations.exportAppearance` | A | 2 | cooperative | ported | Flattens chosen annotation appearances, named by objectNumber or baseIndex refs, into a new single-page PDF; a snapshot, like pages.extract. Builds a scratch document (EPDFPage_ExportAnnotationsAsDocument) and closes it before returning. Gated by vectors/pages/mutate.json, replayed by crates/epdf-engine/tests/flatten_corpus.rs; annotation_flatten.rs ports the export conformance suite. |

## G10

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `document.saveBuffer` | B | 4 | after-first-write | ported |  |
| `document.saveFile` | B | 4 | after-first-write | ported |  |
| `document.saveLayerBuffer` | B | 4 | after-first-write | ported |  |

## G11

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `annotations.create` | C | 4 | after-first-write | ported | A one-op change (applyOne) and an op of document.apply: the v3 draft in page space, converted at the boundary (pdfAnnotationDraftOf), checked field by field against its kind (prepareCreate), and made on its page's /Annots without loading the page (EPDFPage_CreateAnnotRaw). Stamped with a UUIDv7 /NM when it names none; a stamp places a drawing found or added once per artwork (drawingFor); its reverse deletes it while it reads as left. Measurement fields (measure, captions, leader) are refused NotImplemented until 8.25. Gated by vectors/annotation/create.json through crates/epdf-engine/tests/annotation_create.rs. |
| `annotations.update` | C | 4 | after-first-write | ported | A one-op change (applyOne) and an op of document.apply: the v3 patch in page space, converted at the boundary (pdfAnnotationPatchOf), resolved against the annotation's read (pdfResolveAnnotationPatch: a drawn kind's rect becomes the shape that puts the drawing there, what it sends back as it reads is dropped, each field checked against its kind's update schema, what follows stated), then written in place by its kind's writer. The appearance verdict classifies the resolved patch (appearanceImpactOf); /RD is the appearance's. A widget is refused for doc.forms.updateWidget. Its reverse puts back by value the fields it changed that still show it, until 8.30 brings the dictionary captures. Measurement writes (a scale, a leader, a shown or placed caption, a derived label) are refused NotImplemented until 8.25. Gated by vectors/annotation/update.json and write.json through crates/epdf-engine/tests/annotation_update.rs and annotation_write.rs. |
| `annotations.delete` | C | 4 | after-first-write | ported | A one-op change (applyOne) and an op of document.apply: the annotation goes with its thread and popups in one change (deletedWith), each checked against the authority before the first write, the page's inline annotations promoted first so a baseIndex keeps naming them. Answers only meta, the annotation first in meta.changed; the annotations.deleted event lists them all. A widget is refused for the form's ops. Its reverse brings everything back from the fork's capture at the same numbers and positions (annotation.restore), and that restore's reverse deletes it again, so an undone create can be redone. Only a layer can capture, and open.fatMem opens one, so a change that deletes beside other ops is taken back whole when a later op refuses. Gated by vectors/annotation/delete.json through crates/epdf-engine/tests/annotation_delete.rs. |
| `annotations.import` | C | 4 | after-first-write | routed | A one-op change (the annotations.import op, which document.apply refuses and document.applyChanges runs): planned before the first write (planAnnotationImport), then made as one batch (AnnotationBatchApplier). Its reverse removes what it made, thread by thread; a restoring import's steps take its rights. No vector: gated by crates/epdf-engine/tests/annotation_transfer.rs. |
| `annotations.reorder` | C | 4 | after-first-write | ported | Replaces annotations.move (1233d1349). A one-op change and an op of document.apply: the rows go together, in the order given, beside a neighbour of their own family or at the start or the end (ListPosition), as one block move in /Annots; widgets have their own order. Nothing is renamed (bc0057f3a). Answers the page's whole new order. Its reverse puts each row back beside its old neighbours while it is still where the reorder left it. Gated by vectors/annotation/move.json through crates/epdf-engine/tests/annotation_move.rs. |
| `document.apply` | C | 4 | after-first-write | routed | One change as one transaction: annotation, form, page, attachment, metadata and outline ops (Change.ts). Rust runs the metadata, attachment, annotation, page and form value ops, the form's design ops: fields, widgets and the calculation order, and the outline ops (features/changes, tests/document_apply.rs, tests/files_write.rs, tests/page_structure.rs, tests/form_apply.rs, tests/form_widgets.rs, tests/outline.rs) and answers NotImplemented naming the owner sub-task for every other op type; an op's bytes ride as a request payload its resources name, { payload: n } (a pages.insert source too, or the list of bytes a recorded request holds); a refused change is taken back through its ops' reverse steps. Carries the page and attachment writes that were their own kinds, and their invariants. Page identity survives a pages.reorder op within the same live document: FPDF_MovePages rewrites the page tree and leaves /Annots alone, so every page and annotation ref, baseIndex included, names the same thing after it (page_structure.rs). A pages.insert op's source is retained on one owner, the session, until it closes (PagesInserter.ts:88, session.retainUntilClose, reached through changes/internal/pageChanges.ts:53), so a source document handle never crosses owners. Each page op's position is a neighbour or an end (ListPosition), resolved when the op runs; its reverse is pages.rotate, pages.reorder, pages.restore (the fork's page capture, names included) or pages.remove (guarded by a digest of the page). Gated for the page ops by vectors/pages/mutate.json through crates/epdf-engine/tests/pages_mutate.rs. |
| `document.applyChanges` | C | 4 | after-first-write | routed | A server request's changes as one job, each in its own layer transaction. Rust runs the metadata ops, as document.apply does. |

## G12

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `forms.setValue` | C | 4 | after-first-write | ported | A one-op change (forms.setValue, features/changes/forms.rs): the value as a read returns it ({ value }, { checked }, { selectedValues }), written as the field's rules leave it (fieldAfterValue) through the fork's value writers, answered { field, widgets, meta } with FormMutationMeta and a forms.valueSet event; its reverse puts back the field's and widgets' dictionaries from the fork's captures, else the value. Gated by vectors/forms/write.json (tests/form_write.rs) and vectors/forms/apply.json (tests/form_apply.rs). |
| `forms.reset` | C | 4 | after-first-write | ported | A one-op change (forms.reset) over refs, or every field without them: each field back to its /DV through the value writers (fieldAfterReset), answered { fields, widgets, meta }. This host answers the forms.valueSet facts (formResetFacts) as an events array beside the result. Gated by vectors/forms/write.json and vectors/forms/apply.json. |
| `forms.import` | C | 4 | after-first-write | routed | A form bundle's design as a one-op change (the forms.import op; FDF and XFDF import went in 7f0b73a3c): planned before the first write (planFormImport), each field created with its widgets, flags and value, attribution restored or stamped. Its reverse deletes each field it made. No vector: gated by crates/epdf-engine/tests/form_transfer.rs. |
| `forms.repair` | C | 4 | after-first-write | ported | Not a change: a final one that ends undo. Answers the report with needsAppearancesCleared (7bccc4f35) and an emptyMeta that cannot be undone, and publishes forms.repaired. Gated by vectors/forms/write.json documentCases. |
| `forms.createField` | C | 4 | after-first-write | ported | A one-op change (forms.create, features/changes/fields.rs): the v3 draft in page space, each widget placement converted on its page (pdfFormFieldDraftOf), checked before the first write (placedWidgetOf, onStateOf, defaultEntryOfDraft, maxLength), then the field, its settings and /DV, who made it and its group, its widgets born through the annotation plane and adopted, and its place in the calculation order when it has a calculate script, all in one layer transaction. Answered { field, widgets, meta } with a forms.created event; its reverse deletes the field while nobody changed it since (field.remove). Gated by vectors/forms/field.json through crates/epdf-engine/tests/form_field.rs, and tests/form_widgets.rs. |
| `forms.updateField` | C | 4 | after-first-write | ported | A one-op change (forms.update): the patch checked against the field's family (assertPatchFitsFamily), its default against the field as the patch leaves it (formFieldAfter), then each setting written, /DV through EPDFForm_SetFieldDefaultValue, new options with the selection that goes with them. Its reverse puts the field's and widgets' dictionaries back from the fork's captures, else each property still holding what the update set (revertProperties). Gated by vectors/forms/field.json through crates/epdf-engine/tests/form_field.rs. |
| `forms.deleteField` | C | 4 | after-first-write | ported | A one-op change (forms.delete): the field leaves the calculation order, the tree, and its widgets their pages, in one write; a merged field/widget goes in the same cascade (60602f579). Answers only meta (the field in changedFields, its widgets in changedWidgets) and a forms.deleted event; its reverse brings the field back from the fork's capture (EPDFForm_ExportFieldRawToOwnedBuffer, field.restore), then into the calculation order. Gated by vectors/forms/field.json through crates/epdf-engine/tests/form_field.rs. |
| `forms.detachWidget` | C | 4 | after-first-write | ported | A one-op change (forms.removeWidget): the widget leaves its field and stays on its page; a merged field/widget is refused. Answers { field, widgets, meta }, the widget's row in no field; its reverse puts the field's and the widget's dictionaries back from the fork's captures. Gated by vectors/forms/field.json through crates/epdf-engine/tests/form_field.rs. |
| `forms.addWidget` | C | 4 | after-first-write | ported | A one-op change (forms.addWidget, replacing forms.attachWidget, d7bd2d277): the placement in page space converted on its page (pdfWidgetPlacementOf), the widget born, styled and added to the field in one layer transaction; a merged field splits, its widget half at splitObjectNumber. Answers { field, widgets, meta }; its reverse puts back the field's and the pages' dictionaries. Gated by crates/epdf-engine/tests/form_widgets.rs. |
| `forms.updateWidget` | C | 4 | after-first-write | ported | A one-op change (forms.updateWidget): the widget patch written as an annotation update writes it (place, turn, /MK, /BS, /DA, /Q, caption or check style, actions), a caption only on a push button's and a check style only on a toggle's (fitWidgetToFamily), drawn again as its field's family draws it. Its reverse puts back by value what still shows the update, until 8.30 brings the dictionary captures. Gated by crates/epdf-engine/tests/form_widgets.rs. |
| `forms.deleteWidget` | C | 4 | after-first-write | ported | A one-op change (forms.deleteWidget): the widget leaves its field (its dictionaries captured) and its page (the fork's annotation capture); a merged field/widget is refused for forms.delete. Its reverse brings it back at its place and into its field (widget.restore). Gated by crates/epdf-engine/tests/form_widgets.rs. |
| `forms.reorderWidgets` | C | 4 | after-first-write | ported | A one-op change (forms.reorderWidgets): the page's widgets in their own stacking order, apart from its annotations. Its reverse puts them back beside their old neighbours (annotation.reorder, family widgets). Gated by crates/epdf-engine/tests/form_widgets.rs. |
| `forms.reorderCalculations` | C | 4 | after-first-write | ported | A one-op change (forms.reorderCalculations, 98718b780): the fields go together to a position in /AcroForm /CO. Its reverse takes back what it did where nobody changed it since (orderBack). Gated by crates/epdf-engine/tests/form_widgets.rs. |
| `forms.importValues` | C | 4 | after-first-write | routed | A form bundle's values as a one-op change (the forms.importValues op): each field filled by full name (planFormValuesImport); its reverse puts back each value and who filled it. No vector: gated by crates/epdf-engine/tests/form_transfer.rs. |
| `forms.setSignatureAppearance` | C | 4 | after-first-write | routed |  |

## G13

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `pages.flatten` | C | 4 | after-first-write | ported | A content write and a final change: promotes each page, answers per-page applied or unchanged and MutationMeta, and its event carries the write's opId. A failing page or a cancel refuses the call and its layer transaction takes back the pages already flattened, with no sequence bump. Gated by vectors/pages/mutate.json, replayed by crates/epdf-engine/tests/flatten_corpus.rs. |
| `redaction.apply` | C | 4 | after-first-write | ported | A content write and a final change, answered like pages.flatten: takes a {pages} or {annotations} scope, promotes each page, applies a page's refs in one native call, and answers per-page applied or unchanged with MutationMeta. A failing page or a cancel refuses the call and its layer transaction takes back the pages already promoted or applied, with no sequence bump. A no-op answer keeps mutationSequence (kept divergence, 87b61b533). Gated by vectors/redaction/apply.json, replayed by crates/epdf-engine/tests/redaction_apply.rs. |
| `annotations.flatten` | C | 4 | after-first-write | ported | A content write and a final change: pages.flatten for a chosen set of one page's annotations, named by objectNumber or baseIndex refs. Promotes the page's inline annotations first, answers per-ref applied or unchanged and MutationMeta, and its event carries the write's opId. Gated by vectors/pages/mutate.json, replayed by crates/epdf-engine/tests/flatten_corpus.rs; annotation_flatten.rs ports the conformance suite. |
| `pages.setName` | C | 4 | after-first-write | ported | A write in its own transaction answering PageNameResult: the new layout, with namedPages, and MutationMeta naming no page; its pages.named event names the key and the page. Gated by crates/epdf-engine/tests/named_pages.rs, which ports the named pages conformance suite. |
| `pages.removeName` | C | 4 | after-first-write | ported | A write in its own transaction answering PageNameResult: the new layout, with namedPages, and MutationMeta naming no page; its pages.named event names the key and the page (null once removed). Gated by crates/epdf-engine/tests/named_pages.rs, which ports the named pages conformance suite. |

## G14

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `metadata.update` | C | 4 | after-first-write | ported | MetadataPatch is the Phase 0 DTO codegen proof: three-state Keep \| Clear \| Set(T). Gated by vectors/metadata/update.json through crates/epdf-engine/tests/metadata_write.rs, which replays the recorded request. Runs as a one-op change, so its opId undoes it. |
| `metadata.updateCustom` | C | 4 | after-first-write | ported | Gated with metadata.update by vectors/metadata/update.json. A standard key, a leading slash or a key outside printable ASCII is InvalidArg before anything is written; answers the custom keys alone. |
| `pieceInfo.update` | C | 4 | after-first-write | ported | Answers PieceInfoUpdateResult, { pieceInfo, meta }, as WorkerHost.handlePieceInfoUpdate does, and publishes no event. |
| `pieceInfo.delete` | C | 4 | after-first-write | ported | Replaces pieceInfo.clear. Answers { meta } and publishes no event. |
| `measure.setScale` | C | 4 | after-first-write | routed |  |

## G15

| Operation | Stage | Phase | Cancellation | Rust | Note |
| --- | --- | --- | --- | --- | --- |
| `pages.renderEncoded` | C | 5 | non-preemptible | routed | Sole owner of the wire kind; the raster step comes from G4 internal entry point. Encoding stays host-injected. Rust takes the target in page space, as pages.render does. |
| `annotations.renderAppearancesEncoded` | C | 5 | non-preemptible | routed |  |
| `document.renderPageFile` | C | 5 | non-preemptible | routed | The target is page space and converts on the page the render opens (renderOptionsInFileSpace), since no session carries the document to the host boundary. |
| `document.renderPageFileEncoded` | C | 5 | non-preemptible | routed | The target is page space, as document.renderPageFile takes it. |
| `signatures.list` | A | 5 | queued-only | routed | No port group in multiplatform-port-order.md covers signing yet; parked in G15 until the port order assigns one. WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them. |
| `signatures.analyze` | A | 5 | queued-only | routed | WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them. |
| `signatures.contents` | A | 5 | queued-only | routed | WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them. |
| `signatures.digest` | A | 5 | queued-only | routed | WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them. |
| `signatures.revisionBytes` | A | 5 | queued-only | routed | WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them. |
| `signatures.prepare` | B | 5 | after-first-write | routed | A snapshot, like the saves. Never touches the live document: the candidate is parked on the session until complete or cancel. Filed after-first-write by its stage and effect, like the saves, though WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them today. |
| `signatures.complete` | C | 5 | after-first-write | routed | A content write. Installs the sealed bytes as the session document (DocumentSession.install), which advances the edit sequence itself. Filed after-first-write by its stage and effect, like the other content writes, though WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them today. |
| `signatures.cancel` | A | 5 | queued-only | planned | A session effect. Still planned: this branch's Rust host keeps the arm under its pre-rename kind, signatures.abort, which the wire answers as an unknown kind and only Host::call_legacy reaches (crates/epdf-engine/src/host.rs, LEGACY_KINDS) until Task 8.26 renames it. WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them. |
| `signatures.finalizeCandidate` | C | 5 | queued-only | routed | Session-less, like document.renderPageFile: patches the caller's candidate file in place and reads it back through a document that is never a session. Kept at stage C though its effect is read: no session's document changes, but it opens, writes and closes a document of its own. queued-only, not after-first-write: the handler gets no AbortSignal (WorkerHost.ts:626-627, 1071-1083), so once it starts it runs to the end. |
