/**
 * Emits `vectors/actions/submit-flags.json`: what `decodeSubmitFormFlags`
 * produces for every interesting `/Flags` word.
 *
 *   pnpm --filter @embedpdf/tooling-engine-conformance run actions
 *   pnpm --filter @embedpdf/tooling-engine-conformance run actions:check
 *
 * Part of G6. The decode is ISO 32000-2:2020 Table 240, and it is the piece of
 * the actions read where a mistake is silent: a bit at the wrong position
 * sends a form submission to the wrong place in the wrong format, and nothing
 * downstream can tell. Two positions are the usual casualties, bit 12 being
 * ExclFKey rather than "ExclFDFTemplate" and bit 13 being reserved so
 * EmbedForm sits at 14.
 *
 * The corpus is exhaustive over single bits, then adds the combinations where
 * the rules interact.
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { decodeSubmitFormFlags } from '../../../packages/engine/core/src/dto/PdfAction';
import { actionTypeFromCode } from '../../../packages/engine/services/src/features/actions/ActionModelReader';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const vectorsRoot = join(packageRoot, 'vectors', 'actions');

/** Bump when the envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 1;

const bit = (n: number) => 1 << (n - 1);

interface FlagCase {
  readonly name: string;
  readonly covers: string;
  readonly raw: number;
}

const CASES: readonly FlagCase[] = [
  { name: 'none', covers: 'No flags: the Table 240 defaults, FDF and POST.', raw: 0 },

  // Every single bit on its own, including the reserved one and two past the
  // end, so a decoder that shifted its table by one is caught wherever it did.
  ...Array.from({ length: 16 }, (_, i) => ({
    name: `bit-${i + 1}-alone`,
    covers: `Bit ${i + 1} with nothing else set.`,
    raw: bit(i + 1),
  })),

  // Format precedence. Bit 9 dominates, then 6, then 3.
  {
    name: 'pdf-beats-xfdf',
    covers: 'SubmitPDF dominates: the spec says all other flags are ignored except GetMethod.',
    raw: bit(9) | bit(6),
  },
  {
    name: 'pdf-beats-html',
    covers: 'The same, against the HTML bit.',
    raw: bit(9) | bit(3),
  },
  {
    name: 'xfdf-beats-html',
    covers: 'XFDF takes precedence over HTML when both are set.',
    raw: bit(6) | bit(3),
  },
  {
    name: 'all-three-format-bits',
    covers: 'Everything set at once, where the order of the checks is the whole answer.',
    raw: bit(9) | bit(6) | bit(3),
  },

  // The method rule, which is where the interaction lives.
  {
    name: 'get-with-html',
    covers: 'GetMethod is meaningful with an export format.',
    raw: bit(4) | bit(3),
  },
  {
    name: 'get-with-pdf',
    covers: "Bit 9's exception keeps GetMethod alive for SubmitPDF.",
    raw: bit(4) | bit(9),
  },
  {
    name: 'get-with-xfdf',
    covers: 'XFDF is not one of the two, so this posts despite the bit.',
    raw: bit(4) | bit(6),
  },
  {
    name: 'get-with-fdf',
    covers: 'The default format ignores GetMethod entirely.',
    raw: bit(4),
  },

  // The two easily-confused positions, set together and apart.
  {
    name: 'excl-f-key-and-embed-form',
    covers: 'Bits 12 and 14, with the reserved 13 between them.',
    raw: bit(12) | bit(14),
  },
  {
    name: 'reserved-bit-13-only',
    covers: 'Bit 13 is reserved, so setting it changes nothing a consumer sees.',
    raw: bit(13),
  },

  // Words a real producer might write.
  {
    name: 'exclude-with-fields',
    covers: 'Exclude mode, the common non-default.',
    raw: bit(1),
  },
  {
    name: 'html-get-with-coordinates',
    covers: 'An HTML GET submission that also asks for click coordinates.',
    raw: bit(3) | bit(4) | bit(5),
  },
  {
    name: 'fdf-with-everything-fdf-only',
    covers: 'The FDF-only bits together: append saves, annotations, current user only.',
    raw: bit(7) | bit(8) | bit(11),
  },
  { name: 'all-bits', covers: 'Every bit set.', raw: 0xffff },
  {
    name: 'high-bits-outside-the-table',
    covers: 'Bits past 16, which the decode must ignore rather than misread.',
    raw: 0xffff0000 | bit(3),
  },
] as const;

async function emit(relative: string, text: string, check: boolean): Promise<boolean> {
  const absolute = join(vectorsRoot, relative);
  let current: string | undefined;
  try {
    current = await readFile(absolute, 'utf8');
  } catch {
    current = undefined;
  }
  if (current === text) {
    process.stdout.write(`  ok      actions/${relative}\n`);
    return false;
  }
  if (check) {
    process.stdout.write(
      `  DIFFERS actions/${relative}${current === undefined ? ' (missing)' : ''}\n`,
    );
    return true;
  }
  await mkdir(dirname(absolute), { recursive: true });
  await writeFile(absolute, text);
  process.stdout.write(`  written actions/${relative}\n`);
  return true;
}

async function main(): Promise<void> {
  const check = process.argv.includes('--check');

  const names = new Set<string>();
  for (const entry of CASES) {
    if (names.has(entry.name)) throw new Error(`duplicate case name ${entry.name}`);
    names.add(entry.name);
  }

  const types = {
    schemaVersion: SCHEMA_VERSION,
    kind: 'actions.types',
    // Every defined code, plus the ones around the edges. The shared
    // vocabulary reaches page, annotation, field and catalog actions, so a
    // code mapped differently is a divergence in four places at once.
    cases: [-2, -1, ...Array.from({ length: 24 }, (_, i) => i)].map((code) => ({
      code,
      type: actionTypeFromCode(code),
    })),
  };

  const payload = {
    schemaVersion: SCHEMA_VERSION,
    kind: 'actions.submitFlags',
    cases: CASES.map((entry) => {
      // `>>> 0` because the raw values above are written with JavaScript
      // bitwise operators, which return a signed int: bit(16) alone is
      // negative, and so is anything with the high bits set. The word a real
      // caller passes came from PDFium and is unsigned, so the decode is given
      // the unsigned form and the retained `raw` is unsigned too.
      const raw = entry.raw >>> 0;
      return { ...entry, raw, flags: decodeSubmitFormFlags(raw) };
    }),
  };

  let differed = await emit('submit-flags.json', `${JSON.stringify(payload, null, 2)}\n`, check);
  differed =
    (await emit('types.json', `${JSON.stringify(types, null, 2)}\n`, check)) || differed;
  if (check && differed) {
    process.stderr.write(
      '\nAction vectors differ. Regenerate with:\n' +
        '  pnpm --filter @embedpdf/tooling-engine-conformance run actions\n' +
        'and commit the diff.\n',
    );
    process.exitCode = 1;
  }
}

main().catch((error: unknown) => {
  process.stderr.write(
    `generate-action-vectors: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
