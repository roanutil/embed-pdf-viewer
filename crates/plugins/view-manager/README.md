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
(NaN propagates) and then `slice`, which truncates a fraction and reads NaN as 0; the Rust helpers
in `model.rs` do the same, and the action types serialize a non-finite index as
`{"$number": "NaN"}`, as the trace prints it.

## Coverage

The view-manager suites reach every statement and branch of the TypeScript package: 530 of 530
statements and 155 of 155 branches.

## Not ported

Pane ids that name `Object.prototype` members are not ported. `panes['toString']` is truthy in the
TypeScript, so `setFocusedPane('toString')` followed by an open makes the reconcile spread
`target.documentIds` of a function and throw a `TypeError` inside the `connect` watch, which the
store reports. The Rust transitions have no error path, and they treat those ids as absent. The same holds for an
`order` entry with no pane, which only a hand-built state can contain.

The mutation results recorded before this port (137 of 140 viable mutants caught) describe the
reducer-based code and were not re-run for Task 13.30.
