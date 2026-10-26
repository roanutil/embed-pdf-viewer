import path from 'node:path';

// Every workspace crate that package `root` links (normal and build
// dependencies), as sorted crate directories relative to the workspace's
// crates/ (`epdf-engine`, `plugins/actions`). Registry crates are left out.
export function linkedLocalCrates(metadata, root) {
  const nodes = new Map(metadata.resolve.nodes.map((n) => [n.id, n]));
  const packages = new Map(metadata.packages.map((p) => [p.id, p]));
  const linked = new Set();
  const stack = [metadata.packages.find((p) => p.name === root).id];
  while (stack.length) {
    const id = stack.pop();
    if (linked.has(id)) continue;
    linked.add(id);
    for (const dep of nodes.get(id).deps) {
      if (dep.dep_kinds.some((k) => k.kind === null || k.kind === 'build')) stack.push(dep.pkg);
    }
  }
  return [...linked]
    .map((id) => packages.get(id))
    .filter((p) => !p.source)
    .map((p) =>
      path
        .relative(path.join(metadata.workspace_root, 'crates'), path.dirname(p.manifest_path))
        .split(path.sep)
        .join('/'),
    )
    .sort();
}

// The sorted entries of the `variable=(a b c)` line in a shell script.
export function listedCrates(script, variable) {
  return script
    .match(new RegExp(`^${variable}=\\((.*)\\)$`, 'm'))[1]
    .split(/\s+/)
    .sort();
}
