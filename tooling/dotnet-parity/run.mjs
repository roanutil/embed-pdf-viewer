#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const argv = process.argv.slice(2);
const option = (name, fallback) => {
  const index = argv.indexOf(name);
  return index < 0 ? fallback : argv[index + 1];
};
const manifestPath = path.resolve(
  root,
  option('--manifest', 'tooling/dotnet-parity/manifest.json'),
);
const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
const sha = (bytes) => crypto.createHash('sha256').update(bytes).digest('hex');
const digest = (filename) => {
  const bytes = fs.readFileSync(filename);
  return {
    path:
      path.isAbsolute(filename) && !filename.startsWith(root + '/')
        ? filename
        : path.relative(root, filename),
    sha256: sha(bytes),
    bytes: bytes.length,
  };
};
const absolute = (p) => path.resolve(root, p);
const quote = (value) =>
  /^[a-zA-Z0-9_./:=@,-]+$/.test(value) ? value : "'" + value.replaceAll("'", "'\\''") + "'";
const dotnet = process.env.DOTNET_HOST_PATH ?? '/opt/dotnet/dotnet';
const project = 'platforms/dotnet/EmbedPDF.Parity/EmbedPDF.Parity.csproj';
const packedDirectory = option(
  '--pin-packed',
  process.env.EPDF_PARITY_PACKED_DIR ?? manifest.viewer.packedDirectory,
);

function filesBelow(directory) {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((e) => {
    const filename = path.join(directory, e.name);
    if (e.isSymbolicLink())
      throw new Error('Packed artifacts must not link workspace files: ' + filename);
    return e.isDirectory() ? filesBelow(filename) : [filename];
  });
}
if (argv.includes('--pin-packed')) {
  if (!packedDirectory) throw new Error('Expected --pin-packed <installed vite-app directory>');
  const packed = path.resolve(packedDirectory);
  const lock = JSON.parse(fs.readFileSync(path.join(packed, 'package-lock.json')));
  const entry = lock.packages['node_modules/' + manifest.viewer.package];
  if (!entry?.integrity || !entry.resolved?.startsWith('file:') || !entry.resolved.endsWith('.tgz'))
    throw new Error('Viewer must be installed from a packed tarball with npm integrity');
  const unusedNativeRuntimePackages = [];
  const packageArtifacts = Object.entries(lock.packages)
    .filter(([name, entry]) => {
      if (
        !name.startsWith('node_modules/@embedpdf/') &&
        !name.startsWith('node_modules/@cloudpdf/')
      )
        return false;
      if (
        entry.optional &&
        name.startsWith('node_modules/@embedpdf/engine-runtime-') &&
        !name.endsWith('-wasm32')
      ) {
        unusedNativeRuntimePackages.push({
          package: name.slice('node_modules/'.length),
          integrity: entry.integrity,
          reason: 'native sidecar outside the browser artifact graph',
        });
        return false;
      }
      return true;
    })
    .map(([name, entry]) => {
      if (
        !entry.integrity ||
        !entry.resolved?.startsWith('file:') ||
        !entry.resolved.endsWith('.tgz')
      )
        throw new Error('Unpacked dependency ' + name);
      const filename = path.resolve(packed, entry.resolved.slice(5));
      return {
        package: name.slice('node_modules/'.length),
        integrity: entry.integrity,
        ...digest(filename),
      };
    });
  const bindingMap = path.join(
    packed,
    'node_modules/@embedpdf/engine-runtime/dist/index.browser.js.map',
  );
  const bindingSource = absolute('packages/engine/runtime/src/core/pdf-functions.generated.ts');
  const proof = spawnSync(
    process.execPath,
    ['tooling/dotnet-parity/fixtures/check-binding-source.mjs', bindingMap, bindingSource],
    { cwd: root, encoding: 'utf8' },
  );
  if (proof.status !== 0)
    throw new Error(proof.stderr || 'Cannot prove packed runtime binding source');
  manifest.runtime.bindingSource = digest(bindingSource);
  manifest.runtime.bindingMap = digest(bindingMap);
  manifest.viewer.bindingSourceProof = JSON.parse(proof.stdout);
  manifest.viewer.packedDirectory = packed;
  manifest.viewer.packageArtifacts = packageArtifacts;
  manifest.viewer.unusedNativeRuntimePackages = unusedNativeRuntimePackages;
  manifest.viewer.packedFiles = [
    path.join(packed, 'package-lock.json'),
    ...filesBelow(path.join(packed, 'dist')),
  ].map(digest);
  fs.writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + '\n');
  console.log('Pinned packed viewer integrity and production files; rerun the default gate.');
  process.exit(0);
}
const out = absolute(option('--out', 'tooling/dotnet-parity/fixtures/records/latest'));
fs.mkdirSync(out, { recursive: true });
const manifestSnapshot = path.join(out, 'manifest.input.json');
fs.copyFileSync(manifestPath, manifestSnapshot);
const simple = (command, args) => {
  const result = spawnSync(command, args, { cwd: root, encoding: 'utf8', timeout: 120000 });
  if (command === 'git' && result.status !== 0)
    throw new Error(
      'Cannot record source identity: ' + (result.stderr ?? result.error ?? 'git failure'),
    );
  return result;
};
const scope = [
  'tooling/dotnet-parity',
  'platforms/dotnet/EmbedPDF.Parity',
  'examples/viewer-react/src/e2e.tsx',
  'tooling/viewer-e2e/tests/dotnet-parity.spec.ts',
  'examples/viewer-react/package.json',
  'pnpm-lock.yaml',
];
const head = simple('git', ['rev-parse', 'HEAD']).stdout?.trim();
const status = simple('git', ['status', '--porcelain', '--', ...scope]).stdout ?? '';
const diff = simple('git', ['diff', '--binary', 'HEAD', '--', ...scope]).stdout ?? '';
const untracked =
  simple('git', ['ls-files', '--others', '--exclude-standard', '--', ...scope])
    .stdout?.trim()
    .split('\n')
    .filter(Boolean) ?? [];
const source = {
  commit: head,
  dirty: Boolean(status),
  ...(status
    ? {
        diffSha256: sha(
          diff + untracked.map((p) => p + ':' + digest(absolute(p)).sha256).join('\n'),
        ),
      }
    : {}),
  runtimeFork: manifest.runtimeFork,
};
const environment = {
  kind: 'container',
  os: os.platform() + ' ' + os.release(),
  arch: os.arch() === 'arm64' ? 'arm64' : 'x64',
  emulated: process.env.EPDF_PARITY_EMULATED === '1',
  imageId: manifest.imageId,
  tools: {
    node: process.version,
    dotnet: simple(dotnet, ['--version']).stdout?.trim() ?? 'unavailable',
    libc: simple('getconf', ['GNU_LIBC_VERSION']).stdout?.trim() ?? 'unavailable',
  },
};
environment.tools.playwright = JSON.parse(
  fs.readFileSync(
    absolute('tooling/viewer-e2e/node_modules/@playwright/test/package.json'),
    'utf8',
  ),
).version;
const deploymentFontLimitation =
  'consumer deployment font bundle: compatibility not yet verified; re-run the gate with the supplied bundle before relying on it';
const summary = [];
function execute(executable, args, stem, extraEnv = {}) {
  const command = [executable, ...args].map(quote).join(' ');
  const result = spawnSync(executable, args, {
    cwd: root,
    encoding: 'utf8',
    timeout: 300000,
    env: { ...process.env, ...extraEnv },
    maxBuffer: 32 * 1024 * 1024,
  });
  const logPath = path.join(out, stem + '.log');
  fs.writeFileSync(
    logPath,
    (result.stdout ?? '') +
      (result.stderr ?? '') +
      (result.error ? String(result.error) + '\n' : '') +
      (result.signal ? 'signal: ' + result.signal + '\n' : ''),
  );
  return {
    command,
    cwd: root,
    exitCode: result.status ?? 1,
    log: digest(logPath),
    results:
      result.status === 0
        ? 'passed'
        : 'failed; ' + (result.signal ?? result.error?.message ?? 'exit ' + result.status),
  };
}
function record(id, commands, inputs, artifacts, limitations) {
  const passed =
    commands.every((c) => c.exitCode === 0) &&
    limitations.every((limitation) => limitation === deploymentFontLimitation);
  const record = {
    schemaVersion: 1,
    slice: 5,
    rid: environment.arch === 'arm64' ? 'linux-arm64' : 'linux-x64',
    source,
    environment,
    inputs,
    commands,
    artifacts,
    limitations,
  };
  fs.writeFileSync(path.join(out, id + '.record.json'), JSON.stringify(record, null, 2) + '\n');
  summary.push({
    id,
    status: passed ? 'passed' : 'failed',
    record: path.relative(root, path.join(out, id + '.record.json')),
  });
}
const selftest = execute(
  dotnet,
  ['run', '--project', project, '--', 'self-test'],
  'decimal-self-test',
);
const managedArtifacts = [
  'EmbedPDF.Parity.dll',
  'EmbedPDF.Engine.dll',
  'EmbedPDF.Parity.runtimeconfig.json',
]
  .map((p) => path.join(root, 'platforms/dotnet/EmbedPDF.Parity/bin/Debug/net10.0', p))
  .filter((p) => fs.existsSync(p))
  .map(digest);
const probes = fs
  .readFileSync(absolute(selftest.log.path), 'utf8')
  .split('\n')
  .flatMap((line) => {
    try {
      const item = JSON.parse(line);
      return item.id ? [item] : [];
    } catch {
      return [];
    }
  });
if (!probes.length)
  record('decimal-self-test', [selftest], [digest(manifestSnapshot)], managedArtifacts, [
    'Decimal rejection tests did not produce cases',
  ]);
for (const probe of probes)
  record(
    probe.id,
    [
      {
        ...selftest,
        results: JSON.stringify(probe),
        exitCode: probe.status === 'passed' ? selftest.exitCode : 1,
      },
    ],
    [digest(manifestSnapshot)],
    managedArtifacts,
    [],
  );
const requirements = [];
const checkedInputs = [digest(manifestSnapshot)];
const check = (p) => {
  try {
    const actual = digest(absolute(p.path));
    checkedInputs.push(actual);
    if (actual.sha256 !== p.sha256) requirements.push('SHA256 mismatch: ' + p.path);
  } catch (error) {
    requirements.push('Missing pinned input: ' + p.path + '; ' + error.message);
  }
};
if (
  manifest.environment &&
  (manifest.environment.os !== os.platform() ||
    manifest.environment.arch !== os.arch() ||
    manifest.environment.node !== process.version ||
    manifest.environment.dotnet !== environment.tools.dotnet ||
    manifest.environment.libc !== environment.tools.libc ||
    manifest.environment.playwright !== environment.tools.playwright)
)
  requirements.push('Environment differs from the pinned manifest');
if (!Array.isArray(manifest.cases) || manifest.cases.length === 0)
  requirements.push('Corpus has no live cases');
const ids = new Set();
for (const c of manifest.cases) {
  if (!/^[a-zA-Z0-9][a-zA-Z0-9_-]*$/.test(c.id) || ids.has(c.id))
    throw new Error('Invalid or duplicate corpus case id');
  ids.add(c.id);
}
if (new Set(manifest.fonts.map((font) => font.key)).size !== manifest.fonts.length)
  requirements.push('Startup font keys are not unique');
let sawFallback = false;
for (const font of manifest.fonts) {
  if (font.fallback) sawFallback = true;
  else if (sawFallback)
    requirements.push('Startup font order differs: viewer registers plain fonts before fallbacks');
}
for (const artifact of manifest.viewer.packageArtifacts) {
  const content = fs.readFileSync(absolute(artifact.path));
  const integrity = 'sha512-' + crypto.createHash('sha512').update(content).digest('base64');
  if (integrity !== artifact.integrity)
    requirements.push('npm package integrity mismatch: ' + artifact.package);
}
if (
  manifest.runtime.nativeMetadata &&
  JSON.parse(fs.readFileSync(absolute(manifest.runtime.nativeMetadata.path))).sha !==
    manifest.runtimeFork
)
  requirements.push('Native runtime build identity differs from manifest');
if (manifest.schemaVersion !== 1) requirements.push('Unsupported manifest schema');
if (
  manifest.viewer.backend !== 'typescript' ||
  manifest.viewer.flags.rustReads !== false ||
  manifest.viewer.flags.rustSession !== false
)
  requirements.push('Only explicitly observed TypeScript reads are supported');
if (
  manifest.raster.comparison !== 'exact' ||
  manifest.raster.tolerance !== null ||
  manifest.raster.approval !== null
)
  requirements.push(
    'Raster tolerance requires separate human approval; this runner supports exact pixels only',
  );
if (!manifest.viewer.packageArtifacts.length || !manifest.viewer.packedFiles.length)
  requirements.push(
    'Packed viewer integrity/files unpinned; install packed artifacts and run --pin-packed',
  );
for (const p of [
  ...manifest.fonts,
  ...manifest.fonts.map((font) => font.license).filter(Boolean),
  ...manifest.corpusInputs,
  ...Object.entries(manifest.runtime)
    .filter(([key]) => key !== 'wasmBindings')
    .map(([, value]) => value),
  ...manifest.viewer.packageArtifacts,
  ...manifest.viewer.packedFiles,
])
  check(p);
if (
  !manifest.runtime.wasm ||
  !manifest.runtime.wasmBindings ||
  !manifest.runtime.bindingSource ||
  !manifest.runtime.bindingMap
)
  requirements.push('WASM module and exact binding-source/input hashes must be pinned');
else {
  const source = manifest.runtime.bindingSource;
  const bindingCommands = [];
  if (
    source.path !== 'packages/engine/runtime/src/core/pdf-functions.generated.ts' ||
    digest(absolute(source.path)).sha256 !== source.sha256
  ) {
    bindingCommands.push({
      command: 'verify pinned binding source',
      cwd: root,
      exitCode: 1,
      results: 'binding source differs from manifest',
    });
  } else {
    bindingCommands.push(
      execute(
        process.execPath,
        [
          'tooling/dotnet-parity/fixtures/check-binding-source.mjs',
          absolute(manifest.runtime.bindingMap.path),
          absolute(source.path),
        ],
        'packed-binding-source',
      ),
    );
    bindingCommands.push(
      execute(
        process.execPath,
        [
          'tooling/dotnet-parity/fixtures/write-abi-input.mjs',
          absolute(manifest.runtime.wasmBindings.path),
        ],
        'wasm-abi-input',
      ),
    );
    check(manifest.runtime.wasmBindings);
    bindingCommands.push(
      execute(
        process.execPath,
        [
          'packages/engine/runtime/build/verify-wasm-abi.mjs',
          absolute(manifest.runtime.wasm.path),
          absolute(manifest.runtime.wasmBindings.path),
        ],
        'wasm-abi',
      ),
    );
  }
  const passed = bindingCommands.every((command) => command.exitCode === 0);
  record(
    'wasm-abi',
    bindingCommands,
    [...checkedInputs],
    [],
    passed ? [] : ['WASM ABI verification failed; live acceptance refuses this payload'],
  );
  if (!passed)
    requirements.push('WASM ABI verification failed; live acceptance refuses this payload');
}
if (!packedDirectory) requirements.push('Packed production vite-app directory not configured');
const nativeDirectory = process.env.EPDF_NATIVE_DIR;
if (nativeDirectory && path.isAbsolute(nativeDirectory)) {
  const library = path.join(
    nativeDirectory,
    os.platform() === 'win32'
      ? 'epdf_capi.dll'
      : os.platform() === 'darwin'
        ? 'libepdf_capi.dylib'
        : 'libepdf_capi.so',
  );
  if (fs.existsSync(library)) checkedInputs.push(digest(library));
  else requirements.push('Native capi library missing: ' + library);
}
if (!nativeDirectory || !path.isAbsolute(nativeDirectory))
  requirements.push('EPDF_NATIVE_DIR must name the absolute locally built capi directory');
const playwright =
  process.env.EPDF_PARITY_PLAYWRIGHT ??
  absolute('tooling/viewer-e2e/node_modules/@playwright/test/cli.js');
if (!fs.existsSync(playwright))
  requirements.push('Install the repository Playwright dependency: ' + playwright);
for (const c of manifest.cases) {
  const commands = [],
    limitations = [...requirements],
    artifacts = [...managedArtifacts];
  const inputs = [...checkedInputs];
  try {
    const pdf = digest(absolute(c.pdf));
    inputs.push(pdf);
    if (pdf.sha256 !== c.sha256) limitations.push('PDF SHA256 mismatch: ' + c.pdf);
    const requestPath = path.join(out, c.id + '.request.json');
    fs.writeFileSync(
      requestPath,
      JSON.stringify({
        ...c,
        fonts: manifest.fonts,
        runtimeWasmSha256: manifest.runtime.wasm?.sha256,
      }) + '\n',
    );
    inputs.push(digest(requestPath));
    if (!limitations.length) {
      const viewer = path.join(out, c.id + '.viewer.json'),
        native = path.join(out, c.id + '.native.json');
      const env = {
        VIEWER_E2E_PACKED_DIR: packedDirectory,
        VIEWER_E2E_BACKENDS: 'typescript',
        EPDF_PARITY_REQUEST: requestPath,
        EPDF_PARITY_ROOT: root,
        EPDF_PARITY_VIEWER_OUTPUT: viewer,
      };
      commands.push(
        execute(
          process.execPath,
          [
            playwright,
            'test',
            '--config',
            'tooling/viewer-e2e/playwright.config.ts',
            '--project=chromium-typescript',
            'dotnet-parity.spec.ts',
          ],
          c.id + '.viewer',
          env,
        ),
      );
      commands.push(
        execute(
          dotnet,
          ['run', '--no-build', '--project', project, '--', 'snapshot', root, requestPath, native],
          c.id + '.native',
        ),
      );
      if (commands.every((command) => command.exitCode === 0))
        commands.push(
          execute(
            dotnet,
            ['run', '--no-build', '--project', project, '--', 'compare', viewer, native],
            c.id + '.compare',
          ),
        );
      for (const p of [viewer, native, viewer + '.backend.json'])
        if (fs.existsSync(p)) artifacts.push(digest(p));
    } else
      commands.push({
        command: [process.execPath, ...process.argv.slice(1)].map(quote).join(' '),
        cwd: root,
        exitCode: 1,
        results: 'failed prerequisites; no live parity claim',
      });
    if (manifest.deploymentFontBundle.status !== 'supplied')
      limitations.push(deploymentFontLimitation);
  } catch (error) {
    limitations.push(error.stack ?? String(error));
    if (!commands.length)
      commands.push({
        command: 'validate parity case ' + c.id,
        cwd: root,
        exitCode: 1,
        results: 'failed before live execution',
      });
  }
  record(c.id, commands, inputs, artifacts, limitations);
}
fs.writeFileSync(
  path.join(out, 'summary.json'),
  JSON.stringify(
    { status: summary.every((c) => c.status === 'passed') ? 'passed' : 'failed', cases: summary },
    null,
    2,
  ) + '\n',
);
console.log(JSON.stringify(summary, null, 2));
process.exitCode = summary.every((c) => c.status === 'passed') ? 0 : 1;
