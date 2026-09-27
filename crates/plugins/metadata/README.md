# epdf-plugin-metadata

`packages/plugin/metadata` in Rust, at the v3 contract. All 14 cases in
`vectors/plugins/metadata.json` replay identically (`cargo test -p epdf-plugin-metadata`).

The crate has the TypeScript's shape. `model.rs` is `model.ts` (status, value and revision, the
three actions, `changed_keys`), and `controller.rs` is `controller.ts`: `create_metadata_controller`
returns the api and a `connect` that subscribes to the document event stream and starts the seed
read. `publish` is the one place the value changes and `onUpdated` fires. `can_edit` returns
`Result<bool, Error>` because the TypeScript's `ctx.doc` getter can throw, and `update` takes the
`OperationOptions` signal as `Option<AbortSignal>`.

## Coverage

The metadata suite reaches 233 of the package's 234 statements and 39 of its 41 branches. What it
does not reach is in `model.ts`, and neither is reachable from the public surface:

- `model.ts:24`, the `loading` action arriving while the status is already `loading`. `load` is
  single-flight, and every way out of it leaves the status `ready` or `error`, or ends the instance.
- `model.ts:29-30`, the reducer's `default` arm. The action is a three-member union, and in Rust an
  enum with three variants.

## Pinned TypeScript behaviour

`changedKeys` compares `custom` by `JSON.stringify`, so the same entries in another order count as
a change (`on-updated-reports-each-confirmed-change`). It also lists only the keys of the new
value: a key the new metadata drops is never reported. Engines send every key, so this only shows
with a sparse event.

A refused seed read (`PermissionDenied` from the engine) sets the status `forbidden` and keeps the
snapshot `null`. A refused `update` changes nothing, not even the status.

`scripts/mutants.sh -p epdf-plugin-metadata --test-package epdf-plugin-metadata --file <file>`, file
by file over every module but `harness.rs`, catches 35 of 37 viable mutants (35 caught, 0 timeouts;
19 unviable). Two of those are killed by `changed-keys-beyond-the-info-dict`, checked by applying
each by hand: deleting the object-or-array arm of `strict_equals`, and `==` to `!=` in its number arm.
Both survivors are in `src/model.rs`, both in `strict_equals`:

- Deleting the `(None, None)` arm. `changed_keys` only looks up keys of `next`, so `b` is never
  `None`; the arm is unreachable.
- Deleting the number arm. It falls through to `Value ==`, which differs from `as_f64() ==` only
  for the same number held as an integer on one side and a float on the other, or for `-0` against
  `0`. Every number here is parsed from canonical JSON, which writes a number one way and writes
  `-0` as `0`, so the two sides always parse alike.
