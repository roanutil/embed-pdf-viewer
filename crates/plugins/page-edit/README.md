# epdf-plugin-page-edit

`packages/plugin/page-edit` in Rust. All 12 cases in `vectors/plugins/page-edit.json` replay
identically (`cargo test -p epdf-plugin-page-edit`).

The verbs return the engine's own abortable promise, so a deferred edit settles later and an abort
reaches the fake engine's log (`deferred-and-aborted`). There is no client-side gate: `canEdit()`
is the one `doc.pages.assemble` twin every verb shares, and a refused edit is the engine's
`PermissionDenied`, passed through with `required` and `context` (`refused-by-the-engine`).

Two JavaScript behaviours are ported on purpose. `rotateBy` does its wrap in `f64`, so a delta of
`45`, `0.5` or `NaN` reaches the engine exactly as the TypeScript computes it. And an `index`
placement that isn't an integer makes `neighbourSize` read `pages[1.5]`, which throws
`TypeError: Cannot read properties of undefined (reading 'size')`; the Rust returns that error
verbatim (`add-blank-fractional-index`).

The generators reach every line of the TypeScript package (statements 231/231, branches 37/40).
The three branches they cannot reach are the no-document arms in `capability.ts`: `requireDoc` at
line 29, the `ctx.document()?.pages ?? []` fallback at line 60, and `ctx.doc?.` in `canEdit` at
line 72. The plugin is document-scoped, so its context always carries a handle, and a capability
captured before its document closed still reads the last registry the kernel staged
(`a-captured-capability-outlives-its-document`).

A placement object with none of `index`, `after` or `before` is not ported. The TypeScript reads
`placement.before` as `undefined` and throws `placement page not found: undefined`; `PagePlacement`
is an enum and cannot express it.

`cargo mutants -p epdf-plugin-page-edit --test-package epdf-plugin-page-edit` catches all 45
viable mutants. The 32 unviable ones replace a return value with `Default::default()` for a type
that has none: 24 of them the verbs' `EngineCall<Value>`, the rest `AnyPlugin`, `PdfSize`, the
document handle and the placement anchor.
