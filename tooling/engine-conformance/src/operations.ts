/**
 * The operation matrix: every `WorkerRequest` kind, the port group that owns
 * it, the ownership stage it moves under, and the cancellation case it has to
 * document.
 *
 * `multiplatform-plan.md` Phase 0 asks for this, and
 * `multiplatform-port-order.md` Section 6 asks CI to fail when a kind reaches
 * `worker-protocol.ts` without a row here. `check-operations.ts` is that
 * check; it reads the kinds out of the TypeScript type rather than trusting
 * this file to be complete.
 *
 * Group assignments come from `multiplatform-port-order.md` Section 3. Nothing
 * here decides anything the port order has not already decided; changing a row
 * means changing that document too.
 */

/** Port groups, as `multiplatform-port-order.md` Section 3 names them. */
export type PortGroup =
  | 'G2'
  | 'G3'
  | 'G4'
  | 'G5'
  | 'G6'
  | 'G7'
  | 'G8'
  | 'G9'
  | 'G10'
  | 'G11'
  | 'G12'
  | 'G13'
  | 'G14'
  | 'G15';

/**
 * Ownership stage from `multiplatform-plan.md` Section 6.
 *
 * `control` is not a stage. The four control frames steer the worker rather
 * than asking it for a result, and they belong to G3 on every host.
 */
export type OwnershipStage = 'A' | 'B' | 'C' | 'control';

/**
 * Cancellation case from `multiplatform-plan.md` Section 7. Every operation
 * documents exactly one.
 *
 * - `queued-only`: the job can be dropped before it starts and not after. No
 *   `throwIfAborted` reaches the handler.
 * - `cooperative`: the token is checked between units of work, before any
 *   write.
 * - `non-preemptible`: cooperative up to one synchronous native call that
 *   cannot be interrupted. The wait ends; the work completes.
 * - `after-first-write`: bookkeeping completes. Mutations are not
 *   rollback-atomic, and an `AbortError` is never evidence of rollback.
 * - `control`: a control frame, which is the thing that cancels rather than
 *   the thing cancelled.
 */
export type CancellationCase =
  | 'queued-only'
  | 'cooperative'
  | 'non-preemptible'
  | 'after-first-write'
  | 'control';

export interface OperationRow {
  readonly kind: string;
  readonly group: PortGroup;
  readonly stage: OwnershipStage;
  /** The phase that moves it, from `multiplatform-plan.md` Section 12. */
  readonly phase: number;
  readonly cancellation: CancellationCase;
  /**
   * How far the Rust engine has this kind.
   *
   * - `ported`: it reaches an implementation AND a vector under
   *   `vectors/` pins that implementation against this TypeScript.
   * - `routed`: it reaches an implementation, and the only gate on it is
   *   `vectors/wire/requests.json` by way of
   *   `crates/epdf-engine/tests/wire_contract.rs`, which proves the arm
   *   exists and says nothing about what it does.
   * - `planned`: neither.
   *
   * The distinction is worth keeping because `routed` is a real but weak
   * guarantee: the kind answers, and whether it answers CORRECTLY rests on
   * unit tests rather than on the oracle.
   */
  readonly rustStatus: 'ported' | 'routed' | 'planned';
  /** Anything a reader of the matrix alone would otherwise get wrong. */
  readonly note?: string;
}

export const OPERATIONS: readonly OperationRow[] = [
  // --- G2. Page geometry. The first operation ported, and the one the rest
  // of the order was planned around.
  {
    kind: 'pages.geometry',
    group: 'G2',
    stage: 'A',
    phase: 0,
    cancellation: 'cooperative',
    rustStatus: 'ported',
    note: 'Answers PageGeometrySnapshot in page space: the reader answers the file\'s coordinates (runs with start, loose/tight boxes or quads, space/empty, rotation in degrees clockwise) and the host boundary converts them (pageGeometryOf). The C ABI export epdf_page_geometry answers the file\'s coordinates, which RustPageGeometryReader.ts reads as PageGeometrySnapshot<PdfCoordinates>. Gated by tooling/engine-conformance/vectors/pages.geometry through Host::call in crates/epdf-engine/tests/geometry_corpus.rs, and crates/epdf-wasm/tests/borrowed_page.rs.',
  },

  // --- G3. Session core and security. Native-only owner until the G3w
  // rehearsal; on web the TypeScript session stays authoritative until G10.
  {
    kind: 'open.fatMem',
    group: 'G3',
    stage: 'A',
    phase: 1,
    cancellation: 'queued-only',
    rustStatus: 'routed',
    note: 'No throwIfAborted reaches PdfDocumentOpener: once an open starts it runs to completion.',
  },
  {
    kind: 'open.layerMemBase',
    group: 'G3',
    stage: 'A',
    phase: 1,
    cancellation: 'queued-only',
    rustStatus: 'routed',
  },
  {
    kind: 'open.layerFileBase',
    group: 'G3',
    stage: 'A',
    phase: 1,
    cancellation: 'queued-only',
    rustStatus: 'routed',
  },
  {
    kind: 'document.checkPasswordPermissions',
    group: 'G3',
    stage: 'A',
    phase: 1,
    cancellation: 'queued-only',
    rustStatus: 'ported',
    note: 'Answers { security, protection } as WorkerHost.handleDocumentCheckPasswordPermissions does; on a locked session it loads the parked document with the password.',
  },
  {
    kind: 'document.probeSecurityFile',
    group: 'G3',
    stage: 'A',
    phase: 1,
    cancellation: 'queued-only',
    rustStatus: 'ported',
    note: 'Answers { security } as WorkerHost.handleDocumentProbeSecurityFile does. Needs no session.',
  },
  {
    kind: 'close',
    group: 'G3',
    stage: 'control',
    phase: 1,
    cancellation: 'control',
    rustStatus: 'routed',
    note: "Idempotent: rejects new work, invalidates handles, releases on the owner thread. Closing an engine closes its documents. Answered with the tag alone ({ tag: 'close' }, WorkerHost.handleClose), which Rust spells as a null result, whether or not a session was there.",
  },
  {
    kind: 'layer.close',
    group: 'G3',
    stage: 'control',
    phase: 1,
    cancellation: 'control',
    rustStatus: 'routed',
    note: "Idempotent: the caller may be closing a layer this worker never held. Answered with the tag alone ({ tag: 'close' }, WorkerHost.handleLayerClose), which Rust spells as a null result.",
  },
  {
    kind: 'abort',
    group: 'G3',
    stage: 'control',
    phase: 1,
    cancellation: 'control',
    rustStatus: 'routed',
    note: 'Handled in WorkerHost.receive before anything is queued (WorkerHost.ts:413), as pages.workingSet is (WorkerHost.ts:418). The other 87 of the 89 kinds reach a switch: 76 arms in run() (WorkerHost.ts:554) and 11 sliced reads in slicedWork() (WorkerHost.ts:1896).',
  },
  {
    kind: 'shutdown',
    group: 'G3',
    stage: 'control',
    phase: 1,
    cancellation: 'control',
    rustStatus: 'routed',
  },

  // New in the catch-up to upstream b456ee4fd; not ported to Rust yet.
  {
    kind: 'document.version',
    group: 'G3',
    stage: 'A',
    phase: 1,
    cancellation: 'queued-only',
    rustStatus: 'routed',
    note: 'SignatureReader.version (WorkerHost.ts:1018-1024): the hash and length of the base the session is on.',
  },
  {
    kind: 'objectNumbers.reserve',
    group: 'G3',
    stage: 'A',
    phase: 1,
    cancellation: 'queued-only',
    rustStatus: 'routed',
    note: "A session effect: moves the layer's last object number up. Only a session that hands out object numbers itself takes it. Every session is a layer: open.fatMem opens its bytes as a fresh layer over a private base, as WorkerHost.openBytesAsLayer does.",
  },
  {
    kind: 'pages.workingSet',
    group: 'G3',
    stage: 'A',
    phase: 1,
    cancellation: 'non-preemptible',
    rustStatus: 'routed',
    note: 'Never answers and never queues: WorkerHost.receive takes it on arrival, even mid-render (WorkerHost.ts:418), and jobId only names the message. So it is not queued-only, since there is no queue to drop it from; an abort finds no job and the bookkeeping always completes, which is non-preemptible with no wait. Not one of the four control frames (worker-kinds.ts), so it cannot carry the control case. Rust records the sets on the page residency of the host, through the page pool of an open session, and answers null, since Host::call answers every call; input it cannot read is dropped and answered null too, as upstream drops it unanswered. The residency keeps parsed pages within a byte budget across every document on the host (PageResidency.ts, 384 MB by default), closing the pages no view shows first; the host asks its idle timer between calls rather than on a timer, and loads a page at once rather than in slices.',
  },

  // --- G4. Render and fonts. Fonts are engine-level and move as one unit,
  // independently of which backend owns a document.
  {
    kind: 'pages.render',
    group: 'G4',
    stage: 'A',
    phase: 1,
    cancellation: 'cooperative',
    rustStatus: 'ported',
    note: 'Renders in slices (EPDF_RenderPageBitmapWithMatrix_Start/EPDF_RenderPage_Continue, 33ea7b4c9); Rust checks the cancel token between slices inside the synchronous call and does not yield to other jobs. Reads usage, the dpi viewport (checkRenderUsage with no rights, printScale, FPDF_PRINTING) and includeFormFields (resolvePageLayers, EPDF_RENDER_WIDGETS). Takes the target in page space and answers {page, area, raster}, the pixels payload 0; page and area convert at the host boundary (page_space::boundary, pdfRenderTargetOf).',
  },
  {
    kind: 'fonts.register',
    group: 'G4',
    stage: 'A',
    phase: 1,
    cancellation: 'queued-only',
    rustStatus: 'routed',
    note: 'Engine-level, not per document. One Rust FontRegistry per combined worker owns the key-to-ID map and the retained handles.',
  },
  {
    kind: 'fonts.addFallback',
    group: 'G4',
    stage: 'A',
    phase: 1,
    cancellation: 'queued-only',
    rustStatus: 'routed',
  },
  {
    kind: 'fonts.clearFallbacks',
    group: 'G4',
    stage: 'A',
    phase: 1,
    cancellation: 'queued-only',
    rustStatus: 'routed',
  },
  {
    kind: 'fonts.clear',
    group: 'G4',
    stage: 'A',
    phase: 1,
    cancellation: 'queued-only',
    rustStatus: 'routed',
  },

  // New in the catch-up to upstream b456ee4fd; not ported to Rust yet.
  {
    kind: 'fonts.authorizeEditing',
    group: 'G4',
    stage: 'A',
    phase: 1,
    cancellation: 'queued-only',
    rustStatus: 'routed',
  },
  {
    kind: 'document.setFontSettings',
    group: 'G4',
    stage: 'A',
    phase: 1,
    cancellation: 'queued-only',
    rustStatus: 'routed',
    note: 'Per-document session state on the handle (EPDFDoc_SetFontEmbeddingPolicy, EPDFDoc_SetTypographicFeatures, WorkerHost.ts:2285-2290), never written to the file: a session effect. Filed with the fonts it configures.',
  },

  // --- G5. Text and search.
  {
    kind: 'pages.text',
    group: 'G5',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'ported',
    note: 'Already coarse (PageTextReader.ts:93). Gate on correctness, not on the boundary. Answers PageTextSnapshot, which holds no places, so the boundary passes it through. Gated by vectors/pages.text in crates/epdf-engine/tests/text_corpus.rs.',
  },
  {
    kind: 'search.query',
    group: 'G5',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'ported',
    note: 'Takes the flattened SearchScanRequest (query fields at the top level, snippets, from as a PageRef, cursor, limit) and answers SearchSlice: matches as page ranges (start, count) with a snippet of before, match and after, pagesSearched and pageCount. The cursor key is searchQueryKey, mode spelled snippets or matches. The reader answers segments in the file\'s coordinates and the host boundary converts them (pageSearchSliceOf). Gated by vectors/search through crates/epdf-engine/tests/search_dialect.rs, search_slice_protocol.rs, search_reader.rs and search_ignore_whitespace.rs.',
  },

  // --- G6. Document structure reads.
  {
    kind: 'metadata.read',
    group: 'G6',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'ported',
    note: 'Gated by vectors/metadata/read.json through crates/epdf-engine/tests/metadata_corpus.rs. The standard fields only: createdAt and modifiedAt keep the offset the file wrote (pdfDateToIso); the custom keys are metadata.readCustom.',
  },
  {
    kind: 'metadata.readCustom',
    group: 'G6',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'ported',
    note: 'Gated with metadata.read: the generator folds metadata.custom.get() into the read it records, and metadata_corpus.rs does the same.',
  },
  {
    kind: 'pages.list',
    group: 'G6',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'ported',
    note: 'Page identity must agree between lazy per-page lookup and full enumeration (DocumentSession.ts:41-44). Rust reads the boxes by pageBoxesOf and /Rotate by pageRotationOf, and answers them in page space at the host boundary (pageListOf).',
  },
  {
    kind: 'actions.read',
    group: 'G6',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'ported',
    note: 'Rust reads openDestination and answers every destination in page space at the host boundary (mapDocumentActions), triggers first.',
  },
  {
    kind: 'outline.read',
    group: 'G6',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'routed',
    note: "New in 67ccb76dc. Answers the DocumentOutline, every item depth first, each destination of its target and its action in page space at the host boundary (pageOutlineOf); no page is loaded. The outline's writes are the document.apply ops outline.create, outline.update, outline.move and outline.delete. No vector covers it: crates/epdf-engine/tests/outline.rs ports runOutlineConformance.",
  },

  // New in the catch-up to upstream b456ee4fd; not ported to Rust yet.
  {
    kind: 'measure.viewports',
    group: 'G6',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'routed',
    note: "Answers {page, viewports}: every /VP entry in drawing order, its name and measure null when the file has none, a scale's subtype rectilinear, geospatial or unknown (readMeasure), each bbox and scale origin in page space at the host boundary (pageViewportsOf). No vector covers it: crates/epdf-engine/tests/measure.rs ports measurements.test.ts and runMeasurementConformance.",
  },

  // --- G7. Annotation reads. Rust never creates or retains the form model;
  // TypeScript lends the pointer.
  // Not a WorkerRequest kind after the catch-up to upstream b456ee4fd: replaced by annotations.list (a10ed2659 and the Engine API v3 rounds).
  // {
  //   kind: 'annotations.listRawAll',
  //   group: 'G7',
  //   stage: 'A',
  //   phase: 2,
  //   cancellation: 'cooperative',
  //   rustStatus: 'ported',
  // },
  // Not a WorkerRequest kind after the catch-up to upstream b456ee4fd: replaced by annotations.list.
  // {
  //   kind: 'annotations.listRawPage',
  //   group: 'G7',
  //   stage: 'A',
  //   phase: 2,
  //   cancellation: 'cooperative',
  //   rustStatus: 'ported',
  // },
  // Not a WorkerRequest kind after the catch-up to upstream b456ee4fd: replaced by annotations.list.
  // {
  //   kind: 'annotations.listFullPage',
  //   group: 'G7',
  //   stage: 'A',
  //   phase: 2,
  //   cancellation: 'cooperative',
  //   rustStatus: 'ported',
  //   note: 'Reaches the cached form model through joinWidgetField.ts:23, so it carries a repeated-read benchmark against that cache.',
  // },
  {
    kind: 'annotations.renderAppearances',
    group: 'G7',
    stage: 'A',
    phase: 2,
    cancellation: 'non-preemptible',
    rustStatus: 'ported',
    note: "One family per request (annotations, or widgets for page.forms). Every mode and every state an annotation stores, each labelled (17350fb4c); an annotation with no normal appearance is drawn in memory into EPDFAnnot_GetDrawingRect's box, never written (25dd21f38). A box kind drawn turned renders upright into its box, the turn passed to EPDF_RenderAnnotBitmapUnrotated (runtime 5e9241945). Refs by baseIndex or object number; rects converted to page space at the host boundary (pageAppearancesOf). Gated by vectors/annotation/appearances.json through crates/epdf-engine/tests/annotation_appearance.rs.",
  },

  // New in the catch-up to upstream b456ee4fd; not ported to Rust yet.
  {
    kind: 'annotations.list',
    group: 'G7',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'ported',
    note: "Replaces annotations.listRawAll, listRawPage and listFullPage. Unlike listFullPage it builds no widget rows (RawAnnotationReader.list with the 'annotations' family), so it never reaches the cached form model; the widget-to-field join and its repeated-read benchmark moved to forms.list. Answers AnnotationList in upstream's record shape, read in the file's coordinates and converted at the host boundary by field space (pageAnnotationOf); a promoted inline annotation reads by its birth name (EPDFLayer_GetBirthName). Gated by vectors/annotation/read.json through crates/epdf-engine/tests/annotation_reader.rs.",
  },
  {
    kind: 'annotations.readAppearance',
    group: 'G7',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'ported',
    note: "An annotation's appearance as a one-page PDF (bytes): EPDFAnnot_ExportAppearance with the turn the annotation reads taken out (exportUnturnedAppearance). Only a kind with an appearance resource (the stamp) has one.",
  },

  // --- G8. Files, forms reads, piece info.
  {
    kind: 'attachments.list',
    group: 'G8',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'ported',
    note: 'Answers { attachments } (AttachmentList, a9434c0e2): each entry carries its ref, createdAt and modifiedAt as ISO dates, and null for every field the PDF has no value for. Gated by vectors/files/attachments.json through Host::call.',
  },
  {
    kind: 'attachments.readFile',
    group: 'G8',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'ported',
  },
  {
    kind: 'annotations.readFile',
    group: 'G8',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'ported',
    note: 'Byte equality with the TypeScript path, not a semantic comparison. Its ref resolves as upstream resolveAnnotIndexRaw does, a baseIndex on a promoted page through the layer\'s births.',
  },
  {
    kind: 'pieceInfo.read',
    group: 'G8',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'ported',
  },
  {
    kind: 'pieceInfo.applications',
    group: 'G8',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'ported',
  },
  {
    kind: 'forms.list',
    group: 'G8',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'ported',
    note: "Filed by behaviour: a read despite the forms. prefix. Answers FormSnapshot: the fields in upstream's shape (ref by objectNumber, the /Ff bits as readOnly, required and noExport, groupId and attribution from the field's /EMBD_Metadata) and every widget row the pages' /Annots hold, each joined to its field (widgetRows.ts, joinWidgetField.ts). The reader answers the file's coordinates and the host boundary converts them (pageFormSnapshotOf). Upstream joins through its cached form model (acquireFormModel) and carries the repeated-read benchmark annotations.listFullPage carried; the Rust host has no form model cache and builds one model per read. Gated by vectors/forms/snapshot.json through crates/epdf-engine/tests/form_reader.rs.",
  },
  {
    kind: 'forms.export',
    group: 'G8',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'routed',
    note: 'The fields a selection takes, each whole with its widgets, as a form bundle in page space (FormExporter, 7f0b73a3c, which removed FDF and XFDF); a snapshot. No vector: gated by crates/epdf-engine/tests/form_transfer.rs and the bundle unit tests (features/transfer).',
  },

  // New in the catch-up to upstream b456ee4fd.
  {
    kind: 'annotations.export',
    group: 'G8',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'routed',
    note: "Annotations and their resources as one bundle, in page space (AnnotationExporter); a snapshot, like forms.export. Each resource once, by its SHA-256. No vector: gated by crates/epdf-engine/tests/annotation_transfer.rs, whose export of the two Acrobat stamp fixtures hashes to the TypeScript engine's file.",
  },

  // --- G9. Page extract. Rust owns a scratch document end to end and closes
  // it before returning; it never takes ownership of the session's.
  {
    kind: 'pages.extract',
    group: 'G9',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'ported',
    note: 'Creates and closes its own scratch document (PagesExtractor.ts:73-112), a named exception to Stage A "retains nothing, closes nothing": FPDF_CreateNewDocument with FPDF_ImportPagesByIndex for a plain copy, EPDFDoc_ExportPagesFlattened with flatten (what it paints arrives already checked against the caller\'s rights, resolvePageExtract), either keeping the title. Takes PageRefs in the caller\'s order. Gated by vectors/pages/extract.json, which records the sources sent, through crates/epdf-engine/tests/page_extract_corpus.rs; page_extract.rs ports the extract conformance suite.',
  },

  // New in the catch-up to upstream b456ee4fd; not ported to Rust yet.
  {
    kind: 'annotations.exportAppearance',
    group: 'G9',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'ported',
    note: 'Flattens chosen annotation appearances, named by objectNumber or baseIndex refs, into a new single-page PDF; a snapshot, like pages.extract. Builds a scratch document (EPDFPage_ExportAnnotationsAsDocument) and closes it before returning. Gated by vectors/pages/mutate.json, replayed by crates/epdf-engine/tests/flatten_corpus.rs; annotation_flatten.rs ports the export conformance suite.',
  },

  // --- G10. Session authority and save. Stage B.
  {
    kind: 'document.saveBuffer',
    group: 'G10',
    stage: 'B',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
  },
  {
    kind: 'document.saveFile',
    group: 'G10',
    stage: 'B',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
  },
  {
    kind: 'document.saveLayerBuffer',
    group: 'G10',
    stage: 'B',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
  },

  // --- G11. Annotation writes.
  {
    kind: 'annotations.create',
    group: 'G11',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
    note: "A one-op change (applyOne) and an op of document.apply: the v3 draft in page space, converted at the boundary (pdfAnnotationDraftOf), checked field by field against its kind (prepareCreate), and made on its page's /Annots without loading the page (EPDFPage_CreateAnnotRaw). Stamped with a UUIDv7 /NM when it names none; a stamp places a drawing found or added once per artwork (drawingFor); its reverse deletes it while it reads as left. A dimension's caption is stated whole and its label worked out from its points and scale (resolveMeasurementDraft), then written with its scale and leader (writeMeasurementFields). Gated by vectors/annotation/create.json through crates/epdf-engine/tests/annotation_create.rs.",
  },
  {
    kind: 'annotations.update',
    group: 'G11',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
    note: "A one-op change (applyOne) and an op of document.apply: the v3 patch in page space, converted at the boundary (pdfAnnotationPatchOf), resolved against the annotation's read (pdfResolveAnnotationPatch: a drawn kind's rect becomes the shape that puts the drawing there, what it sends back as it reads is dropped, each field checked against its kind's update schema, what follows stated), then written in place by its kind's writer. The appearance verdict classifies the resolved patch (appearanceImpactOf); /RD is the appearance's. A widget is refused for doc.forms.updateWidget. Its reverse puts back by value the fields it changed that still show it, until 8.30 brings the dictionary captures. A dimension's caption merges with the one it has, a rigid move carries a manual caption centre, a foreign scale's marker sent back is kept, and the label follows its points and scale (measurementFollows). Gated by vectors/annotation/update.json and write.json through crates/epdf-engine/tests/annotation_update.rs and annotation_write.rs.",
  },
  {
    kind: 'annotations.delete',
    group: 'G11',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
    note: "A one-op change (applyOne) and an op of document.apply: the annotation goes with its thread and popups in one change (deletedWith), each checked against the authority before the first write, the page's inline annotations promoted first so a baseIndex keeps naming them. Answers only meta, the annotation first in meta.changed; the annotations.deleted event lists them all. A widget is refused for the form's ops. Its reverse brings everything back from the fork's capture at the same numbers and positions (annotation.restore), and that restore's reverse deletes it again, so an undone create can be redone. Only a layer can capture, and open.fatMem opens one, so a change that deletes beside other ops is taken back whole when a later op refuses. Gated by vectors/annotation/delete.json through crates/epdf-engine/tests/annotation_delete.rs.",
  },
  // Not a WorkerRequest kind after the catch-up to upstream b456ee4fd: replaced by annotations.reorder (and the reorder op of document.apply).
  // {
  //   kind: 'annotations.move',
  //   group: 'G11',
  //   stage: 'C',
  //   phase: 4,
  //   cancellation: 'after-first-write',
  //   rustStatus: 'ported',
  // },

  // New in the catch-up to upstream b456ee4fd.
  {
    kind: 'annotations.import',
    group: 'G11',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'routed',
    note: "A one-op change (the annotations.import op, which document.apply refuses and document.applyChanges runs): planned before the first write (planAnnotationImport), then made as one batch (AnnotationBatchApplier). Its reverse removes what it made, thread by thread; a restoring import's steps take its rights. No vector: gated by crates/epdf-engine/tests/annotation_transfer.rs.",
  },
  {
    kind: 'annotations.reorder',
    group: 'G11',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
    note: "Replaces annotations.move (1233d1349). A one-op change and an op of document.apply: the rows go together, in the order given, beside a neighbour of their own family or at the start or the end (ListPosition), as one block move in /Annots; widgets have their own order. Nothing is renamed (bc0057f3a). Answers the page's whole new order. Its reverse puts each row back beside its old neighbours while it is still where the reorder left it. Gated by vectors/annotation/move.json through crates/epdf-engine/tests/annotation_move.rs.",
  },
  {
    kind: 'document.apply',
    group: 'G11',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'routed',
    note: "One change as one transaction: annotation, form, page, attachment, metadata and outline ops (Change.ts). Rust runs the metadata, attachment, annotation, page and form value ops, the form's design ops: fields, widgets and the calculation order, and the outline ops (features/changes, tests/document_apply.rs, tests/files_write.rs, tests/page_structure.rs, tests/form_apply.rs, tests/form_widgets.rs, tests/outline.rs) and answers NotImplemented naming the owner sub-task for every other op type; an op's bytes ride as a request payload its resources name, { payload: n } (a pages.insert source too, or the list of bytes a recorded request holds); a refused change is taken back through its ops' reverse steps. Carries the page and attachment writes that were their own kinds, and their invariants. Page identity survives a pages.reorder op within the same live document: FPDF_MovePages rewrites the page tree and leaves /Annots alone, so every page and annotation ref, baseIndex included, names the same thing after it (page_structure.rs). A pages.insert op's source is retained on one owner, the session, until it closes (PagesInserter.ts:88, session.retainUntilClose, reached through changes/internal/pageChanges.ts:53), so a source document handle never crosses owners. Each page op's position is a neighbour or an end (ListPosition), resolved when the op runs; its reverse is pages.rotate, pages.reorder, pages.restore (the fork's page capture, names included) or pages.remove (guarded by a digest of the page). Gated for the page ops by vectors/pages/mutate.json through crates/epdf-engine/tests/pages_mutate.rs.",
  },
  {
    kind: 'document.applyChanges',
    group: 'G11',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'routed',
    note: "A server request's changes as one job, each in its own layer transaction. Rust runs the metadata ops, as document.apply does.",
  },

  // --- G12. Forms writes.
  {
    kind: 'forms.setValue',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
    note: 'A one-op change (forms.setValue, features/changes/forms.rs): the value as a read returns it ({ value }, { checked }, { selectedValues }), written as the field\'s rules leave it (fieldAfterValue) through the fork\'s value writers, answered { field, widgets, meta } with FormMutationMeta and a forms.valueSet event; its reverse puts back the field\'s and widgets\' dictionaries from the fork\'s captures, else the value. Gated by vectors/forms/write.json (tests/form_write.rs) and vectors/forms/apply.json (tests/form_apply.rs).',
  },
  {
    kind: 'forms.reset',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
    note: 'A one-op change (forms.reset) over refs, or every field without them: each field back to its /DV through the value writers (fieldAfterReset), answered { fields, widgets, meta }. This host answers the forms.valueSet facts (formResetFacts) as an events array beside the result. Gated by vectors/forms/write.json and vectors/forms/apply.json.',
  },
  // Not a WorkerRequest kind after the catch-up to upstream b456ee4fd: removed upstream in 98e5c270c ("applyEffects removed").
  // {
  //   kind: 'forms.applyEffects',
  //   group: 'G12',
  //   stage: 'C',
  //   phase: 4,
  //   cancellation: 'after-first-write',
  //   rustStatus: 'ported',
  // },
  {
    kind: 'forms.import',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'routed',
    note: "A form bundle's design as a one-op change (the forms.import op; FDF and XFDF import went in 7f0b73a3c): planned before the first write (planFormImport), each field created with its widgets, flags and value, attribution restored or stamped. Its reverse deletes each field it made. No vector: gated by crates/epdf-engine/tests/form_transfer.rs.",
  },
  {
    kind: 'forms.repair',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
    note: 'Not a change: a final one that ends undo. Answers the report with needsAppearancesCleared (7bccc4f35) and an emptyMeta that cannot be undone, and publishes forms.repaired. Gated by vectors/forms/write.json documentCases.',
  },
  {
    kind: 'forms.createField',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
    note: 'A one-op change (forms.create, features/changes/fields.rs): the v3 draft in page space, each widget placement converted on its page (pdfFormFieldDraftOf), checked before the first write (placedWidgetOf, onStateOf, defaultEntryOfDraft, maxLength), then the field, its settings and /DV, who made it and its group, its widgets born through the annotation plane and adopted, and its place in the calculation order when it has a calculate script, all in one layer transaction. Answered { field, widgets, meta } with a forms.created event; its reverse deletes the field while nobody changed it since (field.remove). Gated by vectors/forms/field.json through crates/epdf-engine/tests/form_field.rs, and tests/form_widgets.rs.',
  },
  {
    kind: 'forms.updateField',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
    note: "A one-op change (forms.update): the patch checked against the field's family (assertPatchFitsFamily), its default against the field as the patch leaves it (formFieldAfter), then each setting written, /DV through EPDFForm_SetFieldDefaultValue, new options with the selection that goes with them. Its reverse puts the field's and widgets' dictionaries back from the fork's captures, else each property still holding what the update set (revertProperties). Gated by vectors/forms/field.json through crates/epdf-engine/tests/form_field.rs.",
  },
  {
    kind: 'forms.deleteField',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
    note: "A one-op change (forms.delete): the field leaves the calculation order, the tree, and its widgets their pages, in one write; a merged field/widget goes in the same cascade (60602f579). Answers only meta (the field in changedFields, its widgets in changedWidgets) and a forms.deleted event; its reverse brings the field back from the fork's capture (EPDFForm_ExportFieldRawToOwnedBuffer, field.restore), then into the calculation order. Gated by vectors/forms/field.json through crates/epdf-engine/tests/form_field.rs.",
  },
  // Not a WorkerRequest kind after the catch-up to upstream b456ee4fd: replaced by forms.addWidget.
  // {
  //   kind: 'forms.attachWidget',
  //   group: 'G12',
  //   stage: 'C',
  //   phase: 4,
  //   cancellation: 'after-first-write',
  //   rustStatus: 'ported',
  // },
  {
    kind: 'forms.detachWidget',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
    note: "A one-op change (forms.removeWidget): the widget leaves its field and stays on its page; a merged field/widget is refused. Answers { field, widgets, meta }, the widget's row in no field; its reverse puts the field's and the widget's dictionaries back from the fork's captures. Gated by vectors/forms/field.json through crates/epdf-engine/tests/form_field.rs.",
  },

  // New in the catch-up to upstream b456ee4fd.
  {
    kind: 'forms.addWidget',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
    note: "A one-op change (forms.addWidget, replacing forms.attachWidget, d7bd2d277): the placement in page space converted on its page (pdfWidgetPlacementOf), the widget born, styled and added to the field in one layer transaction; a merged field splits, its widget half at splitObjectNumber. Answers { field, widgets, meta }; its reverse puts back the field's and the pages' dictionaries. Gated by crates/epdf-engine/tests/form_widgets.rs.",
  },
  {
    kind: 'forms.updateWidget',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
    note: "A one-op change (forms.updateWidget): the widget patch written as an annotation update writes it (place, turn, /MK, /BS, /DA, /Q, caption or check style, actions), a caption only on a push button's and a check style only on a toggle's (fitWidgetToFamily), drawn again as its field's family draws it. Its reverse puts back by value what still shows the update, until 8.30 brings the dictionary captures. Gated by crates/epdf-engine/tests/form_widgets.rs.",
  },
  {
    kind: 'forms.deleteWidget',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
    note: "A one-op change (forms.deleteWidget): the widget leaves its field (its dictionaries captured) and its page (the fork's annotation capture); a merged field/widget is refused for forms.delete. Its reverse brings it back at its place and into its field (widget.restore). Gated by crates/epdf-engine/tests/form_widgets.rs.",
  },
  {
    kind: 'forms.reorderWidgets',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
    note: "A one-op change (forms.reorderWidgets): the page's widgets in their own stacking order, apart from its annotations. Its reverse puts them back beside their old neighbours (annotation.reorder, family widgets). Gated by crates/epdf-engine/tests/form_widgets.rs.",
  },
  {
    kind: 'forms.reorderCalculations',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
    note: 'A one-op change (forms.reorderCalculations, 98718b780): the fields go together to a position in /AcroForm /CO. Its reverse takes back what it did where nobody changed it since (orderBack). Gated by crates/epdf-engine/tests/form_widgets.rs.',
  },
  {
    kind: 'forms.importValues',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'routed',
    note: "A form bundle's values as a one-op change (the forms.importValues op): each field filled by full name (planFormValuesImport); its reverse puts back each value and who filled it. No vector: gated by crates/epdf-engine/tests/form_transfer.rs.",
  },
  {
    kind: 'forms.setSignatureAppearance',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'routed',
    note: "A one-op change (the forms.setSignatureAppearance op): a one-page PDF drawn into every widget of an unsigned signature field, checked as signing the field (authorizeFieldWrite sign); its reverse puts the widgets' appearances back. Gated by crates/epdf-engine/tests/signature_sign.rs.",
  },

  // --- G13. Page assembly and redaction.
  // Not a WorkerRequest kind after the catch-up to upstream b456ee4fd: now a pages.rotate op inside document.apply (Change.ts), not its own kind.
  // {
  //   kind: 'pages.rotate',
  //   group: 'G13',
  //   stage: 'C',
  //   phase: 4,
  //   cancellation: 'after-first-write',
  //   rustStatus: 'ported',
  // },
  // Not a WorkerRequest kind after the catch-up to upstream b456ee4fd: now a pages.reorder op inside document.apply.
  // {
  //   kind: 'pages.move',
  //   group: 'G13',
  //   stage: 'C',
  //   phase: 4,
  //   cancellation: 'after-first-write',
  //   rustStatus: 'ported',
  //   note: 'Page identity must survive reorder within the same live document.',
  // },
  // Not a WorkerRequest kind after the catch-up to upstream b456ee4fd: now a pages.delete op inside document.apply.
  // {
  //   kind: 'pages.delete',
  //   group: 'G13',
  //   stage: 'C',
  //   phase: 4,
  //   cancellation: 'after-first-write',
  //   rustStatus: 'ported',
  // },
  // Not a WorkerRequest kind after the catch-up to upstream b456ee4fd: now a pages.insertBlank op inside document.apply.
  // {
  //   kind: 'pages.insertBlank',
  //   group: 'G13',
  //   stage: 'C',
  //   phase: 4,
  //   cancellation: 'after-first-write',
  //   rustStatus: 'routed',
  // },
  // Not a WorkerRequest kind after the catch-up to upstream b456ee4fd: now a pages.insert op inside document.apply.
  // {
  //   kind: 'pages.insert',
  //   group: 'G13',
  //   stage: 'C',
  //   phase: 4,
  //   cancellation: 'after-first-write',
  //   rustStatus: 'routed',
  //   note: 'Source retention lives on one owner (PagesInserter.ts:77, session.retainUntilClose). A source document handle never crosses owners.',
  // },
  {
    kind: 'pages.flatten',
    group: 'G13',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
    note: "A content write and a final change: promotes each page, answers per-page applied or unchanged and MutationMeta, and its event carries the write's opId. A failing page or a cancel refuses the call and its layer transaction takes back the pages already flattened, with no sequence bump. Gated by vectors/pages/mutate.json, replayed by crates/epdf-engine/tests/flatten_corpus.rs.",
  },
  {
    kind: 'redaction.apply',
    group: 'G13',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
    note: "A content write and a final change, answered like pages.flatten: takes a {pages} or {annotations} scope, promotes each page, applies a page's refs in one native call, and answers per-page applied or unchanged with MutationMeta. A failing page or a cancel refuses the call and its layer transaction takes back the pages already promoted or applied, with no sequence bump. A no-op answer keeps mutationSequence (kept divergence, 87b61b533). Gated by vectors/redaction/apply.json, replayed by crates/epdf-engine/tests/redaction_apply.rs.",
  },

  // New in the catch-up to upstream b456ee4fd; not ported to Rust yet.
  {
    kind: 'annotations.flatten',
    group: 'G13',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
    note: "A content write and a final change: pages.flatten for a chosen set of one page's annotations, named by objectNumber or baseIndex refs. Promotes the page's inline annotations first, answers per-ref applied or unchanged and MutationMeta, and its event carries the write's opId. Gated by vectors/pages/mutate.json, replayed by crates/epdf-engine/tests/flatten_corpus.rs; annotation_flatten.rs ports the conformance suite.",
  },
  {
    kind: 'pages.setName',
    group: 'G13',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
    note: "A write in its own transaction answering PageNameResult: the new layout, with namedPages, and MutationMeta naming no page; its pages.named event names the key and the page. Gated by crates/epdf-engine/tests/named_pages.rs, which ports the named pages conformance suite.",
  },
  {
    kind: 'pages.removeName',
    group: 'G13',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
    note: "A write in its own transaction answering PageNameResult: the new layout, with namedPages, and MutationMeta naming no page; its pages.named event names the key and the page (null once removed). Gated by crates/epdf-engine/tests/named_pages.rs, which ports the named pages conformance suite.",
  },

  // --- G14. Remaining mutators.
  {
    kind: 'metadata.update',
    group: 'G14',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
    note: 'MetadataPatch is the Phase 0 DTO codegen proof: three-state Keep | Clear | Set(T). Gated by vectors/metadata/update.json through crates/epdf-engine/tests/metadata_write.rs, which replays the recorded request. Runs as a one-op change, so its opId undoes it.',
  },
  {
    kind: 'metadata.updateCustom',
    group: 'G14',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
    note: 'Gated with metadata.update by vectors/metadata/update.json. A standard key, a leading slash or a key outside printable ASCII is InvalidArg before anything is written; answers the custom keys alone.',
  },
  {
    kind: 'pieceInfo.update',
    group: 'G14',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
    note: 'Answers PieceInfoUpdateResult, { pieceInfo, meta }, as WorkerHost.handlePieceInfoUpdate does, and publishes no event.',
  },
  // Not a WorkerRequest kind after the catch-up to upstream b456ee4fd: replaced by pieceInfo.delete.
  // {
  //   kind: 'pieceInfo.clear',
  //   group: 'G14',
  //   stage: 'C',
  //   phase: 4,
  //   cancellation: 'after-first-write',
  //   rustStatus: 'ported',
  // },
  // Not a WorkerRequest kind after the catch-up to upstream b456ee4fd: now an attachments.create op inside document.apply.
  // {
  //   kind: 'attachments.create',
  //   group: 'G14',
  //   stage: 'C',
  //   phase: 4,
  //   cancellation: 'after-first-write',
  //   rustStatus: 'ported',
  // },
  // Not a WorkerRequest kind after the catch-up to upstream b456ee4fd: now an attachments.delete op inside document.apply.
  // {
  //   kind: 'attachments.delete',
  //   group: 'G14',
  //   stage: 'C',
  //   phase: 4,
  //   cancellation: 'after-first-write',
  //   rustStatus: 'ported',
  // },

  // New in the catch-up to upstream b456ee4fd; not ported to Rust yet.
  {
    kind: 'pieceInfo.delete',
    group: 'G14',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
    note: 'Replaces pieceInfo.clear. Answers { meta } and publishes no event.',
  },
  {
    kind: 'measure.setScale',
    group: 'G14',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'routed',
    note: "Takes a rectilinear PdfMeasure with its origin in page space, converted at the boundary (pdfMeasureOf), or null to remove EmbedPDF's viewports; calibrates over the page's visible box (readBoxes crop) in one transaction, answering PageScaleResult with the write's opId in meta. A scale write with no measure is refused InvalidArg (d8027c034, a kept divergence for the user to confirm at Task 14). No vector covers it: crates/epdf-engine/tests/measure.rs.",
  },

  // --- G15. Server and file surfaces. Browser and local workers keep
  // returning NotImplemented for the encoded kinds (worker-protocol.ts:446).
  {
    kind: 'pages.renderEncoded',
    group: 'G15',
    stage: 'C',
    phase: 5,
    cancellation: 'non-preemptible',
    rustStatus: 'routed',
    note: 'Sole owner of the wire kind; the raster step comes from G4 internal entry point, sliced and with pages.render options (usage, dpi viewport, includeFormFields). Encoding stays host-injected and is not interruptible. Rust takes the target in page space, as pages.render does, and answers {image: {contentType, width, height, bytes}}, the bytes payload 0.',
  },
  {
    kind: 'annotations.renderAppearancesEncoded',
    group: 'G15',
    stage: 'C',
    phase: 5,
    cancellation: 'non-preemptible',
    rustStatus: 'routed',
  },
  {
    kind: 'document.renderPageFile',
    group: 'G15',
    stage: 'C',
    phase: 5,
    cancellation: 'non-preemptible',
    rustStatus: 'routed',
    note: 'The target is page space and converts on the page the render opens (renderOptionsInFileSpace), since no session carries the document to the host boundary. Rendered as pages.render renders (sliced, usage, dpi viewport, includeFormFields); answers {page, pageCount, raster}, the pixels payload 0.',
  },
  {
    kind: 'document.renderPageFileEncoded',
    group: 'G15',
    stage: 'C',
    phase: 5,
    cancellation: 'non-preemptible',
    rustStatus: 'routed',
    note: 'The target is page space, as document.renderPageFile takes it. Answers {page, pageCount, image}, the bytes payload 0.',
  },

  // New in the catch-up to upstream b456ee4fd; not ported to Rust yet.
  {
    kind: 'signatures.list',
    group: 'G15',
    stage: 'A',
    phase: 5,
    cancellation: 'queued-only',
    rustStatus: 'routed',
    note: "No port group in multiplatform-port-order.md covers signing yet; parked in G15 until the port order assigns one. WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them.",
  },
  {
    kind: 'signatures.analyze',
    group: 'G15',
    stage: 'A',
    phase: 5,
    cancellation: 'queued-only',
    rustStatus: 'routed',
    note: "WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them.",
  },
  {
    kind: 'signatures.contents',
    group: 'G15',
    stage: 'A',
    phase: 5,
    cancellation: 'queued-only',
    rustStatus: 'routed',
    note: "WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them.",
  },
  {
    kind: 'signatures.digest',
    group: 'G15',
    stage: 'A',
    phase: 5,
    cancellation: 'queued-only',
    rustStatus: 'routed',
    note: "WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them.",
  },
  {
    kind: 'signatures.revisionBytes',
    group: 'G15',
    stage: 'A',
    phase: 5,
    cancellation: 'queued-only',
    rustStatus: 'routed',
    note: "WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them.",
  },
  {
    kind: 'signatures.prepare',
    group: 'G15',
    stage: 'B',
    phase: 5,
    cancellation: 'after-first-write',
    rustStatus: 'routed',
    note: "A snapshot, like the saves. Never touches the live document: the candidate is parked on the session until complete or cancel. Filed after-first-write by its stage and effect, like the saves, though WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them today.",
  },
  {
    kind: 'signatures.complete',
    group: 'G15',
    stage: 'C',
    phase: 5,
    cancellation: 'after-first-write',
    rustStatus: 'routed',
    note: "A content write. Installs the sealed bytes as the session document (DocumentSession.install), which advances the edit sequence itself. Filed after-first-write by its stage and effect, like the other content writes, though WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them today.",
  },
  {
    kind: 'signatures.cancel',
    group: 'G15',
    stage: 'A',
    phase: 5,
    cancellation: 'queued-only',
    rustStatus: 'routed',
    note: "A session effect, renamed from signatures.abort (7bccc4f35): discards the parked candidate and answers { result: { status } }, cancelled, already-completed or unknown. Gated by crates/epdf-engine/tests/signature_sign.rs. WorkerHost.ts:599-628 calls the signature handlers without the job's AbortSignal, so no throwIfAborted reaches them.",
  },
  {
    kind: 'signatures.finalizeCandidate',
    group: 'G15',
    stage: 'C',
    phase: 5,
    cancellation: 'queued-only',
    rustStatus: 'routed',
    note: "Session-less, like document.renderPageFile: patches the caller's candidate file in place and reads it back through a document that is never a session. Kept at stage C though its effect is read: no session's document changes, but it opens, writes and closes a document of its own. queued-only, not after-first-write: the handler gets no AbortSignal (WorkerHost.ts:626-627, 1071-1083), so once it starts it runs to the end.",
  },
];
