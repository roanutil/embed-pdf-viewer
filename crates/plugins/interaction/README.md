# epdf-plugin-interaction

`packages/plugin/interaction` in Rust: the pointer hub (tools, prioritized handlers, gesture
capture, cursor claims, touch pre-flight) and the feedback plugin. All 23 cases in
`vectors/plugins/interaction.json` and all 15 in `vectors/plugins/interaction-functions.json`
replay identically (`cargo test -p epdf-plugin-interaction`).

The generators reach 582 of the package's 583 statements and 103 of its 104 branches. The one line
they can't reach is `reducer.ts:20`, the `default` arm for an action outside the
`SET_TOOL | SET_CURSOR` union. In Rust that union is an enum with two variants, so the arm has
nothing to match.

## The handler API

`InteractionHandler` is a trait, and it's what the Rust selection, annotation, form and stage
plugins implement. The TypeScript interface has six optional members; the trait gives each one a
default that is exactly what the hub does when the member is absent:

- `on_move`, `on_up` and `on_hover` do nothing;
- `on_cancel` calls `on_up`, the `onCancel ?? onUp` fallback in `dispatch`;
- `claims_touch` answers false.

So a handler implements only the members it uses. `tests/api.rs` pins those defaults, since no
vector can: a scripted handler in a vector always overrides them.

Registration returns `Unregister`, which can be called any number of times, as the TypeScript
closure can. That matters for `registerTool`, whose unregister deletes by id again on every call,
including a later replacement under the same id. `onToolChange` returns `Unsubscriber`, whose
`call()` answers `Set.delete`'s boolean. `CallbackSet` is the JavaScript `Set` of callbacks behind
it, with `forEach`'s iteration rules; the selection crate uses it too.

## Where the port differs in shape

Handler methods are infallible. A TypeScript handler that throws aborts `dispatch` mid-pass and the
throw reaches the adapter. No shipped handler throws, and a `Result` on every member would be noise
for every downstream implementation, so the vectors don't script a throwing handler.

`Phase` is a four-variant enum. The TypeScript routes any phase string that isn't `down`, `move` or
`cancel` down the `up` branch; a Rust caller can't construct one.

The hub releases its handler list, its source scopes, the gesture owner and the tool-change
callbacks when the document closes. The TypeScript doesn't need to: a garbage collector reclaims the
cycle between the hub and the handlers that hold it. Rust would leak it. The only observable
difference is through a capability object kept past its document's close, which the kernel no
longer hands out.

`harness.rs` exports the JSON bridges (`sample_from_json`, `sample_json`, `tool_json`,
`ScriptedHandler`) and the `Interaction` adapter, which downstream crates register next to their own
when their vectors drive the hub.

## Mutation testing

`cargo mutants -p epdf-plugin-interaction --test-package epdf-plugin-interaction` catches 82 of 89
viable mutants, and one more times out (an infinite loop in `CallbackSet::for_each`). The 7 that
survive are equivalent:

- 4 in `CallbackSet` (`callbacks.rs:77`, `:78`, `:88`) change when a tombstoned entry is compacted
  out of the list. A tombstone is never visited, so the only difference is memory.
- 3 on the `Greater` arm of the priority comparator in `eligible` (`capability.rs:168`). The stable
  sort only asks whether one handler sorts before another, so `Greater` and `Equal` both leave a
  pair in registration order.
