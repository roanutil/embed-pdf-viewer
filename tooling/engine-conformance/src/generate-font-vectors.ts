/**
 * Emits `vectors/fonts/wire.json`: the five font kinds upstream added
 * (`fonts.list`, `fonts.program`, `document.fonts.list`,
 * `document.fonts.resolve`, `document.fonts.program`), each answered by the
 * TypeScript `WorkerHost` on the wire.
 *
 *   pnpm --filter @embedpdf/tooling-engine-conformance run fonts
 *   pnpm --filter @embedpdf/tooling-engine-conformance run fonts:check
 *
 * `crates/epdf-engine/tests/fonts_wire_corpus.rs` replays it.
 *
 * Each case is a run of requests against one host, as a session sends them.
 * A step records the request as it went out, without its `jobId` and
 * `effect` (the Rust runner fills both), and the bytes it carried as fixture
 * paths in `payloads`, which the TypeScript sends inline (`bytes` on an open,
 * `data` on a register). A later step's request is written out with what an
 * earlier answer said (a program's hash, say), so the vector is a plain list.
 *
 * The answer is the handler's result without its `tag`, or the error's code.
 * Bytes in a result are recorded as their length and SHA-256, so the file
 * stays small and the Rust side checks the payload it answers. The steps
 * before the one a case is about (opens, registers) record only that they
 * succeeded.
 */
import { createHash } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import type { WorkerRequest, WorkerResponse } from '../../../packages/engine/main/src/index';
import { createInlineTransport } from '../../../packages/engine/main/src/transport/InlineTransport';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(packageRoot, '../..');
const vectorsRoot = join(packageRoot, 'vectors', 'fonts');

/** Bump when the envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 1;

const ROBOTO = 'packages/engine/main/test/fixtures/Roboto-Regular.ttf';
const EMBEDDED = 'tooling/engine-conformance/fixtures/embedded_orientations.pdf';
const NO_FONTS = 'tooling/engine-conformance/fixtures/annotation_kinds.pdf';
const DOC_ID = 'doc-1';

/** The field each kind carries its bytes in, on the TypeScript wire. */
const BYTES_FIELD: Record<string, string> = { 'open.fatMem': 'bytes', 'fonts.register': 'data' };

const EFFECTS: Record<string, string> = {
  'open.fatMem': 'open',
  'fonts.register': 'runtimeWrite',
  'fonts.addFallback': 'runtimeWrite',
  'fonts.clearFallbacks': 'runtimeWrite',
  'fonts.clear': 'runtimeWrite',
  'fonts.list': 'read',
  'fonts.program': 'read',
  'document.fonts.list': 'read',
  'document.fonts.resolve': 'read',
  'document.fonts.program': 'read',
};

type Json = null | boolean | number | string | Json[] | { [key: string]: Json };

interface Step {
  readonly request: { readonly kind: string } & Record<string, Json>;
  readonly payloads?: readonly string[];
  /** Setup: only its success is recorded. */
  readonly setup?: boolean;
}

interface FontCase {
  readonly name: string;
  readonly covers: string;
  /** The steps, each made from the answers before it. */
  readonly steps: (answers: readonly Json[]) => Step | null;
}

const open = (fixture: string): Step => ({
  request: { kind: 'open.fatMem', docId: DOC_ID, password: null },
  payloads: [fixture],
  setup: true,
});
const register = (fontKey: string): Step => ({
  request: { kind: 'fonts.register', fontKey, familyName: '', weight: 0, italic: -1 },
  payloads: [ROBOTO],
  setup: true,
});
const fallback = (fontKey: string, tier?: number): Step => ({
  request: { kind: 'fonts.addFallback', fontKey, ...(tier === undefined ? {} : { tier }) },
  setup: true,
});

/** A case written as a fixed list, with a step made from earlier answers. */
const sequence =
  (...steps: ((answers: readonly Json[]) => Step)[]) =>
  (answers: readonly Json[]): Step | null =>
    steps[answers.length]?.(answers) ?? null;
const fixed = (step: Step) => () => step;

/** The answer of the step at `index` as an object, to read a field from. */
const at = (answers: readonly Json[], index: number): Record<string, any> =>
  (answers[index] as { result?: Record<string, any> }).result ?? {};

const CASES: readonly FontCase[] = [
  {
    name: 'engine-fonts',
    covers:
      'fonts.list in registration order with each font program, the fallbacks by tier then by the order added, one program registered twice, and fonts.program by hash, served and unknown.',
    steps: sequence(
      fixed(register('zeta')),
      fixed(register('alpha')),
      fixed(fallback('alpha', 1)),
      fixed(fallback('zeta')),
      fixed({ request: { kind: 'fonts.list' } }),
      (answers) => ({
        request: { kind: 'fonts.program', sha256: at(answers, 4).engineFonts[0].sha256 },
      }),
      fixed({ request: { kind: 'fonts.program', sha256: 'ab' } }),
    ),
  },
  {
    name: 'an-empty-engine',
    covers: 'No font registered: fonts.list names none, and not the last-resort font.',
    steps: sequence(fixed({ request: { kind: 'fonts.list' } })),
  },
  {
    name: 'clear-forgets',
    covers: 'fonts.clear forgets the listed fonts and the fallbacks.',
    steps: sequence(
      fixed(register('roboto')),
      fixed(fallback('roboto')),
      fixed({ request: { kind: 'fonts.clear' }, setup: true }),
      fixed({ request: { kind: 'fonts.list' } }),
    ),
  },
  {
    name: 'document-fonts',
    covers:
      "With no engine font yet: document.fonts.list on a document with one embedded program, document.fonts.program's info and data read from that document program by its display hash, an unknown hash, and a run naming the document's program. Then, with Roboto registered: the list beside the engine font, and document.fonts.program answered from the engine font, which wins over a document program with the same hash.",
    steps: sequence(
      fixed(open(EMBEDDED)),
      fixed({ request: { kind: 'document.fonts.list', docId: DOC_ID } }),
      (answers) => ({
        request: {
          kind: 'document.fonts.program',
          docId: DOC_ID,
          displayHash: at(answers, 1).facts.faces[0].programs[0].displaySha256,
          part: 'info',
        },
      }),
      (answers) => ({
        request: {
          kind: 'document.fonts.program',
          docId: DOC_ID,
          displayHash: at(answers, 1).facts.faces[0].programs[0].displaySha256,
          part: 'data',
        },
      }),
      fixed({
        request: {
          kind: 'document.fonts.program',
          docId: DOC_ID,
          displayHash: 'ab',
          part: 'info',
        },
      }),
      fixed({
        request: {
          kind: 'document.fonts.resolve',
          docId: DOC_ID,
          runs: [{ text: 'Hi', fontFamily: 'Roboto-Regular' }],
        },
      }),
      fixed(register('roboto')),
      fixed({ request: { kind: 'document.fonts.list', docId: DOC_ID } }),
      (answers) => ({
        request: {
          kind: 'document.fonts.program',
          docId: DOC_ID,
          displayHash: at(answers, 7).facts.engineFonts[0].sha256,
          part: 'info',
        },
      }),
    ),
  },
  {
    name: 'resolve-runs',
    covers:
      'document.fonts.resolve on a document with no fonts: a standard font, an engine font by key and by family, a character no font draws, and the facts the runs were read against.',
    steps: sequence(
      fixed(open(NO_FONTS)),
      fixed(register('roboto')),
      fixed(fallback('roboto')),
      fixed({
        request: {
          kind: 'document.fonts.resolve',
          docId: DOC_ID,
          runs: [
            { text: 'Hi', fontFamily: 'helvetica-bold' },
            { text: 'Hi \u{10FFFD}', fontFamily: 'roboto' },
            { text: 'Hi', family: 'Roboto', weight: 400, italic: false },
          ],
        },
      }),
    ),
  },
  {
    name: 'no-document',
    covers: 'Each document font kind names a document that is not open.',
    steps: sequence(
      fixed({ request: { kind: 'document.fonts.list', docId: 'missing' } }),
      fixed({ request: { kind: 'document.fonts.resolve', docId: 'missing', runs: [] } }),
      fixed({
        request: {
          kind: 'document.fonts.program',
          docId: 'missing',
          displayHash: 'ab',
          part: 'info',
        },
      }),
    ),
  },
];

function sha256(bytes: Uint8Array): string {
  return createHash('sha256').update(bytes).digest('hex');
}

/** A result as the vector records it: no `tag`, bytes as their length and hash. */
function recorded(value: unknown): Json {
  if (value instanceof ArrayBuffer) value = new Uint8Array(value);
  if (ArrayBuffer.isView(value)) {
    const bytes = new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
    return { $bytes: { length: bytes.byteLength, sha256: sha256(bytes) } };
  }
  if (Array.isArray(value)) return value.map(recorded);
  if (value === null || typeof value !== 'object') return (value ?? null) as Json;
  const out: Record<string, Json> = {};
  for (const [key, entry] of Object.entries(value)) {
    if (key === 'tag' || entry === undefined) continue;
    out[key] = recorded(entry);
  }
  return out;
}

async function runCase(entry: FontCase): Promise<unknown> {
  const transport = await createInlineTransport({
    prefer: 'wasm',
    wasm: {
      print: (message: string) => process.stderr.write(`${message}\n`),
      printErr: (message: string) => process.stderr.write(`${message}\n`),
    },
  } as Parameters<typeof createInlineTransport>[0]);
  const waiting = new Map<number, (response: WorkerResponse) => void>();
  transport.onMessage((response) => {
    const jobId = (response as { jobId?: number }).jobId;
    if (jobId !== undefined) waiting.get(jobId)?.(response);
  });
  let jobId = 0;
  const send = async (step: Step): Promise<WorkerResponse> => {
    const request: Record<string, unknown> = {
      ...step.request,
      jobId: ++jobId,
      effect: EFFECTS[step.request.kind],
    };
    for (const fixture of step.payloads ?? []) {
      const bytes = new Uint8Array(await readFile(join(repoRoot, fixture)));
      request[BYTES_FIELD[step.request.kind]!] = bytes.buffer;
    }
    return new Promise((done) => {
      waiting.set(request.jobId as number, done);
      transport.send({ payload: request as unknown as WorkerRequest, transfer: [] });
    });
  };

  const steps: unknown[] = [];
  const answers: Json[] = [];
  try {
    for (let step = entry.steps(answers); step; step = entry.steps(answers)) {
      const response = await send(step);
      const answer: Json =
        response.kind === 'resolve'
          ? step.setup
            ? { ok: true }
            : { result: recorded(response.result) }
          : { error: { code: (response.error as { code?: string }).code ?? null } };
      if (step.setup && response.kind !== 'resolve') {
        throw new Error(`${entry.name}: ${step.request.kind} failed: ${JSON.stringify(response)}`);
      }
      // Later steps read the full answer, not the recorded one.
      answers.push(
        response.kind === 'resolve' ? { result: response.result as unknown as Json } : answer,
      );
      steps.push({
        request: step.request,
        ...(step.payloads ? { payloads: step.payloads } : {}),
        answer,
      });
    }
  } finally {
    transport.terminate();
  }
  return { name: entry.name, covers: entry.covers, steps };
}

function serialize(cases: readonly unknown[]): string {
  const head = JSON.stringify(
    { schemaVersion: SCHEMA_VERSION, kind: 'fonts.wire' },
    null,
    2,
  ).replace(/\n}$/, ',');
  const body = cases.map((entry) => `  ${JSON.stringify(entry)}`).join(',\n');
  return `${head}\n  "cases": [\n${body}\n  ]\n}\n`;
}

async function main(): Promise<void> {
  const check = process.argv.includes('--check');
  const cases: unknown[] = [];
  for (const entry of CASES) cases.push(await runCase(entry));
  const text = serialize(cases);
  const absolute = join(vectorsRoot, 'wire.json');

  let current: string | undefined;
  try {
    current = await readFile(absolute, 'utf8');
  } catch {
    current = undefined;
  }
  if (current === text) {
    process.stdout.write('  ok      fonts/wire.json\n');
    return;
  }
  if (check) {
    process.stdout.write(`  DIFFERS fonts/wire.json${current === undefined ? ' (missing)' : ''}\n`);
    process.stderr.write(
      '\nFont vectors differ. Regenerate with:\n' +
        '  pnpm --filter @embedpdf/tooling-engine-conformance run fonts\n' +
        'and commit the diff.\n',
    );
    process.exitCode = 1;
    return;
  }
  await mkdir(dirname(absolute), { recursive: true });
  await writeFile(absolute, text);
  process.stdout.write('  written fonts/wire.json\n');
}

main().catch((error: unknown) => {
  process.stderr.write(
    `generate-font-vectors: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
