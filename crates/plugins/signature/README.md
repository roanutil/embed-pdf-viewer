# epdf-plugin-signature

`packages/plugin/signature` in Rust at the v3 contract: document-scoped digital signing over
`epdf-signature-core`. All 54 cases in two vector files replay identically
(`cargo test -p epdf-plugin-signature`):

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/signature.json` | scenario | 35 | reads over the snapshot, the authority twins, hydration and the working-copy invalidation edge, document events, fills, the sign-here target, one-shot and two-phase signing, the destination rule, the armed-mark handler, the two unit-test shapes |
| `vectors/plugins/signature-functions.json` | function | 19 | the reducer, `blankPagePdf`, `markRoleOf`, `isTarget`, `sameRef` |

The scenario registry runs the real interaction, annotation, form and stamp adapters beside this
one. The generator is `tooling/plugin-conformance/src/suites/signature-plugin.ts`.

## Shape

The files follow the TypeScript. `contract.rs`, `model.rs` and `blank_page.rs` are the vocabulary,
the slice and the clearing appearance. `controller.rs` builds the services once and each area is an
`impl` block on the one `SignatureCapability`: `services/{authority,events,marks,siblings,store}`,
`read/signatures`, `sync/{hydration,document_events}`, `write/{sign,fill,place}` and
`tools/armed_mark`. `signature_plugin` requires the form token and takes the interaction, stamp and
annotation tokens as optional; its `connect` registers `ArmedMarkHandler` when both the hub and the
stamp plugin are there.

Engine DTOs stay `serde_json::Value`, as they do in `epdf-signature-core`: the snapshot, a
`SignatureDTO`, a verdict, a field ref. `Mark` is an enum (`Asset` or `Source`), a signer is a
`SignerSource` (a `SignerPort` or a thunk resolving one), and `placeMark`'s target is `PlaceTarget`.
`analyzeChanges` and `readRevision` answer `Result<Promise<_>, _>`, because the TypeScript throws
synchronously when the engine has no signatures service. The TypeScript memoizes the two lists per
snapshot to keep them reference-stable; the Rust hands out copies.

`SignatureHost::console_error` is the `globalThis.console?.error` the plugin reports a failing
listener and every background failure to.

## Turn order

`src/promise.rs` carries the rules. An awaited engine call resumes one turn later than a plain
promise, because the guarded handle answers an `AbortablePromise`. The first replay put every
snapshot ahead of the form plugin's until `refresh` awaited that way. `return promise` from an
`async` function costs the adoption turns (`adopt`), and `.then(...).catch(...)` chains take one
more turn before the report.

WebCrypto answers from libuv's pool in the TypeScript, so only the cases whose call reaches the
completion gate or a CMS build set `awaitCalls`. Every validation the plugin starts on its own
(the initial read, a new version, a scheduled re-judgement, the one after signing) judges a snapshot
whose revision chain is broken, which never reads a CMS. That keeps background work on microtasks,
where both sides order it the same way. The cost: this suite never has the plugin judge a CMS
itself; `vectors/core/signature-document.json` pins that.

## Harness change

The fake engine's `forms` proxy now drops any `forms.*` member a document's `lacks` list names,
`submit` aside, on both sides (`fake-engine.ts` and `fake_engine.rs`). Before, only
`forms.applyEffects` could be taken away, and the Rust `has` answered false for every other
`forms.*` member. The cases need `forms.setSignatureAppearance` absent to reach `unsupported` and a
false `canFill`.

## Coverage

Across the whole corpus the generators reach all 1,375 of the package's statements and 303 of its
307 branches (`vectors/coverage.json`). The four branches left need a document plugin without a
document, which the v3 kernel never builds: a controller exists only after its session has a handle,
so `ctx.doc` is always set.

- `services/authority.ts:10`, the `?? false` of `ctx.doc?.security`.
- `services/store.ts:27`, `requireDoc`'s `not-ready`.
- `services/store.ts:43`, `documentId`'s `not-ready`.
- `sync/document-events.ts:17`, the early return without a document.

## Pinned TypeScript behaviour

A raw signer's certificate subject becomes the default `/Name` even when the signing then fails,
and the caller's `attribution` spreads over it (`sign-with-a-raw-signer`). `isTarget` in
`write/sign.ts` is exported and never called; a function case covers it. `getSignature` for an
address with neither `kind` nor `annotObjectNumber` compares `undefined` against every widget, so
it would match a signature whose widget is null; no vector sends one, and the Rust keeps that
comparison.

## Mutation testing

Not run yet. It happens centrally through `scripts/mutants.sh`.
