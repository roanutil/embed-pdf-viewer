/**
 * `packages/core/form`: the widget painter, and `packages/core/text`, the
 * layout it draws its text with. Replayed by
 * `crates/plugins/form-core/tests/conformance.rs`.
 *
 * Distinct from `plugins/form-core.json`, which records the form plugin. This
 * file is the pure painter: what a widget draws for its facts, its text's
 * layout, the scene on the page and the HTML editor's layout.
 *
 * Pure functions, so one function suite. The widget matrix follows the
 * engine's parity gate (`packages/engine/main/test/widget-painter-parity.test.ts`),
 * which checks the same painter against PDFium's appearance generator.
 *
 * e40c9c236 moved a widget's text layout out of the painter into
 * `@embedpdf/core-text`: `layoutVariableText(text, font, options)` became
 * `layoutField(text, fonts, options)`, which lays text out in a face, its
 * fallbacks and a last resort, in bidi order, as runs; `layoutFontOf(family)`
 * became the paint fonts (`defaultPaintFonts`, `programPaintFont`); and the
 * line-break rule `needsBreak` moved to `core/text/src/breaks.ts`, unchanged.
 * The cases that called the old functions call the new ones with the same
 * inputs where the contract allows, and the new contract's own behaviour
 * (directions, fallbacks, joins, graphemes) has cases of its own.
 *
 * Several entries are wrappers, because the shipping values carry closures:
 *
 * - A `fonts` argument names the fonts a layout draws in, as data
 *   (`FontsSpec`): a standard font's name, `'last-resort'`, or a program font
 *   made with `programPaintFont` from its key, its ranges, its metrics and a
 *   measure of `advance` thousandths per character (marks and joiners take no
 *   room, and each joiner takes `joined` off, as a cursive face's joined
 *   forms are narrower). A bare string is `defaultPaintFonts(string)`.
 * - `defaultPaintFonts(fontFamily, codes)` and `programPaintFont(spec, codes)`
 *   record each font's data members and ask `covers` and `width` (both
 *   directions) for each code point the case passes.
 * - `layoutField(text, fonts, options)` prints each run's font as its key.
 * - `widgetDrawing` and `textEditorLayout` take an optional `FontsSpec` last,
 *   for the fonts the form plugin passes when it knows the document's.
 * - `widgetScene(widget, field, shown, placement)` draws the widget with
 *   `widgetDrawing` and places that drawing, as the form plugin does.
 * - `paints(kind)` takes the field kind alone.
 * - `widgetText(widget, field, shown)` passes `shown` through as given.
 * - `needsBreak(previous, currents)` asks the line-break rule for each
 *   current character after `previous`.
 *
 * A font in a result prints its methods as `{"$function": true}`, a font
 * program its `load`.
 */
import { fontProgramFamily } from '../../../../packages/core/fonts/src/index';
import type { FontProgramInfo, FormField, WidgetAnnotation } from '@embedpdf/engine-core/runtime';
import { STANDARD_FONTS, isStandardFontName } from '@embedpdf/engine-core/runtime';

import {
  paints,
  textEditorLayout,
  widgetDrawing,
  widgetScene,
  widgetText,
  type ShownValue,
  type WidgetScenePlacement,
} from '../../../../packages/core/form/src/index';
import {
  defaultPaintFonts,
  lastResortPaintFont,
  layoutField,
  programPaintFont,
  standardPaintFont,
  type FieldText,
  type FieldTextOptions,
  type PaintFont,
  type PaintFonts,
} from '../../../../packages/core/text/src/index';
import { needsBreak } from '../../../../packages/core/text/src/breaks';

import type { FunctionCase, Suite } from '../vectors';

/** A program font as a case writes it: what `programPaintFont` is given, as data. */
interface ProgramSpec {
  readonly key: string;
  /** Inclusive code point ranges, in any order. */
  readonly ranges: readonly (readonly [number, number])[];
  readonly ascent?: number;
  readonly descent?: number;
  /** Thousandths per character that takes room. */
  readonly advance: number;
  /** Thousandths each joiner (U+200D) takes off: a joined form's narrowing. */
  readonly joined?: number;
}
type FontSpec = string | ProgramSpec;
type FontsSpec =
  | string
  | {
      readonly face: FontSpec;
      readonly fallbacks?: readonly FontSpec[];
      readonly lastResort?: FontSpec | null;
    };

const ZERO_WIDTH_JOINER = '\u200d';
const TAKES_NO_ROOM = /[\p{M}\u200d]/u;

function programFont(spec: ProgramSpec): PaintFont {
  const info = {
    covers: spec.ranges.map(([from, to]) => ({ start: from, count: to - from + 1 })),
    ascent: spec.ascent ?? 900,
    descent: spec.descent ?? -300,
  } as unknown as FontProgramInfo;
  const program = {
    hash: spec.key,
    family: fontProgramFamily(spec.key),
    weight: 400,
    style: 'normal' as const,
    load: () => Promise.resolve(new Uint8Array()),
  };
  return programPaintFont(
    info,
    program,
    (text) =>
      [...text].filter((c) => !TAKES_NO_ROOM.test(c)).length * spec.advance -
      [...text].filter((c) => c === ZERO_WIDTH_JOINER).length * (spec.joined ?? 0),
  );
}

const programs = new Map<string, PaintFont>();
function fontOf(spec: FontSpec): PaintFont {
  if (typeof spec !== 'string') {
    let font = programs.get(spec.key);
    if (!font) programs.set(spec.key, (font = programFont(spec)));
    return font;
  }
  if (spec === 'last-resort') return lastResortPaintFont();
  if (isStandardFontName(spec)) return standardPaintFont(spec);
  throw new Error(`not a font: ${spec}`);
}

function fontsOf(spec: FontsSpec): PaintFonts {
  if (typeof spec === 'string') return defaultPaintFonts(spec);
  return {
    face: fontOf(spec.face),
    fallbacks: (spec.fallbacks ?? []).map(fontOf),
    lastResort:
      spec.lastResort === undefined
        ? lastResortPaintFont()
        : spec.lastResort === null
          ? null
          : fontOf(spec.lastResort),
  };
}

/** A font's data members, and what it answers for each of `codes`. */
const probe = (font: PaintFont, codes: readonly number[]) => ({
  key: font.key,
  ascent: font.ascent,
  descent: font.descent,
  standard: font.standard,
  fontFamily: font.fontFamily,
  fontWeight: font.fontWeight,
  fontStyle: font.fontStyle,
  program: font.program,
  covers: codes.map((code) => font.covers(code)),
  widths: codes.map((code) => font.width(String.fromCodePoint(code), false)),
  rtlWidths: codes.map((code) => font.width(String.fromCodePoint(code), true)),
});

/** A layout with each run's font as its key. */
const keyed = (laid: FieldText<PaintFont>) => ({
  ...laid,
  lines: laid.lines.map((line) => ({
    ...line,
    runs: line.runs.map((run) => ({ ...run, font: run.font.key })),
  })),
});

const functions = {
  defaultPaintFonts: (fontFamily: string | null, codes: readonly number[]) => {
    const fonts = defaultPaintFonts(fontFamily);
    return {
      face: probe(fonts.face, codes),
      fallbacks: fonts.fallbacks.map((font) => probe(font, codes)),
      lastResort: fonts.lastResort && probe(fonts.lastResort, codes),
    };
  },
  programPaintFont: (spec: ProgramSpec, codes: readonly number[]) =>
    probe(programFont(spec), codes),
  layoutField: (text: string, fonts: FontsSpec, options: FieldTextOptions) =>
    keyed(layoutField(text, fontsOf(fonts), options)),
  widgetDrawing: (
    widget: WidgetAnnotation,
    field: FormField,
    shown?: ShownValue,
    fonts?: FontsSpec,
  ) => widgetDrawing(widget, field, shown, fonts === undefined ? undefined : fontsOf(fonts)),
  widgetScene: (
    widget: WidgetAnnotation,
    field: FormField,
    shown: ShownValue,
    placement: WidgetScenePlacement,
  ) => widgetScene(widgetDrawing(widget, field, shown)!, placement),
  textEditorLayout: (widget: WidgetAnnotation, field: FormField, text: string, fonts?: FontsSpec) =>
    textEditorLayout(widget, field, text, fonts === undefined ? undefined : fontsOf(fonts)),
  paints: (kind: FormField['kind']) => paints({ kind }),
  widgetText: (widget: WidgetAnnotation, field: FormField, shown?: ShownValue) =>
    widgetText(widget, field, shown),
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
  kind: 'text',
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
  kind: 'combobox',
  value,
  defaultValue: '',
  edit: false,
  options: [option('Alpha', 'a'), option('Beta', 'b')],
});

const LETTERS = ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h'];
const list = (selected: readonly string[], topIndex = 0) => ({
  kind: 'listbox',
  selectedValues: [...selected],
  defaultValue: [],
  multiSelect: true,
  options: LETTERS.map((value) =>
    option(`Option ${value.toUpperCase()}`, value, selected.includes(value)),
  ),
  topIndex,
});

const toggle = (kind: 'checkbox' | 'radio') => ({ kind, widgets: [] });

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

// ── defaultPaintFonts (was layoutFontOf) ──

/**
 * The old probes, then a right-to-left pair whose mirrors differ in some
 * faces, Hebrew, Arabic, a mark, a joiner, a supplementary character and the
 * edges of the last resort's coverage.
 */
const PROBES = [
  0x41, 0x6e, 0x20, 0xe9, 0x20ac, 0x201c, 0x416, 0xd83d, 0, 0x2a, 0x3b1, 0x2701, 0x28, 0x5b, 0x2264,
  0xab, 0x5d0, 0x627, 0x301, 0x200d, 0xfe0f, 0x1f600, 0x1f, 0xdfff, 0xe000, 0x10ffff,
];
for (const font of STANDARD_FONTS) add('defaultPaintFonts', font, font, PROBES);
add('defaultPaintFonts', 'null', null, PROBES);
add('defaultPaintFonts', 'undefined', undefined, PROBES);
for (const name of ['', 'Helvetica', 'Roboto', 'arial', 'helvetica ']) {
  add('defaultPaintFonts', `not-standard:${name}`, name, PROBES);
}

// ── programPaintFont ──

/** A Hebrew face, as the core-text tests make one. */
const HEBREW: ProgramSpec = { key: 'hebrew', ranges: [[0x590, 0x5ff]], advance: 500 };
/** An Arabic face whose joined forms are 100 narrower on each joined side. */
const ARABIC: ProgramSpec = { key: 'arabic', ranges: [[0x600, 0x6ff]], advance: 600, joined: 100 };
/** A Latin face whose advances aren't whole thousandths. */
const LATIN: ProgramSpec = { key: 'latin', ranges: [[0x20, 0x7e]], advance: 500.4 };
/** A face of everything, the core-text tests' last resort. */
const EVERYTHING: ProgramSpec = { key: 'everything', ranges: [[0, 0x10ffff]], ascent: 800, descent: -200, advance: 500 };
/** Ranges out of order, touching and one code point wide: the binary search sorts them. */
const SCATTERED: ProgramSpec = {
  key: 'scattered',
  ranges: [[0x100, 0x17f], [0x41, 0x5a], [0x30, 0x39], [0x5b, 0x5b], [0x1f600, 0x1f64f]],
  ascent: 750,
  descent: -250,
  advance: 480,
};
for (const spec of [HEBREW, ARABIC, LATIN, EVERYTHING, SCATTERED]) {
  add('programPaintFont', spec.key, spec, PROBES);
}

// ── layoutField (was layoutVariableText) ──

/**
 * The old cases keep their text, font and options. `substitute: '*'` is
 * `password` now; `centered`, new, is off, as the old layout never centred
 * (the painter moved the lines itself); a font is a `FontsSpec`, so a family
 * lays out in its standard font with the last resort behind it, as the
 * painter does.
 */
const plate = (right: number, top: number, left = 0, bottom = 0) => ({ left, bottom, right, top });
const options = (patch: Record<string, unknown> = {}) => ({
  plate: plate(200, 20),
  fontSize: 10,
  alignment: 0,
  multiline: false,
  password: false,
  limit: 0,
  cells: 0,
  centered: false,
  ...patch,
});

add('layoutField', 'plain', 'Hello', 'helvetica', options());
add('layoutField', 'offset-plate', 'Hello', 'times-roman', options({ plate: plate(130, 47, 12, 9) }));
for (const [index, alignment] of [0, 1, 2].entries()) {
  add('layoutField', `align-${ALIGNS[index]}`, 'Wide WWW text', 'helvetica', options({ alignment }));
  add(
    'layoutField',
    `multiline-align-${ALIGNS[index]}`,
    PARAGRAPHS,
    'helvetica',
    options({ plate: plate(146, 86), fontSize: 9, alignment, multiline: true }),
  );
}
for (const height of [10, 20, 40, 1000, 0, 3]) {
  add('layoutField', `fitted-height-${height}`, 'Hello', 'helvetica', options({ plate: plate(200, height), fontSize: 0 }));
}
add('layoutField', 'fitted-wide-text', 'A long value that will not fit in this box at all', 'helvetica', options({ plate: plate(150, 20), fontSize: 0 }));
add('layoutField', 'fitted-multiline', PARAGRAPHS, 'helvetica', options({ plate: plate(146, 86), fontSize: 0, multiline: true }));
add('layoutField', 'fitted-multiline-tiny', PARAGRAPHS, 'courier', options({ plate: plate(40, 10), fontSize: 0, multiline: true }));
add('layoutField', 'fitted-zero-width', 'Hello', 'helvetica', options({ plate: plate(0, 20), fontSize: 0 }));
add('layoutField', 'fitted-empty', '', 'helvetica', options({ fontSize: 0 }));
add('layoutField', 'near-zero-size-fits', 'Hello', 'helvetica', options({ fontSize: 0.00005 }));
add('layoutField', 'long-word', 'Supercalifragilisticexpialidocious word', 'times-roman', options({ plate: plate(76, 56), fontSize: 12, multiline: true }));
add('layoutField', 'one-char-per-line', 'WWW', 'helvetica', options({ plate: plate(5, 56), multiline: true }));
add('layoutField', 'cjk', '日本語のテキストを折り返す。次の文です', 'helvetica', options({ plate: plate(40, 80), multiline: true }));
add('layoutField', 'punctuation', 'one, two; (three) [four] {five} a-b a/b "q" it\'s', 'helvetica', options({ plate: plate(60, 120), multiline: true }));
add('layoutField', 'open-punctuation', 'a ((((b c', 'helvetica', options({ plate: plate(20, 120), multiline: true }));
add('layoutField', 'currency', '$100 €5 £3 ¥7 №9', 'helvetica', options({ plate: plate(24, 120), multiline: true }));
add('layoutField', 'latin-digits', 'abc123def456 ghi789', 'courier', options({ plate: plate(40, 80), multiline: true }));
add('layoutField', 'line-breaks', 'a\r\nb\rc\nd\n\re', 'helvetica', options({ plate: plate(100, 100), multiline: true }));
add('layoutField', 'line-breaks-single-line', 'a\r\nb\rc\nd', 'helvetica', options());
add('layoutField', 'empty-lines', '\n\n', 'helvetica', options({ plate: plate(100, 100), multiline: true }));
add('layoutField', 'tabs', 'a\tb\t\tc', 'helvetica', options());
add('layoutField', 'empty', '', 'helvetica', options());
add('layoutField', 'empty-multiline', '', 'helvetica', options({ multiline: true }));
add('layoutField', 'limit', 'truncate me please', 'helvetica', options({ limit: 8 }));
add('layoutField', 'limit-counts-breaks', 'ab\ncd\nef', 'helvetica', options({ limit: 5, multiline: true }));
add('layoutField', 'password', 'secret', 'helvetica', options({ password: true }));
add('layoutField', 'password-centered', 'secret', 'helvetica', options({ password: true, centered: true }));
for (const [index, alignment] of [0, 1, 2].entries()) {
  for (const cells of [6, 10]) {
    add('layoutField', `comb-${cells}-${ALIGNS[index]}`, 'AiW1é', 'courier', options({ fontSize: 12, alignment, cells }));
  }
}
add('layoutField', 'comb-proportional', 'AiWm.', 'helvetica', options({ fontSize: 12, cells: 7 }));
add('layoutField', 'comb-overflow', '123456789', 'courier', options({ cells: 4 }));
add('layoutField', 'comb-empty', '', 'courier', options({ cells: 4, alignment: 2 }));
add('layoutField', 'comb-fitted', '1234', 'helvetica', options({ fontSize: 0, cells: 4 }));
add('layoutField', 'comb-multiline', '12\n34', 'courier', options({ cells: 6, multiline: true, plate: plate(120, 60) }));
add('layoutField', 'missing-glyphs', 'Привет, мир', 'helvetica', options());
add('layoutField', 'missing-glyph-comb', 'Ж1', 'courier', options({ cells: 3 }));
add('layoutField', 'astral', 'ok 😀 fine', 'helvetica', options());
add('layoutField', 'symbol-font', 'abc αβγ', 'symbol', options());
add('layoutField', 'zero-width-plate', 'Hello there', 'helvetica', options({ plate: plate(0, 20), multiline: true }));
add('layoutField', 'inverted-plate', 'Hello there', 'helvetica', options({ plate: plate(-20, -10), multiline: true, alignment: 1 }));

// The new contract: fonts, directions, graphemes and joins, after the
// core-text tests (`packages/core/text/test/text.test.ts`).
const FONTS = { face: 'helvetica', fallbacks: [HEBREW], lastResort: EVERYTHING };
const FIELD = options({ plate: plate(298, 23, 2, 1), fontSize: 12, centered: true });
add('layoutField', 'bidi/latin hebrew digits', 'Hello שלום 123', FONTS, FIELD);
add('layoutField', 'bidi/first strong hebrew', '123 שלום', FONTS, FIELD);
add('layoutField', 'bidi/hebrew then latin', 'שלום test 123', FONTS, FIELD);
add('layoutField', 'bidi/latin then hebrew', 'Hello שלום', FONTS, FIELD);
add('layoutField', 'bidi/neutral only', '123', FONTS, FIELD);
add('layoutField', 'bidi/empty', '', FONTS, FIELD);
add('layoutField', 'bidi/nested', 'abc אבג 123 def', FONTS, FIELD);
add('layoutField', 'bidi/mirrored brackets', 'שלום (a) [ב]', FONTS, FIELD);
add('layoutField', 'bidi/arabic digits', 'شكرا ١٢٣ abc', { face: 'helvetica', fallbacks: [ARABIC], lastResort: EVERYTHING }, FIELD);
add('layoutField', 'bidi/trailing spaces', 'שלום   ', FONTS, FIELD);
add('layoutField', 'bidi/isolates', '⁧abc⁩ שלום', FONTS, FIELD);
add('layoutField', 'bidi/right aligned rtl', 'שלום', FONTS, { ...FIELD, alignment: 2 });
add('layoutField', 'fallback/last resort reported', 'A中B', FONTS, FIELD);
add('layoutField', 'fallback/default last resort', 'A中B Привет', 'times-bold', FIELD);
add('layoutField', 'fallback/no last resort', 'A中B', { face: 'helvetica', lastResort: null }, FIELD);
add('layoutField', 'fallback/first that has it all', 'aα', { face: 'courier', fallbacks: ['symbol', 'helvetica'], lastResort: null }, FIELD);
add('layoutField', 'fallback/program face', 'abcd', { face: LATIN, lastResort: null }, FIELD);
add('layoutField', 'fallback/program face and standard fallback', 'abc é ž', { face: SCATTERED, fallbacks: ['helvetica'] }, FIELD);
add('layoutField', 'align/right', 'Hi', FONTS, { ...FIELD, alignment: 2 });
add('layoutField', 'align/centre', 'Hi', FONTS, { ...FIELD, alignment: 1 });
add('layoutField', 'multiline/wraps and breaks', 'one two three four five six seven\nשלום', FONTS, { ...FIELD, plate: plate(100, 99, 2, 1), multiline: true, centered: false });
add('layoutField', 'multiline/rtl wraps', 'שלום עולם שלום עולם שלום עולם', FONTS, { ...FIELD, plate: plate(60, 99, 2, 1), multiline: true, centered: false });
add('layoutField', 'multiline/centred', 'one\ntwo', FONTS, { ...FIELD, plate: plate(100, 99, 2, 1), multiline: true });
add('layoutField', 'fitted/long value', 'A long value that has to shrink', FONTS, { ...FIELD, fontSize: 0 });
add('layoutField', 'fitted/hebrew', 'שלום עולם', FONTS, { ...FIELD, fontSize: 0 });
add('layoutField', 'fitted/multiline', 'one two three four five six seven\nשלום', FONTS, { ...FIELD, plate: plate(100, 99, 2, 1), fontSize: 0, multiline: true });
add('layoutField', 'comb/hebrew', 'אב', FONTS, { ...FIELD, cells: 4 });
add('layoutField', 'comb/arabic joins', 'مرحبا', { face: 'helvetica', fallbacks: [ARABIC], lastResort: EVERYTHING }, { ...FIELD, cells: 5 });
add('layoutField', 'comb/arabic with marks', 'مَرْحَبًا', { face: 'helvetica', fallbacks: [ARABIC], lastResort: EVERYTHING }, { ...FIELD, cells: 6 });
add('layoutField', 'comb/arabic centred', 'بب', { face: 'helvetica', fallbacks: [ARABIC], lastResort: EVERYTHING }, { ...FIELD, cells: 5, alignment: 1 });
add('layoutField', 'comb/non-joiner', 'ب‌ب', { face: 'helvetica', fallbacks: [ARABIC], lastResort: EVERYTHING }, { ...FIELD, cells: 4 });
add('layoutField', 'comb/supplementary', '😀a😀', FONTS, { ...FIELD, cells: 4 });
add('layoutField', 'shaped/arabic face with spaces', 'مرحبا بكم  يا', { face: { ...ARABIC, key: 'arabic-spaced', ranges: [[0x20, 0x20], [0x600, 0x6ff]] } }, FIELD);
add('layoutField', 'shaped/arabic words', 'مرحبا بكم', { face: 'helvetica', fallbacks: [ARABIC], lastResort: EVERYTHING }, FIELD);
add('layoutField', 'password/hebrew', 'שלום', FONTS, { ...FIELD, password: true });
add('layoutField', 'password/supplementary', 'a😀b', FONTS, { ...FIELD, password: true });
add('layoutField', 'graphemes/pointed hebrew', 'שָׁלוֹם', FONTS, FIELD);
add('layoutField', 'graphemes/joined emoji', 'a 👩‍💻 b', FONTS, FIELD);
add('layoutField', 'graphemes/variation selector', 'a ☺️ b', FONTS, FIELD);
add('layoutField', 'graphemes/combining latin', 'é ñ', FONTS, FIELD);
add('layoutField', 'graphemes/invisible', 'a​b⁠c', FONTS, FIELD);
add('layoutField', 'graphemes/leading mark', '́a', FONTS, FIELD);
add('layoutField', 'limit/supplementary', 'a😀b', FONTS, { ...FIELD, limit: 2 });
add('layoutField', 'limit/fits supplementary', 'a😀b', FONTS, { ...FIELD, limit: 3 });
add('layoutField', 'limit/multiline crlf', 'ab\r\ncd', FONTS, { ...FIELD, limit: 4, multiline: true });
add('layoutField', 'breaks/cjk in last resort', '日本語のテキストを折り返す。次の文です', FONTS, { ...FIELD, plate: plate(40, 80), multiline: true, centered: false });

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
    add('widgetDrawing', `button/${fontSize} ${caption}`, widget({ fontFamily: 'helvetica-bold', fontSize, caption }, 110, 28), { kind: 'pushbutton' });
  }
}
add('widgetDrawing', 'button/no caption', widget({ caption: null }, 110, 28), { kind: 'pushbutton' });
add('widgetDrawing', 'button/empty caption', widget({ caption: '' }, 110, 28), { kind: 'pushbutton' });
add('widgetDrawing', 'button/no colours', widget({ ...NO_LOOK, caption: 'Go' }, 110, 28), { kind: 'pushbutton' });
add('widgetDrawing', 'button/no border', widget({ strokeWidth: 0, caption: 'Go' }, 110, 28), { kind: 'pushbutton' });
add('widgetDrawing', 'button/beveled', widget({ borderStyle: 'beveled', caption: 'Go' }, 110, 28), { kind: 'pushbutton' });
add('widgetDrawing', 'button/missing glyphs', widget({ caption: 'Вперёд' }, 110, 28), { kind: 'pushbutton' });

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

add('widgetDrawing', 'signature', widget(), { kind: 'signature' });
add('widgetDrawing', 'unknown family', widget(), { kind: 'unknown', rawValue: '' });

// A widget in the fonts the form plugin passes: no approximation for a font
// it was given, the fallbacks drawing what the face lacks.
add('widgetDrawing', 'fonts/text hebrew fallback', widget(), text('Hello שלום 123'), {}, FONTS);
add('widgetDrawing', 'fonts/text rtl comb', widget(), text('אבג', { comb: true, maxLength: 5 }), {}, FONTS);
add('widgetDrawing', 'fonts/text not standard', widget({ fontFamily: 'Roboto' }), text('Ada'), {}, { face: LATIN, lastResort: null });
add('widgetDrawing', 'fonts/text missing everywhere', widget(), text('A中B'), {}, { face: 'helvetica', fallbacks: [HEBREW] });
add('widgetDrawing', 'fonts/combo arabic', widget(), combo('مرحبا'), {}, { face: 'helvetica', fallbacks: [ARABIC], lastResort: EVERYTHING });
add('widgetDrawing', 'fonts/list hebrew', widget({ fontSize: 9 }, 120, 60), { ...list(['x']), options: [option('שלום', 'x', true), option('Option', 'y')] }, {}, FONTS);
add('widgetDrawing', 'fonts/button hebrew', widget({ caption: 'שלח' }, 110, 28), { kind: 'pushbutton' }, {}, FONTS);
add('widgetDrawing', 'text/rtl without fonts', widget(), text('שלום עולם'));
add('widgetDrawing', 'text/rtl multiline without fonts', widget({}, 80, 60), text('שלום עולם שלום עולם', { multiline: true }));

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
add('widgetScene', 'button/turned 90', buttonRow, { kind: 'pushbutton' }, {}, placementOf(buttonRow, 90));
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
add('textEditorLayout', 'rtl', editorRow(), text(''), 'שלום');
add('textEditorLayout', 'fonts/hebrew fallback', editorRow(), text(''), 'Hello שלום', FONTS);
add('textEditorLayout', 'fonts/fallback not drawn', editorRow(), text(''), 'Hello', FONTS);
add('textEditorLayout', 'fonts/no last resort', editorRow(), text(''), 'Hello', { face: LATIN, lastResort: null });
add('textEditorLayout', 'fonts/comb hebrew', editorRow({ fontSize: 12 }), text('', { comb: true, maxLength: 6 }), 'אבג', FONTS);

// ── paints ──

for (const kind of ['text', 'checkbox', 'radio', 'combobox', 'listbox', 'pushbutton', 'signature', 'unknown']) {
  add('paints', kind, kind);
}

// ── widgetText ──

add('widgetText', 'text', widget(), text('Ada'));
add('widgetText', 'text shown', widget(), text('Ada'), { text: '1,234.00' });
add('widgetText', 'password', widget(), text('secret', { password: true }));
add('widgetText', 'password empty', widget(), text('', { password: true }));
add('widgetText', 'combo', widget(), combo('a'));
add('widgetText', 'combo shown', widget(), combo('a'), { text: 'Alpha' });
add('widgetText', 'list', widget(), list(['b']));
add('widgetText', 'button', widget({ caption: 'Go שלח' }), { kind: 'pushbutton' });
add('widgetText', 'button without caption', widget({ caption: null }), { kind: 'pushbutton' });
add('widgetText', 'checkbox', widget(), toggle('checkbox'), { checked: true });
add('widgetText', 'signature', widget(), { kind: 'signature' });

// ── needsBreak (moved to core/text/src/breaks.ts) ──

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
      'packages/core/form and packages/core/text: the paint fonts, the field layout (fitting, wrapping, combs, limits, line-break classes, bidi order, fallbacks, graphemes, joins), widget drawings for every kind, border style and check style, the text a widget draws, scenes placed and turned, and the text editor layout.',
    functions,
    cases,
  },
];
