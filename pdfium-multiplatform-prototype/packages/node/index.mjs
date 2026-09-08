/**
 * Loads the napi addon for this host, named the way packages/engine/runtime/npm
 * names its targets (Node's spelling: darwin-arm64, linux-x64, win32-x64).
 */
import { createRequire } from 'node:module';
import { existsSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);

/**
 * `process.platform` reports `linux` on both glibc and musl hosts, so it
 * cannot by itself pick between the four Linux targets build/runtime-build.json
 * pins: linux-arm64, linux-x64, linuxmusl-arm64, linuxmusl-x64. The glibc
 * dynamic loader populates `process.report`'s `glibcVersionRuntime`; musl
 * hosts (e.g. Alpine) have no such loader, so the field is absent there. That
 * absence is the probe, but only once the report itself is confirmed to
 * exist: if `process.report` (or `getReport()`, or its `header`) is missing
 * (an older Node, a build without report support, or the property
 * monkey-patched away), there is no way to tell glibc from musl, and this
 * defaults to glibc, matching how packages/engine/runtime/src/core/platform.node.ts
 * treats `detect-libc`'s `familySync()` returning `null`. This file checks
 * the built-in report instead of taking a `detect-libc` dependency, so the
 * scaffold's node package still pulls in nothing extra for this probe.
 */
function isMusl() {
  const header = process.report?.getReport()?.header;
  if (header == null) {
    // The probe is unavailable, not merely "no glibc field": treat this as
    // indeterminate and default to glibc rather than guessing musl.
    return false;
  }
  return header.glibcVersionRuntime == null;
}

function resolveTarget() {
  const { platform, arch } = process;
  if (platform === 'darwin' && arch === 'arm64') return 'darwin-arm64';
  if (platform === 'darwin' && arch === 'x64') return 'darwin-x64';
  if (platform === 'win32' && arch === 'arm64') return 'win32-arm64';
  if (platform === 'win32' && arch === 'x64') return 'win32-x64';
  if (platform === 'linux' && arch === 'arm64') return isMusl() ? 'linuxmusl-arm64' : 'linux-arm64';
  if (platform === 'linux' && arch === 'x64') return isMusl() ? 'linuxmusl-x64' : 'linux-x64';
  return null;
}

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
