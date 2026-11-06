/** Annotation public LF and PDF/native CR boundaries, from the shipping TS oracle. */
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { dirname, resolve, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createLocalEngine } from '../../../packages/engine/main/src/index';
import {
  DEFAULT_RICH_TEXT_BODY,
  richTextPlainText,
} from '../../../packages/engine/core/src/dto/RichText';
import { toLineBreaks } from '../../../packages/engine/core/src/text/lineBreaks';
import {
  engineRichTextJson,
  parseEngineRichText,
} from '../../../packages/engine/services/src/features/annotations/internal/richTextWire';
import type { RichTextDocumentInput } from '../../../packages/engine/core/src/dto/RichText';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const fixture = 'tooling/engine-conformance/fixtures/rich_metadata.pdf';
const output = join(root, 'tooling/engine-conformance/vectors/annotation/line-breaks.json');
const texts = [
  { name: 'lf', text: 'alpha\nbeta' },
  { name: 'crlf', text: 'alpha\r\nbeta' },
  { name: 'cr', text: 'alpha\rbeta' },
  { name: 'mixed', text: 'a\r\nb\rc\nd' },
  { name: 'unicode', text: 'é\r\n😀\n中' },
  { name: 'empty', text: '' },
];

async function main() {
  const engine = createLocalEngine({ runtime: { prefer: 'wasm' } });
  let contentsCases;
  try {
    const bytes = new Uint8Array(await readFile(join(root, fixture)));
    const doc = await engine.open(
      { kind: 'bytes', id: 'annotation-lines', bytes },
      { scope: ['*'] },
    );
    try {
      const first = (await doc.pages.list()).pages[0]!;
      const page = doc.page(first.ref);
      contentsCases = [];
      const inputs: Array<{ name: string; input: { contents?: string | null } }> = [
        ...texts.map(({ name, text }) => ({ name, input: { contents: text } })),
        { name: 'null', input: { contents: null } },
        { name: 'omitted', input: {} },
      ];
      for (const entry of inputs) {
        const result = await page.annotations.create({
          subtype: 'highlight',
          quadPoints: [
            {
              upperLeft: { x: 72, y: 72 },
              upperRight: { x: 200, y: 72 },
              lowerLeft: { x: 72, y: 92 },
              lowerRight: { x: 200, y: 92 },
            },
          ],
          ...entry.input,
        });
        contentsCases.push({
          ...entry,
          contents: result.annotation.contents,
          pdfContents:
            entry.input.contents == null ? null : toLineBreaks(entry.input.contents, '\r'),
        });
      }
    } finally {
      await doc.close();
    }
  } finally {
    await engine.destroy();
  }
  const richCases = texts.map(({ name, text }) => {
    const input: RichTextDocumentInput = {
      body: { ...DEFAULT_RICH_TEXT_BODY, family: 'helvetica' },
      paragraphs: [{ runs: [{ text }] }, { runs: [{ text: 'tail\nline' }] }],
    };
    const rawEngine = {
      body: DEFAULT_RICH_TEXT_BODY,
      paragraphs: input.paragraphs,
      source: 'richtext',
      diagnostics: [],
    };
    return {
      name,
      input,
      plainText: richTextPlainText(input),
      engine: JSON.parse(engineRichTextJson(input)),
      rawEngine,
      parsed: parseEngineRichText(JSON.stringify(rawEngine)),
    };
  });
  const next =
    JSON.stringify(
      { schemaVersion: 1, kind: 'annotation.lineBreaks', fixture, contentsCases, richCases },
      null,
      2,
    ) + '\n';
  let current;
  try {
    current = await readFile(output, 'utf8');
  } catch {
    current = undefined;
  }
  if (current === next) {
    console.log('  ok      annotation/line-breaks.json');
    return;
  }
  if (process.argv.includes('--check')) {
    console.error('  DIFFERS annotation/line-breaks.json');
    process.exitCode = 1;
    return;
  }
  await mkdir(dirname(output), { recursive: true });
  await writeFile(output, next);
  console.log('  written annotation/line-breaks.json');
}
main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
