#!/usr/bin/env node
import { execFileSync } from 'node:child_process';
import { statSync } from 'node:fs';

const [rustBinary, swiftBinary, fixture, iterations = '20'] = process.argv.slice(2);
if (!rustBinary || !swiftBinary || !fixture) {
  console.error('usage: compare-swift-rust.mjs <rust-bin> <swift-bin> <pdf> [iterations]');
  process.exit(2);
}
const run = (binary) => JSON.parse(execFileSync(binary, [fixture, iterations], { encoding: 'utf8' }));
const rust = run(rustBinary);
const swift = run(swiftBinary);
const operations = ['pageCount', 'pageSize', 'pageText', 'search', 'render'];
for (const operation of operations) {
  if (rust[operation].checksum !== swift[operation].checksum) {
    throw new Error(`${operation}: checksum mismatch (Rust ${rust[operation].checksum}, Swift ${swift[operation].checksum})`);
  }
}
console.log('| operation | Rust | Swift | Swift/Rust |');
console.log('|---|---:|---:|---:|');
for (const operation of operations) {
  const r = rust[operation].nsPerIteration;
  const s = swift[operation].nsPerIteration;
  console.log(`| ${operation} | ${(r / 1e6).toFixed(3)} ms | ${(s / 1e6).toFixed(3)} ms | ${(s / r).toFixed(2)}x |`);
}
console.log('\n| release executable | bytes |');
console.log('|---|---:|');
console.log(`| Rust | ${statSync(rustBinary).size.toLocaleString()} |`);
console.log(`| Swift | ${statSync(swiftBinary).size.toLocaleString()} |`);
console.log('\nBoth executables dynamically share PDFium. Swift also uses the macOS Swift runtime; the table is executable bytes, not total deployment bytes.');
