/** Current detached placement inputs. All engine answers are authored before invocation. */
import { formEffectOps } from '@embedpdf/engine-core/runtime';
import type {
  AttachmentFileSource,
  Identity,
  DocumentActionsSnapshot,
  FormFieldDTO,
  FormEffect,
  PageFlattenResult,
  PageLayout,
  PageImageResult,
} from '@embedpdf/engine-core/runtime';
import type { ActionsConfig } from '../../../../packages/plugin/actions/src/contract';
import type { AnnotationHostCapability } from '../../../../packages/plugin/annotation/src/host-contract';
import type { FakeDocumentSpec, ResponseSpec } from '../fake-engine';
import type { PluginSpec, Step } from '../scenario';
import type { Suite } from '../vectors';
import { canonical } from '../canonical';
import {
  STAMP,
  assetDoc,
  assetPage,
  bytes,
  image,
  mutationMeta,
  pdf,
  type StampCurrentInput,
} from './stamp-current-fixtures';
import { doc, hydrate, pr } from './annotation-live-fixtures';
import { valuesAnswer, apply as formApply } from './form-live-fixtures';
import { WITH_ANNOTATION } from './form-live-fixtures';
import { list as formList, snapshot, text, placed, ref } from './form-current-fixtures';
import {
  list,
  js,
  tree,
  type StandaloneConfig,
  type SandboxSpec,
  act,
  fnSpec,
} from './form-script-fixtures';
export const suites: Suite[] = [];
export { pr, assetPage, act, fnSpec };
export const IDENTITY = {
  userId: 'alex',
  groupId: 'EmbedPDF',
  displayName: 'Alex Morgan',
  organization: 'EmbedPDF',
  email: 'alex@example.test',
} satisfies Identity;
export const ONE = [assetPage(0, 100)];
export const TWO = [...ONE, assetPage(1, 101)];
export const DATE = (pon = 100, value = '') => text(1, 'date', value, {}, [placed(201, pon)]);
export const setValue = (value: string): FormEffect => ({
  kind: 'setValue',
  ref: ref(1),
  value: { value },
});
export const BOOT_SETS: SandboxSpec = {
  builds: [{ boot: [{ output: { formEffects: [setValue('2026-07-15')] } }] }],
};
export interface PlacementHostInput extends Omit<ActionsConfig, 'javascript'> {
  readonly javascript?: Omit<StandaloneConfig, 'sandbox'> & {
    readonly enabled: boolean;
    readonly sandbox: SandboxSpec | null;
  };
}
export const PLACEMENT = (
  config: StampCurrentInput = { assetEngine: 'instance' },
  host?: PlacementHostInput,
): PluginSpec[] =>
  STAMP(config, [
    { factory: 'form-script-diagnostics' },
    ...WITH_ANNOTATION,
    ...(host ? [{ factory: 'form-actions-current', config: host }] : []),
  ]);
export const scripting = (over: Partial<StandaloneConfig> = {}): PlacementHostInput => ({
  openSequence: 'off',
  javascript: { enabled: true, sandbox: BOOT_SETS, ...over },
});
export const target = (over: Partial<FakeDocumentSpec> = {}): FakeDocumentSpec =>
  doc(
    [
      hydrate(),
      formList(snapshot([])),
      { path: 'page.annotations.list', value: { annotations: [] } },
      {
        path: 'actions.get',
        value: {
          nameTreeScripts: [],
          openAction: null,
          openDestination: null,
        } satisfies DocumentActionsSnapshot,
      },
    ],
    { pages: [assetPage(0, 1, 600, 800)], security: { allows: '*', identity: IDENTITY }, ...over },
  );
const BOOT_SCRIPTS: DocumentActionsSnapshot = {
  nameTreeScripts: [
    { name: 'stamp', action: tree(js('this.getField("date").value = util.printd(...);')) },
  ],
  openAction: null,
  openDestination: null,
};
export function dynamicDoc(
  input: {
    pages?: PageLayout[];
    fields?: FormFieldDTO[];
    responses?: ResponseSpec[];
    lacks?: string[];
    flatten?: PageFlattenResult;
    selected?: number;
    formValue?: string;
  } = {},
): FakeDocumentSpec {
  const pages = input.pages ?? ONE;
  const selected = pr(input.selected ?? pages[0].ref.objectNumber);
  const formValue = input.formValue ?? '2026-07-15';
  const formWrite = formApply(valuesAnswer(1, [DATE(selected.objectNumber, formValue)]), {
    args: [{ ops: formEffectOps([setValue(formValue)]) }],
  });
  return assetDoc(pages, {
    responses: [
      ...(input.responses ?? []),
      list(input.fields ?? [DATE()]),
      { path: 'actions.get', value: BOOT_SCRIPTS },
      formWrite,
      {
        path: 'pages.flatten',
        args: [[selected], { usage: 'display' }],
        value:
          input.flatten ??
          ({
            pages: [selected],
            usage: 'display',
            results: [{ page: selected, status: 'applied' }],
            meta: mutationMeta(pages),
          } satisfies PageFlattenResult),
      },
      ...pages.map(
        (p): ResponseSpec => ({
          path: 'pages.extract',
          args: [[p.ref]],
          value: canonical(pdf(`flat-${p.ref.objectNumber}`)),
        }),
      ),
      {
        path: 'page.render.image',
        value: canonical({
          ...image(100),
          source: { kind: 'bytes', bytes: bytes('flat-preview') },
        } satisfies PageImageResult),
      },
    ],
    lacks: input.lacks,
  });
}
type Method = {
  [K in keyof AnnotationHostCapability]-?: AnnotationHostCapability[K] extends (
    ...args: never[]
  ) => unknown
    ? K
    : never;
}[keyof AnnotationHostCapability];
interface CallbackInput {
  readonly $fn: string;
  readonly throws?: { readonly name?: string; readonly message: string };
  readonly resolves?: AttachmentFileSource | null;
  readonly defer?: string;
  readonly seq?: readonly {
    readonly resolves?: AttachmentFileSource | null;
    readonly defer?: string;
  }[];
}
type Args<K extends Method> = K extends `on${string}` | 'setFilePickerProvider'
  ? readonly [CallbackInput]
  : AnnotationHostCapability[K] extends (...args: infer A) => unknown
    ? A
    : never;
export function annotation<K extends Method>(method: K, args?: Args<K>, document?: string): Step;
export function annotation(method: Method, args: readonly unknown[] = [], document = 'a'): Step {
  return {
    op: 'call',
    target: { token: 'annotation', doc: document },
    method,
    args: args.map((arg) => canonical(arg)),
  };
}
export const observers = (bad?: 'error' | 'ui'): Step[] => [
  act('setUiAdapter', [
    {
      print: fnSpec('print'),
      alert: {
        $fn: 'onUiEffect',
        ...(bad === 'ui'
          ? { seq: [{}, { throws: { name: 'Error', message: 'observer broke' } }] }
          : {}),
      },
    },
  ]),
  act('onScriptDiagnosticReported', [fnSpec('onDiagnostic')]),
  act('onScriptFailed', [
    {
      $fn: 'onError',
      ...(bad === 'error'
        ? { throws: { name: 'TypeError', message: 'observer is not ready' } }
        : {}),
    },
  ]),
];
