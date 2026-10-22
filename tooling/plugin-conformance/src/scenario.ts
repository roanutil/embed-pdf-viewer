/**
 * The scenario runner: a kernel, its plugins and the fake engine driven through
 * an ordered list of steps, with a trace entry recorded after each one.
 *
 * `README.md` is the specification the Rust runner
 * (`crates/plugin-conformance/src/scenario.rs`) implements. This file is the
 * oracle's half of it.
 */
import FakeTimers from '@sinonjs/fake-timers';
import {
  createCapabilityToken,
  createKernel,
  DocumentsToken,
  type AnyPlugin,
  type CapabilityToken,
  type Kernel,
} from '@embedpdf/core';

import {
  canonical,
  canonicalError,
  isThenable,
  snapshotHooks,
  type CanonicalHooks,
} from './canonical';
import {
  createFakeEngine,
  makeError,
  type ErrorSpec,
  type FakeEngine,
  type FakeEngineSpec,
} from './fake-engine';

// ── the vector format ─────────────────────────────────────────────────────────

export interface PluginSpec {
  /** A factory name from the registry. */
  readonly factory: string;
  readonly config?: unknown;
}

export type Target =
  | 'kernel'
  | 'documents'
  | { readonly token: string; readonly doc?: string }
  | { readonly ref: number };

/** `meter: n` on a synchronous step counts microtask turns: right after the
 *  step's action the runner logs `@@turn [i]` for i = 0..n-1, one per turn,
 *  and every tracked promise that settles while it runs also logs
 *  `@@settled [promise]` from its `.then` reaction. Where the two interleave
 *  in `callbacks` is the turn a promise settled on, which `settled` alone,
 *  an order within one step, cannot show. */
interface Metered {
  readonly meter?: number;
}

export type Step =
  | ({
      readonly op: 'call';
      readonly target: Target;
      readonly method?: string;
      readonly args?: readonly unknown[];
    } & Metered)
  | ({ readonly op: 'emit'; readonly doc: string; readonly event: unknown } & Metered)
  | ({
      readonly op: 'settle';
      readonly label: string;
      readonly value?: unknown;
      readonly error?: ErrorSpec;
    } & Metered)
  | { readonly op: 'advance'; readonly ms: number }
  | ({ readonly op: 'abort'; readonly promise: number; readonly reason?: unknown } & Metered)
  | ({ readonly op: 'drain' } & Metered);

/** A case either snapshots state after every step, or says why it does not.
 *  `reason` lands in the vector beside `snapshot: 'none'`: what the case
 *  compares instead, so a reviewer can tell an outcome-only case from one
 *  that skipped the state its plugins wrote. */
export type ScenarioCase = ScenarioCaseBase &
  (
    | {
        /** Slice keys to snapshot; absent snapshots the whole tree. */
        readonly snapshot?: readonly string[];
        readonly reason?: never;
      }
    | { readonly snapshot: 'none'; readonly reason: string }
  );

interface ScenarioCaseBase {
  readonly name: string;
  readonly covers: string;
  readonly plugins?: readonly PluginSpec[];
  readonly engine?: FakeEngineSpec;
  readonly steps: readonly Step[];
  /** Let an engine call go unanswered (it rejects with FakeEngineUnmatched). */
  readonly allowUnmatched?: boolean;
  /** Await each call's returned promise, in real time, before the step
   *  drains. For code whose promises wait on the host's thread pool
   *  (WebCrypto), which no number of microtask turns settles. The Rust side
   *  has no such pool: its step already runs until nothing is left. */
  readonly awaitCalls?: boolean;
  /** A painting host: deterministic frames every 16 ms on the scenario clock. */
  readonly clock?: { readonly frames: true };
}

export type Outcome =
  | { readonly returned: unknown }
  | { readonly threw: unknown }
  | { readonly promise: number };

export interface TraceEntry {
  outcome?: Outcome;
  settled: { promise: number; resolved?: unknown; rejected?: unknown }[];
  stateChanges: { slice: string; previous: unknown; next: unknown }[];
  documentEvents: { event: string; value: unknown }[];
  engine: unknown[];
  callbacks: { fn: string; args: unknown[] }[];
  reports: unknown[];
  status: string;
  engineState: unknown;
  state?: unknown;
}

// ── the registry: which plugin factories and tokens a case may name ───────────

/** What a factory may use from the running case: log a callback into the
 *  current trace entry, or park a settlement under a label. */
export interface PluginEnv {
  log(fn: string, args: unknown[]): void;
  /** The token with this name: a registered plugin's, else one made for this
   *  run (the first request decides its hint). */
  token(name: string, hint?: string): CapabilityToken<unknown>;
  defer(label: string, resolve: (value: unknown) => void, reject: (error: unknown) => void): void;
}

export interface PluginEntry {
  readonly factory: (config: unknown, env: PluginEnv) => AnyPlugin;
  readonly tokens?: readonly CapabilityToken<unknown>[];
}

const factories = new Map<string, PluginEntry>();
const tokens = new Map<string, CapabilityToken<unknown>>([[DocumentsToken.name, DocumentsToken]]);

/** Tokens made during one run, for names no registered plugin owns. Reset per
 *  case, so a probe's token in one case is never the same object as another's. */
let runTokens = new Map<string, CapabilityToken<unknown>>();

export function tokenNamed(name: string, hint?: string): CapabilityToken<unknown> {
  const registered = tokens.get(name) ?? runTokens.get(name);
  if (registered) return registered;
  const token = createCapabilityToken<unknown>(name, hint === undefined ? undefined : { hint });
  runTokens.set(name, token);
  return token;
}

export function registerPlugin(name: string, entry: PluginEntry): void {
  // Two suites registering one name would leave every case of the first
  // building the second's plugin, depending on import order.
  if (factories.has(name))
    throw new Error(`[scenario] plugin factory "${name}" is registered twice`);
  factories.set(name, entry);
  for (const token of entry.tokens ?? []) tokens.set(token.name, token);
}

/** Resets run when each case ends. A suite that routes a module-global hook
 *  (a console shim) to the current case's env registers one, so output after
 *  the case reaches the real console instead of a finished case's trace. */
const caseEndResets: (() => void)[] = [];

export function onCaseEnd(reset: () => void): void {
  caseEndResets.push(reset);
}

export function registerToken(token: CapabilityToken<unknown>): void {
  tokens.set(token.name, token);
}

// ── running a case ────────────────────────────────────────────────────────────

/** Real `setImmediate`, captured before fake timers install: one turn of it
 *  runs every queued microtask, which is what "drain" means. */
const realSetImmediate = globalThis.setImmediate;
const realSetTimeout = globalThis.setTimeout;
const realClearTimeout = globalThis.clearTimeout;
const drainMicrotasks = () => new Promise<void>((resolve) => realSetImmediate(resolve));

export async function runScenario(
  scenario: ScenarioCase,
): Promise<{ trace: TraceEntry[]; unmatched: string[] }> {
  const base = await instanceBase();
  const { trace, unmatched } = await runScenarioRaw(scenario);
  return { trace: renumberInstances(trace, base) as TraceEntry[], unmatched };
}

async function runScenarioRaw(
  scenario: ScenarioCase,
): Promise<{ trace: TraceEntry[]; unmatched: string[] }> {
  assertPlainAnswers(scenario);
  runTokens = new Map();
  const clock = FakeTimers.install({
    now: 0,
    toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'Date', 'performance'],
  });
  const onUnhandled = () => {};
  process.on('unhandledRejection', onUnhandled);
  try {
    return await runInstalled(scenario, clock);
  } finally {
    process.off('unhandledRejection', onUnhandled);
    clock.uninstall();
    for (const reset of caseEndResets) reset();
  }
}

async function runInstalled(
  scenario: ScenarioCase,
  clock: ReturnType<typeof FakeTimers.install>,
): Promise<{ trace: TraceEntry[]; unmatched: string[] }> {
  let entry = blankEntry();
  let metering = false;
  const refs: { fn: (...args: unknown[]) => unknown; self?: unknown }[] = [];
  const promises: PromiseLike<unknown>[] = [];

  const hooks: CanonicalHooks = {
    fn: (fn, self) => refs.push({ fn, self }) - 1,
    promise: (promise) => {
      const id = promises.push(promise) - 1;
      const metered = () => {
        if (metering) entry.callbacks.push({ fn: '@@settled', args: [id] });
      };
      promise.then(
        (value) => {
          metered();
          entry.settled.push({ promise: id, resolved: canonical(value, hooks) });
        },
        (error) => {
          metered();
          entry.settled.push({ promise: id, rejected: canonicalError(error, hooks) });
        },
      );
      return id;
    },
  };

  let fake: FakeEngine;
  const decode = (value: unknown): unknown =>
    decodeValue(value, { hooks, refs, fake: () => fake, entry: () => entry });
  fake = createFakeEngine(scenario.engine ?? {}, decode, (error) =>
    entry.reports.push(canonicalError(error, snapshotHooks)),
  );

  const spy: AnyPlugin = {
    id: '@@conformance/spy',
    create: (ctx) => ({
      api: {},
      connect: () => {
        const documents = ctx.get(DocumentsToken);
        const observe = <T>(event: string, source: (listener: (value: T) => void) => () => void) =>
          ctx.listen(source, (value) =>
            entry.documentEvents.push({ event, value: canonical(value, snapshotHooks) }),
          );
        observe('opened', documents.onOpened);
        observe('openFailed', documents.onOpenFailed);
        observe('locked', documents.onLocked);
        observe('closed', documents.onClosed);
        observe('activeChanged', documents.onActiveChanged);
        observe('pagesChanged', documents.onPagesChanged);
        observe('unsavedChangesChanged', documents.onUnsavedChangesChanged);
      },
    }),
  };
  const env: PluginEnv = {
    log: (fn, args) =>
      entry.callbacks.push({ fn, args: args.map((arg) => canonical(arg, snapshotHooks)) }),
    defer: (label, resolve, reject) => fake.defer(label, resolve, reject),
    token: tokenNamed,
  };
  // One wrapper per plugin object, so a factory that hands back the same
  // definition twice still installs the same object twice.
  const wrapped = new Map<AnyPlugin, AnyPlugin>();
  const plugins = [
    spy,
    ...(scenario.plugins ?? []).map((spec) => {
      const plugin = instantiate(spec, env);
      let wrapper = wrapped.get(plugin);
      if (!wrapper) {
        wrapper = instrument(plugin, () => entry);
        wrapped.set(plugin, wrapper);
      }
      return wrapper;
    }),
  ];
  let kernel: Kernel;
  try {
    kernel = createKernel({
      ...(scenario.clock?.frames
        ? {
            clock: {
              now: () => performance.now(),
              after: (ms: number, run: () => void) => {
                const timer = setTimeout(run, ms);
                return () => clearTimeout(timer);
              },
              nextFrame: (run: (timeMs: number) => void) => {
                const timer = setTimeout(() => run(performance.now()), 16);
                return () => clearTimeout(timer);
              },
            },
          }
        : {}),
      engine: fake.engine,
      plugins,
      report: (error) => entry.reports.push(canonicalError(error, snapshotHooks)),
    });
  } catch (error) {
    // Construction is where the plan is validated. A case about a bad plugin
    // list has one trace entry, the refusal, and no steps.
    entry.outcome = { threw: canonicalError(error, snapshotHooks) };
    entry.status = 'unconstructed';
    return { trace: [entry], unmatched: fake.unmatched };
  }

  const trace: TraceEntry[] = [];
  let loggedEngine = 0;
  for (const step of scenario.steps) {
    if (trace.length > 0) entry = blankEntry();
    const log = entry.callbacks;
    // Started inside runStep, in the same synchronous run as the step's
    // action: awaiting runStep first would put the meter a turn behind.
    const startMeter = (turns: number) => {
      metering = true;
      void (async () => {
        for (let i = 0; i < turns; i += 1) {
          log.push({ fn: '@@turn', args: [i] });
          await undefined;
        }
      })();
    };
    await runStep(step, { kernel, fake, clock, refs, promises, hooks, decode, entry, startMeter });
    if (scenario.awaitCalls && step.op === 'call' && entry.outcome && 'promise' in entry.outcome) {
      await settledInRealTime(promises[entry.outcome.promise]);
    }
    await drainMicrotasks();
    metering = false;
    entry.engine = fake.log.slice(loggedEngine);
    loggedEngine = fake.log.length;
    entry.status = kernel.status();
    entry.engineState = fake.snapshot();
    if (scenario.snapshot !== 'none') entry.state = snapshot(kernel, scenario.snapshot);
    trace.push(entry);
  }
  return { trace, unmatched: fake.unmatched };
}

/** Wait for a promise to settle without the fake clock: WebCrypto answers
 *  from libuv's pool. Five real seconds is a case that forgot a response. */
async function settledInRealTime(promise: PromiseLike<unknown>): Promise<void> {
  let timer: ReturnType<typeof realSetTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = realSetTimeout(
      () => reject(new Error('[scenario] awaitCalls: a call did not settle in 5 s')),
      5000,
    );
  });
  try {
    await Promise.race([
      Promise.resolve(promise).then(
        () => undefined,
        () => undefined,
      ),
      timeout,
    ]);
  } finally {
    realClearTimeout(timer);
  }
}

function blankEntry(): TraceEntry {
  return {
    settled: [],
    stateChanges: [],
    documentEvents: [],
    engine: [],
    callbacks: [],
    reports: [],
    status: '',
    engineState: {},
  };
}

function instantiate(spec: PluginSpec, env: PluginEnv): AnyPlugin {
  const entry = factories.get(spec.factory);
  if (!entry) throw new Error(`[scenario] no plugin factory "${spec.factory}"`);
  return entry.factory(spec.config, env);
}

/** Observe committed session state without changing the context or its lifecycle. */
function instrument(plugin: AnyPlugin, entry: () => TraceEntry): AnyPlugin {
  return {
    ...plugin,
    create: (ctx) => {
      const slice = ctx.documentId ? `${plugin.id}::${ctx.documentId}` : plugin.id;
      ctx.cleanup(
        ctx.state.onChange(({ previous, next }) => {
          entry().stateChanges.push({
            slice,
            previous: canonical(previous, snapshotHooks),
            next: canonical(next, snapshotHooks),
          });
        }),
      );
      return plugin.create(ctx);
    },
  };
}

// ── instance ids ──────────────────────────────────────────────────────────────

/** The kernel numbers document instances (`<id>#<n>`) from one counter shared
 *  by every kernel in the process, so a raw trace would depend on how many
 *  cases ran before it. `instanceBase()` reads the counter by opening a
 *  document in a throwaway kernel; `renumberInstances` then rewrites every
 *  `<id>#<n>` with `n` above the base to `n - base`, which is what a kernel
 *  whose counter starts at zero writes. The Rust kernel counts per kernel. */
let burned = false;
async function instanceBase(): Promise<number> {
  const fake = createFakeEngine({ documents: { probe: { pages: [] } } }, (value) => value);
  const kernel = createKernel({ engine: fake.engine, plugins: [], report: () => {} });
  await kernel.start();
  // Lane ids (`<lane>#<n>`) share the shape and count from one per lane.
  // Pushing the counter past any lane's count first keeps the two apart.
  if (!burned) {
    burned = true;
    for (let i = 0; i < 1000; i += 1)
      void kernel.documents.open({ kind: 'bytes', id: `burn${i}` } as never).catch(() => {});
  }
  await kernel.documents.open({ kind: 'bytes', id: 'probe', bytes: new Uint8Array() } as never);
  const instanceId = kernel.getState().core.documents.probe!.instanceId;
  await kernel.destroy();
  return Number(instanceId.slice(instanceId.lastIndexOf('#') + 1));
}

function renumberInstances(value: unknown, base: number): unknown {
  if (typeof value === 'string') {
    return value.replace(/([^\s#"'`(]+)#(\d+)/g, (whole, id: string, n: string) =>
      Number(n) > base ? `${id}#${Number(n) - base}` : whole,
    );
  }
  if (Array.isArray(value)) return value.map((item) => renumberInstances(item, base));
  if (value && typeof value === 'object') {
    const out: Record<string, unknown> = {};
    for (const [key, member] of Object.entries(value))
      out[renumberInstances(key, base) as string] = renumberInstances(member, base);
    return out;
  }
  return value;
}

function snapshot(kernel: Kernel, keys: readonly string[] | undefined): unknown {
  const state = kernel.getState();
  if (!keys) return canonical(state, snapshotHooks);
  const plugins: Record<string, unknown> = {};
  for (const key of keys) {
    if (key === '@@core') continue;
    if (key in state.plugins) plugins[key] = state.plugins[key];
  }
  return canonical(
    keys.includes('@@core') ? { core: state.core, plugins } : { plugins },
    snapshotHooks,
  );
}

interface StepContext {
  kernel: Kernel;
  fake: FakeEngine;
  clock: ReturnType<typeof FakeTimers.install>;
  refs: { fn: (...args: unknown[]) => unknown; self?: unknown }[];
  promises: PromiseLike<unknown>[];
  hooks: CanonicalHooks;
  decode: (value: unknown) => unknown;
  entry: TraceEntry;
  startMeter: (turns: number) => void;
}

async function runStep(step: Step, ctx: StepContext): Promise<void> {
  if (step.op === 'advance') {
    if ('meter' in step) throw new Error('[scenario] an advance step cannot be metered');
    await ctx.clock.tickAsync(step.ms);
    return;
  }
  runSyncStep(step, ctx);
  if (step.meter) ctx.startMeter(step.meter);
}

function runSyncStep(step: Exclude<Step, { op: 'advance' }>, ctx: StepContext): void {
  switch (step.op) {
    case 'call':
      ctx.entry.outcome = invoke(step, ctx);
      return;
    case 'emit':
      ctx.fake.emit(step.doc, step.event);
      return;
    case 'settle':
      if (!ctx.fake.settle(step.label, step))
        throw new Error(`[scenario] nothing pending as "${step.label}"`);
      return;
    case 'abort': {
      const promise = ctx.promises[step.promise] as { abort?: (reason?: unknown) => void };
      if (typeof promise?.abort !== 'function')
        throw new Error(`[scenario] promise ${step.promise} is not abortable`);
      promise.abort(ctx.decode(step.reason));
      return;
    }
    case 'drain':
      return;
  }
}

function invoke(step: Extract<Step, { op: 'call' }>, ctx: StepContext): Outcome {
  try {
    const { self, fn } = resolveTarget(step.target, step.method, ctx);
    const args = (step.args ?? []).map(ctx.decode);
    const result = fn.apply(self, args);
    if (isThenable(result)) return { promise: ctx.hooks.promise(result) };
    return { returned: canonical(result, ctx.hooks) };
  } catch (error) {
    return { threw: canonicalError(error, ctx.hooks) };
  }
}

function resolveTarget(
  target: Target,
  method: string | undefined,
  ctx: StepContext,
): { self: unknown; fn: (...args: unknown[]) => unknown } {
  let object: unknown;
  if (target === 'kernel') object = ctx.kernel;
  else if (target === 'documents') object = ctx.kernel.documents;
  else if ('ref' in target) {
    const ref = ctx.refs[target.ref];
    if (!ref) throw new Error(`[scenario] no ref ${target.ref}`);
    if (method !== undefined)
      throw new Error('[scenario] a ref is a function; call it without a method');
    return { self: ref.self, fn: ref.fn };
  } else {
    object = ctx.kernel.capability(tokenNamed(target.token), target.doc);
  }
  if (method === undefined) throw new Error('[scenario] a call on an object needs a method');
  const fn = (object as Record<string, unknown>)[method];
  if (typeof fn !== 'function') throw new Error(`[scenario] "${method}" is not a function`);
  return { self: object, fn: fn as (...args: unknown[]) => unknown };
}

// ── decoding: canonical JSON back into live values ────────────────────────────

interface DecodeContext {
  hooks: CanonicalHooks;
  refs: { fn: (...args: unknown[]) => unknown; self?: unknown }[];
  fake: () => FakeEngine;
  entry: () => TraceEntry;
}

/** A scripted callback. Each call is logged, then answered by the next
 *  `seq` element (the last one repeats), or by the spec itself. */
export interface FnSpec {
  readonly $fn: string;
  readonly returns?: unknown;
  readonly throws?: ErrorSpec;
  readonly resolves?: unknown;
  readonly rejects?: ErrorSpec;
  /** Return a promise that a `settle` step with this label settles. */
  readonly defer?: string;
  readonly seq?: readonly Omit<FnSpec, '$fn' | 'seq'>[];
}

function decodeValue(value: unknown, dc: DecodeContext): unknown {
  if (Array.isArray(value)) return value.map((v) => decodeValue(v, dc));
  if (value === null || typeof value !== 'object') return value;
  const object = value as Record<string, unknown>;
  if ('$undefined' in object) return undefined;
  if ('$number' in object) return Number(object.$number);
  if ('$bytes' in object) return new Uint8Array(Buffer.from(object.$bytes as string, 'base64'));
  if ('$error' in object) return makeError(object.$error as ErrorSpec);
  if ('$map' in object) {
    return new Map(
      (object.$map as [unknown, unknown][]).map(([k, v]) => [
        decodeValue(k, dc),
        decodeValue(v, dc),
      ]),
    );
  }
  if ('$set' in object) return new Set((object.$set as unknown[]).map((v) => decodeValue(v, dc)));
  if ('$token' in object) return tokenNamed(object.$token as string);
  if ('$ref' in object) {
    const ref = dc.refs[object.$ref as number];
    if (!ref) throw new Error(`[scenario] no ref ${String(object.$ref)}`);
    return ref.fn;
  }
  if ('$fn' in object) return scriptedFn(object as unknown as FnSpec, dc);
  if ('$document' in object) return object;
  const out: Record<string, unknown> = {};
  for (const [key, member] of Object.entries(object)) out[key] = decodeValue(member, dc);
  return out;
}

function scriptedFn(spec: FnSpec, dc: DecodeContext): (...args: unknown[]) => unknown {
  let calls = 0;
  return (...args: unknown[]) => {
    dc.entry().callbacks.push({
      fn: spec.$fn,
      args: args.map((arg) => canonical(arg, snapshotHooks)),
    });
    const answer = spec.seq ? spec.seq[Math.min(calls, spec.seq.length - 1)] : spec;
    calls += 1;
    if (answer.throws) throw makeError(answer.throws);
    if (answer.defer !== undefined) {
      return new Promise((resolve, reject) => dc.fake().defer(answer.defer!, resolve, reject));
    }
    if (answer.rejects) return Promise.reject(makeError(answer.rejects));
    if ('resolves' in answer) return Promise.resolve(decodeValue(answer.resolves, dc));
    return decodeValue(answer.returns, dc);
  };
}

/** Tags this runner would turn into live objects, which the Rust runner
 *  (`harness.rs` `Callback::call`, `fake_engine.rs`) hands a plugin as the
 *  tagged JSON itself. An answer carrying one would replay differently, so
 *  `assertPlainAnswers` refuses the case. `$undefined`, `$number` and
 *  `$bytes` are how the Rust kernel holds those values, and a nested `$fn`
 *  is turned into a function by the adapter that reads it, so those stay. */
const LIVE_TAGS = ['$error', '$map', '$set', '$token', '$ref'] as const;

function refuseLiveTags(value: unknown, where: string): void {
  if (Array.isArray(value)) return value.forEach((item) => refuseLiveTags(item, where));
  if (value === null || typeof value !== 'object') return;
  const tag = LIVE_TAGS.find((t) => t in value);
  if (tag) {
    throw new Error(
      `[scenario] ${where} carries ${tag}, which the Rust runner would replay as raw JSON`,
    );
  }
  for (const member of Object.values(value)) refuseLiveTags(member, where);
}

/** Every `$fn` spec in `value`: its `returns` and `resolves` answers. */
function refuseInCallbackAnswers(value: unknown, where: string): void {
  if (Array.isArray(value)) return value.forEach((item) => refuseInCallbackAnswers(item, where));
  if (value === null || typeof value !== 'object') return;
  const spec = value as Partial<FnSpec>;
  if (typeof spec.$fn === 'string') {
    for (const answer of [spec, ...(spec.seq ?? [])]) {
      refuseLiveTags(answer.returns, `${where}: callback "${spec.$fn}"`);
      refuseLiveTags(answer.resolves, `${where}: callback "${spec.$fn}"`);
    }
  }
  for (const member of Object.values(value)) refuseInCallbackAnswers(member, where);
}

/** Refuse a case whose callback or fake-engine answers carry a `LIVE_TAGS`
 *  member: the two runners would hand the plugin different values. */
function assertPlainAnswers(scenario: ScenarioCase): void {
  const where = `case "${scenario.name}"`;
  const engine = scenario.engine ?? {};
  for (const response of engine.responses ?? [])
    refuseLiveTags([response.value, response.events], `${where}: engine ${response.path}`);
  for (const [key, document] of Object.entries(engine.documents ?? {})) {
    for (const response of document.responses ?? []) {
      refuseLiveTags(
        [response.value, response.events],
        `${where}: document ${key} ${response.path}`,
      );
    }
    refuseLiveTags([document.security, document.pages, document.namedPages], `${where}: document ${key}`);
  }
  refuseInCallbackAnswers(scenario.plugins, where);
  for (const step of scenario.steps) {
    if (step.op === 'settle') refuseLiveTags(step.value, `${where}: settle ${step.label}`);
    if (step.op === 'emit') refuseLiveTags(step.event, `${where}: emit to ${step.doc}`);
    if (step.op === 'call') refuseInCallbackAnswers(step.args, where);
  }
}
