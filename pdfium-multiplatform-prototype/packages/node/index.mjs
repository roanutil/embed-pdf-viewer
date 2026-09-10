/**
 * Loads the napi addon for this host, named the way packages/engine/runtime/npm
 * names its targets (Node's spelling: darwin-arm64, linux-x64, win32-x64).
 */
import { createRequire } from 'node:module';
import { existsSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { resolveTarget } from './platform.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);

const target = resolveTarget();

if (!target) {
  throw new Error(`unsupported host: ${process.platform}-${process.arch}`);
}

const addonPath = resolve(here, `epdf-node.${target}.node`);

if (!existsSync(addonPath)) {
  throw new Error(
    `no addon at ${addonPath}\nFix with: bash build/build-node.sh ${target}`,
  );
}

const addon = require(addonPath);

export const openDocument = addon.openDocument;
export const Document = addon.Document;
