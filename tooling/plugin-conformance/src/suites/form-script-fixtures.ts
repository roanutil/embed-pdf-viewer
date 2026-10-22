/** Current scripting host inputs and detached-controller driver. */
import { definePlugin } from '@embedpdf/core';
import type {
  FormFieldDTO,
  FormFieldRef,
  FormFieldValue,
  FormSnapshot,
  PdfActionNode,
  PdfActionTree,
  PdfActionTargetRef,
} from '@embedpdf/engine-core/runtime';
import type {
  ScriptOutput,
  ScriptSandbox,
  ScriptSandboxFactory,
  ScriptIdentity,
  ScriptBudget,
} from '../../../../packages/core/acrojs/src/index';
import { javaScriptProgramFromActionTree } from '../../../../packages/core/acrojs/src/index';
import { actionsPlugin } from '../../../../packages/plugin/actions/src/actions.plugin';
import {
  ActionsToken,
  type DetachedScriptRealm,
  type ActionsHostCapability,
} from '../../../../packages/plugin/actions/src/host-contract';
import type { ActionsConfig } from '../../../../packages/plugin/actions/src/contract';
import {
  createFormScriptingController,
  type FormScriptingController,
} from '../../../../packages/plugin/form/src/scripting/controller';
import {
  registerPlugin,
  onCaseEnd,
  type ScenarioCase,
  type PluginEnv,
  type Step,
} from '../scenario';
import type { ErrorSpec, ResponseSpec } from '../fake-engine';
import { makeError } from '../fake-engine';
import { snapshot, list as currentList, call as rawFormCall } from './form-current-fixtures';
import { row as completeRow } from './form-live-fixtures';
import { doc as currentDoc, boot } from './annotation-live-fixtures';
import type { FakeDocumentSpec } from '../fake-engine';
import type { FormConfig } from '../../../../packages/plugin/form/src/contract';
import type { WidgetAnnotation } from '@embedpdf/engine-core/runtime';
export * from './form-live-fixtures';
export const suites = [];
export interface Scripted {
  readonly $fn: string;
  readonly returns?: unknown;
}
interface SandboxAnswer {
  readonly output?: Partial<ScriptOutput>;
  readonly throws?: ErrorSpec;
  readonly dispose?: boolean;
}
export interface SandboxSpec {
  readonly builds?: readonly {
    readonly rejects?: ErrorSpec;
    readonly defer?: string;
    readonly boot?: readonly SandboxAnswer[];
    readonly run?: readonly SandboxAnswer[];
  }[];
}
export interface StandaloneConfig {
  readonly sandbox: SandboxSpec;
  readonly identity?: Partial<ScriptIdentity> | Scripted;
  readonly fileName?: Scripted;
  readonly now?: Scripted;
  readonly utcOffsetMinutes?: Scripted;
  readonly randomSeed?: Scripted;
  readonly budget?: ScriptBudget;
}
/** Answers describe the host sandbox; they contain no form policy or engine writes. */
function sandboxFactory(spec: SandboxSpec, env: PluginEnv): ScriptSandboxFactory {
  let built = 0;
  return () => {
    const id = built++;
    env.log('sandbox.factory', [id]);
    const builds = spec.builds?.length ? spec.builds : [{}];
    const build = builds[Math.min(id, builds.length - 1)]!;
    let disposed = false,
      boots = 0,
      runs = 0;
    const answer = (answers: readonly SandboxAnswer[] | undefined, index: number): ScriptOutput => {
      const next = answers?.length ? answers[Math.min(index, answers.length - 1)]! : {};
      if (next.dispose) disposed = true;
      if (next.throws) throw makeError(next.throws);
      return {
        event: { rc: true, value: null, change: '', selStart: 0, selEnd: 0 },
        formEffects: [],
        annotEffects: [],
        uiEffects: [],
        diagnostics: [],
        ...next.output,
      };
    };
    const sandbox: ScriptSandbox = {
      get disposed() {
        return disposed;
      },
      boot(sources, input, budget) {
        env.log('sandbox.boot', [id, sources, input, budget]);
        return answer(build.boot, boots++);
      },
      run(source, input, budget) {
        env.log('sandbox.run', [id, source, input, budget]);
        return answer(build.run, runs++);
      },
      dispose() {
        env.log('sandbox.dispose', [id]);
        disposed = true;
      },
    };
    if (build.rejects) return Promise.reject(makeError(build.rejects));
    if (build.defer)
      return new Promise<ScriptSandbox>((resolve, reject) =>
        env.defer(build.defer!, () => resolve(sandbox), reject),
      );
    return Promise.resolve(sandbox);
  };
}
interface HostConfig extends Omit<ActionsConfig, 'javascript'> {
  readonly javascript?: Omit<StandaloneConfig, 'sandbox'> & {
    readonly enabled: boolean;
    readonly sandbox: SandboxSpec | null;
  };
}
const callback = <T>(
  value: Scripted | undefined,
  env: PluginEnv,
  check: (value: unknown) => value is T,
): (() => T) | undefined =>
  value &&
  (() => {
    env.log(value.$fn, []);
    if (!check(value.returns)) throw new Error('invalid scripted host answer: ' + value.$fn);
    return value.returns;
  });
const number = (value: unknown): value is number => typeof value === 'number';
const string = (value: unknown): value is string => typeof value === 'string';
const identity = (value: unknown): value is Partial<ScriptIdentity> =>
  value !== null &&
  typeof value === 'object' &&
  Object.values(value).every((v) => typeof v === 'string');
registerPlugin('form-actions-current', {
  factory: (raw, env) => {
    const config = raw as HostConfig | undefined;
    const { javascript, ...rest } = config ?? {};
    if (!javascript) return actionsPlugin(rest);
    const hostIdentity = javascript.identity;
    return actionsPlugin({
      ...rest,
      javascript: {
        enabled: javascript.enabled && javascript.sandbox !== null,
        ...(javascript.sandbox ? { sandboxFactory: sandboxFactory(javascript.sandbox, env) } : {}),
        ...(hostIdentity
          ? {
              identity:
                '$fn' in hostIdentity ? callback(hostIdentity, env, identity) : hostIdentity,
            }
          : {}),
        fileName: callback(javascript.fileName, env, string),
        now: callback(javascript.now, env, number),
        utcOffsetMinutes: callback(javascript.utcOffsetMinutes, env, number),
        randomSeed: callback(javascript.randomSeed, env, number),
        budget: javascript.budget,
      },
    });
  },
  tokens: [ActionsToken],
});
registerPlugin('form-scripts-current', {
  factory: (_, env) =>
    definePlugin({
      id: 'form-scripts-current',
      token: env.token('form-scripts-current'),
      scope: 'document',
      requires: [ActionsToken],
      create: (ctx) => {
        let realm: DetachedScriptRealm | null = null,
          controller: FormScriptingController | null = null;
        const ready = () => {
          if (controller) return controller;
          const mint = ctx.get(ActionsToken).createDetachedScriptRealm;
          if (!mint) throw new Error('scripting is off');
          realm = mint({
            doc: ctx.doc,
            document: () => ctx.document(),
            bootSources: async () => {
              const catalog = ctx.doc.actions ? await ctx.doc.actions.get() : null;
              return (catalog?.nameTreeScripts ?? []).map(({ action }) =>
                javaScriptProgramFromActionTree(action),
              );
            },
          });
          const port = realm;
          controller = createFormScriptingController({
            document: () => ctx.document(),
            transaction: (body) => port.transaction(body),
            budget: port.budget,
          });
          return controller;
        };
        ctx.cleanup(() => {
          controller?.dispose();
          realm?.dispose();
        });
        return {
          api: {
            commit: (form: FormSnapshot, ref: FormFieldRef, value: FormFieldValue) =>
              ready().commit(form, ref, value),
            activate: (form: FormSnapshot, ref: FormFieldRef, action: PdfActionTree) =>
              ready().activate(form, ref, action),
            recalculate: (form: FormSnapshot) => ready().recalculate(form),
            dispose: () => {
              ready().dispose();
              realm?.dispose();
            },
          },
        };
      },
    }),
});
export const fnSpec = (name: string, over: { returns?: unknown } = {}): Scripted => ({
  $fn: name,
  ...over,
});
export const standalonePlugins = (config: StandaloneConfig) => [
  {
    factory: 'form-actions-current',
    config: { openSequence: 'off', javascript: { enabled: true, ...config } } satisfies HostConfig,
  },
  { factory: 'form-scripts-current' },
];
type ControllerArgs = {
  commit: [FormFieldRef, FormFieldValue];
  activate: [FormFieldRef, PdfActionTree];
  recalculate: [];
  dispose: [];
};
export function standalone<K extends keyof ControllerArgs>(
  method: K,
  args?: ControllerArgs[K],
  doc?: string,
): Step;
export function standalone(
  method: keyof ControllerArgs,
  args: readonly unknown[] = [],
  doc = 'a',
): Step {
  return { op: 'call', target: { token: 'form-scripts-current', doc }, method, args };
}
type ActionMethod = {
  [K in keyof ActionsHostCapability]-?: ActionsHostCapability[K] extends (
    ...args: never[]
  ) => unknown
    ? K
    : never;
}[keyof ActionsHostCapability];
type ActionArgs<K extends ActionMethod> = K extends `on${string}` | 'setSubmitHandler'
  ? [Scripted]
  : K extends 'setUiAdapter'
    ? [{ print: Scripted; alert?: Scripted; gotoPage?: Scripted; openUri?: Scripted }]
    : ActionsHostCapability[K] extends (...args: infer Args) => unknown
      ? Args
      : never;
export function act<K extends ActionMethod>(method: K, args: ActionArgs<K>, doc?: string): Step;
export function act(method: ActionMethod, args: readonly unknown[] = [], doc = 'a'): Step {
  return { op: 'call', target: { token: 'actions', doc }, method, args };
}
export const list = (
  fields: FormFieldDTO[],
  extra: Partial<ResponseSpec> = {},
  calculationOrder: FormSnapshot['calculationOrder'] = [],
): ResponseSpec => ({
  path: 'forms.list',
  value: snapshot(
    fields,
    fields.flatMap((field) =>
      field.widgets.flatMap((widget) =>
        widget.page && widget.objectNumber > 0
          ? [
              completeRow(widget.objectNumber, field.ref, widget.page.objectNumber, undefined, {
                fieldFamily: field.family,
              }),
            ]
          : [],
      ),
    ),
    calculationOrder,
  ),
  ...extra,
});
export const tree = (root: PdfActionNode | null, incomplete = false): PdfActionTree => ({
  root,
  incomplete,
  warningFlags: 0,
  warnings: incomplete ? ['incomplete'] : [],
});
export const js = (script: string, next: PdfActionNode[] = []): PdfActionNode => ({
  type: 'javascript',
  subtype: 'JavaScript',
  script,
  next,
});
export const resetNode = (fields: PdfActionTargetRef[] | null, exclude = false): PdfActionNode => ({
  type: 'reset-form',
  subtype: 'ResetForm',
  fields,
  exclude,
  next: [],
});
export const byName = (name: string): PdfActionTargetRef => ({ kind: 'name', name });
export const byNumber = (objectNumber: number): PdfActionTargetRef => ({
  kind: 'objectNumber',
  objectNumber,
});
export const advance = (ms: number): Step => ({ op: 'advance', ms });

export const doc = (
  responses: ResponseSpec[],
  over: Partial<FakeDocumentSpec> = {},
): FakeDocumentSpec =>
  currentDoc(
    [
      ...responses,
      ...(responses.some((response) => response.path === 'actions.get')
        ? []
        : [
            {
              path: 'actions.get',
              value: { nameTreeScripts: [], openAction: null, openDestination: null },
            },
          ]),
    ],
    over,
  );
let restoreHost: (() => void) | null = null;
onCaseEnd(() => {
  restoreHost?.();
  restoreHost = null;
});
registerPlugin('form-script-diagnostics', {
  factory: (_, env) => {
    if (restoreHost) throw new Error('install scripting diagnostics once per scenario');
    const previousWarn = console.warn,
      previousError = console.error;
    restoreHost = () => {
      console.warn = previousWarn;
      console.error = previousError;
    };
    console.warn = (...args: unknown[]) => {
      if (typeof args[0] === 'string' && args[0].startsWith('[form]'))
        env.log('console.warn', args);
      else previousWarn(...args);
    };
    console.error = (...args: unknown[]) => {
      if (typeof args[0] === 'string' && args[0].startsWith('[actions]'))
        env.log('console.error', args);
      else previousError(...args);
    };
    return definePlugin({
      id: 'form-script-diagnostics',
      token: env.token('form-script-diagnostics'),
      create: () => ({ api: {} }),
    });
  },
});
export function formScenario(
  name: string,
  covers: string,
  form: FormSnapshot,
  steps: Step[],
  responses: ResponseSpec[] = [],
  sandbox?: SandboxSpec | null,
  annotation = false,
  over: Partial<FakeDocumentSpec> = {},
): ScenarioCase {
  const config = { validation: sandbox === undefined ? 'none' : 'scripts' } satisfies FormConfig;
  return {
    name,
    covers,
    plugins: [
      { factory: 'form-script-diagnostics' },
      { factory: 'form-live-diagnostics' },
      { factory: 'interaction' },
      {
        factory: 'form-actions-current',
        config: {
          openSequence: 'off',
          ...(sandbox === undefined ? {} : { javascript: { enabled: true, sandbox } }),
        } satisfies HostConfig,
      },
      ...(annotation ? [{ factory: 'annotation-live' }] : []),
      { factory: 'form-current', config },
    ],
    engine: {
      documents: {
        a: doc(
          [
            currentList(form),
            { path: 'page.annotations.list', value: { annotations: [] } },
            ...(annotation ? [{ path: 'annotations.list', value: { annotations: [] } }] : []),
            ...responses,
          ],
          over,
        ),
      },
    },
    snapshot: ['form::a'],
    steps: [...boot, ...steps],
  };
}

import type { FormHostCapability } from '../../../../packages/plugin/form/src/host-contract';
import type { ActionContext, SubmitIntent } from '../../../../packages/plugin/actions/src/contract';
type FormMethod = {
  [K in keyof FormHostCapability]: FormHostCapability[K] extends (...args: never[]) => unknown
    ? K
    : never;
}[keyof FormHostCapability];
type FormArgs<K extends FormMethod> = K extends `on${string}`
  ? [Scripted]
  : FormHostCapability[K] extends (...args: infer Args) => unknown
    ? Args
    : never;
export function call(
  method: 'resolveSubmitDataset',
  args: [SubmitIntent, ActionContext, Scripted],
  document?: string,
): Step;
export function call<K extends FormMethod>(method: K, args?: FormArgs<K>, document?: string): Step;
export function call(
  method: keyof FormHostCapability,
  args: readonly unknown[] = [],
  document = 'a',
): Step {
  return rawFormCall(method, args, document);
}
