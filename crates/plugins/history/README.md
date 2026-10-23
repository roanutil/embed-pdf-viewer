# epdf-plugin-history

`packages/plugin/history` in Rust, at upstream `b456ee4fd`: undo and redo, one history per open
document, built on the document's change queue (`ctx.changes`). Every change a plugin stages there
is a step, unless it is an undo or was staged with `history: false`. The vectors are
`vectors/plugins/history.json`, 26 scenario cases written by
`tooling/plugin-conformance/src/suites/history.ts` (`cargo test -p epdf-plugin-history`).

The modules follow the TypeScript files. `model.rs` is `model.ts`: `empty_history`, `next_of`,
`recorded`, `stepped`, `forgotten`, `step_refused`, `unavailable` and `cleared`, each returning the
history's `Rc` unchanged where the TypeScript returns `history` itself, so an unchanged history
writes no state. `controller.rs` is `controller.ts`, `contract.rs` is `contract.ts` with the token
`HISTORY`, and `state.rs` is `state.ts`. The plugin is document-scoped, built through `create`
with no reducer, and declares `limit` (default 100) as its settings.

An undo stages the undo of each part of the next entry by `opId`, newest part first, after sending
any open hold. The entry moves to the redo stack at once, its parts renamed to the undos that
staged them. Nothing is restored from a copy: `on_undone` and `on_undo_failed` fire once every part
was answered, and a refused part goes back to its place by sequence number. An `UndoUnavailable`
refusal drops the entry and everything before it. The document events `redaction.applied`,
`pages.flattened`, `signatures.completed`, `forms.repaired` and `document.versioned` clear both
stacks.

## Where the port differs in shape

`undo()` and `redo()` return `Result<(), Error>`. The TypeScript throws from the same place: when
the document closed before the first part could be staged.

`get_settings()` reads `limit` with JavaScript's `ToNumber` into an `f64`, so a string or `null`
limit trims the stack as `Math.max(1, limit)` would. `settings_json()` and `update_settings_json()`
expose the kernel's JSON store as the TypeScript `getSettings`/`updateSettings` see it; the
conformance adapter uses those.

The suite needs a plugin that stages changes, so `history.ts` registers `history-actor`, and
`harness.rs` builds its twin. It is test scaffolding, not part of the package.

## Open

`close-with-a-step-on-its-way` fails until the kernel stops closing the change queue ahead of the
session scope. `close_session` in `crates/epdf-kernel/src/kernel.rs:638` calls
`s.changes.close()` right after revoking the leases; the TypeScript `closeSession`
(`packages/core/main/src/kernel.ts:550-570`) does not, and closes the queue only in its scope
teardown (`kernel.ts:509`), after the plugins' event sources are gone. Rust therefore rejects the
pending undo early enough for `on_undo_failed` to reach a listener the TypeScript has already
disposed. Deleting that one line makes all 26 cases pass, with the kernel and harness suites still
green.
