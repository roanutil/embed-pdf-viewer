# epdf-plugin-metadata

`packages/plugin/metadata` in Rust. All seven cases in `vectors/plugins/metadata.json` replay
identically (`cargo test -p epdf-plugin-metadata`).

The generators reach every line of the TypeScript package. Four branches they cannot reach, and
none of them is reachable from the public surface:

- `capability.ts:23` and `capability.ts:33`, the `ctx.doc` null arms of `requireDoc` and `canEdit`.
  The plugin is document-scoped, so its context always carries a handle.
- `effects.ts:17`, the same guard in the effect.
- `reducer.ts:8`, the arm for an action that is not `SET`. The action type is a one-member union,
  and in Rust an enum with one variant.

`cargo mutants -p epdf-plugin-metadata --test-package epdf-plugin-metadata` catches 13 of 14 viable
mutants. The survivor replaces `initial_metadata_state()` with `Default::default()`, which builds
the same value.
