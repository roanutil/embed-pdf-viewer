/**
 * `packages/core/fonts`. Replayed by `crates/plugins/fonts/tests/conformance.rs`.
 *
 * Pure functions, so one function suite. Three entries are wrappers, because
 * the shipping values carry closures or a promise a function case can't
 * record:
 *
 * - `standardFontMetrics(font, codePoints)` builds the metrics and asks
 *   `advance` for each code point the case supplies.
 * - `standardFontProgram(font)` records the program's data members.
 * - `standardFontProgramLoad(font)` returns the bytes the shipping `load()`
 *   resolved with. Every face is loaded once when this module is imported
 *   (function cases are synchronous), and the case looks the program up
 *   through `standardFontProgram` first, so an unknown name throws as it would.
 *
 * `decodeBase64` is internal to the package; its cases pin the padding rules
 * the program bytes go through.
 */
import { STANDARD_FONTS } from '@embedpdf/engine-core/runtime';

import { decodeBase64 } from '../../../../packages/core/fonts/src/base64';
import {
  fontProgramFamily,
  standardFontCss,
  standardFontMetrics,
  standardFontProgram,
  type FontProgram,
} from '../../../../packages/core/fonts/src/index';
import { FACES } from '../../../../packages/core/fonts/src/standard/faces.generated';

import type { FunctionCase, Suite } from '../vectors';

const loaded = new Map<FontProgram, Uint8Array>();
for (const font of STANDARD_FONTS) {
  const program = standardFontProgram(font);
  loaded.set(program, await program.load());
}

const functions = {
  standardFontMetrics: (font: string, codePoints: readonly number[]) => {
    const metrics = standardFontMetrics(font as (typeof STANDARD_FONTS)[number]);
    return {
      font: metrics.font,
      ascent: metrics.ascent,
      descent: metrics.descent,
      advances: codePoints.map((codePoint) => metrics.advance(codePoint)),
    };
  },
  standardFontProgram: (font: string) => {
    const program = standardFontProgram(font as (typeof STANDARD_FONTS)[number]);
    return {
      hash: program.hash,
      family: program.family,
      weight: program.weight,
      style: program.style,
    };
  },
  standardFontProgramLoad: (font: string) =>
    loaded.get(standardFontProgram(font as (typeof STANDARD_FONTS)[number])),
  standardFontCss: (font: string) => standardFontCss(font as (typeof STANDARD_FONTS)[number]),
  fontProgramFamily: (hash: string) => fontProgramFamily(hash),
  decodeBase64: (text: string) => decodeBase64(text),
};

/** Code points no face writes, or that aren't code points at all. */
const OUTSIDE = [0, 31, 127, 0x416, 0xffff, 0x10ffff, -1, 65.5, Number.NaN];

/** Names that aren't one of the 14: wrong case, a family, padding, empty. */
const UNKNOWN = ['Helvetica', 'times', 'helvetica ', 'arial', ''];

const cases: FunctionCase[] = [];
const add = (fn: keyof typeof functions, label: string, ...args: unknown[]) =>
  cases.push({ name: `${fn}/${label}`, fn, args });

for (const font of STANDARD_FONTS) {
  // Every character the face writes, in its code order, then the misses.
  add('standardFontMetrics', font, font, [...FACES[font].codePoints, ...OUTSIDE]);
}
add('standardFontMetrics', 'cross-face-probes', 'symbol', [65, 945, 8364, 9985]);
for (const name of UNKNOWN) add('standardFontMetrics', `unknown:${name}`, name, [65]);

for (const font of STANDARD_FONTS) add('standardFontProgram', font, font);
for (const name of UNKNOWN) add('standardFontProgram', `unknown:${name}`, name);

for (const font of STANDARD_FONTS) add('standardFontProgramLoad', font, font);
for (const name of UNKNOWN) add('standardFontProgramLoad', `unknown:${name}`, name);

for (const font of STANDARD_FONTS) add('standardFontCss', font, font);
for (const name of UNKNOWN) add('standardFontCss', `unknown:${name}`, name);

for (const font of STANDARD_FONTS) add('fontProgramFamily', font, FACES[font].programHash);
add('fontProgramFamily', 'empty', '');
add('fontProgramFamily', 'short', 'abc');
add('fontProgramFamily', 'exactly-16', '0123456789abcdef');
// `slice` counts UTF-16 code units, not bytes.
add('fontProgramFamily', 'non-ascii', 'é'.repeat(20));

add('decodeBase64', 'empty', '');
add('decodeBase64', 'two-pad', 'TQ==');
add('decodeBase64', 'one-pad', 'TWE=');
add('decodeBase64', 'no-pad', 'TWFu');
add('decodeBase64', 'otto-header', 'T1RUTwAJAIAAAwAQ');
add('decodeBase64', 'high-bytes', '/+7dzLuqmYh3ZlVEMyIRAA==');

export const suites: Suite[] = [
  {
    kind: 'function',
    file: 'core/fonts.json',
    describes:
      'packages/core/fonts: the 14 standard faces: metrics and advances, program members and loaded bytes, CSS, program families, base64 decoding, unknown names.',
    functions,
    cases,
  },
];
