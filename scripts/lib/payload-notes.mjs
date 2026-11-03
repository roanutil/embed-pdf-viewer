// The pure parts of publishing the wasm32 payload (publish-engine-payload.mjs):
// the bindings its postlink check reads, and the release notes.

/**
 * The bound functions in the committed `pdf-functions.generated.ts`, in the
 * shape `build/verify-wasm-abi.mjs` reads (`functions.json`): each entry of
 * `pdfFunctionSignatures`, one per line, `  Name: { params: [...], result: ... },`.
 */
export function committedSignatures(text) {
  const start = text.indexOf('export const pdfFunctionSignatures = {');
  if (start === -1) throw new Error('no pdfFunctionSignatures in the committed bindings');
  const functions = [];
  for (const line of text.slice(start).split('\n')) {
    const match = line.match(/^ {2}(\w+): \{ params: (.*), result: (.*) \},$/);
    if (match) {
      functions.push({
        name: match[1],
        params: JSON.parse(match[2]),
        result: JSON.parse(match[3]),
      });
    }
  }
  if (functions.length === 0) throw new Error('pdfFunctionSignatures has no entries');
  return functions;
}

/**
 * Committed bindings the fork runtime is known not to export yet, each one
 * accepted by name. `EPDFAnnot_GetStructurePosition` waits on the fork merging
 * upstream runtime `28b8b808f`; empty this list when that lands, and the
 * publish refuses any binding the runtime then lacks.
 *
 * The cost of keeping it: this list and the committed bindings move apart on
 * every runtime merge, and a new gap has to be added here by hand before a
 * payload can publish, which is the point, but it is one more file to touch.
 */
export const KNOWN_UNBOUND = Object.freeze(['EPDFAnnot_GetStructurePosition']);

/** Refuses committed bindings the runtime doesn't declare, beyond `known`. */
export function assertKnownUnbound(unbound, known = KNOWN_UNBOUND) {
  const unexpected = unbound.filter((name) => !known.includes(name));
  if (unexpected.length > 0) {
    throw new Error(
      `committed bindings this runtime doesn't declare, and that aren't known gaps: ${unexpected.join(', ')}`,
    );
  }
}

/**
 * What the postlink check holds the linked engine to: every committed binding
 * of a function the runtime's headers declare (`declared`, the names the
 * header generator found, which is what the link exports). A committed binding
 * the runtime doesn't declare can't be exported, and is listed in `unbound`
 * instead of failing the check, so the notes can name it.
 */
export function abiCheckFunctions(committed, declared) {
  const have = new Set(declared);
  return {
    functions: committed.filter((fn) => have.has(fn.name)),
    unbound: committed.filter((fn) => !have.has(fn.name)).map((fn) => fn.name),
  };
}

/**
 * The fork's scripts that may change between the archive's commit and the pin
 * without changing the library: they pin the emsdk, package an archive, and
 * print the gn args, which the delta's `argsMatch` compares separately.
 *
 * The cost of keeping it: a fork commit that changes any other script, however
 * harmless, refuses the publish until it is named here, and a named script
 * that later starts to shape the build would pass unnoticed, so this list
 * needs reading on every runtime merge, beside `KNOWN_UNBOUND`.
 */
export const BUILD_NEUTRAL_SCRIPTS = Object.freeze([
  'scripts/embedpdf-runtime/emsdk-version.sh',
  'scripts/embedpdf-runtime/package-target.sh',
  'scripts/embedpdf-runtime/target-args.sh',
]);

/** Whether a path changed between two fork commits leaves the built library alone. */
const outsideTheBuild = (file) =>
  file.startsWith('docs/') ||
  file.startsWith('scripts/embedpdf-runtime/tests/') ||
  BUILD_NEUTRAL_SCRIPTS.includes(file);

/**
 * Refuses a fork stamp whose files don't match it. `facts` is what the
 * publish read from disk: `libSha256` of `staging/lib/libembedpdf.a`, and for
 * a stamp with `source`, `archiveSha256` of the archive at
 * `source.archivePath` (`null` when there is no such file) and `archiveSha`,
 * the `sha` in the archive's own `BUILD-METADATA.json`.
 */
export function assertStampMatches(fork, facts) {
  const problems = [];
  if (facts.libSha256 !== fork.libSha256) {
    problems.push(`libembedpdf.a hashes to ${facts.libSha256}, the stamp says ${fork.libSha256}`);
  }
  if (fork.source) {
    if (!fork.source.archivePath) problems.push('the stamp names no source.archivePath');
    else if (facts.archiveSha256 === null)
      problems.push(`no archive at ${fork.source.archivePath}`);
    else {
      if (facts.archiveSha256 !== fork.source.archiveSha256) {
        problems.push(
          `the archive hashes to ${facts.archiveSha256}, the stamp says ${fork.source.archiveSha256}`,
        );
      }
      if (facts.archiveSha !== fork.sha) {
        problems.push(
          `the archive's BUILD-METADATA.json names ${facts.archiveSha}, the stamp says ${fork.sha}`,
        );
      }
    }
  }
  if (problems.length > 0)
    throw new Error(`the fork stamp doesn't match its files: ${problems.join('; ')}`);
}

/**
 * The notes' lines on where the payload's PDFium came from.
 *
 * `fork` is the build-arm.mjs stamp: `sha` the fork commit the library was
 * built from, `libSha256`, and, for a library taken from a fork release
 * archive rather than built here, `source` ({ archive, archiveSha256, emsdk }).
 * `runtimeSrc` is the commit the viewer pins. When the two differ, `delta`
 * says how: whether `fork.sha` is an ancestor of the pin (`ancestor`), the
 * commits between them, the paths they change, and whether the
 * archive's `args.gn` is what `target-args.sh` prints at `runtimeSrc`. Only a
 * delta that changes nothing but docs, the fork's tests and the
 * `BUILD_NEUTRAL_SCRIPTS`, with the same gn args, is
 * accepted; anything else throws, since the notes couldn't say the library
 * matches the pin.
 */
export function forkLines({ fork, runtimeSrc, target, delta }) {
  if (fork.sha === runtimeSrc) {
    if (fork.source) throw new Error('a release archive at the pinned commit needs no delta');
    return [
      `- Fork commit (runtime-src): \`${fork.sha}\`, built for ${target}`,
      `- libembedpdf.a sha256: \`${fork.libSha256}\``,
    ];
  }
  if (!fork.source) {
    throw new Error(`the library was built from ${fork.sha}, not the pinned ${runtimeSrc}`);
  }
  if (!delta || !delta.ancestor) {
    throw new Error(`${fork.sha} is not an ancestor of the pinned ${runtimeSrc}`);
  }
  if (delta.commits < 1) throw new Error(`no commits from ${fork.sha} to ${runtimeSrc}`);
  const inside = delta.paths.filter((file) => !outsideTheBuild(file));
  if (inside.length > 0) {
    throw new Error(`${runtimeSrc} changes the build beyond ${fork.sha}: ${inside.join(', ')}`);
  }
  if (!delta.argsMatch) {
    throw new Error(`the archive's args.gn isn't what target-args.sh prints at ${runtimeSrc}`);
  }
  const { archive, archiveSha256, emsdk } = fork.source;
  const commits = delta.commits === 1 ? '1 commit' : `${delta.commits} commits`;
  return [
    `- PDFium: built from fork \`${fork.sha}\`, the ${target} library of its release archive \`${archive}\` (sha256 \`${archiveSha256}\`), with emsdk ${emsdk}`,
    `- runtime-src: \`${runtimeSrc}\`, which adds ${commits} on top of \`${fork.sha}\` that change only scripts/ and docs/. The source and the gn args are identical.`,
    `- libembedpdf.a sha256: \`${fork.libSha256}\``,
  ];
}

/** The release notes of a `runtime-payloads-<hash>` release. */
export function payloadNotes({
  hash,
  head,
  fork,
  runtimeSrc,
  target,
  delta,
  rustc,
  asset,
  digest,
  unbound,
}) {
  return [
    `wasm32 engine payload with the Rust core linked in, for fetch-payload.sh.`,
    '',
    `- Payload hash: \`${hash}\``,
    `- Viewer commit: \`${head}\``,
    ...forkLines({ fork, runtimeSrc, target, delta }),
    `- ${rustc}`,
    `- ${asset} sha256: \`${digest}\``,
    ...(unbound.length > 0
      ? [
          `- Bound in the TypeScript but not exported by this runtime: ${unbound.map((name) => `\`${name}\``).join(', ')}`,
        ]
      : []),
  ].join('\n');
}
