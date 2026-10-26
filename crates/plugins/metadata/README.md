# epdf-plugin-metadata

`packages/plugin/metadata` in Rust, at the current TypeScript. All 17 cases in
`vectors/plugins/metadata.json` replay with no member-order allowance or outcome exception
(`cargo test -p epdf-plugin-metadata`).

The crate has the TypeScript's shape. `contract.rs` is `contract.ts` (the four events and the
`OperationOptions` the verbs take), `model.rs` is `model.ts` (`changed_keys`), and `controller.rs`
is `controller.ts`. The standard fields and the custom keys are two kernel mirrors,
`mirror/metadata` and `mirror/customMetadata`, both `onDemand`: opening a document reads nothing,
and the first `ensure_loaded` reads both. Each mirror changes only when a load lands or its
confirmed event arrives (`metadata.updated`, `metadata.customUpdated`), and its `changed` hook is
the one place `on_updated` and `on_resynced` fire. `refresh` re-reads one half; a refresh during a
read in flight joins it and queues one reread, as `mirror.ts` does. The writes check
`doc.metadata.modify` and the caller's signal before the engine sees anything, then resolve with
the engine's answer after the confirmed event has updated the snapshot.

The metadata stays the engine's JSON (ISO 8601 `createdAt` and `modifiedAt`, the tri-valued
`trapped`). `src/ffi.rs` decodes it into typed records for Swift and Kotlin.

## Tests

`tests/conformance.rs` replays the vectors. `tests/behavior.rs` pins what they do not print: no
read at open, the one queued reread, the exact `previous`, `changedKeys` and `origin` of each event,
custom keys removed, added and set to `''`, key order alone changing no key, the local refusals,
and the turns each verb settles on, measured on the TypeScript with `tsx`.

`src/ffi.rs` and `tests/ffi.rs` were migrated by hand in Task 13.26 and are not compiled:
`epdf-ffi` builds again with Task 13.33, which owns running them.

## Rust-only divergences

None is reached by a vector or by the engine's answers.

- `changed_keys` reads the keys of an object only. TypeScript's `Object.keys` on a non-null
  primitive `next` (a string, say) lists its indices; here it lists nothing.
- A mirror load with no document handle rejects with an `Error`. The TypeScript would throw a
  `TypeError` reading `doc.metadata`; the kernel always hands a mirror its guarded handle.
