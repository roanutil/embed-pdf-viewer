/**
 * `packages/plugin/attachment`. Replayed by
 * `crates/plugins/attachment/tests/conformance.rs`.
 *
 * The attachments are document-scoped and read nothing when a document
 * opens: the scenarios ask (`ensureLoaded`, `refresh`), answer the read from
 * the fake engine's table (`attachments.list`), and then drive the
 * selection, the confirmed event stream (any origin), adds and removes (each
 * one change on the document's queue through `doc.apply`) and downloads
 * (`attachments.download`). Every case snapshots the plugin's slice and its
 * mirror's store cell.
 *
 * The optional plugins are run for real: the annotation plugin (a page's
 * files, removing and saving them, `canDelete`) and the stage (going to a
 * page's file), each case with or without them, and the history once, for
 * what undoing an add or a remove shows.
 *
 * One shim, mirrored by `crates/plugins/attachment/src/harness.rs`: an
 * option `signal: {"$abortSignal": name}` is the signal of a controller
 * named `name`, which the `attachment-signals` test plugin's `abort(name)`
 * aborts.
 *
 * A write's answer names the change's `opId`; ops ids come from one
 * deterministic UUID sequence per case (`installCaseEntropy`), so the n-th
 * id is `operationAt(n)`. Bytes are `{"$bytes": base64}`, which the runner
 * hands the plugin as a `Uint8Array`; a `File` or `Blob` source has no
 * spelling in a vector.
 */
import { createCapabilityToken, type AnyPlugin, type OperationOptions } from '@embedpdf/core';
import {
  changeEvents,
  type Annotation,
  type Attachment,
  type AttachmentRef,
  type ChangeItem,
  type DocumentEvent,
  type EventOrigin,
} from '@embedpdf/engine-core/runtime';

import { attachmentPlugin } from '../../../../packages/plugin/attachment/src/attachment.plugin';
import {
  AttachmentToken,
  type AttachedFile,
  type AttachmentCapability,
} from '../../../../packages/plugin/attachment/src/contract';
import { documentFileOf, pageFileOf } from '../../../../packages/plugin/attachment/src/model';

import { installCaseEntropy, restoreCaseEntropy } from '../case-entropy';
import type { ErrorSpec, FakeDocumentSpec, ResponseSpec } from '../fake-engine';
import { onCaseEnd, registerPlugin, type ScenarioCase, type Step } from '../scenario';
import type { Suite } from '../vectors';
import {
  PAGES,
  createdFact,
  deletedFact,
  fileAttachment,
  hydrate,
  objRef,
  operationAt,
  square,
  write,
  deleted as annotationDeleted,
} from './annotation-live-fixtures';
import { stageStep } from './stage-plugin';

// ── harness shims ─────────────────────────────────────────────────────────────

const controllers = new Map<string, AbortController>();
const controllerNamed = (name: string): AbortController => {
  let controller = controllers.get(name);
  if (!controller) {
    controller = new AbortController();
    controllers.set(name, controller);
  }
  return controller;
};
const withSignal = <T extends OperationOptions | undefined>(options: T): T => {
  const spec = (options as { signal?: { $abortSignal?: string } } | undefined)?.signal;
  return spec?.$abortSignal === undefined
    ? options
    : ({ ...options, signal: controllerNamed(spec.$abortSignal).signal } as T);
};

registerPlugin('attachment', {
  factory: () => {
    controllers.clear();
    installCaseEntropy();
    const plugin = attachmentPlugin() as AnyPlugin;
    return {
      ...plugin,
      create: (ctx) => {
        const { api, connect } = plugin.create!(ctx);
        const cap = api as AttachmentCapability;
        const wrapped: AttachmentCapability = {
          ...cap,
          ensureLoaded: (options) => cap.ensureLoaded(withSignal(options)),
          refresh: (options) => cap.refresh(withSignal(options)),
          add: (files, options) => cap.add(files, withSignal(options)),
          remove: (file, options) => cap.remove(file, withSignal(options)),
          download: (file, options) => cap.download(file, withSignal(options)),
        };
        return { api: wrapped, connect };
      },
    } as AnyPlugin;
  },
  tokens: [AttachmentToken],
});
onCaseEnd(restoreCaseEntropy);

const SignalsToken = createCapabilityToken<{ abort(name: string): void }>(
  'attachment-test/signals',
);
registerPlugin('attachment-signals', {
  factory: () =>
    ({
      id: 'attachment-test/signals',
      token: SignalsToken,
      create: () => ({ api: { abort: (name: string) => controllerNamed(name).abort() } }),
    }) as AnyPlugin,
  tokens: [SignalsToken],
});

// ── the attachments ───────────────────────────────────────────────────────────

const key = (k: string): AttachmentRef => ({ kind: 'key', key: k });
const bytes = (text: string) => ({ $bytes: Buffer.from(text).toString('base64') });

/** An attachment as a read returns it. */
const att = (name: string, index: number, over: Partial<Attachment> = {}): Attachment => ({
  ref: key(name),
  index,
  name,
  mimeType: null,
  description: null,
  size: null,
  checksum: null,
  createdAt: null,
  modifiedAt: null,
  ...over,
});

/**
 * In name-tree order, JavaScript's (UTF-16) order of the keys: `.` before
 * `R` before `n`, `notes (2).txt` before `notes.txt` (a space sorts before a
 * full stop), and the fullwidth `Ｆ` (U+FF26) last.
 */
const ENV = att('.env', 0, { size: 12 });
const README = att('README', 1, { mimeType: 'text/plain', description: 'Read me' });
const NOTES2 = att('notes (2).txt', 2, { mimeType: 'text/plain', size: 3 });
const NOTES = att('notes.txt', 3, {
  mimeType: 'text/plain',
  description: 'The notes',
  size: 5,
  checksum: '5d41402abc4b2a76b9719d911017c592',
  createdAt: '2024-01-02T03:04:05Z',
  modifiedAt: '2024-01-02T03:04:06Z',
});
const WIDE = att('Ｆ.bin', 4, { mimeType: 'application/octet-stream', size: 2 });
const FILES: Attachment[] = [ENV, README, NOTES2, NOTES, WIDE];
const LIST = { attachments: FILES };

/** The engine's record of a file it attached: name-tree position, checksum and dates. */
const made = (name: string, index: number, over: Partial<Attachment> = {}): Attachment =>
  att(name, index, { checksum: 'c0ffee', createdAt: '2025-05-05T00:00:00Z', ...over });

/** The list's `AttachedFile` for an attachment, as the shipping model builds it. */
const docFile = (attachment: Attachment): AttachedFile => documentFileOf(attachment);
/** A file for a caller to pass: what the list shows, with any member changed. */
const fileLike = (attachment: Attachment, over: Partial<AttachedFile> = {}): AttachedFile => ({
  ...docFile(attachment),
  ...over,
});

// ── the pages' files ──────────────────────────────────────────────────────────

const SCAN = fileAttachment(20, {
  file: {
    name: 'scan.png',
    mimeType: 'image/png',
    description: null,
    size: 8,
    checksum: null,
    createdAt: null,
    modifiedAt: null,
  },
});
const LOCKED = fileAttachment(21, {
  page: PAGES[1]!.ref,
  ref: objRef(21, 2),
  locked: true,
  file: {
    name: ' ‎page two‪.txt ',
    mimeType: null,
    description: 'On page two',
    size: null,
    checksum: null,
    createdAt: null,
    modifiedAt: null,
  },
});
/** A file attachment annotation whose file the read lost: no list row. */
const FILELESS = fileAttachment(22, { file: null } as never);
const BOX = square(23);
const ANNOTATIONS: Annotation[] = [SCAN, FILELESS, BOX, LOCKED];
const pageFile = (annotation: Annotation): AttachedFile => pageFileOf(annotation)!;

// ── the document ──────────────────────────────────────────────────────────────

const read = (value: unknown = LIST, times?: number): ResponseSpec => ({
  path: 'attachments.list',
  value,
  ...(times === undefined ? {} : { times }),
});
const deferRead = (label: string): ResponseSpec => ({
  path: 'attachments.list',
  defer: label,
  times: 1,
});
const failRead = (error: ErrorSpec): ResponseSpec => ({
  path: 'attachments.list',
  error,
  times: 1,
});
const downloads = (k: string, text: string): ResponseSpec => ({
  path: 'attachments.download',
  args: [key(k)],
  value: bytes(text),
});

const doc = (
  responses: ResponseSpec[] = [read()],
  over: Partial<FakeDocumentSpec> = {},
): { documents: { a: FakeDocumentSpec } } => ({
  documents: {
    a: {
      pages: PAGES,
      security: { allows: '*' },
      objectNumbers: [500, 501, 502],
      responses,
      ...over,
    },
  },
});

const LOCAL: EventOrigin = { kind: 'local', sessionId: 's1', sub: null, ts: 0, serverId: null };
const REMOTE: EventOrigin = { kind: 'remote', sessionId: 's2', sub: 'u-9', ts: 0, serverId: null };

const meta = (opId: string, changed: AttachmentRef[]) => ({
  affectedPages: [],
  cacheDelta: null,
  opId,
  undoable: true,
  changed,
});

/** The engine's item for one op of a change, built for the change named `opId`. */
type Answer = (opId: string) => ChangeItem;
const created =
  (attachment: Attachment): Answer =>
  (opId) => ({ type: 'attachments.create', attachment, meta: meta(opId, [attachment.ref]) });
const removed =
  (name: string): Answer =>
  (opId) => ({ type: 'attachments.delete', meta: meta(opId, [key(name)]) });
const restored =
  (attachment: Attachment): Answer =>
  (opId) => ({ type: 'attachments.restore', attachment, meta: meta(opId, [attachment.ref]) });

const resultOf = (opId: string, answers: Answer[]) => ({
  items: answers.map((answer) => answer(opId)),
  meta: { affectedPages: [], cacheDelta: null, opId, undoable: true },
});
/** The confirmed events of a result, as the engine publishes them for change `opId`. */
const factsOf = (opId: string, answers: Answer[], origin: EventOrigin = LOCAL): DocumentEvent[] => {
  const events = changeEvents(resultOf(opId, answers) as Parameters<typeof changeEvents>[0]);
  return events.map(
    (event, index) =>
      ({
        ...event,
        origin: { ...origin, tx: { id: opId, index, count: events.length } },
      }) as DocumentEvent,
  );
};

/** The n-th change's answer: its events, then its result. */
const applied = (n: number, ...answers: Answer[]): ResponseSpec => ({
  path: 'apply',
  times: 1,
  value: resultOf(operationAt(n), answers),
  events: factsOf(operationAt(n), answers),
});
const deferApply = (label: string): ResponseSpec => ({ path: 'apply', times: 1, defer: label });
const refusedApply: ResponseSpec = {
  path: 'apply',
  times: 1,
  error: { name: 'EngineError', message: 'no', code: 'Refused' },
};

// ── steps ─────────────────────────────────────────────────────────────────────

const start: Step = { op: 'call', target: 'kernel', method: 'start' };
const open: Step = {
  op: 'call',
  target: 'documents',
  method: 'open',
  args: [{ kind: 'bytes', id: 'a', bytes: { $bytes: '' } }],
};
const close: Step = { op: 'call', target: 'documents', method: 'close', args: ['a'] };

function at<M extends keyof AttachmentCapability>(
  method: M,
  args: readonly unknown[] = [],
  meter?: number,
): Step {
  return {
    op: 'call',
    target: { token: 'attachment', doc: 'a' },
    method,
    args,
    ...(meter === undefined ? {} : { meter }),
  };
}
const an = (method: string, args: readonly unknown[] = []): Step => ({
  op: 'call',
  target: { token: 'annotation', doc: 'a' },
  method,
  args,
});
const abort = (name: string): Step => ({
  op: 'call',
  target: { token: 'attachment-test/signals' },
  method: 'abort',
  args: [name],
});
const signal = (name: string) => ({ signal: { $abortSignal: name } });
const emit = (event: unknown, meter?: number): Step => ({
  op: 'emit',
  doc: 'a',
  event,
  ...(meter === undefined ? {} : { meter }),
});
const settle = (label: string, value: unknown, meter?: number): Step => ({
  op: 'settle',
  label,
  value,
  ...(meter === undefined ? {} : { meter }),
});
const refuse = (label: string, error: ErrorSpec): Step => ({ op: 'settle', label, error });
const st = (method: string, args: unknown[] = []): Step => stageStep('stage', 'a', method, args);
const viewport: Step = st('setViewport', [{ width: 1000, height: 700 }]);
const undo: Step = { op: 'call', target: { token: 'history', doc: 'a' }, method: 'undo', args: [] };
const undoLabel: Step = {
  op: 'call',
  target: { token: 'history', doc: 'a' },
  method: 'getUndoLabel',
  args: [],
};

/** Every read, one step each. */
const look: Step[] = [
  at('list'),
  at('getSelected'),
  at('getStatus'),
  at('canAdd'),
  at('canDownload'),
];
const hooks: Step[] = [
  at('onChanged', [{ $fn: 'changed' }]),
  at('onResynced', [{ $fn: 'resynced' }]),
];

// ── plugins ───────────────────────────────────────────────────────────────────

const ALONE = [{ factory: 'attachment' }, { factory: 'attachment-signals' }];
const INTERACTION = { factory: 'interaction' };
const ANNOTATION = { factory: 'annotation-live' };
const STAGE = { factory: 'stage', config: { viewUnitsPerPoint: 1, scrollBehavior: 'instant' } };
const HISTORY = { factory: 'history' };
const WITH_ANNOTATIONS = [INTERACTION, ANNOTATION, ...ALONE];
const SNAP = ['attachment::a', 'attachment::a/mirror/attachments'];

const scenario = (
  name: string,
  covers: string,
  engine: ReturnType<typeof doc>,
  steps: Step[],
  plugins: readonly { factory: string; config?: unknown }[] = ALONE,
): ScenarioCase => ({
  name,
  covers,
  plugins,
  engine,
  snapshot: SNAP,
  steps: [start, open, ...hooks, ...steps],
});

const denied = (required: string) => ({
  name: 'PermissionDenied',
  message: `permission denied (local): ${required}`,
  required,
  context: 'local',
  code: 'Forbidden',
  details: { required, context: 'local' },
});

// ── cases ─────────────────────────────────────────────────────────────────────

const cases: ScenarioCase[] = [
  scenario(
    'empty-until-loaded',
    'controller.ts:92-108 on-demand mirror: every read is empty and the status idle until ensureLoaded asks; loading while the read is out; the read lands in name-tree order as AttachedFiles (model.ts:41-44, id doc:<key>, home document) and fires onResynced, no onChanged; get by id, null for an id the list lacks; isPending (controller.ts:141-145) is true for an attachment before the first read lands and false once it is confirmed; a second ensureLoaded resolves at once',
    doc([deferRead('r'), read()]),
    [
      ...look,
      at('get', ['doc:notes.txt']),
      at('isPending', [docFile(NOTES)]),
      at('ensureLoaded'),
      at('getStatus'),
      at('list'),
      settle('r', LIST, 8),
      ...look,
      at('get', ['doc:notes.txt']),
      at('get', ['doc:Ｆ.bin']),
      at('get', ['doc:missing.txt']),
      at('get', ['page:obj:20']),
      at('isPending', [docFile(NOTES)]),
      at('isPending', [fileLike(att('missing.txt', 0))]),
      at('ensureLoaded', [], 4),
    ],
  ),
  scenario(
    'a-failed-read-reads-again',
    'controller.ts:288-289 ensureLoaded and refresh: a failed read rejects, the status is error and the next ensureLoaded reads again; refresh reads again and fires onResynced with the new list and no onChanged; a refresh during a read joins it and reads once more',
    doc([
      failRead({ name: 'EngineError', message: 'broken', code: 'Unknown' }),
      read(LIST, 1),
      deferRead('again'),
      deferRead('third'),
      read(),
    ]),
    [
      at('ensureLoaded'),
      at('getStatus'),
      at('list'),
      at('ensureLoaded'),
      at('getStatus'),
      at('refresh'),
      at('refresh'),
      at('getStatus'),
      settle('again', { attachments: [ENV, NOTES] }),
      at('list'),
      settle('third', { attachments: [NOTES] }, 12),
      ...look,
    ],
  ),
  scenario(
    'a-forbidden-read',
    'the mirror status is forbidden for a permission-denied read; the list stays empty and the selection keeps nothing',
    doc([failRead(denied('doc.open'))]),
    [at('ensureLoaded'), ...look, at('select', [[docFile(NOTES)]]), at('getSelected')],
  ),
  scenario(
    'both-homes-in-one-list',
    "controller.ts:114-126 list: the attachments in name-tree order, then each file attachment annotation with a file in page order (model.ts:47-54, id page:<annotationKey>, home page, its file info); a square and a file attachment annotation without a file have no row; before the attachments are read the pages' files list alone; an annotation created remotely joins and one deleted leaves; isPending of a page's file is the annotation plugin's; canRemove of a page's file is the annotation plugin's canDelete (false for a locked annotation, controller.ts:293-296), of an attachment canAdd",
    doc([hydrate(ANNOTATIONS), read()]),
    [
      at('list'),
      at('ensureLoaded'),
      at('list'),
      at('get', ['page:obj:20']),
      at('get', ['page:obj:22']),
      at('get', ['page:obj:23']),
      at('isPending', [pageFile(SCAN)]),
      at('canRemove', [pageFile(SCAN)]),
      at('canRemove', [pageFile(LOCKED)]),
      at('canRemove', [docFile(NOTES)]),
      emit(
        createdFact(
          fileAttachment(24, {
            file: {
              name: 'late.txt',
              mimeType: 'text/plain',
              description: null,
              size: 1,
              checksum: null,
              createdAt: null,
              modifiedAt: null,
            },
          }),
        ),
      ),
      at('list'),
      emit(deletedFact([SCAN.ref])),
      at('list'),
    ],
    WITH_ANNOTATIONS,
  ),
  scenario(
    'names-and-selection',
    'controller.ts:128-139,285 select and getSelected: the files by id in the order selected, each once, files the list lacks dropped; selecting the same ids leaves the state as it is (model.ts:24-29); [] clears; a file the list loses drops out of getSelected while its id stays selected',
    doc(),
    [
      at('select', [[docFile(NOTES)]]),
      at('getSelected'),
      at('ensureLoaded'),
      at('getSelected'),
      at('select', [[docFile(NOTES), docFile(ENV), docFile(NOTES), fileLike(att('gone', 0))]]),
      at('getSelected'),
      at('select', [[docFile(NOTES), docFile(ENV), fileLike(att('gone', 0))]]),
      at('getSelected'),
      emit(factsOf('r1', [removed('notes.txt')], REMOTE)[0]),
      at('getSelected'),
      at('select', [[]]),
      at('getSelected'),
    ],
  ),
  scenario(
    'add-shows-at-once-and-confirms',
    "controller.ts:178-220 add: the files show at once and pending (sync/files.ts:63-77: the name, type and description the op says, the size of its bytes, no checksum or dates), placed by key in UTF-16 order (sync/files.ts:32-39); a name the document has takes the first free number before the extension (model.ts:61-70: notes (3).txt, README (2), .env (2)), and two of one name in one add take two; one apply for the files; a key with a character outside the BMP sorts before U+FF26, as the UTF-16 comparison puts it, pending and confirmed; the confirmed events fire onChanged added with the origin before the promise resolves with the engine's records in order; isPending after",
    doc([read(), deferApply('a1'), deferApply('a2')]),
    [
      at('ensureLoaded'),
      at('add', [
        [
          { data: bytes('hello'), name: 'notes.txt', mimeType: 'text/plain', description: 'Again' },
          { data: bytes('readme'), name: 'README' },
          { data: bytes(''), name: '.env' },
          { data: bytes('x'), name: 'notes.txt' },
        ],
      ]),
      ...look,
      at('isPending', [docFile(att('notes (3).txt', 0))]),
      ...factsOf(operationAt(1), [
        created(made('.env (2)', 1, { size: 0 })),
        created(made('README (2)', 3, { size: 6 })),
        created(
          made('notes (3).txt', 5, { mimeType: 'text/plain', description: 'Again', size: 5 }),
        ),
        created(made('notes (4).txt', 6, { mimeType: 'text/plain', size: 1 })),
      ]).map((event) => emit(event)),
      settle(
        'a1',
        resultOf(operationAt(1), [
          created(made('.env (2)', 1, { size: 0 })),
          created(made('README (2)', 3, { size: 6 })),
          created(
            made('notes (3).txt', 5, { mimeType: 'text/plain', description: 'Again', size: 5 }),
          ),
          created(made('notes (4).txt', 6, { mimeType: 'text/plain', size: 1 })),
        ]),
        6,
      ),
      at('list'),
      at('add', [[{ data: bytes('📄'), name: '📄.txt' }]]),
      at('list'),
      emit(factsOf(operationAt(2), [created(made('📄.txt', 4, { size: 4 }))])[0]),
      settle('a2', resultOf(operationAt(2), [created(made('📄.txt', 4, { size: 4 }))]), 6),
      at('list'),
      at('isPending', [docFile(made('📄.txt', 4))]),
    ],
  ),
  scenario(
    'add-reads-first-and-checks-names',
    'controller.ts:204-210: an add with no files resolves [] and reads nothing; an add before the first read reads the attachments first, since the names in the document decide the free ones; a file without a name (absent, null or empty) rejects invalid-input once the read landed and stages nothing',
    doc([deferRead('r'), applied(1, created(made('notes (3).txt', 4, { size: 2 })))]),
    [
      at('add', [[]]),
      at('getStatus'),
      at('add', [[{ data: bytes('hi'), name: 'notes.txt' }]]),
      at('getStatus'),
      at('list'),
      settle('r', LIST, 10),
      at('list'),
      at('add', [[{ data: bytes('a'), name: 'fine.txt' }, { data: bytes('b') }]]),
      at('add', [[{ data: bytes('b'), name: null }]]),
      at('add', [[{ data: bytes('b'), name: '' }]]),
      at('list'),
    ],
  ),
  scenario(
    'remove-an-attachment',
    'controller.ts:222-243 remove: the file goes at once, one change (label attachment.remove), the confirmed event fires onChanged removed with the file as it was (controller.ts:83-86) before the promise resolves; a key the read list lacks rejects not-found and stages nothing; before the first read lands the engine decides',
    doc([deferRead('r'), applied(1, removed('README')), deferApply('x')]),
    [
      at('ensureLoaded'),
      at('remove', [docFile(README)], 4),
      at('getStatus'),
      settle('r', { attachments: [ENV, NOTES2, NOTES, WIDE] }),
      at('remove', [fileLike(att('missing.txt', 0))]),
      at('remove', [docFile(NOTES)]),
      ...look,
      at('isPending', [docFile(NOTES)]),
      emit(factsOf(operationAt(2), [removed('notes.txt')])[0], 4),
      settle('x', resultOf(operationAt(2), [removed('notes.txt')]), 4),
      at('list'),
    ],
  ),
  scenario(
    'remote-changes-fold-whoever-made-them',
    "sync/files.ts:48-59 foldFiles and controller.ts:73-90,99-107 onChanged with the event's origin: an event before the first read changes nothing; a remote create joins in key order and fires added; a remote delete fires removed with the file as it was; a delete naming no file or a key the list lacks changes nothing and fires nothing; a restore fires restored; an event of another kind fires nothing",
    doc(),
    [
      emit(factsOf('early', [created(made('early.txt', 0))], REMOTE)[0]),
      at('list'),
      at('ensureLoaded'),
      emit(factsOf('r1', [created(made('a.txt', 1))], REMOTE)[0]),
      at('list'),
      emit(factsOf('r2', [removed('notes (2).txt')], REMOTE)[0]),
      emit({ ...factsOf('r3', [removed('nothing')], REMOTE)[0]!, deleted: null }),
      emit(factsOf('r4', [removed('missing.txt')], REMOTE)[0]),
      at('list'),
      emit(factsOf('r5', [restored(NOTES2)], LOCAL)[0]),
      at('list'),
      emit({ type: 'metadata.updated', metadata: {}, previous: null, origin: REMOTE }),
      at('list'),
    ],
  ),
  scenario(
    'undo-shows-the-files-as-they-were',
    'sync/files.ts:98-125 undoOf: undoing a remove shows the file back, in its place, before the engine answers (attachments.restore); undoing an add shows the file gone (attachments.delete); a refused undo drops what it showed; controller.ts:211-213,237-239 the history labels, attachment.remove and attachment.add with the count',
    doc([
      read(),
      applied(1, removed('notes.txt')),
      deferApply('undo1'),
      applied(3, created(made('made.txt', 4, { size: 1 }))),
      deferApply('undo2'),
    ]),
    [
      at('ensureLoaded'),
      at('remove', [docFile(NOTES)]),
      at('list'),
      undoLabel,
      undo,
      at('list'),
      refuse('undo1', { name: 'EngineError', message: 'no', code: 'Refused' }),
      at('list'),
      at('add', [[{ data: bytes('m'), name: 'made.txt' }]]),
      at('list'),
      undoLabel,
      undo,
      at('list'),
    ],
    [HISTORY, ...ALONE],
  ),
  scenario(
    'download-an-attachment',
    'controller.ts:245-265 download: the engine\'s bytes with the file\'s name as a person may see it (names.ts:7-15: control and bidi characters gone, trimmed, "attachment" when nothing is left) and its type; a file this session is still adding is read once the change settled; an engine refusal maps to a PluginError',
    doc([
      read(),
      downloads('notes.txt', 'hello'),
      downloads('README', 'readme'),
      downloads('late.txt', 'late'),
      {
        path: 'attachments.download',
        args: [key('huge.bin')],
        error: {
          name: 'EngineError',
          message: 'too large',
          code: 'PayloadTooLarge',
          details: { limit: 268435456 },
        },
      },
      {
        path: 'attachments.download',
        args: [key('lost.bin')],
        error: { name: 'EngineError', message: 'no such file', code: 'NotFound' },
      },
      deferApply('add'),
    ]),
    [
      at('ensureLoaded'),
      at('download', [docFile(NOTES)]),
      at('download', [
        fileLike(README, { name: 'invoice‮fdp.exe\u0000', mimeType: 'application/x-msdownload' }),
      ]),
      at('download', [fileLike(README, { name: ' \u0007‏⁦ ' })]),
      at('download', [fileLike(att('huge.bin', 0))]),
      at('download', [fileLike(att('lost.bin', 0))]),
      at('add', [[{ data: bytes('late'), name: 'late.txt' }]]),
      at('download', [docFile(att('late.txt', 0))]),
      settle('add', resultOf(operationAt(1), [created(made('late.txt', 4, { size: 4 }))]), 10),
    ],
  ),
  scenario(
    'page-files-go-through-the-annotation-plugin',
    "controller.ts:222-225,251-254,267-279: removing a page's file deletes its annotation, downloading it reads the annotation's file resource; reveal selects the annotation and brings it into view on the stage, a third down; reveal of an attachment, or of a page's file the annotations lack, does nothing",
    doc([
      hydrate(ANNOTATIONS),
      read(),
      { path: 'page.annotations.downloadResource', pon: 2, value: bytes('two') },
      write(1, [{ item: annotationDeleted(SCAN.ref), changed: [SCAN.ref] }]),
    ]),
    [
      at('ensureLoaded'),
      viewport,
      at('reveal', [pageFile(LOCKED)]),
      an('selection.list'),
      st('getCamera'),
      at('reveal', [docFile(NOTES)]),
      at('reveal', [{ ...pageFile(SCAN), home: { kind: 'page', ref: objRef(99) } }]),
      an('selection.list'),
      at('download', [pageFile(LOCKED)], 8),
      at('remove', [pageFile(SCAN)], 6),
      at('list'),
    ],
    [INTERACTION, ANNOTATION, STAGE, ...ALONE],
  ),
  scenario(
    'without-the-optional-plugins',
    "controller.ts:110,141-142,169-176,267-279,293-296: with no annotation plugin the list has the attachments alone, a page's file is never pending and cannot be removed, removing or downloading one rejects not-found and reveal does nothing",
    doc([read()]),
    [
      at('ensureLoaded'),
      at('list'),
      at('isPending', [pageFile(SCAN)]),
      at('canRemove', [pageFile(SCAN)]),
      at('remove', [pageFile(SCAN)]),
      at('download', [pageFile(SCAN)]),
      at('reveal', [pageFile(SCAN)]),
    ],
  ),
  scenario(
    'reveal-without-the-stage',
    "controller.ts:267-279: with the annotation plugin and no stage, reveal selects the page file's annotation and moves nothing",
    doc([hydrate(ANNOTATIONS), read()]),
    [at('reveal', [pageFile(SCAN)]), an('selection.list')],
    WITH_ANNOTATIONS,
  ),
  scenario(
    'without-doc-attachments-modify',
    'controller.ts:149-155,291-296: without doc.attachments.modify canAdd and canRemove of an attachment are false, add and remove reject permission-denied before anything is read, staged or written (no attachments.list, no apply); download still works',
    doc([read(), downloads('notes.txt', 'hello')], {
      security: { allows: ['doc.open', 'doc.download'] },
    }),
    [
      ...look,
      at('canRemove', [docFile(NOTES)]),
      at('add', [[{ data: bytes('x'), name: 'x.txt' }]]),
      at('remove', [docFile(NOTES)]),
      at('getStatus'),
      at('ensureLoaded'),
      at('add', [[{ data: bytes('x'), name: 'x.txt' }]]),
      at('remove', [docFile(NOTES)]),
      at('list'),
      at('download', [docFile(NOTES)]),
    ],
  ),
  scenario(
    'without-doc-download',
    "controller.ts:255,291: without doc.download canDownload is false and downloading an attachment rejects permission-denied without asking the engine, and a page's file is refused by the annotation plugin the same way; adding still works",
    doc(
      [
        read(),
        hydrate(ANNOTATIONS),
        downloads('notes.txt', 'hello'),
        applied(1, created(made('x.txt', 5, { size: 1 }))),
      ],
      {
        security: {
          allows: [
            'doc.open',
            'doc.annotate.read',
            'doc.annotate.modify',
            'doc.attachments.modify',
          ],
        },
      },
    ),
    [
      at('canDownload'),
      at('canAdd'),
      at('ensureLoaded'),
      at('download', [docFile(NOTES)]),
      at('download', [pageFile(SCAN)]),
      at('add', [[{ data: bytes('x'), name: 'x.txt' }]]),
    ],
    WITH_ANNOTATIONS,
  ),
  scenario(
    'annotation-can-delete',
    "controller.ts:293-296: canRemove of a page's file is the annotation plugin's canDelete, false when the document refuses deleting annotations, while canRemove of an attachment is canAdd",
    doc([hydrate(ANNOTATIONS), read()], {
      security: { allows: '*', allowsAnnotation: { delete: false } },
    }),
    [
      at('list'),
      an('canDelete', [SCAN.ref]),
      at('canRemove', [pageFile(SCAN)]),
      at('canRemove', [docFile(NOTES)]),
      at('canAdd'),
    ],
    WITH_ANNOTATIONS,
  ),
  scenario(
    'an-engine-refusal',
    'controller.ts:157-167 answer: a refused add or remove rejects with the refusal as a PluginError and the view drops what it showed; no onChanged',
    doc([read(), deferApply('x'), refusedApply]),
    [
      at('ensureLoaded'),
      at('add', [[{ data: bytes('no'), name: 'no.txt' }]]),
      at('list'),
      refuse('x', { name: 'EngineError', message: 'no', code: 'Refused' }),
      at('list'),
      at('remove', [docFile(NOTES)], 6),
      at('list'),
    ],
  ),
  scenario(
    'cancellation',
    'controller.ts:149-167,208,257-260: a signal aborted before a call refuses add, remove and download before anything starts; an add aborted while it reads rejects operation-cancelled and stages nothing while the read lands; a write aborted while its answer is on its way rejects and the change still lands; a download aborted while the engine reads aborts that call; ensureLoaded races the signal',
    doc([
      deferRead('r'),
      deferApply('c'),
      { path: 'attachments.download', args: [key('.env')], defer: 'd', times: 1 },
    ]),
    [
      at('ensureLoaded', [signal('load')]),
      abort('load'),
      at('add', [[{ data: bytes('x'), name: 'x.txt' }], signal('read')]),
      abort('read'),
      at('getStatus'),
      settle('r', LIST),
      at('getStatus'),
      at('list'),
      abort('gone'),
      at('add', [[{ data: bytes('x'), name: 'x.txt' }], signal('gone')]),
      at('remove', [docFile(NOTES), signal('gone')]),
      at('download', [docFile(NOTES), signal('gone')]),
      at('ensureLoaded', [signal('gone')]),
      at('remove', [docFile(NOTES), signal('late')]),
      abort('late'),
      at('list'),
      emit(factsOf(operationAt(1), [removed('notes.txt')])[0]),
      settle('c', resultOf(operationAt(1), [removed('notes.txt')])),
      at('list'),
      at('download', [docFile(ENV), signal('slow')]),
      abort('slow'),
    ],
  ),
  scenario(
    'close',
    'a read on its way when the document closes resolves undefined (the mirror drops an instance-closed load); a write on its way rejects instance-closed; reopening starts idle',
    doc([deferRead('r'), read(), deferApply('c')]),
    [
      at('ensureLoaded'),
      close,
      open,
      ...hooks,
      ...look,
      at('ensureLoaded'),
      at('remove', [docFile(NOTES)]),
      close,
    ],
  ),
];

export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/attachment.json',
    describes:
      "packages/plugin/attachment: the on-demand attachments mirror joined with the pages' file attachment annotations, the selection, adds and removes through doc.apply, downloads, and the optional annotation and stage plugins.",
    cases,
  },
];
