import assert from 'node:assert/strict';
import test from 'node:test';
import { linkedLocalCrates, listedCrates } from './linked-crates.mjs';

function pkg(name, source = null, dir = name) {
  return { id: name, name, source, manifest_path: `/repo/crates/${dir}/Cargo.toml` };
}
function dep(pkg, kind) {
  return { pkg, dep_kinds: [{ kind }] };
}
function fixture(extra = {}) {
  return {
    workspace_root: '/repo',
    packages: [pkg('a'), pkg('b'), pkg('c'), pkg('d'), ...(extra.packages ?? [])],
    resolve: {
      nodes: [
        {
          id: 'a',
          deps: [dep('b', null), dep('c', 'build'), dep('d', 'dev'), ...(extra.deps ?? [])],
        },
        { id: 'b', deps: [] },
        { id: 'c', deps: [] },
        { id: 'd', deps: [] },
        ...(extra.nodes ?? []),
      ],
    },
  };
}

test('linkedLocalCrates follows normal and build deps, not dev', () => {
  assert.deepEqual(linkedLocalCrates(fixture(), 'a'), ['a', 'b', 'c']);
});

test('linkedLocalCrates drops registry crates', () => {
  const metadata = fixture({
    packages: [pkg('serde', 'registry+https://github.com/rust-lang/crates.io-index')],
    nodes: [{ id: 'serde', deps: [] }],
    deps: [dep('serde', null)],
  });
  assert.deepEqual(linkedLocalCrates(metadata, 'a'), ['a', 'b', 'c']);
});

test('linkedLocalCrates names a nested crate by its path under crates/', () => {
  const metadata = fixture({
    packages: [pkg('actions', null, 'plugins/actions')],
    nodes: [{ id: 'actions', deps: [] }],
    deps: [dep('actions', null)],
  });
  assert.deepEqual(linkedLocalCrates(metadata, 'a'), ['a', 'b', 'c', 'plugins/actions']);
});

test('listedCrates reads the array line', () => {
  assert.deepEqual(listedCrates('X=1\nRUST_CRATES=(b a)\n', 'RUST_CRATES'), ['a', 'b']);
});
