# @epdf/node (scaffold)

Thin Node bindings over the PDFium engine in `crates/epdf-node`, six methods on `Document` and
`openDocument`, no pointers exposed to JavaScript.

## Error codes

Every thrown error carries a `.code` property distinct from its `.message`, so you can switch on
the code instead of matching against the human-readable text. The codes come from
`epdf_ops::OpsError` (see `crates/epdf-node/src/lib.rs`'s `to_napi`), and the match there has no
`_` arm: a new `OpsError` variant fails the Rust build rather than silently landing on `INTERNAL`.

```js
try {
  doc.pageSize(index);
} catch (err) {
  switch (err.code) {
    case 'PAGE_OUT_OF_RANGE':
      // index was past the end of the document
      break;
    default:
      throw err;
  }
}
```

| Code | Meaning |
| --- | --- |
| `LOAD_FAILED` | The bytes passed to `openDocument` are not a document PDFium can open. |
| `PASSWORD_REQUIRED` | The document is encrypted and the password was wrong or missing. |
| `PAGE_OUT_OF_RANGE` | The page index named is past the end of the document. |
| `RENDER_FAILED` | Rendering the page failed. |
| `CLOSED` | The document is closed, either by the caller or because it never existed in this engine's slab. |
| `ENGINE_FAILED` | The worker thread that owns every document in this engine is gone. |
| `INTERNAL` | An unexpected failure with no more specific code. |

Two of the seven are load-bearing rather than merely informative:

- `CLOSED` means the caller used a handle it had already closed. That's a caller bug: open a new
  document, don't retry the same handle.
- `ENGINE_FAILED` means the worker thread behind every `Document` from this engine has died. Every
  document from that engine is now unusable, and the fix is a new `Engine`, not a new `Document`.
  This addon has no such recreation path: it opens one process-wide engine in
  `crates/epdf-node/src/lib.rs`'s `engine()` and never rebuilds it. So `ENGINE_FAILED` here means
  restarting the Node process, not catching the error and continuing.

## Testing

`packages/node/vectors.test.mjs` asserts `LOAD_FAILED`, `CLOSED`, and `PAGE_OUT_OF_RANGE` against
real fixtures. `PASSWORD_REQUIRED`, `RENDER_FAILED`, `ENGINE_FAILED`, and `INTERNAL` aren't
asserted: reaching them needs an encrypted fixture, a way to make rendering fail, or a way to kill
the worker thread on demand, and this scaffold has none of those yet.
