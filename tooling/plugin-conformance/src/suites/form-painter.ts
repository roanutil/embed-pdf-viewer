/**
 * `packages/core/form`: the widget painter. Replayed by
 * `crates/plugins/form-core/tests/conformance.rs`.
 *
 * Distinct from `plugins/form-core.json`, which records the form plugin. This
 * file is the pure painter: what a widget draws for its facts, its text's
 * layout, the scene on the page and the HTML editor's layout.
 *
 * Pure functions, so one function suite. The widget matrix follows the
 * engine's parity gate (`packages/engine/main/test/widget-painter-parity.test.ts`),
 * which checks the same painter against PDFium's appearance generator. Five
 * entries are wrappers, because the shipping values carry closures:
 *
 * - `layoutFontOf(fontFamily, codes)` records the font's data members and
 *   asks `width` for each code unit the case passes.
 * - `layoutVariableText(text, fontFamily, options)` lays out in
 *   `layoutFontOf(fontFamily)`, as every caller does.
 * - `widgetScene(widget, field, shown, placement)` draws the widget with
 *   `widgetDrawing` and places that drawing, as the form plugin does.
 * - `paints(family)` takes the family alone.
 * - `needsBreak(previous, currents)` asks the internal line-break rule for
 *   each current character after `previous`.
 *
 * A font program in a result prints its `load` as `{"$function": true}`.
 */
import type { FormFieldDTO, WidgetAnnotation } from '@embedpdf/engine-core/runtime';
import { STANDARD_FONTS } from '@embedpdf/engine-core/runtime';

import {
  layoutFontOf,
  layoutVariableText,
  paints,
  textEditorLayout,
  widgetDrawing,
  widgetScene,
  type ShownValue,
  type VariableTextOptions,
  type WidgetPlacement,
} from '../../../../packages/core/form/src/index';
import { needsBreak } from '../../../../packages/core/form/src/layout/breaks';

import type { FunctionCase, Suite } from '../vectors';

const functions = {
  layoutFontOf: (fontFamily: string | null | undefined, codes: readonly number[]) => {
    const font = layoutFontOf(fontFamily);
    return (
      font && {
        font: font.font,
        ascent: font.ascent,
        descent: font.descent,
        widths: codes.map((code) => font.width(code)),
      }
    );
  },
  layoutVariableText: (text: string, fontFamily: string, options: VariableTextOptions) =>
    layoutVariableText(text, layoutFontOf(fontFamily)!, options),
  widgetDrawing: (widget: WidgetAnnotation, field: FormFieldDTO, shown?: ShownValue) =>
    widgetDrawing(widget, field, shown),
  widgetScene: (
    widget: WidgetAnnotation,
    field: FormFieldDTO,
    shown: ShownValue,
    placement: WidgetPlacement,
  ) => widgetScene(widgetDrawing(widget, field, shown)!, placement),
  textEditorLayout: (widget: WidgetAnnotation, field: FormFieldDTO, text: string) =>
    textEditorLayout(widget, field, text),
  paints: (family: FormFieldDTO['family']) => paints({ family }),
  needsBreak: (previous: number, currents: readonly number[]) =>
    currents.map((current) => needsBreak(previous, current)),
};

const cases: FunctionCase[] = [];
const add = (fn: keyof typeof functions, label: string, ...args: unknown[]) =>
  cases.push({ name: `${fn}/${label}`, fn, args });

// ── fixtures ──

const LOOK = { color: '#1f6feb', interiorColor: '#f6f8fa', fontColor: '#0d1117' };
const NO_LOOK = { color: null, interiorColor: null, fontColor: null };

const rect = (width: number, height: number) => ({ x: 20, y: 20, width, height });

/** A widget row, as the engine reads it, with the parity gate's colours. */
const widget = (patch: Record<string, unknown> = {}, width = 160, height = 24) => ({
  subtype: 'widget',
  rect: rect(width, height),
  box: rect(width, height),
  rotation: null,
  appearanceState: null,
  strokeWidth: 1,
  borderStyle: 'solid',
  dashArray: null,
  fontFamily: 'helvetica',
  fontSize: 10,
  textAlign: 'left',
  caption: null,
  checkStyle: null,
  ...LOOK,
  ...patch,
});

const text = (value: string, patch: Record<string, unknown> = {}) => ({
  family: 'text',
  value,
  defaultValue: '',
  maxLength: null,
  multiline: false,
  password: false,
  comb: false,
  ...patch,
});

const option = (label: string, value: string, selected = false) => ({ label, value, selected });

const combo = (value: string) => ({
  family: 'combobox',
  value,
  defaultValue: '',
  edit: false,
  options: [option('Alpha', 'a'), option('Beta', 'b')],
});

const LETTERS = ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h'];
const list = (selected: readonly string[], topIndex = 0) => ({
  family: 'listbox',
  selectedValues: [...selected],
  defaultValue: [],
  multiSelect: true,
  options: LETTERS.map((value) =>
    option(`Option ${value.toUpperCase()}`, value, selected.includes(value)),
  ),
  topIndex,
});

const toggle = (family: 'checkbox' | 'radio') => ({ family, widgets: [] });

const ALIGNS = ['left', 'center', 'right'] as const;
const BORDERS = [
  { borderStyle: 'solid', strokeWidth: 1 },
  { borderStyle: 'solid', strokeWidth: 0 },
  { borderStyle: 'solid', strokeWidth: 3 },
  { borderStyle: 'dashed', strokeWidth: 1 },
  { borderStyle: 'dashed', strokeWidth: 2, dashArray: [4, 2] },
  { borderStyle: 'beveled', strokeWidth: 1 },
  { borderStyle: 'inset', strokeWidth: 2 },
  { borderStyle: 'underline', strokeWidth: 1 },
] as const;
const borderLabel = (border: Record<string, unknown>) =>
  `${border.borderStyle} ${border.strokeWidth}${border.dashArray ? ` [${border.dashArray}]` : ''}`;

const SAMPLE = 'Hég, quick €12.50 “fox”';
const PARAGRAPHS =
  'The quick brown fox jumps over the lazy dog, again and again.\nSecond paragraph here\n\nAfter a blank line';

// ── layoutFontOf ──

const PROBES = [0x41, 0x6e, 0x20, 0xe9, 0x20ac, 0x201c, 0x416, 0xd83d, 0, 0x2a, 0x3b1, 0x2701];
for (const font of STANDARD_FONTS) add('layoutFontOf', font, font, PROBES);
add('layoutFontOf', 'null', null, PROBES);
add('layoutFontOf', 'undefined', undefined, PROBES);
for (const name of ['', 'Helvetica', 'Roboto', 'arial', 'helvetica ']) {
  add('layoutFontOf', `not-standard:${name}`, name, PROBES);
}

// ── layoutVariableText ──

const plate = (right: number, top: number, left = 0, bottom = 0) => ({ left, bottom, right, top });
const options = (patch: Record<string, unknown> = {}) => ({
  plate: plate(200, 20),
  fontSize: 10,
  alignment: 0,
  multiline: false,
  limit: 0,
  cells: 0,
  ...patch,
});

add('layoutVariableText', 'plain', 'Hello', 'helvetica', options());
add('layoutVariableText', 'offset-plate', 'Hello', 'times-roman', options({ plate: plate(130, 47, 12, 9) }));
for (const [index, alignment] of [0, 1, 2].entries()) {
  add('layoutVariableText', `align-${ALIGNS[index]}`, 'Wide WWW text', 'helvetica', options({ alignment }));
  add(
    'layoutVariableText',
    `multiline-align-${ALIGNS[index]}`,
    PARAGRAPHS,
    'helvetica',
    options({ plate: plate(146, 86), fontSize: 9, alignment, multiline: true }),
  );
}
for (const height of [10, 20, 40, 1000, 0, 3]) {
  add('layoutVariableText', `fitted-height-${height}`, 'Hello', 'helvetica', options({ plate: plate(200, height), fontSize: 0 }));
}
add('layoutVariableText', 'fitted-wide-text', 'A long value that will not fit in this box at all', 'helvetica', options({ plate: plate(150, 20), fontSize: 0 }));
add('layoutVariableText', 'fitted-multiline', PARAGRAPHS, 'helvetica', options({ plate: plate(146, 86), fontSize: 0, multiline: true }));
add('layoutVariableText', 'fitted-multiline-tiny', PARAGRAPHS, 'courier', options({ plate: plate(40, 10), fontSize: 0, multiline: true }));
add('layoutVariableText', 'fitted-zero-width', 'Hello', 'helvetica', options({ plate: plate(0, 20), fontSize: 0 }));
add('layoutVariableText', 'fitted-empty', '', 'helvetica', options({ fontSize: 0 }));
add('layoutVariableText', 'near-zero-size-fits', 'Hello', 'helvetica', options({ fontSize: 0.00005 }));
add('layoutVariableText', 'long-word', 'Supercalifragilisticexpialidocious word', 'times-roman', options({ plate: plate(76, 56), fontSize: 12, multiline: true }));
add('layoutVariableText', 'one-char-per-line', 'WWW', 'helvetica', options({ plate: plate(5, 56), multiline: true }));
add('layoutVariableText', 'cjk', '日本語のテキストを折り返す。次の文です', 'helvetica', options({ plate: plate(40, 80), multiline: true }));
add('layoutVariableText', 'punctuation', 'one, two; (three) [four] {five} a-b a/b "q" it\'s', 'helvetica', options({ plate: plate(60, 120), multiline: true }));
add('layoutVariableText', 'open-punctuation', 'a ((((b c', 'helvetica', options({ plate: plate(20, 120), multiline: true }));
add('layoutVariableText', 'currency', '$100 €5 £3 ¥7 №9', 'helvetica', options({ plate: plate(24, 120), multiline: true }));
add('layoutVariableText', 'latin-digits', 'abc123def456 ghi789', 'courier', options({ plate: plate(40, 80), multiline: true }));
add('layoutVariableText', 'line-breaks', 'a\r\nb\rc\nd\n\re', 'helvetica', options({ plate: plate(100, 100), multiline: true }));
add('layoutVariableText', 'line-breaks-single-line', 'a\r\nb\rc\nd', 'helvetica', options());
add('layoutVariableText', 'empty-lines', '\n\n', 'helvetica', options({ plate: plate(100, 100), multiline: true }));
add('layoutVariableText', 'tabs', 'a\tb\t\tc', 'helvetica', options());
add('layoutVariableText', 'empty', '', 'helvetica', options());
add('layoutVariableText', 'empty-multiline', '', 'helvetica', options({ multiline: true }));
add('layoutVariableText', 'limit', 'truncate me please', 'helvetica', options({ limit: 8 }));
add('layoutVariableText', 'limit-counts-breaks', 'ab\ncd\nef', 'helvetica', options({ limit: 5, multiline: true }));
add('layoutVariableText', 'substitute', 'secret', 'helvetica', options({ substitute: '*' }));
add('layoutVariableText', 'substitute-empty', 'secret', 'helvetica', options({ substitute: '' }));
for (const [index, alignment] of [0, 1, 2].entries()) {
  for (const cells of [6, 10]) {
    add('layoutVariableText', `comb-${cells}-${ALIGNS[index]}`, 'AiW1é', 'courier', options({ fontSize: 12, alignment, cells }));
  }
}
add('layoutVariableText', 'comb-proportional', 'AiWm.', 'helvetica', options({ fontSize: 12, cells: 7 }));
add('layoutVariableText', 'comb-overflow', '123456789', 'courier', options({ cells: 4 }));
add('layoutVariableText', 'comb-empty', '', 'courier', options({ cells: 4, alignment: 2 }));
add('layoutVariableText', 'comb-fitted', '1234', 'helvetica', options({ fontSize: 0, cells: 4 }));
add('layoutVariableText', 'comb-multiline', '12\n34', 'courier', options({ cells: 6, multiline: true, plate: plate(120, 60) }));
add('layoutVariableText', 'missing-glyphs', 'Привет, мир', 'helvetica', options());
add('layoutVariableText', 'missing-glyph-comb', 'Ж1', 'courier', options({ cells: 3 }));
add('layoutVariableText', 'astral', 'ok 😀 fine', 'helvetica', options());
add('layoutVariableText', 'symbol-font', 'abc αβγ', 'symbol', options());
add('layoutVariableText', 'zero-width-plate', 'Hello there', 'helvetica', options({ plate: plate(0, 20), multiline: true }));
add('layoutVariableText', 'inverted-plate', 'Hello there', 'helvetica', options({ plate: plate(-20, -10), multiline: true, alignment: 1 }));

// ── widgetDrawing ──

for (const fontFamily of STANDARD_FONTS) {
  for (const fontSize of [0, 8, 11, 14]) {
    add('widgetDrawing', `text/font ${fontFamily} ${fontSize}`, widget({ fontFamily, fontSize }), text(SAMPLE));
  }
}
for (const textAlign of ALIGNS) {
  for (const value of ['Wide WWW text', 'i', '']) {
    add('widgetDrawing', `text/align ${textAlign} "${value}"`, widget({ textAlign }), text(value));
  }
}
for (const border of BORDERS) {
  add('widgetDrawing', `text/border ${borderLabel(border)}`, widget({ ...border }), text('Bordered'));
}
add('widgetDrawing', 'text/border dashed default dash', widget({ borderStyle: 'dashed', dashArray: [] }), text('Dashes'));
add('widgetDrawing', 'text/border dashed eleven dashes', widget({ borderStyle: 'dashed', dashArray: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11] }), text('Dashes'));
add('widgetDrawing', 'text/border dashed 0', widget({ borderStyle: 'dashed', strokeWidth: 0 }), text('Dashes'));
add('widgetDrawing', 'text/border beveled no colours', widget({ borderStyle: 'beveled', ...NO_LOOK }), text('Bevel'));
add('widgetDrawing', 'text/border inset 0', widget({ borderStyle: 'inset', strokeWidth: 0 }), text('Inset'));
add('widgetDrawing', 'text/border underline no colour', widget({ borderStyle: 'underline', color: null }), text('Under'));
add('widgetDrawing', 'text/border style and width absent', widget({ borderStyle: undefined, strokeWidth: undefined }), text('Defaults'));
add('widgetDrawing', 'text/no colours', widget({ ...NO_LOOK }), text('Plain'));
add('widgetDrawing', 'text/overflow fitted', widget({ fontSize: 0 }), text('A long value that will not fit in this box at all'));
add('widgetDrawing', 'text/tall fitted', widget({ fontFamily: 'times-roman', fontSize: 0 }, 120, 70), text('Tall'));
add('widgetDrawing', 'text/size absent', widget({ fontSize: null }), text('Fitted'));
for (const fontSize of [0, 9]) {
  for (const textAlign of ALIGNS) {
    add('widgetDrawing', `text/multiline ${fontSize} ${textAlign}`, widget({ fontSize, textAlign }, 150, 90), text(PARAGRAPHS, { multiline: true }));
  }
}
add('widgetDrawing', 'text/multiline long word', widget({ fontFamily: 'times-roman', fontSize: 12 }, 80, 60), text('Supercalifragilisticexpialidocious word', { multiline: true }));
add('widgetDrawing', 'text/multiline empty', widget({}, 150, 90), text('', { multiline: true }));
add('widgetDrawing', 'text/multiline crlf', widget({}, 150, 90), text('one\r\ntwo\rthree', { multiline: true }));
add('widgetDrawing', 'text/password', widget(), text('secret', { password: true }));
add('widgetDrawing', 'text/password multiline', widget({}, 150, 60), text('se\ncret', { password: true, multiline: true }));
add('widgetDrawing', 'text/max length', widget(), text('truncate me please', { maxLength: 8 }));
add('widgetDrawing', 'text/max length 0', widget(), text('no limit at all', { maxLength: 0 }));
for (const textAlign of ALIGNS) {
  for (const maxLength of [6, 10]) {
    add('widgetDrawing', `text/comb ${maxLength} ${textAlign}`, widget({ fontFamily: 'courier', fontSize: 12, textAlign }), text('AiW1é', { comb: true, maxLength }));
  }
}
add('widgetDrawing', 'text/comb proportional', widget({ fontSize: 12 }), text('AiWm.', { comb: true, maxLength: 7 }));
add('widgetDrawing', 'text/comb fitted', widget({ fontSize: 0 }), text('1234', { comb: true, maxLength: 4 }));
add('widgetDrawing', 'text/comb empty', widget({ fontFamily: 'courier' }), text('', { comb: true, maxLength: 5 }));
add('widgetDrawing', 'text/comb overflow', widget({ fontFamily: 'courier' }), text('123456789', { comb: true, maxLength: 4 }));
add('widgetDrawing', 'text/comb password', widget({ fontFamily: 'courier' }), text('1234', { comb: true, maxLength: 6, password: true }));
add('widgetDrawing', 'text/comb without max length', widget({ fontFamily: 'courier' }), text('1234', { comb: true }));
add('widgetDrawing', 'text/shown text', widget(), text('1234.5'), { text: '$1,234.50' });
add('widgetDrawing', 'text/shown empty text', widget(), text('1234.5'), { text: '' });
add('widgetDrawing', 'text/missing glyphs', widget(), text('Привет'));
add('widgetDrawing', 'text/font not standard', widget({ fontFamily: 'Roboto' }), text('Plain'));
add('widgetDrawing', 'text/font not standard, missing glyphs', widget({ fontFamily: 'Roboto' }), text('Привет'));
add('widgetDrawing', 'text/no font', widget({ fontFamily: null }), text('Plain'));
add('widgetDrawing', 'text/tiny box', widget({}, 2, 2), text('Hidden'));
add('widgetDrawing', 'text/zero box', widget({}, 0, 0), text('Hidden'));
add('widgetDrawing', 'text/negative box', widget({}, -40, -12), text('Inverted'));
add('widgetDrawing', 'checkbox/negative box', widget({}, -18, -18), toggle('checkbox'), { checked: true });
add('widgetDrawing', 'combo/negative box', widget({}, -140, -22), combo('b'));
add('widgetDrawing', 'text/rect without box', { ...widget({}, 90, 30), box: undefined }, text('From rect'));
add('widgetDrawing', 'text/turned', widget({ rotation: 90 }, 30, 100), text('Turned'));

for (const fontSize of [0, 10]) {
  for (const border of BORDERS.slice(0, 6)) {
    add('widgetDrawing', `combo/${fontSize} ${borderLabel(border)}`, widget({ fontSize, ...border }, 140, 22), combo('b'));
  }
}
add('widgetDrawing', 'combo/narrow, no button glyph', widget({ fontSize: 4 }, 18, 6), combo('x'));
add('widgetDrawing', 'combo/button only', widget({}, 14, 22), combo('Beta'));
add('widgetDrawing', 'combo/empty value', widget({}, 140, 22), combo(''));
add('widgetDrawing', 'combo/shown text', widget({}, 140, 22), combo('b'), { text: 'Beta (shown)' });
add('widgetDrawing', 'combo/no colours', widget({ ...NO_LOOK }, 140, 22), combo('b'));
add('widgetDrawing', 'combo/centred ignored', widget({ textAlign: 'right' }, 140, 22), combo('b'));

for (const fontSize of [0, 9]) {
  for (const selected of [['b'], ['a', 'c'], []]) {
    add('widgetDrawing', `list/${fontSize} [${selected}]`, widget({ fontFamily: 'times-roman', fontSize }, 120, 60), list(selected));
  }
}
add('widgetDrawing', 'list/top index 2', widget({ fontFamily: 'times-roman', fontSize: 9 }, 120, 60), list(['d'], 2));
add('widgetDrawing', 'list/top index past the end', widget({ fontSize: 9 }, 120, 60), list(['a'], 8));
add('widgetDrawing', 'list/negative top index', widget({ fontSize: 9 }, 120, 60), list(['a'], -1));
add('widgetDrawing', 'list/no font colour after a selected row', widget({ fontSize: 9, fontColor: null }, 120, 60), list(['a', 'c']));
add('widgetDrawing', 'list/no colours', widget({ ...NO_LOOK, fontSize: 9 }, 120, 60), list([]));
add('widgetDrawing', 'list/no options', widget({ fontSize: 9 }, 120, 60), { ...list([]), options: [] });
add('widgetDrawing', 'list/missing glyphs', widget({ fontSize: 9 }, 120, 60), { ...list([]), options: [option('Опция', 'x')] });
add('widgetDrawing', 'list/dashed', widget({ fontSize: 9, borderStyle: 'dashed', strokeWidth: 2 }, 120, 60), list(['b']));

for (const fontSize of [0, 10, 16]) {
  for (const caption of ['Submit', 'A much longer caption']) {
    add('widgetDrawing', `button/${fontSize} ${caption}`, widget({ fontFamily: 'helvetica-bold', fontSize, caption }, 110, 28), { family: 'pushbutton' });
  }
}
add('widgetDrawing', 'button/no caption', widget({ caption: null }, 110, 28), { family: 'pushbutton' });
add('widgetDrawing', 'button/empty caption', widget({ caption: '' }, 110, 28), { family: 'pushbutton' });
add('widgetDrawing', 'button/no colours', widget({ ...NO_LOOK, caption: 'Go' }, 110, 28), { family: 'pushbutton' });
add('widgetDrawing', 'button/no border', widget({ strokeWidth: 0, caption: 'Go' }, 110, 28), { family: 'pushbutton' });
add('widgetDrawing', 'button/beveled', widget({ borderStyle: 'beveled', caption: 'Go' }, 110, 28), { family: 'pushbutton' });
add('widgetDrawing', 'button/missing glyphs', widget({ caption: 'Вперёд' }, 110, 28), { family: 'pushbutton' });

for (const checkStyle of ['check', 'cross', 'star', 'circle', 'square', 'diamond', null] as const) {
  for (const checked of [true, false]) {
    for (const strokeWidth of [1, 2, 0]) {
      add('widgetDrawing', `checkbox/${checkStyle} ${checked} ${strokeWidth}`, widget({ checkStyle, strokeWidth }, 18, 18), toggle('checkbox'), { checked });
    }
  }
}
add('widgetDrawing', 'checkbox/state on', widget({ appearanceState: 'Yes' }, 18, 18), toggle('checkbox'));
add('widgetDrawing', 'checkbox/state off', widget({ appearanceState: 'Off' }, 18, 18), toggle('checkbox'));
add('widgetDrawing', 'checkbox/no state', widget({}, 18, 18), toggle('checkbox'));
add('widgetDrawing', 'checkbox/state absent', widget({ appearanceState: undefined }, 18, 18), toggle('checkbox'));
add('widgetDrawing', 'checkbox/shown over state', widget({ appearanceState: 'Yes' }, 18, 18), toggle('checkbox'), { checked: false });
add('widgetDrawing', 'checkbox/no colours', widget({ ...NO_LOOK }, 18, 18), toggle('checkbox'), { checked: true });
add('widgetDrawing', 'checkbox/cross, no border colour', widget({ checkStyle: 'cross', color: null, strokeWidth: 3 }, 18, 18), toggle('checkbox'), { checked: true });
add('widgetDrawing', 'checkbox/beveled', widget({ borderStyle: 'beveled' }, 18, 18), toggle('checkbox'), { checked: true });
add('widgetDrawing', 'checkbox/wide', widget({ checkStyle: 'star' }, 40, 14), toggle('checkbox'), { checked: true });
add('widgetDrawing', 'checkbox/tall', widget({ checkStyle: 'check' }, 12, 30), toggle('checkbox'), { checked: true });
add('widgetDrawing', 'checkbox/turned', widget({ rotation: 270 }, 30, 18), toggle('checkbox'), { checked: true });

for (const checkStyle of ['circle', 'check', 'star', 'cross', 'square', 'diamond', null] as const) {
  for (const checked of [true, false]) {
    add('widgetDrawing', `radio/${checkStyle} ${checked}`, widget({ checkStyle }, 16, 16), toggle('radio'), { checked });
  }
}
add('widgetDrawing', 'radio/state on', widget({ appearanceState: 'one' }, 16, 16), toggle('radio'));
add('widgetDrawing', 'radio/no border', widget({ strokeWidth: 0 }, 16, 16), toggle('radio'), { checked: true });
add('widgetDrawing', 'radio/no colours', widget({ ...NO_LOOK }, 16, 16), toggle('radio'), { checked: true });
add('widgetDrawing', 'radio/oval', widget({ strokeWidth: 2 }, 30, 14), toggle('radio'), { checked: true });

add('widgetDrawing', 'signature', widget(), { family: 'signature' });
add('widgetDrawing', 'unknown family', widget(), { family: 'unknown', rawValue: '' });

// ── widgetScene ──

const placementOf = (row: ReturnType<typeof widget>, rotation?: number | null) => ({
  box: row.box,
  ...(rotation === undefined ? {} : { rotation }),
});

const hi = widget({ fontColor: '#ff0000', interiorColor: '#ffffff', color: '#000000' }, 100, 20);
add('widgetScene', 'text/upright', hi, text('Hi'), {}, placementOf(hi));
add('widgetScene', 'text/rotation null', hi, text('Hi'), {}, placementOf(hi, null));
for (const rotation of [0, 90, 180, 270, -90, 450, 45]) {
  const row = widget({ rotation }, 100, 30);
  add('widgetScene', `text/turned ${rotation}`, row, text('Turned'), {}, placementOf(row, rotation));
}
add('widgetScene', 'text/multiline', widget({ fontSize: 9 }, 150, 90), text(PARAGRAPHS, { multiline: true }), {}, placementOf(widget({}, 150, 90)));
add('widgetScene', 'text/comb', widget({ fontFamily: 'courier', fontSize: 12 }), text('AiW1', { comb: true, maxLength: 6 }), {}, placementOf(widget()));
add('widgetScene', 'text/dashed', widget({ borderStyle: 'dashed', strokeWidth: 2, dashArray: [4, 2] }), text('Dashes'), {}, placementOf(widget()));
add('widgetScene', 'text/beveled', widget({ borderStyle: 'beveled', fontFamily: 'times-italic' }), text('Bevel'), {}, placementOf(widget()));
add('widgetScene', 'text/off-grid box', widget({}, 77.3, 19.7), text('Odd'), {}, { box: { x: 13.37, y: 101.01, width: 77.3, height: 19.7 }, rotation: 90 });
add('widgetScene', 'text/empty', widget(), text(''), {}, placementOf(widget()));
for (const checkStyle of ['check', 'cross', 'star', 'circle', 'square', 'diamond'] as const) {
  const row = widget({ checkStyle, rotation: 90 }, 18, 18);
  add('widgetScene', `checkbox/${checkStyle} turned 90`, row, toggle('checkbox'), { checked: true }, placementOf(row, 90));
}
const radioRow = widget({ rotation: 180 }, 16, 16);
add('widgetScene', 'radio/turned 180', radioRow, toggle('radio'), { checked: true }, placementOf(radioRow, 180));
const comboRow = widget({ rotation: 270 }, 120, 22);
add('widgetScene', 'combo/turned 270', comboRow, combo('a'), {}, placementOf(comboRow, 270));
const listRow = widget({ fontSize: 9 }, 120, 60);
add('widgetScene', 'list/selected', listRow, list(['b']), {}, placementOf(listRow));
const buttonRow = widget({ caption: 'Go', rotation: 90 }, 90, 26);
add('widgetScene', 'button/turned 90', buttonRow, { family: 'pushbutton' }, {}, placementOf(buttonRow, 90));
add('widgetScene', 'colours clamp and round', widget({ color: '#808080', interiorColor: '#010203', fontColor: '#fefdfc' }), text('Hue'), {}, placementOf(widget()));

// ── textEditorLayout ──

const editorRow = (patch: Record<string, unknown> = {}, width = 120, height = 24) =>
  widget({ ...NO_LOOK, color: '#000000', ...patch }, width, height);

for (const [fontFamily, fontSize, borderStyle] of [
  ['helvetica', 10, 'solid'],
  ['times-roman', 14, 'beveled'],
  ['courier-bold', 0, 'inset'],
] as const) {
  add('textEditorLayout', `one line ${fontFamily} ${fontSize} ${borderStyle}`, editorRow({ fontFamily, fontSize, borderStyle }), text('Ada Lovelace'), 'Ada Lovelace');
}
add('textEditorLayout', 'multiline', editorRow({ fontSize: 9 }, 120, 80), text('one\ntwo', { multiline: true }), 'one\ntwo');
add('textEditorLayout', 'fitted short', editorRow({ fontSize: 0 }), text(''), 'Ada');
add('textEditorLayout', 'fitted long', editorRow({ fontSize: 0 }), text(''), 'Ada Lovelace, Countess of Lovelace');
add('textEditorLayout', 'empty text', editorRow(), text(''), '');
add('textEditorLayout', 'comb', editorRow({ fontFamily: 'courier', fontSize: 12 }), text('1234', { comb: true, maxLength: 6 }), '1234');
add('textEditorLayout', 'comb empty', editorRow({ fontFamily: 'courier', fontSize: 12 }), text('', { comb: true, maxLength: 6 }), '');
add('textEditorLayout', 'comb proportional', editorRow({ fontSize: 12 }), text('', { comb: true, maxLength: 7 }), 'AiWm.');
add('textEditorLayout', 'comb right', editorRow({ fontFamily: 'courier', fontSize: 12, textAlign: 'right' }), text('', { comb: true, maxLength: 6 }), '12');
add('textEditorLayout', 'comb password', editorRow({ fontSize: 12 }), text('', { comb: true, maxLength: 6, password: true }), 'abc');
add('textEditorLayout', 'comb without max length', editorRow({ fontFamily: 'courier' }), text('', { comb: true }), '12');
add('textEditorLayout', 'comb missing glyphs', editorRow({ fontFamily: 'courier' }), text('', { comb: true, maxLength: 4 }), 'ЖЖ');
add('textEditorLayout', 'font not standard', editorRow({ fontFamily: 'Roboto' }), text(''), 'Ada');
add('textEditorLayout', 'no font', editorRow({ fontFamily: null, fontSize: null }), text(''), 'Ada');
add('textEditorLayout', 'italic face', editorRow({ fontFamily: 'times-bold-italic', borderStyle: 'dashed', strokeWidth: 3 }), text(''), 'Ada');
add('textEditorLayout', 'checkbox', editorRow(), toggle('checkbox'), '');
add('textEditorLayout', 'combo', editorRow(), combo('a'), 'Alpha');

// ── paints ──

for (const family of ['text', 'checkbox', 'radio', 'combobox', 'listbox', 'pushbutton', 'signature', 'unknown']) {
  add('paints', family, family);
}

// ── needsBreak ──

/** One of each class the rule tells apart, and the edges of its ranges. */
const CODES = [
  0x41, 0x7a, 0x30, 0x39, 0x20, 0x09, 0x3000, 0x2c, 0x2e, 0x21, 0x3f, 0x27, 0x28, 0x29, 0x5b, 0x7b,
  0x22, 0x24, 0x2d, 0x2f, 0x26, 0x01, 0x80, 0x94, 0x95, 0x96, 0xb4, 0xb8, 0xa3, 0xc0, 0xff, 0x100,
  0x24f, 0x250, 0x1e00, 0x416, 0x2010, 0x2014, 0x2044, 0x20ac, 0x2116, 0x1100, 0x2e80, 0x3001,
  0x3005, 0x300a, 0x3021, 0x3036, 0x3042, 0x4e00, 0x9fbf, 0x9fc0, 0xac00, 0xd7af, 0xd83d, 0xf900,
  0xfe30, 0xfe50, 0xfe5f, 0xfe63, 0xfe69, 0xff01, 0xff04, 0xff08, 0xff21, 0xff41, 0xff66, 0xff9d,
  0xff9e, 0xffe0, 0x20000,
];
for (const previous of CODES) {
  add('needsBreak', `after U+${previous.toString(16).padStart(4, '0')}`, previous, CODES);
}

export const suites: Suite[] = [
  {
    kind: 'function',
    file: 'core/form-painter.json',
    describes:
      'packages/core/form: the widget painter: layout fonts, variable-text layout (fitting, wrapping, combs, limits, line-break classes), widget drawings for every family, border style and check style, scenes placed and turned, and the text editor layout.',
    functions,
    cases,
  },
];
