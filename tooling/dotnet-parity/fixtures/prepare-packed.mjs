import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
const root = process.cwd();
const out = path.resolve(process.argv[2] ?? '/tmp/embedpdf-dotnet-parity');
fs.mkdirSync(out, { recursive: true });
const tarballs = path.join(out, 'tarballs'),
  app = path.join(out, 'vite-app');
fs.mkdirSync(tarballs, { recursive: true });
fs.mkdirSync(path.join(app, 'src'), { recursive: true });
const sh = (command, args, cwd = root) =>
  execFileSync(command, args, {
    cwd,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
    maxBuffer: 32 * 1024 * 1024,
  });
const packages = JSON.parse(sh('/usr/local/bin/pnpm', ['list', '-r', '--depth', '-1', '--json']));
const byName = new Map(packages.map((p) => [p.name, p]));
const selected = new Map();
function visit(name, optional = false) {
  if (selected.has(name)) return;
  const pkg = byName.get(name);
  if (!pkg) {
    if (optional) return;
    throw new Error('Missing workspace ' + name);
  }
  const json = JSON.parse(fs.readFileSync(path.join(pkg.path, 'package.json')));
  if (json.files?.[0] && !fs.existsSync(path.join(pkg.path, json.files[0]))) {
    if (optional) return;
    throw new Error('Build first: ' + name);
  }
  selected.set(name, pkg);
  for (const [dependency, version] of Object.entries(json.dependencies ?? {}))
    if (String(version).startsWith('workspace:')) visit(dependency);
  for (const [dependency, version] of Object.entries(json.optionalDependencies ?? {}))
    if (String(version).startsWith('workspace:')) visit(dependency, true);
}
for (const name of [
  '@embedpdf/viewer-react',
  '@embedpdf/engine',
  '@embedpdf/engine-core',
  '@embedpdf/core-geometry',
])
  visit(name);
const pinned = {};
for (const [name, pkg] of selected) {
  const before = new Set(fs.readdirSync(tarballs));
  sh('/usr/local/bin/pnpm', ['pack', '--pack-destination', tarballs], pkg.path);
  const filename =
    fs.readdirSync(tarballs).find((f) => !before.has(f)) ??
    [...fs.readdirSync(tarballs)].find((f) => f.startsWith(name.slice(1).replace('/', '-') + '-'));
  if (!filename) throw new Error('No packed tarball for ' + name);
  pinned[name] = 'file:' + path.join(tarballs, filename);
}
fs.writeFileSync(
  path.join(app, 'package.json'),
  JSON.stringify(
    {
      private: true,
      type: 'module',
      dependencies: { ...pinned, react: '18.3.1', 'react-dom': '18.3.1', vite: '6.3.5' },
      overrides: pinned,
    },
    null,
    2,
  ),
);
for (const filename of ['e2e.html', 'src/e2e.tsx'])
  fs.copyFileSync(path.join(root, 'examples/viewer-react', filename), path.join(app, filename));
fs.writeFileSync(
  path.join(app, 'vite.config.js'),
  "export default { build: { target: 'es2022', rollupOptions: { input: 'e2e.html' } } };\n",
);
console.log(sh('npm', ['install', '--no-audit', '--no-fund', '--loglevel=error'], app));
console.log(sh('npm', ['exec', 'vite', '--', 'build'], app));
console.log(app);
