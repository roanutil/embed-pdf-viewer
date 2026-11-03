import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { pathToFileURL } from 'node:url';

const source = 'packages/engine/runtime/src/core/pdf-functions.generated.ts';
const output = path.resolve(
  process.argv[2] ?? 'tooling/dotnet-parity/fixtures/.local/functions.json',
);
const bytes = fs.readFileSync(source);
const { pdfFunctionSignatures } = await import(pathToFileURL(path.resolve(source)).href);
const data = {
  source: { path: source, sha256: crypto.createHash('sha256').update(bytes).digest('hex') },
  target: 'wasm32',
  functions: Object.entries(pdfFunctionSignatures).map(([name, signature]) => ({
    name,
    ...signature,
  })),
};
fs.mkdirSync(path.dirname(output), { recursive: true });
fs.writeFileSync(output, JSON.stringify(data, null, 2) + '\n');
console.log(JSON.stringify({ source: data.source, functions: data.functions.length, output }));
