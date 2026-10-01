#!/usr/bin/env node
// Check the linked engine against the bindings generated for it: every
// function in build/generated/functions.json must be exported, with the
// parameter count src/wasm/wasm-runtime.ts will call it with.
//
//   node build/verify-wasm-abi.mjs <embedpdf.wasm> [functions.json]
//
// The runtime splits each i64 argument into two i32 halves (expandI64Args),
// which is what a build without WASM_BIGINT exports, so an i64 parameter
// counts twice and no export may take or return an i64. Emscripten's own
// check for this is an ASSERTIONS-only abort at call time; this one fails the
// build instead.
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const [wasmPath, functionsPath = resolve(root, 'build/generated/functions.json')] = process.argv.slice(2);
if (!wasmPath) {
  console.error('usage: node build/verify-wasm-abi.mjs <embedpdf.wasm> [functions.json]');
  process.exit(2);
}

const I64 = 0x7e;

/** Each exported function's parameter and result value types, by export name. */
function exportedSignatures(bytes) {
  let at = 8; // magic and version
  const u32 = () => {
    let result = 0;
    let shift = 0;
    for (;;) {
      const byte = bytes[at++];
      result |= (byte & 0x7f) << shift;
      if (!(byte & 0x80)) return result >>> 0;
      shift += 7;
    }
  };
  const name = () => {
    const length = u32();
    const text = new TextDecoder().decode(bytes.subarray(at, at + length));
    at += length;
    return text;
  };
  const limits = () => {
    const flags = u32();
    u32();
    if (flags & 1) u32();
  };
  const types = [];
  const functionTypes = []; // imported functions first, then defined ones
  const exports = new Map();
  while (at < bytes.length) {
    const id = bytes[at++];
    const size = u32();
    const end = at + size;
    if (id === 1) {
      for (let n = u32(); n > 0; n--) {
        if (bytes[at++] !== 0x60) throw new Error('unsupported type form');
        const params = Array.from({ length: u32() }, () => bytes[at++]);
        const results = Array.from({ length: u32() }, () => bytes[at++]);
        types.push({ params, results });
      }
    } else if (id === 2) {
      for (let n = u32(); n > 0; n--) {
        name();
        name();
        const kind = bytes[at++];
        if (kind === 0) functionTypes.push(u32());
        else if (kind === 1) {
          at++; // reftype
          limits();
        } else if (kind === 2) limits();
        else if (kind === 3) at += 2; // valtype, mutability
        else if (kind === 4) {
          at++; // tag attribute
          u32();
        } else throw new Error(`unsupported import kind ${kind}`);
      }
    } else if (id === 3) {
      for (let n = u32(); n > 0; n--) functionTypes.push(u32());
    } else if (id === 7) {
      for (let n = u32(); n > 0; n--) {
        const exportName = name();
        const kind = bytes[at++];
        const index = u32();
        if (kind === 0) exports.set(exportName, types[functionTypes[index]]);
      }
    }
    at = end;
  }
  return exports;
}

const exports = exportedSignatures(readFileSync(wasmPath));
const { functions } = JSON.parse(readFileSync(functionsPath, 'utf8'));
const problems = [];
for (const fn of functions) {
  const type = exports.get(fn.name) ?? exports.get(`_${fn.name}`);
  if (!type) {
    problems.push(`${fn.name}: not exported`);
    continue;
  }
  const expected = fn.params.reduce((n, p) => n + (p.wasm.kind === 'i64' ? 2 : 1), 0);
  if (type.params.length !== expected) {
    problems.push(`${fn.name}: the runtime passes ${expected} arguments, the export takes ${type.params.length}`);
  }
  if (type.params.includes(I64) || type.results.includes(I64)) {
    problems.push(`${fn.name}: takes or returns an i64, so the build has WASM_BIGINT on`);
  }
}
if (problems.length) {
  console.error(`${wasmPath}: ${problems.length} of ${functions.length} bound functions don't match their bindings:`);
  for (const problem of problems.slice(0, 40)) console.error(`  ${problem}`);
  if (problems.length > 40) console.error(`  ... and ${problems.length - 40} more`);
  process.exit(1);
}
console.log(`${wasmPath}: all ${functions.length} bound functions match their bindings`);
