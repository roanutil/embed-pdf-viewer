/** Direct current form scenario inputs and static confirmed answers. No runtime request translation. */
import { formResetFacts } from '@embedpdf/engine-core/runtime';
import type {
  ChangeResult,
  DocumentEvent,
  FormFieldDTO,
  FormFieldRef,
  FormWidget,
  FormMutationMeta,
  FormSnapshot,
  WidgetAnnotation,
} from '@embedpdf/engine-core/runtime';
import type { FakeDocumentSpec, ResponseSpec } from '../fake-engine';
import { registerPlugin, onCaseEnd, type ScenarioCase, type Step } from '../scenario';
import { definePlugin } from '@embedpdf/core';
import { doc, boot, LOCAL, REMOTE, operationAt } from './annotation-live-fixtures';
import type { FormConfig } from '../../../../packages/plugin/form/src/contract';
import type { FormHostCapability } from '../../../../packages/plugin/form/src/host-contract';
import { list, snapshot, row as currentRow, call as currentCall } from './form-current-fixtures';
export * from './form-current-fixtures';
export {
  boot,
  doc,
  open,
  settle,
  fail,
  emit,
  hydrate,
  objRef,
  LOCAL,
  REMOTE,
  operationAt,
} from './annotation-live-fixtures';
export { ix, tool, sample } from './annotation-live-inputs';
let diagnosticsActive = false;
let previousConsoleError = console.error;
const restoreDiagnostics = () => {
  if (diagnosticsActive) {
    console.error = previousConsoleError;
    diagnosticsActive = false;
  }
};
onCaseEnd(restoreDiagnostics);
registerPlugin('form-live-diagnostics', {
  factory: (_, env) => {
    if (diagnosticsActive) throw new Error('[form-live] diagnostics host installed twice');
    const previous = console.error;
    previousConsoleError = previous;
    diagnosticsActive = true;
    console.error = (...args: unknown[]) => {
      if (typeof args[0] === 'string' && args[0].startsWith('[form]'))
        env.log('console.error', args);
      else previous(...args);
    };
    try {
      return definePlugin({
        id: 'form-live-diagnostics',
        token: env.token('form-live-diagnostics'),
        create: () => ({ api: {} }),
      });
    } catch (error) {
      restoreDiagnostics();
      throw error;
    }
  },
});
/** Explicit unrotated page-space row; the rect and engine box are both supplied to the DTO constructor. */
export const row = (
  n: number,
  field: FormFieldRef | null,
  page = 1,
  rect = { x: 0, y: 0, width: 20, height: 10 },
  over: Partial<WidgetAnnotation> = {},
): WidgetAnnotation => currentRow(n, field, page, rect, { box: rect, ...over });
const LIVE_CONFIG = { validation: 'none' } satisfies FormConfig;
export const FORM_ONLY = [
  { factory: 'form-live-diagnostics' },
  { factory: 'interaction' },
  { factory: 'form-current', config: LIVE_CONFIG },
];
/** Fixed host order: diagnostics installs before annotation, and its case-end reset runs after annotation. */
export const WITH_ANNOTATION = [
  { factory: 'form-live-diagnostics' },
  { factory: 'interaction' },
  { factory: 'annotation-live' },
  { factory: 'form-current', config: LIVE_CONFIG },
];
export const anno = (method: string, args: readonly unknown[] = []): Step => ({
  op: 'call',
  target: { token: 'annotation', doc: 'a' },
  method,
  args,
});
export const close = (): Step => ({
  op: 'call',
  target: 'documents',
  method: 'close',
  args: ['a'],
});
export const fnSpec = (name: string) => ({ $fn: name });
export const refused = (message = 'intended refusal', code = 'Refused'): ResponseSpec => ({
  path: 'apply',
  times: 1,
  error: { name: 'EngineError', message, code },
});
export const mutationMeta = (
  n: number,
  fields: readonly FormFieldDTO[],
  widgets: readonly WidgetAnnotation[] = [],
  undoable = true,
): FormMutationMeta => ({
  affectedPages: [...new Map(widgets.map((w) => [w.page.objectNumber, w.page])).values()],
  cacheDelta: null,
  opId: operationAt(n),
  undoable,
  changedFields: fields.map((f) => f.ref),
  changedWidgets: fields.flatMap((f) => f.widgets),
  redrawnWidgets: fields.flatMap((f) => f.widgets),
});
type FormMethod = {
  [K in keyof FormHostCapability]: FormHostCapability[K] extends (...args: never[]) => unknown
    ? K
    : never;
}[keyof FormHostCapability];
type FormArgs<K extends FormMethod> = K extends `on${string}`
  ? readonly [{ readonly $fn: string; readonly throws?: { readonly message: string } }]
  : FormHostCapability[K] extends (...args: infer Args) => unknown
    ? Args
    : never;
/** Current data arguments are checked at construction; callback literals describe the runner's host callable. */
export function call<K extends FormMethod>(method: K, args?: FormArgs<K>, document?: string): Step;
export function call(method: FormMethod, args: readonly unknown[] = [], document = 'a'): Step {
  return currentCall(method, args, document);
}
export type Answer = { value: ChangeResult; events: DocumentEvent[] };
export const fieldAnswer = (
  type: 'forms.create' | 'forms.update' | 'forms.addWidget' | 'forms.removeWidget',
  n: number,
  field: FormFieldDTO,
  widgets: WidgetAnnotation[] = [],
): Answer => {
  const meta = mutationMeta(n, [field], widgets);
  const item = { field, widgets, meta };
  const origin = { ...LOCAL, tx: { id: meta.opId, index: 0, count: 1 } };
  const events: DocumentEvent[] = [];
  if (type === 'forms.create') events.push({ type: 'forms.created', ...item, origin });
  if (type === 'forms.update') events.push({ type: 'forms.updated', ...item, origin });
  if (type === 'forms.addWidget') events.push({ type: 'forms.widgetAdded', ...item, origin });
  if (type === 'forms.removeWidget') events.push({ type: 'forms.widgetRemoved', ...item, origin });
  return { value: { items: [{ type, ...item }], meta }, events };
};
export const deleteAnswer = (
  n: number,
  field: FormFieldDTO,
  widgets: WidgetAnnotation[] = [],
): Answer => {
  const meta = mutationMeta(n, [field], widgets);
  return {
    value: { items: [{ type: 'forms.delete', meta }], meta },
    events: [
      {
        type: 'forms.deleted',
        deleted: field.ref,
        meta,
        origin: { ...LOCAL, tx: { id: meta.opId, index: 0, count: 1 } },
      },
    ],
  };
};
export const resetAnswer = (
  n: number,
  fields: FormFieldDTO[],
  widgets: WidgetAnnotation[] = [],
  undoable = true,
): Answer => {
  const meta = mutationMeta(n, fields, widgets, undoable);
  return {
    value: { items: [{ type: 'forms.reset', fields, widgets, meta }], meta },
    events: formResetFacts({ fields, widgets, meta }).map((fact, index) => ({
      type: 'forms.valueSet',
      ...fact,
      origin: { ...LOCAL, tx: { id: meta.opId, index, count: fields.length } },
    })),
  };
};
export const valuesAnswer = (
  n: number,
  fields: FormFieldDTO[],
  widgets: WidgetAnnotation[] = [],
  redrawn: FormWidget[] = fields.flatMap((field) => field.widgets),
): Answer => {
  const meta = { ...mutationMeta(n, fields, widgets), redrawnWidgets: redrawn };
  const own = (field: FormFieldDTO) => ({
    field,
    widgets: widgets.filter(
      (w) => w.field && JSON.stringify(w.field) === JSON.stringify(field.ref),
    ),
    meta: {
      ...mutationMeta(n, [field], widgets),
      redrawnWidgets: redrawn.filter((widget) =>
        field.widgets.some((own) => own.objectNumber === widget.objectNumber),
      ),
    },
  });
  return {
    value: { items: fields.map((field) => ({ type: 'forms.setValue', ...own(field) })), meta },
    events: fields.map((field, index) => ({
      type: 'forms.valueSet',
      ...own(field),
      origin: { ...LOCAL, tx: { id: meta.opId, index, count: fields.length } },
    })),
  };
};
export const apply = (answer: Answer, extra: Partial<ResponseSpec> = {}): ResponseSpec => ({
  path: 'apply',
  times: 1,
  ...answer,
  ...extra,
});
export const valueFact = (
  field: FormFieldDTO,
  widgets: WidgetAnnotation[] = [],
  n = 1,
): Extract<DocumentEvent, { type: 'forms.valueSet' }> => ({
  type: 'forms.valueSet',
  field,
  widgets,
  meta: mutationMeta(n, [field], widgets),
  origin: REMOTE,
});
export function scenario(
  name: string,
  covers: string,
  steps: Step[],
  form: FormSnapshot = snapshot([]),
  responses: ResponseSpec[] = [],
  over: Partial<FakeDocumentSpec> = {},
  annotation = false,
): ScenarioCase {
  return {
    name,
    covers,
    plugins: annotation ? WITH_ANNOTATION : FORM_ONLY,
    engine: {
      documents: {
        a: doc(
          [
            list(form, responses.some((r) => r.path === 'forms.list') ? { times: 1 } : {}),
            ...(annotation ? [{ path: 'annotations.list', value: { annotations: [] } }] : []),
            ...responses,
          ],
          over,
        ),
      },
    },
    snapshot: annotation ? ['form::a', 'annotation::a'] : ['form::a'],
    steps: [...boot, ...steps],
  };
}
