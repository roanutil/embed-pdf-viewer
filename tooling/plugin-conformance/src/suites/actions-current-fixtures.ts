/** Current action inputs. Legacy actions/link fixtures remain separate. */
import type {
  Annotation,
  FormSubmissionReceipt,
  DocumentActionsSnapshot,
  FormSnapshot,
  PageLayout,
  PdfActionNode,
  PdfActionTargetRef,
  PdfActionTree,
  SubmitFormPayload,
} from '@embedpdf/engine-core/runtime';
import type { ScriptIdentity } from '../../../../packages/core/acrojs/src/index';
import { actionsPlugin } from '../../../../packages/plugin/actions/src/actions.plugin';
import {
  ActionsToken,
  type AnnotCommitResult,
  type FormCommitSinkResult,
} from '../../../../packages/plugin/actions/src/host-contract';
import type {
  ActionsConfig,
  ActionContext,
  ActionSubmitRequest,
  ActionTrigger,
  PdfAnnotationEventKind,
  DocumentTriggerEvent,
  ActionTreeSource,
} from '../../../../packages/plugin/actions/src/contract';
import { registerPlugin, type PluginEnv, type Step } from '../scenario';
import type { Suite } from '../vectors';
export const suites: Suite[] = [];
import type { FakeDocumentSpec, ResponseSpec } from '../fake-engine';
import {
  scriptedCallback,
  scriptedSandboxFactory,
  type AnswerSpec,
  type SandboxSpec,
} from './acrojs';
import { pr, objRef, square } from './annotation-current-fixtures';
import { layout } from './annotation-live-fixtures';
import { text, placed, snapshot } from './form-current-fixtures';
export { pr } from './annotation-current-fixtures';
export type Scripted = AnswerSpec & { readonly $fn: string };
export interface CurrentJavaScript {
  readonly enabled: boolean;
  /** null omits the factory override; enabled keeps its actual value. */
  readonly sandbox?: SandboxSpec | null;
  readonly identity?: Partial<ScriptIdentity> | Scripted;
  readonly fileName?: Scripted;
  readonly now?: Scripted;
  readonly utcOffsetMinutes?: Scripted;
  readonly randomSeed?: Scripted;
  readonly budget?: NonNullable<ActionsConfig['javascript']>['budget'];
  readonly maxScriptNodesPerDispatch?: number;
}
export type CurrentConfig = Omit<ActionsConfig, 'javascript'> & {
  readonly javascript?: CurrentJavaScript;
};
export function createCurrentActionsConfig(
  config: CurrentConfig | undefined,
  env: PluginEnv,
): ActionsConfig | undefined {
  if (!config) return undefined;
  const { javascript, ...rest } = config;
  if (!javascript) return rest;
  // Callback answers are an unknown host boundary, narrowed separately by each typed port.
  const callback = <Result>(spec: Scripted): (() => Result) =>
    scriptedCallback(spec.$fn, spec, env) as () => Result;
  const identity = javascript.identity;
  return {
    ...rest,
    javascript: {
      enabled: javascript.enabled,
      ...(javascript.sandbox
        ? { sandboxFactory: scriptedSandboxFactory(javascript.sandbox, env) }
        : {}),
      ...(identity === undefined
        ? {}
        : { identity: '$fn' in identity ? callback<Partial<ScriptIdentity>>(identity) : identity }),
      ...(javascript.fileName ? { fileName: callback<string>(javascript.fileName) } : {}),
      ...(javascript.now ? { now: callback<number>(javascript.now) } : {}),
      ...(javascript.utcOffsetMinutes
        ? { utcOffsetMinutes: callback<number>(javascript.utcOffsetMinutes) }
        : {}),
      ...(javascript.randomSeed ? { randomSeed: callback<number>(javascript.randomSeed) } : {}),
      ...(javascript.budget ? { budget: javascript.budget } : {}),
      ...(javascript.maxScriptNodesPerDispatch === undefined
        ? {}
        : { maxScriptNodesPerDispatch: javascript.maxScriptNodesPerDispatch }),
    },
  };
}
registerPlugin('actions-current', {
  factory: (config, env) =>
    actionsPlugin(createCurrentActionsConfig(config as CurrentConfig | undefined, env)),
  tokens: [ActionsToken],
});
export const plugins = (config?: CurrentConfig) => [
  { factory: 'actions-current', ...(config === undefined ? {} : { config }) },
];
export const page = (
  number: number,
  index: number,
  actions?: PageLayout['actions'],
): PageLayout => ({ ...layout(index, number), ...(actions ? { actions } : {}) });
export const FORMS_EMPTY: ResponseSpec = { path: 'forms.list', value: snapshot([]) };
export const ANNOTS_EMPTY: ResponseSpec = {
  path: 'page.annotations.list',
  value: { annotations: [] },
};
export const catalog = (
  over: Partial<DocumentActionsSnapshot> = {},
  extra: Partial<ResponseSpec> = {},
): ResponseSpec => ({
  path: 'actions.get',
  value: {
    nameTreeScripts: [],
    openAction: null,
    openDestination: null,
    ...over,
  } satisfies DocumentActionsSnapshot,
  ...extra,
});
export const doc = (over: Partial<FakeDocumentSpec> = {}): FakeDocumentSpec => ({
  pages: [page(1, 0), page(2, 1)],
  security: { allows: '*' },
  ...over,
  responses: [...(over.responses ?? []), FORMS_EMPTY, ANNOTS_EMPTY, catalog()],
});
export const engineWith = (over: Partial<FakeDocumentSpec> = {}) => ({
  documents: { a: doc(over) },
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
export const goto = (number: number, next: PdfActionNode[] = []): PdfActionNode => ({
  type: 'goto',
  subtype: 'GoTo',
  destination: { kind: 'fit', page: pr(number) },
  next,
});
export const uri = (value: string, next: PdfActionNode[] = [], isMap = false): PdfActionNode => ({
  type: 'uri',
  subtype: 'URI',
  uri: value,
  isMap,
  next,
});
export const named = (name: string, next: PdfActionNode[] = []): PdfActionNode => ({
  type: 'named',
  subtype: 'Named',
  name,
  next,
});
export const hide = (
  targets: PdfActionTargetRef[],
  hidden = true,
  next: PdfActionNode[] = [],
): PdfActionNode => ({ type: 'hide', subtype: 'Hide', targets, hide: hidden, next });
export const reset = (
  fields: PdfActionTargetRef[] | null,
  exclude = false,
  next: PdfActionNode[] = [],
): PdfActionNode => ({ type: 'reset-form', subtype: 'ResetForm', fields, exclude, next });
type BareType = Extract<
  PdfActionNode,
  {
    type:
      | 'launch'
      | 'goto-remote'
      | 'goto-embedded'
      | 'sound'
      | 'movie'
      | 'import-data'
      | 'rendition'
      | 'thread'
      | 'set-ocg-state'
      | 'transition'
      | 'goto-3d-view'
      | 'unknown';
  }
>['type'];
export const bare = (type: BareType, filePath = '', next: PdfActionNode[] = []): PdfActionNode => {
  if (type === 'launch' || type === 'goto-remote' || type === 'goto-embedded')
    return { type, filePath, subtype: type, next };
  return { type, subtype: type, next };
};
/** Deliberately malformed type witnesses, separate from valid DTO construction. */
export const malformedTree = (type: 'print' | 'bogus') => ({
  root: { type, subtype: type, next: [] },
  incomplete: false,
  warningFlags: 0,
  warnings: [],
});
export const byName = (name: string): PdfActionTargetRef => ({ kind: 'name', name });
export const byNumber = (objectNumber: number): PdfActionTargetRef => ({
  kind: 'objectNumber',
  objectNumber,
});
export const payload = (over: Partial<SubmitFormPayload> = {}): SubmitFormPayload => ({
  url: 'https://home.test/submit',
  fields: null,
  flags: {
    raw: 0,
    exclude: false,
    includeNoValueFields: false,
    format: 'fdf',
    method: 'post',
    submitCoordinates: false,
    includeAppendSaves: false,
    includeAnnotations: false,
    canonicalFormat: false,
    exclNonUserAnnots: false,
    exclFKey: false,
    embedForm: false,
  },
  ...over,
});
export const submit = (payload?: SubmitFormPayload, next: PdfActionNode[] = []): PdfActionNode => ({
  type: 'submit-form',
  subtype: 'SubmitForm',
  ...(payload === undefined ? {} : { payload }),
  next,
});
export const aref = (page: number, number: number) => objRef(number, page);
export const annot = (page: number, number: number, actions?: Annotation['actions']): Annotation =>
  square(number, { ref: aref(page, number), page: pr(page), ...(actions ? { actions } : {}) });
export const annotations = (
  pon: number,
  annotations: Annotation[],
  extra: Partial<ResponseSpec> = {},
): ResponseSpec => ({ path: 'page.annotations.list', pon, value: { annotations }, ...extra });
export const formField = (
  name: string,
  number: number,
  widgets: number[] = [],
  over: Partial<ReturnType<typeof text>> = {},
) =>
  text(
    number,
    name,
    `${name}-value`,
    over,
    widgets.map((number) => placed(number)),
  );
export const forms = (
  fields: FormSnapshot['fields'],
  extra: Partial<ResponseSpec> = {},
): ResponseSpec => ({ path: 'forms.list', value: snapshot(fields), ...extra });
export const USER: ActionContext = {
  origin: 'user',
  source: { kind: 'api' },
  event: { scope: 'activate' },
};
export const HOVER: ActionContext = {
  origin: 'hover',
  source: { kind: 'api' },
  event: { scope: 'annotation', name: 'cursorEnter' },
};
export const LIFECYCLE: ActionContext = {
  origin: 'lifecycle',
  source: { kind: 'api' },
  event: { scope: 'document', name: 'open' },
};
export const FORM_OK = () => ({ returns: { status: 'applied' } satisfies FormCommitSinkResult });
export const RESOLVED: ActionSubmitRequest = {
  url: 'https://home.test/submit',
  method: 'post',
  format: 'fdf',
  flagsRaw: 0,
  entries: [{ name: 'plain', value: 'visible' }],
  origin: 'user',
  event: { scope: 'activate' },
};
export const resolverEcho = (
  entries = RESOLVED.entries,
  extra: Partial<ActionSubmitRequest> = {},
) => ({
  $fn: 'resolver',
  resolves: { ...RESOLVED, entries, ...extra } satisfies ActionSubmitRequest,
});
export const activate = (page: number): ActionTrigger => ({
  scope: 'activate',
  ref: aref(page, 1),
  page: pr(page),
});
export const annotationEvent = (event: PdfAnnotationEventKind, page: number): ActionTrigger => ({
  scope: 'annotation',
  event,
  ref: aref(page, 1),
  page: pr(page),
});
export const pageEvent = (
  event: 'open' | 'close' | 'visible' | 'invisible',
  page: number,
): ActionTrigger => ({ scope: 'page', event, page: pr(page) });
export const docEvent = (event: DocumentTriggerEvent): ActionTrigger => ({
  scope: 'document',
  event,
});

export const readTree = (source: ActionTreeSource, document = 'a'): Step => ({
  op: 'call',
  target: { token: 'actions', doc: document },
  method: 'getActionTree',
  args: [source],
});

export const receipt = (submissionId: string): FormSubmissionReceipt => ({
  submissionId,
  receivedAt: '1970-01-01T00:00:02.000Z',
});

export const ANNOT_OK = (numbers: number[]) => ({
  returns: {
    results: numbers.map((annotObjectNumber) => ({
      annotObjectNumber,
      status: 'applied' as const,
    })),
  } satisfies AnnotCommitResult,
});
export const formFailure = (error?: string) => ({
  returns: {
    status: 'failed',
    ...(error === undefined ? {} : { error }),
  } satisfies FormCommitSinkResult,
});
