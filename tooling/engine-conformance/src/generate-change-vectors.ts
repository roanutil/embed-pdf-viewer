/**
 * Emits `vectors/changes/*.json`: the change ops and reads upstream added
 * after 074a190c9, as the worker answers them.
 *
 *   pnpm --filter @embedpdf/tooling-engine-conformance run changes
 *   pnpm --filter @embedpdf/tooling-engine-conformance run changes:check
 *
 * `crates/epdf-engine/tests/change_ops.rs` replays it.
 *
 * | File | Upstream |
 * |---|---|
 * | `viewports.json` | `pages.list` carries each page's `/VP` viewports (726ec2dd6, c7f8d75ff) |
 * | `scale.json` | `pages.setScale` as a change op, and its undo (c7f8d75ff, 65b0f137e) |
 * | `rename.json` | an annotation update's `patch.nm` renames it (756f8e56b) |
 * | `annotation-copy.json` | `annotations.copy` (1481e4556, 9eead5531, 5496eb084) |
 * | `form-copy.json` | `forms.copy` (a390171cc, 2612ae1e1) |
 * | `kinds.json` | the version each change kind arrived in (a4805a492) |
 *
 * # What is recorded
 *
 * A case is a list of steps against one freshly opened fixture. Each step is
 * one call of the public API (`doc.apply`, `doc.pages.list`, a page's
 * `annotations.list`, `doc.forms.list`), and what it records is what crossed
 * the worker boundary: every request the call sent, and the worker's answer
 * to each, `resolve` with its result or `reject` with its error's code (the
 * two engines word their refusals differently). The worker is what the Rust
 * `Host` replaces, so that is the level the two are compared at.
 *
 * `jobId` is the session's own counter and is recorded as `"<job>"`. Every
 * write names its `opId`, so an undo can name it and the answers are the
 * same on every run. A name the engine makes up for a copy (a UUIDv7) is
 * recorded as `"<uuid>"`, and a timestamp the clock wrote as `"<now>"`;
 * the Rust runner masks its own answers the same way.
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  LazyTransport,
  LocalEngine,
  toPageRef,
  type Transport,
  type WorkerRequest,
} from '../../../packages/engine/main/src/index';
import { createInlineTransport } from '../../../packages/engine/main/src/transport/InlineTransport';
import type { Change } from '../../../packages/engine/core/src/mutation/Change';
import { API_VERSION, lacksWhatArrivedIn } from '../../../packages/engine/core/src/wire/apiVersion';
import {
  CHANGE_KIND_SINCE,
  changeKindSince,
  changeKindsUnknownTo,
  type ChangeKind,
} from '../../../packages/engine/core/src/wire/changeKinds';
import { CHANGE_KINDS } from '../../../packages/engine/core/src/wire/changeSchemas';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(packageRoot, '../..');
const vectorsRoot = join(packageRoot, 'vectors', 'changes');

/** Bump when the envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 1;

const VIEWPORTS = 'tooling/engine-conformance/fixtures/viewports.pdf';
const ANNOTATIONS = 'tooling/engine-conformance/fixtures/annotation_base.pdf';
const FORMS = 'tooling/engine-conformance/fixtures/form_fields.pdf';

type Step =
  | { readonly apply: Change; readonly opId: string }
  | { readonly read: 'pages.list' }
  | { readonly read: 'annotations.list'; readonly page: number }
  | { readonly read: 'forms.list' };

interface Case {
  readonly name: string;
  readonly covers: string;
  readonly steps: readonly Step[];
}

interface Group {
  readonly file: string;
  readonly kind: string;
  readonly fixture: string;
  readonly cases: readonly Case[];
}

const page = (objectNumber: number) => toPageRef(objectNumber);
/** An annotation (or widget) on page 3, the one page of both annotation fixtures. */
const annot = (objectNumber: number) =>
  ({ kind: 'objectNumber', page: page(3), objectNumber }) as const;
const field = (objectNumber: number) => ({ kind: 'objectNumber', objectNumber }) as const;

/** The scale `measurements.test.ts` writes. */
const SCALE = {
  subtype: 'rectilinear',
  ratio: '1 in = 2 m',
  x: [{ unit: 'm', conversion: 0.25, precision: 100 }],
  distance: [{ unit: 'm', conversion: 1, precision: 100 }],
  area: [{ unit: 'm²', conversion: 1, precision: 100 }],
} as const;

const OTHER_SCALE = {
  subtype: 'rectilinear',
  ratio: '1 in = 4 ft',
  x: [{ unit: 'ft', conversion: 0.5, precision: 10 }],
  distance: [{ unit: 'ft', conversion: 1, precision: 10 }],
  area: [{ unit: 'ft²', conversion: 1, precision: 10 }],
} as const;

const apply = (opId: string, change: unknown): Step => ({ apply: change as Change, opId });
const LIST: Step = { read: 'pages.list' };

const GROUPS: readonly Group[] = [
  {
    file: 'viewports.json',
    kind: 'pages.list.viewports',
    fixture: VIEWPORTS,
    cases: [
      {
        name: 'the-layout-carries-each-pages-viewports',
        covers:
          'Page 3 has a foreign viewport then the engine\'s own, in drawing order, the own one marked owned. Page 4 has none and carries no viewports key. Page 5 is turned a quarter: its geospatial viewport comes back in page space, not owned, with the measure the file has.',
        steps: [LIST],
      },
    ],
  },
  {
    file: 'scale.json',
    kind: 'document.apply.pages.setScale',
    fixture: VIEWPORTS,
    cases: [
      {
        name: 'a-scale-on-a-page-without-one',
        covers:
          'Page 4 gets a viewport of its own, over the visible box (its crop box), named EmbedPDF. The result names the page and carries the new layout; so does the read after it.',
        steps: [apply('scale:add', { ops: [{ type: 'pages.setScale', pages: [page(4)], measure: SCALE }] }), LIST],
      },
      {
        name: 'a-scale-replaces-the-engines-own-viewport',
        covers:
          'Page 3 already has one the engine owns: that one becomes the new scale over the visible page, in its place; the foreign one stays as it is.',
        steps: [apply('scale:replace', { ops: [{ type: 'pages.setScale', pages: [page(3)], measure: OTHER_SCALE }] }), LIST],
      },
      {
        name: 'null-removes-the-engines-own-viewport',
        covers: 'A null measure removes every viewport the engine owns on the page and leaves the foreign one.',
        steps: [apply('scale:remove', { ops: [{ type: 'pages.setScale', pages: [page(3)], measure: null }] }), LIST],
      },
      {
        name: 'null-on-a-page-with-none-of-its-own',
        covers: 'Page 5 has only a geospatial viewport, which the engine does not own: removing the scale leaves it.',
        steps: [apply('scale:none', { ops: [{ type: 'pages.setScale', pages: [page(5)], measure: null }] }), LIST],
      },
      {
        name: 'a-scale-on-a-turned-page',
        covers:
          'Page 5 is turned a quarter. Its own viewport covers the visible page in the file, and the layout reads it back in page space, after the geospatial one.',
        steps: [apply('scale:turned', { ops: [{ type: 'pages.setScale', pages: [page(5)], measure: SCALE }] }), LIST],
      },
      {
        name: 'one-change-scales-several-pages',
        covers:
          'Three pages of three sizes in one op: each takes the scale on its own visible box. The result names the first page; the item names them all.',
        steps: [apply('scale:many', { ops: [{ type: 'pages.setScale', pages: [page(3), page(4), page(5)], measure: SCALE }] }), LIST],
      },
      {
        name: 'a-page-named-twice-is-refused',
        covers: 'The same page twice is refused with InvalidArg before anything is written.',
        steps: [apply('scale:twice', { ops: [{ type: 'pages.setScale', pages: [page(4), page(4)], measure: SCALE }] }), LIST],
      },
      {
        name: 'an-unknown-page-is-refused',
        covers: 'An object number no page has refuses the change.',
        steps: [apply('scale:unknown', { ops: [{ type: 'pages.setScale', pages: [page(99)], measure: SCALE }] })],
      },
      {
        name: 'an-undo-puts-back-each-pages-scale',
        covers:
          'Three pages with three different scales before (another one, none, none of its own): the undo puts each back, in steps grouped by what the page had and its box.',
        steps: [
          apply('scale:set', { ops: [{ type: 'pages.setScale', pages: [page(3), page(4), page(5)], measure: SCALE }] }),
          apply('scale:undo', { undoOf: 'scale:set' }),
          LIST,
          apply('scale:redo', { undoOf: 'scale:undo' }),
          LIST,
        ],
      },
      {
        name: 'an-undo-leaves-a-page-whose-scale-moved-on',
        covers:
          'A page given another scale after the change is not the change\'s to undo: the undo skips it and puts back only the other page.',
        steps: [
          apply('scale:first', { ops: [{ type: 'pages.setScale', pages: [page(3), page(4)], measure: SCALE }] }),
          apply('scale:later', { ops: [{ type: 'pages.setScale', pages: [page(4)], measure: OTHER_SCALE }] }),
          apply('scale:undo-first', { undoOf: 'scale:first' }),
          LIST,
        ],
      },
    ],
  },
  {
    file: 'rename.json',
    kind: 'document.apply.annotations.rename',
    fixture: ANNOTATIONS,
    cases: [
      {
        name: 'an-update-renames',
        covers: 'patch.nm writes the annotation\'s own /NM on the same object; its replies still reply to it.',
        steps: [
          apply('rename:new', { ops: [{ type: 'annotations.update', ref: annot(5), patch: { nm: 'renamed-anchor' } }] }),
          { read: 'annotations.list', page: 3 },
        ],
      },
      {
        name: 'null-removes-the-name',
        covers: 'null removes /NM.',
        steps: [
          apply('rename:null', { ops: [{ type: 'annotations.update', ref: annot(5), patch: { nm: null } }] }),
          { read: 'annotations.list', page: 3 },
        ],
      },
      {
        name: 'an-empty-name-removes-it',
        covers: 'An empty name names nothing: it removes /NM, as a create given one writes none.',
        steps: [apply('rename:empty', { ops: [{ type: 'annotations.update', ref: annot(7), patch: { nm: '' } }] })],
      },
      {
        name: 'a-name-another-annotation-has-is-refused',
        covers: 'A new name must be free on the page (ISO 32000-2 §12.5.2): InvalidArg.',
        steps: [apply('rename:taken', { ops: [{ type: 'annotations.update', ref: annot(5), patch: { nm: 'a-reply' } }] })],
      },
      {
        name: 'the-name-it-has-is-no-change',
        covers: 'A name sent back as it reads is not a new name, and passes.',
        steps: [apply('rename:same', { ops: [{ type: 'annotations.update', ref: annot(5), patch: { nm: 'the-anchor' } }] })],
      },
      {
        name: 'a-name-an-earlier-op-freed',
        covers:
          'Checked against the page as the change\'s earlier ops left it: the first op frees the-anchor, and the second takes it.',
        steps: [
          apply('rename:swap', {
            ops: [
              { type: 'annotations.update', ref: annot(5), patch: { nm: 'was-the-anchor' } },
              { type: 'annotations.update', ref: annot(7), patch: { nm: 'the-anchor' } },
            ],
          }),
          { read: 'annotations.list', page: 3 },
        ],
      },
      {
        name: 'an-undo-puts-the-name-back',
        covers: 'The undo of a rename gives the annotation the name it had.',
        steps: [
          apply('rename:do', { ops: [{ type: 'annotations.update', ref: annot(5), patch: { nm: 'renamed-anchor' } }] }),
          apply('rename:undo', { undoOf: 'rename:do' }),
          { read: 'annotations.list', page: 3 },
        ],
      },
    ],
  },
  {
    file: 'annotation-copy.json',
    kind: 'document.apply.annotations.copy',
    fixture: ANNOTATIONS,
    cases: [
      {
        name: 'copy-one',
        covers:
          'One annotation copied onto its own page, where it was: a new object with the source\'s data, a name of its own, no links. The refMap maps the source to its copy.',
        steps: [
          apply('copy:one', { ops: [{ type: 'annotations.copy', refs: [annot(5)] }] }),
          { read: 'annotations.list', page: 3 },
        ],
      },
      {
        name: 'copy-moved-by-an-offset',
        covers: 'The offset is in page points from the top-left: x to the right, y down.',
        steps: [apply('copy:offset', { ops: [{ type: 'annotations.copy', refs: [annot(5)], offset: { x: 30, y: 40 } }] })],
      },
      {
        name: 'a-reply-copied-with-its-parent-replies-to-its-copy',
        covers:
          'The anchor and two replies (one a reply, one a group member): the copies form a group of their own, each reply replying to the anchor\'s copy with the same type.',
        steps: [
          apply('copy:group', { ops: [{ type: 'annotations.copy', refs: [annot(5), annot(7), annot(8)] }] }),
          { read: 'annotations.list', page: 3 },
        ],
      },
      {
        name: 'a-reply-copied-alone-stands-alone',
        covers: 'A reply whose parent is not copied loses its link.',
        steps: [apply('copy:alone', { ops: [{ type: 'annotations.copy', refs: [annot(7)] }] })],
      },
      {
        name: 'an-unsupported-kind-is-dropped',
        covers: 'A kind the engine does not model is left out, and dropped names it; the rest is copied.',
        steps: [apply('copy:drop', { ops: [{ type: 'annotations.copy', refs: [annot(10), annot(9)] }] })],
      },
      {
        name: 'a-reversed-rect-refuses-the-copy',
        covers:
          'Annotation 6 has its /Rect corners the other way round and reads with a negative width, which its create schema refuses: the whole change is refused with InvalidArg before the first write.',
        steps: [apply('copy:reversed', { ops: [{ type: 'annotations.copy', refs: [annot(9), annot(6)] }] })],
      },
      {
        name: 'one-named-twice-is-copied-twice',
        covers: 'Named twice, copied twice; the refMap has it twice.',
        steps: [apply('copy:twice', { ops: [{ type: 'annotations.copy', refs: [annot(9), annot(9)] }] })],
      },
      {
        name: 'the-copies-take-the-numbers-given',
        covers: 'objectNumbers names each copy, in the order of the refs.',
        steps: [apply('copy:numbers', { ops: [{ type: 'annotations.copy', refs: [annot(5), annot(9)], objectNumbers: [40, 41] }] })],
      },
      {
        name: 'an-unknown-annotation-is-refused',
        covers: 'A ref no annotation has refuses the change.',
        steps: [apply('copy:unknown', { ops: [{ type: 'annotations.copy', refs: [annot(99)] }] })],
      },
      {
        name: 'an-undo-takes-the-copies-away',
        covers: 'The undo deletes the copies; the page reads as it did.',
        steps: [
          apply('copy:do', { ops: [{ type: 'annotations.copy', refs: [annot(5), annot(7)] }] }),
          apply('copy:undo', { undoOf: 'copy:do' }),
          { read: 'annotations.list', page: 3 },
        ],
      },
    ],
  },
  {
    file: 'form-copy.json',
    kind: 'document.apply.forms.copy',
    fixture: FORMS,
    cases: [
      {
        name: 'copy-a-text-field',
        covers:
          'A text field copied: a new field under a free name, with its widget on the same page, and its source\'s value.',
        steps: [apply('fcopy:text', { ops: [{ type: 'forms.copy', fields: [field(4)] }] }), { read: 'forms.list' }],
      },
      {
        name: 'copy-without-values',
        covers: 'values: false leaves the copy at its default value.',
        steps: [apply('fcopy:novalue', { ops: [{ type: 'forms.copy', fields: [field(4)], values: false }] })],
      },
      {
        name: 'copy-a-radio-group',
        covers: 'A radio group copies with both its widgets, moved by the offset.',
        steps: [
          apply('fcopy:radio', { ops: [{ type: 'forms.copy', fields: [field(9)], offset: { x: 0, y: 100 } }] }),
          { read: 'forms.list' },
        ],
      },
      {
        name: 'copy-one-widget-of-a-group',
        covers: 'widgets names the widgets to copy when not every one; without values the copy has its default.',
        steps: [apply('fcopy:widget', { ops: [{ type: 'forms.copy', fields: [field(9)], widgets: [annot(7)], values: false }] })],
      },
      {
        name: 'a-value-on-a-widget-left-out-is-refused',
        covers:
          'The group is on Blue, the export value of widget 8; copying only widget 7 with its value names a value no copied widget has, and the change is refused with InvalidArg.',
        steps: [apply('fcopy:widget-value', { ops: [{ type: 'forms.copy', fields: [field(9)], widgets: [annot(7)] }] })],
      },
      {
        name: 'the-copies-take-the-numbers-given',
        covers: 'objectNumbers: for each field, its own number, then one for each widget copied.',
        steps: [apply('fcopy:numbers', { ops: [{ type: 'forms.copy', fields: [field(9)], objectNumbers: [50, 51, 52] }] })],
      },
      {
        name: 'an-unknown-field-is-refused',
        covers: 'A field ref no field has refuses the change.',
        steps: [apply('fcopy:unknown', { ops: [{ type: 'forms.copy', fields: [field(9999)] }] })],
      },
      {
        name: 'an-undo-takes-the-copies-away',
        covers: 'The undo deletes the copied fields and their widgets; the form reads as it did.',
        steps: [
          apply('fcopy:do', { ops: [{ type: 'forms.copy', fields: [field(4), field(9)] }] }),
          apply('fcopy:undo', { undoOf: 'fcopy:do' }),
          { read: 'forms.list' },
        ],
      },
    ],
  },
];

// --- Recording at the worker boundary ---------------------------------------

/** Every name a copy makes up is a UUIDv7. */
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

/**
 * A write stamps `modifiedAt` (and a copy its dates) from the clock. Every
 * date the fixtures carry is from 2024, so a timestamp after 2025 is the
 * clock's, and is recorded as `"<now>"`.
 */
const TIMESTAMP = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})$/;
const CLOCK_AFTER = Date.parse('2025-01-01T00:00:00Z');

function plain(value: unknown): unknown {
  if (value instanceof ArrayBuffer) return Array.from(new Uint8Array(value));
  if (ArrayBuffer.isView(value)) {
    return Array.from(new Uint8Array(value.buffer, value.byteOffset, value.byteLength));
  }
  if (Array.isArray(value)) return value.map(plain);
  if (typeof value === 'string' && UUID.test(value)) return '<uuid>';
  if (typeof value === 'string' && TIMESTAMP.test(value) && Date.parse(value) >= CLOCK_AFTER)
    return '<now>';
  if (value === null || typeof value !== 'object') return value;
  return Object.fromEntries(
    Object.entries(value)
      .filter(([, entry]) => entry !== undefined)
      .map(([key, entry]) => {
        if (key === 'jobId') return [key, '<job>'];
        if (key === 'docId') return [key, '<doc>'];
        return [key, plain(entry)];
      }),
  );
}

interface Exchange {
  request: unknown;
  answer?: unknown;
}

function recordingEngine(): { engine: LocalEngine; take: () => Exchange[] } {
  let exchanges: Exchange[] = [];
  const byJob = new Map<unknown, Exchange>();
  const inner = new LazyTransport(() =>
    createInlineTransport({
      prefer: 'wasm',
      wasm: {
        print: (message: string) => process.stderr.write(`${message}\n`),
        printErr: (message: string) => process.stderr.write(`${message}\n`),
      },
    }),
  );
  const transport: Transport = {
    send(pack) {
      const request = pack.payload as WorkerRequest & { jobId?: unknown };
      const exchange: Exchange = { request: plain(request) };
      exchanges.push(exchange);
      byJob.set(request.jobId, exchange);
      inner.send(pack);
    },
    start: () => inner.start(),
    onMessage: (handler) =>
      inner.onMessage((message) => {
        const response = ((message as { payload?: unknown }).payload ?? message) as {
          kind: string;
          jobId: unknown;
          result?: unknown;
          error?: { code?: unknown };
        };
        const exchange = byJob.get(response.jobId);
        if (exchange && exchange.answer === undefined) {
          exchange.answer =
            response.kind === 'resolve'
              ? { resolve: plain(response.result) }
              : { reject: String(response.error?.code) };
        }
        handler(message);
      }),
    terminate: () => inner.terminate(),
  };
  return {
    engine: LocalEngine.fromTransport({ transport }),
    take: () => {
      const taken = exchanges;
      exchanges = [];
      return taken;
    },
  };
}

async function runCase(group: Group, entry: Case): Promise<unknown> {
  const bytes = new Uint8Array(await readFile(join(repoRoot, group.fixture)));
  const { engine, take } = recordingEngine();
  try {
    const doc = await engine.open(
      { kind: 'bytes', id: `changes:${entry.name}`, bytes },
      { scope: ['*'] },
    );
    take();
    const steps: unknown[] = [];
    try {
      for (const step of entry.steps) {
        try {
          if ('apply' in step) await doc.apply(step.apply, { opId: step.opId });
          else if (step.read === 'pages.list') await doc.pages.list();
          else if (step.read === 'annotations.list')
            await doc.page(toPageRef(step.page)).annotations.list();
          else await doc.forms.list();
        } catch (error) {
          if ((error as { code?: unknown })?.code === undefined) throw error;
        }
        steps.push({ ...step, exchanges: take() });
      }
    } finally {
      await doc.close();
    }
    return { name: entry.name, covers: entry.covers, steps };
  } finally {
    await engine.destroy();
  }
}

// --- Change kinds -----------------------------------------------------------

/** The sides `changeKindsUnknownTo` is asked about: none said, older, this one, newer. */
const VERSIONS = [null, '2.9', '3.0', '3.1', '3.2', '3.3', '4.0'] as const;

function kindsVector(): unknown {
  const kinds = [...CHANGE_KINDS];
  const asked = [...kinds, 'pages.someKindFromANewerServer'];
  return {
    schemaVersion: SCHEMA_VERSION,
    kind: 'wire.changeKinds',
    apiVersion: API_VERSION,
    since: Object.fromEntries(kinds.map((kind) => [kind, changeKindSince(kind as ChangeKind)])),
    rows: CHANGE_KIND_SINCE,
    unknownTo: VERSIONS.map((version) => ({
      version,
      unknown: changeKindsUnknownTo(version, asked),
    })),
    lacks: VERSIONS.flatMap((version) =>
      [null, '3.0', '3.1', '3.2', '3.3'].map((since) => ({
        version,
        since,
        lacks: lacksWhatArrivedIn(version, since),
      })),
    ),
  };
}

// --- Output -----------------------------------------------------------------

function serialize(group: Group, cases: readonly unknown[]): string {
  const head = JSON.stringify(
    { schemaVersion: SCHEMA_VERSION, kind: group.kind, fixture: group.fixture },
    null,
    2,
  ).replace(/\n}$/, ',');
  const body = cases.map((entry) => `  ${JSON.stringify(entry)}`).join(',\n');
  return `${head}\n  "cases": [\n${body}\n  ]\n}\n`;
}

async function emit(relative: string, text: string, check: boolean): Promise<boolean> {
  const absolute = join(vectorsRoot, relative);
  let current: string | undefined;
  try {
    current = await readFile(absolute, 'utf8');
  } catch {
    current = undefined;
  }
  if (current === text) {
    process.stdout.write(`  ok      changes/${relative}\n`);
    return false;
  }
  if (check) {
    process.stdout.write(`  DIFFERS changes/${relative}${current === undefined ? ' (missing)' : ''}\n`);
    return true;
  }
  await mkdir(dirname(absolute), { recursive: true });
  await writeFile(absolute, text);
  process.stdout.write(`  written changes/${relative}\n`);
  return false;
}

async function main(): Promise<void> {
  const check = process.argv.includes('--check');
  let differs = false;
  for (const group of GROUPS) {
    const names = new Set<string>();
    for (const entry of group.cases) {
      if (names.has(entry.name)) throw new Error(`${group.file}: duplicate case ${entry.name}`);
      names.add(entry.name);
    }
    const cases: unknown[] = [];
    for (const entry of group.cases) cases.push(await runCase(group, entry));
    differs = (await emit(group.file, serialize(group, cases), check)) || differs;
  }
  differs = (await emit('kinds.json', `${JSON.stringify(kindsVector(), null, 2)}\n`, check)) || differs;
  if (differs) {
    process.stderr.write(
      '\nChange vectors differ. Regenerate with:\n' +
        '  pnpm --filter @embedpdf/tooling-engine-conformance run changes\n' +
        'and commit the diff.\n',
    );
    process.exitCode = 1;
  }
}

main().catch((error: unknown) => {
  process.stderr.write(
    `generate-change-vectors: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
