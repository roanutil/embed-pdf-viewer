# epdf-plugin-view-manager

`packages/plugin/view-manager` in Rust. All 61 cases in `vectors/plugins/view-manager/reducer.json`
and all 8 in `vectors/plugins/view-manager/plugin.json` replay identically
(`cargo test -p epdf-plugin-view-manager`).

The effect watches `documents.order().join('|')` and reconciles on every change, which includes a
document that is still loading and one whose open failed (`first-open-makes-a-pane`,
`a-failed-open`). The Rust watch compares the same joined string, so two orders that join alike
don't reconcile on either side.

Index arguments are JavaScript numbers. `insertAt` and `moveView` clamp with `Math.min`/`Math.max`
(NaN propagates) and then `slice`, which truncates a fraction and reads NaN as 0; the Rust helpers
in `reducer.rs` do the same, and the action types serialize a non-finite index as
`{"$number": "NaN"}`, as the trace prints it.

The generators reach every line and branch of the TypeScript package (statements 405/405, branches
102/102).

View ids that name `Object.prototype` members are not ported. `views['toString']` is truthy in the
TypeScript, so `setFocused('toString')` followed by an open makes the reconcile spread
`target.documentIds` of a function and throw a `TypeError` inside the reducer, which the store
reports. A Rust reducer has no error path, and it treats those ids as absent. The same holds for an
`order` entry with no view, which only a hand-built state can contain.

`cargo mutants -p epdf-plugin-view-manager --test-package epdf-plugin-view-manager` catches 97 of
99 viable mutants (one of them a timeout: inverting the watch comparison reconciles forever). Both
survivors are equivalent. `initial_view_manager_state()` replaced with `Default::default()` builds
the same value. And `||` becoming `&&` in `js_max` (`reducer.rs:20`) only changes `Math.max(0, NaN)`
from NaN to 0, which `slice_end` reads as 0 either way; every call passes the literal `0.0` first.
