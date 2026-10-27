/**
 * `packages/core/acrojs`: the host bridge the actions and form plugins run
 * scripts through. Replayed by `crates/plugins/acrojs/tests/conformance.rs`.
 *
 * `prelude.ts` does not port: it is JavaScript that runs inside the sandbox.
 * This module writes its `PRELUDE_SOURCE` to `crates/plugins/acrojs/prelude.js`
 * (or with `--check` fails when that file is stale), and the Rust crate ships
 * the text with `include_str!`.
 *
 * The script sandbox is a capability the host supplies, not a port. Vectors
 * drive the host with a scripted one, `scriptedSandboxFactory`, which the
 * actions suite shares: every factory call, `boot`, `run` and `dispose` is
 * logged as a callback with its canonical input, and answered from the
 * vector's queue. The Rust twin is `crates/plugins/acrojs/src/harness.rs`.
 */
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createContext, runInContext } from 'node:vm';
import ts from 'typescript';

import { createCapabilityToken, definePlugin, type AnyPlugin } from '@embedpdf/core';
import type { DocumentHandle, FormSnapshot, Identity } from '@embedpdf/engine-core/runtime';
import { text, listbox, checkbox, radio, snapshot, placed } from './form-current-fixtures';

import {
  javaScriptProgramFromActionTree,
  javaScriptSourcesFromActionTree,
} from '../../../../packages/core/acrojs/src/actions';
import {
  cssToScriptColor,
  isScriptColorArray,
  scriptColorToCss,
  scriptColorToRgb,
} from '../../../../packages/core/acrojs/src/color';
import {
  createScriptHost,
  resolveScriptIdentity,
  seedFrom,
  type ScriptWorldInput,
} from '../../../../packages/core/acrojs/src/host';
import { scriptFieldsFromSnapshot } from '../../../../packages/core/acrojs/src/input';
import {
  ANNOT_WRITABLE_KEYS,
  DEFAULT_SCRIPT_BUDGET,
  DEFAULT_SCRIPT_SECURITY_POLICY,
  SCRIPT_EVENT_MATRIX,
  type ScriptBudget,
  type ScriptDocumentInput,
  type ScriptIdentity,
  type ScriptOutput,
  type ScriptSandbox,
  type ScriptSandboxFactory,
} from '../../../../packages/core/acrojs/src/types';
import { makeError, type ErrorSpec } from '../fake-engine';
import { registerPlugin, type PluginEnv, type ScenarioCase, type Step } from '../scenario';
import type { FunctionCase, Suite } from '../vectors';

/** Why these cases record no state; see `ScenarioCase` in `../scenario`. */
const UNSNAPSHOTTED = {
  snapshot: 'none',
  reason: "The script host runs under a probe plugin with no slice. These cases compare each transaction's outcome and settlement and the sandbox calls the probe logs as callbacks.",
} as const;

// ── the prelude, shipped as data ──────────────────────────────────────────────

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, '../../../..');
const preludePath = join(repoRoot, 'crates/plugins/acrojs/prelude.js');

/**
 * `PRELUDE_SOURCE` is `installAcroJs.toString()`, so its text is whatever the
 * loader made of `prelude.ts`. tsx's esbuild wraps every arrow in a `__name`
 * helper that exists only in the loading module, which makes that text dead in
 * a sandbox realm. The source is type-stripped here with the TypeScript
 * compiler instead (formatting kept, nothing injected) and evaluated, and the
 * result is checked to install `__acrojsBoot` and `__acrojsRun` in a fresh
 * realm. The published build prints a behaviourally identical text
 * (`packages/core/acrojs/test/built-parity.test.ts` pins that).
 */
async function preludeSource(): Promise<string> {
  const source = readFileSync(join(repoRoot, 'packages/core/acrojs/src/prelude.ts'), 'utf8');
  const { outputText } = ts.transpileModule(source, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
  });
  const module = (await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`)) as {
    PRELUDE_SOURCE: string;
  };
  const realm = createContext({});
  runInContext(module.PRELUDE_SOURCE, realm);
  const installed = realm as { __acrojsBoot?: unknown; __acrojsRun?: unknown };
  if (typeof installed.__acrojsBoot !== 'function' || typeof installed.__acrojsRun !== 'function') {
    throw new Error('[acrojs] PRELUDE_SOURCE does not install the VM entry points');
  }
  return module.PRELUDE_SOURCE;
}

const PRELUDE_SOURCE = await preludeSource();
{
  const current = (() => {
    try {
      return readFileSync(preludePath, 'utf8');
    } catch {
      return null;
    }
  })();
  if (current !== PRELUDE_SOURCE) {
    if (process.argv.includes('--check')) {
      console.error('stale crates/plugins/acrojs/prelude.js (run `pnpm --filter @embedpdf/tooling-plugin-conformance run vectors`)');
      process.exitCode = 1;
    } else {
      writeFileSync(preludePath, PRELUDE_SOURCE);
      console.log('wrote crates/plugins/acrojs/prelude.js');
    }
  }
}

// ── scripted callbacks and the scripted sandbox (shared with actions.ts) ─────

/** One scripted answer, as `scenario.ts`'s `$fn` answers: checked in the order
 *  `throws`, `defer`, `rejects`, `resolves`, `returns`; `seq` gives one per call
 *  and repeats the last. Values are passed through undecoded. */
export interface AnswerSpec {
  readonly returns?: unknown;
  readonly throws?: ErrorSpec;
  readonly resolves?: unknown;
  readonly rejects?: ErrorSpec;
  readonly defer?: string;
  readonly seq?: readonly AnswerSpec[];
}

export function scriptedCallback(name: string, spec: AnswerSpec, env: PluginEnv): (...args: unknown[]) => unknown {
  let calls = 0;
  return (...args: unknown[]) => {
    env.log(name, args);
    const answer = spec.seq && spec.seq.length > 0 ? spec.seq[Math.min(calls, spec.seq.length - 1)] : spec;
    calls += 1;
    if (answer.throws) throw makeError(answer.throws);
    if (answer.defer !== undefined) {
      const label = answer.defer;
      return new Promise((resolve, reject) => env.defer(label, resolve, reject));
    }
    if (answer.rejects) return Promise.reject(makeError(answer.rejects));
    if ('resolves' in answer) return Promise.resolve(answer.resolves);
    return answer.returns;
  };
}

/** What one `boot` or `run` call answers. */
export interface SandboxAnswerSpec {
  /** Merged over an empty output (`rc: true`, no effects, no diagnostics). */
  readonly output?: Partial<ScriptOutput>;
  /** Throw from the call instead of answering. */
  readonly throws?: ErrorSpec;
  /** Report `disposed` from now on: a resource fault. */
  readonly dispose?: boolean;
}

/** One realm the factory builds. `boot` and `run` answer per call, the last
 *  repeating; absent, every call answers the empty output. */
export interface SandboxBuildSpec {
  readonly rejects?: ErrorSpec;
  readonly defer?: string;
  readonly boot?: readonly SandboxAnswerSpec[];
  readonly run?: readonly SandboxAnswerSpec[];
}

/** The factory: one build per call, the last repeating. */
export interface SandboxSpec {
  readonly builds?: readonly SandboxBuildSpec[];
}

const emptyOutput = (over: Partial<ScriptOutput> = {}): ScriptOutput => ({
  event: { rc: true, value: null, change: '', selStart: 0, selEnd: 0 },
  formEffects: [],
  annotEffects: [],
  uiEffects: [],
  diagnostics: [],
  ...over,
});

export function scriptedSandboxFactory(spec: SandboxSpec, env: PluginEnv): ScriptSandboxFactory {
  let built = 0;
  return () => {
    const id = built;
    built += 1;
    env.log('sandbox.factory', [id]);
    const builds = spec.builds && spec.builds.length > 0 ? spec.builds : [{}];
    const build: SandboxBuildSpec = builds[Math.min(id, builds.length - 1)];
    let disposed = false;
    let boots = 0;
    let runs = 0;
    const respond = (list: readonly SandboxAnswerSpec[] | undefined, index: number): ScriptOutput => {
      const answer: SandboxAnswerSpec = list && list.length > 0 ? list[Math.min(index, list.length - 1)] : {};
      if (answer.dispose) disposed = true;
      if (answer.throws) throw makeError(answer.throws);
      return emptyOutput(answer.output);
    };
    const sandbox: ScriptSandbox = {
      get disposed() {
        return disposed;
      },
      boot(sources, input, budget) {
        env.log('sandbox.boot', [id, sources, input, budget]);
        boots += 1;
        return respond(build.boot, boots - 1);
      },
      run(source, input, budget) {
        env.log('sandbox.run', [id, source, input, budget]);
        runs += 1;
        return respond(build.run, runs - 1);
      },
      dispose() {
        env.log('sandbox.dispose', [id]);
        disposed = true;
      },
    };
    if (build.rejects) return Promise.reject(makeError(build.rejects));
    if (build.defer !== undefined) {
      const label = build.defer;
      return new Promise<ScriptSandbox>((resolve, reject) => env.defer(label, () => resolve(sandbox), reject));
    }
    return Promise.resolve(sandbox);
  };
}

// ── the script-host driver: createScriptHost behind a capability ─────────────

interface ScriptHostDriverConfig {
  readonly sandbox?: SandboxSpec;
  /** `bootSources()`; default returns `["boot()"]`. */
  readonly bootSources?: AnswerSpec;
  readonly budget?: ScriptBudget;
  readonly document?: ScriptDocumentInput;
  readonly identity?: ScriptIdentity;
}

type DriverOp =
  | { readonly op: 'boot'; readonly world: ScriptWorldInput; readonly budget?: ScriptBudget }
  | { readonly op: 'run'; readonly program: string; readonly world: ScriptWorldInput; readonly budget?: ScriptBudget }
  | { readonly op: 'wait'; readonly label: string }
  | { readonly op: 'throw'; readonly message: string };

interface ScriptHostDriver {
  transaction(ops: readonly DriverOp[]): Promise<unknown[]>;
  dispose(): void;
}

const ScriptHostToken = createCapabilityToken<ScriptHostDriver>('script-host');

function scriptHostDriver(config: ScriptHostDriverConfig = {}, env: PluginEnv): AnyPlugin {
  return definePlugin({
    id: 'script-host',
    token: ScriptHostToken,
    create: (ctx) => {
      const host = createScriptHost({
        sandboxFactory: scriptedSandboxFactory(config.sandbox ?? {}, env),
        document: () => config.document ?? { id: 'doc-1', fileName: 'x.pdf', pageCount: 1, pageNumber: 0 },
        identity: () => config.identity ?? { name: '', loginName: '', corporation: '', email: '' },
        environment: (sequence) => ({ nowMs: 1000, utcOffsetMinutes: 0, randomSeed: seedFrom('doc-1', sequence) }),
        bootSources: scriptedCallback('bootSources', config.bootSources ?? { returns: ['boot()'] }, env) as () => Promise<
          string[]
        >,
        ...(config.budget ? { budget: config.budget } : {}),
      });
      ctx.cleanup(() => host.dispose());
      const api: ScriptHostDriver = {
        transaction: (ops) =>
          host.transaction(async (txn) => {
            const out: unknown[] = [];
            for (const op of ops) {
              if (op.op === 'boot') out.push(await txn.boot(op.world, op.budget));
              else if (op.op === 'run') out.push(await txn.run(op.program, op.world, op.budget));
              else if (op.op === 'wait') {
                const label = op.label;
                await new Promise((resolve, reject) => env.defer(label, resolve, reject));
                out.push(label);
              } else throw new Error(op.message);
            }
            return out;
          }),
        dispose: () => host.dispose(),
      };
      return { api };
    },
  });
}

registerPlugin('script-host', {
  factory: (config, env) => scriptHostDriver(config as ScriptHostDriverConfig | undefined, env),
  tokens: [ScriptHostToken],
});

// ── function cases ────────────────────────────────────────────────────────────

const node = (type: string, extra: Record<string, unknown> = {}, next: unknown[] = []) => ({
  type,
  subtype: type,
  ...extra,
  next,
});
const actionTree = (root: unknown, incomplete = false) => ({ root, incomplete, warningFlags: 0, warnings: [] });
const claims = (over: Partial<Identity> = {}): Identity => ({
  userId: 'u-1',
  displayName: 'Ada',
  organization: 'g-1',
  ...over,
});
// A typed option may carry engine extension data; the script view copies only label/value.
const firstOption = { label: 'A', value: 'a', selected: true, extra: 1 };
const fieldsSnapshot: FormSnapshot = snapshot([
  text(5, 'text', 'hi'),
  listbox(6, 'list', ['a', 'b'], {
    defaultValueEntry: { kind: 'unsupported' },
    options: [firstOption, { label: 'B', value: 'b', selected: true }],
    readOnly: true, required: true,
  }),
  checkbox(7, 'box', false, { defaultValueEntry: { kind: 'scalar', value: 'Yes' } }),
  radio(0, 'radio', [{ ...placed(108), onState: 'One', exportValue: 'One', checked: true }], {
    valueEntry: { kind: 'scalar', value: 'One' },
  }),
  checkbox(9, 'toggle-unsupported', false, { valueEntry: { kind: 'unsupported' } }),
  text(10, 'unsupported', '', { valueEntry: { kind: 'unsupported' } }),
]);

const functionCases: FunctionCase[] = [
  // color.ts
  { name: 'rgb-transparent', fn: 'scriptColorToRgb', args: [['T']] },
  { name: 'rgb-gray', fn: 'scriptColorToRgb', args: [['G', 0.25]] },
  { name: 'rgb-rgb', fn: 'scriptColorToRgb', args: [['RGB', 0.1, 0.2, 0.3]] },
  { name: 'rgb-cmyk', fn: 'scriptColorToRgb', args: [['CMYK', 0.1, 0.2, 0.3, 0.4]] },
  { name: 'rgb-cmyk-saturates', covers: 'channel + k past 1 clamps to 0.', fn: 'scriptColorToRgb', args: [['CMYK', 0.9, 0.2, 0.7, 0.5]] },
  { name: 'css-transparent', fn: 'scriptColorToCss', args: [['T']] },
  { name: 'css-gray', fn: 'scriptColorToCss', args: [['G', 0.5]] },
  { name: 'css-rgb-rounds-half-up', covers: 'Math.round: 0.5 * 255 = 127.5 rounds to 128.', fn: 'scriptColorToCss', args: [['RGB', 0.5, 1, 0]] },
  { name: 'css-clamps', fn: 'scriptColorToCss', args: [['RGB', -2, 3, 0.002]] },
  { name: 'css-cmyk', fn: 'scriptColorToCss', args: [['CMYK', 0, 1, 0.5, 0.25]] },
  { name: 'css-infinity-clamps', fn: 'scriptColorToCss', args: [['RGB', { $number: 'Infinity' }, { $number: '-Infinity' }, 0]] },
  { name: 'css-nan-propagates', covers: 'Math.max and Math.min propagate NaN, and NaN.toString(16) is "NaN".', fn: 'scriptColorToCss', args: [['G', { $number: 'NaN' }]] },
  { name: 'from-css-null', fn: 'cssToScriptColor', args: [null] },
  { name: 'from-css-undefined', fn: 'cssToScriptColor', args: [{ $undefined: true }] },
  { name: 'from-css-empty', fn: 'cssToScriptColor', args: [''] },
  { name: 'from-css-short', fn: 'cssToScriptColor', args: ['#abc'] },
  { name: 'from-css-short-no-hash', fn: 'cssToScriptColor', args: ['F0a'] },
  { name: 'from-css-long', fn: 'cssToScriptColor', args: ['#FF8000'] },
  { name: 'from-css-trims', fn: 'cssToScriptColor', args: [' \t#00ff7f\n'] },
  { name: 'from-css-trims-js-whitespace', covers: 'String.prototype.trim removes U+FEFF and U+00A0.', fn: 'cssToScriptColor', args: ['﻿#123456 '] },
  { name: 'from-css-keeps-nel', covers: 'U+0085 is not JavaScript whitespace, so it stays and the value is invalid.', fn: 'cssToScriptColor', args: ['\u0085#123456'] },
  { name: 'from-css-one-hash-only', fn: 'cssToScriptColor', args: ['##abc'] },
  { name: 'from-css-bad-digit', fn: 'cssToScriptColor', args: ['#ggg'] },
  { name: 'from-css-bad-length', fn: 'cssToScriptColor', args: ['#12345'] },
  { name: 'from-css-three-non-ascii', fn: 'cssToScriptColor', args: ['éab'] },
  { name: 'from-css-astral', covers: 'Length is counted in UTF-16 units: "a" plus one astral character is three.', fn: 'cssToScriptColor', args: ['a😀'] },
  { name: 'is-color-not-array', fn: 'isScriptColorArray', args: ['RGB'] },
  { name: 'is-color-empty', fn: 'isScriptColorArray', args: [[]] },
  { name: 'is-color-t', fn: 'isScriptColorArray', args: [['T']] },
  { name: 'is-color-t-extra', fn: 'isScriptColorArray', args: [['T', 1]] },
  { name: 'is-color-g', fn: 'isScriptColorArray', args: [['G', 0.5]] },
  { name: 'is-color-g-string', fn: 'isScriptColorArray', args: [['G', '0.5']] },
  { name: 'is-color-g-nan', fn: 'isScriptColorArray', args: [['G', { $number: 'NaN' }]] },
  { name: 'is-color-g-infinity', fn: 'isScriptColorArray', args: [['G', { $number: 'Infinity' }]] },
  { name: 'is-color-rgb', fn: 'isScriptColorArray', args: [['RGB', 1, 2, 3]] },
  { name: 'is-color-rgb-short', fn: 'isScriptColorArray', args: [['RGB', 1, 2]] },
  { name: 'is-color-cmyk', fn: 'isScriptColorArray', args: [['CMYK', 0, 0, 0, 1]] },
  { name: 'is-color-unknown-space', fn: 'isScriptColorArray', args: [['X']] },
  { name: 'is-color-lowercase-space', fn: 'isScriptColorArray', args: [['rgb', 1, 2, 3]] },
  { name: 'is-color-space-coerced', covers: 'String(["RGB"]) is "RGB": the space is coerced before the lookup.', fn: 'isScriptColorArray', args: [[['RGB'], 1, 2, 3]] },
  { name: 'is-color-space-null', fn: 'isScriptColorArray', args: [[null]] },
  { name: 'is-color-space-number', fn: 'isScriptColorArray', args: [[3, 1, 2, 3]] },
  { name: 'is-color-space-prototype-member', covers: 'counts["toString"] is a function, never a count.', fn: 'isScriptColorArray', args: [['toString']] },
  { name: 'is-color-space-proto', fn: 'isScriptColorArray', args: [['__proto__']] },
  { name: 'is-color-part-null', fn: 'isScriptColorArray', args: [['G', null]] },
  // actions.ts
  { name: 'sources-incomplete-throws', fn: 'javaScriptSourcesFromActionTree', args: [actionTree(node('javascript', { script: 'x()' }), true)] },
  { name: 'sources-rootless', fn: 'javaScriptSourcesFromActionTree', args: [actionTree(null)] },
  {
    name: 'sources-depth-first',
    covers: 'Root, then /Next depth-first; only javascript nodes are collected (a rendition /JS is not).',
    fn: 'javaScriptSourcesFromActionTree',
    args: [
      actionTree(
        node('javascript', { script: 'a()' }, [
          node('goto', { destination: { kind: 'fit', pageObjectNumber: 1 } }, [node('javascript', { script: 'b()' })]),
          node('rendition', { script: 'no()' }),
          node('javascript', { script: 'c()' }),
        ]),
      ),
    ],
  },
  { name: 'sources-incomplete-is-truthiness', covers: 'incomplete is read for truthiness: 1 and a non-empty string refuse, 0 and the empty string do not.', fn: 'javaScriptSourcesFromActionTree', args: [{ root: node('javascript', { script: 'one()' }), incomplete: 1 }] },
  { name: 'sources-incomplete-zero', fn: 'javaScriptSourcesFromActionTree', args: [{ root: node('javascript', { script: 'zero()' }), incomplete: 0 }] },
  { name: 'sources-incomplete-string', fn: 'javaScriptSourcesFromActionTree', args: [{ root: node('javascript', { script: 's()' }), incomplete: 'yes' }] },
  { name: 'sources-incomplete-empty-string', fn: 'javaScriptSourcesFromActionTree', args: [{ root: node('javascript', { script: 'e()' }), incomplete: '' }] },
  { name: 'sources-root-zero', fn: 'javaScriptSourcesFromActionTree', args: [{ root: 0, incomplete: false }] },
  { name: 'program-joins', fn: 'javaScriptProgramFromActionTree', args: [actionTree(node('javascript', { script: 'a()' }, [node('javascript', { script: 'b()' })]))] },
  { name: 'program-empty', fn: 'javaScriptProgramFromActionTree', args: [actionTree(node('named', { name: 'NextPage' }))] },
  { name: 'program-incomplete-throws', fn: 'javaScriptProgramFromActionTree', args: [actionTree(null, true)] },
  // input.ts
  {
    name: 'fields-from-snapshot',
    covers: 'Every value-entry kind, the Off token for toggles, and options only where the DTO has them.',
    fn: 'scriptFieldsFromSnapshot',
    args: [fieldsSnapshot],
  },
  { name: 'fields-empty', fn: 'scriptFieldsFromSnapshot', args: [snapshot([])] },
  // host.ts
  { name: 'seed-basic', fn: 'seedFrom', args: ['doc-1', 0] },
  { name: 'seed-next', fn: 'seedFrom', args: ['doc-1', 1] },
  { name: 'seed-empty-id', fn: 'seedFrom', args: ['', 0] },
  { name: 'seed-large-sequence', fn: 'seedFrom', args: ['doc', 123456789] },
  { name: 'seed-non-ascii', fn: 'seedFrom', args: ['é漢', 3] },
  {
    name: 'seed-astral-hashes-the-high-surrogate',
    covers: 'for...of walks code points and charCodeAt(0) takes the first UTF-16 unit, so the low surrogate never enters the hash.',
    fn: 'seedFrom',
    args: ['a😀b', 2],
  },
  { name: 'identity-from-claims', fn: 'resolveScriptIdentity', args: [claims(), null, false] },
  { name: 'identity-no-claims', fn: 'resolveScriptIdentity', args: [null, null, false] },
  { name: 'identity-user-id-fallback', fn: 'resolveScriptIdentity', args: [claims({ displayName: undefined }), {}, false] },
  { name: 'identity-overrides', fn: 'resolveScriptIdentity', args: [claims(), { name: 'Grace', email: 'g@x.test' }, false] },
  { name: 'identity-override-null-falls-through', covers: '?? treats null like absent.', fn: 'resolveScriptIdentity', args: [claims(), { name: null, loginName: 'root', corporation: 'Acme' }, false] },
  { name: 'identity-override-function', fn: 'resolveScriptIdentity', args: [claims({ organization: undefined }), { corporation: 'Fn Inc' }, true] },
  // types.ts
  { name: 'annot-writable-keys', fn: 'annotWritableKeys', args: [] },
  { name: 'default-script-budget', fn: 'defaultScriptBudget', args: [] },
  { name: 'default-script-security-policy', fn: 'defaultScriptSecurityPolicy', args: [] },
  { name: 'script-event-matrix', fn: 'scriptEventMatrix', args: [] },
  // prelude.ts, as data
  { name: 'prelude-source', covers: 'The text crates/plugins/acrojs/prelude.js carries.', fn: 'preludeSource', args: [] },
];

// ── the host scenarios ────────────────────────────────────────────────────────

const world = (event: ScriptWorldInput['event'] = { kind: 'widget-activate' }, over: Omit<Partial<ScriptWorldInput>, 'event'> = {}): ScriptWorldInput => ({
  fields: [],
  event,
  ...over,
});
const boot = (over: Partial<Extract<DriverOp, { op: 'boot' }>> = {}): DriverOp => ({ op: 'boot', world: world(), ...over });
const run = (program: string, over: Partial<Extract<DriverOp, { op: 'run' }>> = {}): DriverOp => ({ op: 'run', program, world: world(), ...over });
const txn = (...ops: DriverOp[]): Step => ({ op: 'call', target: { token: 'script-host' }, method: 'transaction', args: [ops] });
const host = (config: ScriptHostDriverConfig = {}) => [{ factory: 'script-host', config }];
const start: Step = { op: 'call', target: 'kernel', method: 'start' };

const hostCases: ScenarioCase[] = [
  {
    name: 'kernel-destroy-owns-host',
    covers: 'Kernel destroy invokes the probe cleanup and releases the constructed sandbox once.',
    plugins: host({ bootSources: { returns: [] } }),
    steps: [start, txn(run('owned()')), { op: 'call', target: 'kernel', method: 'destroy' }],
    ...UNSNAPSHOTTED,
  },
  {
    name: 'dispose-pending-factory',
    covers: 'Disposal releases the pending factory resource. An admitted run builds a second realm and still finishes; repeated disposal releases that realm too.',
    plugins: host({ bootSources: { returns: [] }, sandbox: { builds: [{ defer: 'vm' }, {}] } }),
    steps: [start, txn(run('admitted()')), { op: 'call', target: { token: 'script-host' }, method: 'dispose' }, { op: 'settle', label: 'vm' }, { op: 'call', target: { token: 'script-host' }, method: 'dispose' }],
    ...UNSNAPSHOTTED,
  },
  {
    name: 'sandbox-output-is-returned',
    covers: 'The host returns sandbox effects, rejection rc, event edits and diagnostics without translating them.',
    plugins: host({ bootSources: { returns: [] }, sandbox: { builds: [{ run: [{ output: {
      event: { rc: false, value: 'edited', change: 'x', selStart: 1, selEnd: 2 },
      formEffects: [{ kind: 'setValue', ref: { kind: 'fqn', name: 't' }, value: { value: 'edited' } }],
      uiEffects: [{ kind: 'print' }],
      diagnostics: [{ code: 'script-error', message: 'script refusal' }],
    } }] }] } }),
    steps: [start, txn(run('output()'))],
    ...UNSNAPSHOTTED,
  },
  {
    name: 'boots-once-and-hands-the-output-to-one-caller',
    covers: 'The first boot() builds the realm, reads the name tree and boots it; a later boot() in any transaction returns null.',
    plugins: host({ sandbox: { builds: [{ boot: [{ output: { diagnostics: [{ code: 'script-error', message: 'boot#0' }] } }] }] } }),
    steps: [start, txn(boot(), run('a()')), txn(boot())],
    ...UNSNAPSHOTTED,
  },
  {
    name: 'run-auto-boots-and-stashes-the-output',
    covers: 'A run before any boot boots defensively; that boot output waits for the next boot() and is returned exactly once.',
    plugins: host(),
    steps: [start, txn(run('a()')), txn(boot(), boot())],
    ...UNSNAPSHOTTED,
  },
  {
    name: 'transactions-serialize',
    covers: 'A pending transaction holds the realm: the next one starts only after it settles, and one that fails does not poison the queue.',
    plugins: host({ bootSources: { returns: [] } }),
    steps: [
      start,
      txn({ op: 'wait', label: 'slow' }, run('slow()')),
      txn(run('fast()')),
      txn({ op: 'throw', message: 'body broke' }),
      txn(run('after()')),
      { op: 'settle', label: 'slow' },
    ],
    ...UNSNAPSHOTTED,
  },
  {
    name: 'no-boot-sources',
    covers: 'An empty name tree marks the realm booted without calling sandbox.boot.',
    plugins: host({ bootSources: { resolves: [] } }),
    steps: [start, txn(boot(), run('a()'), boot())],
    ...UNSNAPSHOTTED,
  },
  {
    name: 'boot-sources-read-fails',
    covers: 'A failed name-tree read degrades to a synthetic boot output with a script-error diagnostic, and the cache is dropped so a rebuilt realm retries.',
    plugins: host({
      bootSources: { seq: [{ rejects: { message: 'catalog unreadable' } }, { returns: ['retry()'] }] },
      sandbox: { builds: [{ run: [{ dispose: true }] }, {}] },
    }),
    steps: [start, txn(boot(), run('a()')), txn(boot(), run('b()'))],
    ...UNSNAPSHOTTED,
  },
  {
    name: 'resource-fault-rebuilds-and-reboots',
    covers: 'A sandbox that reports disposed after a run poisons the realm; the next transaction builds a new one and boots it again.',
    plugins: host({ sandbox: { builds: [{ run: [{ dispose: true }] }, {}] } }),
    steps: [start, txn(boot(), run('boom()')), txn(run('after()'))],
    ...UNSNAPSHOTTED,
  },
  {
    name: 'boot-fault',
    covers: 'A boot that disposes the sandbox is also a fault: the next run in the same transaction auto-boots on a fresh realm.',
    plugins: host({ sandbox: { builds: [{ boot: [{ dispose: true }] }, {}] } }),
    steps: [start, txn(boot(), run('a()'))],
    ...UNSNAPSHOTTED,
  },
  {
    name: 'budgets',
    covers: 'The host default budget, a configured one, and a per-run override all reach the sandbox; the environment sequence advances per input.',
    plugins: host({ budget: { maxExecutionMs: 5, maxMemoryBytes: 1024, maxStackBytes: 256, maxEffects: 2, maxOutputBytes: 64 } }),
    steps: [
      start,
      txn(
        boot({ budget: { maxExecutionMs: 7, maxMemoryBytes: 1, maxStackBytes: 1, maxEffects: 1, maxOutputBytes: 1 } }),
        run('a()'),
        run('b()', { budget: { maxExecutionMs: 9, maxMemoryBytes: 2, maxStackBytes: 2, maxEffects: 3, maxOutputBytes: 4 } }),
      ),
    ],
    ...UNSNAPSHOTTED,
  },
  {
    name: 'input-shape',
    covers: 'inputFor: pageNumber overrides the document input only when given; annots, annotPages and annotsCoverDocument appear only when given.',
    plugins: host({
      bootSources: { returns: [] },
      document: { id: 'd', fileName: 'f.pdf', pageCount: 4, pageNumber: 1 },
      identity: { name: 'N', loginName: 'L', corporation: 'C', email: 'E' },
    }),
    steps: [
      start,
      txn(
        run('a()', {
          world: world(
            { kind: 'field-keystroke', type: 'Field', name: 'Keystroke', change: 'x', selStart: 0, selEnd: 1, willCommit: false, modifier: true, shift: false, value: 'v', target: { kind: 'fqn', name: 't' }, source: { kind: 'fqn', name: 't' } },
            {
              pageNumber: 3,
              annots: [],
              annotPages: [0, 2],
              annotsCoverDocument: false,
              fields: [
                { ref: { kind: 'fqn', name: 't' }, name: 't', family: 'text', value: 'v', defaultValue: null, display: 'visible', readOnly: false, required: true, options: [{ label: 'l', value: 'v' }] },
              ],
            },
          ),
        }),
        run('b()', { world: world({ kind: 'field-format' }, { annotsCoverDocument: true }) }),
      ),
    ],
    ...UNSNAPSHOTTED,
  },
  {
    name: 'sandbox-run-throws',
    covers: 'A sandbox call that throws rejects the transaction; the realm is not poisoned.',
    plugins: host({ bootSources: { returns: [] }, sandbox: { builds: [{ run: [{ throws: { message: 'vm exploded' } }, {}] }] } }),
    steps: [start, txn(run('a()')), txn(run('b()'))],
    ...UNSNAPSHOTTED,
  },
  {
    name: 'sandbox-boot-throws',
    covers: 'A throwing boot rejects the transaction after the realm was marked booted: the boot never re-runs.',
    plugins: host({ sandbox: { builds: [{ boot: [{ throws: { message: 'boot exploded' } }] }] } }),
    steps: [start, txn(boot()), txn(boot(), run('a()'))],
    ...UNSNAPSHOTTED,
  },
  {
    name: 'boot-sources-throw',
    covers: 'bootSources() throwing synchronously is outside the degrade path: the transaction rejects.',
    plugins: host({ bootSources: { throws: { message: 'no catalog' } } }),
    steps: [start, txn(boot()), txn(run('a()'))],
    ...UNSNAPSHOTTED,
  },
  {
    name: 'factory-rejects',
    covers: 'A factory that rejects fails the transaction; the rejected promise stays cached for the realm.',
    plugins: host({ sandbox: { builds: [{ rejects: { message: 'no vm' } }, {}] } }),
    steps: [start, txn(run('a()')), txn(run('b()'))],
    ...UNSNAPSHOTTED,
  },
  {
    name: 'factory-deferred',
    covers: 'The realm is built once even when two runs wait on the same pending factory.',
    plugins: host({ bootSources: { returns: [] }, sandbox: { builds: [{ defer: 'vm' }] } }),
    steps: [start, txn(run('a()'), run('b()')), { op: 'settle', label: 'vm' }],
    ...UNSNAPSHOTTED,
  },
  {
    name: 'dispose',
    covers: 'dispose() releases the realm one turn after its factory settled, is safe to repeat, and every later transaction rejects.',
    plugins: host({ bootSources: { returns: [] } }),
    steps: [
      start,
      txn(run('a()')),
      { op: 'call', target: { token: 'script-host' }, method: 'dispose' },
      { op: 'call', target: { token: 'script-host' }, method: 'dispose' },
      txn(run('b()')),
    ],
    ...UNSNAPSHOTTED,
  },
  {
    name: 'dispose-before-any-realm',
    covers: 'dispose() with no realm built does nothing but refuse later transactions.',
    plugins: host(),
    steps: [start, { op: 'call', target: { token: 'script-host' }, method: 'dispose' }, txn(boot())],
    ...UNSNAPSHOTTED,
  },
  {
    name: 'dispose-with-a-rejected-factory',
    covers: 'A factory rejection is swallowed on dispose.',
    plugins: host({ sandbox: { builds: [{ rejects: { message: 'no vm' } }] } }),
    steps: [start, txn(run('a()')), { op: 'call', target: { token: 'script-host' }, method: 'dispose' }],
    ...UNSNAPSHOTTED,
  },
];

export const suites: Suite[] = [
  {
    kind: 'function',
    file: 'core/acrojs.json',
    describes: 'packages/core/acrojs: color conversion, action-tree sources, the field view, seeds, identity, and the shipped constants.',
    functions: {
      scriptColorToRgb,
      scriptColorToCss,
      cssToScriptColor,
      isScriptColorArray,
      javaScriptSourcesFromActionTree,
      javaScriptProgramFromActionTree,
      scriptFieldsFromSnapshot,
      seedFrom,
      resolveScriptIdentity: (identity: unknown, overrides: Partial<ScriptIdentity> | null, asFunction: boolean) =>
        resolveScriptIdentity(
          { security: { identity } } as unknown as DocumentHandle,
          overrides === null ? undefined : asFunction ? () => overrides : overrides,
        ),
      annotWritableKeys: () => ANNOT_WRITABLE_KEYS,
      defaultScriptBudget: () => DEFAULT_SCRIPT_BUDGET,
      defaultScriptSecurityPolicy: () => DEFAULT_SCRIPT_SECURITY_POLICY,
      scriptEventMatrix: () => SCRIPT_EVENT_MATRIX,
      preludeSource: () => PRELUDE_SOURCE,
    },
    cases: functionCases,
  },
  {
    kind: 'scenario',
    file: 'core/acrojs-host.json',
    describes: 'packages/core/acrojs createScriptHost: the per-document realm mutex, lazy boot, fault ladder and disposal, over a scripted sandbox.',
    cases: hostCases,
  },
];
