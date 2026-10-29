/**
 * Packs dist/ and types/ into a tarball another app installs with no registry.
 *
 * Not `pnpm pack`: that would keep this package's workspace dependencies,
 * rewritten to versions no registry has. The bundle is self-contained, so the
 * packed manifest has none. The tarball lands in packages/rust-viewer-bundle/.
 */
import { execFileSync } from 'node:child_process';
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const source = JSON.parse(readFileSync(path.join(here, 'package.json'), 'utf8'));

const stage = mkdtempSync(path.join(tmpdir(), 'rust-viewer-pack-'));
try {
  for (const entry of source.files) cpSync(path.join(here, entry), path.join(stage, entry), { recursive: true });
  const manifest = {
    name: source.name,
    version: source.version,
    description: source.description,
    type: source.type,
    main: source.main,
    module: source.module,
    types: source.types,
    exports: source.exports,
    files: source.files,
    sideEffects: source.sideEffects,
  };
  writeFileSync(path.join(stage, 'package.json'), `${JSON.stringify(manifest, null, 2)}\n`);
  const out = execFileSync('npm', ['pack', '--pack-destination', here], { cwd: stage, encoding: 'utf8' });
  console.log(path.join(here, out.trim().split('\n').at(-1)));
} finally {
  rmSync(stage, { recursive: true, force: true });
}
