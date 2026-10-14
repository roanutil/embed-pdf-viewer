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
    note: 'Gated by tooling/engine-conformance/vectors/pages.geometry and crates/epdf-engine/tests/geometry_corpus.rs.',
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
  },
  {
    kind: 'document.probeSecurityFile',
    group: 'G3',
    stage: 'A',
    phase: 1,
    cancellation: 'queued-only',
    rustStatus: 'ported',
  },
  {
    kind: 'close',
    group: 'G3',
    stage: 'control',
    phase: 1,
    cancellation: 'control',
    rustStatus: 'routed',
    note: 'Idempotent: rejects new work, invalidates handles, releases on the owner thread. Closing an engine closes its documents.',
  },
  {
    kind: 'layer.close',
    group: 'G3',
    stage: 'control',
    phase: 1,
    cancellation: 'control',
    rustStatus: 'routed',
  },
  {
    kind: 'abort',
    group: 'G3',
    stage: 'control',
    phase: 1,
    cancellation: 'control',
    rustStatus: 'routed',
    note: 'Handled before the WorkerHost switch (WorkerHost.ts:282), which is why that switch has 83 arms and the protocol has 84 kinds.',
  },
  {
    kind: 'shutdown',
    group: 'G3',
    stage: 'control',
    phase: 1,
    cancellation: 'control',
    rustStatus: 'routed',
  },

  // --- G4. Render and fonts. Fonts are engine-level and move as one unit,
  // independently of which backend owns a document.
  {
    kind: 'pages.render',
    group: 'G4',
    stage: 'A',
    phase: 1,
    cancellation: 'non-preemptible',
    rustStatus: 'ported',
    note: 'Cooperative up to FPDF_RenderPageBitmapWithMatrix (PageRenderReader.ts:51), which cannot be interrupted.',
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
    note: 'Per-document session state on the handle (EPDFDoc_SetFontEmbeddingPolicy, EPDFDoc_SetTypographicFeatures), never written to the file. Filed with the fonts it configures.',
  },

  // --- G5. Text and search.
  {
    kind: 'pages.text',
    group: 'G5',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'ported',
    note: 'Already coarse (PageTextReader.ts:93). Gate on correctness, not on the boundary.',
  },
  {
    kind: 'search.query',
    group: 'G5',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'ported',
    note: 'Entry gate: a dialect vector file for regex.ts and fold.ts semantics before any porting starts.',
  },

  // --- G6. Document structure reads.
  {
    kind: 'metadata.read',
    group: 'G6',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'ported',
  },
  {
    kind: 'pages.list',
    group: 'G6',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'ported',
    note: 'Page identity must agree between lazy per-page lookup and full enumeration (DocumentSession.ts:41-44).',
  },
  {
    kind: 'actions.read',
    group: 'G6',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'ported',
  },
  {
    kind: 'measure.viewports',
    group: 'G6',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'routed',
  },

  // --- G7. Annotation reads. Rust never creates or retains the form model;
  // TypeScript lends the pointer.
  {
    kind: 'annotations.listRawAll',
    group: 'G7',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'ported',
  },
  {
    kind: 'annotations.listRawPage',
    group: 'G7',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'ported',
  },
  {
    kind: 'annotations.listFullPage',
    group: 'G7',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'ported',
    note: 'Reaches the cached form model through joinWidgetField.ts:23, so it carries a repeated-read benchmark against that cache.',
  },
  {
    kind: 'annotations.renderAppearances',
    group: 'G7',
    stage: 'A',
    phase: 2,
    cancellation: 'non-preemptible',
    rustStatus: 'ported',
  },

  // --- G8. Files, forms reads, piece info.
  {
    kind: 'attachments.list',
    group: 'G8',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'ported',
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
    note: 'Byte equality with the TypeScript path, not a semantic comparison.',
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
    note: 'Filed by behaviour: a read despite the forms. prefix.',
  },
  {
    kind: 'forms.export',
    group: 'G8',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'routed',
  },
  {
    kind: 'signatures.list',
    group: 'G8',
    stage: 'A',
    phase: 2,
    cancellation: 'queued-only',
    rustStatus: 'routed',
    note: 'The signature handlers take no AbortSignal (WorkerHost.ts), so no throwIfAborted reaches them.',
  },
  {
    kind: 'signatures.contents',
    group: 'G8',
    stage: 'A',
    phase: 2,
    cancellation: 'queued-only',
    rustStatus: 'routed',
  },
  {
    kind: 'signatures.digest',
    group: 'G8',
    stage: 'A',
    phase: 2,
    cancellation: 'queued-only',
    rustStatus: 'routed',
  },
  {
    kind: 'signatures.revisionBytes',
    group: 'G8',
    stage: 'A',
    phase: 2,
    cancellation: 'queued-only',
    rustStatus: 'routed',
  },
  {
    kind: 'signatures.analyze',
    group: 'G8',
    stage: 'A',
    phase: 2,
    cancellation: 'queued-only',
    rustStatus: 'routed',
  },
  {
    kind: 'document.version',
    group: 'G8',
    stage: 'A',
    phase: 2,
    cancellation: 'queued-only',
    rustStatus: 'routed',
    note: 'SignatureReader.version: the hash and length of the base the session is on.',
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
    note: 'Creates and closes its own FPDF_CreateNewDocument scratch document (PagesExtractor.ts:56-89), a named exception to Stage A "retains nothing, closes nothing".',
  },
  {
    kind: 'annotations.exportAppearance',
    group: 'G9',
    stage: 'A',
    phase: 2,
    cancellation: 'non-preemptible',
    rustStatus: 'ported',
    note: 'A read that builds a scratch single-page document (EPDFPage_ExportAnnotationsAsDocument) and closes it before returning, like pages.extract. Gated by vectors/pages/mutate.json.',
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
    note: 'The Phase 0 Stage B bridge spike runs over this operation, then is thrown away.',
  },
  {
    kind: 'annotations.update',
    group: 'G11',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
  },
  {
    kind: 'annotations.delete',
    group: 'G11',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
  },
  {
    kind: 'annotations.move',
    group: 'G11',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
  },

  // --- G12. Forms writes.
  {
    kind: 'forms.setValue',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
  },
  {
    kind: 'forms.reset',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
  },
  {
    kind: 'forms.applyEffects',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
  },
  {
    kind: 'forms.import',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
  },
  {
    kind: 'forms.repair',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
  },
  {
    kind: 'forms.createField',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
  },
  {
    kind: 'forms.updateField',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
  },
  {
    kind: 'forms.deleteField',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
  },
  {
    kind: 'forms.attachWidget',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
  },
  {
    kind: 'forms.detachWidget',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
  },
  {
    kind: 'forms.setSignatureAppearance',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'routed',
  },
  {
    kind: 'signatures.prepare',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'queued-only',
    rustStatus: 'routed',
    note: 'Never touches the live document: the candidate is parked on the session until complete or abort.',
  },
  {
    kind: 'signatures.complete',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'queued-only',
    rustStatus: 'routed',
    note: 'Installs the sealed bytes as the session document; the install advances the mutation sequence itself.',
  },
  {
    kind: 'signatures.abort',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'queued-only',
    rustStatus: 'routed',
  },

  // --- G13. Page assembly and redaction.
  {
    kind: 'pages.rotate',
    group: 'G13',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
  },
  {
    kind: 'pages.move',
    group: 'G13',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
    note: 'Page identity must survive reorder within the same live document.',
  },
  {
    kind: 'pages.delete',
    group: 'G13',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
  },
  {
    kind: 'pages.insertBlank',
    group: 'G13',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'routed',
  },
  {
    kind: 'pages.insert',
    group: 'G13',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'routed',
    note: 'Source retention lives on one owner (PagesInserter.ts:77, session.retainUntilClose). A source document handle never crosses owners.',
  },
  {
    kind: 'pages.setName',
    group: 'G13',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'routed',
  },
  {
    kind: 'pages.removeName',
    group: 'G13',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'routed',
  },
  {
    kind: 'pages.flatten',
    group: 'G13',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
  },
  {
    kind: 'annotations.flatten',
    group: 'G13',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
    note: 'pages.flatten for a chosen set of one page\'s annotations. Gated by vectors/pages/mutate.json.',
  },
  {
    kind: 'redaction.apply',
    group: 'G13',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
  },

  // --- G14. Remaining mutators.
  {
    kind: 'metadata.update',
    group: 'G14',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
    note: 'MetadataPatch is the Phase 0 DTO codegen proof: three-state Keep | Clear | Set(T). Operation parity lands here.',
  },
  {
    kind: 'pieceInfo.update',
    group: 'G14',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
  },
  {
    kind: 'pieceInfo.clear',
    group: 'G14',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
  },
  {
    kind: 'measure.setScale',
    group: 'G14',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'routed',
  },
  {
    kind: 'attachments.create',
    group: 'G14',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
  },
  {
    kind: 'attachments.delete',
    group: 'G14',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'ported',
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
    note: 'Sole owner of the wire kind; the raster step comes from G4 internal entry point. Encoding stays host-injected.',
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
  },
  {
    kind: 'document.renderPageFileEncoded',
    group: 'G15',
    stage: 'C',
    phase: 5,
    cancellation: 'non-preemptible',
    rustStatus: 'routed',
  },
  {
    kind: 'signatures.finalizeCandidate',
    group: 'G15',
    stage: 'C',
    phase: 5,
    cancellation: 'queued-only',
    rustStatus: 'routed',
    note: 'Session-less, like document.renderPageFile: patches the caller\'s candidate file in place and reads it back through a document that is never a session.',
  },
];
