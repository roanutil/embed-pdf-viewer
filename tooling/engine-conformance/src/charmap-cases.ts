/**
 * The character-to-text corpus: where a page's two index spaces diverge.
 *
 * G5's entry gate asks for "fixtures with non-empty `charMap` (non-BMP,
 * non-printing), combining and RTL text" and "UTF-8-to-UTF-16 offset
 * conversion and lone-surrogate handling". This is the pure half of that: the
 * translation itself, with hand-built snapshots rather than extracted pages,
 * so a case can be exactly one deviation instead of whatever a real PDF
 * happens to contain.
 *
 * `core/src/text/charmap.ts` stays in TypeScript because web selection
 * plugins consume it, so the Rust copy is a deliberate duplicate and these
 * vectors are what keep the two the same rule.
 *
 * Every offset is a UTF-16 code unit. The Rust side works in UTF-8, so the
 * conversion is part of what it has to get right; that is why several cases
 * carry supplementary characters and why one is entirely non-ASCII.
 */

import type { CharMapAnchor } from '../../../packages/engine/core/src/text/charmap';

export interface CharMapCase {
  readonly name: string;
  readonly covers: string;
  readonly text: string;
  readonly charCount: number;
  readonly charMap?: readonly CharMapAnchor[];
}

/** A snapshot whose two spaces differ only by dropped characters. */
const EMOJI = '\u{1F600}';
const MATH_A = '\u{1D400}';

export const CHARMAP_CASES: readonly CharMapCase[] = [
  {
    name: 'identity-ascii',
    covers: 'The common page: no map, and the two spaces are the same size.',
    text: 'Hello, world.',
    charCount: 13,
  },
  {
    name: 'identity-empty',
    covers: 'A page with no text at all.',
    text: '',
    charCount: 0,
  },
  {
    name: 'identity-bmp-non-ascii',
    covers: 'Accented and CJK text is still one unit per character, so still identity.',
    text: 'café 中文 ÀÉÎ',
    charCount: 11,
  },
  {
    name: 'identity-combining',
    covers:
      'A combining mark is its own character and its own unit, so it does not make a map.',
    text: 'école',
    charCount: 6,
  },
  {
    name: 'identity-rtl',
    covers: 'Right-to-left text is a display concern, not an index one.',
    text: 'أهلا بالعالم',
    charCount: 12,
  },

  // Dropped characters: a character slot that contributes no text.
  {
    name: 'one-dropped-character-at-the-start',
    covers: 'The first character is non-printing, so text starts at character one.',
    text: 'abc',
    charCount: 4,
    charMap: [[1, 0]],
  },
  {
    name: 'one-dropped-character-in-the-middle',
    covers: 'A plateau in the middle, which is what the bias rules exist for.',
    text: 'abc',
    charCount: 4,
    charMap: [[2, 1]],
  },
  {
    name: 'one-dropped-character-at-the-end',
    covers: 'A trailing non-printing character, so the tail rule is exercised.',
    text: 'abc',
    charCount: 4,
    charMap: [[4, 3]],
  },
  {
    name: 'several-consecutive-dropped-characters',
    covers: 'A wide plateau, where a boundary search can land in the middle.',
    text: 'ab',
    charCount: 5,
    charMap: [
      [2, 1],
      [3, 1],
      [4, 1],
    ],
  },
  {
    name: 'every-character-dropped',
    covers: 'A page of nothing but non-printing characters.',
    text: '',
    charCount: 3,
    charMap: [
      [1, 0],
      [2, 0],
      [3, 0],
    ],
  },

  // Supplementary characters: one character, two text units.
  {
    name: 'one-supplementary-character',
    covers: 'A surrogate pair is one character and two units.',
    text: `a${EMOJI}b`,
    charCount: 3,
    charMap: [[2, 3]],
  },
  {
    name: 'supplementary-at-the-start',
    covers: 'The pair is the first character, so the map starts at anchor one.',
    text: `${EMOJI}ab`,
    charCount: 3,
    charMap: [[1, 2]],
  },
  {
    name: 'supplementary-at-the-end',
    covers: 'The pair is last, so the tail rule sees the expansion.',
    text: `ab${EMOJI}`,
    charCount: 3,
    charMap: [[3, 4]],
  },
  {
    name: 'consecutive-supplementary-characters',
    covers: 'Two pairs in a row, so the anchors compound.',
    text: `${EMOJI}${MATH_A}`,
    charCount: 2,
    charMap: [
      [1, 2],
      [2, 4],
    ],
  },

  // Both deviations on one page, which is the case a real document produces.
  {
    name: 'dropped-and-supplementary-together',
    covers: 'The two deviations interleaved, which is what a real page looks like.',
    // a, dropped, emoji, b, dropped: five characters projecting onto four
    // units. Boundaries run 0, 1, 1, 3, 4, 4.
    text: `a${EMOJI}b`,
    charCount: 5,
    charMap: [
      [2, 1],
      [3, 3],
      [5, 4],
    ],
  },
  {
    name: 'astral-text-with-no-ascii',
    covers: 'Nothing in the string is one byte or one unit, which is where UTF-8 maths slips.',
    text: `${EMOJI}${EMOJI}${EMOJI}`,
    charCount: 3,
    charMap: [
      [1, 2],
      [2, 4],
      [3, 6],
    ],
  },
] as const;

/**
 * The probes every case is asked, expressed as fractions so one list works for
 * pages of different sizes.
 *
 * Boundaries and offsets are enumerated exhaustively instead: every case here
 * is small, and an exhaustive sweep catches an off-by-one that a sampled one
 * would step over.
 */
export const CHARMAP_CASE_NAMES = CHARMAP_CASES.map((c) => c.name);

/** Maps that must be rejected, with the reason they are wrong. */
export interface CharMapViolationCase {
  readonly name: string;
  readonly covers: string;
  readonly charCount: number;
  readonly textLength: number;
  readonly charMap?: readonly CharMapAnchor[];
}

export const CHARMAP_VIOLATION_CASES: readonly CharMapViolationCase[] = [
  {
    name: 'valid-identity',
    covers: 'The control: a sound absent map.',
    charCount: 5,
    textLength: 5,
  },
  {
    name: 'identity-with-mismatched-sizes',
    covers: 'No map, but the spaces differ, so one of the two numbers is wrong.',
    charCount: 5,
    textLength: 4,
  },
  {
    name: 'anchor-not-increasing',
    covers: 'Two anchors at the same character.',
    charCount: 5,
    textLength: 5,
    charMap: [
      [2, 1],
      [2, 2],
    ],
  },
  {
    name: 'anchor-going-backwards',
    covers: 'An anchor before the previous one.',
    charCount: 5,
    textLength: 5,
    charMap: [
      [3, 2],
      [2, 1],
    ],
  },
  {
    name: 'anchor-at-zero',
    covers: 'The head anchor is implicit, so an explicit [0, 0] is malformed.',
    charCount: 5,
    textLength: 5,
    charMap: [[0, 0]],
  },
  {
    name: 'anchor-past-the-page',
    covers: 'An anchor beyond charCount.',
    charCount: 3,
    textLength: 3,
    charMap: [[9, 9]],
  },
  {
    name: 'step-of-one',
    covers: 'A step of one is what an anchor means NOT to encode, so it is a violation.',
    charCount: 4,
    textLength: 5,
    charMap: [[2, 2]],
  },
  {
    name: 'step-of-one-that-the-tail-rule-would-accept',
    covers:
      'Only the step rule catches this: the anchors increase, sit in range, and project onto the right length. A step of one is what an anchor means NOT to encode.',
    charCount: 4,
    textLength: 3,
    charMap: [
      [2, 2],
      [3, 2],
    ],
  },
  {
    name: 'step-of-three',
    covers: 'No character contributes three units.',
    charCount: 4,
    textLength: 7,
    charMap: [[2, 4]],
  },
  {
    name: 'negative-step',
    covers: 'A map that runs the text backwards.',
    charCount: 4,
    textLength: 2,
    charMap: [
      [1, 0],
      [2, 0],
    ],
  },
  {
    name: 'tail-too-short',
    covers: 'A sound map that projects onto the wrong length.',
    charCount: 5,
    textLength: 9,
    charMap: [[2, 3]],
  },
  {
    name: 'valid-dropped',
    covers: 'A sound map with one dropped character.',
    charCount: 4,
    textLength: 3,
    charMap: [[2, 1]],
  },
  {
    name: 'valid-supplementary',
    covers: 'A sound map with one surrogate pair.',
    charCount: 3,
    textLength: 4,
    charMap: [[2, 3]],
  },
] as const;
