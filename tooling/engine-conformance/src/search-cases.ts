/**
 * The search dialect corpus: what `regex.ts` accepts, what it refuses, and
 * what it matches, plus what `fold.ts` produces.
 *
 * `multiplatform-port-order.md` G5 gives this an entry gate rather than an
 * exit gate, and lists its contents: "`regex.ts` accepted and rejected syntax,
 * `\d` `\w` `\b` under JavaScript `u` and `iu` modes (including K and ſ),
 * `\s`, Unicode properties, dot/line terminators, case folding, `^` `$` under
 * `m`, zero-length matches and cursor advance, fold map back to UTF-16
 * offsets". Nothing in G5 starts until a Rust engine can reproduce these.
 *
 * Offsets everywhere are UTF-16 code units, because that is what JavaScript
 * produces and what `PageTextSnapshot` addresses. A Rust engine works in
 * UTF-8, so converting is part of what the candidate has to get right; that is
 * why the vectors carry deliberately non-BMP text.
 */

import { toPageRef, type PageRef } from '../../../packages/engine/core/src/identity/PageRef';

export interface RegexCase {
  /** Stable slug, and what a failure names. */
  readonly name: string;
  /** Why the case exists, in one line. */
  readonly covers: string;
  readonly pattern: string;
  /** `matchCase: false` is the default, and means the `i` flag. */
  readonly matchCase?: boolean;
  /** Post-filter with the shared Unicode word test, never `\b`. */
  readonly wholeWord?: boolean;
  /** Texts the pattern runs over. Empty for a case that only tests syntax. */
  readonly texts: readonly string[];
}

/**
 * Shared haystacks, so a case that is about a pattern does not also have to
 * be about a string literal.
 */
const ASCII = 'The cat sat on the CAT mat. Concatenate.';
const LINES = 'first line\nsecond line\nthird line';
const ACCENTED = 'café CAFÉ café Café';
const ASTRAL = 'a\u{1F600}b\u{1D400}c'; // emoji and MATHEMATICAL BOLD CAPITAL A
const FOLDING = 'KK k sſ S ß ss ς σ'; // K, Kelvin, k, s, long s, S, sharp s, ss, final sigma, sigma
const MIXED_SCRIPT = 'abc أهلا 123 中文 ́combining';

/** A literal query and the page it runs against. */
export interface LiteralCase {
  readonly name: string;
  readonly covers: string;
  readonly haystack: string;
  readonly needle: string;
  readonly matchCase?: boolean;
  readonly matchDiacritics?: boolean;
  readonly wholeWord?: boolean;
}

/**
 * Literal search, which is the default mode and the one most queries use.
 *
 * The cases lean on the interactions rather than on breadth: folding against
 * whole-word, folding against diacritics, and the places a fold changes the
 * length of what it matched so the range has to translate back.
 */
export const LITERAL_CASES: readonly LiteralCase[] = [
  {
    name: 'plain-ascii',
    covers: 'The dominant query.',
    haystack: 'The cat sat on the mat.',
    needle: 'cat',
  },
  {
    name: 'case-insensitive-by-default',
    covers: 'A query with no flags ignores case, which is what a search box does.',
    haystack: 'The CAT sat.',
    needle: 'cat',
  },
  {
    name: 'case-sensitive',
    covers: 'matchCase keeps the fold from lowering either side.',
    haystack: 'The CAT sat on the cat.',
    needle: 'cat',
    matchCase: true,
  },
  {
    name: 'accents-ignored-by-default',
    covers: 'café is found by cafe, which is the fold doing its job.',
    haystack: 'Un caf\u00e9 noir.',
    needle: 'cafe',
  },
  {
    name: 'accents-respected',
    covers:
      'matchDiacritics keeps the marks, so cafe must not match caf\u00e9 by stopping before the accent.',
    haystack: 'Un caf\u00e9 noir et un cafe clair.',
    needle: 'cafe',
    matchDiacritics: true,
  },
  {
    name: 'ligature-in-the-page',
    covers: 'NFKD splits the ligature, so a two-letter needle finds a one-character page.',
    haystack: 'The \ufb01nal word.',
    needle: 'final',
  },
  {
    name: 'ligature-in-the-needle',
    covers: 'The same fold applies to what was typed.',
    haystack: 'The final word.',
    needle: '\ufb01nal',
  },
  {
    name: 'line-wrapped-phrase',
    covers: 'A whitespace run collapses, so a wrapped phrase is still one match.',
    haystack: 'hello\n   world',
    needle: 'hello world',
  },
  {
    name: 'needle-with-its-own-whitespace-run',
    covers: 'Both sides collapse, so the query need not guess the page layout.',
    haystack: 'hello world',
    needle: 'hello    world',
  },
  {
    name: 'whole-word-rejects-a-prefix',
    covers: 'Concatenate contains cat and is not the word cat.',
    haystack: 'The cat in Concatenate.',
    needle: 'cat',
    wholeWord: true,
  },
  {
    name: 'whole-word-across-an-accent',
    covers:
      'The word test uses Unicode letters, so caf never whole-word matches inside caf\u00e9 even after the accent is folded away.',
    haystack: 'Un caf\u00e9.',
    needle: 'caf',
    wholeWord: true,
  },
  {
    name: 'whole-word-against-a-non-ascii-letter',
    covers:
      'The word test is Unicode letters, not the ASCII set. A CJK character beside the needle is a word character, so this must NOT match; an ASCII-only rule would return a hit.',
    haystack: '\u732bcat\u732b and a lone cat',
    needle: 'cat',
    wholeWord: true,
  },
  {
    name: 'whole-word-against-cyrillic',
    covers: 'The same rule in another script, so the case is not about CJK width.',
    haystack: '\u0434cat\u0434 and a lone cat',
    needle: 'cat',
    wholeWord: true,
  },
  {
    name: 'overlapping-candidates',
    covers: 'Matches do not overlap: the scan resumes after the one it took.',
    haystack: 'aaaa',
    needle: 'aa',
  },
  {
    name: 'empty-needle',
    covers: 'Nothing searchable, which would otherwise match every position.',
    haystack: 'anything',
    needle: '',
  },
  {
    name: 'whitespace-only-needle',
    covers: 'Folds to a single space, which would match every collapsed run.',
    haystack: 'a   b',
    needle: '   ',
  },
  {
    name: 'needle-longer-than-the-page',
    covers: 'The scan must not run off the end.',
    haystack: 'ab',
    needle: 'abcdef',
  },
  {
    name: 'no-match',
    covers: 'The empty answer.',
    haystack: 'The cat sat.',
    needle: 'dog',
  },
  {
    name: 'sharp-s-folds-to-ss',
    covers: 'The case round trip, which a plain lowercase would miss.',
    haystack: 'Stra\u00dfe',
    needle: 'strasse',
  },
  {
    name: 'astral-haystack',
    covers:
      'A supplementary character is two code units, so a match after it has to translate through the map rather than by counting characters.',
    haystack: 'a\u{1F600}bcd',
    needle: 'bcd',
  },
  {
    name: 'match-at-the-very-start',
    covers: 'Offset zero, where an off-by-one hides.',
    haystack: 'cat sat',
    needle: 'cat',
  },
  {
    name: 'match-at-the-very-end',
    covers: 'The last unit, where the other off-by-one hides.',
    haystack: 'sat cat',
    needle: 'cat',
  },
] as const;

/** A snippet request: a page, a match inside it, and the context width. */
export interface SnippetCase {
  readonly name: string;
  readonly covers: string;
  readonly text: string;
  readonly matchStart: number;
  readonly matchLength: number;
  readonly context: number;
}

/**
 * The excerpt shown around a match.
 *
 * The rules interact at the edges, which is where the cases sit: a window that
 * reaches the start of the page, one that finds no whitespace to cut at, and
 * one whose boundary lands inside a surrogate pair.
 */
const LOREM =
  'The quick brown fox jumps over the lazy dog. Pack my box with five dozen liquor jugs. How vexingly quick daft zebras jump.';

export const SNIPPET_CASES: readonly SnippetCase[] = [
  {
    name: 'middle-of-a-long-page',
    covers: 'Context on both sides, both trimmed to a word boundary.',
    text: LOREM,
    matchStart: 60,
    matchLength: 3,
    context: 20,
  },
  {
    name: 'match-at-the-very-start',
    covers: 'No leading context to trim, so the snippet starts at zero.',
    text: LOREM,
    matchStart: 0,
    matchLength: 3,
    context: 20,
  },
  {
    name: 'match-at-the-very-end',
    covers: 'No trailing context, so the snippet ends at the page end.',
    text: LOREM,
    matchStart: LOREM.length - 5,
    matchLength: 5,
    context: 20,
  },
  {
    name: 'whole-page-inside-the-context',
    covers: 'The window covers everything, so neither side trims.',
    text: 'short page',
    matchStart: 6,
    matchLength: 4,
    context: 100,
  },
  {
    name: 'no-whitespace-to-cut-at',
    covers:
      'One very long token: the rule accepts a mid-word cut rather than widening the window.',
    text: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaTARGETaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
    matchStart: 30,
    matchLength: 6,
    context: 10,
  },
  {
    name: 'zero-context',
    covers: 'The snippet is the match.',
    text: LOREM,
    matchStart: 4,
    matchLength: 5,
    context: 0,
  },
  {
    name: 'whitespace-flattened-length-preserving',
    covers:
      'Newlines and tabs become single spaces one for one, so the reported match offset stays valid.',
    text: 'first line\n\tsecond\u00a0line here and more',
    matchStart: 12,
    matchLength: 6,
    context: 10,
  },
  {
    name: 'boundary-inside-a-surrogate-pair',
    covers:
      'The window edge lands between the halves of an astral character, and must move rather than split it.',
    text: `aaaa\u{1F600}aaaaTARGETaaaa\u{1F600}aaaa`,
    matchStart: 10,
    matchLength: 6,
    context: 5,
  },
] as const;

export const REGEX_CASES: readonly RegexCase[] = [
  // --- Dialect membership: what the validator refuses before anything runs.
  {
    name: 'empty-pattern',
    covers: 'An empty pattern is refused, not matched.',
    pattern: '',
    texts: [],
  },
  {
    name: 'too-long',
    covers: 'SEARCH_REGEX_MAX_LENGTH is 512 and the 513th character is one too many.',
    pattern: 'a'.repeat(513),
    texts: [],
  },
  {
    name: 'at-the-length-limit',
    covers: 'Exactly 512 is accepted, so the boundary is a boundary and not an off-by-one.',
    pattern: 'a'.repeat(512),
    texts: [],
  },
  {
    name: 'numbered-backreference',
    covers: 'RE2 has no backreferences, so the dialect has none either.',
    pattern: '(a)\\1',
    texts: [],
  },
  {
    name: 'named-backreference',
    covers: 'The \\k<name> spelling of the same thing.',
    pattern: '(?<x>a)\\k<x>',
    texts: [],
  },
  {
    name: 'backreference-inside-a-class',
    covers: '\\1 inside [] is an escape, not a backreference, and stays legal.',
    pattern: '[\\1]',
    texts: ['\u0001x'],
  },
  {
    name: 'lookahead',
    covers: 'Positive lookahead is out of the dialect.',
    pattern: 'a(?=b)',
    texts: [],
  },
  {
    name: 'negative-lookahead',
    covers: 'And the negative form.',
    pattern: 'a(?!b)',
    texts: [],
  },
  { name: 'lookbehind', covers: 'Positive lookbehind is out.', pattern: '(?<=a)b', texts: [] },
  {
    name: 'negative-lookbehind',
    covers: 'And the negative form.',
    pattern: '(?<!a)b',
    texts: [],
  },
  {
    name: 'named-group-is-not-lookbehind',
    covers: '(?<name> opens a group; only (?<= and (?<! are lookbehind.',
    pattern: '(?<word>\\w+)',
    texts: [ASCII],
  },
  {
    name: 'non-capturing-group',
    covers: '(?: stays legal.',
    pattern: '(?:ca)+t',
    texts: [ASCII],
  },
  {
    name: 'unbalanced-paren',
    covers: 'Syntax the RegExp constructor itself refuses.',
    pattern: '(a',
    texts: [],
  },
  {
    name: 'lone-brace-under-u',
    covers: 'u mode refuses a bare { that v-less JavaScript would have tolerated.',
    pattern: 'a{',
    texts: [],
  },
  {
    name: 'invalid-unicode-property',
    covers: 'A property name that does not exist.',
    pattern: '\\p{Nope}',
    texts: [],
  },

  // --- Character classes, under u and iu.
  {
    name: 'digit-class',
    covers: '\\d is ASCII 0-9 in JavaScript, not every Unicode digit.',
    pattern: '\\d+',
    matchCase: true,
    texts: [MIXED_SCRIPT, '٠١٢ 012'],
  },
  {
    name: 'word-class',
    covers: '\\w is [A-Za-z0-9_] in JavaScript even under u, unlike the shared word test.',
    pattern: '\\w+',
    matchCase: true,
    texts: [MIXED_SCRIPT, ACCENTED],
  },
  {
    name: 'word-boundary',
    covers: '\\b uses the same ASCII word set, which is why wholeWord never uses it.',
    pattern: '\\bcafe\\b',
    matchCase: true,
    texts: [ACCENTED, 'cafe cafes'],
  },
  {
    name: 'word-boundary-under-i',
    covers: 'The case-insensitive form of the same boundary.',
    pattern: '\\bcat\\b',
    texts: [ASCII],
  },
  {
    name: 'whitespace-class',
    covers: '\\s includes the Unicode spaces, unlike \\w and \\d.',
    pattern: '\\s+',
    matchCase: true,
    texts: ['a b\tc\nd e f'],
  },
  {
    name: 'not-whitespace',
    covers: 'The negated form over the same text.',
    pattern: '\\S+',
    matchCase: true,
    texts: ['a b\tc\nd e f'],
  },
  {
    name: 'unicode-letter-property',
    covers: '\\p{L} is the property the shared word test actually uses.',
    pattern: '\\p{L}+',
    matchCase: true,
    texts: [MIXED_SCRIPT, ACCENTED],
  },
  {
    name: 'unicode-script-property',
    covers: 'Script properties, which RE2 also has.',
    pattern: '\\p{Script=Arabic}+',
    matchCase: true,
    texts: [MIXED_SCRIPT],
  },
  {
    name: 'unicode-mark-property',
    covers: '\\p{M}, the property fold.ts strips.',
    pattern: '\\p{M}+',
    matchCase: true,
    texts: [ACCENTED, MIXED_SCRIPT],
  },
  {
    name: 'negated-unicode-property',
    covers: '\\P{L}, the negated form.',
    pattern: '\\P{L}+',
    matchCase: true,
    texts: ['ab12cd'],
  },

  // --- Case folding, which is where an ASCII-only fold diverges loudly.
  {
    name: 'kelvin-sign-folds-to-k',
    covers: 'U+212A KELVIN SIGN case-folds to k under iu. An ASCII fold misses it.',
    pattern: 'k',
    texts: [FOLDING],
  },
  {
    name: 'long-s-folds-to-s',
    covers: 'U+017F LATIN SMALL LETTER LONG S folds to s under iu.',
    pattern: 's',
    texts: [FOLDING],
  },
  {
    name: 'kelvin-sign-under-match-case',
    covers: 'With matchCase the same pattern must not reach either of them.',
    pattern: 'k',
    matchCase: true,
    texts: [FOLDING],
  },
  {
    name: 'final-sigma-folds-to-sigma',
    covers: 'Greek final sigma folds to sigma under iu, which plain toLowerCase also does.',
    pattern: 'σ',
    texts: [FOLDING],
  },
  {
    name: 'sharp-s-does-not-expand-in-regex-mode',
    covers: 'Unlike fold.ts, regex simple folding does not turn ss into sharp s.',
    pattern: 'ss',
    texts: [FOLDING],
  },
  {
    name: 'accented-under-i',
    covers: 'Case folding is not diacritic folding: regex runs on the raw plane.',
    pattern: 'café',
    texts: [ACCENTED],
  },

  // --- Anchors and line terminators. The matcher always sets m.
  {
    name: 'caret-under-m',
    covers: '^ is a line start because the matcher always passes m.',
    pattern: '^\\w+',
    matchCase: true,
    texts: [LINES],
  },
  {
    name: 'dollar-under-m',
    covers: 'And $ is a line end.',
    pattern: '\\w+$',
    matchCase: true,
    texts: [LINES],
  },
  {
    name: 'dot-does-not-cross-a-newline',
    covers: 'No s flag, so . stops at \\n.',
    pattern: 'line.second',
    matchCase: true,
    texts: [LINES],
  },
  {
    name: 'caret-after-a-carriage-return',
    covers:
      'JavaScript m-mode anchors break on \\r, U+2028 and U+2029 as well as \\n. Most engines only know \\n.',
    pattern: '^\\w+',
    matchCase: true,
    texts: ['one\rtwo\u2028three\u2029four'],
  },
  {
    name: 'dollar-before-a-carriage-return',
    covers: 'The same question at the other anchor.',
    pattern: '\\w+$',
    matchCase: true,
    texts: ['one\rtwo\u2028three\u2029four'],
  },
  {
    name: 'anchored-pattern-matching-a-literal-carriage-return',
    covers:
      'An anchored pattern runs against a haystack whose terminators are normalized, so a literal \\r in the same pattern sees the normalized character.',
    pattern: '^a\\r',
    matchCase: true,
    texts: ['a\rb'],
  },
  {
    name: 'anchored-pattern-with-a-negated-newline-class',
    covers: 'The other face of the same normalization: [^\\n] over a carriage return.',
    pattern: '^[^\\n]+',
    matchCase: true,
    texts: ['a\rb\nc'],
  },
  {
    name: 'unanchored-negated-newline-class',
    covers:
      'Without an anchor there is no normalization and no approximation, which is what keeps the cost off ordinary queries.',
    pattern: '[^\\n]+',
    matchCase: true,
    texts: ['a\rb\nc'],
  },
  {
    name: 'unanchored-literal-carriage-return',
    covers: 'Same again for a literal terminator.',
    pattern: 'a\\rb',
    matchCase: true,
    texts: ['a\rb'],
  },
  {
    name: 'dot-and-the-other-line-terminators',
    covers: 'JavaScript . also stops at \\r, U+2028 and U+2029.',
    pattern: 'a.b',
    matchCase: true,
    texts: ['a\rb a b a b a\u000bb'],
  },

  // --- Zero-length matches and cursor advance.
  {
    name: 'star-matches-nothing-and-does-not-loop',
    covers: 'Zero-length matches are skipped; the cursor advances a full code point.',
    pattern: 'x*',
    matchCase: true,
    texts: [ASTRAL, 'xx yy'],
  },
  {
    name: 'empty-alternative',
    covers: 'Another way to spell a zero-length match.',
    pattern: '(?:a|)',
    matchCase: true,
    texts: ['bab'],
  },
  {
    name: 'anchored-empty-match',
    covers: 'A pattern that matches only emptiness finds nothing at all.',
    pattern: '^',
    matchCase: true,
    texts: [LINES],
  },

  // --- Astral text, where UTF-16 and UTF-8 offsets disagree.
  {
    name: 'astral-literal',
    covers: 'A supplementary-plane character is one code point and two UTF-16 units.',
    pattern: '\u{1F600}',
    matchCase: true,
    texts: [ASTRAL],
  },
  {
    name: 'dot-spans-a-surrogate-pair',
    covers: 'Under u, . is one code point, so the match length is 2 in UTF-16 units.',
    pattern: 'a.b',
    matchCase: true,
    texts: [ASTRAL],
  },
  {
    name: 'astral-range-in-a-class',
    covers: 'A class range over supplementary code points, which needs u.',
    pattern: '[\u{1D400}-\u{1D419}]+',
    matchCase: true,
    texts: [ASTRAL],
  },

  // --- wholeWord, which is a post-filter and not \b.
  {
    name: 'whole-word-post-filter',
    covers: 'cat inside Concatenate is dropped by the shared word test.',
    pattern: 'cat',
    wholeWord: true,
    texts: [ASCII],
  },
  {
    name: 'whole-word-is-unicode-aware',
    covers: 'caf never whole-word matches inside café, which \\b would allow.',
    pattern: 'caf',
    wholeWord: true,
    texts: [ACCENTED],
  },
  {
    name: 'whole-word-with-a-trailing-underscore',
    covers: 'Underscore is a word character in the shared test as well.',
    pattern: 'cat',
    wholeWord: true,
    texts: ['cat cat_ _cat'],
  },

  // --- Adversarial patterns. The dialect has no backtracking escape hatch,
  // and the server side is RE2, so a candidate engine has to stay linear.
  {
    name: 'nested-quantifier',
    covers: 'The classic catastrophic-backtracking shape, on a non-matching subject.',
    pattern: '(?:a+)+b',
    matchCase: true,
    texts: ['aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'],
  },
  {
    name: 'alternation-blowup',
    covers: 'The other classic, same subject.',
    pattern: '(?:a|aa)+b',
    matchCase: true,
    texts: ['aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'],
  },
  {
    name: 'wide-bounded-repeat',
    covers: 'A bounded repeat wide enough to matter for compiled-program size.',
    pattern: '(?:ab){1,200}',
    matchCase: true,
    texts: ['ab'.repeat(250)],
  },
];

export interface FoldCase {
  readonly name: string;
  readonly covers: string;
  readonly text: string;
  readonly keepCase?: boolean;
  readonly keepMarks?: boolean;
}

export const FOLD_CASES: readonly FoldCase[] = [
  {
    name: 'default-fold',
    covers: 'Case and marks both folded, which is the persistable corpus fold.',
    text: ACCENTED,
  },
  {
    name: 'keep-case',
    covers: 'matchCase: marks folded, case kept.',
    text: ACCENTED,
    keepCase: true,
  },
  {
    name: 'keep-marks',
    covers: 'matchDiacritics: case folded, marks kept and left decomposed.',
    text: ACCENTED,
    keepMarks: true,
  },
  {
    name: 'keep-both',
    covers: 'Both flags on: only whitespace collapse and NFKD remain.',
    text: ACCENTED,
    keepCase: true,
    keepMarks: true,
  },
  {
    name: 'whitespace-collapse',
    covers: 'A line-wrapped run collapses to one space, and the map still points at the first one.',
    text: 'hello \n\t  world',
  },
  {
    name: 'decomposition-emits-whitespace',
    covers:
      'U+00A8 decomposes to space plus combining diaeresis, so the collapse runs at unit level.',
    text: 'a¨¨b',
  },
  {
    name: 'ligature-expands',
    covers: 'NFKD splits a ligature into two units that both map back to one code point.',
    text: 'oﬃce',
  },
  {
    name: 'superscript-expands',
    covers: 'A compatibility decomposition that changes the character class.',
    text: 'x² y⁵',
  },
  {
    name: 'sharp-s-expands',
    covers: 'The upper-lower round trip catches ss, which toLowerCase alone misses.',
    text: 'Straße',
  },
  {
    name: 'final-sigma-normalizes',
    covers: 'And the Greek case it also catches.',
    text: 'ος οσ',
  },
  {
    name: 'astral-maps-back-to-two-units',
    covers:
      'The map is in UTF-16 code units, so a supplementary code point occupies one entry per folded unit and points at its first unit.',
    text: ASTRAL,
  },
  {
    name: 'leading-and-trailing-whitespace',
    covers: 'Collapse at the edges, which the needle trim depends on.',
    text: '  padded  ',
  },
  {
    name: 'kelvin-and-long-s',
    covers: 'The characters the regex case-folding cases use, through the other path.',
    text: FOLDING,
  },
];

/**
 * One request as it comes off the wire, and what the slice protocol makes of
 * it before a single page is scanned.
 *
 * The three things pinned here are the three a caller can get wrong without
 * the engine ever reading a page: the flag combination, the identity a cursor
 * is minted against, and whether a cursor may be resumed at all. Each one
 * refuses rather than degrades, which is the part worth pinning: a query that
 * silently returns no matches is indistinguishable from a document that
 * contains none.
 */
export interface SliceCase {
  readonly name: string;
  readonly covers: string;
  /** Exactly the JSON a caller sends. */
  readonly request: {
    readonly query: {
      readonly text: string;
      readonly regex?: boolean;
      readonly matchCase?: boolean;
      readonly wholeWord?: boolean;
      readonly matchDiacritics?: boolean;
    };
    readonly mode?: 'rects' | 'full';
    readonly cursor?: string;
    readonly startPage?: PageRef;
  };
  /** The sequence the session is on, for the cursor's staleness check. */
  readonly seq?: number;
}

export const SLICE_CASES: readonly SliceCase[] = [
  {
    name: 'literal-is-always-valid',
    covers: 'A literal query skips the pattern check entirely, flags and all.',
    request: { query: { text: 'a(', matchCase: true, matchDiacritics: true } },
  },
  {
    name: 'regex-with-diacritics-is-refused',
    covers:
      'A rejected flag combination. Diacritic folding is a property of the folded text plane and a regex runs on the raw one, so this asks for something that does not exist.',
    request: { query: { text: 'caf.', regex: true, matchDiacritics: true } },
  },
  {
    name: 'regex-with-ignore-whitespace-is-refused',
    covers:
      'The other rejected combination, for the same reason: dropping whitespace is a property of the folded plane, and a pattern spells its own \\s*.',
    request: { query: { text: 'in.oice', regex: true, ignoreWhitespace: true } },
  },
  {
    name: 'diacritics-is-reported-before-a-bad-pattern',
    covers:
      'Both wrong at once reports the flag, because that is the one a caller can fix without rewriting the pattern.',
    request: { query: { text: '(?=x)', regex: true, matchDiacritics: true } },
  },
  {
    name: 'regex-lookaround-is-refused',
    covers: 'A dialect refusal reaches the query validator with its own issue intact.',
    request: { query: { text: 'a(?=b)', regex: true } },
  },
  {
    name: 'regex-backreference-is-refused',
    covers: 'The other dialect refusal, through the same path.',
    request: { query: { text: '(a)\\1', regex: true } },
  },
  {
    name: 'empty-pattern-is-refused',
    covers: 'An empty regex is refused where an empty literal is merely fruitless.',
    request: { query: { text: '', regex: true } },
  },
  {
    name: 'key-defaults-to-full-mode',
    covers: 'An absent mode is full, and the cursor key says so.',
    request: { query: { text: 'the' } },
  },
  {
    name: 'key-spells-the-rects-mode',
    covers:
      'The spelling in the key is the wire spelling. A cursor minted by one implementation and replayed against another compares as a string, so a synonym here is a refusal no caller could diagnose.',
    request: { query: { text: 'the' }, mode: 'rects' },
  },
  {
    name: 'key-separates-every-flag',
    covers: 'Each flag moves the key, so a cursor is not transferable across a toggle.',
    request: {
      query: { text: 'the', matchCase: true, wholeWord: true, matchDiacritics: true },
      mode: 'full',
    },
  },
  {
    name: 'key-separates-ignore-whitespace',
    covers: 'ignoreWhitespace has its own slot, after wholeWord, as searchQueryKey spells it.',
    request: { query: { text: 'the', ignoreWhitespace: true } },
  },
  {
    name: 'key-separates-regex-from-literal',
    covers: 'Same text, different plane, different identity.',
    request: { query: { text: 'the', regex: true } },
  },
  {
    name: 'cursor-resumes-its-own-query',
    covers: 'The accepting case, so the refusals below are not vacuous.',
    request: {
      query: { text: 'the' },
      cursor: '{"v":1,"seq":7,"key":"[\\"l\\",\\"the\\",0,0,0,0,\\"full\\"]","start":0,"scanned":5}',
    },
    seq: 7,
  },
  {
    name: 'cursor-from-another-query-is-refused',
    covers: 'The key pins the query. Resuming under a different one would scan the wrong needle.',
    request: {
      query: { text: 'other' },
      cursor: '{"v":1,"seq":7,"key":"[\\"l\\",\\"the\\",0,0,0,0,\\"full\\"]","start":0,"scanned":5}',
    },
    seq: 7,
  },
  {
    name: 'cursor-from-before-a-mutation-is-refused',
    covers:
      'The sequence pins the content. Resuming over mutated pages could serve text a redaction just removed.',
    request: {
      query: { text: 'the' },
      cursor: '{"v":1,"seq":6,"key":"[\\"l\\",\\"the\\",0,0,0,0,\\"full\\"]","start":0,"scanned":5}',
    },
    seq: 7,
  },
  {
    name: 'cursor-beside-a-conflicting-start-page-is-refused',
    covers:
      'The cursor owns the position. Letting it quietly win would hand back matches from a part of the document the caller did not ask about.',
    request: {
      query: { text: 'the' },
      cursor: '{"v":1,"seq":7,"key":"[\\"l\\",\\"the\\",0,0,0,0,\\"full\\"]","start":0,"scanned":5}',
      startPage: toPageRef(4),
    },
    seq: 7,
  },
  {
    name: 'cursor-beside-the-matching-start-page-is-allowed',
    covers: 'A startPage that agrees with the cursor is redundant rather than wrong.',
    request: {
      query: { text: 'the' },
      cursor: '{"v":1,"seq":7,"key":"[\\"l\\",\\"the\\",0,0,0,0,\\"full\\"]","start":4,"scanned":5}',
      startPage: toPageRef(4),
    },
    seq: 7,
  },
  {
    name: 'a-cursor-that-is-not-json-is-refused',
    covers: 'The opaque contract cuts both ways: nothing is read out of a malformed one.',
    request: { query: { text: 'the' }, cursor: 'not-json' },
    seq: 7,
  },
  {
    name: 'a-cursor-of-another-version-is-refused',
    covers: 'A future format is refused rather than reinterpreted through this one.',
    request: {
      query: { text: 'the' },
      cursor: '{"v":2,"seq":7,"key":"[\\"l\\",\\"the\\",0,0,0,0,\\"full\\"]","start":0,"scanned":5}',
    },
    seq: 7,
  },
];
