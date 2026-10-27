# epdf-plugin-signature

`packages/plugin/signature` in Rust on the current kernel: document-scoped digital signing over
`epdf-signature-core`. All 53 cases in two vector files replay identically
(`cargo test -p epdf-plugin-signature`), with no member-order allowance and no outcome exception:

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/signature.json` | scenario | 39 | reads over the mirrored snapshot, the per-field authority twins, loading and the working-copy invalidation edge, document events, fills, the sign-here target, one-shot and two-phase signing, the destination rule, the armed-mark handler, the connected controller |
| `vectors/plugins/signature-functions.json` | function | 14 | `initialSignatureState`, `setVerdicts`, `setTarget`, `setBusy`, `blankPagePdf`, `markRoleOf`, `sameFieldRef` |

The scenario registry runs the real interaction, annotation, form and stamp adapters beside this
one. The generator is `tooling/plugin-conformance/src/suites/signature-plugin.ts`. Some of its
`covers` texts still describe the pre-upstream plugin; the traces are current. The list is in
"Stale case descriptions" below.

`tests/behavior.rs` names the facts the brief asks for on top of the replay (a free asset placement
falls back to the stamp plugin and settles on annotation 60, an imported signatures library's mark
reaches a field as a sign request, a refused fill and a failed signer clean up, an annotation hit
keeps precedence over the armed mark) and drives what no vector reaches: a settings change, a
signal that fires before the seal, a parked signing the engine disowns, and a cancellation for
another signing.

## Shape

The files follow the TypeScript. `contract.rs`, `model.rs`, `blank_page.rs` and `connect.rs` are
the vocabulary, the session state and its transitions, the clearing appearance and the sibling
wiring. `controller.rs` builds the services once; each area is an `impl` block on the one
`SignatureCapability`: `services/{authority,errors,events,marks,settings,siblings,store}`,
`read/{signatures,judging}`, `sync/{signatures,validation}`, `write/{sign,fill,place}` and
`tools/armed_mark`.

The session state is `{ verdicts, target, busy }`, changed only by `set_verdicts`, `set_target` and
`set_busy`. The verdicts and the target are held by `Rc`, so a transition tells the same value from
an equal one as `===` does. The signature facts (`{ snapshot, pending }`) are the kernel mirror
`signatures`, loaded from `signatures.list` and folded from confirmed events by
`fold_signature_event`. `onSigned` and `onProtectionChanged` fire from the mirror's `changed`, for
every session alike, and a two-phase signing parks and clears through `signatures.prepared`,
`signatures.cancelled` and `signatures.completed`.

The settings are `{ key, mode, trust, allowCertify }` in the kernel's settings store. A key (a
`SignerPort` or a function answering one) and a trust port are not JSON, so the store holds a handle
into the registration's `PortTable`, as stamp holds its asset engine. Both are `whole`. Engine DTOs
stay `serde_json::Value`, as they do in `epdf-signature-core`.

`console.rs` is the `globalThis.console?.error` the plugin reports background failures to (a
validation, an armed mark that fails to land). Listener failures go to the kernel report.

## Migrating from the pre-13.25 API

For `epdf-viewer-host` and `epdf-web` (Task 13.33).

| Old | New |
|---|---|
| `signature_plugin(SignatureConfig { mode, signer, trust, allow_certify, host })` | `signature_plugin(SignatureConfig { key, mode, trust, allow_certify })`, each `Option<Option<_>>` for "not given" and `null`; `SignatureHost` is `console::set_sink` |
| `SignerSource::{Port, Thunk, Host { resolve, available }}` | `SignatureKey::{Port, Thunk}`. The mode follows the key setting, as `services/authority.ts` reads it: a host that wants `visual` until its slot holds a signer registers the mode or updates the key setting |
| `SignFieldInput.signer`, `.attribution` | `.key` (a per-call key) and `.signer` (the `SignatureSignerInput` facts). The ffi records `SignatureSignInput` and `SignaturePrepareInput` name it `signer` (`SignatureSignerInput`), as the TypeScript does, since Task 14.7 |
| `SignerPort` as a setting | `SignatureKey::port(port)`, held by `Rc` and compared by identity: a clone is the same key, a port rebuilt from the same parts is another (a Rust-only divergence, below) |
| `create_signature_capability`, `signature_reducer`, `SignatureAction`, `ResourceStatus` | removed with the TypeScript; `get_status` answers the kernel's `mirror::ResourceStatus` |
| `abort_pending()` | `cancel_pending(options)`, over `signatures.cancel` |
| `can_sign()`, `can_fill()` | `can_sign(&field)`, `can_fill(&field)`: per field, by `ctx.allowsField`; new `can_read_revision()` |
| `on_invalidating`, `SignatureInvalidatingEvent`, `SignatureFieldEvent` | `on_invalidation_predicted`, `SignatureInvalidationPredictedEvent`, `SignatureFilledEvent`, `SignatureClearedEvent` (no `origin`) |
| `refresh()`, `validate(ValidateRequest)`, `analyze_changes(input) -> Result<Promise>`, `read_revision(&target) -> Result<Promise>` | every verb takes `OperationOptions` or `SignatureValidateOptions` and answers a `Promise`; `read_revision` needs `doc.download` and reads `signatures.downloadRevision` |
| `list_signatures`, `list_unsigned_fields` | answer `Rc<Vec<Value>>`, memoized per snapshot and verdicts; each signed entry carries its `verdict` |
| `PlaceTarget::Free(StampPlacement)` | `PlaceTarget::Free(Value)`, a `StampPlacement` as JSON (`page`, `center`, `targetWidth`) |
| `is_target`, `same_ref` | `set_target`, `same_field_ref`; refs are `{ kind: 'objectNumber', objectNumber }` |

`src/ffi/` and `tests/ffi.rs` were migrated by hand, and are compiled and tested since Task 13.33. `SignatureHandle::abort_pending` is `cancel_pending`, `can_sign` and `can_fill` take
a field, `SignatureEvent::Invalidating` is `InvalidationPredicted`, `SignatureSignerClaims` reads
`signedAt`, and a signer completion's `fail` raises a plain error, which the verb answers as
`operation-failed` with that cause, as a rejecting TypeScript signer port is answered
(`sign-with-a-raw-signer`). Known call sites: `crates/epdf-viewer-host` (`session.rs`,
`api.rs`, `lib.rs`, `tests/common/mod.rs`, `tests/viewer_host.rs`) and `crates/epdf-web`
(`src/adapters.rs`).

## Turn order

Each TypeScript `await` is a plain `.await`, and `return promise` from an `async` function adopts it
(`promise::adopt`). The one exception is the dynamic import, listed below.

## Kept and new Rust-only divergences

None is reached by a vector.

- The key and trust settings are stored as handles (`services/settings.rs`).
- `strict_eq` compares two JSON objects as unequal (`js.rs`): a copy has no identity to share.
  `getSignature` compares widget object numbers, which are numbers or `undefined`.
- `SealUnlessCancelled` forwards every `DocumentHandle` member but `signatures.complete` to the
  guarded handle. The TypeScript's stand-in has only `signatures`, so any other member would be a
  `TypeError`; `epdf-signature-core`'s `sign` calls only `signatures.*`.
- `await import('@embedpdf/core-signature')` is one turn (`sync/validation.rs` `import_module`),
  Ruling13.25. Node 24.20.0 resumes it eight turns after the call for an already evaluated module
  (measured with a scratch probe); module-load turns are the host's, not a contract, and the
  vectors pass at one and at two. The cost: a native caller racing a validation against another
  microtask chain can see a different interleaving than Node.
- The key and trust settings compare by identity (`SignatureKey::same`, the trust `Rc`), where the
  TypeScript store's `sameValue` deep-compares plain objects
  (`packages/core/main/src/settings.ts:204-218`, called at `:156` and `:191`). The consequence: a
  CMS port rebuilt over the same `sign` function reports a settings change here and none in the
  TypeScript. A raw port's chain is an array of `Uint8Array`s, which `sameValue` compares
  element by element and each buffer by identity: a raw port rebuilt with new buffers is a change
  on both sides, one reusing the same buffers and `sign` is a change only here. Task 13.11 made the same choice for
  interaction's tools (`docs/research/rust-plugins-branch-review.md:92`).
- `PortTable` keeps every key and trust port it has handed a handle for, for the registration's
  life: the store's registered value still names the first ones, and `resetSettings` must find
  them. A host that swaps keys often grows the table by one entry per swap. Task 14 carries this
  with stamp's `EngineTable` and interaction's `ToolTable`, which have the same shape.

## Pinned TypeScript behaviour

`foldSignatureEvent` clears a parked signing on any `signatures.cancelled`, whatever its
`signingId` (`model.ts:145-146`), and the Rust fold does the same
(`a_cancellation_clears_the_parked_signing_whatever_its_id`). A cancellation published for another
signing therefore unparks this one. That is upstream behaviour, carried to Task 14 for a decision.

## Stale case descriptions

The generator's `covers` texts for these cases describe the pre-upstream plugin. The traces are the
current TypeScript's; the generator is not changed here.

| Case | Stale claim | Current behaviour |
|---|---|---|
| `signature-service-refuses-unsupported` | no signatures service; `readRevision` throws synchronously; `abortPending` | the service answers `Unsupported`; `readRevision` rejects; `cancelPending` |
| `authority-twins` | `canSign` needs a configured signer, `canFill` the engine door | both are `allowsField` on the form's field |
| `the-initial-read-fails` | the initial read is reported | the mirror's status reads `error`; nothing is reported |
| `document-events` | an aborted signing; a field-set change re-judges soon | `signatures.cancelled`; a field-set change reloads, and a load re-judges at once |
| `fill-and-clear` | a fill re-reads the form | no form re-read |
| `two-phase-signing`, `sign-one-shot` | completion re-reads; the caller's attribution | the mirror folds `signatures.completed` and re-judges soon; `signer` |
| `abort-the-parked-signing`, `a-parked-signing-from-elsewhere` | `abortPending` | `cancelPending` |
| `sign-with-a-raw-signer` | the caller's attribution | `signer` |
| `sign-without-a-signer` | an unresolvable mark rejects `not-found` | the one step rejects `invalid-input` (no key) |

## Mutation testing

Not rerun for this port. The counts the pre-13.25 README gave were over files this task replaced
(`sync/hydration.rs`, `sync/document_events.rs`); Task 14 reruns `scripts/mutants.sh` file by file.
