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

/** Whether a path changed between two fork commits leaves the built library alone. */
const outsideTheBuild = (file) => file.startsWith('scripts/') || file.startsWith('docs/');

/**
 * The notes' lines on where the payload's PDFium came from.
 *
 * `fork` is the build-arm.mjs stamp: `sha` the fork commit the library was
 * built from, `libSha256`, and, for a library taken from a fork release
 * archive rather than built here, `source` ({ archive, archiveSha256, emsdk }).
 * `runtimeSrc` is the commit the viewer pins. When the two differ, `delta`
 * says how: the commits between them, the paths they change, and whether the
 * archive's `args.gn` is what `target-args.sh` prints at `runtimeSrc`. Only a
 * delta that changes nothing but scripts and docs, with the same gn args, is
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
  if (!delta || delta.commits < 1) throw new Error(`no commits from ${fork.sha} to ${runtimeSrc}`);
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
