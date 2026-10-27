// Generates src/script_data.rs: `new Intl.Locale(tag).maximize().script` for
// every two- and three-letter language subtag, alone and with every
// two-letter and three-digit region subtag, each with and without the script
// `Zzzz`, as tables `src/script.rs` reads.
//
//   node crates/plugins/i18n/scripts/script-data.mjs
//   cargo fmt -p epdf-plugin-i18n
//
// It needs the ICU the vectors were recorded under
// (tooling/plugin-conformance/vectors/environment.json); it refuses any other.
// About 61 million constructor calls: roughly 12 minutes on an M-series Mac.
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const crate = join(dirname(fileURLToPath(import.meta.url)), '..');
const environment = JSON.parse(
  readFileSync(join(crate, '../../../tooling/plugin-conformance/vectors/environment.json'), 'utf8'),
);
if (process.versions.icu !== environment.icu) {
  console.error(`ICU ${process.versions.icu}; the vectors were recorded under ICU ${environment.icu}`);
  process.exit(1);
}
const scriptOf = (c) => { try { return new Intl.Locale(c).maximize().script ?? null; } catch { return undefined; } };
const L = 'abcdefghijklmnopqrstuvwxyz';
const langs = [];
for (const a of L) for (const b of L) langs.push(a + b);
for (const a of L) for (const b of L) for (const c of L) langs.push(a + b + c);
const regions = [];
for (const a of L) for (const b of L) regions.push(a + b);
for (let i = 0; i < 1000; i++) regions.push(String(i).padStart(3, '0'));
const byScript = new Map();
const pairs = [];
const zzzz = [];
for (const lang of langs) {
  const base = scriptOf(lang);
  if (base === undefined) throw new Error(`invalid ${lang}`);
  if (base !== null) { if (!byScript.has(base)) byScript.set(base, []); byScript.get(base).push(lang); }
  const z = scriptOf(`${lang}-Zzzz`);
  if (z !== (base ?? 'Zzzz')) zzzz.push([lang, '', z]);
  for (const region of regions) {
    const s = scriptOf(`${lang}-${region}`);
    if (s === undefined) throw new Error(`invalid ${lang}-${region}`);
    if (s !== base) pairs.push([lang, region, s]);
    const sz = scriptOf(`${lang}-Zzzz-${region}`);
    if (sz !== 'Zzzz') zzzz.push([lang, region, sz]);
  }
}
const lit = (v) => (v === null ? '""' : JSON.stringify(v));
const wrap = (words) => {
  const lines = []; let line = '';
  for (const w of words) { if (line.length + w.length + 1 > 84) { lines.push(line); line = ''; } line += (line ? ' ' : '') + w; }
  if (line) lines.push(line);
  return lines.map((l, i) => `        "${l}${i < lines.length - 1 ? ' ' : ''}"`).join(',\n');
};
const scripts = [...byScript.keys()].sort();
const out = `//! Generated tables for [\`crate::script\`]. Do not edit by hand.
//!
//! From Node v${process.versions.node} (ICU ${process.versions.icu}, CLDR ${process.versions.cldr}), by asking
//! \`new Intl.Locale(tag).maximize().script\` of every two- and three-letter language subtag,
//! alone and with every two-letter and three-digit region subtag, each with and without the
//! script subtag \`Zzzz\`.

/// The languages whose tag alone maximizes to a script, by script: each entry's languages,
/// space-separated. Sorted by script.
pub const LANGUAGE_SCRIPTS: &[(&str, &[&str])] = &[
${scripts.map((s) => `    (\n        "${s}",\n        &[\n${wrap(byScript.get(s))},\n        ],\n    ),`).join('\n')}
];

/// \`language-region\` where the script differs from the language's own (an empty script is
/// none). Sorted by language, then region.
pub const REGION_SCRIPTS: &[(&str, &str, &str)] = &[
${pairs.map((x) => `    (${x.map(lit).join(', ')}),`).join('\n')}
];

/// \`language-Zzzz\` (empty region) and \`language-Zzzz-region\` where the script is not the rule's:
/// the language's own script (else \`Zzzz\`) without a region, \`Zzzz\` with one. An empty
/// script is none.
pub const ZZZZ_SCRIPTS: &[(&str, &str, &str)] = &[
${zzzz.map((x) => `    (${x.map(lit).join(', ')}),`).join('\n')}
];
`;
writeFileSync(join(crate, 'src/script_data.rs'), out);
console.error('scripts', scripts.length, 'pairs', pairs.length, 'zzzz', zzzz.length);
