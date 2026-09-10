#!/usr/bin/env node
// Print a separate pin for local development; never edit the committed release pin.
import { readFile, stat } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

try {
  const entries = process.argv.slice(2);
  if (!entries.length) throw new Error('usage: node build/local-pin.mjs <target>=<archive.tar.gz> [...]');
  const pin = JSON.parse(await readFile(new URL('./runtime-build.json', import.meta.url), 'utf8'));
  for (const entry of entries) {
    const separator = entry.indexOf('=');
    const target = entry.slice(0, separator);
    if (separator < 1 || !Object.hasOwn(pin.artifacts, target)) {
      throw new Error(`unknown target or invalid entry: ${entry}`);
    }
    const path = resolve(entry.slice(separator + 1));
    if (!(await stat(path)).isFile()) throw new Error(`not an archive file: ${path}`);
    const sha256 = createHash('sha256').update(await readFile(path)).digest('hex');
    pin.artifacts[target] = { url: pathToFileURL(path).href, sha256 };
  }
  console.log(JSON.stringify(pin, null, 2));
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
