# epdf-plugin-view-manager

`packages/plugin/view-manager` in Rust, at the current contract. All 82 cases in
`vectors/plugins/view-manager/reducer.json` and all 13 in `vectors/plugins/view-manager/plugin.json`
replay identically (`cargo test -p epdf-plugin-view-manager`). `tests/behavior.rs` covers what the
vectors don't print: where `removePane` sends its documents, the events of each intermediate
update, and read identity.

The modules follow the TypeScript files. `model.rs` holds the named pure transitions of `model.ts`
(`createPane`, `movePane`, `renameDocument`, `reconcile` and the rest), and the reducer suite calls
them by name. There is no reducer and no action type. The controller applies each transition with
`Controller::update_state`, which is `ctx.state.update`, and derives `onPaneCreated`,
`onPaneRemoved`, `onFocusChanged` and `onDocumentMoved` from every state change in
`on_state_change`, so `createPane` with documents announces each step on its own. Its `connect`
watches `documents.getOrder()` with element-wise equality (`sameIds`), renames a document that got
its real id in place, then reconciles. A failing listener goes to the kernel's report.

The state keeps the TypeScript's reference identity: `panes`, each pane and `order` are `Rc`s, and
a transition that returns the state unchanged hands back the same `Rc`. `get_pane` and `list_panes`
are the `memoByKey` and `memo` reads, holding the input `Rc`s and comparing with `Rc::ptr_eq`, so a
pane's `PaneInfo` stays the same `Rc` until its record changes. `get_pane_order` returns the state's
own `order`.

The pane record is a `JsRecord` in JavaScript property order, because `Object.values(panes)` orders
the `onDocumentMoved` events.

Index arguments are JavaScript numbers. `insertAt` and `movePane` clamp with `Math.min`/`Math.max`
(NaN propagates) and then `slice`, which truncates a fraction and reads NaN as 0, and `insertAt`
appends for an index that is `undefined` or `null`. The Rust helpers in `model.rs` do the same,
with `None` as the absent index.

## Coverage

The coverage measured before this port (530 of 530 statements and 155 of 155 branches) describes
the reducer-based TypeScript package. It was not re-measured for Task 13.30.

## Not ported

Pane ids that name `Object.prototype` members are not ported; the Rust transitions treat them as
absent, the choice Ruling13.7 made for kind names. `panes['toString']` finds a function in the
TypeScript, which changes three paths:

- `setFocusedPane('toString')` followed by an open makes the reconcile spread `target.documentIds`
  of a function and throw a `TypeError` inside the `connect` watch, which the store reports.
- `addDocument('toString', id)` throws a `TypeError` to the caller (`pane.documentIds.includes`,
  `model.ts:127`).
- `setActiveDocument('toString', null)` spreads the function into `{ activeDocumentId: null }` and
  writes it under the key `undefined` (`model.ts:116`).

Rust ignores all three. An `order` entry with no pane, which only a hand-built state can contain, is
skipped where the TypeScript's `state.panes[id].documentIds` throws.

The mutation results recorded before this port (137 of 140 viable mutants caught) describe the
reducer-based code and were not re-run for Task 13.30.
