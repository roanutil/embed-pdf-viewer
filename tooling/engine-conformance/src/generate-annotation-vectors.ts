/**
 * Emits `vectors/annotation/vocabulary.json`: every annotation enum the read
 * path decodes, swept across a code range that runs past both ends of each
 * enum, plus the `/IT` and `/State` string maps.
 *
 * From the repository root:
 *   pnpm --filter @embedpdf/tooling-engine-conformance run annotation
 *   pnpm --filter @embedpdf/tooling-engine-conformance run annotation:check
 *
 * This is the pure half of G7's gate. `crates/epdf-engine/tests/
 * annotation_vocabulary.rs` runs the Rust codecs against the same file.
 *
 * The sweep matters more than the table. Every one of these decoders is total
 * and most of them fall back to a default, so the entries a hand-written test
 * would cover are the ones that cannot go wrong; what goes wrong is code 10
 * of `FPDF_ANNOT_LINE_END` (`Unknown`, which has no wire name), or the gap
 * between the note icon block and the file-attachment block, or a negative
 * that a cast wraps into the last table entry. Those are all outside the
 * enum, so the range is too.
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  ANNOTATION_SUBTYPES,
  PDF_SUBTYPE_TO_CODE,
  subtypeFromCode,
} from '../../../packages/engine/core/src/annotation/subtype';
import {
  ANNOT_FLAG_BITS,
  bitsToFlags,
} from '../../../packages/engine/services/src/features/annotations/internal/annotationFlagBits';
import {
  FILE_CODE_TO_ICON,
  FILE_ICON_TO_CODE,
  NOTE_CODE_TO_ICON,
  NOTE_ICON_TO_CODE,
} from '../../../packages/engine/services/src/features/annotations/internal/annotationIcon';
import {
  stateFromPdf,
  stateModelFromPdf,
  stateModelToPdf,
  stateToPdf,
} from '../../../packages/engine/services/src/features/annotations/internal/annotationState';
import {
  blendModeFromCode,
  blendModeToCode,
} from '../../../packages/engine/services/src/features/annotations/internal/blendMode';
import { FPDFANNOT_COLORTYPE } from '../../../packages/engine/services/src/features/annotations/internal/colorType';
import {
  freeTextIntentFromName,
  freeTextIntentToName,
} from '../../../packages/engine/services/src/features/annotations/internal/freeTextIntent';
import {
  inkIntentFromName,
  inkIntentToName,
} from '../../../packages/engine/services/src/features/annotations/internal/inkIntent';
import {
  lineEndingFromCode,
  lineEndingToCode,
} from '../../../packages/engine/services/src/features/annotations/internal/lineEnding';
import {
  replyTypeFromCode,
  replyTypeToCode,
} from '../../../packages/engine/services/src/features/annotations/internal/replyType';
import {
  borderStyleFromCode,
  borderStyleToCode,
} from '../../../packages/engine/services/src/features/annotations/internal/shapeBorderStyle';
import {
  STAMP_CODE_TO_NAME,
  STAMP_NAME_TO_CODE,
} from '../../../packages/engine/services/src/features/annotations/internal/stampName';
import {
  standardFontFromCode,
  standardFontToCode,
} from '../../../packages/engine/services/src/features/annotations/internal/standardFont';
import {
  textAlignmentFromCode,
  textAlignmentToCode,
} from '../../../packages/engine/services/src/features/annotations/internal/textAlignment';
import {
  caretIntentFromName,
  caretIntentToName,
  strikeoutIntentFromName,
  strikeoutIntentToName,
} from '../../../packages/engine/services/src/features/annotations/internal/textEditIntent';
import type {
  AnnotationBorderStyle,
  AnnotationSubtype,
  BlendMode,
  FileAttachmentIcon,
  LineEnding,
  NoteIcon,
  StandardFont,
  TextAlignment,
} from '../../../packages/engine/core/src/index';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const vectorsRoot = join(packageRoot, 'vectors', 'annotation');

/** Bump when the envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 1;

/**
 * The swept range. Low enough to catch a negative wrapping into a table's
 * tail, high enough to run past the stamp block, which ends at 35 and is the
 * largest `FPDF_ANNOT_NAME` value.
 */
const CODE_RANGE = { from: -2, to: 40 } as const;

function codes(): number[] {
  const out: number[] = [];
  for (let code = CODE_RANGE.from; code <= CODE_RANGE.to; code += 1) out.push(code);
  return out;
}

interface CodecVector {
  readonly name: string;
  /** What this codec decodes, for a reader of the file. */
  readonly of: string;
  /**
   * `decoded[i]` is the wire value for code `CODE_RANGE.from + i`. `null`
   * where the map has no entry and the caller supplies the default.
   */
  readonly decoded: readonly (string | null)[];
  /** Wire value to the code a write path stores. */
  readonly encoded: readonly (readonly [string, number])[];
}

function sweep<T extends string>(
  name: string,
  of: string,
  decode: (code: number) => T | null | undefined,
  encodePairs: readonly (readonly [T, number])[],
): CodecVector {
  return {
    name,
    of,
    decoded: codes().map((code) => decode(code) ?? null),
    encoded: encodePairs.map(([wire, code]) => [wire, code] as const),
  };
}

const BLEND_MODES: readonly BlendMode[] = [
  'normal',
  'multiply',
  'screen',
  'overlay',
  'darken',
  'lighten',
  'color-dodge',
  'color-burn',
  'hard-light',
  'soft-light',
  'difference',
  'exclusion',
  'hue',
  'saturation',
  'color',
  'luminosity',
];

const LINE_ENDINGS: readonly LineEnding[] = [
  'none',
  'square',
  'circle',
  'diamond',
  'open-arrow',
  'closed-arrow',
  'butt',
  'r-open-arrow',
  'r-closed-arrow',
  'slash',
];

const BORDER_STYLES: readonly AnnotationBorderStyle[] = ['solid', 'dashed', 'beveled', 'inset'];

const STANDARD_FONTS: readonly StandardFont[] = [
  'courier',
  'courier-bold',
  'courier-bold-oblique',
  'courier-oblique',
  'helvetica',
  'helvetica-bold',
  'helvetica-bold-oblique',
  'helvetica-oblique',
  'times-roman',
  'times-bold',
  'times-bold-italic',
  'times-italic',
  'symbol',
  'zapf-dingbats',
];

const TEXT_ALIGNMENTS: readonly TextAlignment[] = ['left', 'center', 'right'];

const NOTE_ICONS = Object.keys(NOTE_ICON_TO_CODE) as NoteIcon[];
const FILE_ICONS = Object.keys(FILE_ICON_TO_CODE) as FileAttachmentIcon[];

const CODECS: readonly CodecVector[] = [
  sweep(
    'subtype',
    'FPDFAnnot_GetSubtype',
    (code) => subtypeFromCode(code),
    ANNOTATION_SUBTYPES.map((s: AnnotationSubtype) => [s, PDF_SUBTYPE_TO_CODE[s]] as const),
  ),
  sweep(
    'blendMode',
    'FPDF_BLENDMODE',
    (code) => blendModeFromCode(code),
    BLEND_MODES.map((m) => [m, blendModeToCode(m)] as const),
  ),
  sweep(
    'lineEnding',
    'FPDF_ANNOT_LINE_END',
    (code) => lineEndingFromCode(code),
    LINE_ENDINGS.map((e) => [e, lineEndingToCode(e)] as const),
  ),
  sweep(
    'borderStyle',
    'FPDF_ANNOT_BORDER_STYLE',
    (code) => borderStyleFromCode(code),
    BORDER_STYLES.map((s) => [s, borderStyleToCode(s)] as const),
  ),
  sweep(
    'standardFont',
    'FPDF_STANDARD_FONT',
    (code) => standardFontFromCode(code),
    STANDARD_FONTS.map((f) => [f, standardFontToCode(f)] as const),
  ),
  sweep(
    'textAlignment',
    '/Q quadding',
    (code) => textAlignmentFromCode(code),
    TEXT_ALIGNMENTS.map((a) => [a, textAlignmentToCode(a)] as const),
  ),
  sweep(
    'replyType',
    'FPDF_ANNOT_REPLY_TYPE, gated on /IRT',
    (code) => replyTypeFromCode(code),
    (['reply', 'group'] as const).map((r) => [r, replyTypeToCode(r)] as const),
  ),
  sweep(
    'noteIcon',
    'FPDF_ANNOT_NAME, the text block',
    (code) => NOTE_CODE_TO_ICON[code],
    NOTE_ICONS.map((i) => [i, NOTE_ICON_TO_CODE[i]] as const),
  ),
  sweep(
    'fileAttachmentIcon',
    'FPDF_ANNOT_NAME, the file-attachment block',
    (code) => FILE_CODE_TO_ICON[code],
    FILE_ICONS.map((i) => [i, FILE_ICON_TO_CODE[i]] as const),
  ),
  sweep(
    'stampName',
    'FPDF_ANNOT_NAME, the stamp block',
    (code) => STAMP_CODE_TO_NAME[code],
    Object.entries(STAMP_NAME_TO_CODE).map(([name, code]) => [name, code] as const),
  ),
];

/**
 * The `/IT` maps. Names rather than codes, so they are swept over the names
 * that appear in the wild plus the near misses: a differing capital is the
 * failure these maps actually have, and `StrikeOutTextEdit` carries one.
 */
const INTENT_PROBES: readonly (string | null)[] = [
  null,
  '',
  'FreeText',
  'FreeTextCallout',
  'FreeTextTypeWriter',
  'freetextcallout',
  'InkHighlight',
  'inkhighlight',
  'Replace',
  'replace',
  'StrikeOutTextEdit',
  'StrikeoutTextEdit',
  'Highlight',
];

interface IntentVector {
  readonly name: string;
  /** `decoded[i]` for `INTENT_PROBES[i]`. `null` where the map has no entry. */
  readonly decoded: readonly (string | null)[];
  readonly encoded: readonly (readonly [string, string])[];
}

const INTENTS: readonly IntentVector[] = [
  {
    name: 'freeTextIntent',
    decoded: INTENT_PROBES.map((probe) => freeTextIntentFromName(probe)),
    encoded: (['free-text', 'free-text-callout'] as const).map(
      (i) => [i, freeTextIntentToName(i)] as const,
    ),
  },
  {
    name: 'inkIntent',
    decoded: INTENT_PROBES.map((probe) => inkIntentFromName(probe)),
    encoded: [['ink-highlight', inkIntentToName('ink-highlight')] as const],
  },
  {
    name: 'caretIntent',
    decoded: INTENT_PROBES.map((probe) => caretIntentFromName(probe)),
    encoded: [['replace', caretIntentToName('replace')] as const],
  },
  {
    name: 'strikeoutIntent',
    decoded: INTENT_PROBES.map((probe) => strikeoutIntentFromName(probe)),
    encoded: [
      ['strikeout-text-edit', strikeoutIntentToName('strikeout-text-edit')] as const,
    ],
  },
];

/**
 * `/State` and `/StateModel`. Swept over both spellings of every standard
 * value plus values no table holds, because the pass-through is the behaviour
 * that keeps a foreign review workflow intact and the one a closed enum would
 * break.
 */
const STATE_PROBES: readonly string[] = [
  'review',
  'Review',
  'marked',
  'Marked',
  'unmarked',
  'Unmarked',
  'accepted',
  'Accepted',
  'rejected',
  'Rejected',
  'cancelled',
  'Cancelled',
  'completed',
  'Completed',
  'none',
  'None',
  '',
  'Escalated',
  'legal-hold',
  'Review ',
  'REVIEW',
];

interface StateVector {
  readonly probe: string;
  readonly stateToPdf: string;
  readonly stateFromPdf: string;
  readonly stateModelToPdf: string;
  readonly stateModelFromPdf: string;
}

const STATES: readonly StateVector[] = STATE_PROBES.map((probe) => ({
  probe,
  stateToPdf: stateToPdf(probe),
  stateFromPdf: stateFromPdf(probe),
  stateModelToPdf: stateModelToPdf(probe),
  stateModelFromPdf: stateModelFromPdf(probe),
}));

/**
 * The `/F` word. Every single bit on its own, which is what catches a
 * transposition, plus the words where a reserved bit is set and the reader
 * has to drop it rather than report it.
 */
const FLAG_PROBES: readonly number[] = [
  0,
  ...Object.values(ANNOT_FLAG_BITS),
  0x3ff,
  0x400,
  0xffffffff,
  0x7fffffff,
  0b1010101010,
  0b0101010101,
];

const FLAGS = FLAG_PROBES.map((bits) => ({ bits, flags: bitsToFlags(bits) }));

function serialize(): string {
  const head = JSON.stringify(
    {
      schemaVersion: SCHEMA_VERSION,
      kind: 'annotation.vocabulary',
      codeRange: CODE_RANGE,
      colorType: FPDFANNOT_COLORTYPE,
      intentProbes: INTENT_PROBES,
      flagBits: ANNOT_FLAG_BITS,
    },
    null,
    2,
  ).replace(/\n}$/, ',');
  const section = (label: string, entries: readonly unknown[]): string =>
    `  ${JSON.stringify(label)}: [\n${entries.map((e) => `    ${JSON.stringify(e)}`).join(',\n')}\n  ]`;
  return `${head}\n${[
    section('codecs', CODECS),
    section('intents', INTENTS),
    section('states', STATES),
    section('flags', FLAGS),
  ].join(',\n')}\n}\n`;
}

async function main(): Promise<void> {
  const check = process.argv.includes('--check');
  const text = serialize();
  const absolute = join(vectorsRoot, 'vocabulary.json');

  let current: string | undefined;
  try {
    current = await readFile(absolute, 'utf8');
  } catch {
    current = undefined;
  }
  if (current === text) {
    process.stdout.write('  ok      annotation/vocabulary.json\n');
    return;
  }
  if (check) {
    process.stdout.write(
      `  DIFFERS annotation/vocabulary.json${current === undefined ? ' (missing)' : ''}\n`,
    );
    process.stderr.write(
      '\nAnnotation vectors differ. Regenerate with:\n' +
        '  pnpm --filter @embedpdf/tooling-engine-conformance run annotation\n' +
        'and commit the diff.\n',
    );
    process.exitCode = 1;
    return;
  }
  await mkdir(dirname(absolute), { recursive: true });
  await writeFile(absolute, text);
  process.stdout.write('  written annotation/vocabulary.json\n');
}

main().catch((error: unknown) => {
  process.stderr.write(
    `generate-annotation-vectors: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
