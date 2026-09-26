# epdf-plugin-view-manager

`packages/plugin/view-manager` in Rust, at the v3 contract. All 63 cases in
`vectors/plugins/view-manager/reducer.json` and all 12 in `vectors/plugins/view-manager/plugin.json`
replay identically (`cargo test -p epdf-plugin-view-manager`).

The modules follow the TypeScript files: `contract.rs`, `model.rs` and `controller.rs`. Views are
panes now (`listPanes`, `createPane`, `splitPane`, `removePane` with `moveDocumentsTo`, pane ids
`pane-<n>`), and `ADD_DOC` moves a document out of the pane that held it, since panes partition the
open documents. `create_view_manager_controller` returns the api and a `connect` that reconciles
against `documents.getOrder().join('|')` on every store change, which includes a document that is
still loading and one whose open failed (`first-open-makes-a-pane`, `a-failed-open`). Every write
goes through one diffing `dispatch`, which fires `onPaneCreated`, `onPaneRemoved`,
`onFocusChanged` and `onDocumentMoved` (`events`). A failing listener goes to stderr, where the
TypeScript writes to `console.error`; the vector shows its siblings still fire.

The pane record is a `JsRecord` in JavaScript property order, because `Object.values(panes)` orders
the `onDocumentMoved` events. `listPanes` and `getPane` build their views on every call; the
TypeScript memoizes them per record, which only reference identity could observe.

Index arguments are JavaScript numbers. `insertAt` and `movePane` clamp with `Math.min`/`Math.max`
(NaN propagates) and then `slice`, which truncates a fraction and reads NaN as 0; the Rust helpers
in `model.rs` do the same, and the action types serialize a non-finite index as
`{"$number": "NaN"}`, as the trace prints it.

## Coverage

The view-manager suites reach every statement and branch of the TypeScript package: 530 of 530
statements and 155 of 155 branches.

## TypeScript defect: the shipping plugin cannot be installed

`viewManagerPlugin()` cannot be built in a real kernel. Its controller builds
`{ ...rawCtx, dispatch }`, and spreading the controller context reads its `doc` getter, which
throws for a workspace plugin: `createKernel` fails with `[kernel] workspace plugin
"view-manager" has no bound document; use ctx.forDocument(token, documentId).` whatever else is
installed. `a-kernel-refuses-the-shipping-plugin` pins that through the `view-manager-as-shipped`
factory. Every other case installs the plugin through a generator factory that hides `doc` from the
spread, so the vectors describe the panes as the controller means them.

The Rust controller has no spread, so `view_manager_plugin()` installs fine. The harness's
`view-manager-as-shipped` factory reproduces the TypeScript's read by calling `cx.doc()` before it
builds the controller, which refuses with the same error. The fix on the TypeScript side is to stop
spreading the context (wrap `dispatch` without copying the getters).

## Not ported

Pane ids that name `Object.prototype` members are not ported. `panes['toString']` is truthy in the
TypeScript, so `setFocusedPane('toString')` followed by an open makes the reconcile spread
`target.documentIds` of a function and throw a `TypeError` inside the reducer, which the store
reports. A Rust reducer has no error path, and it treats those ids as absent. The same holds for an
`order` entry with no pane, which only a hand-built state can contain. `splitPane`'s `from` option
is accepted and ignored by the TypeScript, so the Rust `split_pane` does not take it.

Mutation numbers are pre-v3, rerun pending. `cargo mutants -p epdf-plugin-view-manager
--test-package epdf-plugin-view-manager` caught 97 of 99 viable mutants in the v2 crate; both
survivors were equivalent.
