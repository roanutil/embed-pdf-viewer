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
 * without changing the library: they pin the emsdk (`emsdk-version.sh`),
 * package an archive, and print the gn args. The delta's `emsdkPin` and
 * `argsMatch` compare the two that could matter against the archive.
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

/**
 * Build scripts that may change only as one known change each, a pair of
 * blobs: `from` at the archive's commit, `to` at the pin. `build-target.sh` is
 * the build driver, so a path alone can't vouch for it. The pair is its
 * change from `6dfc03697` to `8b1e69abd`, which checks the active emsdk
 * against `emsdk-version.sh` and records it, and compiles nothing.
 *
 * The cost of keeping it: every fork merge that touches one of these files
 * needs its new pair (`git rev-parse <commit>:<path>` at both ends) read and
 * recorded here, or the publish refuses.
 */
export const PINNED_SCRIPT_BLOBS = Object.freeze({
  'scripts/embedpdf-runtime/build-target.sh': Object.freeze({
    from: 'e5380cab4705750ba3a42773ebc135831db5bea2',
    to: '88272b4ee0b326199d4a5668fc16475072ca14db',
  }),
});

/**
 * Whether a path changed between two fork commits leaves the built library
 * alone. `blobs` is each pinned script's `{ from, to }` blobs, read at the
 * archive's commit and at the pin.
 */
const outsideTheBuild = (file, blobs = {}) =>
  file.startsWith('docs/') ||
  file.startsWith('scripts/embedpdf-runtime/tests/') ||
  BUILD_NEUTRAL_SCRIPTS.includes(file) ||
  (file in PINNED_SCRIPT_BLOBS &&
    blobs[file]?.from === PINNED_SCRIPT_BLOBS[file].from &&
    blobs[file]?.to === PINNED_SCRIPT_BLOBS[file].to);

/**
 * Refuses a fork stamp whose files don't match it. `facts` is what the
 * publish read from disk: `libSha256` of `staging/lib/libembedpdf.a`, and for
 * a stamp with `source`, `archiveSha256` of the archive at
 * `source.archivePath` (`null` when there is no such file) and `archiveSha`
 * and `archiveEmsdk`, the `sha` and `emsdkVersion` in the archive's own
 * `BUILD-METADATA.json` (`archiveEmsdk` is `null` for an archive that predates
 * the field, and the stamp's `source.emsdk` is then taken on trust), and
 * `archiveLibSha256` and `archiveArgs`, its `lib/libembedpdf.a` hash and
 * `args.gn`, which must be the staging ones (`libSha256`, `stagingArgs`), and
 * `archiveTarget`, the `target` its metadata names, which must be `target`.
 * `source.archivePath` must end in `source.archive`.
 */
export function assertStampMatches(fork, facts, target) {
  const problems = [];
  if (facts.libSha256 !== fork.libSha256) {
    problems.push(`libembedpdf.a hashes to ${facts.libSha256}, the stamp says ${fork.libSha256}`);
  }
  if (fork.source) {
    if (!fork.source.archivePath) problems.push('the stamp names no source.archivePath');
    else if (fork.source.archivePath.split('/').at(-1) !== fork.source.archive) {
      problems.push(`source.archivePath isn't the archive the stamp names, ${fork.source.archive}`);
    } else if (facts.archiveSha256 === null)
      problems.push(`no archive at ${fork.source.archivePath}`);
    else {
      if (facts.archiveSha256 !== fork.source.archiveSha256) {
        problems.push(
          `the archive hashes to ${facts.archiveSha256}, the stamp says ${fork.source.archiveSha256}`,
        );
      }
      if (facts.archiveEmsdk != null && facts.archiveEmsdk !== fork.source.emsdk) {
        problems.push(
          `the archive was built with emsdk ${facts.archiveEmsdk}, the stamp says ${fork.source.emsdk}`,
        );
      }
      if (facts.archiveLibSha256 !== facts.libSha256) {
        problems.push(
          `the archive's libembedpdf.a hashes to ${facts.archiveLibSha256}, the staging one to ${facts.libSha256}`,
        );
      }
      if (facts.archiveArgs !== facts.stagingArgs) {
        problems.push("the staging args.gn isn't the archive's");
      }
      // The plain wasm32 archive has the same gn args, so only its metadata says
      // it isn't the target this payload links.
      if (facts.archiveTarget !== target) {
        problems.push(`the archive is for target ${facts.archiveTarget}, not ${target}`);
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
 * commits between them, the paths they change, each `PINNED_SCRIPT_BLOBS`
 * script's blobs at both ends (`blobs`), the emsdk the pin builds the target
 * with (`emsdkPin`, from `emsdk-version.sh` at the pin), and whether the
 * archive's `args.gn` is what `target-args.sh` prints at `runtimeSrc`. Only a
 * delta that changes nothing but docs, the fork's tests, the
 * `BUILD_NEUTRAL_SCRIPTS` and the `PINNED_SCRIPT_BLOBS` (each as its recorded
 * pair), with the pin's emsdk the archive's and the same gn args, is
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
  const inside = delta.paths.filter((file) => !outsideTheBuild(file, delta.blobs));
  if (inside.length > 0) {
    throw new Error(`${runtimeSrc} changes the build beyond ${fork.sha}: ${inside.join(', ')}`);
  }
  if (delta.emsdkPin !== fork.source.emsdk) {
    throw new Error(
      `${runtimeSrc} builds ${target} with emsdk ${delta.emsdkPin}, the archive was built with ${fork.source.emsdk}`,
    );
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

/**
 * What a release found on the fork says: `missing` (no release, `null`),
 * `published` (not a draft, with the asset and its `.sha256`), or `incomplete`
 * (a draft, or an asset missing), which a `gh release create` that failed part
 * way leaves. Only `published` is something a publish can skip.
 */
export function releaseVerdict(release, asset) {
  if (!release) return 'missing';
  const names = new Set((release.assets ?? []).map((a) => a.name));
  if (release.isDraft || !names.has(asset) || !names.has(`${asset}.sha256`)) return 'incomplete';
  return 'published';
}
