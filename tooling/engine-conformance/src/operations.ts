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
  /** Set when the Rust reader exists and a conformance gate covers it. */
  readonly rustStatus: 'ported' | 'planned';
  /** Anything a reader of the matrix alone would otherwise get wrong. */
  readonly note?: string;
}

export const OPERATIONS: readonly OperationRow[] = [
  // --- G2. Page geometry. The first operation ported, and the only one whose
  // Rust reader exists today.
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
    rustStatus: 'planned',
    note: 'No throwIfAborted reaches PdfDocumentOpener: once an open starts it runs to completion.',
  },
  {
    kind: 'open.layerMemBase',
    group: 'G3',
    stage: 'A',
    phase: 1,
    cancellation: 'queued-only',
    rustStatus: 'planned',
  },
  {
    kind: 'open.layerFileBase',
    group: 'G3',
    stage: 'A',
    phase: 1,
    cancellation: 'queued-only',
    rustStatus: 'planned',
  },
  {
    kind: 'document.checkPasswordPermissions',
    group: 'G3',
    stage: 'A',
    phase: 1,
    cancellation: 'queued-only',
    rustStatus: 'planned',
  },
  {
    kind: 'document.probeSecurityFile',
    group: 'G3',
    stage: 'A',
    phase: 1,
    cancellation: 'queued-only',
    rustStatus: 'planned',
  },
  {
    kind: 'close',
    group: 'G3',
    stage: 'control',
    phase: 1,
    cancellation: 'control',
    rustStatus: 'planned',
    note: 'Idempotent: rejects new work, invalidates handles, releases on the owner thread. Closing an engine closes its documents.',
  },
  {
    kind: 'layer.close',
    group: 'G3',
    stage: 'control',
    phase: 1,
    cancellation: 'control',
    rustStatus: 'planned',
  },
  {
    kind: 'abort',
    group: 'G3',
    stage: 'control',
    phase: 1,
    cancellation: 'control',
    rustStatus: 'planned',
    note: 'Handled before the WorkerHost switch (WorkerHost.ts:199), which is why that switch has 64 arms and the protocol has 65 kinds.',
  },
  {
    kind: 'shutdown',
    group: 'G3',
    stage: 'control',
    phase: 1,
    cancellation: 'control',
    rustStatus: 'planned',
  },

  // --- G4. Render and fonts. Fonts are engine-level and move as one unit,
  // independently of which backend owns a document.
  {
    kind: 'pages.render',
    group: 'G4',
    stage: 'A',
    phase: 1,
    cancellation: 'non-preemptible',
    rustStatus: 'planned',
    note: 'Cooperative up to FPDF_RenderPageBitmapWithMatrix (PageRenderReader.ts:51), which cannot be interrupted.',
  },
  {
    kind: 'fonts.register',
    group: 'G4',
    stage: 'A',
    phase: 1,
    cancellation: 'queued-only',
    rustStatus: 'planned',
    note: 'Engine-level, not per document. One Rust FontRegistry per combined worker owns the key-to-ID map and the retained handles.',
  },
  {
    kind: 'fonts.addFallback',
    group: 'G4',
    stage: 'A',
    phase: 1,
    cancellation: 'queued-only',
    rustStatus: 'planned',
  },
  {
    kind: 'fonts.clearFallbacks',
    group: 'G4',
    stage: 'A',
    phase: 1,
    cancellation: 'queued-only',
    rustStatus: 'planned',
  },
  {
    kind: 'fonts.clear',
    group: 'G4',
    stage: 'A',
    phase: 1,
    cancellation: 'queued-only',
    rustStatus: 'planned',
  },

  // --- G5. Text and search.
  {
    kind: 'pages.text',
    group: 'G5',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'planned',
    note: 'Already coarse (PageTextReader.ts:93). Gate on correctness, not on the boundary.',
  },
  {
    kind: 'search.query',
    group: 'G5',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'planned',
    note: 'Entry gate: a dialect vector file for regex.ts and fold.ts semantics before any porting starts.',
  },

  // --- G6. Document structure reads.
  {
    kind: 'metadata.read',
    group: 'G6',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'planned',
  },
  {
    kind: 'pages.list',
    group: 'G6',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'planned',
    note: 'Page identity must agree between lazy per-page lookup and full enumeration (DocumentSession.ts:41-44).',
  },
  {
    kind: 'actions.read',
    group: 'G6',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'planned',
  },

  // --- G7. Annotation reads. Rust never creates or retains the form model;
  // TypeScript lends the pointer.
  {
    kind: 'annotations.listRawAll',
    group: 'G7',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'planned',
  },
  {
    kind: 'annotations.listRawPage',
    group: 'G7',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'planned',
  },
  {
    kind: 'annotations.listFullPage',
    group: 'G7',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'planned',
    note: 'Reaches the cached form model through joinWidgetField.ts:23, so it carries a repeated-read benchmark against that cache.',
  },
  {
    kind: 'annotations.renderAppearances',
    group: 'G7',
    stage: 'A',
    phase: 2,
    cancellation: 'non-preemptible',
    rustStatus: 'planned',
  },

  // --- G8. Files, forms reads, piece info.
  {
    kind: 'attachments.list',
    group: 'G8',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'planned',
  },
  {
    kind: 'attachments.readFile',
    group: 'G8',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'planned',
  },
  {
    kind: 'annotations.readFile',
    group: 'G8',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'planned',
    note: 'Byte equality with the TypeScript path, not a semantic comparison.',
  },
  {
    kind: 'pieceInfo.read',
    group: 'G8',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'planned',
  },
  {
    kind: 'pieceInfo.applications',
    group: 'G8',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'planned',
  },
  {
    kind: 'forms.list',
    group: 'G8',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'planned',
    note: 'Filed by behaviour: a read despite the forms. prefix.',
  },
  {
    kind: 'forms.export',
    group: 'G8',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'planned',
  },

  // --- G9. Page extract. Rust owns a scratch document end to end and closes
  // it before returning; it never takes ownership of the session's.
  {
    kind: 'pages.extract',
    group: 'G9',
    stage: 'A',
    phase: 2,
    cancellation: 'cooperative',
    rustStatus: 'planned',
    note: 'Creates and closes its own FPDF_CreateNewDocument scratch document (PagesExtractor.ts:56-89), a named exception to Stage A "retains nothing, closes nothing".',
  },

  // --- G10. Session authority and save. Stage B.
  {
    kind: 'document.saveBuffer',
    group: 'G10',
    stage: 'B',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'planned',
  },
  {
    kind: 'document.saveFile',
    group: 'G10',
    stage: 'B',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'planned',
  },
  {
    kind: 'document.saveLayerBuffer',
    group: 'G10',
    stage: 'B',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'planned',
  },

  // --- G11. Annotation writes.
  {
    kind: 'annotations.create',
    group: 'G11',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'planned',
    note: 'The Phase 0 Stage B bridge spike runs over this operation, then is thrown away.',
  },
  {
    kind: 'annotations.update',
    group: 'G11',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'planned',
  },
  {
    kind: 'annotations.delete',
    group: 'G11',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'planned',
  },
  {
    kind: 'annotations.move',
    group: 'G11',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'planned',
  },

  // --- G12. Forms writes.
  {
    kind: 'forms.setValue',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'planned',
  },
  {
    kind: 'forms.reset',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'planned',
  },
  {
    kind: 'forms.applyEffects',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'planned',
  },
  {
    kind: 'forms.import',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'planned',
  },
  {
    kind: 'forms.repair',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'planned',
  },
  {
    kind: 'forms.createField',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'planned',
  },
  {
    kind: 'forms.updateField',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'planned',
  },
  {
    kind: 'forms.deleteField',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'planned',
  },
  {
    kind: 'forms.attachWidget',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'planned',
  },
  {
    kind: 'forms.detachWidget',
    group: 'G12',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'planned',
  },

  // --- G13. Page assembly and redaction.
  {
    kind: 'pages.rotate',
    group: 'G13',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'planned',
  },
  {
    kind: 'pages.move',
    group: 'G13',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'planned',
    note: 'Page identity must survive reorder within the same live document.',
  },
  {
    kind: 'pages.delete',
    group: 'G13',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'planned',
  },
  {
    kind: 'pages.insertBlank',
    group: 'G13',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'planned',
  },
  {
    kind: 'pages.insert',
    group: 'G13',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'planned',
    note: 'Source retention lives on one owner (PagesInserter.ts:77, session.retainUntilClose). A source document handle never crosses owners.',
  },
  {
    kind: 'pages.flatten',
    group: 'G13',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'planned',
  },
  {
    kind: 'redaction.apply',
    group: 'G13',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'planned',
  },

  // --- G14. Remaining mutators.
  {
    kind: 'metadata.update',
    group: 'G14',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'planned',
    note: 'MetadataPatch is the Phase 0 DTO codegen proof: three-state Keep | Clear | Set(T). Operation parity lands here.',
  },
  {
    kind: 'pieceInfo.update',
    group: 'G14',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'planned',
  },
  {
    kind: 'pieceInfo.clear',
    group: 'G14',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'planned',
  },
  {
    kind: 'attachments.create',
    group: 'G14',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'planned',
  },
  {
    kind: 'attachments.delete',
    group: 'G14',
    stage: 'C',
    phase: 4,
    cancellation: 'after-first-write',
    rustStatus: 'planned',
  },

  // --- G15. Server and file surfaces. Browser and local workers keep
  // returning NotImplemented for the encoded kinds (worker-protocol.ts:446).
  {
    kind: 'pages.renderEncoded',
    group: 'G15',
    stage: 'C',
    phase: 5,
    cancellation: 'non-preemptible',
    rustStatus: 'planned',
    note: 'Sole owner of the wire kind; the raster step comes from G4 internal entry point. Encoding stays host-injected.',
  },
  {
    kind: 'annotations.renderAppearancesEncoded',
    group: 'G15',
    stage: 'C',
    phase: 5,
    cancellation: 'non-preemptible',
    rustStatus: 'planned',
  },
  {
    kind: 'document.renderPageFile',
    group: 'G15',
    stage: 'C',
    phase: 5,
    cancellation: 'non-preemptible',
    rustStatus: 'planned',
  },
  {
    kind: 'document.renderPageFileEncoded',
    group: 'G15',
    stage: 'C',
    phase: 5,
    cancellation: 'non-preemptible',
    rustStatus: 'planned',
  },
];
