/** Current form read records. Inputs are complete engine DTOs before invocation. */
import {
  formWidget,
  type FormFieldDTO,
  type FormFieldRef,
  type FormSnapshot,
  type FormWidget,
  type TextFieldDTO,
  type CheckboxFieldDTO,
  type RadioFieldDTO,
  type ComboBoxFieldDTO,
  type ListBoxFieldDTO,
  type PushButtonFieldDTO,
  type SignatureFieldDTO,
  type UnknownFieldDTO,
  type WidgetAnnotation,
  type ChangeResult,
  type DocumentEvent,
  type FormMutationMeta,
} from '@embedpdf/engine-core/runtime';
import { formPlugin } from '../../../../packages/plugin/form/src/form.plugin';
import { FormToken } from '../../../../packages/plugin/form/src/host-contract';
import type { FormConfig } from '../../../../packages/plugin/form/src/contract';
import { registerPlugin, onCaseEnd, type Step } from '../scenario';
import { installCaseEntropy, restoreCaseEntropy } from '../case-entropy';
import type { ResponseSpec } from '../fake-engine';
import type { Suite } from '../vectors';
import { widget, objRef, pr, operationAt, LOCAL } from './annotation-live-fixtures';
export { objRef, pr } from './annotation-live-fixtures';
export const suites: Suite[] = [];

registerPlugin('form-current', {
  factory: (config) => {
    installCaseEntropy();
    try {
      return formPlugin(config as FormConfig | undefined);
    } catch (error) {
      restoreCaseEntropy();
      throw error;
    }
  },
  tokens: [FormToken],
});
onCaseEnd(restoreCaseEntropy);
export const ref = (objectNumber: number): FormFieldRef => ({ kind: 'objectNumber', objectNumber });
export const fqn = (name: string): FormFieldRef => ({ kind: 'fqn', name });
export const placed = (objectNumber: number, page: number | null = 1): FormWidget =>
  formWidget(objectNumber, page === null ? null : pr(page));

const base = (n: number, name: string, widgets: FormWidget[]) => ({
  ref: n > 0 ? ref(n) : fqn(name),
  name,
  origin: 'acroform' as const,
  readOnly: false,
  required: false,
  noExport: false,
  alternateName: null,
  mappingName: null,
  valueEntry: { kind: 'none' as const },
  defaultValueEntry: { kind: 'none' as const },
  groupId: null,
  createdBy: null,
  createdAt: null,
  filledBy: null,
  filledByName: null,
  filledAt: null,
  importedBy: null,
  widgets,
});
export const text = (
  n: number,
  name: string,
  value = '',
  over: Partial<TextFieldDTO> = {},
  widgets = [placed(n + 100)],
): TextFieldDTO => ({
  ...base(n, name, widgets),
  family: 'text',
  value,
  defaultValue: '',
  maxLength: null,
  multiline: false,
  password: false,
  comb: false,
  valueEntry: value === '' ? { kind: 'none' } : { kind: 'scalar', value },
  ...over,
});
export const checkbox = (
  n: number,
  name: string,
  checked = false,
  over: Partial<CheckboxFieldDTO> = {},
): CheckboxFieldDTO => ({
  ...base(n, name, []),
  family: 'checkbox',
  checked,
  exportValue: 'Yes',
  widgets: [{ ...placed(n + 100), onState: 'Yes', exportValue: 'Yes', checked }],
  valueEntry: checked ? { kind: 'scalar', value: 'Yes' } : { kind: 'none' },
  ...over,
});
export const radio = (
  n: number,
  name: string,
  widgets: RadioFieldDTO['widgets'],
  over: Partial<RadioFieldDTO> = {},
): RadioFieldDTO => ({
  ...base(n, name, widgets),
  family: 'radio',
  widgets,
  value: 'Off',
  radiosInUnison: false,
  noToggleToOff: false,
  ...over,
});
export const OPTIONS = [
  { label: 'Red', value: 'r', selected: false },
  { label: 'Green', value: 'g', selected: false },
];
export const combobox = (
  n: number,
  name: string,
  value = '',
  over: Partial<ComboBoxFieldDTO> = {},
): ComboBoxFieldDTO => ({
  ...base(n, name, [placed(n + 100)]),
  family: 'combobox',
  value,
  defaultValue: '',
  edit: false,
  options: OPTIONS.map((option) => ({ ...option, selected: option.value === value })),
  valueEntry: value === '' ? { kind: 'none' } : { kind: 'scalar', value },
  ...over,
});
export const listbox = (
  n: number,
  name: string,
  selectedValues: string[] = [],
  over: Partial<ListBoxFieldDTO> = {},
): ListBoxFieldDTO => ({
  ...base(n, name, [placed(n + 100)]),
  family: 'listbox',
  selectedValues,
  defaultValue: [],
  multiSelect: true,
  options: OPTIONS.map((option) => ({
    ...option,
    selected: selectedValues.includes(option.value),
  })),
  topIndex: 0,
  valueEntry: selectedValues.length ? { kind: 'array', values: selectedValues } : { kind: 'none' },
  ...over,
});
export const pushbutton = (
  n: number,
  name: string,
  over: Partial<PushButtonFieldDTO> = {},
): PushButtonFieldDTO => ({
  ...base(n, name, [placed(n + 100)]),
  family: 'pushbutton',
  ...over,
});
export const signature = (
  n: number,
  name: string,
  over: Partial<SignatureFieldDTO> = {},
): SignatureFieldDTO => ({
  ...base(n, name, [placed(n + 100)]),
  family: 'signature',
  ...over,
});
export const unknown = (
  n: number,
  name: string,
  over: Partial<UnknownFieldDTO> = {},
): UnknownFieldDTO => ({
  ...base(n, name, [placed(n + 100)]),
  family: 'unknown',
  rawValue: '',
  ...over,
});
export const row = (
  n: number,
  field: FormFieldRef | null,
  page = 1,
  rect = { x: 0, y: 0, width: 20, height: 10 },
  over: Partial<WidgetAnnotation> = {},
): WidgetAnnotation =>
  widget(n, 'text', { ref: objRef(n, page), page: pr(page), field, rect, ...over });
/** Rows are supplied independently from the field relationship refs. */
export const snapshot = (
  fields: FormFieldDTO[],
  widgets: WidgetAnnotation[] = [],
  calculationOrder: FormSnapshot['calculationOrder'] = [],
): FormSnapshot => ({
  formKind: 'acroform',
  needsAppearances: false,
  fields,
  widgets,
  calculationOrder,
});
export const list = (value: FormSnapshot, extra: Partial<ResponseSpec> = {}): ResponseSpec => ({
  path: 'forms.list',
  value,
  ...extra,
});
export const call = (method: string, args: readonly unknown[] = [], doc = 'a'): Step => ({
  op: 'call',
  target: { token: 'form', doc },
  method,
  args,
});

/** A static, typed value-set answer; never examines a runtime request. */
export function valueSet(
  n: number,
  field: FormFieldDTO,
  widgets: WidgetAnnotation[] = [],
  ms = 0,
): { value: ChangeResult; events: DocumentEvent[] } {
  const opId = operationAt(n, ms);
  const pages = [...new Map(widgets.map((w) => [w.page.objectNumber, w.page])).values()];
  const meta: FormMutationMeta = {
    affectedPages: pages,
    cacheDelta: null,
    opId,
    undoable: true,
    changedFields: [field.ref],
    changedWidgets: field.widgets,
    redrawnWidgets: field.widgets,
  };
  const value: ChangeResult = {
    items: [{ type: 'forms.setValue', field, widgets, meta }],
    meta: { affectedPages: pages, cacheDelta: null, opId, undoable: true },
  };
  const events: DocumentEvent[] = [
    {
      type: 'forms.valueSet',
      field,
      widgets,
      meta,
      origin: { ...LOCAL, tx: { id: opId, index: 0, count: 1 } },
    },
  ];
  return { value, events };
}
