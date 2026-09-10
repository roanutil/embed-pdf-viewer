# Node prototype bindings

[`crates/epdf-node`](../../crates/epdf-node/src/lib.rs) exposes `openDocument`
and six `Document` methods: `pageCount`, `pageSize`, `renderPage`, `pageText`,
`search`, and `close`. Calls are synchronous; JavaScript receives values rather
than raw PDFium pointers.

Build and test from the prototype root:

```bash
bash build/build-node.sh
node --test packages/node/*.test.mjs
```

## Error codes

Errors converted from `epdf_ops::OpsError` carry a `.code` alongside `.message`.
The mapping is defined in [`error.rs`](../../crates/epdf-ops/src/error.rs).
The web adapter exposes the same core error codes. Module-loading errors and
binding-level argument errors are outside this mapping.

| Code | Meaning |
| --- | --- |
| `LOAD_FAILED` | PDFium could not open the supplied bytes. |
| `PASSWORD_REQUIRED` | The password is missing or incorrect. |
| `PAGE_OUT_OF_RANGE` | The requested page is past the document's last page. |
| `RENDER_FAILED` | Rendering failed, or the scale/buffer size was rejected. |
| `CLOSED` | The document is closed or its internal entry is unavailable. |
| `ENGINE_FAILED` | The worker's job or reply channel is unavailable. |
| `INTERNAL` | Another core failure, including a caught Rust panic. |

```js
try {
  doc.pageSize(index);
} catch (err) {
  if (err.code !== 'PAGE_OUT_OF_RANGE') throw err;
  // Choose a valid zero-based page index.
}
```

A closed document cannot be reopened through the same handle. `ENGINE_FAILED`
should be treated as requiring a replacement engine, even though a lost reply
alone does not prove that its worker thread died. This addon caches one engine
and exposes no replacement API, so recovery requires restarting the Node process.
An initial engine-construction failure is not cached: a later `openDocument`
tries initialization again.

## Coverage

`vectors.test.mjs` checks fixture output and the codes `LOAD_FAILED`, `CLOSED`,
and `PAGE_OUT_OF_RANGE`. `additional.test.mjs` runs shared password and color
checks. The password test checks error messages, not the `PASSWORD_REQUIRED`
code. `platform.test.mjs` checks target selection.

This Node suite does not directly assert `RENDER_FAILED`, `ENGINE_FAILED`, or
`INTERNAL` codes. Rust tests separately cover invalid render scales and a
caught worker panic. Test coverage in one adapter does not establish coverage
in every adapter.
